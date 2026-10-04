use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};
use time::OffsetDateTime;

use crate::domain::{MimeType, Sources};

/// A take found by the stale-upload sweep.
pub struct StaleTake {
    pub take_id: TakeId,
    pub recording_id: RecordingId,
    pub workspace_id: WorkspaceId,
}

/// Unfinalized, not-yet-abandoned takes with no activity (take created, chunk acked) since
/// `cutoff`, oldest first; marks them abandoned in the same statement so the next run moves on.
pub async fn abandon_idle_takes(
    executor: impl PgExecutor<'_>,
    now: OffsetDateTime,
    cutoff: OffsetDateTime,
    limit: i64,
) -> Result<Vec<StaleTake>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        WITH idle AS (
            SELECT t.id, t.workspace_id
            FROM takes t
            WHERE t.finalized_at IS NULL
              AND t.abandoned_at IS NULL
              AND GREATEST(
                    t.created_at,
                    COALESCE(
                      (SELECT max(c.received_at) FROM chunks c
                       WHERE c.take_id = t.id AND c.workspace_id = t.workspace_id),
                      t.created_at)
                  ) < $1
            ORDER BY t.created_at
            LIMIT $2
            FOR UPDATE OF t SKIP LOCKED
        )
        UPDATE takes t
        SET abandoned_at = $3
        FROM idle
        WHERE t.id = idle.id AND t.workspace_id = idle.workspace_id
        RETURNING t.id, t.recording_id, t.workspace_id
        "#,
        cutoff,
        limit,
        now,
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| StaleTake {
            take_id: TakeId::from_uuid(row.id),
            recording_id: RecordingId::from_uuid(row.recording_id),
            workspace_id: WorkspaceId::from_uuid(row.workspace_id),
        })
        .collect())
}

/// Abandoned takes, idle since before `cutoff`, that still have chunks to delete.
pub async fn purgeable_takes(
    executor: impl PgExecutor<'_>,
    cutoff: OffsetDateTime,
    limit: i64,
) -> Result<Vec<StaleTake>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.recording_id, t.workspace_id
        FROM takes t
        WHERE t.finalized_at IS NULL
          AND t.abandoned_at IS NOT NULL
          AND EXISTS (SELECT 1 FROM chunks c WHERE c.take_id = t.id AND c.workspace_id = t.workspace_id)
          AND NOT EXISTS (
                SELECT 1 FROM chunks c
                WHERE c.take_id = t.id AND c.workspace_id = t.workspace_id AND c.received_at >= $1)
          AND t.created_at < $1
        ORDER BY t.created_at
        LIMIT $2
        "#,
        cutoff,
        limit,
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| StaleTake {
            take_id: TakeId::from_uuid(row.id),
            recording_id: RecordingId::from_uuid(row.recording_id),
            workspace_id: WorkspaceId::from_uuid(row.workspace_id),
        })
        .collect())
}

pub async fn delete_chunks(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM chunks WHERE take_id = $1 AND workspace_id = $2",
        take_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

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

/// A chunk the server has acknowledged, with its index.
pub struct IndexedReceipt {
    pub idx: i32,
    pub size_bytes: i32,
    pub sha256: Vec<u8>,
}

/// Every acknowledged chunk of a take, ascending by index.
pub async fn receipts(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
) -> Result<Vec<IndexedReceipt>, sqlx::Error> {
    sqlx::query_as!(
        IndexedReceipt,
        "SELECT idx, size_bytes, sha256 FROM chunks WHERE take_id = $1 AND workspace_id = $2 ORDER BY idx",
        take_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_all(executor)
    .await
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

/// Like [`find_take_for_owner`], but locks the take row until the transaction ends, so
/// concurrent finalizes of one take run one after another.
pub async fn lock_take_for_owner(
    conn: &mut PgConnection,
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
        FOR UPDATE OF t
        "#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        owner_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| UploadableTake {
        recording_id: RecordingId::from_uuid(row.recording_id),
        mime_type: row.mime_type,
        finalized: row.finalized,
    }))
}

/// The chunk indexes the server has acknowledged for a take, ascending.
pub async fn received_indexes(
    executor: impl PgExecutor<'_>,
    take_id: TakeId,
    workspace_id: WorkspaceId,
) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT idx FROM chunks WHERE take_id = $1 AND workspace_id = $2 ORDER BY idx",
        take_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_all(executor)
    .await
}

pub async fn mark_take_finalized(
    conn: &mut PgConnection,
    take_id: TakeId,
    workspace_id: WorkspaceId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE takes SET finalized_at = now() WHERE id = $1 AND workspace_id = $2",
        take_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}
