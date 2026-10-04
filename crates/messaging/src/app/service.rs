use std::sync::Arc;

use kernel::{RecordingId, UserId, WorkspaceId};
use platform::JobQueue;
use serde::Deserialize;

/// Who a notice goes to. Identity owns this data; the binary adapts it (port, §5).
#[derive(Debug, Clone)]
pub struct Recipient {
    pub email: String,
    pub display_name: String,
}

#[async_trait::async_trait]
pub trait RecipientDirectory: Send + Sync {
    /// `None` if the user no longer exists.
    async fn recipient(&self, user_id: UserId) -> Result<Option<Recipient>, String>;
}

/// `RecordingReady` as messaging reads it from the outbox envelope (media's contract).
#[derive(Debug, Clone, Deserialize)]
pub struct RecordingReadyMessage {
    pub workspace_id: WorkspaceId,
    pub data: RecordingReadyData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordingReadyData {
    pub recording_id: RecordingId,
    pub owner_id: UserId,
    pub title: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MessagingError {
    #[error("recipient lookup failed: {0}")]
    Recipient(String),
    #[error("could not enqueue the email: {0}")]
    Enqueue(#[from] platform::JobQueueError),
}

pub struct MessagingService {
    recipients: Arc<dyn RecipientDirectory>,
    queue: JobQueue,
    public_base_url: String,
}

impl MessagingService {
    pub fn new(
        recipients: Arc<dyn RecipientDirectory>,
        queue: JobQueue,
        public_base_url: String,
    ) -> Self {
        Self {
            recipients,
            queue,
            public_base_url,
        }
    }

    /// Subscribed to `RecordingReady`: emails the creator that the recording can be watched
    /// (US-21). A creator who no longer exists gets nothing. The email goes out as a
    /// `SendEmail` job, so SMTP trouble is retried by the queue, not by the outbox.
    #[tracing::instrument(skip_all, fields(recording_id = %message.data.recording_id, workspace_id = %message.workspace_id))]
    pub async fn recording_ready(
        &self,
        message: RecordingReadyMessage,
    ) -> Result<bool, MessagingError> {
        let data = message.data;
        let Some(recipient) = self
            .recipients
            .recipient(data.owner_id)
            .await
            .map_err(MessagingError::Recipient)?
        else {
            tracing::warn!("RecordingReady: the creator no longer exists, no email");
            return Ok(false);
        };
        // TODO: Verify — the watch page lands with sharing (M6); until then the link opens the
        // recording's page in the app.
        let link = format!("{}/recordings/{}", self.public_base_url, data.recording_id);
        let body = format!(
            "Hi {name},\n\nYour recording \"{title}\" is ready to watch and share:\n\n{link}\n",
            name = recipient.display_name,
            title = data.title,
        );
        self.queue
            .enqueue(
                "SendEmail",
                serde_json::json!({
                    "to": recipient.email,
                    "subject": format!("Your recording is ready: {}", data.title),
                    "body": body,
                }),
            )
            .await?;
        tracing::info!("recording-ready email enqueued");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::PgPool;

    use super::*;

    struct Fixed(Option<Recipient>);

    #[async_trait::async_trait]
    impl RecipientDirectory for Fixed {
        async fn recipient(&self, _user_id: UserId) -> Result<Option<Recipient>, String> {
            Ok(self.0.clone())
        }
    }

    fn message() -> RecordingReadyMessage {
        RecordingReadyMessage {
            workspace_id: WorkspaceId::new_v7(),
            data: RecordingReadyData {
                recording_id: RecordingId::new_v7(),
                owner_id: UserId::new_v7(),
                title: "Demo".to_string(),
            },
        }
    }

    fn service(pool: PgPool, recipient: Option<Recipient>) -> MessagingService {
        MessagingService::new(
            Arc::new(Fixed(recipient)),
            JobQueue::new(pool),
            "https://app.test".to_string(),
        )
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn ready_enqueues_an_email_to_the_creator_with_the_link(pool: PgPool) {
        let recipient = Recipient {
            email: "creator@example.com".to_string(),
            display_name: "Amina".to_string(),
        };
        let message = message();
        let recording_id = message.data.recording_id;
        assert!(
            service(pool.clone(), Some(recipient))
                .recording_ready(message)
                .await
                .expect("handled")
        );

        let job = JobQueue::new(pool)
            .claim_next("test", Duration::from_secs(30))
            .await
            .expect("claim")
            .expect("a SendEmail job");
        assert_eq!(job.kind, "SendEmail");
        assert_eq!(job.payload["to"], "creator@example.com");
        assert_eq!(job.payload["subject"], "Your recording is ready: Demo");
        let body = job.payload["body"].as_str().expect("body");
        assert!(body.contains("Hi Amina"));
        assert!(body.contains(&format!("https://app.test/recordings/{recording_id}")));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_missing_creator_gets_no_email(pool: PgPool) {
        assert!(
            !service(pool.clone(), None)
                .recording_ready(message())
                .await
                .expect("handled")
        );
        assert!(
            JobQueue::new(pool)
                .claim_next("test", Duration::from_secs(30))
                .await
                .expect("claim")
                .is_none()
        );
    }
}
