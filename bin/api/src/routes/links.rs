use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use kernel::{AppError, Permission, RecordingId, ShareLinkId};
use serde::{Deserialize, Deserializer, Serialize};
use sharing::{LinkPatch, NewLink, ShareLinkView, SharingError, Visibility};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::csrf::verify_csrf;
use crate::error::{ApiError, Problem};
use crate::workspace_context::WorkspaceContext;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum VisibilityDto {
    Private,
    Workspace,
    Link,
    Public,
}

impl From<VisibilityDto> for Visibility {
    fn from(value: VisibilityDto) -> Self {
        match value {
            VisibilityDto::Private => Self::Private,
            VisibilityDto::Workspace => Self::Workspace,
            VisibilityDto::Link => Self::Link,
            VisibilityDto::Public => Self::Public,
        }
    }
}

impl From<Visibility> for VisibilityDto {
    fn from(value: Visibility) -> Self {
        match value {
            Visibility::Private => Self::Private,
            Visibility::Workspace => Self::Workspace,
            Visibility::Link => Self::Link,
            Visibility::Public => Self::Public,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLinkBody {
    /// Defaults to `link` (anyone with the link).
    pub visibility: Option<VisibilityDto>,
    /// Whether viewers may download the MP4. Defaults to `false`.
    pub allow_download: Option<bool>,
    /// RFC 3339 instant after which the link stops working; in the future.
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLinkBody {
    pub visibility: Option<VisibilityDto>,
    pub allow_download: Option<bool>,
    /// A future RFC 3339 instant sets the expiry; `null` removes it; absent leaves it.
    #[serde(default, deserialize_with = "present")]
    pub expires_at: Option<Option<String>>,
}

/// Distinguishes `"field": null` (clear) from an absent field (leave alone).
fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShareLinkResponse {
    #[schema(value_type = uuid::Uuid)]
    pub id: ShareLinkId,
    #[schema(value_type = uuid::Uuid)]
    pub recording_id: RecordingId,
    /// The public identifier: the watch page is `/s/{slug}`.
    pub slug: String,
    pub visibility: VisibilityDto,
    pub allow_download: bool,
    /// RFC 3339, or `null` for no expiry.
    pub expires_at: Option<String>,
    /// RFC 3339, or `null` while the link works.
    pub revoked_at: Option<String>,
    pub created_at: String,
}

fn rfc3339(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).unwrap_or_default()
}

impl From<ShareLinkView> for ShareLinkResponse {
    fn from(link: ShareLinkView) -> Self {
        Self {
            id: link.id,
            recording_id: link.recording_id,
            slug: link.slug,
            visibility: link.visibility.into(),
            allow_download: link.allow_download,
            expires_at: link.expires_at.map(rfc3339),
            revoked_at: link.revoked_at.map(rfc3339),
            created_at: rfc3339(link.created_at),
        }
    }
}

fn parse_instant(raw: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(raw, &Rfc3339).map_err(|_| {
        AppError::Validation("expires_at must be an RFC 3339 instant".to_string()).into()
    })
}

fn map_error(error: SharingError) -> ApiError {
    match error {
        SharingError::NotFound => AppError::NotFound.into(),
        SharingError::ExpiryInPast => AppError::Validation(error.to_string()).into(),
        other => {
            tracing::error!(error = %other, "sharing failed");
            AppError::Internal("sharing unavailable".to_string()).into()
        }
    }
}

/// Creates a share link for a recording in the caller's workspace.
#[utoipa::path(
    post,
    path = "/api/v1/recordings/{recording_id}/links",
    tag = "sharing",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording to share")),
    request_body = CreateLinkBody,
    responses(
        (status = 201, description = "Link created", body = ShareLinkResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or the role can't edit recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Invalid expiry", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn create_link(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path(recording_id): Path<RecordingId>,
    Json(body): Json<CreateLinkBody>,
) -> Result<(StatusCode, Json<ShareLinkResponse>), ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    let expires_at = body.expires_at.as_deref().map(parse_instant).transpose()?;
    let link = state
        .sharing
        .create_link(
            ctx.workspace_id,
            ctx.user_id,
            recording_id,
            NewLink {
                visibility: body.visibility.map_or(Visibility::Link, Visibility::from),
                allow_download: body.allow_download.unwrap_or(false),
                expires_at,
            },
        )
        .await
        .map_err(map_error)?;
    Ok((StatusCode::CREATED, Json(link.into())))
}

/// Lists a recording's share links, newest first (revoked ones included).
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/links",
    tag = "sharing",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "The recording's links", body = Vec<ShareLinkResponse>),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn list_links(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<Json<Vec<ShareLinkResponse>>, ApiError> {
    let links = state
        .sharing
        .list_links(ctx.workspace_id, recording_id)
        .await
        .map_err(map_error)?;
    Ok(Json(links.into_iter().map(Into::into).collect()))
}

/// Changes a live link's visibility, download flag or expiry.
#[utoipa::path(
    patch,
    path = "/api/v1/recordings/{recording_id}/links/{link_id}",
    tag = "sharing",
    params(
        ("recording_id" = uuid::Uuid, Path, description = "The recording"),
        ("link_id" = uuid::Uuid, Path, description = "The link"),
    ),
    request_body = UpdateLinkBody,
    responses(
        (status = 200, description = "Link updated", body = ShareLinkResponse),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or the role can't edit recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such live link in the caller's workspace", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Invalid expiry", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn update_link(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path((recording_id, link_id)): Path<(RecordingId, ShareLinkId)>,
    Json(body): Json<UpdateLinkBody>,
) -> Result<Json<ShareLinkResponse>, ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    let expires_at = match body.expires_at {
        None => None,
        Some(None) => Some(None),
        Some(Some(raw)) => Some(Some(parse_instant(&raw)?)),
    };
    let link = state
        .sharing
        .update_policy(
            ctx.workspace_id,
            recording_id,
            link_id,
            LinkPatch {
                visibility: body.visibility.map(Visibility::from),
                allow_download: body.allow_download,
                expires_at,
            },
        )
        .await
        .map_err(map_error)?;
    Ok(Json(link.into()))
}

/// Revokes a link: it stops working immediately. Idempotent.
#[utoipa::path(
    delete,
    path = "/api/v1/recordings/{recording_id}/links/{link_id}",
    tag = "sharing",
    params(
        ("recording_id" = uuid::Uuid, Path, description = "The recording"),
        ("link_id" = uuid::Uuid, Path, description = "The link"),
    ),
    responses(
        (status = 204, description = "Link revoked"),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Missing CSRF token, or the role can't edit recordings", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such link in the caller's workspace", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn revoke_link(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    headers: HeaderMap,
    Path((recording_id, link_id)): Path<(RecordingId, ShareLinkId)>,
) -> Result<StatusCode, ApiError> {
    verify_csrf(&headers)?;
    ctx.require(Permission::EditRecording)?;
    state
        .sharing
        .revoke(ctx.workspace_id, recording_id, link_id)
        .await
        .map_err(map_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use kernel::{RecordingId, UserId, WorkspaceId};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::build_router;
    use crate::app::tests::{
        test_clock, test_identity, test_ingest, test_rate_limiter, test_tenancy,
    };
    use crate::csrf::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};
    use crate::session::ACCESS_COOKIE_NAME;

    struct Caller {
        user_id: UserId,
        workspace_id: WorkspaceId,
        cookie: String,
    }

    async fn caller(pool: &PgPool) -> Caller {
        let identity = test_identity(pool.clone());
        let email = format!("links-{}@example.com", uuid::Uuid::now_v7());
        let password = "correct-horse-battery-staple-42".to_string();
        identity
            .register(identity::RegisterRequest {
                email: email.clone(),
                password: password.clone(),
                display_name: "Linker".to_string(),
            })
            .await
            .expect("register");
        let session = identity
            .login(identity::LoginRequest { email, password })
            .await
            .expect("login");
        let claims = identity
            .verify_access_cookie(&session.access_token)
            .expect("token");
        Caller {
            user_id: claims.user_id,
            workspace_id: claims.workspace_id,
            cookie: format!("{ACCESS_COOKIE_NAME}={}", session.access_token),
        }
    }

    async fn recording(pool: &PgPool, caller: &Caller) -> RecordingId {
        let id = RecordingId::new_v7();
        sqlx::query!(
            "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
             VALUES ($1, $2, $3, 'Mine', 'ready')",
            id.into_uuid(),
            caller.workspace_id.into_uuid(),
            caller.user_id.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("recording");
        id
    }

    async fn call(
        pool: &PgPool,
        caller: &Caller,
        method: Method,
        uri: &str,
        body: Option<Value>,
        csrf: bool,
    ) -> (StatusCode, Value) {
        let app = build_router(
            pool.clone(),
            test_identity(pool.clone()),
            test_tenancy(pool.clone()),
            test_ingest(pool.clone()),
            test_rate_limiter(pool.clone()),
            test_clock(),
            "http://localhost:4200",
        );
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        request = if csrf {
            request
                .header(
                    "cookie",
                    format!("{}; {CSRF_COOKIE_NAME}=token", caller.cookie),
                )
                .header(CSRF_HEADER_NAME, "token")
        } else {
            request.header("cookie", caller.cookie.clone())
        };
        let response = app
            .oneshot(
                request
                    .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                    .expect("request"),
            )
            .await
            .expect("call");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_update_list_and_revoke_a_link(pool: PgPool) {
        let alice = caller(&pool).await;
        let recording = recording(&pool, &alice).await;
        let base = format!("/api/v1/recordings/{recording}/links");

        let (status, created) =
            call(&pool, &alice, Method::POST, &base, Some(json!({})), true).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["visibility"], "link");
        assert_eq!(created["allow_download"], false);
        assert_eq!(created["slug"].as_str().map(str::len), Some(12));
        let link_id = created["id"].as_str().expect("id").to_string();

        let one = format!("{base}/{link_id}");
        let (status, updated) = call(
            &pool,
            &alice,
            Method::PATCH,
            &one,
            Some(json!({"visibility": "workspace", "allow_download": true, "expires_at": "2999-01-01T00:00:00Z"})),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{updated}");
        assert_eq!(updated["visibility"], "workspace");
        assert_eq!(updated["allow_download"], true);
        assert!(updated["expires_at"].is_string());

        let (_, cleared) = call(
            &pool,
            &alice,
            Method::PATCH,
            &one,
            Some(json!({"expires_at": null})),
            true,
        )
        .await;
        assert!(cleared["expires_at"].is_null(), "null clears the expiry");
        assert_eq!(cleared["visibility"], "workspace", "absent fields stay");

        let (status, listed) = call(&pool, &alice, Method::GET, &base, None, false).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed.as_array().map(Vec::len), Some(1));

        let (status, _) = call(&pool, &alice, Method::DELETE, &one, None, true).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, listed) = call(&pool, &alice, Method::GET, &base, None, false).await;
        assert!(listed[0]["revoked_at"].is_string());
        let (status, _) = call(&pool, &alice, Method::PATCH, &one, Some(json!({})), true).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a revoked link can't be edited"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn bad_input_and_missing_csrf_are_refused(pool: PgPool) {
        let alice = caller(&pool).await;
        let recording = recording(&pool, &alice).await;
        let base = format!("/api/v1/recordings/{recording}/links");

        let (status, _) = call(&pool, &alice, Method::POST, &base, Some(json!({})), false).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        for body in [
            json!({"expires_at": "yesterday"}),
            json!({"expires_at": "2001-01-01T00:00:00Z"}),
            json!({"visibility": "everyone"}),
        ] {
            let (status, problem) =
                call(&pool, &alice, Method::POST, &base, Some(body), true).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_viewer_cannot_manage_links(pool: PgPool) {
        let viewer = caller(&pool).await;
        let recording = recording(&pool, &viewer).await;
        sqlx::query!(
            "UPDATE memberships SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
            viewer.workspace_id.into_uuid(),
            viewer.user_id.into_uuid(),
        )
        .execute(&pool)
        .await
        .expect("demote");
        let (status, _) = call(
            &pool,
            &viewer,
            Method::POST,
            &format!("/api/v1/recordings/{recording}/links"),
            Some(json!({})),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
