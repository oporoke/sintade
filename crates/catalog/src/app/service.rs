use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::PgConnection;

use crate::domain::{Cursor, LIBRARY_PAGE_SIZE, Title};
use crate::infra;
pub use crate::infra::{LibraryItem, RecordingOwner, Reopened, WatchInfo};

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

    /// How many recordings count towards the workspace's plan. Takes a per-workspace lock held
    /// until the caller's transaction ends, so the caller can check a quota and insert without
    /// a race.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id))]
    pub async fn lock_and_count_active(
        &self,
        conn: &mut PgConnection,
        workspace_id: WorkspaceId,
    ) -> Result<u64, sqlx::Error> {
        infra::lock_workspace_recordings(conn, workspace_id).await?;
        let count = infra::count_active_recordings(conn, workspace_id).await?;
        Ok(u64::try_from(count).unwrap_or_default())
    }

    /// `Recording → Abandoned` for recordings whose upload went idle (`SweepStaleUploads`).
    /// Only recordings still in `recording`/`uploading` change; returns those.
    #[tracing::instrument(skip_all, fields(candidates = recordings.len()))]
    pub async fn mark_abandoned(
        &self,
        conn: &mut PgConnection,
        recordings: &[(RecordingId, WorkspaceId)],
    ) -> Result<Vec<RecordingId>, sqlx::Error> {
        infra::mark_abandoned(conn, recordings).await
    }

    /// `Processing → Ready` once media has the MP4 (docs/design.md §4), with what the MP4
    /// measured. Runs on media's transaction, so the state, the renditions and `RecordingReady`
    /// commit together (ADR-0012). `None`: the recording isn't `processing` any more.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn mark_ready(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
        measured: Measured,
    ) -> Result<Option<RecordingOwner>, sqlx::Error> {
        infra::mark_ready(
            conn,
            id,
            workspace_id,
            measured.duration_ms,
            measured.width,
            measured.height,
            measured.size_bytes,
        )
        .await
    }

    /// `Processing → Failed` (bad input, or processing gave up). Runs on media's transaction.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn mark_failed(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<RecordingOwner>, sqlx::Error> {
        infra::mark_failed(conn, id, workspace_id).await
    }

    /// The recording as a watch page shows it; `None` if it doesn't exist in the workspace or
    /// is in the trash.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn watch_info(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<WatchInfo>, sqlx::Error> {
        infra::watch_info(conn, id, workspace_id).await
    }

    /// One library page: the workspace's recordings, newest first, plus the cursor of the next
    /// page when there is one.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id))]
    pub async fn library_page(
        &self,
        conn: &mut PgConnection,
        workspace_id: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<LibraryPage, sqlx::Error> {
        let mut items =
            infra::list_library(conn, workspace_id, cursor, LIBRARY_PAGE_SIZE + 1).await?;
        let next = if items.len() as i64 > LIBRARY_PAGE_SIZE {
            items.truncate(LIBRARY_PAGE_SIZE as usize);
            items.last().map(|last| Cursor {
                created_at_micros: i64::try_from(last.created_at.unix_timestamp_nanos() / 1_000)
                    .unwrap_or_default(),
                id: last.id.into_uuid(),
            })
        } else {
            None
        };
        Ok(LibraryPage { items, next })
    }

    /// Whether the recording exists in the workspace (trashed ones included: the caller decides
    /// what trashed means).
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn exists_in_workspace(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
    ) -> Result<bool, sqlx::Error> {
        infra::exists_in_workspace(conn, id, workspace_id).await
    }

    /// `Failed → Processing` for a manual retry. Runs on media's transaction, so the state and
    /// the re-queued job commit together.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn reopen_failed(
        &self,
        conn: &mut PgConnection,
        id: RecordingId,
        workspace_id: WorkspaceId,
    ) -> Result<Reopened, sqlx::Error> {
        infra::reopen_failed(conn, id, workspace_id).await
    }
}

/// What a finished MP4 measured: the recording's duration, picture size and file size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    pub duration_ms: i32,
    pub width: i32,
    pub height: i32,
    pub size_bytes: i64,
}

/// A page of the library and where the next one starts.
#[derive(Debug)]
pub struct LibraryPage {
    pub items: Vec<LibraryItem>,
    pub next: Option<Cursor>,
}
