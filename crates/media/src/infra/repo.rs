use kernel::{RecordingId, TakeId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};
use time::OffsetDateTime;

/// A `media_jobs` row: what to process, and how far it got.
pub struct MediaJobRow {
    pub take_id: TakeId,
    pub workspace_id: WorkspaceId,
    pub recording_id: RecordingId,
    pub mime_type: String,
    pub duration_ms: i32,
    pub chunks: serde_json::Value,
    pub attempts: i32,
}

/// Records a take to process. `false` if it was recorded before (a replayed `TakeFinalized`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_job(
    conn: &mut PgConnection,
    take_id: TakeId,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    mime_type: &str,
    duration_ms: i32,
    chunks: serde_json::Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        INSERT INTO media_jobs (take_id, workspace_id, recording_id, mime_type, duration_ms, chunks)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (take_id) DO NOTHING
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        mime_type,
        duration_ms,
        chunks,
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn find_job(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
) -> Result<Option<MediaJobRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT take_id, workspace_id, recording_id, mime_type, duration_ms, chunks, attempts
        FROM media_jobs
        WHERE take_id = $1 AND workspace_id = $2
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|row| MediaJobRow {
        take_id: TakeId::from_uuid(row.take_id),
        workspace_id: WorkspaceId::from_uuid(row.workspace_id),
        recording_id: RecordingId::from_uuid(row.recording_id),
        mime_type: row.mime_type,
        duration_ms: row.duration_ms,
        chunks: row.chunks,
        attempts: row.attempts,
    }))
}

/// `queued`/`running` → `running`, counting the attempt. `false` if the job is already
/// finished (`done`/`failed`), so a stray duplicate job does nothing.
pub async fn mark_running(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
    now: OffsetDateTime,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE media_jobs
        SET state = 'running', attempts = attempts + 1, started_at = $3
        WHERE take_id = $1 AND workspace_id = $2 AND state IN ('queued', 'running')
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        now,
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}
