use std::sync::Arc;

use ingest::IngestService;
use serde::Deserialize;

use crate::handler::{JobCtx, JobError, JobHandler};

/// `SweepStaleUploads` (docs/design.md §10 Recover step 4): abandons uploads idle for more than
/// 24 h and deletes their chunks after 7 days. Scheduled hourly by the worker.
#[derive(Clone)]
pub struct SweepStaleUploadsHandler {
    ingest: Arc<IngestService>,
}

impl SweepStaleUploadsHandler {
    pub fn new(ingest: Arc<IngestService>) -> Self {
        Self { ingest }
    }
}

/// The job carries nothing: each run sweeps whatever is stale at that moment.
#[derive(Deserialize)]
pub struct SweepStaleUploadsPayload {}

impl JobHandler for SweepStaleUploadsHandler {
    const KIND: &'static str = "SweepStaleUploads";
    type Payload = SweepStaleUploadsPayload;

    async fn run(&self, ctx: &JobCtx, _payload: Self::Payload) -> Result<(), JobError> {
        let report = self
            .ingest
            .sweep_stale_uploads()
            .await
            .map_err(|error| JobError::Failed(error.to_string()))?;
        tracing::info!(
            job_id = %ctx.job_id,
            abandoned = report.abandoned,
            purged_takes = report.purged_takes,
            "sweep_stale_uploads job ran"
        );
        Ok(())
    }
}
