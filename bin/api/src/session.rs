use axum::extract::FromRequestParts;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use identity::AccessTokenError;
use kernel::{AppError, UserId, WorkspaceId};

use crate::app::AppState;
use crate::error::ApiError;

pub const ACCESS_COOKIE_NAME: &str = "sintade_session";
pub const REFRESH_COOKIE_NAME: &str = "sintade_refresh";

/// The decoded identity of the caller, extracted from the `sintade_session` cookie. Any route
/// that takes this as an argument requires a valid, unexpired session -- axum runs the
/// extractor before the handler body, so there is no way to reach handler code without one.
pub struct SessionClaims {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
}

impl FromRequestParts<AppState> for SessionClaims {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cookie_header = parts
            .headers
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");

        let token = read_cookie(cookie_header, ACCESS_COOKIE_NAME).ok_or_else(|| {
            ApiError::from(AppError::Unauthorized("missing session cookie".to_string()))
        })?;

        let claims = state
            .identity
            .verify_access_cookie(token)
            .map_err(|error| {
                let detail = match error {
                    AccessTokenError::Malformed => "malformed session cookie",
                    AccessTokenError::BadSignature => "invalid session cookie",
                    AccessTokenError::Expired => "session expired",
                };
                ApiError::from(AppError::Unauthorized(detail.to_string()))
            })?;

        Ok(SessionClaims {
            user_id: claims.user_id,
            workspace_id: claims.workspace_id,
        })
    }
}

/// Builds a `Set-Cookie` header value. `HttpOnly; Secure; SameSite=Lax` per US-02 -- `Secure`
/// cookies are still sent by browsers over `http://localhost` (treated as a secure context),
/// so this doesn't need relaxing for local dev.
pub fn set_cookie_header(name: &str, value: &str, max_age_secs: i64, path: &str) -> String {
    format!("{name}={value}; Path={path}; Max-Age={max_age_secs}; HttpOnly; Secure; SameSite=Lax")
}

fn read_cookie<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header.split(';').find_map(|pair| {
        let pair = pair.trim();
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_cookie_among_several() {
        let header = "sintade_refresh=abc; sintade_session=def; other=ghi";
        assert_eq!(read_cookie(header, "sintade_session"), Some("def"));
        assert_eq!(read_cookie(header, "sintade_refresh"), Some("abc"));
        assert_eq!(read_cookie(header, "missing"), None);
    }

    #[test]
    fn reads_a_single_cookie() {
        assert_eq!(
            read_cookie("sintade_session=only-value", "sintade_session"),
            Some("only-value")
        );
    }
}
