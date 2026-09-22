use lettre::message::Mailbox;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{AsyncTransport, Message, Tokio1Executor};

#[derive(Debug, thiserror::Error)]
pub enum MailerError {
    #[error("invalid email address: {0}")]
    InvalidAddress(#[from] lettre::address::AddressError),

    #[error("failed to build message: {0}")]
    Build(#[from] lettre::error::Error),

    #[error("smtp transport error: {0}")]
    Transport(#[from] lettre::transport::smtp::Error),
}

pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub text_body: String,
}

#[async_trait::async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, message: &EmailMessage) -> Result<(), MailerError>;
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn new(smtp_url: &str, from: &str) -> Result<Self, MailerError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(smtp_url)?.build();
        Ok(Self {
            transport,
            from: from.parse()?,
        })
    }
}

#[async_trait::async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, message: &EmailMessage) -> Result<(), MailerError> {
        let email = Message::builder()
            .from(self.from.clone())
            .to(message.to.parse()?)
            .subject(&message.subject)
            .body(message.text_body.clone())?;

        self.transport.send(email).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::Duration;

    #[tokio::test]
    async fn sent_email_appears_in_mailpit() {
        let smtp_url = std::env::var("SMTP_URL").expect("SMTP_URL set");
        let mailer = SmtpMailer::new(&smtp_url, "no-reply@sintade.test").expect("mailer builds");

        let subject = format!("day-8-test-{}", uuid::Uuid::now_v7());
        mailer
            .send(&EmailMessage {
                to: "someone@example.test".to_string(),
                subject: subject.clone(),
                text_body: "hello from the day 8 integration test".to_string(),
            })
            .await
            .expect("send succeeds");

        // Mailpit indexes asynchronously; poll its REST API briefly.
        let http = reqwest::Client::new();
        let mut found = false;
        for _ in 0..20 {
            let response: Value = http
                .get(format!(
                    "http://localhost:8025/api/v1/search?query=subject:{subject}"
                ))
                .send()
                .await
                .expect("mailpit api reachable")
                .json()
                .await
                .expect("valid json");
            if response["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
            {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(found, "sent email did not appear in Mailpit: {subject}");
    }
}
