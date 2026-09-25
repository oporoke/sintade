use kernel::{SessionId, UserId};
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

pub struct SessionRecord {
    pub id: SessionId,
    pub user_id: UserId,
    pub family_id: Uuid,
    pub revoked_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}

/// Locks the row (`FOR UPDATE`) so two concurrent refreshes of the same token can't both see
/// it as un-rotated -- caller must run this inside a transaction for the lock to hold.
pub async fn find_session_by_refresh_hash_for_update(
    conn: &mut PgConnection,
    refresh_hash: &[u8],
) -> Result<Option<SessionRecord>, sqlx::Error> {
    sqlx::query!(
        r#"
        SELECT id, user_id, family_id, revoked_at, expires_at
        FROM sessions
        WHERE refresh_hash = $1
        FOR UPDATE
        "#,
        refresh_hash,
    )
    .fetch_optional(&mut *conn)
    .await
    .map(|row| {
        row.map(|row| SessionRecord {
            id: SessionId::from_uuid(row.id),
            user_id: UserId::from_uuid(row.user_id),
            family_id: row.family_id,
            revoked_at: row.revoked_at,
            expires_at: row.expires_at,
        })
    })
}

pub async fn revoke_session(
    executor: impl PgExecutor<'_>,
    id: SessionId,
    revoked_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE sessions SET revoked_at = $2 WHERE id = $1",
        id.into_uuid(),
        revoked_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Revokes every currently-active session in a family -- the family-wide response to detecting
/// a rotated-away refresh token being replayed.
pub async fn revoke_family(
    executor: impl PgExecutor<'_>,
    family_id: Uuid,
    revoked_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE sessions SET revoked_at = $2 WHERE family_id = $1 AND revoked_at IS NULL",
        family_id,
        revoked_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn revoke_session_by_refresh_hash(
    executor: impl PgExecutor<'_>,
    refresh_hash: &[u8],
    revoked_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE sessions SET revoked_at = $2 WHERE refresh_hash = $1 AND revoked_at IS NULL",
        refresh_hash,
        revoked_at,
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

#[allow(clippy::too_many_arguments)]
pub async fn insert_email_token(
    executor: impl PgExecutor<'_>,
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
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn find_user_id_by_email(
    executor: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<UserId>, sqlx::Error> {
    sqlx::query_scalar!("SELECT id FROM users WHERE email = $1", email)
        .fetch_optional(executor)
        .await
        .map(|maybe_id| maybe_id.map(UserId::from_uuid))
}

pub struct EmailTokenRecord {
    pub id: Uuid,
    pub user_id: UserId,
}

/// Locks the row (`FOR UPDATE`) so two concurrent uses of the same token can't both see it as
/// unused -- caller must run this inside a transaction for the lock to hold.
pub async fn find_valid_email_token_for_update(
    conn: &mut PgConnection,
    token_hash: &[u8],
    purpose: &str,
    now: OffsetDateTime,
) -> Result<Option<EmailTokenRecord>, sqlx::Error> {
    sqlx::query!(
        r#"
        SELECT id, user_id
        FROM email_tokens
        WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > $3
        FOR UPDATE
        "#,
        token_hash,
        purpose,
        now,
    )
    .fetch_optional(&mut *conn)
    .await
    .map(|row| {
        row.map(|row| EmailTokenRecord {
            id: row.id,
            user_id: UserId::from_uuid(row.user_id),
        })
    })
}

pub async fn mark_email_token_used(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    used_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE email_tokens SET used_at = $2 WHERE id = $1",
        id,
        used_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn mark_email_verified(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users SET email_verified = true WHERE id = $1",
        user_id.into_uuid(),
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn update_password_hash(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE credentials SET password_hash = $2 WHERE user_id = $1",
        user_id.into_uuid(),
        password_hash,
    )
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn revoke_all_sessions_for_user(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
    revoked_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        user_id.into_uuid(),
        revoked_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}
