//! The generated tenant-isolation harness (`docs/design.md` §18, CLAUDE.md rule 5).
//!
//! Test cases are generated from `routes::table()`, the same list `build_router` mounts, so a
//! new route is covered the moment it exists:
//!
//! - every `Session`/`Workspace` route rejects an anonymous caller with `401`;
//! - every `Public` route is actually mounted (not `404`/`405`);
//! - every `Workspace` route must have an entry in [`tenant_table`], or the harness fails and
//!   names it. Each entry builds a request for a resource owned by workspace A; the harness
//!   sends it as a member of workspace B (must be `404`) and as A's owner (must not be).
//!
//! There are no real workspace-scoped routes yet (the first is `POST /recordings`, Day 33), so a
//! test-only probe route stands in to exercise `WorkspaceContext` end to end.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode};
use kernel::{AppError, UserId, WorkspaceId};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::app::tests::{test_clock, test_identity, test_rate_limiter, test_tenancy};
use crate::app::{AppState, build_router_from};
use crate::error::ApiError;
use crate::routes::{self, Access, Route};
use crate::session::ACCESS_COOKIE_NAME;
use crate::workspace_context::WorkspaceContext;

const TEST_ORIGIN: &str = "https://localhost:4200";
const PROBE_PATH: &str = "/api/v1/__tenant-probe/workspaces/{workspace_id}";

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Creates a resource owned by the given workspace and returns a request that reads or
/// mutates it. The harness adds the session cookie.
type Setup = fn(PgPool, WorkspaceId) -> BoxFuture<Request<Body>>;

/// The tenant-isolation table: one entry per `Access::Workspace` route.
fn tenant_table() -> Vec<(Method, &'static str, Setup)> {
    vec![(Method::GET, PROBE_PATH, |_pool, workspace_id| {
        Box::pin(
            async move { get(&PROBE_PATH.replace("{workspace_id}", &workspace_id.to_string())) },
        )
    })]
}

/// Stands in for a real workspace-scoped read: the resource (here, the workspace itself) is
/// visible only when it belongs to the caller's `WorkspaceContext`.
async fn probe(
    ctx: WorkspaceContext,
    Path(workspace_id): Path<WorkspaceId>,
) -> Result<StatusCode, ApiError> {
    if workspace_id == ctx.workspace_id {
        Ok(StatusCode::OK)
    } else {
        Err(AppError::NotFound.into())
    }
}

fn table_with_probe() -> Vec<Route> {
    let mut table = routes::table();
    table.push(Route::new(
        Method::GET,
        PROBE_PATH,
        Access::Workspace,
        probe,
    ));
    table
}

fn app(pool: &PgPool) -> Router {
    build_router_from(
        table_with_probe(),
        AppState {
            pool: pool.clone(),
            identity: test_identity(pool.clone()),
            tenancy: test_tenancy(pool.clone()),
            rate_limiter: test_rate_limiter(pool.clone()),
            clock: test_clock(),
        },
        TEST_ORIGIN,
    )
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("valid request")
}

/// `/a/{id}/b` -> `/a/<random uuid>/b`, for calls that only need the route to match.
fn concrete_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') {
                uuid::Uuid::now_v7().to_string()
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

struct Tenant {
    user_id: UserId,
    workspace_id: WorkspaceId,
    cookie: String,
}

/// Registers and logs in a fresh user through the real services; returns their personal
/// workspace and a session cookie.
async fn tenant(pool: &PgPool, label: &str) -> Tenant {
    let identity = test_identity(pool.clone());
    let email = format!("tenant-{label}-{}@example.com", uuid::Uuid::now_v7());
    let password = "correct-horse-battery-staple-42".to_string();
    identity
        .register(identity::RegisterRequest {
            email: email.clone(),
            password: password.clone(),
            display_name: format!("Tenant {label}"),
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
    Tenant {
        user_id: claims.user_id,
        workspace_id: claims.workspace_id,
        cookie: format!("{ACCESS_COOKIE_NAME}={}", session.access_token),
    }
}

async fn send(app: &Router, mut request: Request<Body>, cookie: Option<&str>) -> StatusCode {
    if let Some(cookie) = cookie {
        request.headers_mut().insert(
            axum::http::header::COOKIE,
            cookie.parse().expect("valid cookie header"),
        );
    }
    app.clone()
        .oneshot(request)
        .await
        .expect("router call succeeds")
        .status()
}

#[test]
fn route_table_has_no_duplicates() {
    let mut seen = HashSet::new();
    for route in table_with_probe() {
        assert!(
            seen.insert((route.method.clone(), route.path)),
            "{} {} listed twice",
            route.method,
            route.path
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn anonymous_callers_get_401_on_every_protected_route(pool: PgPool) {
    let app = app(&pool);
    for route in table_with_probe() {
        let request = Request::builder()
            .method(route.method.clone())
            .uri(concrete_path(route.path))
            .body(Body::empty())
            .expect("valid request");
        let status = send(&app, request, None).await;
        match route.access {
            Access::Public => assert!(
                status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
                "{} {} is in the route table but not mounted ({status})",
                route.method,
                route.path
            ),
            Access::Session | Access::Workspace => assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{} {} must require a session",
                route.method,
                route.path
            ),
        }
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn every_workspace_route_hides_other_tenants_resources(pool: PgPool) {
    let table = tenant_table();
    let workspace_routes: Vec<Route> = table_with_probe()
        .into_iter()
        .filter(|route| route.access == Access::Workspace)
        .collect();

    for (method, path, _) in &table {
        assert!(
            workspace_routes
                .iter()
                .any(|route| route.method == method && route.path == *path),
            "tenant-isolation table lists {method} {path}, which is not a Workspace route"
        );
    }

    let app = app(&pool);
    let alice = tenant(&pool, "a").await;
    let bob = tenant(&pool, "b").await;
    assert_ne!(alice.workspace_id, bob.workspace_id);

    for route in &workspace_routes {
        let (_, _, setup) = table
            .iter()
            .find(|(method, path, _)| *method == route.method && *path == route.path)
            .unwrap_or_else(|| {
                panic!(
                    "{} {} is a Workspace route but has no entry in the tenant-isolation table \
                     (bin/api/src/tenant_isolation.rs::tenant_table)",
                    route.method, route.path
                )
            });

        let as_bob = send(
            &app,
            setup(pool.clone(), alice.workspace_id).await,
            Some(&bob.cookie),
        )
        .await;
        assert_eq!(
            as_bob,
            StatusCode::NOT_FOUND,
            "{} {}: workspace B reached workspace A's resource",
            route.method,
            route.path
        );

        let as_alice = send(
            &app,
            setup(pool.clone(), alice.workspace_id).await,
            Some(&alice.cookie),
        )
        .await;
        assert!(
            as_alice.is_success(),
            "{} {}: owner of workspace A got {as_alice}; the cross-tenant 404 proves nothing \
             if the route 404s for everyone",
            route.method,
            route.path
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_removed_member_loses_access_on_the_next_request(pool: PgPool) {
    let app = app(&pool);
    let alice = tenant(&pool, "removed").await;
    let uri = PROBE_PATH.replace("{workspace_id}", &alice.workspace_id.to_string());
    assert_eq!(
        send(&app, get(&uri), Some(&alice.cookie)).await,
        StatusCode::OK
    );

    sqlx::query!(
        "DELETE FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        alice.workspace_id.into_uuid(),
        alice.user_id.into_uuid(),
    )
    .execute(&pool)
    .await
    .expect("remove membership");

    // Her access cookie still names the workspace; the membership lookup must win.
    assert_eq!(
        send(&app, get(&uri), Some(&alice.cookie)).await,
        StatusCode::NOT_FOUND
    );
}
