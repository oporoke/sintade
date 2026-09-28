use std::sync::Arc;
use std::time::Duration;

use catalog::{CatalogService, NewRecording, Title, TitleError};
use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use platform::{ObjectStore, StorageError};
use sqlx::PgPool;
use url::Url;

use crate::domain::{ChunkRangeError, MimeType, MimeTypeError, Sources, chunk_key, chunk_range};
use crate::infra;

/// How long a presigned chunk PUT stays valid (docs/design.md §9: 5 minutes).
pub const CHUNK_URL_TTL: Duration = Duration::from_secs(5 * 60);

/// `POST /recordings`: who is recording, where, and what the take will contain. The caller has
/// already checked the owner may create recordings in the workspace.
#[derive(Debug, Clone)]
pub struct StartRecording {
    pub workspace_id: WorkspaceId,
    pub owner_id: UserId,
    pub title: Option<String>,
    pub mime_type: String,
    pub sources: Sources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedRecording {
    pub recording_id: RecordingId,
    pub take_id: TakeId,
}

#[derive(Debug, thiserror::Error)]
pub enum StartRecordingError {
    #[error(transparent)]
    InvalidTitle(#[from] TitleError),

    #[error(transparent)]
    InvalidMimeType(#[from] MimeTypeError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Presigned PUT URLs for chunks `first_idx..first_idx + count` of a take.
#[derive(Debug, Clone, Copy)]
pub struct PresignChunks {
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub take_id: TakeId,
    pub first_idx: u32,
    pub count: u32,
}

/// One chunk's upload URL. The URL is a bearer credential: never log it.
#[derive(Clone)]
pub struct PresignedChunk {
    pub idx: u32,
    pub url: Url,
}

impl std::fmt::Debug for PresignedChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresignedChunk")
            .field("idx", &self.idx)
            .field("url", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PresignError {
    /// No such take, or it isn't the caller's to upload to (CLAUDE.md rule 5: `404`).
    #[error("take not found")]
    NotFound,

    #[error("take is already finalized")]
    Finalized,

    #[error(transparent)]
    InvalidRange(#[from] ChunkRangeError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct IngestService {
    pool: PgPool,
    catalog: Arc<CatalogService>,
    store: Arc<dyn ObjectStore>,
}

impl IngestService {
    pub fn new(pool: PgPool, catalog: Arc<CatalogService>, store: Arc<dyn ObjectStore>) -> Self {
        Self {
            pool,
            catalog,
            store,
        }
    }

    /// Creates a recording (via `catalog`) and its first take in one transaction, both in the
    /// caller's workspace. The recording starts in state `recording` with `current_take` set.
    #[tracing::instrument(skip_all, fields(workspace_id = %request.workspace_id, user_id = %request.owner_id))]
    pub async fn start_recording(
        &self,
        request: StartRecording,
    ) -> Result<StartedRecording, StartRecordingError> {
        let title = Title::parse(request.title.as_deref())?;
        let mime_type = MimeType::parse(&request.mime_type)?;
        let started = StartedRecording {
            recording_id: RecordingId::new_v7(),
            take_id: TakeId::new_v7(),
        };

        let mut tx = self.pool.begin().await?;
        self.catalog
            .create_recording(
                &mut tx,
                &NewRecording {
                    id: started.recording_id,
                    workspace_id: request.workspace_id,
                    owner_id: request.owner_id,
                    title,
                    current_take: started.take_id,
                },
            )
            .await?;
        infra::insert_take(
            &mut tx,
            started.take_id,
            request.workspace_id,
            started.recording_id,
            &mime_type,
            request.sources,
        )
        .await?;
        tx.commit().await?;

        tracing::info!(recording_id = %started.recording_id, take_id = %started.take_id, "recording started");
        Ok(started)
    }

    /// Presigned PUT URLs for a run of chunk indexes (upload protocol v1). Only the recording's
    /// owner, acting in its workspace, gets them, and only until the take is finalized.
    /// Re-presigning an index is allowed: a retry after a lost response, and a same-hash
    /// re-upload is a no-op at ack time.
    #[tracing::instrument(skip_all, fields(take_id = %request.take_id, first_idx = request.first_idx, count = request.count))]
    pub async fn presign_chunks(
        &self,
        request: PresignChunks,
    ) -> Result<Vec<PresignedChunk>, PresignError> {
        let range = chunk_range(request.first_idx, request.count)?;
        let take = infra::find_take_for_owner(
            &self.pool,
            request.take_id,
            request.workspace_id,
            request.user_id,
        )
        .await?
        .ok_or(PresignError::NotFound)?;
        if take.finalized {
            return Err(PresignError::Finalized);
        }

        let mut chunks = Vec::with_capacity(range.len());
        for idx in range {
            let key = chunk_key(
                request.workspace_id,
                take.recording_id,
                request.take_id,
                idx,
                &take.mime_type,
            );
            let url = self.store.presign_put(&key, CHUNK_URL_TTL).await?;
            chunks.push(PresignedChunk { idx, url });
        }
        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user and their workspace, inserted directly: ingest tests only need the foreign keys.
    async fn owner_and_workspace(pool: &PgPool) -> (UserId, WorkspaceId) {
        let user_id = UserId::new_v7();
        let workspace_id = WorkspaceId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Ingest Tester')",
            user_id.into_uuid(),
            format!("ingest-{user_id}@example.com"),
        )
        .execute(pool)
        .await
        .expect("insert user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name, is_personal) VALUES ($1, 'Ingest', true)",
            workspace_id.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("insert workspace");
        (user_id, workspace_id)
    }

    fn request(owner_id: UserId, workspace_id: WorkspaceId) -> StartRecording {
        StartRecording {
            workspace_id,
            owner_id,
            title: None,
            mime_type: "video/webm;codecs=vp9,opus".to_string(),
            sources: Sources {
                system_audio: false,
                mic: true,
                camera: false,
            },
        }
    }

    fn service(pool: &PgPool) -> IngestService {
        IngestService::new(
            pool.clone(),
            Arc::new(CatalogService::new()),
            Arc::new(FakeStore),
        )
    }

    /// Presigns deterministic fake URLs, so service tests need no MinIO.
    struct FakeStore;

    #[async_trait::async_trait]
    impl ObjectStore for FakeStore {
        async fn presign_put(&self, key: &str, _ttl: Duration) -> Result<Url, StorageError> {
            Ok(Url::parse(&format!("http://store.test/{key}"))?)
        }
        async fn presign_get(&self, key: &str, _ttl: Duration) -> Result<Url, StorageError> {
            Ok(Url::parse(&format!("http://store.test/{key}"))?)
        }
        async fn head(&self, _key: &str) -> Result<Option<platform::ObjectMeta>, StorageError> {
            Ok(None)
        }
        async fn delete_prefix(&self, _prefix: &str) -> Result<u64, StorageError> {
            Ok(0)
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn creates_the_recording_and_its_take_in_the_callers_workspace(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let started = service(&pool)
            .start_recording(request(owner, workspace))
            .await
            .expect("start");

        let recording = sqlx::query!(
            r#"SELECT workspace_id, owner_id, title, state::text AS "state!", current_take
               FROM recordings WHERE id = $1"#,
            started.recording_id.into_uuid(),
        )
        .fetch_one(&pool)
        .await
        .expect("recording row");
        assert_eq!(recording.workspace_id, workspace.into_uuid());
        assert_eq!(recording.owner_id, owner.into_uuid());
        assert_eq!(recording.title, "Untitled recording");
        assert_eq!(recording.state, "recording");
        assert_eq!(recording.current_take, Some(started.take_id.into_uuid()));

        let take = sqlx::query!(
            "SELECT workspace_id, recording_id, mime_type, has_system_audio, has_mic, has_camera, finalized_at
             FROM takes WHERE id = $1",
            started.take_id.into_uuid(),
        )
        .fetch_one(&pool)
        .await
        .expect("take row");
        assert_eq!(take.workspace_id, workspace.into_uuid());
        assert_eq!(take.recording_id, started.recording_id.into_uuid());
        assert_eq!(take.mime_type, "video/webm;codecs=vp9,opus");
        assert!(!take.has_system_audio && take.has_mic && !take.has_camera);
        assert!(take.finalized_at.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn invalid_input_writes_nothing(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);

        let mut bad_mime = request(owner, workspace);
        bad_mime.mime_type = "audio/ogg".to_string();
        assert!(matches!(
            ingest.start_recording(bad_mime).await,
            Err(StartRecordingError::InvalidMimeType(
                MimeTypeError::Unsupported
            ))
        ));

        let mut bad_title = request(owner, workspace);
        bad_title.title = Some("x".repeat(catalog::MAX_TITLE_CHARS + 1));
        assert!(matches!(
            ingest.start_recording(bad_title).await,
            Err(StartRecordingError::InvalidTitle(TitleError::TooLong))
        ));

        let rows = sqlx::query_scalar!(
            r#"SELECT (SELECT count(*) FROM recordings) + (SELECT count(*) FROM takes) AS "n!""#
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(rows, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_failed_take_insert_leaves_no_recording_behind(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        // Make the second statement of the transaction fail, after the recording insert.
        sqlx::query!("ALTER TABLE takes ADD CONSTRAINT fail_every_take CHECK (false) NOT VALID")
            .execute(&pool)
            .await
            .expect("add failing constraint");

        let result = service(&pool)
            .start_recording(request(owner, workspace))
            .await;
        assert!(
            matches!(result, Err(StartRecordingError::Database(_))),
            "{result:?}"
        );

        let recordings = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM recordings"#)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(recordings, 0, "the recording must roll back with its take");
    }

    fn presign(
        owner: UserId,
        workspace: WorkspaceId,
        take: TakeId,
        first_idx: u32,
        count: u32,
    ) -> PresignChunks {
        PresignChunks {
            workspace_id: workspace,
            user_id: owner,
            take_id: take,
            first_idx,
            count,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn presigns_a_batch_of_chunk_keys_under_the_recording(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);
        let started = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("start");

        let chunks = ingest
            .presign_chunks(presign(owner, workspace, started.take_id, 3, 3))
            .await
            .expect("presign");
        let keys: Vec<String> = chunks
            .iter()
            .map(|chunk| format!("{}:{}", chunk.idx, chunk.url.path()))
            .collect();
        let prefix = format!(
            "/ws/{workspace}/rec/{}/takes/{}/chunks",
            started.recording_id, started.take_id
        );
        assert_eq!(
            keys,
            vec![
                format!("3:{prefix}/000003.webm"),
                format!("4:{prefix}/000004.webm"),
                format!("5:{prefix}/000005.webm"),
            ]
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_the_owner_in_the_takes_workspace_may_presign(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let (stranger, strangers_workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);
        let started = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("start");

        for (user, ws) in [
            (stranger, strangers_workspace),
            (stranger, workspace),
            (owner, strangers_workspace),
        ] {
            let result = ingest
                .presign_chunks(presign(user, ws, started.take_id, 0, 1))
                .await;
            assert!(matches!(result, Err(PresignError::NotFound)), "{result:?}");
        }
        let unknown = ingest
            .presign_chunks(presign(owner, workspace, TakeId::new_v7(), 0, 1))
            .await;
        assert!(
            matches!(unknown, Err(PresignError::NotFound)),
            "{unknown:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_finalized_take_gets_no_more_urls(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);
        let started = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("start");
        sqlx::query!(
            "UPDATE takes SET finalized_at = now() WHERE id = $1",
            started.take_id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("finalize");

        let result = ingest
            .presign_chunks(presign(owner, workspace, started.take_id, 0, 1))
            .await;
        assert!(matches!(result, Err(PresignError::Finalized)), "{result:?}");
    }
}
