//! The token on a rung playlist's URL (docs/design.md §16, ADR-0033): proof that the master
//! playlist was fetched, by someone the link admits, for this recording, a moment ago. The master
//! hands one to each rung; a rung playlist is refused without a valid one, and its segments are
//! signed only until the token expires.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use kernel::RecordingId;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("not a valid playlist token")]
    Invalid,
    #[error("the playlist token has expired")]
    Expired,
}

pub struct ManifestSigner {
    key: Vec<u8>,
}

impl ManifestSigner {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    fn mac(&self, payload: &[u8]) -> HmacSha256 {
        let mut mac =
            HmacSha256::new_from_slice(&self.key).expect("hmac-sha256 accepts any key length");
        mac.update(payload);
        mac
    }

    /// `base64url(recording id ‖ expiry as big-endian unix seconds) "." base64url(hmac)`.
    pub fn issue(&self, recording: RecordingId, expires_at: i64) -> String {
        let mut payload = Vec::with_capacity(24);
        payload.extend_from_slice(recording.into_uuid().as_bytes());
        payload.extend_from_slice(&expires_at.to_be_bytes());
        let signature = self.mac(&payload).finalize().into_bytes();
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(signature)
        )
    }

    /// The token's expiry (unix seconds) if it is genuine, for `recording`, and not yet expired
    /// at `now`.
    pub fn verify(&self, token: &str, recording: RecordingId, now: i64) -> Result<i64, TokenError> {
        let (payload_b64, signature_b64) = token.split_once('.').ok_or(TokenError::Invalid)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| TokenError::Invalid)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_b64)
            .map_err(|_| TokenError::Invalid)?;
        self.mac(&payload)
            .verify_slice(&signature)
            .map_err(|_| TokenError::Invalid)?;
        let (id, exp) = payload.split_at_checked(16).ok_or(TokenError::Invalid)?;
        let exp: [u8; 8] = exp.try_into().map_err(|_| TokenError::Invalid)?;
        if id != recording.into_uuid().as_bytes() {
            return Err(TokenError::Invalid);
        }
        let expires_at = i64::from_be_bytes(exp);
        if expires_at <= now {
            return Err(TokenError::Expired);
        }
        Ok(expires_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> ManifestSigner {
        ManifestSigner::new(b"k".repeat(32))
    }

    #[test]
    fn a_fresh_token_verifies_for_its_recording_only() {
        let recording = RecordingId::new_v7();
        let token = signer().issue(recording, 2_000);
        assert_eq!(signer().verify(&token, recording, 1_000), Ok(2_000));
        assert_eq!(
            signer().verify(&token, RecordingId::new_v7(), 1_000),
            Err(TokenError::Invalid)
        );
    }

    #[test]
    fn an_expired_token_is_refused_from_its_expiry_second() {
        let recording = RecordingId::new_v7();
        let token = signer().issue(recording, 2_000);
        assert_eq!(signer().verify(&token, recording, 1_999), Ok(2_000));
        assert_eq!(
            signer().verify(&token, recording, 2_000),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn forged_tokens_are_invalid() {
        let recording = RecordingId::new_v7();
        let token = signer().issue(recording, 2_000);
        // Another key, a stretched expiry, junk.
        let other = ManifestSigner::new(b"z".repeat(32));
        assert_eq!(other.verify(&token, recording, 0), Err(TokenError::Invalid));
        let (payload, signature) = token.split_once('.').expect("two parts");
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).expect("payload");
        bytes[23] ^= 0xff;
        let forged = format!("{}.{signature}", URL_SAFE_NO_PAD.encode(bytes));
        assert_eq!(
            signer().verify(&forged, recording, 0),
            Err(TokenError::Invalid)
        );
        for junk in ["", "a", "a.b", "...", "AAAA.AAAA"] {
            assert_eq!(
                signer().verify(junk, recording, 0),
                Err(TokenError::Invalid),
                "{junk}"
            );
        }
    }
}
