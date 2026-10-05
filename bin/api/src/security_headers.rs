use axum::extract::Request;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, HeaderName, HeaderValue, REFERRER_POLICY,
    STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::middleware::Next;
use axum::response::Response;

/// `docs/design.md` §11: "CSP (no inline scripts), HSTS 1 year, X-Content-Type-Options,
/// Referrer-Policy: strict-origin-when-cross-origin, Permissions-Policy: display-capture=(self)".
/// Applied to every response, including error responses -- security headers protect the client
/// regardless of whether the request succeeded.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let is_api = request.uri().path().starts_with("/api/");
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
            "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'self'; \
             frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(
            "display-capture=(self), microphone=(self), camera=(self), geolocation=(), payment=(), usb=()",
        ),
    );
    // Not embeddable anywhere (clickjacking); the CSP's frame-ancestors covers modern browsers.
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    // API answers are per-user (and some carry signed URLs): never stored by a cache, unless the
    // handler said something more specific.
    if is_api && !headers.contains_key(CACHE_CONTROL) {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }

    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;

    async fn headers_of(uri: &str) -> axum::http::HeaderMap {
        let app = Router::new()
            .route("/api/v1/plain", get(|| async { "x" }))
            .route(
                "/api/v1/cached",
                get(|| async { ([(CACHE_CONTROL, "private, max-age=5")], "x") }),
            )
            .route("/healthz", get(|| async { "ok" }))
            .layer(middleware::from_fn(security_headers));
        app.oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .headers()
        .clone()
    }

    #[tokio::test]
    async fn every_response_carries_the_hardening_headers() {
        for uri in ["/api/v1/plain", "/healthz", "/nowhere"] {
            let h = headers_of(uri).await;
            let get = |name: &str| {
                h.get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string()
            };
            assert_eq!(
                get("strict-transport-security"),
                "max-age=31536000",
                "{uri}"
            );
            assert_eq!(get("x-content-type-options"), "nosniff");
            assert_eq!(get("x-frame-options"), "DENY");
            assert_eq!(get("referrer-policy"), "strict-origin-when-cross-origin");
            assert_eq!(get("cross-origin-opener-policy"), "same-origin");
            assert_eq!(get("cross-origin-resource-policy"), "same-origin");
            let csp = get("content-security-policy");
            for directive in [
                "default-src 'self'",
                "script-src 'self'",
                "object-src 'none'",
                "frame-ancestors 'none'",
                "base-uri 'self'",
                "form-action 'self'",
            ] {
                assert!(csp.contains(directive), "{directive} in {csp}");
            }
            assert!(
                !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
                "{csp}"
            );
            let pp = get("permissions-policy");
            assert!(
                pp.contains("display-capture=(self)") && pp.contains("geolocation=()"),
                "{pp}"
            );
        }
    }

    #[tokio::test]
    async fn api_answers_are_not_cached_unless_the_handler_says_otherwise() {
        let plain = headers_of("/api/v1/plain").await;
        assert_eq!(plain.get(CACHE_CONTROL).expect("cache-control"), "no-store");
        let cached = headers_of("/api/v1/cached").await;
        assert_eq!(
            cached.get(CACHE_CONTROL).expect("cache-control"),
            "private, max-age=5"
        );
        assert!(headers_of("/healthz").await.get(CACHE_CONTROL).is_none());
    }
}
