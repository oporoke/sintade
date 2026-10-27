use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use kernel::{AppError, Permission, RecordingId};
use media::SubmitEditError;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::recordings::require_owner_or_admin;
use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem};
use crate::workspace_context::WorkspaceContext;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EditBody {
    /// The ranges of the original to keep, as `[start_ms, end_ms]` pairs in any order. They must
    /// not overlap, each is at least 100 ms, none ends after the recording, and there are at most
    /// 100. Everything not listed is cut. Touching ranges are joined.
    #[schema(example = json!([[0, 12000], [15000, 60000]]))]
    pub ranges: Vec<[u32; 2]>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EditAccepted {
    pub id: uuid::Uuid,
    /// 1 for the recording's first saved edit, then counting up.
    pub version: u32,
    /// How long the edited recording will be.
    pub kept_ms: u32,
    /// The length of the original the ranges were checked against.
    pub source_duration_ms: u32,
    /// `queued`: the render has been scheduled.
    pub status: String,
}

/// Saves an edit of the recording and queues its render (owner or admin). The original is never
/// changed; the edited MP4 is made as a new version.
#[utoipa::path(
    post,
    path = "/api/v1/recordings/{recording_id}/edits",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    request_body = EditBody,
    responses(
        (status = 202, description = "Saved; the render is queued", body = EditAccepted),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or not the owner and not an admin", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The recording has no MP4 yet (still processing, or failed)", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Overlapping, too short, past-the-end, empty or too many ranges, or ranges that keep everything", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn post_edit(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path(recording_id): Path<RecordingId>,
    Json(body): Json<EditBody>,
) -> Result<(StatusCode, Json<EditAccepted>), ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    require_owner_or_admin(&state, &ctx, recording_id).await?;
    let submitted = state
        .edits
        .submit(ctx.workspace_id, recording_id, ctx.user_id, &body.ranges)
        .await
        .map_err(|error| match error {
            SubmitEditError::NotFound => ApiError::from(AppError::NotFound),
            SubmitEditError::NotReady => ApiError::from(AppError::Conflict(error.to_string())),
            SubmitEditError::Invalid(_) | SubmitEditError::NoChange => {
                ApiError::from(AppError::Validation(error.to_string()))
            }
            other => {
                tracing::error!(error = %other, "submit edit failed");
                ApiError::from(AppError::Internal("could not save the edit".to_string()))
            }
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(EditAccepted {
            id: submitted.edit_id,
            version: submitted.version,
            kept_ms: submitted.kept_ms,
            source_duration_ms: submitted.source_duration_ms,
            status: "queued".to_string(),
        }),
    ))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct WaveformResponse {
    /// A signed URL of the peaks JSON (ADR-0039): `{version, duration_ms, bucket_ms, has_audio,
    /// peaks}`, one peak (0–255) per `bucket_ms` of the original recording.
    pub url: String,
    pub expires_in_s: u64,
}

/// The waveform peaks of the original recording, for the editor's timeline (owner or admin).
/// Peaks are made after processing; for a recording made before that existed, asking queues
/// them and answers `409` until they are ready.
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/waveform",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "Signed URL of the peaks JSON", body = WaveformResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Not the owner and not an admin", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The peaks aren't made yet; try again shortly", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn get_waveform(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<axum::response::Response, ApiError> {
    ctx.require(Permission::EditRecording)?;
    require_owner_or_admin(&state, &ctx, recording_id).await?;
    let grant = state
        .delivery
        .waveform(ctx.workspace_id, recording_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "waveform failed");
            ApiError::from(AppError::Internal("waveform unavailable".to_string()))
        })?
        .ok_or_else(|| {
            ApiError::from(AppError::Conflict(
                "the waveform is being made; try again shortly".to_string(),
            ))
        })?;
    Ok(super::watch::no_store(Json(WaveformResponse {
        url: grant.url,
        expires_in_s: grant.expires_in_s,
    })))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EditorSourceResponse {
    /// A signed URL of the **original** MP4: the editor always works in the original's time,
    /// whichever version viewers get.
    pub video_url: String,
    /// The original's length; the timeline spans exactly this.
    pub duration_ms: u32,
    /// The original's scrub-sprite cue file, each sheet signed (`sheet-url#xywh=x,y,w,h`), for
    /// the timeline's thumbnails; `null` when the recording has no sprite.
    pub sprite_vtt: Option<String>,
    pub expires_in_s: u64,
}

/// What the editor works on (owner or admin): the original recording, its length and its
/// thumbnails, all in original time (ADR-0040).
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/editor",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "The original, signed", body = EditorSourceResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Not the owner and not an admin", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The recording has no MP4 yet (still processing, or failed)", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn get_editor_source(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<axum::response::Response, ApiError> {
    ctx.require(Permission::EditRecording)?;
    require_owner_or_admin(&state, &ctx, recording_id).await?;
    let source = state
        .delivery
        .editor_source(ctx.workspace_id, recording_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "editor source failed");
            ApiError::from(AppError::Internal("the editor is unavailable".to_string()))
        })?
        .ok_or_else(|| {
            ApiError::from(AppError::Conflict(
                "this recording is still being processed".to_string(),
            ))
        })?;
    Ok(super::watch::no_store(Json(EditorSourceResponse {
        video_url: source.video_url,
        duration_ms: source.duration_ms,
        sprite_vtt: source.sprite_vtt,
        expires_in_s: source.expires_in_s,
    })))
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use sqlx::PgPool;

    use crate::routes::testkit::{call, caller, recording, renditions};

    fn uri(id: kernel::RecordingId) -> String {
        format!("/api/v1/recordings/{id}/edits")
    }

    async fn queued(pool: &PgPool) -> Vec<serde_json::Value> {
        sqlx::query_scalar!(r#"SELECT payload FROM jobs WHERE kind = 'RenderEdit' ORDER BY id"#)
            .fetch_all(pool)
            .await
            .expect("jobs")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_owner_saves_edits_that_queue_a_render_and_count_up(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;

        // A trim and a cut, given out of order: 3 s + 5 s kept of 12 s.
        let first = call(
            &pool,
            Some(&owner),
            Method::POST,
            &uri(id),
            Some(json!({"ranges": [[6000, 11000], [1000, 4000]]})),
        )
        .await;
        assert_eq!(first.status, StatusCode::ACCEPTED, "{}", first.body);
        assert_eq!(first.body["version"], 1);
        assert_eq!(first.body["kept_ms"], 8000);
        assert_eq!(first.body["source_duration_ms"], 12000);
        assert_eq!(first.body["status"], "queued");

        let second = call(
            &pool,
            Some(&owner),
            Method::POST,
            &uri(id),
            Some(json!({"ranges": [[0, 2000]]})),
        )
        .await;
        assert_eq!(second.body["version"], 2);
        let jobs = queued(&pool).await;
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0]["edit_id"], first.body["id"]);
        let stored = sqlx::query_scalar!(
            "SELECT edl FROM edits WHERE recording_id = $1 AND version = 1",
            id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("edl");
        assert_eq!(stored, json!([[1000, 4000], [6000, 11000]]));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn bad_ranges_are_422_and_save_nothing(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;
        for bad in [
            json!({"ranges": []}),
            json!({"ranges": [[0, 3000], [2000, 5000]]}),
            json!({"ranges": [[5000, 5000]]}),
            json!({"ranges": [[6000, 5000]]}),
            json!({"ranges": [[0, 50]]}),
            json!({"ranges": [[0, 12_001]]}),
            // Keeping everything changes nothing.
            json!({"ranges": [[0, 12_000]]}),
        ] {
            let reply = call(
                &pool,
                Some(&owner),
                Method::POST,
                &uri(id),
                Some(bad.clone()),
            )
            .await;
            assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        }
        // Not the shape: rejected before it gets that far.
        for malformed in [
            json!({}),
            json!({"ranges": [[1, 2, 3]]}),
            json!({"ranges": [[-1, 5]]}),
        ] {
            let reply = call(
                &pool,
                Some(&owner),
                Method::POST,
                &uri(id),
                Some(malformed.clone()),
            )
            .await;
            assert!(reply.status.is_client_error(), "{malformed}");
        }
        assert!(queued(&pool).await.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_recording_without_an_mp4_is_409(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let reply = call(
            &pool,
            Some(&owner),
            Method::POST,
            &uri(id),
            Some(json!({"ranges": [[0, 2000]]})),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CONFLICT);
        assert!(queued(&pool).await.is_empty());
    }

    fn waveform_uri(id: kernel::RecordingId) -> String {
        format!("/api/v1/recordings/{id}/waveform")
    }

    async fn with_waveform(pool: &PgPool, id: kernel::RecordingId) {
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
             SELECT $1, workspace_id, recording_id, take_id, 'waveform', 'peaks', 'ws/rec/img/waveform.json'
             FROM renditions WHERE recording_id = $2 AND kind = 'mp4'",
            uuid::Uuid::now_v7(),
            id.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("waveform");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_owner_gets_a_signed_url_of_the_peaks_once_they_are_made(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;
        let before = call(&pool, Some(&owner), Method::GET, &waveform_uri(id), None).await;
        assert_eq!(before.status, StatusCode::CONFLICT, "{}", before.body);

        with_waveform(&pool, id).await;
        let reply = call(&pool, Some(&owner), Method::GET, &waveform_uri(id), None).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        let url = reply.body["url"].as_str().expect("url");
        assert!(url.contains("ws/rec/img/waveform.json"), "{url}");
        assert_eq!(reply.body["expires_in_s"], 900);
        assert_eq!(reply.cache_control.as_deref(), Some("no-store"));

        let other = caller(&pool).await;
        let stranger = call(&pool, Some(&other), Method::GET, &waveform_uri(id), None).await;
        assert_eq!(stranger.status, StatusCode::NOT_FOUND);
        let anon = call(&pool, None, Method::GET, &waveform_uri(id), None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
    }

    /// Day 93: the editor gets the original's MP4 and length, signed and not cached; nothing
    /// before processing, and nothing for anyone outside the workspace.
    #[sqlx::test(migrations = "../../migrations")]
    async fn the_owner_gets_the_original_to_edit(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let uri = format!("/api/v1/recordings/{id}/editor");
        let before = call(&pool, Some(&owner), Method::GET, &uri, None).await;
        assert_eq!(before.status, StatusCode::CONFLICT, "{}", before.body);

        renditions(&pool, &owner, id).await;
        let reply = call(&pool, Some(&owner), Method::GET, &uri, None).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        let url = reply.body["video_url"].as_str().expect("url");
        let original = format!("ws/{}/rec/{id}/mp4/default.mp4", owner.workspace_id);
        assert!(url.contains(&original), "{url}");
        assert_eq!(reply.body["duration_ms"], 12000);
        assert_eq!(reply.body["sprite_vtt"], serde_json::Value::Null);
        assert_eq!(reply.body["expires_in_s"], 900);
        assert_eq!(reply.cache_control.as_deref(), Some("no-store"));

        let other = caller(&pool).await;
        let stranger = call(&pool, Some(&other), Method::GET, &uri, None).await;
        assert_eq!(stranger.status, StatusCode::NOT_FOUND);
        let anon = call(&pool, None, Method::GET, &uri, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_the_owner_and_admins_of_the_workspace_can_edit(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;
        let body = Some(json!({"ranges": [[0, 2000]]}));
        let other = caller(&pool).await;
        let stranger = call(&pool, Some(&other), Method::POST, &uri(id), body.clone()).await;
        assert_eq!(stranger.status, StatusCode::NOT_FOUND);
        let anon = call(&pool, None, Method::POST, &uri(id), body).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
        assert!(queued(&pool).await.is_empty());
    }
}
