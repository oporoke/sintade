use kernel::{RecordingId, TakeId, WorkspaceId};
use sqlx::PgConnection;

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
