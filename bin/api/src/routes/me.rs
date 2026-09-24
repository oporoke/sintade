use axum::Json;
use axum::extract::State;
use kernel::{AppError, WorkspaceId};
use serde::Serialize;

use crate::app::AppState;
use crate::error::ApiError;
use crate::session::SessionClaims;

#[derive(Debug, Serialize)]
pub struct MeResponse {
    #[serde(flatten)]
    pub view: identity::MeView,
    /// The workspace the session's access cookie was issued for -- lets the client pick the
    /// right entry out of `workspaces` as "current" without guessing.
    pub current_workspace_id: WorkspaceId,
}

#[tracing::instrument(skip_all)]
pub async fn me(
    State(state): State<AppState>,
    claims: SessionClaims,
) -> Result<Json<MeResponse>, ApiError> {
    let view = state
        .identity
        .me(claims.user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "me: database error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?
        .ok_or_else(|| ApiError::from(AppError::Unauthorized("session invalid".to_string())))?;

    Ok(Json(MeResponse {
        view,
        current_workspace_id: claims.workspace_id,
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::test_identity;

    const TEST_ORIGIN: &str = "http://localhost:4200";

    /// Registers a user via the real HTTP handler, then returns the two `Set-Cookie` values
    /// (bare `name=value`, attributes stripped) from a successful login.
    async fn register_and_login(pool: &PgPool, email: &str) -> (String, String) {
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let register_body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "Me Route Tester",
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

        let login_body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
        });
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
        assert_eq!(set_cookies.len(), 2, "login sets exactly two cookies");

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
    async fn login_then_me_returns_user_and_workspace(pool: PgPool) {
        let email = format!("day13-{}@example.com", uuid::Uuid::now_v7());
        let (access_cookie, refresh_cookie) = register_and_login(&pool, &email).await;

        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header("cookie", format!("{access_cookie}; {refresh_cookie}"))
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid json body");

        assert_eq!(json["user"]["email"], email);
        assert_eq!(json["workspaces"].as_array().expect("array").len(), 1);
        assert_eq!(json["workspaces"][0]["role"], "owner");
        assert_eq!(json["workspaces"][0]["is_personal"], true);
        assert_eq!(json["current_workspace_id"], json["workspaces"][0]["id"]);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn login_with_wrong_password_returns_401(pool: PgPool) {
        let email = format!("day13-badpw-{}@example.com", uuid::Uuid::now_v7());
        let app = build_router(pool.clone(), test_identity(pool.clone()), TEST_ORIGIN);

        let register_body = json!({
            "email": email,
            "password": "correct-horse-battery-staple-42",
            "display_name": "Wrong Password Tester",
        });
        app.clone()
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

        let login_body = json!({"email": email, "password": "totally-the-wrong-password"});
        let response = app
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn login_with_unknown_email_returns_401(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);

        let login_body = json!({
            "email": "no-such-account@example.com",
            "password": "whatever-password-12345",
        });
        let response = app
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn me_without_cookie_returns_401(pool: PgPool) {
        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn me_with_tampered_cookie_returns_401(pool: PgPool) {
        let email = format!("day13-tamper-{}@example.com", uuid::Uuid::now_v7());
        let (access_cookie, _) = register_and_login(&pool, &email).await;

        // flip the last character of the signature to invalidate it without changing length
        let tampered = {
            let mut chars: Vec<char> = access_cookie.chars().collect();
            let last = chars.len() - 1;
            chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
            chars.into_iter().collect::<String>()
        };

        let app = build_router(pool.clone(), test_identity(pool), TEST_ORIGIN);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header("cookie", tampered)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
