use axum::Json;
use axum::extract::State;
use identity::VerifyEmailError;
use kernel::AppError;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::error::ApiError;

#[derive(Debug, Deserialize)]
pub struct VerifyEmailBody {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyEmailResponse {
    pub message: &'static str,
}

#[tracing::instrument(skip_all)]
pub async fn verify_email(
    State(state): State<AppState>,
    Json(body): Json<VerifyEmailBody>,
) -> Result<Json<VerifyEmailResponse>, ApiError> {
    state
        .identity
        .verify_email(&body.token)
        .await
        .map_err(|error| {
            let app_error = match error {
                VerifyEmailError::InvalidToken => {
                    AppError::BadRequest("invalid or expired verification token".to_string())
                }
                VerifyEmailError::Database(source) => {
                    tracing::error!(error = %source, "verify_email: database error");
                    AppError::Internal("database unavailable".to_string())
                }
            };
            ApiError::from(app_error)
        })?;

    Ok(Json(VerifyEmailResponse {
        message: "email verified",
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use platform::JobQueue;
    use serde_json::json;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::{test_clock, test_identity, test_rate_limiter, test_tenancy};

    const TEST_ORIGIN: &str = "http://localhost:4200";

    /// Claims the next enqueued `SendEmail` job and pulls the `?token=...` value out of its
    /// body link -- this is exactly what a user clicking the link in their inbox would give the
    /// API, without needing a real mailbox for these tests (Mailpit round-trips are covered by
    /// Day 12's register tests).
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
    async fn verify_email_with_garbage_token_returns_400(pool: PgPool) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );

        let body = json!({"token": "not-a-real-token"});
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/verify-email")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn verify_email_with_real_token_marks_user_verified_and_is_single_use(pool: PgPool) {
        let email = format!("day15-verify-{}@example.com", uuid::Uuid::now_v7());
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
            "display_name": "Verify Email Tester",
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

        let token = claim_email_link_token(&pool).await;

        let verify_body = json!({"token": token});
        let verify_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/verify-email")
                    .header("content-type", "application/json")
                    .body(Body::from(verify_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(verify_response.status(), StatusCode::OK);

        let email_verified: bool =
            sqlx::query_scalar!("SELECT email_verified FROM users WHERE email = $1", email)
                .fetch_one(&pool)
                .await
                .expect("user row exists");
        assert!(email_verified, "email_verified is set after verification");

        // single-use: presenting the same token again must fail
        let replay_body = json!({"token": token});
        let replay_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/verify-email")
                    .header("content-type", "application/json")
                    .body(Body::from(replay_body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(
            replay_response.status(),
            StatusCode::BAD_REQUEST,
            "a verification token can only be used once"
        );
    }
}
