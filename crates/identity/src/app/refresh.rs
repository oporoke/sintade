use kernel::SessionId;

use crate::app::login::{ACCESS_TOKEN_TTL, REFRESH_TOKEN_TTL};
use crate::app::service::IdentityService;
use crate::app::token::{generate_random_hex_token, issue_access_token, sha256_digest};
use crate::infra;

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("invalid or expired refresh token")]
    InvalidToken,

    /// A refresh token that had already been rotated away was presented again. The whole
    /// session family has just been revoked (US-02) -- this is not a normal 401, it's a signal
    /// the caller should treat the same as one, but callers may want to log it distinctly.
    #[error("refresh token reuse detected; session family revoked")]
    ReuseDetected,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct RefreshResult {
    pub access_token: String,
    pub access_ttl: time::Duration,
    pub refresh_token: String,
    pub refresh_ttl: time::Duration,
}

impl IdentityService {
    /// Rotates a refresh token (US-02). The presented token's row is retired (`revoked_at`
    /// set), not deleted, so that presenting it again -- the signature of a stolen and reused
    /// token -- can be detected and answered by revoking every other session in its family.
    #[tracing::instrument(skip_all)]
    pub async fn refresh(&self, raw_refresh_token: &str) -> Result<RefreshResult, RefreshError> {
        let hash = sha256_digest(raw_refresh_token);
        let now = self.clock.now();

        let mut tx = self.pool.begin().await?;

        let Some(session) = infra::find_session_by_refresh_hash_for_update(&mut tx, &hash).await?
        else {
            tx.rollback().await?;
            return Err(RefreshError::InvalidToken);
        };

        if session.revoked_at.is_some() {
            infra::revoke_family(&mut *tx, session.family_id, now).await?;
            tx.commit().await?;
            tracing::warn!(family_id = %session.family_id, "refresh token reuse detected; family revoked");
            return Err(RefreshError::ReuseDetected);
        }

        if session.expires_at <= now {
            tx.rollback().await?;
            return Err(RefreshError::InvalidToken);
        }

        infra::revoke_session(&mut *tx, session.id, now).await?;

        let workspace_id = infra::personal_workspace_id_for_user(&mut *tx, session.user_id)
            .await?
            .ok_or(RefreshError::InvalidToken)?;

        let access_token = issue_access_token(
            session.user_id,
            workspace_id,
            now,
            ACCESS_TOKEN_TTL,
            &self.session_secret,
        );

        let raw_new_refresh = generate_random_hex_token();
        let new_hash = sha256_digest(&raw_new_refresh);
        let new_session_id = SessionId::new_v7();
        let new_expires_at = now + REFRESH_TOKEN_TTL;
        infra::insert_session(
            &mut *tx,
            new_session_id,
            session.user_id,
            &new_hash,
            session.family_id,
            new_expires_at,
        )
        .await?;

        tx.commit().await?;

        Ok(RefreshResult {
            access_token,
            access_ttl: ACCESS_TOKEN_TTL,
            refresh_token: raw_new_refresh,
            refresh_ttl: REFRESH_TOKEN_TTL,
        })
    }
}
