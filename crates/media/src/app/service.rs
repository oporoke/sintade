use std::sync::Arc;

use kernel::{RecordingId, TakeId, WorkspaceId};
use platform::{Clock, JobQueue, JobQueueError, ObjectStore, StorageError};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::domain::probe::{ProbeRejection, SourceInfo, validate};
use crate::domain::{ChunkManifest, Container, ManifestChunk, ManifestError, source_key};
use crate::infra;
use crate::infra::scratch::{ScratchDir, ScratchSpace};
use crate::infra::source::{AssembleError, assemble_source};
use crate::infra::tools::{MediaTools, Probed, ToolError, probe};

/// The job kind that turns a finalized take into renditions (docs/design.md §10 Process).
pub const PROCESS_TAKE: &str = "ProcessTake";

/// `TakeFinalized` as media reads it from the outbox envelope (ingest's contract, ADR-0012).
#[derive(Debug, Clone, Deserialize)]
pub struct TakeFinalizedMessage {
    pub workspace_id: WorkspaceId,
    pub data: TakeFinalizedData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TakeFinalizedData {
    pub take_id: TakeId,
    pub recording_id: RecordingId,
    pub chunk_count: u32,
    pub duration_ms: u32,
    pub mime: String,
    pub chunks: Vec<ManifestChunk>,
}

/// The `ProcessTake` job's payload: which take; the rest is in `media_jobs`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessTake {
    pub take_id: TakeId,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, thiserror::Error)]
pub enum EnqueueError {
    #[error("invalid chunk manifest: {0}")]
    InvalidManifest(#[from] ManifestError),

    #[error("duration_ms must be at most {}", i32::MAX)]
    InvalidDuration,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Queue(#[from] JobQueueError),

    #[error("could not encode the manifest: {0}")]
    Encode(#[from] serde_json::Error),
}

/// How a `ProcessTake` run ended, when it didn't fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessOutcome {
    /// The take was processed.
    Ran,
    /// The take was already processed or failed for good (a duplicate job).
    AlreadyFinished,
    /// No such take to process (the recording was deleted meanwhile).
    NotFound,
    /// The take's input can't be processed (a corrupt or missing chunk, an unplayable file);
    /// it failed for good and the job is finished.
    Rejected,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("the stored manifest can't be read: {0}")]
    StoredManifest(#[from] serde_json::Error),

    #[error("the stored manifest is invalid: {0}")]
    InvalidManifest(#[from] ManifestError),

    #[error("scratch disk: {0}")]
    Scratch(#[from] std::io::Error),

    #[error("assembling the source: {0}")]
    Assemble(AssembleError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Input the pipeline can't use: the take fails for good (ADR-0012 §7).
#[derive(Debug)]
struct Rejection(String);

/// A pipeline step's failure: retry it, or give up on the take.
enum StepError {
    Retry(ProcessError),
    Reject(Rejection),
}

impl From<AssembleError> for StepError {
    fn from(error: AssembleError) -> Self {
        if error.is_permanent() {
            Self::Reject(Rejection(error.to_string()))
        } else {
            Self::Retry(ProcessError::Assemble(error))
        }
    }
}

impl From<ProbeRejection> for StepError {
    fn from(rejection: ProbeRejection) -> Self {
        Self::Reject(Rejection(rejection.to_string()))
    }
}

impl<E: Into<ProcessError>> From<E> for StepError {
    fn from(error: E) -> Self {
        Self::Retry(error.into())
    }
}

pub struct MediaService {
    pool: PgPool,
    store: Arc<dyn ObjectStore>,
    clock: Arc<dyn Clock>,
    scratch: ScratchSpace,
    tools: MediaTools,
}

impl MediaService {
    pub fn new(
        pool: PgPool,
        store: Arc<dyn ObjectStore>,
        clock: Arc<dyn Clock>,
        scratch: ScratchSpace,
        tools: MediaTools,
    ) -> Self {
        Self {
            pool,
            store,
            clock,
            scratch,
            tools,
        }
    }

    pub fn scratch(&self) -> &ScratchSpace {
        &self.scratch
    }

    /// Subscribed to `TakeFinalized`: records the take's manifest and enqueues `ProcessTake`,
    /// in one transaction. Idempotent: a replayed event finds the take recorded and enqueues
    /// nothing. Returns whether a job was enqueued.
    #[tracing::instrument(skip_all, fields(take_id = %message.data.take_id, workspace_id = %message.workspace_id))]
    pub async fn enqueue_processing(
        &self,
        message: TakeFinalizedMessage,
    ) -> Result<bool, EnqueueError> {
        let data = message.data;
        let manifest = ChunkManifest::new(data.chunks, data.chunk_count)?;
        let duration_ms =
            i32::try_from(data.duration_ms).map_err(|_| EnqueueError::InvalidDuration)?;
        let chunks = serde_json::to_value(manifest.chunks())?;

        let mut tx = self.pool.begin().await?;
        let inserted = infra::insert_job(
            &mut tx,
            data.take_id,
            message.workspace_id,
            data.recording_id,
            &data.mime,
            duration_ms,
            chunks,
        )
        .await?;
        if inserted {
            let payload = serde_json::to_value(ProcessTake {
                take_id: data.take_id,
                workspace_id: message.workspace_id,
            })?;
            JobQueue::enqueue_in(&mut tx, PROCESS_TAKE, payload).await?;
        }
        tx.commit().await?;
        if inserted {
            tracing::info!(chunks = manifest.len(), "ProcessTake enqueued");
        }
        Ok(inserted)
    }

    /// Runs `ProcessTake` for one take in a scratch directory that is removed afterwards,
    /// whatever happens.
    #[tracing::instrument(skip_all, fields(take_id = %request.take_id, workspace_id = %request.workspace_id))]
    pub async fn process_take(&self, request: ProcessTake) -> Result<ProcessOutcome, ProcessError> {
        let Some(job) = infra::find_job(&self.pool, request.take_id, request.workspace_id).await?
        else {
            return Ok(ProcessOutcome::NotFound);
        };
        if !infra::mark_running(&self.pool, job.take_id, job.workspace_id, self.clock.now()).await?
        {
            return Ok(ProcessOutcome::AlreadyFinished);
        }

        let chunks: Vec<ManifestChunk> = serde_json::from_value(job.chunks.clone())?;
        let manifest = ChunkManifest::new(
            chunks.clone(),
            u32::try_from(chunks.len()).unwrap_or(u32::MAX),
        )?;

        let scratch = self.scratch.create(job.take_id).await?;
        tracing::info!(
            recording_id = %job.recording_id,
            attempt = job.attempts + 1,
            chunks = manifest.len(),
            bytes = manifest.total_bytes(),
            mime = %job.mime_type,
            duration_ms = job.duration_ms,
            scratch = %scratch.path().display(),
            "ProcessTake started"
        );
        let result = self.run_pipeline(&job, &manifest, &scratch).await;
        scratch.remove().await?;
        match result {
            Ok(()) => Ok(ProcessOutcome::Ran),
            Err(StepError::Retry(error)) => Err(error),
            Err(StepError::Reject(Rejection(reason))) => {
                tracing::warn!(%reason, "ProcessTake rejected the take");
                infra::mark_rejected(
                    &self.pool,
                    job.take_id,
                    job.workspace_id,
                    &reason,
                    self.clock.now(),
                )
                .await?;
                Ok(ProcessOutcome::Rejected)
            }
        }
    }

    /// docs/design.md §10 Process. Steps 1–3 so far: fetch every chunk, verify it, concatenate
    /// into the source file and store it (kept even if rejected, for support), then validate it
    /// with ffprobe. Renditions follow (Days 46–47).
    async fn run_pipeline(
        &self,
        job: &infra::MediaJobRow,
        manifest: &ChunkManifest,
        scratch: &ScratchDir,
    ) -> Result<(), StepError> {
        let container = Container::from_mime(&job.mime_type);
        let source = scratch.file(&format!("source.{}", container.extension()));
        let bytes = assemble_source(self.store.as_ref(), manifest, &source).await?;
        let key = source_key(job.workspace_id, job.recording_id, job.take_id, container);
        self.store
            .put_file(&key, &source, container.content_type())
            .await?;
        tracing::info!(bytes, key = %key, "source assembled and stored");

        let info = self.validate_source(&source, container).await?;
        tracing::info!(
            video = %info.video_codec,
            audio = info.audio_codec.as_deref().unwrap_or("none"),
            width = info.width,
            height = info.height,
            remux = info.is_remuxable(),
            "source validated"
        );
        Ok(())
    }

    /// §10 Process step 3: ffprobe, then the acceptance rules (`domain::probe::validate`).
    async fn validate_source(
        &self,
        source: &std::path::Path,
        container: Container,
    ) -> Result<SourceInfo, StepError> {
        match probe(&self.tools, source).await? {
            Probed::Unreadable { detail } => {
                tracing::warn!(%detail, "ffprobe can't read the source");
                Err(ProbeRejection::Unreadable.into())
            }
            Probed::Readable(report) => Ok(validate(&report, container)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryStore;

    const HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// A user, their workspace, a recording in `processing` and its finalized take, seeded
    /// directly (media's tests don't go through ingest).
    pub(crate) struct Seeded {
        pub workspace: WorkspaceId,
        pub recording: RecordingId,
        pub take: TakeId,
    }

    pub(crate) async fn seed(pool: &PgPool) -> Seeded {
        let user = uuid::Uuid::now_v7();
        let workspace = WorkspaceId::new_v7();
        let recording = RecordingId::new_v7();
        let take = TakeId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Media Test')",
            user,
            format!("media-{user}@example.com"),
        )
        .execute(pool)
        .await
        .expect("user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name) VALUES ($1, 'Media Test')",
            workspace.into_uuid()
        )
        .execute(pool)
        .await
        .expect("workspace");
        sqlx::query!(
            "INSERT INTO recordings (id, workspace_id, owner_id, title, state, current_take)
             VALUES ($1, $2, $3, 'Test', 'processing', $4)",
            recording.into_uuid(),
            workspace.into_uuid(),
            user,
            take.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("recording");
        sqlx::query!(
            "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                                has_mic, has_camera, finalized_at)
             VALUES ($1, $2, $3, 'video/webm;codecs=vp9,opus', false, true, false, now())",
            take.into_uuid(),
            workspace.into_uuid(),
            recording.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("take");
        Seeded {
            workspace,
            recording,
            take,
        }
    }

    pub(crate) fn message(seeded: &Seeded, chunk_count: u32) -> TakeFinalizedMessage {
        TakeFinalizedMessage {
            workspace_id: seeded.workspace,
            data: TakeFinalizedData {
                take_id: seeded.take,
                recording_id: seeded.recording,
                chunk_count,
                duration_ms: 6_000,
                mime: "video/webm;codecs=vp9,opus".to_string(),
                chunks: (0..chunk_count)
                    .map(|idx| ManifestChunk {
                        idx,
                        key: format!(
                            "ws/{}/rec/{}/takes/{}/chunks/{idx:06}.webm",
                            seeded.workspace, seeded.recording, seeded.take
                        ),
                        size_bytes: 1000,
                        sha256: HASH.to_string(),
                    })
                    .collect(),
            },
        }
    }

    fn service(pool: &PgPool) -> MediaService {
        service_with(pool).0
    }

    fn service_with(pool: &PgPool) -> (MediaService, Arc<MemoryStore>) {
        let root =
            std::env::temp_dir().join(format!("sintade-media-test-{}", uuid::Uuid::now_v7()));
        let store = Arc::new(MemoryStore::default());
        let media = MediaService::new(
            pool.clone(),
            store.clone(),
            Arc::new(platform::SystemClock),
            ScratchSpace::new(root),
            MediaTools::default(),
        );
        (media, store)
    }

    /// Splits `bytes` into chunks of `chunk_size`, stores them where ingest would, and
    /// returns the `TakeFinalized` that lists them.
    fn upload_chunks(
        store: &MemoryStore,
        seeded: &Seeded,
        bytes: &[u8],
        chunk_size: usize,
    ) -> TakeFinalizedMessage {
        let parts: Vec<&[u8]> = bytes.chunks(chunk_size).collect();
        let mut message = message(seeded, u32::try_from(parts.len()).expect("few chunks"));
        for (chunk, part) in message.data.chunks.iter_mut().zip(parts) {
            store.insert(&chunk.key, part.to_vec());
            chunk.size_bytes = u32::try_from(part.len()).expect("small chunk");
            chunk.sha256 = crate::testing::sha256_hex(part);
        }
        message
    }

    async fn run(media: &MediaService, seeded: &Seeded) -> ProcessOutcome {
        media
            .process_take(ProcessTake {
                take_id: seeded.take,
                workspace_id: seeded.workspace,
            })
            .await
            .expect("process")
    }

    async fn job_state(pool: &PgPool, take: TakeId) -> (String, Option<String>) {
        let row = sqlx::query!(
            r#"SELECT state::text AS "state!", last_error FROM media_jobs WHERE take_id = $1"#,
            take.into_uuid()
        )
        .fetch_one(pool)
        .await
        .expect("media job");
        (row.state, row.last_error)
    }

    async fn scratch_is_empty(media: &MediaService) -> bool {
        let mut entries = tokio::fs::read_dir(media.scratch().root())
            .await
            .expect("scratch root");
        let empty = entries.next_entry().await.expect("read").is_none();
        tokio::fs::remove_dir_all(media.scratch().root())
            .await
            .expect("tidy");
        empty
    }

    fn source_of(seeded: &Seeded) -> String {
        format!(
            "ws/{}/rec/{}/takes/{}/source.webm",
            seeded.workspace, seeded.recording, seeded.take
        )
    }

    async fn process_take_jobs(pool: &PgPool) -> Vec<serde_json::Value> {
        sqlx::query_scalar!("SELECT payload FROM jobs WHERE kind = 'ProcessTake'")
            .fetch_all(pool)
            .await
            .expect("jobs")
    }

    /// Day 43's Check, part 1: finalize (its `TakeFinalized`) enqueues `ProcessTake`, once.
    #[sqlx::test(migrations = "../../migrations")]
    async fn take_finalized_enqueues_process_take_once(pool: PgPool) {
        let seeded = seed(&pool).await;
        let media = service(&pool);

        assert!(
            media
                .enqueue_processing(message(&seeded, 3))
                .await
                .expect("enqueue")
        );
        // The relay may deliver an event twice; the second time is a no-op.
        assert!(
            !media
                .enqueue_processing(message(&seeded, 3))
                .await
                .expect("replay")
        );

        let jobs = process_take_jobs(&pool).await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["take_id"], seeded.take.to_string());
        assert_eq!(jobs[0]["workspace_id"], seeded.workspace.to_string());
        let stored = sqlx::query!(
            r#"SELECT state::text AS "state!", jsonb_array_length(chunks) AS "chunks!", duration_ms
               FROM media_jobs WHERE take_id = $1"#,
            seeded.take.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("media job");
        assert_eq!(
            (stored.state.as_str(), stored.chunks, stored.duration_ms),
            ("queued", 3, 6_000)
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_invalid_manifest_enqueues_nothing(pool: PgPool) {
        let seeded = seed(&pool).await;
        let mut gap = message(&seeded, 3);
        gap.data.chunks.remove(1);
        gap.data.chunk_count = 3;
        let result = service(&pool).enqueue_processing(gap).await;
        assert!(
            matches!(result, Err(EnqueueError::InvalidManifest(_))),
            "{result:?}"
        );
        assert!(process_take_jobs(&pool).await.is_empty());
    }

    /// Day 43's Check, part 2: the job starts (the take is `running`, attempt counted) and its
    /// scratch directory is gone afterwards.
    #[sqlx::test(migrations = "../../migrations")]
    async fn process_take_starts_and_cleans_its_scratch_dir(pool: PgPool) {
        if !crate::testing::ffmpeg_available().await {
            return;
        }
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let webm = crate::testing::fixture("real_chrome_vp9_opus_10s.webm").await;
        media
            .enqueue_processing(upload_chunks(&store, &seeded, &webm, 64 * 1024))
            .await
            .expect("enqueue");

        let request = ProcessTake {
            take_id: seeded.take,
            workspace_id: seeded.workspace,
        };
        assert_eq!(
            media.process_take(request).await.expect("run"),
            ProcessOutcome::Ran
        );
        let row = sqlx::query!(
            r#"SELECT state::text AS "state!", attempts, started_at FROM media_jobs WHERE take_id = $1"#,
            seeded.take.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("media job");
        assert_eq!((row.state.as_str(), row.attempts), ("running", 1));
        assert!(row.started_at.is_some());

        let mut left = tokio::fs::read_dir(media.scratch().root())
            .await
            .expect("scratch root exists");
        assert!(
            left.next_entry().await.expect("read").is_none(),
            "the job's scratch dir was removed"
        );
        tokio::fs::remove_dir_all(media.scratch().root())
            .await
            .expect("tidy");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn process_take_is_scoped_to_the_workspace(pool: PgPool) {
        let seeded = seed(&pool).await;
        let media = service(&pool);
        media
            .enqueue_processing(message(&seeded, 1))
            .await
            .expect("enqueue");
        let elsewhere = ProcessTake {
            take_id: seeded.take,
            workspace_id: WorkspaceId::new_v7(),
        };
        assert_eq!(
            media.process_take(elsewhere).await.expect("run"),
            ProcessOutcome::NotFound
        );
    }

    /// Day 44: the chunks, verified, concatenate into the stored source, byte for byte.
    #[sqlx::test(migrations = "../../migrations")]
    async fn chunks_are_verified_and_concatenated_into_the_stored_source(pool: PgPool) {
        if !crate::testing::ffmpeg_available().await {
            return;
        }
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let original = crate::testing::fixture("real_firefox_vp8_opus_10s.webm").await;
        media
            .enqueue_processing(upload_chunks(&store, &seeded, &original, 100 * 1024))
            .await
            .expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Ran);
        assert_eq!(store.bytes(&source_of(&seeded)), Some(original));
        assert_eq!(
            store.content_type(&source_of(&seeded)).as_deref(),
            Some("video/webm")
        );
        assert!(scratch_is_empty(&media).await);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_chunk_that_does_not_match_its_hash_rejects_the_take(pool: PgPool) {
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let message = upload_chunks(&store, &seeded, &[1u8; 5_000], 2_000);
        // Storage holds different bytes (same size) than the client hashed for chunk 1.
        store.insert(&message.data.chunks[1].key, vec![2u8; 2_000]);
        media.enqueue_processing(message).await.expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Rejected);
        assert_eq!(
            job_state(&pool, seeded.take).await,
            (
                "failed".to_string(),
                Some("chunk 1 doesn't match its recorded SHA-256".to_string())
            )
        );
        assert_eq!(store.bytes(&source_of(&seeded)), None, "nothing stored");
        assert!(
            scratch_is_empty(&media).await,
            "cleaned up after a failure too"
        );
        // A duplicate job finds it finished.
        assert_eq!(run(&media, &seeded).await, ProcessOutcome::AlreadyFinished);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_missing_chunk_rejects_the_take(pool: PgPool) {
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let message = upload_chunks(&store, &seeded, &[1u8; 5_000], 2_000);
        store.remove(&message.data.chunks[2].key);
        media.enqueue_processing(message).await.expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Rejected);
        assert_eq!(
            job_state(&pool, seeded.take).await.1.as_deref(),
            Some("chunk 2 is missing from storage")
        );
        assert!(scratch_is_empty(&media).await);
    }

    /// Day 44's Check: a real WebM, cut into chunks like a recorder does, comes back as a
    /// source file that matches every chunk hash and decodes from start to end.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_real_webm_reassembles_into_a_playable_source(pool: PgPool) {
        let Some(webm) = crate::testing::generated_webm(4).await else {
            return; // no FFmpeg on this machine (CI requires it: MEDIA_REQUIRE_FFMPEG)
        };
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let original = tokio::fs::read(&webm).await.expect("read fixture");
        media
            .enqueue_processing(upload_chunks(&store, &seeded, &original, 16 * 1024))
            .await
            .expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Ran);
        let source = store.bytes(&source_of(&seeded)).expect("source stored");
        assert_eq!(source, original);
        let reassembled = webm.with_extension("reassembled.webm");
        tokio::fs::write(&reassembled, &source)
            .await
            .expect("write");
        assert!(
            crate::testing::decodes_fully(&reassembled).await,
            "the reassembled source plays to the end"
        );
        assert!(scratch_is_empty(&media).await);
    }

    /// Day 45's Check: bytes that only claim to be a WebM are refused with a clear reason,
    /// and the job ends (no retries).
    #[sqlx::test(migrations = "../../migrations")]
    async fn not_a_video_is_rejected_cleanly(pool: PgPool) {
        if !crate::testing::ffmpeg_available().await {
            return;
        }
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let bytes = crate::testing::fixture("not_a_video.webm").await;
        media
            .enqueue_processing(upload_chunks(&store, &seeded, &bytes, 1_000))
            .await
            .expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Rejected);
        assert_eq!(
            job_state(&pool, seeded.take).await,
            (
                "failed".to_string(),
                Some("the recording isn't a readable video file".to_string())
            )
        );
        // Kept for support to look at.
        assert_eq!(store.bytes(&source_of(&seeded)), Some(bytes));
        assert!(scratch_is_empty(&media).await);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_take_declared_as_webm_but_holding_mp4_is_rejected(pool: PgPool) {
        if !crate::testing::ffmpeg_available().await {
            return;
        }
        let seeded = seed(&pool).await;
        let (media, store) = service_with(&pool);
        let mp4 = crate::testing::fixture("h264_aac_safari_20s.mp4").await;
        media
            .enqueue_processing(upload_chunks(&store, &seeded, &mp4, 256 * 1024))
            .await
            .expect("enqueue");

        assert_eq!(run(&media, &seeded).await, ProcessOutcome::Rejected);
        let reason = job_state(&pool, seeded.take).await.1.expect("reason");
        assert!(
            reason.starts_with("the recording is in an unexpected format"),
            "{reason}"
        );
        assert!(scratch_is_empty(&media).await);
    }
}
