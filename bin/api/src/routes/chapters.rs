use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kernel::{Permission, RecordingId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::recordings::{manage_error, require_owner_or_admin};
use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem};
use crate::workspace_context::WorkspaceContext;

/// A titled start time.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChapterDto {
    /// Milliseconds from the start of the recording.
    pub start_ms: u32,
    /// 1 to 120 characters.
    pub title: String,
}

impl From<catalog::Chapter> for ChapterDto {
    fn from(chapter: catalog::Chapter) -> Self {
        Self {
            start_ms: chapter.start_ms,
            title: chapter.title,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChaptersBody {
    /// The whole list: it replaces the recording's chapters. At most 100; unique start times
    /// within the recording's length.
    pub chapters: Vec<ChapterDto>,
}

/// The recording's chapters, earliest first.
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/chapters",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "The chapters", body = ChaptersBody),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn get_chapters(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<Json<ChaptersBody>, ApiError> {
    ctx.require(Permission::EditRecording)?;
    require_owner_or_admin(&state, &ctx, recording_id).await?;
    let chapters = state
        .recordings
        .chapters(ctx.workspace_id, recording_id)
        .await
        .map_err(manage_error)?;
    Ok(Json(ChaptersBody {
        chapters: chapters.into_iter().map(ChapterDto::from).collect(),
    }))
}

/// Replaces the recording's chapters (owner or admin). They show on the watch page as a table
/// of contents and as marks on the scrub bar.
#[utoipa::path(
    put,
    path = "/api/v1/recordings/{recording_id}/chapters",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    request_body = ChaptersBody,
    responses(
        (status = 200, description = "The chapters as stored (sorted, titles trimmed)", body = ChaptersBody),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or not the owner and not an admin", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Too many, duplicate or out-of-range chapters, or a bad title", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn put_chapters(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path(recording_id): Path<RecordingId>,
    Json(body): Json<ChaptersBody>,
) -> Result<Json<ChaptersBody>, ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    require_owner_or_admin(&state, &ctx, recording_id).await?;
    let chapters = state
        .recordings
        .set_chapters(
            ctx.workspace_id,
            recording_id,
            body.chapters
                .into_iter()
                .map(|chapter| catalog::Chapter {
                    start_ms: chapter.start_ms,
                    title: chapter.title,
                })
                .collect(),
        )
        .await
        .map_err(manage_error)?;
    Ok(Json(ChaptersBody {
        chapters: chapters.into_iter().map(ChapterDto::from).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use sqlx::PgPool;

    use crate::routes::testkit::{call, caller, link, recording, renditions};

    fn uri(id: kernel::RecordingId) -> String {
        format!("/api/v1/recordings/{id}/chapters")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_owner_sets_chapters_and_a_viewer_sees_them_sorted(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        renditions(&pool, &owner, id).await;
        let (slug, _) = link(&pool, &owner, id, "link").await;

        let none = call(&pool, Some(&owner), Method::GET, &uri(id), None).await;
        assert_eq!(none.status, StatusCode::OK);
        assert_eq!(none.body["chapters"], json!([]));

        let put = call(
            &pool,
            Some(&owner),
            Method::PUT,
            &uri(id),
            Some(json!({"chapters": [
                {"start_ms": 8000, "title": "  Wrap-up "},
                {"start_ms": 0, "title": "Intro"},
            ]})),
        )
        .await;
        assert_eq!(put.status, StatusCode::OK, "{}", put.body);
        assert_eq!(
            put.body["chapters"],
            json!([{"start_ms": 0, "title": "Intro"}, {"start_ms": 8000, "title": "Wrap-up"}])
        );

        // A stranger with the link sees them on the watch data.
        let page = call(&pool, None, Method::GET, &format!("/api/v1/s/{slug}"), None).await;
        assert_eq!(page.body["chapters"], put.body["chapters"]);

        // Replacing is wholesale.
        let again = call(
            &pool,
            Some(&owner),
            Method::PUT,
            &uri(id),
            Some(json!({"chapters": [{"start_ms": 1000, "title": "Only"}]})),
        )
        .await;
        assert_eq!(
            again.body["chapters"],
            json!([{"start_ms": 1000, "title": "Only"}])
        );
        let cleared = call(
            &pool,
            Some(&owner),
            Method::PUT,
            &uri(id),
            Some(json!({"chapters": []})),
        )
        .await;
        assert_eq!(cleared.body["chapters"], json!([]));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn bad_chapter_lists_are_422_and_change_nothing(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        call(
            &pool,
            Some(&owner),
            Method::PUT,
            &uri(id),
            Some(json!({"chapters": [{"start_ms": 0, "title": "Keep"}]})),
        )
        .await;
        for bad in [
            json!({"chapters": [{"start_ms": 5, "title": "a"}, {"start_ms": 5, "title": "b"}]}),
            json!({"chapters": [{"start_ms": 0, "title": "  "}]}),
            // The test recording is 12 s long.
            json!({"chapters": [{"start_ms": 12_001, "title": "late"}]}),
        ] {
            let reply = call(
                &pool,
                Some(&owner),
                Method::PUT,
                &uri(id),
                Some(bad.clone()),
            )
            .await;
            assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        }
        let kept = call(&pool, Some(&owner), Method::GET, &uri(id), None).await;
        assert_eq!(
            kept.body["chapters"],
            json!([{"start_ms": 0, "title": "Keep"}])
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_the_owners_workspace_can_read_or_change_them(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "ready").await;
        let other = caller(&pool).await;
        for method in [Method::GET, Method::PUT] {
            let body = (method == Method::PUT).then(|| json!({"chapters": []}));
            let stranger = call(&pool, Some(&other), method.clone(), &uri(id), body.clone()).await;
            assert_eq!(stranger.status, StatusCode::NOT_FOUND, "{method}");
            let anon = call(&pool, None, method.clone(), &uri(id), body).await;
            assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{method}");
        }
    }
}
