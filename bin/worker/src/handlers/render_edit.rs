use std::sync::Arc;

use media::{MediaService, RenderEdit, RenderOutcome};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `RenderEdit` (docs/design.md §10): cuts an edit's kept ranges out of the recording's MP4 and
/// stores the result under `edits/{version}/`. Enqueued when an edit is saved; an edit whose MP4
/// isn't stored fails and is retried with backoff, and is marked failed on the last attempt.
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
            Ok(RenderOutcome::Rendered { duration_ms, bytes }) => {
                tracing::info!(job_id = %ctx.job_id, duration_ms, bytes, "RenderEdit done");
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
