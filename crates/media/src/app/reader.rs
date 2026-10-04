use kernel::{RecordingId, WorkspaceId};
use sqlx::PgPool;

/// The storage keys a viewer can be given for a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackKeys {
    pub mp4: String,
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

    /// The default MP4 and poster of a recording, or `None` while there is no MP4.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn playback_keys(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<PlaybackKeys>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT kind::text AS "kind!", storage_key
            FROM renditions
            WHERE workspace_id = $1 AND recording_id = $2
              AND ((kind = 'mp4' AND variant = 'default') OR (kind = 'thumbnail' AND variant = 'poster'))
            "#,
            workspace_id.into_uuid(),
            recording_id.into_uuid(),
        )
        .fetch_all(&self.pool)
        .await?;
        let key = |kind: &str| {
            rows.iter()
                .find(|row| row.kind == kind)
                .map(|row| row.storage_key.clone())
        };
        Ok(key("mp4").map(|mp4| PlaybackKeys {
            mp4,
            poster: key("thumbnail"),
        }))
    }
}
