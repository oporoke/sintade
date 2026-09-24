use kernel::{SessionId, UserId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};
use time::OffsetDateTime;
use uuid::Uuid;

pub async fn user_exists_by_email(
    conn: &mut PgConnection,
    email: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)", email)
        .fetch_one(&mut *conn)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn insert_user(
    conn: &mut PgConnection,
    id: UserId,
    email: &str,
    display_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)",
        id.into_uuid(),
        email,
        display_name,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn insert_credential(
    conn: &mut PgConnection,
    user_id: UserId,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO credentials (user_id, password_hash) VALUES ($1, $2)",
        user_id.into_uuid(),
        password_hash,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `workspaces`/`memberships` are owned by the not-yet-existing `tenancy` module -- see
/// ADR-0004. Remove this once `tenancy` exists (Day 18) and call its service instead.
pub async fn insert_personal_workspace(
    conn: &mut PgConnection,
    id: WorkspaceId,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO workspaces (id, name, is_personal) VALUES ($1, $2, true)",
        id.into_uuid(),
        name,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn insert_owner_membership(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
        workspace_id.into_uuid(),
        user_id.into_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub struct CredentialRecord {
    pub user_id: UserId,
    pub password_hash: String,
}

pub async fn find_credential_by_email(
    executor: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<CredentialRecord>, sqlx::Error> {
    sqlx::query!(
        r#"
        SELECT u.id as "user_id!", c.password_hash
        FROM users u
        JOIN credentials c ON c.user_id = u.id
        WHERE u.email = $1
        "#,
        email,
    )
    .fetch_optional(executor)
    .await
    .map(|row| {
        row.map(|row| CredentialRecord {
            user_id: UserId::from_uuid(row.user_id),
            password_hash: row.password_hash,
        })
    })
}

/// See ADR-0004: `workspaces`/`memberships` are owned by the not-yet-existing `tenancy` module.
pub async fn personal_workspace_id_for_user(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<Option<WorkspaceId>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT w.id
        FROM workspaces w
        JOIN memberships m ON m.workspace_id = w.id
        WHERE m.user_id = $1 AND w.is_personal = true
        "#,
        user_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await
    .map(|maybe_id| maybe_id.map(WorkspaceId::from_uuid))
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_session(
    executor: impl PgExecutor<'_>,
    id: SessionId,
    user_id: UserId,
    refresh_hash: &[u8],
    family_id: Uuid,
    expires_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO sessions (id, user_id, refresh_hash, family_id, expires_at) VALUES ($1, $2, $3, $4, $5)",
        id.into_uuid(),
        user_id.into_uuid(),
        refresh_hash,
        family_id,
        expires_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub struct UserRecord {
    pub id: UserId,
    pub email: String,
    pub display_name: String,
    pub email_verified: bool,
}

pub async fn find_user(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<Option<UserRecord>, sqlx::Error> {
    sqlx::query!(
        r#"SELECT id, email::text as "email!", display_name, email_verified FROM users WHERE id = $1"#,
        user_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await
    .map(|row| {
        row.map(|row| UserRecord {
            id: UserId::from_uuid(row.id),
            email: row.email,
            display_name: row.display_name,
            email_verified: row.email_verified,
        })
    })
}

pub struct WorkspaceMembershipRecord {
    pub id: WorkspaceId,
    pub name: String,
    pub role: String,
    pub is_personal: bool,
}

/// See ADR-0004: `workspaces`/`memberships` are owned by the not-yet-existing `tenancy` module.
pub async fn list_user_workspaces(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<Vec<WorkspaceMembershipRecord>, sqlx::Error> {
    sqlx::query!(
        r#"
        SELECT w.id, w.name, w.is_personal, m.role::text as "role!"
        FROM workspaces w
        JOIN memberships m ON m.workspace_id = w.id
        WHERE m.user_id = $1
        ORDER BY w.created_at
        "#,
        user_id.into_uuid(),
    )
    .fetch_all(executor)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| WorkspaceMembershipRecord {
                id: WorkspaceId::from_uuid(row.id),
                name: row.name,
                role: row.role,
                is_personal: row.is_personal,
            })
            .collect()
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_email_token(
    conn: &mut PgConnection,
    id: Uuid,
    user_id: UserId,
    token_hash: &[u8],
    purpose: &str,
    expires_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO email_tokens (id, user_id, token_hash, purpose, expires_at) VALUES ($1, $2, $3, $4, $5)",
        id,
        user_id.into_uuid(),
        token_hash,
        purpose,
        expires_at,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}
