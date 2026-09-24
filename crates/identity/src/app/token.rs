use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use kernel::{UserId, WorkspaceId};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AccessTokenError {
    #[error("malformed access token")]
    Malformed,
    #[error("access token signature invalid")]
    BadSignature,
    #[error("access token expired")]
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessClaims {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
}

#[derive(Serialize, Deserialize)]
struct ClaimsPayload {
    user_id: UserId,
    workspace_id: WorkspaceId,
    exp: i64,
}

/// A compact `{base64url(json payload)}.{base64url(hmac-sha256 signature)}` token, signed with
/// `SESSION_SECRET`. Not JWT-compliant on purpose -- no algorithm negotiation, no header, so
/// there's no "alg: none" class of bug to worry about.
pub fn issue_access_token(
    user_id: UserId,
    workspace_id: WorkspaceId,
    now: OffsetDateTime,
    ttl: Duration,
    secret: &[u8],
) -> String {
    let payload = ClaimsPayload {
        user_id,
        workspace_id,
        exp: (now + ttl).unix_timestamp(),
    };
    let payload_json = serde_json::to_vec(&payload).expect("claims payload serializes");
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);
    let signature_b64 = sign(payload_b64.as_bytes(), secret);
    format!("{payload_b64}.{signature_b64}")
}

pub fn verify_access_token(
    token: &str,
    now: OffsetDateTime,
    secret: &[u8],
) -> Result<AccessClaims, AccessTokenError> {
    let (payload_b64, signature_b64) = token.split_once('.').ok_or(AccessTokenError::Malformed)?;

    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|_| AccessTokenError::Malformed)?;

    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac-sha256 accepts any key length");
    mac.update(payload_b64.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| AccessTokenError::BadSignature)?;

    let payload_json = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| AccessTokenError::Malformed)?;
    let payload: ClaimsPayload =
        serde_json::from_slice(&payload_json).map_err(|_| AccessTokenError::Malformed)?;

    if payload.exp < now.unix_timestamp() {
        return Err(AccessTokenError::Expired);
    }

    Ok(AccessClaims {
        user_id: payload.user_id,
        workspace_id: payload.workspace_id,
    })
}

fn sign(payload: &[u8], secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac-sha256 accepts any key length");
    mac.update(payload);
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// A random 32-byte token, hex-encoded, for opaque single-use secrets (refresh tokens, email
/// verification tokens) that are stored server-side only as a SHA-256 hash.
pub fn generate_random_hex_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_digest(input: &str) -> Vec<u8> {
    Sha256::digest(input.as_bytes()).to_vec()
}

/// Argon2::default() is argon2id, m=19 MiB, t=2, p=1 -- exactly docs/design.md's spec
/// ("argon2id, m=19 MiB, t=2, p=1 (OWASP baseline)"); hash_password() generates its own
/// random salt internally.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    use argon2::Argon2;
    use argon2::password_hash::PasswordHasher;

    let argon2 = Argon2::default();
    Ok(argon2.hash_password(password.as_bytes())?.to_string())
}

impl crate::app::service::IdentityService {
    /// Verifies an access-cookie value against this service's `SESSION_SECRET` and clock.
    pub fn verify_access_cookie(&self, token: &str) -> Result<AccessClaims, AccessTokenError> {
        verify_access_token(token, self.clock.now(), &self.session_secret)
    }
}

/// Decodes `SESSION_SECRET` (base64, per `docs/design.md` §16: "64 random bytes, base64") into
/// raw bytes for HMAC signing. Panics on an invalid value -- this is startup configuration, not
/// runtime input.
pub fn decode_session_secret(base64_secret: &str) -> Vec<u8> {
    use base64::engine::general_purpose::STANDARD;
    STANDARD
        .decode(base64_secret)
        .expect("SESSION_SECRET must be valid base64")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-session-secret-32-bytes-ok";

    fn user_id() -> UserId {
        UserId::new_v7()
    }

    fn workspace_id() -> WorkspaceId {
        WorkspaceId::new_v7()
    }

    #[test]
    fn round_trips_a_valid_token() {
        let now = OffsetDateTime::now_utc();
        let user_id = user_id();
        let workspace_id = workspace_id();
        let token = issue_access_token(user_id, workspace_id, now, Duration::minutes(15), SECRET);

        let claims = verify_access_token(&token, now, SECRET).expect("valid token verifies");
        assert_eq!(claims.user_id, user_id);
        assert_eq!(claims.workspace_id, workspace_id);
    }

    #[test]
    fn rejects_a_token_signed_with_a_different_secret() {
        let now = OffsetDateTime::now_utc();
        let token = issue_access_token(
            user_id(),
            workspace_id(),
            now,
            Duration::minutes(15),
            SECRET,
        );

        let result = verify_access_token(&token, now, b"a-completely-different-secret!!");
        assert_eq!(result, Err(AccessTokenError::BadSignature));
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let now = OffsetDateTime::now_utc();
        let token = issue_access_token(
            user_id(),
            workspace_id(),
            now,
            Duration::minutes(15),
            SECRET,
        );
        let (_, signature_b64) = token.split_once('.').expect("token has two parts");
        let forged_payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&ClaimsPayload {
                user_id: user_id(),
                workspace_id: workspace_id(),
                exp: (now + Duration::days(365)).unix_timestamp(),
            })
            .expect("serializes"),
        );
        let forged = format!("{forged_payload}.{signature_b64}");

        let result = verify_access_token(&forged, now, SECRET);
        assert_eq!(result, Err(AccessTokenError::BadSignature));
    }

    #[test]
    fn rejects_an_expired_token() {
        let now = OffsetDateTime::now_utc();
        let token = issue_access_token(
            user_id(),
            workspace_id(),
            now,
            Duration::minutes(15),
            SECRET,
        );

        let later = now + Duration::minutes(16);
        let result = verify_access_token(&token, later, SECRET);
        assert_eq!(result, Err(AccessTokenError::Expired));
    }

    #[test]
    fn rejects_a_malformed_token() {
        let now = OffsetDateTime::now_utc();
        assert_eq!(
            verify_access_token("not-a-real-token", now, SECRET),
            Err(AccessTokenError::Malformed)
        );
    }
}
