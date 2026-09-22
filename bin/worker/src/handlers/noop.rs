use serde::Deserialize;

use crate::handler::{JobCtx, JobError, JobHandler};

#[derive(Clone, Copy)]
pub struct NoopHandler;

#[derive(Deserialize)]
pub struct NoopPayload {}

impl JobHandler for NoopHandler {
    const KIND: &'static str = "noop";
    type Payload = NoopPayload;

    async fn run(&self, ctx: &JobCtx, _payload: Self::Payload) -> Result<(), JobError> {
        tracing::info!(job_id = %ctx.job_id, attempt = ctx.attempt, "noop job ran");
        Ok(())
    }
}
