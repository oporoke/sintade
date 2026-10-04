use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use ingest::{Sources, StartRecording, StartRecordingError};
use kernel::{AppError, Permission, RecordingId, TakeId};
use media::RetryError;
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
    /// The longest take the workspace's plan accepts (pauses excluded). The recorder stops by
    /// itself before this; finalize rejects anything longer.
    pub max_duration_ms: u32,
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
        (status = 402, description = "The plan's recording limit is reached (free tier: 50)", body = Problem, content_type = "application/problem+json"),
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
            StartRecordingError::LimitReached { .. } => {
                ApiError::from(AppError::LimitReached(error.to_string()))
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
            max_duration_ms: started.max_duration_ms,
        }),
    ))
}

/// Reprocesses a recording whose processing failed (`failed → processing`, docs/design.md §4).
/// The work runs in the background; poll the recording for `ready` or `failed` again.
#[utoipa::path(
    post,
    path = "/api/v1/recordings/{recording_id}/retry",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The failed recording")),
    responses(
        (status = 202, description = "Processing queued again"),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or the role can't edit recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The recording isn't failed", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn retry_recording(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path(recording_id): Path<RecordingId>,
) -> Result<StatusCode, ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    state
        .retry
        .retry_processing(ctx.workspace_id, recording_id)
        .await
        .map_err(|error| match error {
            RetryError::NotFound => ApiError::from(AppError::NotFound),
            RetryError::NotFailed => ApiError::from(AppError::Conflict(error.to_string())),
            other => {
                tracing::error!(error = %other, "retry recording failed");
                ApiError::from(AppError::Internal("could not queue the retry".to_string()))
            }
        })?;
    Ok(StatusCode::ACCEPTED)
}

/// Downloads the recording's MP4: a signed URL that saves `<title>.mp4`. The recording's owner,
/// or a workspace admin, may.
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/download",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "Signed download URL", body = crate::routes::watch::DownloadResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Not the owner, and the role can't manage recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The MP4 isn't ready yet", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn download_recording(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<axum::response::Response, ApiError> {
    let internal = |error: &dyn std::fmt::Display| {
        tracing::error!(%error, "download recording failed");
        ApiError::from(AppError::Internal("download unavailable".to_string()))
    };
    let mut conn = state.pool.acquire().await.map_err(|e| internal(&e))?;
    let info = state
        .catalog
        .watch_info(&mut conn, recording_id, ctx.workspace_id)
        .await
        .map_err(|e| internal(&e))?
        .ok_or(AppError::NotFound)?;
    drop(conn);
    if info.owner_id != ctx.user_id {
        ctx.require(Permission::DeleteRecording)?;
    }
    let grant = state
        .delivery
        .download(ctx.workspace_id, recording_id, &info.title)
        .await
        .map_err(|e| internal(&e))?
        .filter(|_| info.state == "ready")
        .ok_or_else(|| {
            ApiError::from(AppError::Conflict(
                "this recording is still being processed".to_string(),
            ))
        })?;
    Ok(crate::routes::watch::download_response(grant))
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
    use crate::app::tests::{
        test_clock, test_identity, test_ingest, test_rate_limiter, test_tenancy,
    };
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
            test_ingest(pool.clone()),
            crate::app::tests::test_store(),
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

    /// Day 41's Check over HTTP: the 51st recording is `402` problem+json; the first 50 carry
    /// the plan's take limit.
    #[sqlx::test(migrations = "../../migrations")]
    async fn the_51st_recording_is_402(pool: PgPool) {
        let alice = caller(&pool).await;
        for _ in 0..50 {
            let (status, body) = post(&pool, &alice, valid_body(), true).await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            assert_eq!(body["max_duration_ms"], 600_000);
        }
        let (status, problem) = post(&pool, &alice, valid_body(), true).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED, "{problem}");
        assert_eq!(problem["status"], 402);
        assert_eq!(
            problem["detail"],
            "your plan allows 50 recordings; delete one to record another"
        );
    }

    async fn retry(
        pool: &PgPool,
        caller: &Caller,
        recording_id: uuid::Uuid,
        csrf: bool,
    ) -> StatusCode {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_ingest(pool.clone()),
            crate::app::tests::test_store(),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/recordings/{recording_id}/retry"));
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
        app.oneshot(request.body(Body::empty()).expect("valid request"))
            .await
            .expect("router call succeeds")
            .status()
    }

    /// A recording of the caller's that processing gave up on, with its take's job.
    async fn failed_recording(pool: &PgPool, caller: &Caller) -> uuid::Uuid {
        let (_, body) = post(pool, caller, valid_body(), true).await;
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
        sqlx::query!(
            "UPDATE recordings SET state = 'failed' WHERE id = $1",
            recording_id
        )
        .execute(pool)
        .await
        .expect("fail the recording");
        sqlx::query!(
            r#"INSERT INTO media_jobs (take_id, workspace_id, recording_id, mime_type, duration_ms,
                                       chunks, state, attempts, last_error)
               VALUES ($1, $2, $3, 'video/webm', 1000, '[]'::jsonb, 'failed', 5, 'gave up')"#,
            take_id,
            caller.workspace_id.into_uuid(),
            recording_id,
        )
        .execute(pool)
        .await
        .expect("media job");
        recording_id
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retry_reprocesses_a_failed_recording(pool: PgPool) {
        let alice = caller(&pool).await;
        let recording_id = failed_recording(&pool, &alice).await;

        assert_eq!(
            retry(&pool, &alice, recording_id, true).await,
            StatusCode::ACCEPTED
        );

        let state = sqlx::query_scalar!(
            r#"SELECT state::text AS "state!" FROM recordings WHERE id = $1"#,
            recording_id
        )
        .fetch_one(&pool)
        .await
        .expect("recording");
        assert_eq!(state, "processing");
        let job = sqlx::query!(
            r#"SELECT state::text AS "state!", attempts, last_error FROM media_jobs
               WHERE recording_id = $1"#,
            recording_id
        )
        .fetch_one(&pool)
        .await
        .expect("media job");
        assert_eq!((job.state.as_str(), job.attempts), ("queued", 0));
        assert!(job.last_error.is_none());
        let queued = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM jobs WHERE kind = 'ProcessTake' AND done_at IS NULL"#
        )
        .fetch_one(&pool)
        .await
        .expect("jobs");
        assert_eq!(queued, 1);

        // A second press finds it processing again: nothing more is queued.
        assert_eq!(
            retry(&pool, &alice, recording_id, true).await,
            StatusCode::CONFLICT
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retry_needs_the_csrf_token_and_an_editing_role(pool: PgPool) {
        let alice = caller(&pool).await;
        let recording_id = failed_recording(&pool, &alice).await;
        assert_eq!(
            retry(&pool, &alice, recording_id, false).await,
            StatusCode::FORBIDDEN
        );

        sqlx::query!(
            "UPDATE memberships SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
            alice.workspace_id.into_uuid(),
            alice.user_id.into_uuid(),
        )
        .execute(&pool)
        .await
        .expect("demote to viewer");
        assert_eq!(
            retry(&pool, &alice, recording_id, true).await,
            StatusCode::FORBIDDEN
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_owner_downloads_an_mp4(pool: PgPool) {
        use crate::routes::testkit::{call, caller, recording, renditions};
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;
        let uri = format!("/api/v1/recordings/{id}/download");

        let reply = call(&pool, Some(&owner), axum::http::Method::GET, &uri, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(reply.body["filename"], "Mine.mp4");
        assert_eq!(reply.body["expires_in_s"], 900);

        // A recording without an MP4 yet is a conflict, not a broken URL.
        let pending = recording(&pool, &owner, "processing").await;
        let reply = call(
            &pool,
            Some(&owner),
            axum::http::Method::GET,
            &format!("/api/v1/recordings/{pending}/download"),
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::CONFLICT);
    }
}
