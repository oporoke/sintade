pub mod auth;
pub mod me;
pub mod verify_email;

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
    /// Touches workspace-owned resources (`WorkspaceContext`). Needs a cross-tenant probe in
    /// the tenant-isolation table.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no workspace-scoped route until Day 33; the tenant-isolation harness uses it"
        )
    )]
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
            handler: on(filter, handler),
        }
    }
}

/// The single list of API routes. `build_router` mounts exactly these.
pub fn table() -> Vec<Route> {
    use Access::{Public, Session};
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
    ]
}
