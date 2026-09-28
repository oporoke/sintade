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
}
