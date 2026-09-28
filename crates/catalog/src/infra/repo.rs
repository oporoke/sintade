use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::PgConnection;

pub async fn insert_recording(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    owner_id: UserId,
    title: &str,
    current_take: TakeId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO recordings (id, workspace_id, owner_id, title, current_take)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        owner_id.into_uuid(),
        title,
        current_take.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Moves an uploading recording to `processing` and records its duration. Returns `false` if
/// the recording isn't in `recording`/`uploading` (e.g. abandoned) or isn't in the workspace.
pub async fn mark_processing(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    duration_ms: i32,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE recordings
        SET state = 'processing', duration_ms = $3, updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND state IN ('recording', 'uploading')
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        duration_ms,
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
