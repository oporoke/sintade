use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};

use crate::domain::{MimeType, Sources};

pub async fn insert_take(
    conn: &mut PgConnection,
    id: TakeId,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    mime_type: &MimeType,
    sources: Sources,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio, has_mic, has_camera)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        mime_type.as_str(),
        sources.system_audio,
        sources.mic,
        sources.camera,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// What an upload call needs to know about a take.
pub struct UploadableTake {
    pub recording_id: RecordingId,
    pub mime_type: String,
    pub finalized: bool,
}

/// The take, if it is in `workspace_id` and its recording belongs to `owner_id`. Anything else
/// is `None`, so a stranger can't tell a missing take from someone else's.
pub async fn find_take_for_owner(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
    owner_id: UserId,
) -> Result<Option<UploadableTake>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT t.recording_id, t.mime_type, t.finalized_at IS NOT NULL AS "finalized!"
        FROM takes t
        JOIN recordings r ON r.id = t.recording_id AND r.workspace_id = t.workspace_id
        WHERE t.id = $1 AND t.workspace_id = $2 AND r.owner_id = $3
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        owner_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|row| UploadableTake {
        recording_id: RecordingId::from_uuid(row.recording_id),
        mime_type: row.mime_type,
        finalized: row.finalized,
    }))
}

/// A chunk the server has acknowledged.
pub struct ChunkReceipt {
    pub size_bytes: i32,
    pub sha256: Vec<u8>,
}

pub async fn find_chunk(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
    idx: i32,
) -> Result<Option<ChunkReceipt>, sqlx::Error> {
    sqlx::query_as!(
        ChunkReceipt,
        "SELECT size_bytes, sha256 FROM chunks WHERE take_id = $1 AND workspace_id = $2 AND idx = $3",
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        idx,
    )
    .fetch_optional(executor)
    .await
}

/// Records a chunk receipt. Returns `false` if `(take_id, idx)` was already recorded (a
/// concurrent ack won the race); the caller re-reads and compares.
pub async fn insert_chunk(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
    idx: i32,
    size_bytes: i32,
    sha256: &[u8],
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        INSERT INTO chunks (take_id, workspace_id, idx, size_bytes, sha256)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (take_id, idx) DO NOTHING
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        idx,
        size_bytes,
        sha256,
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}
