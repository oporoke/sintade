use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use kernel::AppError;

use crate::app::AppState;
use crate::client_ip::client_ip;
use crate::error::ApiError;
use crate::session::{ACCESS_COOKIE_NAME, cookie_from_headers};

/// Who a limit is counted against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// The client address (docs/design.md §11: public watch 120/min/IP).
    Ip,
    /// The signed-in user, falling back to the address for callers with no valid session.
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub name: &'static str,
    pub window: Duration,
    pub limit: u32,
    pub subject: Subject,
}

/// Multiplies every limit in this module (not the login and signup limits, which are about
/// guessing and abuse, not load). 1 in production. The dev server and the e2e suite send all of
/// their traffic from one address, so they run with `RATE_LIMIT_SCALE=20`.
static SCALE: AtomicU32 = AtomicU32::new(1);

pub fn set_scale(scale: u32) {
    SCALE.store(scale.clamp(1, 100), Ordering::Relaxed);
}

const MINUTE: Duration = Duration::from_secs(60);

const WATCH: Policy = Policy {
    name: "watch",
    window: MINUTE,
    limit: 120,
    subject: Subject::Ip,
};
const INGEST: Policy = Policy {
    name: "ingest",
    window: MINUTE,
    limit: 600,
    subject: Subject::User,
};
/// Every auth endpoint, on top of login's 5/min/IP+email and signup's 3/hour/IP.
const AUTH: Policy = Policy {
    name: "auth",
    window: MINUTE,
    limit: 60,
    subject: Subject::Ip,
};
/// Everything else under `/api/v1`, and anything that matches no route (scanners).
const API: Policy = Policy {
    name: "api",
    window: MINUTE,
    limit: 600,
    subject: Subject::User,
};

/// The limit a request falls under; `None` only for the liveness/readiness probes. A request
/// matching no route still gets one (`API`), so probing for routes is rate limited too.
pub fn classify(_method: &Method, path: &str) -> Option<Policy> {
    if path == "/healthz" || path == "/readyz" {
        return None;
    }
    Some(if path.starts_with("/api/v1/s/") {
        WATCH
    } else if path.starts_with("/api/v1/takes/") {
        INGEST
    } else if path.starts_with("/api/v1/auth/") {
        AUTH
    } else {
        API
    })
}

/// Counts every request against its policy and answers `429` with `Retry-After` once the limit
/// is reached. If the counter store itself fails the request goes through (an outage of the
/// limiter must not become an outage of the API) and the failure is logged.
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(policy) = classify(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let headers = request.headers();
    let key = match policy.subject {
        Subject::Ip => format!("rl:{}:ip:{}", policy.name, client_ip(headers)),
        Subject::User => {
            let user = cookie_from_headers(headers, ACCESS_COOKIE_NAME)
                .and_then(|token| state.identity.verify_access_cookie(token).ok())
                .map(|claims| claims.user_id);
            match user {
                Some(user) => format!("rl:{}:user:{user}", policy.name),
                None => format!("rl:{}:ip:{}", policy.name, client_ip(headers)),
            }
        }
    };
    let now = state.clock.now();
    let limit = policy.limit.saturating_mul(SCALE.load(Ordering::Relaxed));
    match state
        .rate_limiter
        .check(&key, policy.window, limit, now)
        .await
    {
        Ok(true) => next.run(request).await,
        Ok(false) => {
            let window = i64::try_from(policy.window.as_secs()).unwrap_or(60).max(1);
            let retry_after = window - now.unix_timestamp().rem_euclid(window);
            let mut response = ApiError::from(AppError::RateLimited).into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
        Err(error) => {
            tracing::error!(%error, policy = policy.name, "rate limiter unavailable; letting the request through");
            next.run(request).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes;

    #[test]
    fn requests_fall_under_the_documented_limits() {
        let get = Method::GET;
        assert_eq!(
            classify(&get, "/api/v1/s/abcdefghijkl").map(|p| p.name),
            Some("watch")
        );
        assert_eq!(
            classify(&get, "/api/v1/s/abcdefghijkl/playback").map(|p| p.limit),
            Some(120)
        );
        assert_eq!(
            classify(&Method::POST, "/api/v1/takes/x/chunks/0/url").map(|p| p.name),
            Some("ingest")
        );
        assert_eq!(
            classify(&Method::POST, "/api/v1/auth/login").map(|p| p.name),
            Some("auth")
        );
        assert_eq!(
            classify(&get, "/api/v1/recordings").map(|p| p.name),
            Some("api")
        );
        assert_eq!(
            classify(&get, "/anything/else").map(|p| p.name),
            Some("api")
        );
        assert_eq!(classify(&get, "/healthz"), None);
        assert_eq!(classify(&get, "/readyz"), None);
    }

    #[test]
    fn the_public_watch_limit_is_the_designed_one() {
        let watch = classify(&Method::GET, "/api/v1/s/x").expect("limited");
        assert_eq!(
            (watch.limit, watch.window, watch.subject),
            (120, MINUTE, Subject::Ip)
        );
        let presign = classify(&Method::POST, "/api/v1/takes/t/chunks/1/url").expect("limited");
        assert_eq!(
            (presign.limit, presign.window, presign.subject),
            (600, MINUTE, Subject::User)
        );
    }

    /// No route escapes: every mounted route is limited, apart from the two probes.
    #[test]
    fn every_route_in_the_table_is_rate_limited() {
        for route in routes::table() {
            let probe = matches!(route.path, "/healthz" | "/readyz");
            let concrete = route.path.replace(['{', '}'], "");
            assert_eq!(
                classify(&route.method, &concrete).is_none(),
                probe,
                "{} {}",
                route.method,
                route.path
            );
        }
    }

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::{
        test_fixed_clock, test_identity, test_ingest, test_rate_limiter, test_store, test_tenancy,
    };

    fn app(pool: &PgPool) -> axum::Router {
        build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_ingest(pool.clone()),
            test_store(),
            test_rate_limiter(pool.clone()),
            // Frozen, so a test can't straddle the end of a one-minute window.
            test_fixed_clock(),
            "http://localhost:4200",
        )
    }

    async fn hit(
        app: &axum::Router,
        uri: &str,
        forwarded: Option<&str>,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::builder().uri(uri);
        if let Some(value) = forwarded {
            request = request.header("x-forwarded-for", value);
        }
        if let Some(value) = cookie {
            request = request.header("cookie", value);
        }
        app.clone()
            .oneshot(request.body(Body::empty()).expect("request"))
            .await
            .expect("response")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_public_watch_limit_is_120_a_minute_per_address(pool: PgPool) {
        let app = app(&pool);
        for n in 0..120 {
            let response = hit(&app, "/api/v1/s/abcdefghijkl", Some("203.0.113.1"), None).await;
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "request {n} is served"
            );
        }
        let limited = hit(&app, "/api/v1/s/abcdefghijkl", Some("203.0.113.1"), None).await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry: i64 = limited
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .expect("Retry-After seconds");
        assert!((1..=60).contains(&retry), "{retry}");
        assert_eq!(
            limited
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json")
        );

        // Another address is untouched, and the playback route shares the watch bucket.
        let other = hit(
            &app,
            "/api/v1/s/abcdefghijkl/playback",
            Some("203.0.113.2"),
            None,
        )
        .await;
        assert_eq!(other.status(), StatusCode::NOT_FOUND);
        let playback = hit(
            &app,
            "/api/v1/s/abcdefghijkl/playback",
            Some("203.0.113.1"),
            None,
        )
        .await;
        assert_eq!(playback.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_spoofed_leftmost_address_does_not_buy_a_fresh_bucket(pool: PgPool) {
        let app = app(&pool);
        // The proxy's own address is last; the client controls everything before it.
        for n in 0..120 {
            let spoof = format!("10.0.0.{n}, 198.51.100.7");
            hit(&app, "/api/v1/s/abcdefghijkl", Some(&spoof), None).await;
        }
        let response = hit(
            &app,
            "/api/v1/s/abcdefghijkl",
            Some("9.9.9.9, 198.51.100.7"),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn probes_are_never_limited(pool: PgPool) {
        let app = app(&pool);
        for _ in 0..700 {
            let response = hit(&app, "/healthz", Some("203.0.113.5"), None).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unknown_paths_are_limited_too(pool: PgPool) {
        let app = app(&pool);
        for _ in 0..600 {
            hit(&app, "/wp-login.php", Some("203.0.113.6"), None).await;
        }
        let response = hit(&app, "/wp-login.php", Some("203.0.113.6"), None).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn signed_in_callers_are_counted_per_user_not_per_address(pool: PgPool) {
        use crate::routes::testkit::caller;
        let alice = caller(&pool).await;
        let bob = caller(&pool).await;
        let app = app(&pool);
        // Same address for both: only Alice spends her 600.
        for _ in 0..600 {
            hit(
                &app,
                "/api/v1/recordings",
                Some("203.0.113.8"),
                Some(&alice.cookie),
            )
            .await;
        }
        let alice_next = hit(
            &app,
            "/api/v1/recordings",
            Some("203.0.113.8"),
            Some(&alice.cookie),
        )
        .await;
        assert_eq!(alice_next.status(), StatusCode::TOO_MANY_REQUESTS);
        let bob_next = hit(
            &app,
            "/api/v1/recordings",
            Some("203.0.113.8"),
            Some(&bob.cookie),
        )
        .await;
        assert_eq!(bob_next.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_broken_limiter_lets_requests_through(pool: PgPool) {
        sqlx::query!("DROP TABLE rate_limit_buckets")
            .execute(&pool)
            .await
            .expect("drop");
        let app = app(&pool);
        let response = hit(&app, "/api/v1/recordings", Some("203.0.113.9"), None).await;
        // Served (and refused for want of a session), not an outage and not a 429.
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
