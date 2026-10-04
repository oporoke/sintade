use std::sync::Arc;

use catalog::RecordingManager;
use serde::Deserialize;

use crate::handler::{JobCtx, JobError, JobHandler};

/// `PurgeRecording` (docs/design.md §4 Media): deletes recordings that have been in the trash
/// for 30 days, storage prefix first. Scheduled hourly by the worker.
#[derive(Clone)]
pub struct PurgeRecordingHandler {
    recordings: Arc<RecordingManager>,
}

impl PurgeRecordingHandler {
    pub fn new(recordings: Arc<RecordingManager>) -> Self {
        Self { recordings }
    }
}

/// The job carries nothing: each run purges whatever is due at that moment.
#[derive(Deserialize)]
pub struct PurgeRecordingPayload {}

impl JobHandler for PurgeRecordingHandler {
    const KIND: &'static str = "PurgeRecording";
    type Payload = PurgeRecordingPayload;

    async fn run(&self, ctx: &JobCtx, _payload: Self::Payload) -> Result<(), JobError> {
        let report = self
            .recordings
            .purge_due()
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        tracing::info!(
            job_id = %ctx.job_id,
            recordings = report.recordings,
            objects = report.objects,
            "purge_recording job ran"
        );
        Ok(())
    }
}
