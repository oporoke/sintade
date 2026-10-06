use super::service::{BUILD_HLS, BuildHls};
use kernel::{RecordingId, TakeId, WorkspaceId};
use platform::JobQueue;
use sqlx::PgPool;

/// The original recording file, playable by most browsers while the MP4 is still being made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKey {
    pub key: String,
    /// `video/webm` (Chrome, Firefox) or `video/mp4` (Safari).
    pub content_type: String,
}

/// The storage keys a viewer can be given for a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackKeys {
    /// The fast-start MP4, once it exists.
    pub mp4: Option<String>,
    /// The concatenated original, once it exists.
    pub source: Option<SourceKey>,
    pub poster: Option<String>,
}

/// Read-only lookups of a recording's renditions. Separate from `MediaService` for the same
/// reason as `RetryService`: the API needs no ffmpeg, scratch space or storage handle.
pub struct RenditionReader {
    pool: PgPool,
}

impl RenditionReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The poster key of each of the given recordings that has one.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, recordings = recording_ids.len()))]
    pub async fn poster_keys(
        &self,
        workspace_id: WorkspaceId,
        recording_ids: &[RecordingId],
    ) -> Result<Vec<(RecordingId, String)>, sqlx::Error> {
        let ids: Vec<uuid::Uuid> = recording_ids.iter().map(|id| id.into_uuid()).collect();
        let rows = sqlx::query!(
            r#"
            SELECT recording_id, storage_key
            FROM renditions
            WHERE workspace_id = $1 AND recording_id = ANY($2)
              AND kind = 'thumbnail' AND variant = 'poster'
            "#,
            workspace_id.into_uuid(),
            &ids,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (RecordingId::from_uuid(row.recording_id), row.storage_key))
            .collect())
    }

    /// The default MP4, the original and the poster of a recording; `None` while there is
    /// nothing to play yet.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn playback_keys(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<PlaybackKeys>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT kind::text AS "kind!", storage_key, meta
            FROM renditions
            WHERE workspace_id = $1 AND recording_id = $2
              AND ((kind = 'mp4' AND variant = 'default')
                OR (kind = 'source' AND variant = 'default')
                OR (kind = 'thumbnail' AND variant = 'poster'))
            "#,
            workspace_id.into_uuid(),
            recording_id.into_uuid(),
        )
        .fetch_all(&self.pool)
        .await?;
        let find = |kind: &str| rows.iter().find(|row| row.kind == kind);
        let mp4 = find("mp4").map(|row| row.storage_key.clone());
        let source = find("source").map(|row| SourceKey {
            key: row.storage_key.clone(),
            content_type: row
                .meta
                .get("content_type")
                .and_then(|value| value.as_str())
                .unwrap_or("video/webm")
                .to_string(),
        });
        if mp4.is_none() && source.is_none() {
            return Ok(None);
        }
        Ok(Some(PlaybackKeys {
            mp4,
            source,
            poster: find("thumbnail").map(|row| row.storage_key.clone()),
        }))
    }

    /// Which renditions the recording has, as `(kind, variant)`: what live status reports.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn available(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT kind::text AS "kind!", variant
            FROM renditions
            WHERE workspace_id = $1 AND recording_id = $2
            "#,
            workspace_id.into_uuid(),
            recording_id.into_uuid(),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.kind, row.variant))
            .collect())
    }

    /// The first view of a recording asks for its HLS ladder (docs/design.md §14: rungs only for
    /// recordings viewed at least once). Enqueues `BuildHls` for the recording's processed take
    /// unless it has a ladder already or one is queued. Returns whether a job was enqueued.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn request_ladder(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<bool, RequestLadderError> {
        let take = sqlx::query_scalar!(
            r#"
            SELECT take_id FROM media_jobs
            WHERE workspace_id = $1 AND recording_id = $2 AND state = 'done'
            ORDER BY created_at DESC LIMIT 1
            "#,
            workspace_id.into_uuid(),
            recording_id.into_uuid(),
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(take) = take else {
            return Ok(false);
        };
        let built = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM renditions
                WHERE workspace_id = $1 AND recording_id = $2 AND take_id = $3
                  AND kind = 'hls' AND variant = 'master'
            ) AS "built!"
            "#,
            workspace_id.into_uuid(),
            recording_id.into_uuid(),
            take,
        )
        .fetch_one(&self.pool)
        .await?;
        if built {
            return Ok(false);
        }
        let payload = serde_json::to_value(BuildHls {
            take_id: TakeId::from_uuid(take),
            workspace_id,
        })?;
        let queued = JobQueue::new(self.pool.clone())
            .enqueue_unless_identical_pending(BUILD_HLS, payload)
            .await?;
        Ok(queued.is_some())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RequestLadderError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Queue(#[from] platform::JobQueueError),

    #[error("could not encode the job: {0}")]
    Encode(#[from] serde_json::Error),
}
