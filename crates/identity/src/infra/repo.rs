use kernel::{UserId, WorkspaceId};
use sqlx::PgConnection;
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
