use argon2::Argon2;
use argon2::password_hash::PasswordVerifier;
use argon2::password_hash::phc::PasswordHash;
use kernel::SessionId;
use time::Duration as TimeDuration;
use uuid::Uuid;

use crate::app::service::IdentityService;
use crate::app::token::{generate_random_hex_token, issue_access_token, sha256_digest};
use crate::domain::Email;
use crate::infra;

pub const ACCESS_TOKEN_TTL: TimeDuration = TimeDuration::minutes(15);
pub const REFRESH_TOKEN_TTL: TimeDuration = TimeDuration::days(30);

/// A syntactically valid but never-issued argon2id hash (same cost params as real hashes), used
/// to keep login's response time roughly constant whether or not the email exists -- otherwise
/// "unknown email skips the hash verify" is a timing oracle for account enumeration.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$3sQ/SSFnS4XIrgEaxneWhA$tjdIG9z4KnjoP6ID1jgT766aOPuI1gjn4Xs/JFhYzu8";

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("invalid email or password")]
    InvalidCredentials,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub struct LoginSession {
    pub access_token: String,
    pub access_ttl: TimeDuration,
    pub refresh_token: String,
    pub refresh_ttl: TimeDuration,
}

impl IdentityService {
    #[tracing::instrument(skip_all)]
    pub async fn login(&self, request: LoginRequest) -> Result<LoginSession, LoginError> {
        let email = match Email::parse(&request.email) {
            Ok(email) => email,
            Err(_) => {
                verify_password(&request.password, DUMMY_PASSWORD_HASH);
                return Err(LoginError::InvalidCredentials);
            }
        };

        let Some(credential) = infra::find_credential_by_email(&self.pool, email.as_str()).await?
        else {
            verify_password(&request.password, DUMMY_PASSWORD_HASH);
            return Err(LoginError::InvalidCredentials);
        };

        if !verify_password(&request.password, &credential.password_hash) {
            return Err(LoginError::InvalidCredentials);
        }

        let workspace_id = infra::personal_workspace_id_for_user(&self.pool, credential.user_id)
            .await?
            .ok_or(LoginError::InvalidCredentials)?;

        let now = self.clock.now();
        let access_token = issue_access_token(
            credential.user_id,
            workspace_id,
            now,
            ACCESS_TOKEN_TTL,
            &self.session_secret,
        );

        let raw_refresh = generate_random_hex_token();
        let refresh_hash = sha256_digest(&raw_refresh);
        let session_id = SessionId::new_v7();
        let family_id = Uuid::now_v7();
        let expires_at = now + REFRESH_TOKEN_TTL;
        infra::insert_session(
            &self.pool,
            session_id,
            credential.user_id,
            &refresh_hash,
            family_id,
            expires_at,
        )
        .await?;

        Ok(LoginSession {
            access_token,
            access_ttl: ACCESS_TOKEN_TTL,
            refresh_token: raw_refresh,
            refresh_ttl: REFRESH_TOKEN_TTL,
        })
    }
}

/// Constant-time-ish (via `argon2`'s own verifier) password check. Returns `false` on any
/// error, including a malformed stored hash -- that should never happen for a real credential
/// row, only for the intentionally-malformed-never dummy path above.
fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}
