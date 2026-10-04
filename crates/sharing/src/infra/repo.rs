use kernel::{RecordingId, ShareLinkId, WorkspaceId};
use sqlx::PgConnection;
use time::OffsetDateTime;

use crate::domain::{ShareLinkView, Visibility};

struct Row {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    recording_id: uuid::Uuid,
    slug: String,
    visibility: String,
    allow_download: bool,
    expires_at: Option<OffsetDateTime>,
    revoked_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl Row {
    fn into_view(self) -> Result<ShareLinkView, sqlx::Error> {
        let visibility = Visibility::parse(&self.visibility).ok_or_else(|| {
            sqlx::Error::Decode(format!("unknown visibility {:?}", self.visibility).into())
        })?;
        Ok(ShareLinkView {
            id: ShareLinkId::from_uuid(self.id),
            workspace_id: WorkspaceId::from_uuid(self.workspace_id),
            recording_id: RecordingId::from_uuid(self.recording_id),
            slug: self.slug,
            visibility,
            allow_download: self.allow_download,
            expires_at: self.expires_at,
            revoked_at: self.revoked_at,
            created_at: self.created_at,
        })
    }
}

#[allow(clippy::too_many_arguments)]
/// Inserts a link. `Ok(false)`: the slug is taken (the caller draws another).
pub async fn insert(
    conn: &mut PgConnection,
    id: ShareLinkId,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    slug: &str,
    visibility: Visibility,
    allow_download: bool,
    expires_at: Option<OffsetDateTime>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        INSERT INTO share_links (id, workspace_id, recording_id, slug, visibility, allow_download, expires_at)
        VALUES ($1, $2, $3, $4, $5::text::visibility, $6, $7)
        ON CONFLICT (slug) DO NOTHING
        "#,
        id.into_uuid(),
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        slug,
        visibility.as_str(),
        allow_download,
        expires_at,
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn list_for_recording(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Result<Vec<ShareLinkView>, sqlx::Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT id, workspace_id, recording_id, slug, visibility::text AS "visibility!",
               allow_download, expires_at, revoked_at, created_at
        FROM share_links
        WHERE workspace_id = $1 AND recording_id = $2
        ORDER BY created_at DESC, id DESC
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter().map(Row::into_view).collect()
}

/// Changes a not-revoked link of the recording. `None`: no such live link in the workspace.
pub async fn update(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    id: ShareLinkId,
    visibility: Option<Visibility>,
    allow_download: Option<bool>,
    expires_at: Option<Option<OffsetDateTime>>,
) -> Result<Option<ShareLinkView>, sqlx::Error> {
    let (set_expiry, expiry) = match expires_at {
        Some(value) => (true, value),
        None => (false, None),
    };
    let row = sqlx::query_as!(
        Row,
        r#"
        UPDATE share_links
        SET visibility = COALESCE($4::text::visibility, visibility),
            allow_download = COALESCE($5, allow_download),
            expires_at = CASE WHEN $6 THEN $7 ELSE expires_at END
        WHERE id = $3 AND workspace_id = $1 AND recording_id = $2 AND revoked_at IS NULL
        RETURNING id, workspace_id, recording_id, slug, visibility::text AS "visibility!",
                  allow_download, expires_at, revoked_at, created_at
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        id.into_uuid(),
        visibility.map(Visibility::as_str),
        allow_download,
        set_expiry,
        expiry,
    )
    .fetch_optional(&mut *conn)
    .await?;
    row.map(Row::into_view).transpose()
}

/// Revokes a link (idempotent). `false`: no such link of the recording in the workspace.
pub async fn revoke(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    id: ShareLinkId,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        UPDATE share_links SET revoked_at = COALESCE(revoked_at, now())
        WHERE id = $3 AND workspace_id = $1 AND recording_id = $2
        RETURNING id
        "#,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        id.into_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.is_some())
}

/// The link behind a slug, whatever its state. Slugs are global (the viewer has no workspace).
pub async fn find_by_slug(
    conn: &mut PgConnection,
    slug: &str,
) -> Result<Option<ShareLinkView>, sqlx::Error> {
    let row = sqlx::query_as!(
        Row,
        r#"
        SELECT id, workspace_id, recording_id, slug, visibility::text AS "visibility!",
               allow_download, expires_at, revoked_at, created_at
        FROM share_links
        WHERE slug = $1
        "#,
        slug,
    )
    .fetch_optional(&mut *conn)
    .await?;
    row.map(Row::into_view).transpose()
}
