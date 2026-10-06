use std::sync::Arc;

use media::{BuildHls, HlsOutcome, MediaService};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `BuildHls` (docs/design.md §10 Process): cuts a processed recording's MP4 into the HLS
/// ladder. A take whose MP4 isn't stored yet fails and is retried with backoff.
#[derive(Clone)]
pub struct BuildHlsHandler {
    media: Arc<MediaService>,
}

impl BuildHlsHandler {
    pub fn new(media: Arc<MediaService>) -> Self {
        Self { media }
    }
}

impl JobHandler for BuildHlsHandler {
    const KIND: &'static str = media::BUILD_HLS;
    type Payload = BuildHls;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        match self.media.build_hls(payload).await {
            Ok(HlsOutcome::Built { rungs }) => {
                tracing::info!(job_id = %ctx.job_id, rungs, "BuildHls done");
                Ok(())
            }
            Ok(HlsOutcome::NotFound) => {
                tracing::info!(job_id = %ctx.job_id, "BuildHls had nothing to do");
                Ok(())
            }
            Err(error) => Err(JobError::Failed(error.to_string())),
        }
    }
}
