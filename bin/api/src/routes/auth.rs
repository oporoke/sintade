use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use identity::{RegisterError, RegisterRequest};
use kernel::AppError;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::error::ApiError;

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
}
