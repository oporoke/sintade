use crate::app::service::IdentityService;
use crate::app::token::sha256_digest;
use crate::infra;

#[derive(Debug, thiserror::Error)]
pub enum VerifyEmailError {
    #[error("invalid or expired verification token")]
    InvalidToken,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IdentityService {
    #[tracing::instrument(skip_all)]
    pub async fn verify_email(&self, raw_token: &str) -> Result<(), VerifyEmailError> {
        let hash = sha256_digest(raw_token);
        let now = self.clock.now();

        let mut tx = self.pool.begin().await?;

        let Some(token) =
            infra::find_valid_email_token_for_update(&mut tx, &hash, "verify_email", now).await?
        else {
            tx.rollback().await?;
            return Err(VerifyEmailError::InvalidToken);
        };

        infra::mark_email_token_used(&mut *tx, token.id, now).await?;
        infra::mark_email_verified(&mut *tx, token.user_id).await?;

        tx.commit().await?;
        Ok(())
    }
}
