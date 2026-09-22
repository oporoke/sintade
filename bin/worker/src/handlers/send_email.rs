use std::sync::Arc;

use platform::{EmailMessage, Mailer};
use serde::Deserialize;

use crate::handler::{JobCtx, JobError, JobHandler};

#[derive(Clone)]
pub struct SendEmailHandler {
    mailer: Arc<dyn Mailer>,
}

impl SendEmailHandler {
    pub fn new(mailer: Arc<dyn Mailer>) -> Self {
        Self { mailer }
    }
}

#[derive(Deserialize)]
pub struct SendEmailPayload {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl JobHandler for SendEmailHandler {
    const KIND: &'static str = "SendEmail";
    type Payload = SendEmailPayload;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        self.mailer
            .send(&EmailMessage {
                to: payload.to,
                subject: payload.subject,
                text_body: payload.body,
            })
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        tracing::info!(job_id = %ctx.job_id, "send_email job ran");
        Ok(())
    }
}
