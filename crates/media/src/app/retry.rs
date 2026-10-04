use std::sync::Arc;

use catalog::{CatalogService, Reopened};
use kernel::{RecordingId, WorkspaceId};
use platform::{JobQueue, JobQueueError};
use sqlx::PgPool;

use crate::infra;

use super::service::{PROCESS_TAKE, ProcessTake};

/// Why a manual retry didn't start.
#[derive(Debug, thiserror::Error)]
pub enum RetryError {
    /// No such recording in the workspace (also what another workspace's recording looks like).
    #[error("recording not found")]
    NotFound,
    #[error("only a failed recording can be retried")]
    NotFailed,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Queue(#[from] JobQueueError),
    #[error("could not encode the job: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Manual retry of failed recordings. Separate from `MediaService` because it only queues work:
/// the API can offer it without ffmpeg, scratch space or storage (media never runs in the API).
pub struct RetryService {
    pool: PgPool,
    catalog: Arc<CatalogService>,
}

impl RetryService {
    pub fn new(pool: PgPool, catalog: Arc<CatalogService>) -> Self {
        Self { pool, catalog }
    }

    /// Manual retry of a `failed` recording (§4 `Failed → Processing`): the recording goes back
    /// to `processing`, its take's job is reset and a fresh `ProcessTake` is queued, in one
    /// transaction. A recording that isn't failed is refused (`NotFailed`), so a double click
    /// queues one job.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn retry_processing(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<(), RetryError> {
        let mut tx = self.pool.begin().await?;
        match self
            .catalog
            .reopen_failed(&mut tx, recording_id, workspace_id)
            .await?
        {
            Reopened::NotFound => return Err(RetryError::NotFound),
            Reopened::NotFailed => return Err(RetryError::NotFailed),
            Reopened::Reopened => {}
        }
        // A failed recording always has a take that was processed; without one there is
        // nothing to re-run, and the rollback leaves it `failed`.
        let Some(take_id) = infra::requeue_latest_job(&mut tx, recording_id, workspace_id).await?
        else {
            return Err(RetryError::NotFailed);
        };
        let payload = serde_json::to_value(ProcessTake {
            take_id,
            workspace_id,
        })?;
        JobQueue::enqueue_in(&mut tx, PROCESS_TAKE, payload).await?;
        tx.commit().await?;
        tracing::info!(%take_id, "retry queued");
        Ok(())
    }
}
