use std::sync::Arc;
use std::time::Duration;

use billing::BillingService;
use catalog::{CatalogService, NewRecording, Title, TitleError};
use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use platform::{Clock, ObjectStore, Outbox, OutboxError, StorageError};
use sqlx::PgPool;
use url::Url;

use crate::domain::{
    ChunkRangeError, ChunkSizeError, DigestError, MAX_CHUNK_INDEX, MimeType, MimeTypeError,
    Sha256Digest, Sources, chunk_key, chunk_range, chunk_size,
};
use crate::events::TakeFinalized;
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
    /// The longest take the workspace's plan accepts, so the recorder can stop in time.
    pub max_duration_ms: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum StartRecordingError {
    #[error(transparent)]
    InvalidTitle(#[from] TitleError),

    #[error(transparent)]
    InvalidMimeType(#[from] MimeTypeError),

    /// The plan's recording limit is reached (free tier: 50).
    #[error("your plan allows {max} recordings; delete one to record another")]
    LimitReached { max: u32 },

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

/// The client's claim that chunk `idx` is uploaded: its size and SHA-256 (hex).
#[derive(Debug, Clone)]
pub struct AckChunk {
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub take_id: TakeId,
    pub idx: u32,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// First ack for this index: recorded now.
    Acked,
    /// The same size and hash were already recorded: a retry, nothing changed.
    AlreadyAcked,
}

#[derive(Debug, thiserror::Error)]
pub enum AckError {
    #[error("take not found")]
    NotFound,

    #[error("take is already finalized")]
    Finalized,

    /// A different size or hash is already recorded for this index (docs/design.md §9: `409`).
    #[error("chunk {idx} was already acknowledged with a different hash or size")]
    Mismatch { idx: u32 },

    #[error("chunk index must be at most {MAX_CHUNK_INDEX}")]
    IndexTooLarge,

    #[error(transparent)]
    InvalidDigest(#[from] DigestError),

    #[error(transparent)]
    InvalidSize(#[from] ChunkSizeError),

    #[error("chunk {idx} has not been uploaded")]
    NotUploaded { idx: u32 },

    #[error("chunk {idx} is {stored} bytes in storage, not {claimed}")]
    SizeMismatch { idx: u32, stored: u64, claimed: u64 },

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// What the server has for a take, so an interrupted upload can resume (docs/design.md §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeStatus {
    pub recording_id: RecordingId,
    pub finalized: bool,
    /// Acknowledged chunk indexes, ascending.
    pub received: Vec<u32>,
    /// The same chunks with their recorded size and hash, so a client can check its local copy
    /// matches before skipping it.
    pub chunks: Vec<ReceivedChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedChunk {
    pub idx: u32,
    pub size_bytes: u32,
    /// Lowercase hex.
    pub sha256: String,
}

/// Declares the take complete: `chunk_count` chunks (indexes `0..chunk_count`) lasting
/// `duration_ms` (pauses excluded).
#[derive(Debug, Clone, Copy)]
pub struct FinalizeTake {
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub take_id: TakeId,
    pub chunk_count: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeOutcome {
    /// Finalized now: the recording is `processing` and `TakeFinalized` is in the outbox.
    Finalized { recording_id: RecordingId },
    /// Already finalized with the same chunk count: a retry, nothing changed.
    AlreadyFinalized { recording_id: RecordingId },
}

/// Longest list of missing indexes returned; `missing_count` always has the total.
pub const MAX_MISSING_LISTED: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum FinalizeError {
    #[error("take not found")]
    NotFound,

    #[error("chunk_count must be between 1 and {}", MAX_CHUNK_INDEX + 1)]
    InvalidChunkCount,

    #[error("duration_ms must be at most {}", i32::MAX)]
    InvalidDuration,

    /// Longer than the plan allows (free tier: 10 minutes).
    #[error("your plan allows takes of up to {} minutes", max_ms / 60_000)]
    DurationExceeded { max_ms: u32 },

    /// Some of `0..chunk_count` were never acknowledged (docs/design.md §9: `422` with the
    /// missing indexes). `missing` lists at most [`MAX_MISSING_LISTED`].
    #[error("{missing_count} chunk(s) missing")]
    MissingChunks {
        missing: Vec<u32>,
        missing_count: usize,
    },

    #[error("chunks at or beyond chunk_count were acknowledged: {0:?}")]
    UnexpectedChunks(Vec<u32>),

    /// Finalized earlier with a different number of chunks.
    #[error("take was already finalized with {finalized_with} chunk(s)")]
    CountMismatch { finalized_with: u32 },

    /// The recording was abandoned or already moved on.
    #[error("the recording no longer accepts uploads")]
    RecordingClosed,

    #[error(transparent)]
    Outbox(#[from] OutboxError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// What one `SweepStaleUploads` run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Recordings moved to `abandoned` (upload idle > 24 h).
    pub abandoned: usize,
    /// Unfinalized takes whose chunks were deleted (idle > 7 days).
    pub purged_takes: usize,
}

/// An upload idle this long is abandoned (docs/design.md §10 Recover step 4).
pub const ABANDON_AFTER: time::Duration = time::Duration::hours(24);
/// Chunks of an abandoned upload are deleted this long after its last activity.
pub const PURGE_AFTER: time::Duration = time::Duration::days(7);
/// Most takes one sweep handles per step; the next run continues.
const SWEEP_BATCH: i64 = 500;

pub struct IngestService {
    pool: PgPool,
    catalog: Arc<CatalogService>,
    billing: Arc<BillingService>,
    store: Arc<dyn ObjectStore>,
    clock: Arc<dyn Clock>,
    outbox: Outbox,
}

impl IngestService {
    pub fn new(
        pool: PgPool,
        catalog: Arc<CatalogService>,
        billing: Arc<BillingService>,
        store: Arc<dyn ObjectStore>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            pool,
            catalog,
            billing,
            store,
            outbox: Outbox::new(clock.clone()),
            clock,
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
        let entitlements = self.billing.entitlements(request.workspace_id).await;
        let started = StartedRecording {
            recording_id: RecordingId::new_v7(),
            take_id: TakeId::new_v7(),
            max_duration_ms: entitlements.max_duration_ms,
        };

        let mut tx = self.pool.begin().await?;
        // Count and insert under a per-workspace lock: concurrent creates can't both take the
        // last free slot.
        let active = self
            .catalog
            .lock_and_count_active(&mut tx, request.workspace_id)
            .await?;
        if active >= u64::from(entitlements.max_recordings) {
            return Err(StartRecordingError::LimitReached {
                max: entitlements.max_recordings,
            });
        }
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

    /// Records that chunk `idx` is uploaded (upload protocol v1). Idempotent on
    /// `(take_id, idx)`: the same size and hash again is a no-op (even after finalize, so a
    /// retried ack whose response was lost still succeeds); a different one is a conflict.
    /// A new ack must match what is actually stored: the object exists with the claimed size.
    /// The hash is the client's; the API never reads media (CLAUDE.md rule 4).
    #[tracing::instrument(skip_all, fields(take_id = %request.take_id, idx = request.idx))]
    pub async fn ack_chunk(&self, request: AckChunk) -> Result<AckOutcome, AckError> {
        if request.idx > MAX_CHUNK_INDEX {
            return Err(AckError::IndexTooLarge);
        }
        let digest = Sha256Digest::parse_hex(&request.sha256)?;
        let size = chunk_size(request.size_bytes)?;
        // Both fit: idx <= 99 999 and size <= 16 MiB.
        let idx = i32::try_from(request.idx).map_err(|_| AckError::IndexTooLarge)?;
        let size_i32 = i32::try_from(size).map_err(|_| ChunkSizeError::OutOfRange)?;

        let take = infra::find_take_for_owner(
            &self.pool,
            request.take_id,
            request.workspace_id,
            request.user_id,
        )
        .await?
        .ok_or(AckError::NotFound)?;

        let compare = |receipt: infra::ChunkReceipt| {
            if receipt.size_bytes == size_i32
                && Sha256Digest::from_bytes(&receipt.sha256) == Some(digest)
            {
                Ok(AckOutcome::AlreadyAcked)
            } else {
                Err(AckError::Mismatch { idx: request.idx })
            }
        };

        if let Some(receipt) =
            infra::find_chunk(&self.pool, request.take_id, request.workspace_id, idx).await?
        {
            return compare(receipt);
        }
        if take.finalized {
            return Err(AckError::Finalized);
        }

        let key = chunk_key(
            request.workspace_id,
            take.recording_id,
            request.take_id,
            request.idx,
            &take.mime_type,
        );
        let stored = self
            .store
            .head(&key)
            .await?
            .ok_or(AckError::NotUploaded { idx: request.idx })?;
        if stored.size != size {
            return Err(AckError::SizeMismatch {
                idx: request.idx,
                stored: stored.size,
                claimed: size,
            });
        }

        let inserted = infra::insert_chunk(
            &self.pool,
            request.take_id,
            request.workspace_id,
            idx,
            size_i32,
            digest.as_bytes(),
        )
        .await?;
        if inserted {
            return Ok(AckOutcome::Acked);
        }
        // A concurrent ack for the same index got there first.
        let receipt = infra::find_chunk(&self.pool, request.take_id, request.workspace_id, idx)
            .await?
            .ok_or(AckError::NotFound)?;
        compare(receipt)
    }

    /// Which chunks the server has, for resuming an interrupted upload. Owner only.
    #[tracing::instrument(skip_all, fields(take_id = %take_id))]
    pub async fn take_status(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        take_id: TakeId,
    ) -> Result<Option<TakeStatus>, sqlx::Error> {
        let Some(take) =
            infra::find_take_for_owner(&self.pool, take_id, workspace_id, user_id).await?
        else {
            return Ok(None);
        };
        let chunks: Vec<ReceivedChunk> = infra::receipts(&self.pool, take_id, workspace_id)
            .await?
            .into_iter()
            .filter_map(|receipt| {
                Some(ReceivedChunk {
                    idx: u32::try_from(receipt.idx).ok()?,
                    size_bytes: u32::try_from(receipt.size_bytes).ok()?,
                    sha256: Sha256Digest::from_bytes(&receipt.sha256)?.to_hex(),
                })
            })
            .collect();
        Ok(Some(TakeStatus {
            recording_id: take.recording_id,
            finalized: take.finalized,
            received: chunks.iter().map(|chunk| chunk.idx).collect(),
            chunks,
        }))
    }

    /// Finalizes a take once every chunk `0..chunk_count` is acknowledged: marks the take
    /// finalized, moves the recording to `processing`, and writes `TakeFinalized` to the outbox,
    /// all in one transaction. Retrying with the same count after success is a no-op.
    #[tracing::instrument(skip_all, fields(take_id = %request.take_id, chunk_count = request.chunk_count))]
    pub async fn finalize(&self, request: FinalizeTake) -> Result<FinalizeOutcome, FinalizeError> {
        if request.chunk_count == 0 || request.chunk_count > MAX_CHUNK_INDEX + 1 {
            return Err(FinalizeError::InvalidChunkCount);
        }
        let duration_ms =
            i32::try_from(request.duration_ms).map_err(|_| FinalizeError::InvalidDuration)?;
        let max_ms = self
            .billing
            .entitlements(request.workspace_id)
            .await
            .max_duration_ms;
        if request.duration_ms > max_ms {
            return Err(FinalizeError::DurationExceeded { max_ms });
        }

        let mut tx = self.pool.begin().await?;
        let take = infra::lock_take_for_owner(
            &mut tx,
            request.take_id,
            request.workspace_id,
            request.user_id,
        )
        .await?
        .ok_or(FinalizeError::NotFound)?;
        let received: Vec<u32> =
            infra::received_indexes(&mut *tx, request.take_id, request.workspace_id)
                .await?
                .into_iter()
                .filter_map(|idx| u32::try_from(idx).ok())
                .collect();

        if take.finalized {
            let finalized_with = u32::try_from(received.len()).unwrap_or(u32::MAX);
            return if finalized_with == request.chunk_count {
                Ok(FinalizeOutcome::AlreadyFinalized {
                    recording_id: take.recording_id,
                })
            } else {
                Err(FinalizeError::CountMismatch { finalized_with })
            };
        }

        let unexpected: Vec<u32> = received
            .iter()
            .copied()
            .filter(|&idx| idx >= request.chunk_count)
            .collect();
        if !unexpected.is_empty() {
            return Err(FinalizeError::UnexpectedChunks(unexpected));
        }
        // `received` is ascending and all below chunk_count, so walk both in step.
        let mut have = received.iter().copied().peekable();
        let mut missing = Vec::new();
        let mut missing_count = 0usize;
        for idx in 0..request.chunk_count {
            if have.peek() == Some(&idx) {
                have.next();
            } else {
                missing_count += 1;
                if missing.len() < MAX_MISSING_LISTED {
                    missing.push(idx);
                }
            }
        }
        if missing_count > 0 {
            return Err(FinalizeError::MissingChunks {
                missing,
                missing_count,
            });
        }

        infra::mark_take_finalized(&mut tx, request.take_id, request.workspace_id).await?;
        if !self
            .catalog
            .mark_processing(
                &mut tx,
                take.recording_id,
                request.workspace_id,
                duration_ms,
            )
            .await?
        {
            return Err(FinalizeError::RecordingClosed);
        }
        self.outbox
            .push(
                &mut tx,
                &TakeFinalized {
                    take_id: request.take_id,
                    recording_id: take.recording_id,
                    workspace_id: request.workspace_id,
                    chunk_count: request.chunk_count,
                    duration_ms: request.duration_ms,
                    mime: take.mime_type,
                },
            )
            .await?;
        tx.commit().await?;

        tracing::info!(recording_id = %take.recording_id, "take finalized");
        Ok(FinalizeOutcome::Finalized {
            recording_id: take.recording_id,
        })
    }

    /// `SweepStaleUploads` (docs/design.md §10 Recover step 4): recordings whose upload has been
    /// idle for more than [`ABANDON_AFTER`] become `abandoned` (catalog decides, from
    /// `recording`/`uploading` only), and the chunks of unfinalized takes idle for more than
    /// [`PURGE_AFTER`] are deleted from storage and the database. Idle means no chunk acked and
    /// no take created since. A system job: it spans workspaces by design, and every row it
    /// touches is addressed by its own workspace.
    #[tracing::instrument(skip_all)]
    pub async fn sweep_stale_uploads(&self) -> Result<SweepReport, SweepError> {
        let now = self.clock.now();
        let mut report = SweepReport::default();

        // The takes are marked and their recordings abandoned in one transaction.
        let mut tx = self.pool.begin().await?;
        let idle =
            infra::abandon_idle_takes(&mut *tx, now, now - ABANDON_AFTER, SWEEP_BATCH).await?;
        if !idle.is_empty() {
            let recordings: Vec<(RecordingId, WorkspaceId)> = idle
                .iter()
                .map(|take| (take.recording_id, take.workspace_id))
                .collect();
            report.abandoned = self
                .catalog
                .mark_abandoned(&mut tx, &recordings)
                .await?
                .len();
        }
        tx.commit().await?;

        let expired = infra::purgeable_takes(&self.pool, now - PURGE_AFTER, SWEEP_BATCH).await?;
        for take in expired {
            let prefix = format!(
                "ws/{}/rec/{}/takes/{}/chunks/",
                take.workspace_id, take.recording_id, take.take_id
            );
            self.store.delete_prefix(&prefix).await?;
            infra::delete_chunks(&self.pool, take.take_id, take.workspace_id).await?;
            report.purged_takes += 1;
        }

        tracing::info!(
            abandoned = report.abandoned,
            purged_takes = report.purged_takes,
            "stale uploads swept"
        );
        Ok(report)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SweepError {
    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
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
        service_with(pool, Arc::new(FakeStore::default()))
    }

    fn service_with(pool: &PgPool, store: Arc<FakeStore>) -> IngestService {
        IngestService::new(
            pool.clone(),
            Arc::new(CatalogService::new()),
            Arc::new(BillingService::new()),
            store,
            Arc::new(platform::SystemClock),
        )
    }

    /// Presigns deterministic fake URLs and answers `HEAD` from `objects`, so service tests
    /// need no MinIO.
    #[derive(Default)]
    struct FakeStore {
        objects: std::sync::Mutex<std::collections::HashMap<String, u64>>,
        deleted: std::sync::Mutex<Vec<String>>,
    }

    impl FakeStore {
        fn put(&self, key: String, size: u64) {
            self.objects.lock().expect("lock").insert(key, size);
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for FakeStore {
        async fn presign_put(&self, key: &str, _ttl: Duration) -> Result<Url, StorageError> {
            Ok(Url::parse(&format!("http://store.test/{key}"))?)
        }
        async fn presign_get(&self, key: &str, _ttl: Duration) -> Result<Url, StorageError> {
            Ok(Url::parse(&format!("http://store.test/{key}"))?)
        }
        async fn head(&self, key: &str) -> Result<Option<platform::ObjectMeta>, StorageError> {
            Ok(self
                .objects
                .lock()
                .expect("lock")
                .get(key)
                .map(|&size| platform::ObjectMeta {
                    size,
                    content_type: None,
                }))
        }
        async fn delete_prefix(&self, prefix: &str) -> Result<u64, StorageError> {
            self.deleted.lock().expect("lock").push(prefix.to_string());
            let mut objects = self.objects.lock().expect("lock");
            let before = objects.len();
            objects.retain(|key, _| !key.starts_with(prefix));
            Ok((before - objects.len()) as u64)
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

    const HASH_A: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const HASH_B: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    struct Uploading {
        store: Arc<FakeStore>,
        ingest: IngestService,
        owner: UserId,
        workspace: WorkspaceId,
        started: StartedRecording,
    }

    async fn uploading(pool: &PgPool) -> Uploading {
        let (owner, workspace) = owner_and_workspace(pool).await;
        let store = Arc::new(FakeStore::default());
        let ingest = service_with(pool, store.clone());
        let started = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("start");
        Uploading {
            store,
            ingest,
            owner,
            workspace,
            started,
        }
    }

    impl Uploading {
        /// Puts chunk `idx` of `size` bytes into the fake store.
        fn upload(&self, idx: u32, size: u64) {
            self.store.put(
                chunk_key(
                    self.workspace,
                    self.started.recording_id,
                    self.started.take_id,
                    idx,
                    "video/webm",
                ),
                size,
            );
        }

        async fn ack(
            &self,
            idx: u32,
            size_bytes: u64,
            sha256: &str,
        ) -> Result<AckOutcome, AckError> {
            self.ingest
                .ack_chunk(AckChunk {
                    workspace_id: self.workspace,
                    user_id: self.owner,
                    take_id: self.started.take_id,
                    idx,
                    size_bytes,
                    sha256: sha256.to_string(),
                })
                .await
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn ack_is_idempotent_for_the_same_hash(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload(0, 1000);
        assert_eq!(
            up.ack(0, 1000, HASH_A).await.expect("first"),
            AckOutcome::Acked
        );
        assert_eq!(
            up.ack(0, 1000, HASH_A).await.expect("again"),
            AckOutcome::AlreadyAcked
        );
        // Case of the hex doesn't matter.
        assert_eq!(
            up.ack(0, 1000, &HASH_A.to_ascii_uppercase())
                .await
                .expect("upper"),
            AckOutcome::AlreadyAcked
        );

        let rows = sqlx::query!(
            "SELECT idx, size_bytes, sha256, workspace_id FROM chunks WHERE take_id = $1",
            up.started.take_id.into_uuid()
        )
        .fetch_all(&pool)
        .await
        .expect("chunks");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].idx, rows[0].size_bytes), (0, 1000));
        assert_eq!(
            Sha256Digest::from_bytes(&rows[0].sha256)
                .expect("32 bytes")
                .to_hex(),
            HASH_A
        );
        assert_eq!(rows[0].workspace_id, up.workspace.into_uuid());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_different_hash_or_size_for_an_acked_index_conflicts(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload(0, 1000);
        up.ack(0, 1000, HASH_A).await.expect("first");

        assert!(matches!(
            up.ack(0, 1000, HASH_B).await,
            Err(AckError::Mismatch { idx: 0 })
        ));
        up.upload(0, 1001);
        assert!(matches!(
            up.ack(0, 1001, HASH_A).await,
            Err(AckError::Mismatch { idx: 0 })
        ));

        let stored = sqlx::query_scalar!(
            "SELECT sha256 FROM chunks WHERE take_id = $1 AND idx = 0",
            up.started.take_id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("chunk");
        assert_eq!(
            Sha256Digest::from_bytes(&stored)
                .expect("32 bytes")
                .to_hex(),
            HASH_A,
            "the first ack stands"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_new_ack_must_match_the_stored_object(pool: PgPool) {
        let up = uploading(&pool).await;
        assert!(matches!(
            up.ack(0, 1000, HASH_A).await,
            Err(AckError::NotUploaded { idx: 0 })
        ));
        up.upload(0, 999);
        assert!(matches!(
            up.ack(0, 1000, HASH_A).await,
            Err(AckError::SizeMismatch {
                idx: 0,
                stored: 999,
                claimed: 1000
            })
        ));
        assert!(matches!(
            up.ack(0, 0, HASH_A).await,
            Err(AckError::InvalidSize(_))
        ));
        assert!(matches!(
            up.ack(0, crate::domain::MAX_CHUNK_BYTES + 1, HASH_A).await,
            Err(AckError::InvalidSize(_))
        ));
        assert!(matches!(
            up.ack(0, 999, "not-hex").await,
            Err(AckError::InvalidDigest(_))
        ));
        assert!(matches!(
            up.ack(MAX_CHUNK_INDEX + 1, 999, HASH_A).await,
            Err(AckError::IndexTooLarge)
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn after_finalize_only_identical_retries_succeed(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload(0, 1000);
        up.upload(1, 1000);
        up.ack(0, 1000, HASH_A).await.expect("ack 0");
        sqlx::query!(
            "UPDATE takes SET finalized_at = now() WHERE id = $1",
            up.started.take_id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("finalize");

        assert_eq!(
            up.ack(0, 1000, HASH_A).await.expect("retry"),
            AckOutcome::AlreadyAcked
        );
        assert!(matches!(
            up.ack(1, 1000, HASH_A).await,
            Err(AckError::Finalized)
        ));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_the_owner_may_ack(pool: PgPool) {
        let up = uploading(&pool).await;
        let (stranger, strangers_workspace) = owner_and_workspace(&pool).await;
        up.upload(0, 1000);
        let result = up
            .ingest
            .ack_chunk(AckChunk {
                workspace_id: strangers_workspace,
                user_id: stranger,
                take_id: up.started.take_id,
                idx: 0,
                size_bytes: 1000,
                sha256: HASH_A.to_string(),
            })
            .await;
        assert!(matches!(result, Err(AckError::NotFound)), "{result:?}");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn concurrent_acks_of_one_index_record_it_once(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload(0, 1000);
        up.upload(1, 1000);

        let (a, b) = tokio::join!(up.ack(0, 1000, HASH_A), up.ack(0, 1000, HASH_A));
        let mut outcomes = [a.expect("a"), b.expect("b")];
        outcomes.sort_by_key(|outcome| *outcome == AckOutcome::AlreadyAcked);
        assert_eq!(outcomes, [AckOutcome::Acked, AckOutcome::AlreadyAcked]);

        let (a, b) = tokio::join!(up.ack(1, 1000, HASH_A), up.ack(1, 1000, HASH_B));
        let results = [a, b];
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Ok(AckOutcome::Acked)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(AckError::Mismatch { idx: 1 })))
                .count(),
            1
        );
    }

    impl Uploading {
        /// Uploads and acks each index with a 1000-byte chunk.
        async fn upload_and_ack(&self, indexes: &[u32]) {
            for &idx in indexes {
                self.upload(idx, 1000);
                self.ack(idx, 1000, HASH_A).await.expect("ack");
            }
        }

        async fn finalize(&self, chunk_count: u32) -> Result<FinalizeOutcome, FinalizeError> {
            self.ingest
                .finalize(FinalizeTake {
                    workspace_id: self.workspace,
                    user_id: self.owner,
                    take_id: self.started.take_id,
                    chunk_count,
                    duration_ms: 6_000,
                })
                .await
        }

        async fn status(&self) -> TakeStatus {
            self.ingest
                .take_status(self.workspace, self.owner, self.started.take_id)
                .await
                .expect("status")
                .expect("owner sees the take")
        }
    }

    async fn recording_state(pool: &PgPool, id: RecordingId) -> (String, Option<i32>) {
        let row = sqlx::query!(
            r#"SELECT state::text AS "state!", duration_ms FROM recordings WHERE id = $1"#,
            id.into_uuid()
        )
        .fetch_one(pool)
        .await
        .expect("recording");
        (row.state, row.duration_ms)
    }

    async fn outbox_events(pool: &PgPool, take: TakeId) -> Vec<serde_json::Value> {
        sqlx::query_scalar!(
            "SELECT payload FROM outbox_events WHERE event_type = 'TakeFinalized' AND aggregate_id = $1",
            take.into_uuid()
        )
        .fetch_all(pool)
        .await
        .expect("outbox")
    }

    /// Day 36's Check: finalizing with a gap returns the missing indexes and changes nothing.
    #[sqlx::test(migrations = "../../migrations")]
    async fn finalize_with_a_gap_returns_the_missing_indexes(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0, 1, 3, 5]).await;

        match up.finalize(7).await {
            Err(FinalizeError::MissingChunks {
                missing,
                missing_count,
            }) => {
                assert_eq!(missing, vec![2, 4, 6]);
                assert_eq!(missing_count, 3);
            }
            other => panic!("expected missing chunks, got {other:?}"),
        }
        let status = up.status().await;
        assert!(!status.finalized);
        assert_eq!(status.received, vec![0, 1, 3, 5]);
        assert_eq!(
            status.chunks[1],
            ReceivedChunk {
                idx: 1,
                size_bytes: 1000,
                sha256: HASH_A.to_string()
            }
        );
        assert_eq!(
            recording_state(&pool, up.started.recording_id).await,
            ("recording".to_string(), None)
        );
        assert!(outbox_events(&pool, up.started.take_id).await.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn finalize_moves_the_recording_to_processing_and_emits_take_finalized(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0, 1, 2]).await;

        let outcome = up.finalize(3).await.expect("finalize");
        assert_eq!(
            outcome,
            FinalizeOutcome::Finalized {
                recording_id: up.started.recording_id
            }
        );
        assert!(up.status().await.finalized);
        assert_eq!(
            recording_state(&pool, up.started.recording_id).await,
            ("processing".to_string(), Some(6_000))
        );

        let events = outbox_events(&pool, up.started.take_id).await;
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event["event_type"], "TakeFinalized");
        assert_eq!(event["workspace_id"], up.workspace.to_string());
        assert_eq!(event["data"]["take_id"], up.started.take_id.to_string());
        assert_eq!(
            event["data"]["recording_id"],
            up.started.recording_id.to_string()
        );
        assert_eq!(event["data"]["chunk_count"], 3);
        assert_eq!(event["data"]["duration_ms"], 6_000);
        assert_eq!(event["data"]["mime"], "video/webm;codecs=vp9,opus");

        // Retrying the same finalize is a no-op; a different count conflicts.
        assert_eq!(
            up.finalize(3).await.expect("retry"),
            FinalizeOutcome::AlreadyFinalized {
                recording_id: up.started.recording_id
            }
        );
        assert!(matches!(
            up.finalize(4).await,
            Err(FinalizeError::CountMismatch { finalized_with: 3 })
        ));
        assert_eq!(
            outbox_events(&pool, up.started.take_id).await.len(),
            1,
            "one event only"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn finalize_rejects_bad_counts_and_extra_chunks(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0, 1, 2]).await;
        assert!(matches!(
            up.finalize(0).await,
            Err(FinalizeError::InvalidChunkCount)
        ));
        assert!(matches!(
            up.finalize(MAX_CHUNK_INDEX + 2).await,
            Err(FinalizeError::InvalidChunkCount)
        ));
        match up.finalize(2).await {
            Err(FinalizeError::UnexpectedChunks(extra)) => assert_eq!(extra, vec![2]),
            other => panic!("expected unexpected chunks, got {other:?}"),
        }
        let many = up.finalize(MAX_CHUNK_INDEX + 1).await;
        match many {
            Err(FinalizeError::MissingChunks {
                missing,
                missing_count,
            }) => {
                assert_eq!(missing.len(), MAX_MISSING_LISTED);
                assert_eq!(missing[0], 3);
                assert_eq!(missing_count, (MAX_CHUNK_INDEX + 1 - 3) as usize);
            }
            other => panic!("expected missing chunks, got {other:?}"),
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_abandoned_recording_cannot_be_finalized(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0]).await;
        sqlx::query!(
            "UPDATE recordings SET state = 'abandoned' WHERE id = $1",
            up.started.recording_id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("abandon");

        assert!(matches!(
            up.finalize(1).await,
            Err(FinalizeError::RecordingClosed)
        ));
        assert!(
            !up.status().await.finalized,
            "the take's update rolled back"
        );
        assert!(outbox_events(&pool, up.started.take_id).await.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn status_and_finalize_are_owner_only(pool: PgPool) {
        let up = uploading(&pool).await;
        let (stranger, strangers_workspace) = owner_and_workspace(&pool).await;
        assert!(
            up.ingest
                .take_status(strangers_workspace, stranger, up.started.take_id)
                .await
                .expect("query")
                .is_none()
        );
        let result = up
            .ingest
            .finalize(FinalizeTake {
                workspace_id: strangers_workspace,
                user_id: stranger,
                take_id: up.started.take_id,
                chunk_count: 1,
                duration_ms: 1,
            })
            .await;
        assert!(matches!(result, Err(FinalizeError::NotFound)), "{result:?}");
    }

    /// Day 41's Check, part 1: the 51st recording is rejected (free tier: 50).
    #[sqlx::test(migrations = "../../migrations")]
    async fn the_51st_recording_is_rejected(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);
        for _ in 0..50 {
            let started = ingest
                .start_recording(request(owner, workspace))
                .await
                .expect("within the plan");
            assert_eq!(started.max_duration_ms, 600_000);
        }
        let result = ingest.start_recording(request(owner, workspace)).await;
        assert!(
            matches!(result, Err(StartRecordingError::LimitReached { max: 50 })),
            "{result:?}"
        );

        // Abandoned recordings don't count: one frees a slot.
        sqlx::query!(
            "UPDATE recordings SET state = 'abandoned'
             WHERE id = (SELECT id FROM recordings WHERE workspace_id = $1 LIMIT 1)",
            workspace.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("abandon one");
        ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("a slot was freed");

        // Another workspace has its own 50.
        let (other_owner, other_workspace) = owner_and_workspace(&pool).await;
        ingest
            .start_recording(request(other_owner, other_workspace))
            .await
            .expect("other workspace unaffected");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn concurrent_creates_cannot_exceed_the_limit(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);
        for _ in 0..48 {
            ingest
                .start_recording(request(owner, workspace))
                .await
                .expect("within the plan");
        }
        // Four requests race for the last two slots.
        let results = futures_join4(
            ingest.start_recording(request(owner, workspace)),
            ingest.start_recording(request(owner, workspace)),
            ingest.start_recording(request(owner, workspace)),
            ingest.start_recording(request(owner, workspace)),
        )
        .await;
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 2);
        let total = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM recordings WHERE workspace_id = $1"#,
            workspace.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(total, 50);
    }

    async fn futures_join4<T>(
        a: impl std::future::Future<Output = T>,
        b: impl std::future::Future<Output = T>,
        c: impl std::future::Future<Output = T>,
        d: impl std::future::Future<Output = T>,
    ) -> [T; 4] {
        let (a, b, c, d) = tokio::join!(a, b, c, d);
        [a, b, c, d]
    }

    /// Day 41's Check, part 2: an 11-minute take is rejected at finalize (free tier: 10 min).
    #[sqlx::test(migrations = "../../migrations")]
    async fn an_11_minute_take_is_rejected(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0]).await;
        let eleven_minutes = up
            .ingest
            .finalize(FinalizeTake {
                workspace_id: up.workspace,
                user_id: up.owner,
                take_id: up.started.take_id,
                chunk_count: 1,
                duration_ms: 11 * 60 * 1000,
            })
            .await;
        assert!(
            matches!(
                eleven_minutes,
                Err(FinalizeError::DurationExceeded { max_ms: 600_000 })
            ),
            "{eleven_minutes:?}"
        );
        assert!(!up.status().await.finalized);

        let ten_minutes = up
            .ingest
            .finalize(FinalizeTake {
                workspace_id: up.workspace,
                user_id: up.owner,
                take_id: up.started.take_id,
                chunk_count: 1,
                duration_ms: 10 * 60 * 1000,
            })
            .await;
        assert!(
            ten_minutes.is_ok(),
            "exactly the limit is allowed: {ten_minutes:?}"
        );
    }

    /// Ages every timestamp of the take (and its chunks) by `by`, as if it happened earlier.
    async fn age_take(pool: &PgPool, take: TakeId, by: time::Duration) {
        sqlx::query!(
            "UPDATE takes SET created_at = created_at - $2::interval WHERE id = $1",
            take.into_uuid(),
            sqlx::postgres::types::PgInterval::try_from(by).expect("interval"),
        )
        .execute(pool)
        .await
        .expect("age take");
        sqlx::query!(
            "UPDATE chunks SET received_at = received_at - $2::interval WHERE take_id = $1",
            take.into_uuid(),
            sqlx::postgres::types::PgInterval::try_from(by).expect("interval"),
        )
        .execute(pool)
        .await
        .expect("age chunks");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_sweep_abandons_idle_uploads_then_purges_their_chunks(pool: PgPool) {
        let store = Arc::new(FakeStore::default());
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service_with(&pool, store.clone());
        let idle = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("idle take");
        let active = ingest
            .start_recording(request(owner, workspace))
            .await
            .expect("active take");
        for take in [idle.take_id, active.take_id] {
            sqlx::query!(
                "INSERT INTO chunks (take_id, workspace_id, idx, size_bytes, sha256)
                 VALUES ($1, $2, 0, 10, $3)",
                take.into_uuid(),
                workspace.into_uuid(),
                &[0u8; 32][..],
            )
            .execute(&pool)
            .await
            .expect("chunk");
        }

        // 25 hours idle: abandoned, chunks kept.
        age_take(&pool, idle.take_id, time::Duration::hours(25)).await;
        let report = ingest.sweep_stale_uploads().await.expect("sweep");
        assert_eq!(
            report,
            SweepReport {
                abandoned: 1,
                purged_takes: 0
            }
        );
        let states = sqlx::query!(
            r#"SELECT id, state::text AS "state!" FROM recordings WHERE workspace_id = $1"#,
            workspace.into_uuid()
        )
        .fetch_all(&pool)
        .await
        .expect("states");
        for row in states {
            let expected = if row.id == idle.recording_id.into_uuid() {
                "abandoned"
            } else {
                "recording"
            };
            assert_eq!(row.state, expected);
        }
        // A second run finds nothing new.
        assert_eq!(
            ingest.sweep_stale_uploads().await.expect("sweep"),
            SweepReport::default()
        );

        // 8 days idle: its chunks go, in storage and in the database.
        age_take(&pool, idle.take_id, time::Duration::days(7)).await;
        let report = ingest.sweep_stale_uploads().await.expect("sweep");
        assert_eq!(
            report,
            SweepReport {
                abandoned: 0,
                purged_takes: 1
            }
        );
        assert_eq!(
            store.deleted.lock().expect("lock").as_slice(),
            [format!(
                "ws/{workspace}/rec/{}/takes/{}/chunks/",
                idle.recording_id, idle.take_id
            )]
        );
        let left = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM chunks WHERE take_id = $1"#,
            idle.take_id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(left, 0);
        let active_left = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM chunks WHERE take_id = $1"#,
            active.take_id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(active_left, 1, "the active upload is untouched");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_sweep_leaves_finalized_takes_alone(pool: PgPool) {
        let up = uploading(&pool).await;
        up.upload_and_ack(&[0]).await;
        up.finalize(1).await.expect("finalize");
        age_take(&pool, up.started.take_id, time::Duration::days(30)).await;
        assert_eq!(
            up.ingest.sweep_stale_uploads().await.expect("sweep"),
            SweepReport::default()
        );
    }
}
