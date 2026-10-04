pub mod auth;
pub mod links;
pub mod me;
pub mod recordings;
pub mod takes;
#[cfg(test)]
pub mod testkit;
pub mod verify_email;
pub mod watch;

use axum::handler::Handler;
use axum::http::Method;
use axum::routing::{MethodFilter, MethodRouter, on};
use serde::Serialize;
use utoipa::ToSchema;

use crate::app::{AppState, healthz, readyz};

/// The body of every endpoint that only reports an outcome.
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: &'static str,
}

/// Who may call a route. Every route must pick one -- the tenant-isolation harness
/// (`crate::tenant_isolation`) generates its test cases from this, so a route cannot be added
/// without also being classified (CLAUDE.md rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// No session needed (health checks, the auth endpoints themselves).
    Public,
    /// Needs a session but touches only the caller's own user data (`SessionClaims`).
    Session,
    /// Reached by a share link's slug, with or without a session. What the viewer may see is
    /// decided by `sharing::decide`; whatever they may not see is `404`. The tenant-isolation
    /// harness checks a private link against another tenant and an anonymous visitor.
    Viewer,
    /// Touches workspace-owned resources (`WorkspaceContext`). Needs a cross-tenant probe in
    /// the tenant-isolation table.
    Workspace,
}

pub struct Route {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the tenant-isolation harness")
    )]
    pub method: Method,
    pub path: &'static str,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the tenant-isolation harness")
    )]
    pub access: Access,
    pub handler: MethodRouter<AppState>,
    /// The handler's type path (`api::routes::links::create_link`), for the classification lint
    /// in the tenant-isolation harness.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the tenant-isolation harness")
    )]
    pub handler_name: &'static str,
}

impl Route {
    pub fn new<H, T>(method: Method, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        let filter = MethodFilter::try_from(method.clone())
            .unwrap_or_else(|_| panic!("unsupported method {method} for {path}"));
        Self {
            method,
            path,
            access,
            handler_name: std::any::type_name::<H>(),
            handler: on(filter, handler),
        }
    }
}

/// The single list of API routes. `build_router` mounts exactly these.
pub fn table() -> Vec<Route> {
    use Access::{Public, Session, Viewer, Workspace};
    vec![
        Route::new(Method::GET, "/healthz", Public, healthz),
        Route::new(Method::GET, "/readyz", Public, readyz),
        Route::new(
            Method::POST,
            "/api/v1/auth/register",
            Public,
            auth::register,
        ),
        Route::new(Method::POST, "/api/v1/auth/login", Public, auth::login),
        Route::new(Method::POST, "/api/v1/auth/refresh", Public, auth::refresh),
        Route::new(Method::POST, "/api/v1/auth/logout", Public, auth::logout),
        Route::new(
            Method::POST,
            "/api/v1/auth/logout-all",
            Session,
            auth::logout_all,
        ),
        Route::new(
            Method::POST,
            "/api/v1/auth/verify-email",
            Public,
            verify_email::verify_email,
        ),
        Route::new(
            Method::POST,
            "/api/v1/auth/password/forgot",
            Public,
            auth::forgot_password,
        ),
        Route::new(
            Method::POST,
            "/api/v1/auth/password/reset",
            Public,
            auth::reset_password,
        ),
        Route::new(Method::GET, "/api/v1/me", Session, me::me),
        Route::new(Method::PATCH, "/api/v1/me", Session, me::update_me),
        Route::new(
            Method::POST,
            "/api/v1/recordings",
            Workspace,
            recordings::create_recording,
        ),
        Route::new(
            Method::POST,
            "/api/v1/recordings/{recording_id}/retry",
            Workspace,
            recordings::retry_recording,
        ),
        Route::new(Method::GET, "/api/v1/s/{slug}", Viewer, watch::watch),
        Route::new(
            Method::GET,
            "/api/v1/s/{slug}/download",
            Viewer,
            watch::download,
        ),
        Route::new(
            Method::GET,
            "/api/v1/s/{slug}/playback",
            Viewer,
            watch::playback,
        ),
        Route::new(
            Method::GET,
            "/api/v1/recordings",
            Workspace,
            recordings::list_recordings,
        ),
        Route::new(
            Method::PATCH,
            "/api/v1/recordings/{recording_id}",
            Workspace,
            recordings::rename_recording,
        ),
        Route::new(
            Method::DELETE,
            "/api/v1/recordings/{recording_id}",
            Workspace,
            recordings::trash_recording,
        ),
        Route::new(
            Method::GET,
            "/api/v1/recordings/{recording_id}/download",
            Workspace,
            recordings::download_recording,
        ),
        Route::new(
            Method::POST,
            "/api/v1/recordings/{recording_id}/links",
            Workspace,
            links::create_link,
        ),
        Route::new(
            Method::GET,
            "/api/v1/recordings/{recording_id}/links",
            Workspace,
            links::list_links,
        ),
        Route::new(
            Method::PATCH,
            "/api/v1/recordings/{recording_id}/links/{link_id}",
            Workspace,
            links::update_link,
        ),
        Route::new(
            Method::DELETE,
            "/api/v1/recordings/{recording_id}/links/{link_id}",
            Workspace,
            links::revoke_link,
        ),
        Route::new(
            Method::POST,
            "/api/v1/takes/{take_id}/chunks/{idx}/url",
            Workspace,
            takes::presign_chunks,
        ),
        Route::new(
            Method::POST,
            "/api/v1/takes/{take_id}/chunks/{idx}/ack",
            Workspace,
            takes::ack_chunk,
        ),
        Route::new(
            Method::GET,
            "/api/v1/takes/{take_id}/status",
            Workspace,
            takes::take_status,
        ),
        Route::new(
            Method::POST,
            "/api/v1/takes/{take_id}/finalize",
            Workspace,
            takes::finalize_take,
        ),
    ]
}
