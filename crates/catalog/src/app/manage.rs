use std::sync::Arc;

use kernel::{RecordingId, UserId, WorkspaceId};
use platform::{Clock, ObjectStore, Outbox, OutboxError, StorageError};
use sqlx::PgPool;

use crate::domain::{Chapter, ChapterError, ChapterList, Title};
use crate::events::{RecordingPurged, RecordingTrashed};
use crate::infra;

/// How long a recording stays in the trash before it is deleted for good.
pub const TRASH_RETENTION: time::Duration = time::Duration::days(30);

/// Recordings purged per run: a backlog drains over a few runs rather than one long one.
const PURGE_BATCH: i64 = 50;

#[derive(Debug, thiserror::Error)]
pub enum ManageError {
    /// No such recording in the caller's workspace.
    #[error("recording not found")]
    NotFound,
    #[error("invalid chapters: {0}")]
    InvalidChapters(#[from] ChapterError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("outbox error: {0}")]
    Outbox(#[from] OutboxError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

/// What one purge run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub recordings: usize,
    pub objects: u64,
}

/// Renaming, trashing and purging recordings. Owns its transactions (the other catalog entry
/// point, `CatalogService`, runs on its callers').
pub struct RecordingManager {
    pool: PgPool,
    store: Arc<dyn ObjectStore>,
    clock: Arc<dyn Clock>,
    outbox: Outbox,
}

impl RecordingManager {
    pub fn new(pool: PgPool, store: Arc<dyn ObjectStore>, clock: Arc<dyn Clock>) -> Self {
        Self {
            pool,
            store,
            outbox: Outbox::new(clock.clone()),
            clock,
        }
    }

    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn rename(
        &self,
        workspace_id: WorkspaceId,
        id: RecordingId,
        title: &Title,
    ) -> Result<(), ManageError> {
        let mut conn = self.pool.acquire().await?;
        if infra::rename(&mut conn, id, workspace_id, title.as_str()).await? {
            Ok(())
        } else {
            Err(ManageError::NotFound)
        }
    }

    /// The recording's chapters, earliest first.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn chapters(
        &self,
        workspace_id: WorkspaceId,
        id: RecordingId,
    ) -> Result<Vec<Chapter>, ManageError> {
        let mut conn = self.pool.acquire().await?;
        let rows = infra::chapters(&mut conn, id, workspace_id).await?;
        Ok(rows
            .into_iter()
            .map(|(start_ms, title)| Chapter {
                start_ms: u32::try_from(start_ms).unwrap_or(0),
                title,
            })
            .collect())
    }

    /// Replaces the recording's chapters with `chapters` (validated against its duration).
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn set_chapters(
        &self,
        workspace_id: WorkspaceId,
        id: RecordingId,
        chapters: Vec<Chapter>,
    ) -> Result<Vec<Chapter>, ManageError> {
        let mut tx = self.pool.begin().await?;
        let duration = infra::live_duration(&mut tx, id, workspace_id)
            .await?
            .ok_or(ManageError::NotFound)?;
        let list = ChapterList::parse(chapters, duration.and_then(|ms| u32::try_from(ms).ok()))?;
        let rows: Vec<(i32, &str)> = list
            .as_slice()
            .iter()
            .map(|chapter| {
                (
                    i32::try_from(chapter.start_ms).unwrap_or(i32::MAX),
                    chapter.title.as_str(),
                )
            })
            .collect();
        infra::replace_chapters(&mut tx, id, workspace_id, &rows).await?;
        tx.commit().await?;
        Ok(list.as_slice().to_vec())
    }

    /// Moves the recording to the trash. Its links stop resolving on the next request: the
    /// watch page only serves recordings outside the trash. Idempotent.
    #[tracing::instrument(skip_all, fields(recording_id = %id, workspace_id = %workspace_id))]
    pub async fn trash(
        &self,
        workspace_id: WorkspaceId,
        actor: UserId,
        id: RecordingId,
    ) -> Result<(), ManageError> {
        let mut tx = self.pool.begin().await?;
        if !infra::trash(&mut tx, id, workspace_id, self.clock.now()).await? {
            return Err(ManageError::NotFound);
        }
        self.outbox
            .push(
                &mut tx,
                &RecordingTrashed {
                    recording_id: id,
                    workspace_id,
                    trashed_by: actor,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `PurgeRecording`: deletes everything of recordings trashed more than
    /// [`TRASH_RETENTION`] ago: first the storage prefix (chunks, source, MP4, poster), then the
    /// rows, with `RecordingPurged` in the same transaction as the row delete. A storage failure
    /// leaves the recording trashed for the next run.
    #[tracing::instrument(skip_all)]
    pub async fn purge_due(&self) -> Result<PurgeReport, ManageError> {
        let cutoff = self.clock.now() - TRASH_RETENTION;
        let due = {
            let mut conn = self.pool.acquire().await?;
            infra::trashed_before(&mut conn, cutoff, PURGE_BATCH).await?
        };
        let mut report = PurgeReport::default();
        for (id, workspace_id) in due {
            let prefix = format!("ws/{workspace_id}/rec/{id}/");
            let objects = self.store.delete_prefix(&prefix).await?;
            let mut tx = self.pool.begin().await?;
            if infra::delete_trashed(&mut tx, id, workspace_id).await? {
                self.outbox
                    .push(
                        &mut tx,
                        &RecordingPurged {
                            recording_id: id,
                            workspace_id,
                            objects_deleted: objects,
                        },
                    )
                    .await?;
                tx.commit().await?;
                report.recordings += 1;
                report.objects += objects;
            }
        }
        tracing::info!(
            recordings = report.recordings,
            objects = report.objects,
            "purged trashed recordings"
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use platform::S3ObjectStore;

    use super::*;

    fn store() -> Arc<dyn ObjectStore> {
        Arc::new(S3ObjectStore::new(
            &std::env::var("S3_ENDPOINT").expect("S3_ENDPOINT set"),
            &std::env::var("S3_BUCKET").expect("S3_BUCKET set"),
            &std::env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY set"),
            &std::env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY set"),
        ))
    }

    fn manager(pool: PgPool) -> RecordingManager {
        RecordingManager::new(pool, store(), Arc::new(platform::SystemClock))
    }

    struct Seeded {
        workspace: WorkspaceId,
        owner: UserId,
        recording: RecordingId,
    }

    async fn seed(pool: &PgPool) -> Seeded {
        let owner = UserId::new_v7();
        let workspace = WorkspaceId::new_v7();
        let recording = RecordingId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'M')",
            owner.into_uuid(),
            format!("manage-{owner}@example.com")
        )
        .execute(pool)
        .await
        .expect("user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name) VALUES ($1, 'M')",
            workspace.into_uuid()
        )
        .execute(pool)
        .await
        .expect("workspace");
        sqlx::query!(
            "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
             VALUES ($1, $2, $3, 'Before', 'ready')",
            recording.into_uuid(),
            workspace.into_uuid(),
            owner.into_uuid()
        )
        .execute(pool)
        .await
        .expect("recording");
        Seeded {
            workspace,
            owner,
            recording,
        }
    }

    /// Puts a real object under the recording's prefix in MinIO.
    async fn put_object(seeded: &Seeded, name: &str) -> String {
        let key = format!("ws/{}/rec/{}/{name}", seeded.workspace, seeded.recording);
        let path = std::env::temp_dir().join(format!("catalog-purge-{}", uuid::Uuid::now_v7()));
        tokio::fs::write(&path, b"bytes").await.expect("write");
        store()
            .put_file(&key, &path, "application/octet-stream")
            .await
            .expect("put");
        tokio::fs::remove_file(&path).await.expect("tidy");
        key
    }

    async fn trashed_days_ago(pool: &PgPool, seeded: &Seeded, days: i32) {
        sqlx::query!(
            "UPDATE recordings SET trashed_at = now() - make_interval(days => $2) WHERE id = $1",
            seeded.recording.into_uuid(),
            days
        )
        .execute(pool)
        .await
        .expect("trash");
    }

    async fn exists(pool: &PgPool, seeded: &Seeded) -> bool {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM recordings WHERE id = $1) AS "e!""#,
            seeded.recording.into_uuid()
        )
        .fetch_one(pool)
        .await
        .expect("exists")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rename_changes_the_title_in_the_callers_workspace_only(pool: PgPool) {
        let seeded = seed(&pool).await;
        let other = seed(&pool).await;
        let manager = manager(pool.clone());
        let title = Title::parse(Some("  After  ")).expect("title");
        manager
            .rename(seeded.workspace, seeded.recording, &title)
            .await
            .expect("rename");
        let stored = sqlx::query_scalar!(
            "SELECT title FROM recordings WHERE id = $1",
            seeded.recording.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("title");
        assert_eq!(stored, "After");
        assert!(matches!(
            manager
                .rename(other.workspace, seeded.recording, &title)
                .await,
            Err(ManageError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn trash_hides_the_recording_at_once_and_writes_an_event(pool: PgPool) {
        let seeded = seed(&pool).await;
        let manager = manager(pool.clone());
        let mut conn = pool.acquire().await.expect("conn");
        assert!(
            crate::CatalogService::new()
                .watch_info(&mut conn, seeded.recording, seeded.workspace)
                .await
                .expect("info")
                .is_some()
        );
        drop(conn);

        manager
            .trash(seeded.workspace, seeded.owner, seeded.recording)
            .await
            .expect("trash");
        manager
            .trash(seeded.workspace, seeded.owner, seeded.recording)
            .await
            .expect("idempotent");

        let mut conn = pool.acquire().await.expect("conn");
        assert!(
            crate::CatalogService::new()
                .watch_info(&mut conn, seeded.recording, seeded.workspace)
                .await
                .expect("info")
                .is_none(),
            "a trashed recording can't be watched"
        );
        let page = crate::CatalogService::new()
            .library_page(&mut conn, seeded.workspace, None)
            .await
            .expect("page");
        assert!(page.items.is_empty(), "…or listed");
        drop(conn);
        let events = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM outbox_events WHERE event_type = 'RecordingTrashed' AND aggregate_id = $1"#,
            seeded.recording.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("events");
        assert_eq!(
            events, 2,
            "one per call; trashing twice keeps the first trash time"
        );

        let other = seed(&pool).await;
        assert!(matches!(
            manager
                .trash(other.workspace, other.owner, seeded.recording)
                .await,
            Err(ManageError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn purge_removes_the_storage_prefix_and_every_row_after_thirty_days(pool: PgPool) {
        let seeded = seed(&pool).await;
        let key_a = put_object(&seeded, "mp4/default.mp4").await;
        let key_b = put_object(&seeded, "takes/t/chunks/000000.webm").await;
        let manager = manager(pool.clone());
        // A link and a rendition hang off the recording and must go with it.
        sqlx::query!(
            "INSERT INTO share_links (id, workspace_id, recording_id, slug)
             VALUES ($1, $2, $3, 'purgeSlug123')",
            uuid::Uuid::now_v7(),
            seeded.workspace.into_uuid(),
            seeded.recording.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("link");
        trashed_days_ago(&pool, &seeded, 31).await;

        let report = manager.purge_due().await.expect("purge");
        assert_eq!(report.recordings, 1);
        assert_eq!(report.objects, 2);
        assert!(!exists(&pool, &seeded).await);
        for key in [key_a, key_b] {
            assert!(store().head(&key).await.expect("head").is_none(), "{key}");
        }
        let links = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM share_links"#)
            .fetch_one(&pool)
            .await
            .expect("links");
        assert_eq!(links, 0);
        let events = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM outbox_events WHERE event_type = 'RecordingPurged'"#
        )
        .fetch_one(&pool)
        .await
        .expect("events");
        assert_eq!(events, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn purge_leaves_recent_trash_live_recordings_and_other_prefixes(pool: PgPool) {
        let recent = seed(&pool).await;
        let live = seed(&pool).await;
        let old = seed(&pool).await;
        let recent_key = put_object(&recent, "mp4/default.mp4").await;
        let live_key = put_object(&live, "mp4/default.mp4").await;
        let _ = put_object(&old, "mp4/default.mp4").await;
        trashed_days_ago(&pool, &recent, 29).await;
        trashed_days_ago(&pool, &old, 40).await;

        let report = manager(pool.clone()).purge_due().await.expect("purge");
        assert_eq!(report.recordings, 1);
        assert!(exists(&pool, &recent).await, "29 days is not enough");
        assert!(exists(&pool, &live).await);
        assert!(!exists(&pool, &old).await);
        for key in [recent_key, live_key] {
            assert!(store().head(&key).await.expect("head").is_some(), "{key}");
        }
        for seeded in [&recent, &live] {
            let prefix = format!("ws/{}/rec/{}/", seeded.workspace, seeded.recording);
            store().delete_prefix(&prefix).await.expect("tidy storage");
        }
        // Nothing left to do on the next run.
        assert_eq!(
            manager(pool).purge_due().await.expect("purge"),
            PurgeReport::default()
        );
    }
}
