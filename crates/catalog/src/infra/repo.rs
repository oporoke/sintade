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

/// What a watch page shows about a recording.
pub struct WatchInfo {
    pub owner_id: UserId,
    pub title: String,
    pub state: String,
    pub duration_ms: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub created_at: time::OffsetDateTime,
}

/// A recording that isn't in the trash. Trashed recordings (Day 58) read as missing.
pub async fn watch_info(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<Option<WatchInfo>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT owner_id, title, state::text AS "state!", duration_ms, width, height, created_at
        FROM recordings
        WHERE id = $1 AND workspace_id = $2 AND trashed_at IS NULL
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| WatchInfo {
        owner_id: UserId::from_uuid(row.owner_id),
        title: row.title,
        state: row.state,
        duration_ms: row.duration_ms,
        width: row.width,
        height: row.height,
        created_at: row.created_at,
    }))
}

/// One row of the library.
#[derive(Debug)]
pub struct LibraryItem {
    pub id: RecordingId,
    pub owner_id: UserId,
    pub title: String,
    pub state: String,
    pub duration_ms: Option<i32>,
    pub created_at: time::OffsetDateTime,
}

/// The workspace's recordings, newest first, after `cursor`. Trashed and abandoned ones are not
/// part of the library. Fetches `limit` rows (callers ask for one more than a page to know
/// whether another page exists).
pub async fn list_library(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    cursor: Option<crate::domain::Cursor>,
    limit: i64,
) -> Result<Vec<LibraryItem>, sqlx::Error> {
    let (after_at, after_id) = match cursor {
        Some(cursor) => (
            time::OffsetDateTime::from_unix_timestamp_nanos(
                i128::from(cursor.created_at_micros) * 1_000,
            )
            .ok(),
            Some(cursor.id),
        ),
        None => (None, None),
    };
    let rows = sqlx::query!(
        r#"
        SELECT id, owner_id, title, state::text AS "state!", duration_ms, created_at
        FROM recordings
        WHERE workspace_id = $1
          AND trashed_at IS NULL
          AND state NOT IN ('abandoned', 'trashed')
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
        workspace_id.into_uuid(),
        after_at,
        after_id,
        limit,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| LibraryItem {
            id: RecordingId::from_uuid(row.id),
            owner_id: UserId::from_uuid(row.owner_id),
            title: row.title,
            state: row.state,
            duration_ms: row.duration_ms,
            created_at: row.created_at,
        })
        .collect())
}

/// Renames a recording that is not in the trash. `false`: no such recording in the workspace.
pub async fn rename(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    title: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE recordings SET title = $3, updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND trashed_at IS NULL
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        title,
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Moves a recording to the trash (idempotent; keeps the first trash time). `false`: no such
/// recording in the workspace.
pub async fn trash(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    now: time::OffsetDateTime,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        UPDATE recordings SET trashed_at = COALESCE(trashed_at, $3), updated_at = now()
        WHERE id = $1 AND workspace_id = $2
        RETURNING id
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        now,
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.is_some())
}

/// Recordings trashed before `cutoff`, oldest first.
pub async fn trashed_before(
    conn: &mut PgConnection,
    cutoff: time::OffsetDateTime,
    limit: i64,
) -> Result<Vec<(RecordingId, WorkspaceId)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, workspace_id FROM recordings
        WHERE trashed_at IS NOT NULL AND trashed_at < $1
        ORDER BY trashed_at
        LIMIT $2
        "#,
        cutoff,
        limit,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                RecordingId::from_uuid(row.id),
                WorkspaceId::from_uuid(row.workspace_id),
            )
        })
        .collect())
}

/// Deletes a trashed recording's row; takes, chunks, renditions, media jobs and share links go
/// with it (`ON DELETE CASCADE`). Only a still-trashed recording is deleted.
pub async fn delete_trashed(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM recordings WHERE id = $1 AND workspace_id = $2 AND trashed_at IS NOT NULL",
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The recording's duration if it exists in the workspace and is not in the trash.
/// `None`: no such recording; `Some(None)`: it exists but has no duration yet.
pub async fn live_duration(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<Option<Option<i32>>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT duration_ms FROM recordings WHERE id = $1 AND workspace_id = $2 AND trashed_at IS NULL",
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| row.duration_ms))
}

/// The recording's chapters, earliest first.
pub async fn chapters(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
) -> Result<Vec<(i32, String)>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT start_ms, title FROM chapters
         WHERE recording_id = $1 AND workspace_id = $2 ORDER BY start_ms",
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.start_ms, row.title))
        .collect())
}

/// Replaces the recording's chapters with `chapters` (in the caller's transaction).
pub async fn replace_chapters(
    conn: &mut PgConnection,
    id: RecordingId,
    workspace_id: WorkspaceId,
    chapters: &[(i32, &str)],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM chapters WHERE recording_id = $1 AND workspace_id = $2",
        id.into_uuid(),
        workspace_id.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    for (start_ms, title) in chapters {
        sqlx::query!(
            "INSERT INTO chapters (id, workspace_id, recording_id, start_ms, title)
             VALUES ($1, $2, $3, $4, $5)",
            uuid::Uuid::now_v7(),
            workspace_id.into_uuid(),
            id.into_uuid(),
            start_ms,
            title,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
