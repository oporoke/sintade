//! `edits` queries (this crate's table only; docs/design.md §8).

use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

/// What an edit needs to be submitted: the take whose MP4 it cuts and the original's length.
pub struct SourceMp4 {
    pub take_id: TakeId,
    pub duration_ms: u32,
}

/// The recording's default MP4, if it is made, with the **original's** length: once an edit is
/// applied the MP4 row describes the edit, but ranges are always in original time, so the length
/// comes from the first edit, which was checked against the original (ADR-0038). `None` also when
/// the recording isn't in the workspace or is in the trash.
pub async fn source_mp4(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Result<Option<SourceMp4>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT r.take_id,
               COALESCE(
                   (SELECT e.source_duration_ms FROM edits e
                    WHERE e.workspace_id = r.workspace_id AND e.recording_id = r.recording_id
                    ORDER BY e.version LIMIT 1),
                   (r.meta ->> 'duration_ms')::int
               ) AS duration_ms
        FROM renditions r
        JOIN recordings rec ON rec.id = r.recording_id AND rec.workspace_id = r.workspace_id
        WHERE r.workspace_id = $1 AND r.recording_id = $2
          AND r.kind = 'mp4' AND r.variant = 'default'
          AND rec.trashed_at IS NULL
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.and_then(|row| {
        Some(SourceMp4 {
            take_id: TakeId::from_uuid(row.take_id),
            duration_ms: u32::try_from(row.duration_ms?).ok()?,
        })
    }))
}

/// Locks the recording row so two edits submitted together get different versions. `false` if
/// there is no such recording in the workspace.
pub async fn lock_recording(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_scalar!(
        "SELECT id FROM recordings WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
        recording_id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some())
}

pub async fn next_version(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Result<u32, sqlx::Error> {
    let last = sqlx::query_scalar!(
        r#"SELECT max(version) FROM edits WHERE workspace_id = $1 AND recording_id = $2"#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(u32::try_from(last.unwrap_or(0)).unwrap_or(0) + 1)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_edit(
    conn: &mut PgConnection,
    id: Uuid,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    take_id: TakeId,
    version: u32,
    edl: &serde_json::Value,
    source_duration_ms: u32,
    created_by: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO edits (id, workspace_id, recording_id, take_id, version, edl,
                           source_duration_ms, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        id,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        take_id.into_uuid(),
        i32::try_from(version).unwrap_or(i32::MAX),
        edl,
        i32::try_from(source_duration_ms).unwrap_or(i32::MAX),
        created_by.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// A stored edit, as rendering reads it.
pub struct EditRow {
    pub id: Uuid,
    pub workspace_id: WorkspaceId,
    pub recording_id: RecordingId,
    pub take_id: TakeId,
    pub version: u32,
    pub edl: serde_json::Value,
    pub source_duration_ms: u32,
    pub status: String,
    /// Where the rendered MP4 is, once it is.
    pub output_key: Option<String>,
}

pub async fn find_edit(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
) -> Result<Option<EditRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, recording_id, take_id AS "take_id!", version, edl,
               source_duration_ms, status, output_key
        FROM edits
        WHERE id = $1 AND workspace_id = $2 AND take_id IS NOT NULL
        "#,
        id,
        workspace_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|row| EditRow {
        id: row.id,
        workspace_id: WorkspaceId::from_uuid(row.workspace_id),
        recording_id: RecordingId::from_uuid(row.recording_id),
        take_id: TakeId::from_uuid(row.take_id),
        version: u32::try_from(row.version).unwrap_or(0),
        edl: row.edl,
        source_duration_ms: u32::try_from(row.source_duration_ms).unwrap_or(0),
        status: row.status,
        output_key: row.output_key,
    }))
}

/// `queued`/`rendering` → `rendering`. `false` once it is rendered or finished (a duplicate
/// job), so nothing is rendered twice.
pub async fn mark_rendering(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE edits SET status = 'rendering', failure = NULL
        WHERE id = $1 AND workspace_id = $2 AND status IN ('queued', 'rendering')
        "#,
        id,
        workspace_id.into_uuid(),
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_rendered(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
    output_key: &str,
    output_duration_ms: u32,
    output_bytes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE edits
        SET status = 'rendered', output_key = $3, output_duration_ms = $4, output_bytes = $5,
            rendered_at = now(), failure = NULL
        WHERE id = $1 AND workspace_id = $2
        "#,
        id,
        workspace_id.into_uuid(),
        output_key,
        i32::try_from(output_duration_ms).unwrap_or(i32::MAX),
        output_bytes,
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn mark_failed(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE edits SET status = 'failed', failure = $3 WHERE id = $1 AND workspace_id = $2",
        id,
        workspace_id.into_uuid(),
        reason,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// `rendered` → `applied`: its renditions are what viewers get now.
pub async fn mark_applied(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE edits SET status = 'applied', applied_at = now(), failure = NULL
        WHERE id = $1 AND workspace_id = $2
        "#,
        id,
        workspace_id.into_uuid(),
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// A newer edit was applied first: this one is kept, never shown.
pub async fn mark_superseded(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: WorkspaceId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE edits SET status = 'superseded' WHERE id = $1 AND workspace_id = $2",
        id,
        workspace_id.into_uuid(),
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Whether an edit of the recording newer than `version` is applied already.
pub async fn newer_applied(
    executor: impl PgExecutor<'_>,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    version: u32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM edits
            WHERE workspace_id = $1 AND recording_id = $2 AND version > $3
              AND status = 'applied'
        ) AS "newer!"
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        i32::try_from(version).unwrap_or(i32::MAX),
    )
    .fetch_one(executor)
    .await
}

/// Whether another edit of the recording is still on its way (queued, rendering or rendered but
/// not applied): while one is, the recording stays `processing`.
pub async fn others_pending(
    executor: impl PgExecutor<'_>,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM edits
            WHERE workspace_id = $1 AND recording_id = $2 AND id <> $3
              AND status IN ('queued', 'rendering', 'rendered')
        ) AS "pending!"
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        id,
    )
    .fetch_one(executor)
    .await
}
