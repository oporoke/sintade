use axum::extract::Request;
use axum::http::header::{
    CONTENT_SECURITY_POLICY, HeaderName, HeaderValue, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::middleware::Next;
use axum::response::Response;

/// `docs/design.md` §11: "CSP (no inline scripts), HSTS 1 year, X-Content-Type-Options,
/// Referrer-Policy: strict-origin-when-cross-origin, Permissions-Policy: display-capture=(self)".
/// Applied to every response, including error responses -- security headers protect the client
/// regardless of whether the request succeeded.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'self'",
        ),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("display-capture=(self)"),
    );

    response
}
