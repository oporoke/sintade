use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use ingest::{CHUNK_URL_TTL, PresignChunks, PresignError};
use kernel::{AppError, TakeId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem};
use crate::workspace_context::WorkspaceContext;

#[derive(Debug, Deserialize, IntoParams)]
pub struct PresignQuery {
    /// How many consecutive chunk URLs to return, starting at `idx` (1–10, default 1). Used to
    /// batch round trips after a reconnect.
    pub count: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChunkUploadUrl {
    pub idx: u32,
    /// Presigned `PUT` for this chunk's bytes. A bearer credential: don't log it.
    pub url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PresignChunksResponse {
    pub urls: Vec<ChunkUploadUrl>,
    /// Seconds each URL stays valid.
    pub expires_in_s: u64,
}

/// Presigned `PUT` URLs for chunk `idx` (and, with `?count=`, the chunks after it). Only the
/// recording's owner gets them, and only until the take is finalized. The browser then `PUT`s
/// the bytes straight to object storage (media never passes through the API).
#[utoipa::path(
    post,
    path = "/api/v1/takes/{take_id}/chunks/{idx}/url",
    tag = "takes",
    params(
        ("take_id" = uuid::Uuid, Path, description = "The take"),
        ("idx" = u32, Path, description = "First chunk index (0-based)"),
        PresignQuery,
    ),
    responses(
        (status = 200, description = "Upload URLs", body = PresignChunksResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such take, or not the caller's", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The take is already finalized", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "count outside 1–10 or index too large", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn presign_chunks(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path((take_id, idx)): Path<(TakeId, u32)>,
    Query(query): Query<PresignQuery>,
) -> Result<Json<PresignChunksResponse>, ApiError> {
    verify_csrf(&headers)?;
    let chunks = state
        .ingest
        .presign_chunks(PresignChunks {
            workspace_id: ctx.workspace_id,
            user_id: ctx.user_id,
            take_id,
            first_idx: idx,
            count: query.count.unwrap_or(1),
        })
        .await
        .map_err(|error| match error {
            PresignError::NotFound => ApiError::from(AppError::NotFound),
            PresignError::Finalized => {
                ApiError::from(AppError::Conflict("take is already finalized".to_string()))
            }
            PresignError::InvalidRange(source) => {
                ApiError::from(AppError::Validation(source.to_string()))
            }
            PresignError::Storage(source) => {
                tracing::error!(error = %source, "presign: object store error");
                ApiError::from(AppError::Internal("storage unavailable".to_string()))
            }
            PresignError::Database(source) => {
                tracing::error!(error = %source, "presign: database error");
                ApiError::from(AppError::Internal("database unavailable".to_string()))
            }
        })?;
    Ok(Json(PresignChunksResponse {
        urls: chunks
            .into_iter()
            .map(|chunk| ChunkUploadUrl {
                idx: chunk.idx,
                url: chunk.url.into(),
            })
            .collect(),
        expires_in_s: CHUNK_URL_TTL.as_secs(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use kernel::{TakeId, WorkspaceId};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::{
        test_clock, test_identity, test_ingest, test_rate_limiter, test_store, test_tenancy,
    };
    use crate::csrf::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};
    use crate::session::ACCESS_COOKIE_NAME;

    const TEST_ORIGIN: &str = "http://localhost:4200";

    struct Owner {
        workspace_id: WorkspaceId,
        take_id: TakeId,
        cookie: String,
    }

    /// Registers, logs in and starts a recording; returns the take and a cookie header that
    /// carries the session and the CSRF cookie.
    async fn owner_with_take(pool: &PgPool) -> Owner {
        let identity = test_identity(pool.clone());
        let email = format!("takes-{}@example.com", uuid::Uuid::now_v7());
        let password = "correct-horse-battery-staple-42".to_string();
        identity
            .register(identity::RegisterRequest {
                email: email.clone(),
                password: password.clone(),
                display_name: "Uploader".to_string(),
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
        let started = test_ingest(pool.clone())
            .start_recording(ingest::StartRecording {
                workspace_id: claims.workspace_id,
                owner_id: claims.user_id,
                title: None,
                mime_type: "video/webm;codecs=vp9,opus".to_string(),
                sources: ingest::Sources {
                    system_audio: false,
                    mic: true,
                    camera: false,
                },
            })
            .await
            .expect("start recording");
        Owner {
            workspace_id: claims.workspace_id,
            take_id: started.take_id,
            cookie: format!(
                "{ACCESS_COOKIE_NAME}={}; {CSRF_COOKIE_NAME}=token",
                session.access_token
            ),
        }
    }

    async fn presign(pool: &PgPool, owner: &Owner, path_suffix: &str) -> (StatusCode, Value) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_ingest(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            TEST_ORIGIN,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/takes/{}/chunks/{path_suffix}",
                        owner.take_id
                    ))
                    .header("cookie", &owner.cookie)
                    .header(CSRF_HEADER_NAME, "token")
                    .body(Body::empty())
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

    /// Day 34's Check: a PUT to the returned URL lands in object storage under the chunk key.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_put_to_the_presigned_url_lands_in_object_storage(pool: PgPool) {
        let owner = owner_with_take(&pool).await;
        let (status, body) = presign(&pool, &owner, "0/url?count=2").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["expires_in_s"], 300);
        let urls = body["urls"].as_array().expect("urls");
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0]["idx"], 0);
        assert_eq!(urls[1]["idx"], 1);

        let url = urls[1]["url"].as_str().expect("url");
        let bytes = vec![7u8; 4096];
        let put = reqwest::Client::new()
            .put(url)
            .body(bytes.clone())
            .send()
            .await
            .expect("PUT reaches MinIO");
        assert!(put.status().is_success(), "PUT got {}", put.status());

        let recording_id = sqlx::query_scalar!(
            "SELECT recording_id FROM takes WHERE id = $1",
            owner.take_id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("take");
        let key = format!(
            "ws/{}/rec/{recording_id}/takes/{}/chunks/000001.webm",
            owner.workspace_id, owner.take_id
        );
        let store = test_store();
        let meta = store
            .head(&key)
            .await
            .expect("head")
            .expect("chunk is in object storage");
        assert_eq!(meta.size, bytes.len() as u64);
        store
            .delete_prefix(&format!("ws/{}/", owner.workspace_id))
            .await
            .expect("clean up");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn bad_ranges_are_422_and_finalized_takes_409(pool: PgPool) {
        let owner = owner_with_take(&pool).await;
        for suffix in ["0/url?count=0", "0/url?count=11", "100000/url"] {
            let (status, body) = presign(&pool, &owner, suffix).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{suffix}: {body}");
        }

        sqlx::query!(
            "UPDATE takes SET finalized_at = now() WHERE id = $1",
            owner.take_id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("finalize");
        let (status, body) = presign(&pool, &owner, "0/url").await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["status"], 409);
    }
}
