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

/// Serialises recording creation per workspace until the transaction ends, so a count-then-insert
/// quota check can't be raced past by concurrent requests.
pub async fn lock_workspace_recordings(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended('catalog.recordings:' || $1::uuid::text, 0))",
        workspace_id.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Recordings that count towards the plan: everything but abandoned and trashed ones.
pub async fn count_active_recordings(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM recordings
        WHERE workspace_id = $1 AND state NOT IN ('abandoned', 'trashed') AND trashed_at IS NULL
        "#,
        workspace_id.into_uuid(),
    )
    .fetch_one(&mut *conn)
    .await
}

/// `recording`/`uploading` → `abandoned` for the given recordings (each in its own workspace).
/// Returns the ones that changed.
pub async fn mark_abandoned(
    conn: &mut PgConnection,
    recordings: &[(RecordingId, WorkspaceId)],
) -> Result<Vec<RecordingId>, sqlx::Error> {
    let ids: Vec<uuid::Uuid> = recordings.iter().map(|(id, _)| id.into_uuid()).collect();
    let workspaces: Vec<uuid::Uuid> = recordings.iter().map(|(_, ws)| ws.into_uuid()).collect();
    let changed = sqlx::query_scalar!(
        r#"
        UPDATE recordings r
        SET state = 'abandoned', updated_at = now()
        FROM unnest($1::uuid[], $2::uuid[]) AS target(id, workspace_id)
        WHERE r.id = target.id AND r.workspace_id = target.workspace_id
          AND r.state IN ('recording', 'uploading')
        RETURNING r.id
        "#,
        &ids,
        &workspaces,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(changed.into_iter().map(RecordingId::from_uuid).collect())
}

/// Who owns a recording and what it's called: what "ready"/"failed" notices need.
pub struct RecordingOwner {
    pub owner_id: UserId,
    pub title: String,
}

/// `processing` → `ready` with what the MP4 measured. `None` if the recording isn't
/// `processing` in that workspace (trashed or abandoned meanwhile).
#[allow(clippy::too_many_arguments)]
pub async fn mark_ready(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    duration_ms: i32,
    width: i32,
    height: i32,
    size_bytes: i64,
) -> Result<Option<RecordingOwner>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE recordings
        SET state = 'ready', duration_ms = $3, width = $4, height = $5, size_bytes = $6,
            updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND state = 'processing'
        RETURNING owner_id, title
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        duration_ms,
        width,
        height,
        size_bytes,
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| RecordingOwner {
        owner_id: UserId::from_uuid(row.owner_id),
        title: row.title,
    }))
}

/// `processing` → `failed`. `None` if the recording isn't `processing` in that workspace.
pub async fn mark_failed(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<Option<RecordingOwner>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE recordings SET state = 'failed', updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND state = 'processing'
        RETURNING owner_id, title
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| RecordingOwner {
        owner_id: UserId::from_uuid(row.owner_id),
        title: row.title,
    }))
}

/// How a retry found the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reopened {
    /// `failed` → `processing`.
    Reopened,
    /// The recording exists in the workspace but isn't `failed`.
    NotFailed,
    /// No such recording in the workspace.
    NotFound,
}

/// `failed` → `processing` (manual retry, docs/design.md §4). One statement, so two racing
/// retries reopen it once; the other sees `NotFailed`.
pub async fn reopen_failed(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<Reopened, sqlx::Error> {
    let reopened = sqlx::query_scalar!(
        r#"
        UPDATE recordings SET state = 'processing', updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND state = 'failed'
        RETURNING id
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    if reopened.is_some() {
        return Ok(Reopened::Reopened);
    }
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM recordings WHERE id = $1 AND workspace_id = $2) AS "exists!""#,
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(if exists {
        Reopened::NotFailed
    } else {
        Reopened::NotFound
    })
}

pub async fn exists_in_workspace(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM recordings WHERE id = $1 AND workspace_id = $2) AS "exists!""#,
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_one(&mut *conn)
    .await
}
