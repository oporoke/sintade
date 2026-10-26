use std::sync::Arc;

use media::{GenerateWaveform, MediaService, WaveformOutcome};

use crate::handler::{JobCtx, JobError, JobHandler};

/// `GenerateWaveform` (ADR-0039): the editor's waveform peaks, made from the original MP4.
/// Enqueued when `ProcessTake` finishes, or by the editor for a recording made before the job
/// existed; a take whose MP4 isn't stored yet fails and is retried with backoff.
#[derive(Clone)]
pub struct GenerateWaveformHandler {
    media: Arc<MediaService>,
}

impl GenerateWaveformHandler {
    pub fn new(media: Arc<MediaService>) -> Self {
        Self { media }
    }
}

impl JobHandler for GenerateWaveformHandler {
    const KIND: &'static str = media::GENERATE_WAVEFORM;
    type Payload = GenerateWaveform;

    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<(), JobError> {
        match self.media.generate_waveform(payload).await {
            Ok(WaveformOutcome::Built { peaks, has_audio }) => {
                tracing::info!(job_id = %ctx.job_id, peaks, has_audio, "GenerateWaveform done");
                Ok(())
            }
            Ok(WaveformOutcome::NotFound) => {
                tracing::info!(job_id = %ctx.job_id, "GenerateWaveform had nothing to do");
                Ok(())
            }
            Err(error) => Err(JobError::Failed(error.to_string())),
        }
    }
}
