use std::sync::Arc;

use media::{MediaService, ProcessOutcome, ProcessTake};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `ProcessTake` (docs/design.md §10 Process): turns a finalized take into renditions.
/// Enqueued by media's `TakeFinalized` subscriber.
#[derive(Clone)]
pub struct ProcessTakeHandler {
    media: Arc<MediaService>,
}

impl ProcessTakeHandler {
    pub fn new(media: Arc<MediaService>) -> Self {
        Self { media }
    }
}

impl JobHandler for ProcessTakeHandler {
    const KIND: &'static str = media::PROCESS_TAKE;
    type Payload = ProcessTake;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        let outcome = self
            .media
            .process_take(payload, ctx.is_last_attempt())
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        if outcome != ProcessOutcome::Ran {
            tracing::info!(job_id = %ctx.job_id, ?outcome, "ProcessTake had nothing to do");
        }
        Ok(())
    }
}
