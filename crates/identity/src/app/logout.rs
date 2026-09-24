use crate::app::service::IdentityService;
use crate::app::token::sha256_digest;
use crate::infra;

impl IdentityService {
    /// Revokes the session identified by a raw refresh token. Idempotent by design (matches the
    /// anti-enumeration style used elsewhere): an unknown, already-revoked, or missing token is
    /// not an error -- the caller's goal ("make sure this session is dead") is already true.
    #[tracing::instrument(skip_all)]
    pub async fn logout(&self, raw_refresh_token: &str) -> Result<(), sqlx::Error> {
        let hash = sha256_digest(raw_refresh_token);
        infra::revoke_session_by_refresh_hash(&self.pool, &hash, self.clock.now()).await
    }
}
