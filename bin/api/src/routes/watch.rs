use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use kernel::{AppError, Permission, RecordingId, WorkspaceId};
use serde::Serialize;
use sharing::{Decision, ShareLinkView, Viewer, decide};
use time::format_description::well_known::Rfc3339;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::error::{ApiError, Problem};
use crate::session::MaybeSession;

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Requirement {
    /// Anyone with the link can watch.
    None,
    /// Sign in first (a workspace link opened anonymously).
    Login,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WatchState {
    /// The MP4 is still being made.
    Processing,
    Ready,
    /// Processing gave up; the creator can retry.
    Failed,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WatchResponse {
    pub requirement: Requirement,
    /// Absent while a `login` requirement is unmet.
    pub title: Option<String>,
    pub state: Option<WatchState>,
    pub duration_ms: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// RFC 3339.
    pub created_at: Option<String>,
    pub allow_download: Option<bool>,
    /// A signed poster URL (15 minutes) once the poster exists.
    pub poster_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaybackResponse {
    /// A signed URL for the fast-start MP4.
    pub mp4_url: String,
    pub poster_url: Option<String>,
    /// Seconds the URLs stay valid (900).
    pub expires_in_s: u64,
    pub duration_ms: Option<u32>,
}

/// A viewer's link, recording and rights, resolved. Every "no" is `404` (or `401` for a login
/// requirement): nothing here distinguishes a missing slug from a hidden one.
struct Resolved {
    link: ShareLinkView,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    info: catalog::WatchInfo,
    decision: Decision,
}

async fn resolve(
    state: &AppState,
    session: &MaybeSession,
    slug: &str,
) -> Result<Resolved, ApiError> {
    let not_found = || ApiError::from(AppError::NotFound);
    let internal = |error: &dyn std::fmt::Display| {
        tracing::error!(%error, "watch: lookup failed");
        ApiError::from(AppError::Internal("watch page unavailable".to_string()))
    };
    let link = state
        .sharing
        .resolve(slug)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(not_found)?;
    let mut conn = state.pool.acquire().await.map_err(|e| internal(&e))?;
    let info = state
        .catalog
        .watch_info(&mut conn, link.recording_id, link.workspace_id)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(not_found)?;
    drop(conn);
    let viewer = match &session.0 {
        None => Viewer::ANONYMOUS,
        Some(claims) => Viewer {
            signed_in: true,
            workspace_member: state
                .tenancy
                .authorize(claims.user_id, Permission::ViewWorkspace, link.workspace_id)
                .await
                .is_ok(),
            owner: claims.user_id == info.owner_id,
        },
    };
    let decision = decide(&link, viewer);
    Ok(Resolved {
        workspace_id: link.workspace_id,
        recording_id: link.recording_id,
        link,
        info,
        decision,
    })
}

fn watch_state(state: &str) -> Option<WatchState> {
    match state {
        "processing" => Some(WatchState::Processing),
        "ready" => Some(WatchState::Ready),
        "failed" => Some(WatchState::Failed),
        _ => None,
    }
}

/// Watch links are per-viewer and carry signed URLs: never cached by browsers or proxies.
fn no_store<T: IntoResponse>(body: T) -> Response {
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The watch page's data for a share link: title, state, poster and what the viewer must do
/// first. A link the viewer can't see is `404`, like a link that doesn't exist.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "Watch page data, or a login requirement", body = WatchResponse),
        (status = 404, description = "No such live link, or not for this viewer", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn watch(
    State(state): State<AppState>,
    session: MaybeSession,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let resolved = resolve(&state, &session, &slug).await?;
    match resolved.decision {
        Decision::Hidden => return Err(AppError::NotFound.into()),
        Decision::LoginRequired => {
            return Ok(no_store(Json(WatchResponse {
                requirement: Requirement::Login,
                title: None,
                state: None,
                duration_ms: None,
                width: None,
                height: None,
                created_at: None,
                allow_download: None,
                poster_url: None,
            })));
        }
        Decision::Allow => {}
    }
    let Some(watch_state) = watch_state(&resolved.info.state) else {
        // Still recording, abandoned, trashed: nothing to watch.
        return Err(AppError::NotFound.into());
    };
    let poster_url = match watch_state {
        WatchState::Ready => state
            .delivery
            .poster(resolved.workspace_id, resolved.recording_id)
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "watch: could not sign the poster");
                None
            }),
        _ => None,
    };
    let to_u32 = |value: Option<i32>| value.and_then(|v| u32::try_from(v).ok());
    Ok(no_store(Json(WatchResponse {
        requirement: Requirement::None,
        title: Some(resolved.info.title),
        state: Some(watch_state),
        duration_ms: to_u32(resolved.info.duration_ms),
        width: to_u32(resolved.info.width),
        height: to_u32(resolved.info.height),
        created_at: resolved.info.created_at.format(&Rfc3339).ok(),
        allow_download: Some(resolved.link.allow_download),
        poster_url,
    })))
}

/// Signed MP4 (and poster) URLs, valid 15 minutes, for a viewer the link admits.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/playback",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "Signed URLs", body = PlaybackResponse),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, or not for this viewer", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The MP4 isn't ready yet", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn playback(
    State(state): State<AppState>,
    session: MaybeSession,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let resolved = resolve(&state, &session, &slug).await?;
    match resolved.decision {
        Decision::Hidden => return Err(AppError::NotFound.into()),
        Decision::LoginRequired => {
            return Err(
                AppError::Unauthorized("sign in to watch this recording".to_string()).into(),
            );
        }
        Decision::Allow => {}
    }
    if resolved.info.state != "ready" {
        return Err(
            AppError::Conflict("this recording is still being processed".to_string()).into(),
        );
    }
    let grant = state
        .delivery
        .grant(resolved.workspace_id, resolved.recording_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "playback: could not sign");
            ApiError::from(AppError::Internal("playback unavailable".to_string()))
        })?
        .ok_or_else(|| {
            ApiError::from(AppError::Conflict(
                "this recording is still being processed".to_string(),
            ))
        })?;
    Ok(no_store((
        StatusCode::OK,
        Json(PlaybackResponse {
            mp4_url: grant.mp4_url,
            poster_url: grant.poster_url,
            expires_in_s: grant.expires_in_s,
            duration_ms: resolved
                .info
                .duration_ms
                .and_then(|v| u32::try_from(v).ok()),
        }),
    )))
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use sqlx::PgPool;

    use crate::routes::testkit::{call, caller, link, recording, renditions};

    fn watch_uri(slug: &str) -> String {
        format!("/api/v1/s/{slug}")
    }

    fn playback_uri(slug: &str) -> String {
        format!("/api/v1/s/{slug}/playback")
    }

    /// A ready recording with an MP4, owned by a fresh user.
    async fn ready(pool: &PgPool) -> (crate::routes::testkit::Caller, kernel::RecordingId) {
        let owner = caller(pool).await;
        let id = recording(pool, &owner, "ready").await;
        renditions(pool, &owner, id).await;
        (owner, id)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn anyone_with_a_link_can_watch_and_gets_signed_urls(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let (slug, _) = link(&pool, &owner, id, "link").await;

        let page = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        assert_eq!(page.body["requirement"], "none");
        assert_eq!(page.body["title"], "Mine");
        assert_eq!(page.body["state"], "ready");
        assert_eq!(page.body["duration_ms"], 12000);
        assert!(
            page.body["poster_url"]
                .as_str()
                .is_some_and(|u| u.contains("X-Amz-Signature"))
        );
        assert_eq!(page.cache_control.as_deref(), Some("no-store"));

        let play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(play.status, StatusCode::OK, "{}", play.body);
        assert_eq!(play.body["expires_in_s"], 900);
        let mp4 = play.body["mp4_url"].as_str().expect("mp4 url");
        assert!(
            mp4.contains("default.mp4") && mp4.contains("X-Amz-Expires=900"),
            "{mp4}"
        );
        assert_eq!(play.cache_control.as_deref(), Some("no-store"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_private_link_is_404_to_everyone_but_the_owner(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let stranger = caller(&pool).await;
        let (slug, _) = link(&pool, &owner, id, "private").await;

        for viewer in [None, Some(&stranger)] {
            for uri in [watch_uri(&slug), playback_uri(&slug)] {
                let reply = call(&pool, viewer, Method::GET, &uri, None).await;
                assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
            }
        }
        // Indistinguishable from a link that doesn't exist.
        let missing = call(&pool, None, Method::GET, &watch_uri("zzzzzzzzzzzz"), None).await;
        let hidden = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(missing.status, hidden.status);
        assert_eq!(missing.body, hidden.body);

        let own = call(&pool, Some(&owner), Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(own.status, StatusCode::OK);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_workspace_link_wants_a_login_then_a_member(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let outsider = caller(&pool).await;
        let colleague = caller(&pool).await;
        sqlx::query!(
            "INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')",
            owner.workspace_id.into_uuid(),
            colleague.user_id.into_uuid(),
        )
        .execute(&pool)
        .await
        .expect("membership");
        let (slug, _) = link(&pool, &owner, id, "workspace").await;

        let anon = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(anon.status, StatusCode::OK);
        assert_eq!(anon.body["requirement"], "login");
        assert!(anon.body["title"].is_null(), "nothing leaks before login");
        let anon_play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(anon_play.status, StatusCode::UNAUTHORIZED);

        let stranger = call(
            &pool,
            Some(&outsider),
            Method::GET,
            &playback_uri(&slug),
            None,
        )
        .await;
        assert_eq!(stranger.status, StatusCode::NOT_FOUND);

        let member = call(
            &pool,
            Some(&colleague),
            Method::GET,
            &playback_uri(&slug),
            None,
        )
        .await;
        assert_eq!(member.status, StatusCode::OK, "{}", member.body);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn revoking_kills_the_link_at_once(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let (slug, link_id) = link(&pool, &owner, id, "public").await;
        assert_eq!(
            call(&pool, None, Method::GET, &playback_uri(&slug), None)
                .await
                .status,
            StatusCode::OK
        );
        let revoked = call(
            &pool,
            Some(&owner),
            Method::DELETE,
            &format!("/api/v1/recordings/{id}/links/{link_id}"),
            None,
        )
        .await;
        assert_eq!(revoked.status, StatusCode::NO_CONTENT);
        for uri in [watch_uri(&slug), playback_uri(&slug)] {
            assert_eq!(
                call(&pool, None, Method::GET, &uri, None).await.status,
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_recording_that_is_not_ready_has_no_playback_yet(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let (slug, _) = link(&pool, &owner, id, "link").await;
        let page = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_eq!(page.body["state"], "processing");
        assert!(page.body["poster_url"].is_null());
        let play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(play.status, StatusCode::CONFLICT);

        // Still recording, or trashed: not watchable at all.
        let recording_id = recording(&pool, &owner, "recording").await;
        let (slug, _) = link(&pool, &owner, recording_id, "link").await;
        assert_eq!(
            call(&pool, None, Method::GET, &watch_uri(&slug), None)
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn malformed_and_unknown_slugs_are_404(pool: PgPool) {
        for slug in ["x", "short", "has-a-dash!!", "zzzzzzzzzzzz", "a%20b"] {
            assert_eq!(
                call(&pool, None, Method::GET, &watch_uri(slug), None)
                    .await
                    .status,
                StatusCode::NOT_FOUND,
                "{slug}"
            );
        }
    }
}
