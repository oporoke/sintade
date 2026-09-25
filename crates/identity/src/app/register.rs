use kernel::UserId;
use time::Duration as TimeDuration;
use uuid::Uuid;

use crate::app::service::IdentityService;
use crate::app::token::{generate_random_hex_token, hash_password, sha256_digest};
use crate::domain::{Email, EmailError, Password, PasswordError};
use crate::infra;

const VERIFY_EMAIL_TTL_HOURS: i64 = 24;

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error(transparent)]
    InvalidEmail(#[from] EmailError),

    #[error(transparent)]
    InvalidPassword(#[from] PasswordError),

    #[error("failed to hash password")]
    Hash,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

impl IdentityService {
    /// Registers a user and their personal workspace in one transaction (US-01), then enqueues
    /// a verification email. Deliberately returns the same `Ok(())` whether this was a fresh
    /// registration or the email was already taken -- no account enumeration (US-01).
    #[tracing::instrument(skip_all)]
    pub async fn register(&self, request: RegisterRequest) -> Result<(), RegisterError> {
        let email = Email::parse(&request.email)?;
        let password = Password::parse(&request.password)?;

        let mut tx = self.pool.begin().await?;

        if infra::user_exists_by_email(&mut tx, email.as_str()).await? {
            tx.rollback().await?;
            return Ok(());
        }

        let password_hash = hash_password(password.as_str()).map_err(|_| RegisterError::Hash)?;

        let user_id = UserId::new_v7();
        infra::insert_user(&mut tx, user_id, email.as_str(), &request.display_name).await?;
        infra::insert_credential(&mut tx, user_id, &password_hash).await?;

        let workspace_name = format!("{}'s workspace", request.display_name);
        self.workspaces
            .create_personal_workspace(&mut tx, user_id, &workspace_name)
            .await?;

        let raw_token = generate_random_hex_token();
        let token_hash = sha256_digest(&raw_token);
        let expires_at = self.clock.now() + TimeDuration::hours(VERIFY_EMAIL_TTL_HOURS);
        infra::insert_email_token(
            &mut *tx,
            Uuid::now_v7(),
            user_id,
            &token_hash,
            "verify_email",
            expires_at,
        )
        .await?;

        tx.commit().await?;

        let link = format!("{}/verify-email?token={raw_token}", self.public_base_url);
        self.enqueue_email(
            email.as_str(),
            "Verify your Sintade account",
            &format!("Click to verify your email: {link}"),
        )
        .await;

        Ok(())
    }
}
