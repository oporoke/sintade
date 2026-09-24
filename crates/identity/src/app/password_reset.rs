use time::Duration as TimeDuration;
use uuid::Uuid;

use crate::app::service::IdentityService;
use crate::app::token::{generate_random_hex_token, hash_password, sha256_digest};
use crate::domain::{Email, Password, PasswordError};
use crate::infra;

const RESET_TOKEN_TTL_HOURS: i64 = 1;

pub struct ForgotPasswordRequest {
    pub email: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ResetPasswordError {
    #[error("invalid or expired reset token")]
    InvalidToken,

    #[error(transparent)]
    InvalidPassword(#[from] PasswordError),

    #[error("failed to hash password")]
    Hash,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct ResetPasswordRequest {
    pub token: String,
    pub password: String,
}

impl IdentityService {
    /// Deliberately returns the same `Ok(())` whether or not the email exists, and whether or
    /// not it's even well-formed -- no account enumeration (US-03), same style as `register`.
    #[tracing::instrument(skip_all)]
    pub async fn forgot_password(&self, request: ForgotPasswordRequest) -> Result<(), sqlx::Error> {
        let Ok(email) = Email::parse(&request.email) else {
            return Ok(());
        };

        let Some(user_id) = infra::find_user_id_by_email(&self.pool, email.as_str()).await? else {
            return Ok(());
        };

        let raw_token = generate_random_hex_token();
        let token_hash = sha256_digest(&raw_token);
        let now = self.clock.now();
        let expires_at = now + TimeDuration::hours(RESET_TOKEN_TTL_HOURS);
        infra::insert_email_token(
            &self.pool,
            Uuid::now_v7(),
            user_id,
            &token_hash,
            "password_reset",
            expires_at,
        )
        .await?;

        let link = format!("{}/reset-password?token={raw_token}", self.public_base_url);
        self.enqueue_email(
            email.as_str(),
            "Reset your Sintade password",
            &format!("Click to reset your password: {link}"),
        )
        .await;

        Ok(())
    }

    /// Resets the password and revokes every session for the user (US-03) in one transaction --
    /// a reset is exactly the moment an attacker's still-live sessions must not survive.
    #[tracing::instrument(skip_all)]
    pub async fn reset_password(
        &self,
        request: ResetPasswordRequest,
    ) -> Result<(), ResetPasswordError> {
        let password = Password::parse(&request.password)?;
        let hash = sha256_digest(&request.token);
        let now = self.clock.now();

        let mut tx = self.pool.begin().await?;

        let Some(token) =
            infra::find_valid_email_token_for_update(&mut tx, &hash, "password_reset", now).await?
        else {
            tx.rollback().await?;
            return Err(ResetPasswordError::InvalidToken);
        };

        let password_hash =
            hash_password(password.as_str()).map_err(|_| ResetPasswordError::Hash)?;

        infra::mark_email_token_used(&mut *tx, token.id, now).await?;
        infra::update_password_hash(&mut *tx, token.user_id, &password_hash).await?;
        infra::revoke_all_sessions_for_user(&mut *tx, token.user_id, now).await?;

        tx.commit().await?;
        Ok(())
    }
}
