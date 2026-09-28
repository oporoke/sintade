use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::PgConnection;

use crate::domain::Title;
use crate::infra;

/// A recording to create, in state `recording`, with its first take already chosen.
#[derive(Debug, Clone)]
pub struct NewRecording {
    pub id: RecordingId,
    pub workspace_id: WorkspaceId,
    pub owner_id: UserId,
    pub title: Title,
    pub current_take: TakeId,
}

#[derive(Debug, Default)]
pub struct CatalogService;

impl CatalogService {
    pub fn new() -> Self {
        Self
    }

    /// Inserts the recording row. Runs on the caller's connection rather than opening its own
    /// transaction: `POST /recordings` must create the recording and its take atomically, and
    /// `ingest` owns that transaction (ADR-0009).
    #[tracing::instrument(skip_all, fields(recording_id = %recording.id, workspace_id = %recording.workspace_id))]
    pub async fn create_recording(
        &self,
        conn: &mut PgConnection,
        recording: &NewRecording,
    ) -> Result<(), sqlx::Error> {
        infra::insert_recording(
            conn,
            recording.id,
            recording.workspace_id,
            recording.owner_id,
            recording.title.as_str(),
            recording.current_take,
        )
        .await
    }

    /// `Uploading → Processing` when ingest finalizes the current take (the Media lifecycle,
    /// docs/design.md §4). Runs on ingest's transaction, like `create_recording`. `false` means
    /// the recording no longer accepts uploads.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn mark_processing(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
        duration_ms: i32,
    ) -> Result<bool, sqlx::Error> {
        infra::mark_processing(conn, id, workspace_id, duration_ms).await
    }
}
