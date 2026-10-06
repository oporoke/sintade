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
    /// Whether this viewer may download the MP4: the link allows it, or they own the recording.
    pub can_download: Option<bool>,
    /// A signed poster URL (15 minutes) once the poster exists.
    pub poster_url: Option<String>,
    /// The owner's chapters, earliest first (empty when there are none).
    pub chapters: Vec<super::chapters::ChapterDto>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackKind {
    /// The fast-start MP4: seekable, plays in every browser.
    Mp4,
    /// The original recording, offered while the MP4 is still being made.
    Preview,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaybackResponse {
    pub kind: PlaybackKind,
    /// A signed URL for the video.
    pub url: String,
    /// The video's MIME type, e.g. `video/mp4` or (for a preview) `video/webm`. A browser that
    /// can't play it should keep showing the processing notice.
    pub content_type: String,
    pub poster_url: Option<String>,
    /// The adaptive (HLS) master playlist, once the recording's ladder is built: same-origin,
    /// with signed segments behind it. Absent before that (the first view asks for the ladder),
    /// when `url` is the one to play.
    pub hls_url: Option<String>,
    /// The scrub sprite's cue file (`/s/{slug}/sprite.vtt`) once the sprite exists.
    pub sprite_url: Option<String>,
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
    is_owner: bool,
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
        is_owner: viewer.owner,
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
                can_download: None,
                poster_url: None,
                chapters: Vec::new(),
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
    let chapters = state
        .recordings
        .chapters(resolved.workspace_id, resolved.recording_id)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "watch: could not read the chapters");
            Vec::new()
        })
        .into_iter()
        .map(super::chapters::ChapterDto::from)
        .collect();
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
        can_download: Some(resolved.link.allow_download || resolved.is_owner),
        poster_url,
        chapters,
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
    // A recording that is still `processing` may already have its original to preview.
    if !matches!(resolved.info.state.as_str(), "ready" | "processing") {
        return Err(
            AppError::Conflict("this recording is still being processed".to_string()).into(),
        );
    }
    let grant = state
        .delivery
        .grant(resolved.workspace_id, resolved.recording_id, true)
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
    let hls_url = match (
        grant.kind,
        state
            .delivery
            .has_ladder(resolved.workspace_id, resolved.recording_id)
            .await,
    ) {
        (delivery::PlaybackKind::Mp4, Ok(true)) => {
            Some(format!("/api/v1/s/{slug}/hls/master.m3u8"))
        }
        (_, Err(error)) => {
            tracing::warn!(%error, "playback: could not look up the HLS ladder");
            None
        }
        _ => None,
    };
    let sprite_url = match state
        .delivery
        .has_sprite(resolved.workspace_id, resolved.recording_id)
        .await
    {
        Ok(true) => Some(format!("/api/v1/s/{slug}/sprite.vtt")),
        Ok(false) => None,
        Err(error) => {
            tracing::warn!(%error, "playback: could not look up the sprite");
            None
        }
    };
    Ok(no_store((
        StatusCode::OK,
        Json(PlaybackResponse {
            hls_url,
            sprite_url,
            kind: match grant.kind {
                delivery::PlaybackKind::Mp4 => PlaybackKind::Mp4,
                delivery::PlaybackKind::Preview => PlaybackKind::Preview,
            },
            url: grant.url,
            content_type: grant.content_type,
            poster_url: grant.poster_url,
            expires_in_s: grant.expires_in_s,
            duration_ms: resolved
                .info
                .duration_ms
                .and_then(|v| u32::try_from(v).ok()),
        }),
    )))
}

const HLS_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";

fn playlist_response(playlist: String) -> Response {
    no_store((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, HLS_CONTENT_TYPE)],
        playlist,
    ))
}

/// Resolves the link and requires that this viewer may watch: `404` when hidden, `401` when a
/// login is needed.
async fn admitted(
    state: &AppState,
    session: &MaybeSession,
    slug: &str,
) -> Result<Resolved, ApiError> {
    let resolved = resolve(state, session, slug).await?;
    match resolved.decision {
        Decision::Hidden => Err(AppError::NotFound.into()),
        Decision::LoginRequired => {
            Err(AppError::Unauthorized("sign in to watch this recording".to_string()).into())
        }
        Decision::Allow => Ok(resolved),
    }
}

fn internal(error: delivery::DeliveryError) -> ApiError {
    tracing::error!(%error, "hls: could not serve a playlist");
    ApiError::from(AppError::Internal("playback unavailable".to_string()))
}

/// The master playlist of the recording's adaptive ladder. It names each rung relatively, so
/// the player asks this API for each rung's playlist next.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/hls/master.m3u8",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "An HLS master playlist", content_type = "application/vnd.apple.mpegurl", body = String),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, not for this viewer, or no ladder yet", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn hls_master(
    State(state): State<AppState>,
    session: MaybeSession,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let resolved = admitted(&state, &session, &slug).await?;
    let playlist = state
        .delivery
        .hls_master(resolved.workspace_id, resolved.recording_id)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)?;
    Ok(playlist_response(playlist))
}

/// The scrub sprite's cue file: each cue names a signed sheet and the tile's rectangle in it.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/sprite.vtt",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "WebVTT whose cues are `<signed sheet url>#xywh=x,y,w,h`", content_type = "text/vtt", body = String),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, not for this viewer, or no sprite yet", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn sprite_vtt(
    State(state): State<AppState>,
    session: MaybeSession,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let resolved = admitted(&state, &session, &slug).await?;
    let vtt = state
        .delivery
        .sprite_vtt(resolved.workspace_id, resolved.recording_id)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)?;
    Ok(no_store((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/vtt")],
        vtt,
    )))
}

/// One rung's playlist, its init and media segments signed for 15 minutes.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/hls/{rung}/index.m3u8",
    tag = "watch",
    params(
        ("slug" = String, Path, description = "The link's slug"),
        ("rung" = String, Path, description = "`360p`, `720p` or `1080p`"),
    ),
    responses(
        (status = 200, description = "An HLS media playlist with signed segment URLs", content_type = "application/vnd.apple.mpegurl", body = String),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, not for this viewer, or no such rung", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn hls_rung(
    State(state): State<AppState>,
    session: MaybeSession,
    Path((slug, rung)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let resolved = admitted(&state, &session, &slug).await?;
    let playlist = state
        .delivery
        .hls_rung(resolved.workspace_id, resolved.recording_id, &rung)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)?;
    Ok(playlist_response(playlist))
}

/// Live status of the recording behind a share link, for a viewer the link admits: the watch
/// page waits on this while the recording is processing instead of polling.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/events",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "A `text/event-stream` of `status` events, each a RecordingStatus", body = super::events::RecordingStatus, content_type = "text/event-stream"),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, or not for this viewer", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn events(
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
    Ok(no_store(super::events::stream(
        state,
        resolved.workspace_id,
        resolved.recording_id,
    )))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadResponse {
    /// A signed URL that saves the MP4; open it (the browser downloads it).
    pub url: String,
    /// The file name the browser will use.
    pub filename: String,
    pub expires_in_s: u64,
}

pub(crate) fn download_response(grant: delivery::DownloadGrant) -> Response {
    no_store(Json(DownloadResponse {
        url: grant.url,
        filename: grant.filename,
        expires_in_s: grant.expires_in_s,
    }))
}

/// A signed download URL for the MP4, when the link allows downloads or the viewer owns the
/// recording.
#[utoipa::path(
    get,
    path = "/api/v1/s/{slug}/download",
    tag = "watch",
    params(("slug" = String, Path, description = "The link's slug")),
    responses(
        (status = 200, description = "Signed download URL", body = DownloadResponse),
        (status = 401, description = "Sign in to watch this link", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "The link doesn't allow downloads", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link, or not for this viewer", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "The MP4 isn't ready yet", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all)]
pub async fn download(
    State(state): State<AppState>,
    session: MaybeSession,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let resolved = resolve(&state, &session, &slug).await?;
    match resolved.decision {
        Decision::Hidden => return Err(AppError::NotFound.into()),
        Decision::LoginRequired => {
            return Err(
                AppError::Unauthorized("sign in to download this recording".to_string()).into(),
            );
        }
        Decision::Allow => {}
    }
    if !(resolved.link.allow_download || resolved.is_owner) {
        return Err(
            AppError::Forbidden("downloads are turned off for this link".to_string()).into(),
        );
    }
    let grant = state
        .delivery
        .download(
            resolved.workspace_id,
            resolved.recording_id,
            &resolved.info.title,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "download: could not sign");
            ApiError::from(AppError::Internal("download unavailable".to_string()))
        })?
        .filter(|_| resolved.info.state == "ready")
        .ok_or_else(|| {
            ApiError::from(AppError::Conflict(
                "this recording is still being processed".to_string(),
            ))
        })?;
    Ok(download_response(grant))
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use sqlx::PgPool;

    use crate::routes::testkit::{
        call, caller, ladder, link, recording, renditions, source_only, sprite,
    };

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
    async fn a_built_ladder_is_offered_and_its_segments_are_signed(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let (slug, _) = link(&pool, &owner, id, "link").await;
        let hls = format!("/api/v1/s/{slug}/hls");

        // No ladder yet: nothing offered, and the playlists are 404.
        let before = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(before.body["hls_url"], serde_json::Value::Null);
        let none = call(
            &pool,
            None,
            Method::GET,
            &format!("{hls}/master.m3u8"),
            None,
        )
        .await;
        assert_eq!(none.status, StatusCode::NOT_FOUND);

        ladder(&pool, owner.workspace_id, id).await;
        let play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(play.body["hls_url"], format!("{hls}/master.m3u8"));

        let master = call(
            &pool,
            None,
            Method::GET,
            &format!("{hls}/master.m3u8"),
            None,
        )
        .await;
        assert_eq!(master.status, StatusCode::OK);
        assert_eq!(
            master.content_type.as_deref(),
            Some("application/vnd.apple.mpegurl")
        );
        assert!(
            master.text.contains("\n720p/index.m3u8\n"),
            "{}",
            master.text
        );

        let rung = call(
            &pool,
            None,
            Method::GET,
            &format!("{hls}/720p/index.m3u8"),
            None,
        )
        .await;
        assert_eq!(rung.status, StatusCode::OK, "{}", rung.text);
        assert!(rung.text.contains("URI=\"http"), "{}", rung.text);
        assert!(rung.text.contains("X-Amz-Signature="), "{}", rung.text);
        assert!(!rung.text.contains("\nseg_0000.m4s"), "{}", rung.text);
        assert_eq!(rung.cache_control.as_deref(), Some("no-store"));

        // A rung that isn't in the ladder, or isn't a rung at all.
        for bad in ["480p", "..", "master"] {
            let reply = call(
                &pool,
                None,
                Method::GET,
                &format!("{hls}/{bad}/index.m3u8"),
                None,
            )
            .await;
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{bad}");
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_sprite_cue_file_has_signed_sheets_and_follows_link_access(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let (slug, link_id) = link(&pool, &owner, id, "link").await;
        let uri = format!("/api/v1/s/{slug}/sprite.vtt");
        let none = call(&pool, None, Method::GET, &uri, None).await;
        assert_eq!(none.status, StatusCode::NOT_FOUND);
        let before = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(before.body["sprite_url"], serde_json::Value::Null);

        sprite(&pool, owner.workspace_id, id).await;
        let play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(play.body["sprite_url"], uri);
        let vtt = call(&pool, None, Method::GET, &uri, None).await;
        assert_eq!(vtt.status, StatusCode::OK, "{}", vtt.text);
        assert_eq!(vtt.content_type.as_deref(), Some("text/vtt"));
        assert!(vtt.text.starts_with("WEBVTT\n"), "{}", vtt.text);
        assert!(vtt.text.contains("X-Amz-Signature="), "{}", vtt.text);
        assert!(vtt.text.contains("#xywh=160,0,160,90"), "{}", vtt.text);
        assert!(!vtt.text.contains("\nsprite_0.jpg"), "{}", vtt.text);

        // A private link hides it like everything else.
        let _ = link_id;
        let (private, _) = link(&pool, &owner, id, "private").await;
        let hidden = call(
            &pool,
            None,
            Method::GET,
            &format!("/api/v1/s/{private}/sprite.vtt"),
            None,
        )
        .await;
        assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_private_links_ladder_is_404_to_everyone_but_the_owner(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        ladder(&pool, owner.workspace_id, id).await;
        let (slug, _) = link(&pool, &owner, id, "private").await;
        let hls = format!("/api/v1/s/{slug}/hls");
        for uri in [
            format!("{hls}/master.m3u8"),
            format!("{hls}/720p/index.m3u8"),
        ] {
            let anon = call(&pool, None, Method::GET, &uri, None).await;
            assert_eq!(anon.status, StatusCode::NOT_FOUND, "{uri}");
            let other = caller(&pool).await;
            let stranger = call(&pool, Some(&other), Method::GET, &uri, None).await;
            assert_eq!(stranger.status, StatusCode::NOT_FOUND, "{uri}");
            let own = call(&pool, Some(&owner), Method::GET, &uri, None).await;
            assert_eq!(own.status, StatusCode::OK, "{uri}");
        }
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
        assert_eq!(play.body["kind"], "mp4");
        assert_eq!(play.body["content_type"], "video/mp4");
        let mp4 = play.body["url"].as_str().expect("url");
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
    async fn a_processing_recording_plays_its_original_until_the_mp4_exists(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        source_only(&pool, &owner, id).await;
        let (slug, _) = link(&pool, &owner, id, "link").await;

        let play = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(play.status, StatusCode::OK, "{}", play.body);
        assert_eq!(play.body["kind"], "preview");
        assert_eq!(play.body["content_type"], "video/webm");
        assert!(
            play.body["url"]
                .as_str()
                .is_some_and(|u| u.contains("source.webm"))
        );
        // The watch data still says processing, so the page keeps checking for the MP4.
        let page = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(page.body["state"], "processing");

        // A private link doesn't leak the original either.
        let (private, _) = link(&pool, &owner, id, "private").await;
        let hidden = call(&pool, None, Method::GET, &playback_uri(&private), None).await;
        assert_eq!(hidden.status, StatusCode::NOT_FOUND);

        // A failed recording's original is not offered.
        sqlx::query!(
            "UPDATE recordings SET state = 'failed' WHERE id = $1",
            id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("fail");
        let failed = call(&pool, None, Method::GET, &playback_uri(&slug), None).await;
        assert_eq!(failed.status, StatusCode::CONFLICT);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn downloads_need_the_link_to_allow_them_or_the_owner(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let (slug, link_id) = link(&pool, &owner, id, "link").await;
        let uri = format!("/api/v1/s/{slug}/download");

        // Not allowed for strangers by default…
        let denied = call(&pool, None, Method::GET, &uri, None).await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN);
        let page = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(page.body["can_download"], false);
        // …but the owner can.
        let own = call(&pool, Some(&owner), Method::GET, &uri, None).await;
        assert_eq!(own.status, StatusCode::OK, "{}", own.body);
        assert_eq!(own.body["filename"], "Mine.mp4");
        assert!(
            own.body["url"]
                .as_str()
                .is_some_and(|u| u.contains("response-content-disposition"))
        );
        assert_eq!(own.cache_control.as_deref(), Some("no-store"));

        // Turning downloads on opens it to everyone with the link.
        let patched = call(
            &pool,
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/recordings/{id}/links/{link_id}"),
            Some(serde_json::json!({"allow_download": true})),
        )
        .await;
        assert_eq!(patched.status, StatusCode::OK);
        let open = call(&pool, None, Method::GET, &uri, None).await;
        assert_eq!(open.status, StatusCode::OK, "{}", open.body);
        let page = call(&pool, None, Method::GET, &watch_uri(&slug), None).await;
        assert_eq!(page.body["can_download"], true);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_hidden_or_unfinished_recording_cannot_be_downloaded(pool: PgPool) {
        let (owner, id) = ready(&pool).await;
        let stranger = caller(&pool).await;
        let (slug, _) = link(&pool, &owner, id, "private").await;
        let uri = format!("/api/v1/s/{slug}/download");
        for viewer in [None, Some(&stranger)] {
            assert_eq!(
                call(&pool, viewer, Method::GET, &uri, None).await.status,
                StatusCode::NOT_FOUND
            );
        }

        let processing = recording(&pool, &owner, "processing").await;
        let (slug, _) = link(&pool, &owner, processing, "link").await;
        let reply = call(
            &pool,
            Some(&owner),
            Method::GET,
            &format!("/api/v1/s/{slug}/download"),
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::CONFLICT);
    }
}
