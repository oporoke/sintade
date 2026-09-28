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
