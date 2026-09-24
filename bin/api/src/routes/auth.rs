use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::AppendHeaders;
use identity::{
    ForgotPasswordRequest, LoginError, LoginRequest, RefreshError, RegisterError, RegisterRequest,
    ResetPasswordError, ResetPasswordRequest,
};
use kernel::AppError;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::error::ApiError;
use crate::session::{
    ACCESS_COOKIE_NAME, REFRESH_COOKIE_NAME, clear_cookie_header, refresh_token_from_headers,
    set_cookie_header,
};

#[derive(Debug, Deserialize)]
pub struct RegisterBody {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub message: &'static str,
}

#[tracing::instrument(skip_all)]
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterBody>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiError> {
    state
        .identity
        .register(RegisterRequest {
            email: body.email,
            password: body.password,
            display_name: body.display_name,
        })
        .await
        .map_err(|error| {
            let app_error = match error {
                RegisterError::InvalidEmail(source) => AppError::Validation(source.to_string()),
                RegisterError::InvalidPassword(source) => AppError::Validation(source.to_string()),
                RegisterError::Hash => AppError::Internal("failed to hash password".to_string()),
                RegisterError::Database(source) => {
                    tracing::error!(error = %source, "register: database error");
                    AppError::Internal("database unavailable".to_string())
                }
            };
            ApiError::from(app_error)
        })?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            message: "if the details are valid, a verification email has been sent",
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub message: &'static str,
}

type LoginCookies = AppendHeaders<[(axum::http::HeaderName, String); 2]>;

#[tracing::instrument(skip_all)]
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<(StatusCode, LoginCookies, Json<LoginResponse>), ApiError> {
    let session = state
        .identity
        .login(LoginRequest {
            email: body.email,
            password: body.password,
        })
        .await
        .map_err(|error| {
            let app_error = match error {
                LoginError::InvalidCredentials => {
                    AppError::Unauthorized("invalid email or password".to_string())
                }
                LoginError::Database(source) => {
                    tracing::error!(error = %source, "login: database error");
                    AppError::Internal("database unavailable".to_string())
                }
            };
            ApiError::from(app_error)
        })?;

    let access_cookie = set_cookie_header(
        ACCESS_COOKIE_NAME,
        &session.access_token,
        session.access_ttl.whole_seconds(),
        "/",
    );
    let refresh_cookie = set_cookie_header(
        REFRESH_COOKIE_NAME,
        &session.refresh_token,
        session.refresh_ttl.whole_seconds(),
        "/api/v1/auth",
    );

    Ok((
        StatusCode::OK,
        AppendHeaders([(SET_COOKIE, access_cookie), (SET_COOKIE, refresh_cookie)]),
        Json(LoginResponse {
            message: "logged in",
        }),
    ))
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub message: &'static str,
}

/// Keys off the refresh cookie, not `SessionClaims` -- a still-valid refresh token should be
/// usable to get a new access cookie even after the old access cookie has already expired,
/// which is the whole point of having a separate, longer-lived refresh token (US-02).
#[tracing::instrument(skip_all)]
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, LoginCookies, Json<RefreshResponse>), ApiError> {
    let raw_refresh_token = refresh_token_from_headers(&headers).ok_or_else(|| {
        ApiError::from(AppError::Unauthorized("missing refresh cookie".to_string()))
    })?;

    let result = state
        .identity
        .refresh(raw_refresh_token)
        .await
        .map_err(|error| {
            let app_error = match error {
                RefreshError::InvalidToken | RefreshError::ReuseDetected => {
                    AppError::Unauthorized("invalid or expired refresh token".to_string())
                }
                RefreshError::Database(source) => {
                    tracing::error!(error = %source, "refresh: database error");
                    AppError::Internal("database unavailable".to_string())
                }
            };
            ApiError::from(app_error)
        })?;

    let access_cookie = set_cookie_header(
        ACCESS_COOKIE_NAME,
        &result.access_token,
        result.access_ttl.whole_seconds(),
        "/",
    );
    let refresh_cookie = set_cookie_header(
        REFRESH_COOKIE_NAME,
        &result.refresh_token,
        result.refresh_ttl.whole_seconds(),
        "/api/v1/auth",
    );

    Ok((
        StatusCode::OK,
        AppendHeaders([(SET_COOKIE, access_cookie), (SET_COOKIE, refresh_cookie)]),
        Json(RefreshResponse {
            message: "session refreshed",
        }),
    ))
}

type LogoutCookies = AppendHeaders<[(axum::http::HeaderName, String); 2]>;

/// Idempotent and keyed off the refresh cookie (see `IdentityService::logout`): logging out with
/// no cookie, an already-revoked cookie, or a garbage cookie all just clear client cookies and
/// return `204`, matching the anti-enumeration style used elsewhere in this module.
#[tracing::instrument(skip_all)]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, LogoutCookies), ApiError> {
    if let Some(raw_refresh_token) = refresh_token_from_headers(&headers) {
        state
            .identity
            .logout(raw_refresh_token)
            .await
            .map_err(|error| {
                tracing::error!(%error, "logout: database error");
                ApiError::from(AppError::Internal("database unavailable".to_string()))
            })?;
    }

    let clear_access = clear_cookie_header(ACCESS_COOKIE_NAME, "/");
    let clear_refresh = clear_cookie_header(REFRESH_COOKIE_NAME, "/api/v1/auth");

    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, clear_access), (SET_COOKIE, clear_refresh)]),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordBody {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct ForgotPasswordResponse {
    pub message: &'static str,
}

/// Always `202` with the same message, whether or not the email exists (US-03) -- the identity
/// service already enforces this at the service layer, this handler just can't turn it into a
/// non-generic error either.
#[tracing::instrument(skip_all)]
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordBody>,
) -> Result<(StatusCode, Json<ForgotPasswordResponse>), ApiError> {
    state
        .identity
        .forgot_password(ForgotPasswordRequest { email: body.email })
        .await
        .map_err(|error| {
            tracing::error!(%error, "forgot_password: database error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ForgotPasswordResponse {
            message: "if that email exists, a reset link has been sent",
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordBody {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct ResetPasswordResponse {
    pub message: &'static str,
}

#[tracing::instrument(skip_all)]
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> Result<Json<ResetPasswordResponse>, ApiError> {
    state
        .identity
        .reset_password(ResetPasswordRequest {
            token: body.token,
            password: body.password,
        })
        .await
        .map_err(|error| {
            let app_error = match error {
                ResetPasswordError::InvalidToken => {
                    AppError::BadRequest("invalid or expired reset token".to_string())
                }
                ResetPasswordError::InvalidPassword(source) => {
                    AppError::Validation(source.to_string())
                }
                ResetPasswordError::Hash => {
                    AppError::Internal("failed to hash password".to_string())
                }
                ResetPasswordError::Database(source) => {
                    tracing::error!(error = %source, "reset_password: database error");
                    AppError::Internal("database unavailable".to_string())
                }
            };
            ApiError::from(app_error)
        })?;

    Ok(Json(ResetPasswordResponse {
        message: "password reset",
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use platform::{JobQueue, Mailer, SmtpMailer};
    use serde_json::json;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::test_identity;

    const TEST_ORIGIN: &str = "http://localhost:4200";
    const FROM_ADDRESS: &str = "no-reply@sintade.app";
    const MAILPIT_API: &str = "http://localhost:8025/api/v1";

    fn smtp_url() -> String {
        std::env::var("SMTP_URL").unwrap_or_else(|_| "smtp://localhost:1025".to_string())
    }

    /// Drains the one job the register call is expected to have enqueued and hands it to a
    /// real `SmtpMailer`, mirroring what `bin/worker`'s `SendEmailHandler` does -- there is no
    /// shared library between the two binaries to call directly (see Day 12 plan).
    async fn deliver_pending_email(pool: &PgPool) -> serde_json::Value {
        let queue = JobQueue::new(pool.clone());
        let claimed = queue
            .claim_next("test-worker", Duration::from_secs(30))
            .await
            .expect("claim succeeds")
            .expect("a SendEmail job was enqueued");
        assert_eq!(claimed.kind, "SendEmail");

        let mailer = SmtpMailer::new(&smtp_url(), FROM_ADDRESS).expect("smtp mailer builds");
        mailer
            .send(&platform::EmailMessage {
                to: claimed.payload["to"]
                    .as_str()
                    .expect("to field")
                    .to_string(),
                subject: claimed.payload["subject"]
                    .as_str()
                    .expect("subject field")
                    .to_string(),
                text_body: claimed.payload["body"]
                    .as_str()
                    .expect("body field")
                    .to_string(),
            })
            .await
            .expect("smtp send succeeds");
        queue.complete(claimed.id).await.expect("complete succeeds");
        claimed.payload
    }

    async fn find_mailpit_message_to(to: &str) -> Option<serde_json::Value> {
        let client = reqwest::Client::new();
        for _ in 0..20 {
            let messages: serde_json::Value = client
                .get(format!("{MAILPIT_API}/messages"))
                .send()
                .await
                .expect("mailpit reachable")
                .json()
                .await
                .expect("valid json from mailpit");
            if let Some(found) = messages["messages"]
                .as_array()
                .and_then(|list| list.iter().find(|m| m["To"][0]["Address"] == to).cloned())
            {
                return Some(found);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        None
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn register_creates_rows_and_sends_verification_email(pool: PgPool) {
        let email = format!("day12-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "Day Twelve Tester",
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::CREATED);

        let user_row = sqlx::query!("SELECT id FROM users WHERE email = $1", email)
            .fetch_one(&pool)
            .await
            .expect("user row exists");

        sqlx::query!(
            "SELECT 1 as one FROM credentials WHERE user_id = $1",
            user_row.id
        )
        .fetch_one(&pool)
        .await
        .expect("credential row exists");

        let membership = sqlx::query!(
            "SELECT workspace_id FROM memberships WHERE user_id = $1 AND role = 'owner'",
            user_row.id
        )
        .fetch_one(&pool)
        .await
        .expect("owner membership row exists");

        sqlx::query!(
            "SELECT 1 as one FROM workspaces WHERE id = $1 AND is_personal = true",
            membership.workspace_id
        )
        .fetch_one(&pool)
        .await
        .expect("personal workspace row exists");

        sqlx::query!(
            "SELECT 1 as one FROM email_tokens WHERE user_id = $1 AND purpose = 'verify_email'",
            user_row.id
        )
        .fetch_one(&pool)
        .await
        .expect("verify_email token row exists");

        let payload = deliver_pending_email(&pool).await;
        assert_eq!(payload["to"], email);

        let mailpit_message = find_mailpit_message_to(&email)
            .await
            .expect("verification email arrived in Mailpit");
        assert_eq!(
            mailpit_message["Subject"], "Verify your Sintade account",
            "mailpit message subject matches"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn register_with_duplicate_email_returns_same_response_no_enumeration(pool: PgPool) {
        let email = format!("day12-dup-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "First Registration",
        });
        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(first.status(), StatusCode::CREATED);

        // drain the first job so the second attempt starts from a clean queue
        deliver_pending_email(&pool).await;

        let second_body = json!({
            "email": email,
            "password": "another-strong-password-99",
            "display_name": "Second Attempt",
        });
        let second = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(second_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            second.status(),
            StatusCode::CREATED,
            "duplicate email returns the same success response, not an error"
        );

        let user_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) as \"count!\" FROM users WHERE email = $1",
            email
        )
        .fetch_one(&pool)
        .await
        .expect("count query succeeds");
        assert_eq!(user_count, 1, "no second user row was created");

        let queue = JobQueue::new(pool.clone());
        let no_second_job = queue
            .claim_next("test-worker", Duration::from_secs(30))
            .await
            .expect("claim succeeds");
        assert!(
            no_second_job.is_none(),
            "no second verification email was enqueued"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn register_with_short_password_returns_422(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);

        let body = json!({
            "email": "day12-short-pw@example.com",
            "password": "short1",
            "display_name": "Too Short",
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Registers a user via the real HTTP handler, then returns the two `Set-Cookie` values
    /// (bare `name=value`, attributes stripped) from a successful login.
    async fn register_and_login(pool: &PgPool, email: &str) -> (String, String) {
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let register_body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "Refresh Route Tester",
        });
        let register_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(register_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(register_response.status(), StatusCode::CREATED);

        let login_body = json!({"email": email, "password": "correct-horse-battery-staple-42"});
        let login_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(login_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(login_response.status(), StatusCode::OK);

        let set_cookies: Vec<String> = login_response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .map(|value| {
                let raw = value.to_str().expect("valid header string");
                raw.split_once(';').map_or(raw, |(kv, _)| kv).to_string()
            })
            .collect();

        let access = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_session="))
            .expect("access cookie present")
            .clone();
        let refresh = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_refresh="))
            .expect("refresh cookie present")
            .clone();

        (access, refresh)
    }

    fn set_cookies_from(response: &axum::http::Response<Body>) -> (String, String) {
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .map(|value| {
                let raw = value.to_str().expect("valid header string");
                raw.split_once(';').map_or(raw, |(kv, _)| kv).to_string()
            })
            .collect();
        let access = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_session="))
            .expect("access cookie present")
            .clone();
        let refresh = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_refresh="))
            .expect("refresh cookie present")
            .clone();
        (access, refresh)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn refresh_rotates_and_replaying_old_token_revokes_family(pool: PgPool) {
        let email = format!("day14-{}@example.com", uuid::Uuid::now_v7());
        let (_, refresh_a) = register_and_login(&pool, &email).await;
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        // first refresh: rotates A -> B, succeeds
        let first_refresh = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", &refresh_a)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(first_refresh.status(), StatusCode::OK);
        let (_, refresh_b) = set_cookies_from(&first_refresh);
        assert_ne!(refresh_a, refresh_b, "rotation issues a new refresh token");

        // replaying the retired token A is reuse -- must fail and nuke the whole family
        let replay = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", &refresh_a)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            replay.status(),
            StatusCode::UNAUTHORIZED,
            "replaying a rotated-away refresh token is rejected"
        );

        // B was the legitimate, still-unused token from the first rotation -- reuse detection
        // must have revoked it too, not just A
        let after_family_revoked = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", &refresh_b)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            after_family_revoked.status(),
            StatusCode::UNAUTHORIZED,
            "replaying an old token revokes the whole family, including its other member"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn refresh_with_missing_cookie_returns_401(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn refresh_with_garbage_token_returns_401(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", "sintade_refresh=not-a-real-token")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_revokes_session_and_subsequent_refresh_fails(pool: PgPool) {
        let email = format!("day14-logout-{}@example.com", uuid::Uuid::now_v7());
        let (_, refresh_token) = register_and_login(&pool, &email).await;
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let logout_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header("cookie", &refresh_token)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);

        let clear_cookies: Vec<String> = logout_response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("valid header string").to_string())
            .collect();
        assert!(
            clear_cookies
                .iter()
                .any(|c| c.starts_with("sintade_session=;"))
        );
        assert!(
            clear_cookies
                .iter()
                .any(|c| c.starts_with("sintade_refresh=;"))
        );

        let refresh_after_logout = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", &refresh_token)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            refresh_after_logout.status(),
            StatusCode::UNAUTHORIZED,
            "the revoked session can no longer be refreshed"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_without_cookie_returns_204(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    /// Claims the next enqueued `SendEmail` job and pulls the `?token=...` value out of its
    /// body link, standing in for "the user clicked the link in their inbox".
    async fn claim_email_link_token(pool: &PgPool) -> String {
        let queue = JobQueue::new(pool.clone());
        let claimed = queue
            .claim_next("test-worker", Duration::from_secs(30))
            .await
            .expect("claim succeeds")
            .expect("a SendEmail job was enqueued");
        assert_eq!(claimed.kind, "SendEmail");
        queue.complete(claimed.id).await.expect("complete succeeds");

        let body = claimed.payload["body"]
            .as_str()
            .expect("body field")
            .to_string();
        let (_, token) = body
            .split_once("token=")
            .expect("body contains a token link");
        token.to_string()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn forgot_password_with_unknown_email_returns_same_response_no_job_enqueued(
        pool: PgPool,
    ) {
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let body = json!({"email": "no-such-account-day15@example.com"});
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/forgot")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let queue = JobQueue::new(pool.clone());
        let job = queue
            .claim_next("test-worker", Duration::from_secs(30))
            .await
            .expect("claim succeeds");
        assert!(
            job.is_none(),
            "no reset email is enqueued for an unknown address"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn reset_password_flow_changes_password_revokes_sessions_and_is_single_use(pool: PgPool) {
        let email = format!("day15-reset-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        // register, then log in so there's a live session to prove gets revoked
        let (_, refresh_token) = register_and_login(&pool, &email).await;
        // drain the verify-email job so it doesn't get mistaken for the reset job below
        claim_email_link_token(&pool).await;

        let forgot_body = json!({"email": email});
        let forgot_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/forgot")
                    .header("content-type", "application/json")
                    .body(Body::from(forgot_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(forgot_response.status(), StatusCode::ACCEPTED);

        let reset_token = claim_email_link_token(&pool).await;

        let new_password = "a-brand-new-strong-password-77";
        let reset_body = json!({"token": reset_token, "password": new_password});
        let reset_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/reset")
                    .header("content-type", "application/json")
                    .body(Body::from(reset_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(reset_response.status(), StatusCode::OK);

        // old session must be revoked -- refreshing with the pre-reset refresh cookie fails
        let refresh_after_reset = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", &refresh_token)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            refresh_after_reset.status(),
            StatusCode::UNAUTHORIZED,
            "sessions are revoked on password reset"
        );

        // old password no longer works
        let login_old = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"email": email, "password": "correct-horse-battery-staple-42"})
                            .to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(login_old.status(), StatusCode::UNAUTHORIZED);

        // new password works
        let login_new = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"email": email, "password": new_password}).to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(login_new.status(), StatusCode::OK);

        // the reset token is single-use
        let replay = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/reset")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"token": reset_token, "password": "yet-another-password-88"})
                            .to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn reset_password_with_short_password_returns_422(pool: PgPool) {
        let email = format!("day15-reset-short-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);
        register_and_login(&pool, &email).await;
        claim_email_link_token(&pool).await; // drain verify-email job

        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/forgot")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"email": email}).to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        let reset_token = claim_email_link_token(&pool).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/reset")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"token": reset_token, "password": "short1"}).to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn reset_password_with_garbage_token_returns_400(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/reset")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"token": "not-a-real-token", "password": "a-fine-password-99"})
                            .to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
