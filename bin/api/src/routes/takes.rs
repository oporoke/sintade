use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ingest::{
    AckChunk, AckError, AckOutcome, CHUNK_URL_TTL, FinalizeError, FinalizeOutcome, FinalizeTake,
    PresignChunks, PresignError,
};
use kernel::{AppError, RecordingId, TakeId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem, problem_with_extensions};
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

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AckChunkBody {
    /// Bytes PUT for this chunk (1 to 16 MiB). Must match the stored object.
    pub size_bytes: u64,
    /// SHA-256 of the chunk's bytes, 64 hex characters.
    pub sha256: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AckStatus {
    /// Recorded now.
    Acked,
    /// The same size and hash were already recorded; nothing changed.
    AlreadyAcked,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AckChunkResponse {
    pub idx: u32,
    pub status: AckStatus,
}

/// Confirms chunk `idx` is uploaded, with its size and SHA-256. Idempotent on
/// `(take_id, idx)`: the same size and hash again is a no-op; a different one is `409`.
#[utoipa::path(
    post,
    path = "/api/v1/takes/{take_id}/chunks/{idx}/ack",
    tag = "takes",
    params(
        ("take_id" = uuid::Uuid, Path, description = "The take"),
        ("idx" = u32, Path, description = "Chunk index (0-based)"),
    ),
    request_body = AckChunkBody,
    responses(
        (status = 200, description = "Chunk recorded (or already recorded identically)", body = AckChunkResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such take, or not the caller's", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "Acked before with a different hash or size, or the take is finalized", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Not uploaded, size differs from storage, or invalid size/hash/index", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn ack_chunk(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path((take_id, idx)): Path<(TakeId, u32)>,
    Json(body): Json<AckChunkBody>,
) -> Result<Json<AckChunkResponse>, ApiError> {
    verify_csrf(&headers)?;
    let outcome = state
        .ingest
        .ack_chunk(AckChunk {
            workspace_id: ctx.workspace_id,
            user_id: ctx.user_id,
            take_id,
            idx,
            size_bytes: body.size_bytes,
            sha256: body.sha256,
        })
        .await
        .map_err(|error| match error {
            AckError::NotFound => ApiError::from(AppError::NotFound),
            AckError::Finalized | AckError::Mismatch { .. } => {
                ApiError::from(AppError::Conflict(error.to_string()))
            }
            AckError::IndexTooLarge
            | AckError::InvalidDigest(_)
            | AckError::InvalidSize(_)
            | AckError::NotUploaded { .. }
            | AckError::SizeMismatch { .. } => {
                ApiError::from(AppError::Validation(error.to_string()))
            }
            AckError::Storage(source) => {
                tracing::error!(error = %source, "ack: object store error");
                ApiError::from(AppError::Internal("storage unavailable".to_string()))
            }
            AckError::Database(source) => {
                tracing::error!(error = %source, "ack: database error");
                ApiError::from(AppError::Internal("database unavailable".to_string()))
            }
        })?;
    Ok(Json(AckChunkResponse {
        idx,
        status: match outcome {
            AckOutcome::Acked => AckStatus::Acked,
            AckOutcome::AlreadyAcked => AckStatus::AlreadyAcked,
        },
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TakeStatusResponse {
    #[schema(value_type = uuid::Uuid)]
    pub take_id: TakeId,
    #[schema(value_type = uuid::Uuid)]
    pub recording_id: RecordingId,
    pub finalized: bool,
    /// Acknowledged chunk indexes, ascending. Upload the rest.
    pub received: Vec<u32>,
    /// The acknowledged chunks with the size and SHA-256 recorded for each.
    pub chunks: Vec<ReceivedChunkBody>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivedChunkBody {
    pub idx: u32,
    pub size_bytes: u32,
    /// Lowercase hex.
    pub sha256: String,
}

/// Which chunks the server has acknowledged, so an interrupted or recovered upload sends only
/// the missing ones. Owner only.
#[utoipa::path(
    get,
    path = "/api/v1/takes/{take_id}/status",
    tag = "takes",
    params(("take_id" = uuid::Uuid, Path, description = "The take")),
    responses(
        (status = 200, description = "Upload state of the take", body = TakeStatusResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such take, or not the caller's", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn take_status(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(take_id): Path<TakeId>,
) -> Result<Json<TakeStatusResponse>, ApiError> {
    let status = state
        .ingest
        .take_status(ctx.workspace_id, ctx.user_id, take_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "take status: database error");
            ApiError::from(AppError::Internal("database unavailable".to_string()))
        })?
        .ok_or(AppError::NotFound)?;
    Ok(Json(TakeStatusResponse {
        take_id,
        recording_id: status.recording_id,
        finalized: status.finalized,
        received: status.received,
        chunks: status
            .chunks
            .into_iter()
            .map(|chunk| ReceivedChunkBody {
                idx: chunk.idx,
                size_bytes: chunk.size_bytes,
                sha256: chunk.sha256,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalizeBody {
    /// Number of chunks in the take; indexes `0..chunk_count` must all be acknowledged.
    pub chunk_count: u32,
    /// The take's length, pauses excluded.
    pub duration_ms: u32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStateName {
    Processing,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FinalizeResponse {
    #[schema(value_type = uuid::Uuid)]
    pub recording_id: RecordingId,
    pub state: RecordingStateName,
}

/// The `422` body when chunks are missing: problem details plus the indexes to upload.
#[derive(Debug, Serialize, ToSchema)]
pub struct MissingChunksProblem {
    pub r#type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    /// Missing indexes, ascending; at most 1000 listed.
    pub missing: Vec<u32>,
    /// How many are missing in total.
    pub missing_count: usize,
}

pub enum FinalizeRejection {
    Api(ApiError),
    Missing {
        missing: Vec<u32>,
        missing_count: usize,
    },
}

impl From<AppError> for FinalizeRejection {
    fn from(error: AppError) -> Self {
        Self::Api(error.into())
    }
}

impl From<ApiError> for FinalizeRejection {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl IntoResponse for FinalizeRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Api(error) => error.into_response(),
            Self::Missing {
                missing,
                missing_count,
            } => {
                let mut extensions = serde_json::Map::new();
                extensions.insert("missing".into(), missing.into());
                extensions.insert("missing_count".into(), missing_count.into());
                problem_with_extensions(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Unprocessable Entity",
                    format!("{missing_count} chunk(s) missing"),
                    extensions,
                )
            }
        }
    }
}

/// Declares the take complete. When every chunk `0..chunk_count` is acknowledged the take is
/// finalized, the recording moves to `processing` and `TakeFinalized` is emitted; otherwise
/// `422` lists the missing indexes. Retrying a successful finalize is `202` again.
#[utoipa::path(
    post,
    path = "/api/v1/takes/{take_id}/finalize",
    tag = "takes",
    params(("take_id" = uuid::Uuid, Path, description = "The take")),
    request_body = FinalizeBody,
    responses(
        (status = 202, description = "Finalized; processing starts", body = FinalizeResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing or mismatched CSRF token", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such take, or not the caller's", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "Finalized before with another count, or the recording no longer accepts uploads", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Chunks missing (listed in `missing`), extra chunks, or invalid count/duration", body = MissingChunksProblem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn finalize_take(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path(take_id): Path<TakeId>,
    Json(body): Json<FinalizeBody>,
) -> Result<(StatusCode, Json<FinalizeResponse>), FinalizeRejection> {
    verify_csrf(&headers)?;
    let outcome = state
        .ingest
        .finalize(FinalizeTake {
            workspace_id: ctx.workspace_id,
            user_id: ctx.user_id,
            take_id,
            chunk_count: body.chunk_count,
            duration_ms: body.duration_ms,
        })
        .await
        .map_err(|error| match error {
            FinalizeError::NotFound => AppError::NotFound.into(),
            FinalizeError::MissingChunks {
                missing,
                missing_count,
            } => FinalizeRejection::Missing {
                missing,
                missing_count,
            },
            FinalizeError::InvalidChunkCount
            | FinalizeError::InvalidDuration
            | FinalizeError::UnexpectedChunks(_) => AppError::Validation(error.to_string()).into(),
            FinalizeError::CountMismatch { .. } | FinalizeError::RecordingClosed => {
                AppError::Conflict(error.to_string()).into()
            }
            FinalizeError::Outbox(source) => {
                tracing::error!(error = %source, "finalize: outbox error");
                AppError::Internal("database unavailable".to_string()).into()
            }
            FinalizeError::Database(source) => {
                tracing::error!(error = %source, "finalize: database error");
                AppError::Internal("database unavailable".to_string()).into()
            }
        })?;
    let recording_id = match outcome {
        FinalizeOutcome::Finalized { recording_id }
        | FinalizeOutcome::AlreadyFinalized { recording_id } => recording_id,
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(FinalizeResponse {
            recording_id,
            state: RecordingStateName::Processing,
        }),
    ))
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
        call(pool, owner, path_suffix, Body::empty()).await
    }

    async fn call(
        pool: &PgPool,
        owner: &Owner,
        path_suffix: &str,
        body: Body,
    ) -> (StatusCode, Value) {
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
                    .header("content-type", "application/json")
                    .body(body)
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

    async fn ack(
        pool: &PgPool,
        owner: &Owner,
        idx: u32,
        size: usize,
        sha256: &str,
    ) -> (StatusCode, Value) {
        call(
            pool,
            owner,
            &format!("{idx}/ack"),
            Body::from(serde_json::json!({ "size_bytes": size, "sha256": sha256 }).to_string()),
        )
        .await
    }

    /// Presigns chunk `idx` and PUTs `bytes` to it in real MinIO.
    async fn upload(pool: &PgPool, owner: &Owner, idx: u32, bytes: &[u8]) {
        let (status, body) = presign(pool, owner, &format!("{idx}/url")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let put = reqwest::Client::new()
            .put(body["urls"][0]["url"].as_str().expect("url"))
            .body(bytes.to_vec())
            .send()
            .await
            .expect("PUT reaches MinIO");
        assert!(put.status().is_success(), "PUT got {}", put.status());
    }

    const HASH_A: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const HASH_B: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// Day 35's Check: idempotency and conflict over HTTP, against real storage.
    #[sqlx::test(migrations = "../../migrations")]
    async fn ack_is_idempotent_and_a_different_hash_is_409(pool: PgPool) {
        let owner = owner_with_take(&pool).await;
        let bytes = vec![1u8; 2048];

        let (status, body) = ack(&pool, &owner, 0, bytes.len(), HASH_A).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "not uploaded yet: {body}"
        );

        upload(&pool, &owner, 0, &bytes).await;
        let (status, body) = ack(&pool, &owner, 0, bytes.len() + 1, HASH_A).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "size differs from storage: {body}"
        );

        let (status, body) = ack(&pool, &owner, 0, bytes.len(), HASH_A).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "acked");
        let (status, body) = ack(&pool, &owner, 0, bytes.len(), HASH_A).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "already_acked");

        let (status, body) = ack(&pool, &owner, 0, bytes.len(), HASH_B).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["status"], 409);

        let count = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM chunks WHERE take_id = $1"#,
            owner.take_id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 1);
        test_store()
            .delete_prefix(&format!("ws/{}/", owner.workspace_id))
            .await
            .expect("clean up");
    }

    async fn get_status(pool: &PgPool, owner: &Owner) -> (StatusCode, Value) {
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
                    .uri(format!("/api/v1/takes/{}/status", owner.take_id))
                    .header("cookie", &owner.cookie)
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

    async fn finalize(
        pool: &PgPool,
        owner: &Owner,
        chunk_count: u32,
    ) -> (StatusCode, Value, Option<String>) {
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
                    .uri(format!("/api/v1/takes/{}/finalize", owner.take_id))
                    .header("cookie", &owner.cookie)
                    .header(CSRF_HEADER_NAME, "token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "chunk_count": chunk_count, "duration_ms": 4000 })
                            .to_string(),
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            content_type,
        )
    }

    /// Day 36's Check over HTTP: a gap → 422 listing the missing indexes; fill it → 202.
    #[sqlx::test(migrations = "../../migrations")]
    async fn finalize_with_a_gap_returns_the_missing_indexes(pool: PgPool) {
        let owner = owner_with_take(&pool).await;
        for idx in [0, 2] {
            upload(&pool, &owner, idx, b"chunk").await;
            let (status, body) = ack(&pool, &owner, idx, 5, HASH_A).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }

        let (status, body) = get_status(&pool, &owner).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["received"], serde_json::json!([0, 2]));
        assert_eq!(
            body["chunks"][1],
            serde_json::json!({ "idx": 2, "size_bytes": 5, "sha256": HASH_A })
        );
        assert_eq!(body["finalized"], false);

        let (status, body, content_type) = finalize(&pool, &owner, 4).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(content_type.as_deref(), Some("application/problem+json"));
        assert_eq!(body["missing"], serde_json::json!([1, 3]));
        assert_eq!(body["missing_count"], 2);
        assert_eq!(body["status"], 422);

        for idx in [1, 3] {
            upload(&pool, &owner, idx, b"chunk").await;
            ack(&pool, &owner, idx, 5, HASH_A).await;
        }
        let (status, body, _) = finalize(&pool, &owner, 4).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["state"], "processing");
        let (status, _, _) = finalize(&pool, &owner, 4).await;
        assert_eq!(status, StatusCode::ACCEPTED, "retry is idempotent");
        let (_, body) = get_status(&pool, &owner).await;
        assert_eq!(body["finalized"], true);

        test_store()
            .delete_prefix(&format!("ws/{}/", owner.workspace_id))
            .await
            .expect("clean up");
    }
}
