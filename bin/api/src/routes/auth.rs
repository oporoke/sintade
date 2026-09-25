use std::time::Duration;

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
use serde::Deserialize;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::csrf::{CSRF_COOKIE_NAME, generate_csrf_token, verify_csrf};
use crate::error::{ApiError, Problem};
use crate::routes::MessageResponse;
use crate::session::SessionClaims;
use crate::session::{
    ACCESS_COOKIE_NAME, REFRESH_COOKIE_NAME, clear_cookie_header, refresh_token_from_headers,
    set_cookie_header,
};

const LOGIN_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_RATE_LIMIT: u32 = 5;
const SIGNUP_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(3600);
const SIGNUP_RATE_LIMIT: u32 = 3;

/// `docs/design.md` §11 rate limits are per-IP (or IP+email); this trusts `X-Forwarded-For`
/// rather than the raw TCP peer address, matching the architecture (Cloudflare/BunnyCDN always
/// sits in front per §2) -- direct-to-origin traffic without that header all share one
/// "unknown" bucket, which is the conservative failure mode (stricter, not laxer, than intended).
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|ip| ip.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterBody {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/register",
    tag = "auth",
    request_body = RegisterBody,
    responses(
        (status = 201, description = "Accepted; a verification email is sent if the details are valid", body = MessageResponse),
        (status = 422, description = "Invalid email, password or display name", body = Problem, content_type = "application/problem+json"),
        (status = 429, description = "Signup rate limit hit", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> Result<(StatusCode, Json<MessageResponse>), ApiError> {
    let key = format!("signup:{}", client_ip(&headers));
    let allowed = state
        .rate_limiter
        .check(
            &key,
            SIGNUP_RATE_LIMIT_WINDOW,
            SIGNUP_RATE_LIMIT,
            state.clock.now(),
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "register: rate limiter error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?;
    if !allowed {
        return Err(ApiError::from(AppError::RateLimited));
    }

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
                RegisterError::InvalidDisplayName(source) => {
                    AppError::Validation(source.to_string())
                }
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
        Json(MessageResponse {
            message: "if the details are valid, a verification email has been sent",
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

type SessionCookies = AppendHeaders<[(axum::http::HeaderName, String); 3]>;

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginBody,
    responses(
        (status = 200, description = "Logged in; sets session, refresh and CSRF cookies", body = MessageResponse),
        (status = 401, description = "Invalid email or password", body = Problem, content_type = "application/problem+json"),
        (status = 429, description = "Login rate limit hit", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Result<(StatusCode, SessionCookies, Json<MessageResponse>), ApiError> {
    let key = format!(
        "login:{}:{}",
        client_ip(&headers),
        body.email.trim().to_lowercase()
    );
    let allowed = state
        .rate_limiter
        .check(
            &key,
            LOGIN_RATE_LIMIT_WINDOW,
            LOGIN_RATE_LIMIT,
            state.clock.now(),
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "login: rate limiter error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?;
    if !allowed {
        return Err(ApiError::from(AppError::RateLimited));
    }

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
    // Not HttpOnly: the SPA must be able to read this to echo it back as X-CSRF-Token.
    let csrf_cookie = format!(
        "{CSRF_COOKIE_NAME}={}; Path=/; Max-Age={}; Secure; SameSite=Lax",
        generate_csrf_token(),
        session.refresh_ttl.whole_seconds(),
    );

    Ok((
        StatusCode::OK,
        AppendHeaders([
            (SET_COOKIE, access_cookie),
            (SET_COOKIE, refresh_cookie),
            (SET_COOKIE, csrf_cookie),
        ]),
        Json(MessageResponse {
            message: "logged in",
        }),
    ))
}

/// Keys off the refresh cookie, not `SessionClaims` -- a still-valid refresh token should be
/// usable to get a new access cookie even after the old access cookie has already expired,
/// which is the whole point of having a separate, longer-lived refresh token (US-02). Acts
/// purely on an ambient cookie, so it's one of the two routes that need the CSRF double-submit
/// check (see `crate::csrf`).
#[utoipa::path(
    post,
    path = "/api/v1/auth/refresh",
    tag = "auth",
    responses(
        (status = 200, description = "Rotated; sets new session, refresh and CSRF cookies", body = MessageResponse),
        (status = 401, description = "Missing, invalid or reused refresh token", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, SessionCookies, Json<MessageResponse>), ApiError> {
    let raw_refresh_token = refresh_token_from_headers(&headers).ok_or_else(|| {
        ApiError::from(AppError::Unauthorized("missing refresh cookie".to_string()))
    })?;
    verify_csrf(&headers)?;

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
    let csrf_cookie = format!(
        "{CSRF_COOKIE_NAME}={}; Path=/; Max-Age={}; Secure; SameSite=Lax",
        generate_csrf_token(),
        result.refresh_ttl.whole_seconds(),
    );

    Ok((
        StatusCode::OK,
        AppendHeaders([
            (SET_COOKIE, access_cookie),
            (SET_COOKIE, refresh_cookie),
            (SET_COOKIE, csrf_cookie),
        ]),
        Json(MessageResponse {
            message: "session refreshed",
        }),
    ))
}

type LogoutCookies = AppendHeaders<[(axum::http::HeaderName, String); 3]>;

/// Idempotent and keyed off the refresh cookie (see `IdentityService::logout`): logging out with
/// no cookie, an already-revoked cookie, or a garbage cookie all just clear client cookies and
/// return `204`, matching the anti-enumeration style used elsewhere in this module. Also acts on
/// an ambient cookie, so it needs the CSRF double-submit check too.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 204, description = "Session revoked (idempotent); cookies cleared"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, LogoutCookies), ApiError> {
    if let Some(raw_refresh_token) = refresh_token_from_headers(&headers) {
        verify_csrf(&headers)?;
        state
            .identity
            .logout(raw_refresh_token)
            .await
            .map_err(|error| {
                tracing::error!(%error, "logout: database error");
                ApiError::from(AppError::Internal("database unavailable".to_string()))
            })?;
    }

    Ok((StatusCode::NO_CONTENT, clear_session_cookies()))
}

fn clear_session_cookies() -> LogoutCookies {
    AppendHeaders([
        (SET_COOKIE, clear_cookie_header(ACCESS_COOKIE_NAME, "/")),
        (
            SET_COOKIE,
            clear_cookie_header(REFRESH_COOKIE_NAME, "/api/v1/auth"),
        ),
        (SET_COOKIE, clear_cookie_header(CSRF_COOKIE_NAME, "/")),
    ])
}

/// "Log out everywhere": revokes every refresh session for the caller and clears this
/// browser's cookies. Needs a live session (it acts on the user, not one refresh token) and
/// the CSRF check. Other devices' access cookies lapse within `ACCESS_TOKEN_TTL` (15 min).
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout-all",
    tag = "auth",
    responses(
        (status = 204, description = "Every session revoked; cookies cleared"),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn logout_all(
    State(state): State<AppState>,
    claims: SessionClaims,
    headers: HeaderMap,
) -> Result<(StatusCode, LogoutCookies), ApiError> {
    verify_csrf(&headers)?;
    state
        .identity
        .logout_all(claims.user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "logout_all: database error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?;

    Ok((StatusCode::NO_CONTENT, clear_session_cookies()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ForgotPasswordBody {
    pub email: String,
}

/// Always `202` with the same message, whether or not the email exists (US-03) -- the identity
/// service already enforces this at the service layer, this handler just can't turn it into a
/// non-generic error either.
#[utoipa::path(
    post,
    path = "/api/v1/auth/password/forgot",
    tag = "auth",
    request_body = ForgotPasswordBody,
    responses(
        (status = 202, description = "Same response whether or not the email exists", body = MessageResponse),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordBody>,
) -> Result<(StatusCode, Json<MessageResponse>), ApiError> {
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
        Json(MessageResponse {
            message: "if that email exists, a reset link has been sent",
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetPasswordBody {
    pub token: String,
    pub password: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/password/reset",
    tag = "auth",
    request_body = ResetPasswordBody,
    responses(
        (status = 200, description = "Password changed; all sessions revoked", body = MessageResponse),
        (status = 400, description = "Invalid or expired reset token", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Password too weak", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> Result<Json<MessageResponse>, ApiError> {
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

    Ok(Json(MessageResponse {
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
    use crate::app::tests::{
        test_clock, test_fixed_clock, test_identity, test_rate_limiter, test_tenancy,
    };

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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

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

    /// The refresh and csrf `Set-Cookie` values (bare `name=value`, attributes stripped) from a
    /// successful login, plus the bare CSRF token value for the `X-CSRF-Token` header
    /// double-submit tests need to send alongside the `csrf` cookie.
    struct LoginCookieJar {
        access: String,
        refresh: String,
        csrf: String,
        csrf_value: String,
    }

    async fn register_and_login(pool: &PgPool, email: &str) -> LoginCookieJar {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

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
        let csrf = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_csrf="))
            .expect("csrf cookie present")
            .clone();
        let csrf_value = csrf
            .strip_prefix("sintade_csrf=")
            .expect("csrf cookie has expected prefix")
            .to_string();

        LoginCookieJar {
            access,
            refresh,
            csrf,
            csrf_value,
        }
    }

    fn set_cookies_from(response: &axum::http::Response<Body>) -> LoginCookieJar {
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
        let csrf = set_cookies
            .iter()
            .find(|c| c.starts_with("sintade_csrf="))
            .expect("csrf cookie present")
            .clone();
        let csrf_value = csrf
            .strip_prefix("sintade_csrf=")
            .expect("csrf cookie has expected prefix")
            .to_string();
        LoginCookieJar {
            access,
            refresh,
            csrf,
            csrf_value,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn refresh_rotates_and_replaying_old_token_revokes_family(pool: PgPool) {
        let email = format!("day14-{}@example.com", uuid::Uuid::now_v7());
        let login = register_and_login(&pool, &email).await;
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

        // first refresh: rotates A -> B, succeeds
        let first_refresh = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .header("x-csrf-token", &login.csrf_value)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(first_refresh.status(), StatusCode::OK);
        let rotated = set_cookies_from(&first_refresh);
        assert_ne!(
            login.refresh, rotated.refresh,
            "rotation issues a new refresh token"
        );

        // replaying the retired token A is reuse -- must fail and nuke the whole family
        let replay = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .header("x-csrf-token", &login.csrf_value)
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
                    .header("cookie", format!("{}; {}", rotated.refresh, rotated.csrf))
                    .header("x-csrf-token", &rotated.csrf_value)
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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header(
                        "cookie",
                        "sintade_refresh=not-a-real-token; sintade_csrf=matching-value",
                    )
                    .header("x-csrf-token", "matching-value")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn refresh_with_missing_csrf_header_returns_403(pool: PgPool) {
        let email = format!("day16-csrf-{}@example.com", uuid::Uuid::now_v7());
        let login = register_and_login(&pool, &email).await;
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

        // has a valid refresh cookie, but no X-CSRF-Token header at all
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_revokes_session_and_subsequent_refresh_fails(pool: PgPool) {
        let email = format!("day14-logout-{}@example.com", uuid::Uuid::now_v7());
        let login = register_and_login(&pool, &email).await;
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

        let logout_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .header("x-csrf-token", &login.csrf_value)
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
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .header("x-csrf-token", &login.csrf_value)
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

    async fn send(
        pool: &PgPool,
        method: &str,
        uri: &str,
        cookie: String,
        csrf: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> axum::http::Response<Body> {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie);
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        let body = match body {
            Some(json) => {
                request = request.header("content-type", "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        app.oneshot(request.body(body).expect("valid request"))
            .await
            .expect("router call succeeds")
    }

    fn all_cookies(jar: &LoginCookieJar) -> String {
        format!("{}; {}; {}", jar.access, jar.refresh, jar.csrf)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_all_revokes_every_session_of_the_user(pool: PgPool) {
        let email = format!("day19-logout-all-{}@example.com", uuid::Uuid::now_v7());
        let laptop = register_and_login(&pool, &email).await;
        // Registering again is an anonymous no-op; the second login is a second device.
        let phone = register_and_login(&pool, &email).await;

        let response = send(
            &pool,
            "POST",
            "/api/v1/auth/logout-all",
            all_cookies(&laptop),
            Some(&laptop.csrf_value),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            response
                .headers()
                .get_all(axum::http::header::SET_COOKIE)
                .iter()
                .any(|c| c.to_str().is_ok_and(|c| c.starts_with("sintade_refresh=;"))),
            "this browser's refresh cookie is cleared"
        );

        for (device, jar) in [("laptop", &laptop), ("phone", &phone)] {
            let refresh = send(
                &pool,
                "POST",
                "/api/v1/auth/refresh",
                format!("{}; {}", jar.refresh, jar.csrf),
                Some(&jar.csrf_value),
                None,
            )
            .await;
            assert_eq!(
                refresh.status(),
                StatusCode::UNAUTHORIZED,
                "{device}'s session must be revoked"
            );
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_all_without_csrf_header_returns_403(pool: PgPool) {
        let email = format!("day19-logout-all-csrf-{}@example.com", uuid::Uuid::now_v7());
        let jar = register_and_login(&pool, &email).await;
        let response = send(
            &pool,
            "POST",
            "/api/v1/auth/logout-all",
            all_cookies(&jar),
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_me_changes_the_display_name(pool: PgPool) {
        let email = format!("day19-profile-{}@example.com", uuid::Uuid::now_v7());
        let jar = register_and_login(&pool, &email).await;

        let response = send(
            &pool,
            "PATCH",
            "/api/v1/me",
            all_cookies(&jar),
            Some(&jar.csrf_value),
            Some(json!({"display_name": "  Renamed Tester  "})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let user: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(user["display_name"], "Renamed Tester");

        let me = send(&pool, "GET", "/api/v1/me", all_cookies(&jar), None, None).await;
        let body = axum::body::to_bytes(me.into_body(), usize::MAX)
            .await
            .expect("read body");
        let me: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(me["user"]["display_name"], "Renamed Tester");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_me_rejects_blank_and_missing_csrf(pool: PgPool) {
        let email = format!("day19-profile-bad-{}@example.com", uuid::Uuid::now_v7());
        let jar = register_and_login(&pool, &email).await;

        let blank = send(
            &pool,
            "PATCH",
            "/api/v1/me",
            all_cookies(&jar),
            Some(&jar.csrf_value),
            Some(json!({"display_name": "   "})),
        )
        .await;
        assert_eq!(blank.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let no_csrf = send(
            &pool,
            "PATCH",
            "/api/v1/me",
            all_cookies(&jar),
            None,
            Some(json!({"display_name": "Sneaky"})),
        )
        .await;
        assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn logout_without_cookie_returns_204(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

        // register, then log in so there's a live session to prove gets revoked
        let login = register_and_login(&pool, &email).await;
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
                    .header("cookie", format!("{}; {}", login.refresh, login.csrf))
                    .header("x-csrf-token", &login.csrf_value)
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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
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
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
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

    /// The literal Day 16 Check: "6th login in a minute -> 429; headers present". Uses a
    /// `FixedClock` (frozen, not wall-clock) so this can't flake from the test straddling a
    /// real minute-window boundary under parallel-test contention.
    #[sqlx::test(migrations = "../../migrations")]
    async fn sixth_login_attempt_in_a_minute_returns_429(pool: PgPool) {
        let email = format!("day16-rate-limit-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_fixed_clock(),
            TEST_ORIGIN,
        );

        let register_body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "Rate Limit Tester",
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.9")
                    .body(Body::from(register_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");

        let login_body = json!({"email": email, "password": "totally-the-wrong-password"});
        for attempt in 1..=5 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/auth/login")
                        .header("content-type", "application/json")
                        .header("x-forwarded-for", "203.0.113.9")
                        .body(Body::from(login_body.to_string()))
                        .expect("valid request"),
                )
                .await
                .expect("router call succeeds");
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "attempt {attempt} should still be within the 5/min limit (wrong password, but not rate-limited)"
            );
        }

        let sixth = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.9")
                    .body(Body::from(login_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(sixth.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn different_ips_have_independent_login_rate_limits(pool: PgPool) {
        let email = format!("day16-rate-limit-ip-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_fixed_clock(),
            TEST_ORIGIN,
        );

        let login_body = json!({"email": email, "password": "wrong-password-entirely"});
        for _ in 0..5 {
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/auth/login")
                        .header("content-type", "application/json")
                        .header("x-forwarded-for", "203.0.113.10")
                        .body(Body::from(login_body.to_string()))
                        .expect("valid request"),
                )
                .await
                .expect("router call succeeds");
        }

        // a different IP, same email, is not affected by the first IP's exhausted limit
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.11")
                    .body(Body::from(login_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn fourth_signup_in_an_hour_from_the_same_ip_returns_429(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_fixed_clock(),
            TEST_ORIGIN,
        );

        for attempt in 1..=3 {
            let body = json!({
                "email": format!("day16-signup-{attempt}-{}@example.com", uuid::Uuid::now_v7()),
                "password": "correct-horse-battery-staple-42",
                "display_name": "Signup Rate Limit Tester",
            });
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/auth/register")
                        .header("content-type", "application/json")
                        .header("x-forwarded-for", "203.0.113.20")
                        .body(Body::from(body.to_string()))
                        .expect("valid request"),
                )
                .await
                .expect("router call succeeds");
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let fourth_body = json!({
            "email": format!("day16-signup-fourth-{}@example.com", uuid::Uuid::now_v7()),
            "password": "correct-horse-battery-staple-42",
            "display_name": "Fourth Signup",
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.20")
                    .body(Body::from(fourth_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn security_headers_are_present_on_every_response(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool),
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

        let headers = response.headers();
        assert_eq!(
            headers
                .get("strict-transport-security")
                .expect("HSTS header present"),
            "max-age=31536000"
        );
        assert_eq!(
            headers
                .get("x-content-type-options")
                .expect("X-Content-Type-Options header present"),
            "nosniff"
        );
        assert_eq!(
            headers
                .get("referrer-policy")
                .expect("Referrer-Policy header present"),
            "strict-origin-when-cross-origin"
        );
        assert!(headers.get("content-security-policy").is_some());
        assert_eq!(
            headers
                .get("permissions-policy")
                .expect("Permissions-Policy header present"),
            "display-capture=(self)"
        );
    }
}
