use axum::http::HeaderMap;
use kernel::AppError;
use uuid::Uuid;

use crate::error::ApiError;
use crate::session::cookie_from_headers;

pub const CSRF_COOKIE_NAME: &str = "sintade_csrf";
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";
/// Sent by the browser extension on every request (`extension/<version>`). A cross-site page
/// cannot add a custom header without a CORS preflight, and the API's CORS policy allows only the
/// app's own origin and does not list this header, so its presence proves the request came from
/// an extension page (OWASP: "custom request headers" defence). The extension can't read the
/// `sintade_csrf` cookie without the `cookies` permission, which the store review would question.
pub const CLIENT_HEADER_NAME: &str = "x-sintade-client";

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
    if is_extension_request(headers) {
        return Ok(());
    }
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

/// `X-Sintade-Client: extension/<version>`. Anything else in the header doesn't count.
fn is_extension_request(headers: &HeaderMap) -> bool {
    headers
        .get(CLIENT_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("extension/") && value.len() > "extension/".len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        map
    }

    #[test]
    fn double_submit_still_works() {
        let ok = headers(&[("cookie", "sintade_csrf=abc"), ("x-csrf-token", "abc")]);
        assert!(verify_csrf(&ok).is_ok());
    }

    #[test]
    fn a_mismatch_or_nothing_is_refused() {
        assert!(verify_csrf(&HeaderMap::new()).is_err());
        let wrong = headers(&[("cookie", "sintade_csrf=abc"), ("x-csrf-token", "xyz")]);
        assert!(verify_csrf(&wrong).is_err());
        let header_only = headers(&[("x-csrf-token", "abc")]);
        assert!(verify_csrf(&header_only).is_err());
    }

    #[test]
    fn the_extension_header_stands_in_for_the_cookie() {
        assert!(verify_csrf(&headers(&[("x-sintade-client", "extension/0.2.0")])).is_ok());
    }

    #[test]
    fn other_values_of_the_client_header_do_not() {
        for value in [
            "",
            "extension/",
            "web/1.0",
            "extension",
            "browser extension/1",
        ] {
            let refused = verify_csrf(&headers(&[("x-sintade-client", value)]));
            assert!(refused.is_err(), "{value:?}");
        }
    }
}
