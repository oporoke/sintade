use std::sync::Arc;

use kernel::{DomainEvent, EventEnvelope};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::clock::Clock;

pub const NOTIFY_CHANNEL: &str = "outbox_events";

#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("failed to serialize event: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub struct Outbox {
    clock: Arc<dyn Clock>,
}

impl Outbox {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }

    /// Writes a domain event to `outbox_events` in the caller's transaction, and issues a
    /// `pg_notify` on the same transaction so it's only delivered to listeners if the
    /// transaction actually commits.
    pub async fn push<E: DomainEvent>(
        &self,
        tx: &mut PgConnection,
        event: &E,
    ) -> Result<(), OutboxError> {
        let aggregate_id: Uuid = event.aggregate_id();
        let envelope = EventEnvelope {
            id: Uuid::now_v7(),
            event_type: E::EVENT_TYPE,
            aggregate_id,
            workspace_id: event.workspace_id(),
            occurred_at: self.clock.now(),
            version: 1,
            data: event,
        };
        let payload = serde_json::to_value(&envelope)?;

        sqlx::query!(
            "INSERT INTO outbox_events (event_type, aggregate_id, payload) VALUES ($1, $2, $3)",
            E::EVENT_TYPE,
            aggregate_id,
            payload,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!("SELECT pg_notify($1, $2)", NOTIFY_CHANNEL, E::EVENT_TYPE)
            .execute(&mut *tx)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use kernel::WorkspaceId;
    use serde::Serialize;
    use sqlx::PgPool;
    use std::time::Duration;

    #[derive(Serialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        const EVENT_TYPE: &'static str = "test.event";

        fn aggregate_id(&self) -> Uuid {
            Uuid::nil()
        }

        fn workspace_id(&self) -> WorkspaceId {
            WorkspaceId::new_v7()
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn push_writes_row_and_notifies_listeners(pool: PgPool) {
        let mut listener = crate::notify::listen(&pool, NOTIFY_CHANNEL)
            .await
            .expect("listen succeeds");

        let outbox = Outbox::new(Arc::new(SystemClock));
        let mut tx = pool.begin().await.expect("begin tx");
        outbox
            .push(
                &mut tx,
                &TestEvent {
                    message: "hi".to_string(),
                },
            )
            .await
            .expect("push succeeds");
        tx.commit().await.expect("commit succeeds");

        let notification = tokio::time::timeout(Duration::from_secs(2), listener.recv())
            .await
            .expect("notification arrives before timeout")
            .expect("notification is Ok");
        assert_eq!(notification.payload(), TestEvent::EVENT_TYPE);

        let row = sqlx::query!(
            "SELECT event_type, payload FROM outbox_events WHERE event_type = $1",
            TestEvent::EVENT_TYPE
        )
        .fetch_one(&pool)
        .await
        .expect("row exists");
        assert_eq!(row.payload["data"]["message"], "hi");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn push_without_commit_does_not_notify(pool: PgPool) {
        let mut listener = crate::notify::listen(&pool, NOTIFY_CHANNEL)
            .await
            .expect("listen succeeds");

        let outbox = Outbox::new(Arc::new(SystemClock));
        let mut tx = pool.begin().await.expect("begin tx");
        outbox
            .push(
                &mut tx,
                &TestEvent {
                    message: "rolled back".to_string(),
                },
            )
            .await
            .expect("push succeeds");
        tx.rollback().await.expect("rollback succeeds");

        let result = tokio::time::timeout(Duration::from_millis(300), listener.recv()).await;
        assert!(
            result.is_err(),
            "a rolled-back transaction must not notify listeners"
        );
    }
}
