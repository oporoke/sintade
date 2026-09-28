use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use ingest::{Sources, StartRecording, StartRecordingError};
use kernel::{AppError, Permission, RecordingId, TakeId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem};
use crate::workspace_context::WorkspaceContext;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRecordingBody {
    /// Optional; blank or missing becomes "Untitled recording". At most 200 characters.
    pub title: Option<String>,
    /// The take's MIME type as `MediaRecorder` reports it, e.g. `video/webm;codecs=vp9,opus`.
    /// The container must be `video/webm` or `video/mp4`.
    pub mime_type: String,
    pub has_system_audio: bool,
    pub has_mic: bool,
    pub has_camera: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateRecordingResponse {
    #[schema(value_type = uuid::Uuid)]
    pub recording_id: RecordingId,
    #[schema(value_type = uuid::Uuid)]
    pub take_id: TakeId,
}

/// Starts a recording: creates the recording and its first take in the caller's current
/// workspace (upload protocol v1, docs/design.md §9). Chunks are presigned and acked against
/// the returned `take_id`.
#[utoipa::path(
    post,
    path = "/api/v1/recordings",
    tag = "recordings",
    request_body = CreateRecordingBody,
    responses(
        (status = 201, description = "Recording and take created", body = CreateRecordingResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or the role can't create recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "Not a member of the session's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Invalid title or MIME type", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn create_recording(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Json(body): Json<CreateRecordingBody>,
) -> Result<(StatusCode, Json<CreateRecordingResponse>), ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::CreateRecording)?;
    let started = state
        .ingest
        .start_recording(StartRecording {
            workspace_id: ctx.workspace_id,
            owner_id: ctx.user_id,
            title: body.title,
            mime_type: body.mime_type,
            sources: Sources {
                system_audio: body.has_system_audio,
                mic: body.has_mic,
                camera: body.has_camera,
            },
        })
        .await
        .map_err(|error| match error {
            StartRecordingError::InvalidTitle(source) => {
                ApiError::from(AppError::Validation(source.to_string()))
            }
            StartRecordingError::InvalidMimeType(source) => {
                ApiError::from(AppError::Validation(source.to_string()))
            }
            StartRecordingError::Database(source) => {
                tracing::error!(error = %source, "create recording: database error");
                ApiError::from(AppError::Internal("database unavailable".to_string()))
            }
        })?;
    Ok((
        StatusCode::CREATED,
        Json(CreateRecordingResponse {
            recording_id: started.recording_id,
            take_id: started.take_id,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use kernel::{UserId, WorkspaceId};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::{test_clock, test_identity, test_rate_limiter, test_tenancy};
    use crate::csrf::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};
    use crate::session::ACCESS_COOKIE_NAME;

    const TEST_ORIGIN: &str = "http://localhost:4200";

    struct Caller {
        user_id: UserId,
        workspace_id: WorkspaceId,
        access_cookie: String,
    }

    async fn caller(pool: &PgPool) -> Caller {
        let identity = test_identity(pool.clone());
        let email = format!("recordings-{}@example.com", uuid::Uuid::now_v7());
        let password = "correct-horse-battery-staple-42".to_string();
        identity
            .register(identity::RegisterRequest {
                email: email.clone(),
                password: password.clone(),
                display_name: "Recorder".to_string(),
            })
            .await
            .expect("register");
        let session = identity
            .login(identity::LoginRequest { email, password })
            .await
            .expect("login");
        let claims = identity
            .verify_access_cookie(&session.access_token)
            .expect("fresh token verifies");
        Caller {
            user_id: claims.user_id,
            workspace_id: claims.workspace_id,
            access_cookie: format!("{ACCESS_COOKIE_NAME}={}", session.access_token),
        }
    }

    async fn post(pool: &PgPool, caller: &Caller, body: Value, csrf: bool) -> (StatusCode, Value) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/recordings")
            .header("content-type", "application/json");
        request = if csrf {
            request
                .header(
                    "cookie",
                    format!("{}; {CSRF_COOKIE_NAME}=token", caller.access_cookie),
                )
                .header(CSRF_HEADER_NAME, "token")
        } else {
            request.header("cookie", caller.access_cookie.clone())
        };
        let response = app
            .oneshot(
                request
                    .body(Body::from(body.to_string()))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn valid_body() -> Value {
        json!({
            "title": "Sprint demo",
            "mime_type": "video/webm;codecs=vp9,opus",
            "has_system_audio": true,
            "has_mic": true,
            "has_camera": false,
        })
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn returns_ids_of_rows_scoped_to_the_callers_workspace(pool: PgPool) {
        let alice = caller(&pool).await;
        let (status, body) = post(&pool, &alice, valid_body(), true).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let recording_id: uuid::Uuid = body["recording_id"]
            .as_str()
            .expect("recording_id")
            .parse()
            .expect("uuid");
        let take_id: uuid::Uuid = body["take_id"]
            .as_str()
            .expect("take_id")
            .parse()
            .expect("uuid");
        assert_eq!(recording_id.get_version_num(), 7);
        assert_eq!(take_id.get_version_num(), 7);

        let row = sqlx::query!(
            r#"SELECT r.workspace_id, r.owner_id, r.title, r.current_take,
                      t.workspace_id AS take_workspace_id, t.has_system_audio
               FROM recordings r JOIN takes t ON t.recording_id = r.id
               WHERE r.id = $1 AND t.id = $2"#,
            recording_id,
            take_id,
        )
        .fetch_one(&pool)
        .await
        .expect("both rows exist and belong together");
        assert_eq!(row.workspace_id, alice.workspace_id.into_uuid());
        assert_eq!(row.take_workspace_id, alice.workspace_id.into_uuid());
        assert_eq!(row.owner_id, alice.user_id.into_uuid());
        assert_eq!(row.title, "Sprint demo");
        assert_eq!(row.current_take, Some(take_id));
        assert!(row.has_system_audio);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn requires_the_csrf_token(pool: PgPool) {
        let alice = caller(&pool).await;
        let (status, _) = post(&pool, &alice, valid_body(), false).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_viewer_cannot_create_recordings(pool: PgPool) {
        let viewer = caller(&pool).await;
        sqlx::query!(
            "UPDATE memberships SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
            viewer.workspace_id.into_uuid(),
            viewer.user_id.into_uuid(),
        )
        .execute(&pool)
        .await
        .expect("demote to viewer");

        let (status, _) = post(&pool, &viewer, valid_body(), true).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let recordings = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM recordings"#)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(recordings, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn invalid_input_is_422_problem_json(pool: PgPool) {
        let alice = caller(&pool).await;
        let mut bad_mime = valid_body();
        bad_mime["mime_type"] = json!("audio/ogg");
        let mut long_title = valid_body();
        long_title["title"] = json!("x".repeat(catalog::MAX_TITLE_CHARS + 1));
        let mut missing_sources = valid_body();
        missing_sources
            .as_object_mut()
            .expect("object")
            .remove("has_mic");

        for body in [bad_mime, long_title] {
            let (status, problem) = post(&pool, &alice, body, true).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
            assert_eq!(problem["status"], 422);
        }
        let (status, _) = post(&pool, &alice, missing_sources, true).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}
