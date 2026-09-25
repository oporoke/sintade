use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use identity::IdentityService;
use kernel::AppError;
use platform::{Clock, RateLimiter};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::routes::{auth, me, verify_email};
use crate::security_headers::security_headers;

const REQUEST_ID_HEADER: &str = "x-request-id";
const MAX_BODY_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub identity: Arc<IdentityService>,
    pub rate_limiter: Arc<RateLimiter>,
    pub clock: Arc<dyn Clock>,
}

pub fn build_router(
    pool: PgPool,
    identity: Arc<IdentityService>,
    rate_limiter: Arc<RateLimiter>,
    clock: Arc<dyn Clock>,
    public_base_url: &str,
) -> Router {
    let request_id_header = axum::http::HeaderName::from_static(REQUEST_ID_HEADER);
    // `allow_credentials(true)` is required for the browser to send/receive cookies on
    // cross-origin requests (the SPA on :4200 talking to the API on :8080 in dev) -- the CORS
    // spec forbids combining credentials with a wildcard origin or header list, so both must be
    // an explicit, non-`Any` list once this is on.
    let cors = CorsLayer::new()
        .allow_origin(
            public_base_url
                .parse::<HeaderValue>()
                .expect("PUBLIC_BASE_URL must be a valid header value"),
        )
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-csrf-token"),
        ])
        .allow_credentials(true);

    let state = AppState {
        pool,
        identity,
        rate_limiter,
        clock,
    };

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/auth/register", post(auth::register))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/refresh", post(auth::refresh))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route(
            "/api/v1/auth/verify-email",
            post(verify_email::verify_email),
        )
        .route("/api/v1/auth/password/forgot", post(auth::forgot_password))
        .route("/api/v1/auth/password/reset", post(auth::reset_password))
        .route("/api/v1/me", get(me::me))
        .fallback(not_found)
        .with_state(state)
        // axum's Router::layer wraps outward on each call (the *last* .layer() ends up
        // outermost, seeing the request first) -- the reverse of tower::ServiceBuilder. This
        // chain is written innermost-first so the request actually flows, outer to inner:
        // BodyLimit -> Timeout -> Cors -> SecurityHeaders -> SetRequestId -> Trace ->
        // PropagateRequestId -> routing.
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(middleware::from_fn(security_headers))
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

async fn healthz() -> StatusCode {
    StatusCode::OK
}

async fn readyz(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    sqlx::query!("SELECT 1 as one")
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "readyz db check failed");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?;
    Ok(StatusCode::OK)
}

async fn not_found() -> ApiError {
    ApiError::from(AppError::NotFound)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    const TEST_ORIGIN: &str = "http://localhost:4200";
    const TEST_SESSION_SECRET: &[u8] = b"test-session-secret-at-least-32-bytes-long";

    pub(crate) fn test_identity(pool: PgPool) -> Arc<IdentityService> {
        let queue = platform::JobQueue::new(pool.clone());
        Arc::new(IdentityService::new(
            pool,
            queue,
            Arc::new(platform::SystemClock),
            TEST_ORIGIN.to_string(),
            TEST_SESSION_SECRET.to_vec(),
        ))
    }

    pub(crate) fn test_rate_limiter(pool: PgPool) -> Arc<RateLimiter> {
        Arc::new(RateLimiter::new(pool))
    }

    pub(crate) fn test_clock() -> Arc<dyn Clock> {
        Arc::new(platform::SystemClock)
    }

    /// A clock frozen at a fixed instant, for tests (e.g. rate-limit window boundaries) that
    /// would otherwise flake if wall-clock time happened to cross a window edge mid-test.
    pub(crate) fn test_fixed_clock() -> Arc<dyn Clock> {
        Arc::new(platform::FixedClock(
            time::OffsetDateTime::from_unix_timestamp(1_800_000_000)
                .expect("valid fixed timestamp"),
        ))
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn healthz_returns_200(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn readyz_returns_200_with_real_db_check(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unknown_route_returns_problem_json_with_request_id(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/nope")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .expect("content-type header present"),
            "application/problem+json",
        );
        let request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .expect("x-request-id header present");
        assert!(!request_id.is_empty());

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid json body");
        assert_eq!(json["status"], 404);
        assert_eq!(json["title"], "Not Found");
    }
}
