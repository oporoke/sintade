use std::sync::Arc;

use media::{MediaService, RenderEdit, RenderOutcome};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `RenderEdit` (docs/design.md §10): makes an edit into a version of the recording under
/// `edits/{version}/` and swaps it in for what viewers get (ADR-0038). Enqueued when an edit is
/// saved; a failed attempt is retried with backoff, and the last one gives the edit up, leaving
/// the recording as it was.
#[derive(Clone)]
pub struct RenderEditHandler {
    media: Arc<MediaService>,
}

impl RenderEditHandler {
    pub fn new(media: Arc<MediaService>) -> Self {
        Self { media }
    }
}

impl JobHandler for RenderEditHandler {
    const KIND: &'static str = media::RENDER_EDIT;
    type Payload = RenderEdit;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        match self.media.render_edit(payload, ctx.is_last_attempt()).await {
            Ok(RenderOutcome::Applied {
                duration_ms,
                bytes,
                rungs,
            }) => {
                tracing::info!(job_id = %ctx.job_id, duration_ms, bytes, rungs, "RenderEdit applied");
                Ok(())
            }
            Ok(outcome) => {
                tracing::info!(job_id = %ctx.job_id, ?outcome, "RenderEdit had nothing to do");
                Ok(())
            }
            Err(error) => Err(JobError::Failed(error.to_string())),
        }
    }
}
