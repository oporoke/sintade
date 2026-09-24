use axum::http::HeaderMap;
use kernel::AppError;
use uuid::Uuid;

use crate::error::ApiError;
use crate::session::cookie_from_headers;

pub const CSRF_COOKIE_NAME: &str = "sintade_csrf";
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";

pub fn generate_csrf_token() -> String {
    Uuid::new_v4().to_string()
}

/// Double-submit CSRF check (`docs/design.md` §11: "SameSite cookies + double-submit token on
/// state-changing requests"). Only applied to `/auth/refresh` and `/auth/logout`: every other
/// state-changing route (register, login, verify-email, forgot/reset-password) requires a
/// secret in the request body (a password or a token) that a cross-site attacker can't supply
/// on a victim's behalf, so CSRF against them doesn't gain an attacker anything they couldn't
/// already do directly by calling the API themselves. Refresh and logout act purely on the
/// ambient refresh cookie, which *is* exactly the kind of "browser sends it automatically"
/// credential CSRF exists to abuse.
pub fn verify_csrf(headers: &HeaderMap) -> Result<(), ApiError> {
    let cookie_value = cookie_from_headers(headers, CSRF_COOKIE_NAME);
    let header_value = headers
        .get(CSRF_HEADER_NAME)
        .and_then(|value| value.to_str().ok());

    match (cookie_value, header_value) {
        (Some(cookie), Some(header)) if cookie == header => Ok(()),
        _ => Err(ApiError::from(AppError::Forbidden(
            "missing or invalid CSRF token".to_string(),
        ))),
    }
}
