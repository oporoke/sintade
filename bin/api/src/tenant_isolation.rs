//! The generated tenant-isolation harness (`docs/design.md` §18, CLAUDE.md rule 5).
//!
//! Test cases are generated from `routes::table()`, the same list `build_router` mounts, so a
//! new route is covered the moment it exists:
//!
//! - every `Session`/`Workspace` route rejects an anonymous caller with `401`;
//! - every `Public` route is actually mounted (not `404`/`405`);
//! - every `Workspace` route must have an entry in [`tenant_table`], or the harness fails and
//!   names it. An [`Probe::Owned`] entry builds a request for a resource owned by workspace A;
//!   the harness sends it as a member of workspace B (must be `404`) and as A's owner (must
//!   not be). A [`Probe::Create`] entry is for routes that create in the caller's own workspace
//!   (there is no A-owned resource to aim at): the harness creates as B and as A and checks
//!   each resource landed in its creator's workspace and never in the other's.
//!
//! A test-only probe route (a workspace-scoped read) also exercises `WorkspaceContext` end to
//! end, including membership removal.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode};
use kernel::{AppError, TakeId, UserId, WorkspaceId};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::app::tests::{
    test_clock, test_identity, test_ingest, test_rate_limiter, test_store, test_tenancy,
};
use crate::app::{AppState, build_router_from};
use crate::csrf::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};
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

/// How many of the route's resources the given workspace holds.
type Count = fn(PgPool, WorkspaceId) -> BoxFuture<i64>;

enum Probe {
    /// The route reads or mutates an existing resource.
    Owned(Setup),
    /// The route creates a resource in the caller's workspace. `request` builds the call (the
    /// harness adds the session cookie); `count` checks where the result landed.
    Create {
        request: fn() -> Request<Body>,
        count: Count,
    },
}

/// The tenant-isolation table: one entry per `Access::Workspace` route.
fn tenant_table() -> Vec<(Method, &'static str, Probe)> {
    vec![
        (
            Method::GET,
            PROBE_PATH,
            Probe::Owned(|_pool, workspace_id| {
                Box::pin(async move {
                    get(&PROBE_PATH.replace("{workspace_id}", &workspace_id.to_string()))
                })
            }),
        ),
        (
            Method::POST,
            "/api/v1/recordings",
            Probe::Create {
                request: || {
                    post_json(
                        "/api/v1/recordings",
                        r#"{"mime_type":"video/webm;codecs=vp9,opus","has_system_audio":false,"has_mic":true,"has_camera":false}"#,
                    )
                },
                count: |pool, workspace_id| {
                    Box::pin(async move {
                        sqlx::query_scalar!(
                            r#"SELECT count(*) AS "n!" FROM recordings r
                               JOIN takes t ON t.recording_id = r.id
                               WHERE r.workspace_id = $1 AND t.workspace_id = $1"#,
                            workspace_id.into_uuid(),
                        )
                        .fetch_one(&pool)
                        .await
                        .expect("count recordings")
                    })
                },
            },
        ),
        (
            Method::POST,
            "/api/v1/takes/{take_id}/chunks/{idx}/url",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let take_id = take_owned_by(&pool, workspace_id).await;
                    post_json(&format!("/api/v1/takes/{take_id}/chunks/0/url?count=2"), "")
                })
            }),
        ),
        (
            Method::POST,
            "/api/v1/takes/{take_id}/chunks/{idx}/ack",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let take_id = take_owned_by(&pool, workspace_id).await;
                    put_chunk(&pool, take_id, 0, b"x").await;
                    post_json(
                        &format!("/api/v1/takes/{take_id}/chunks/0/ack"),
                        r#"{"size_bytes":1,"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}"#,
                    )
                })
            }),
        ),
    ]
}

/// Starts a recording in `workspace_id` as its owner, returning the take.
async fn take_owned_by(pool: &PgPool, workspace_id: WorkspaceId) -> TakeId {
    let owner = sqlx::query_scalar!(
        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role = 'owner'",
        workspace_id.into_uuid(),
    )
    .fetch_one(pool)
    .await
    .expect("workspace owner");
    test_ingest(pool.clone())
        .start_recording(ingest::StartRecording {
            workspace_id,
            owner_id: UserId::from_uuid(owner),
            title: None,
            mime_type: "video/webm;codecs=vp9,opus".to_string(),
            sources: ingest::Sources {
                system_audio: false,
                mic: true,
                camera: false,
            },
        })
        .await
        .expect("start recording")
        .take_id
}

/// PUTs `bytes` as chunk `idx` of the take, into real MinIO, as the browser would.
async fn put_chunk(pool: &PgPool, take_id: TakeId, idx: u32, bytes: &'static [u8]) {
    let take = sqlx::query!(
        "SELECT workspace_id, recording_id, mime_type FROM takes WHERE id = $1",
        take_id.into_uuid()
    )
    .fetch_one(pool)
    .await
    .expect("take");
    let key = ingest::chunk_key(
        WorkspaceId::from_uuid(take.workspace_id),
        kernel::RecordingId::from_uuid(take.recording_id),
        take_id,
        idx,
        &take.mime_type,
    );
    let url = test_store()
        .presign_put(&key, std::time::Duration::from_secs(60))
        .await
        .expect("presign");
    let put = reqwest::Client::new()
        .put(url)
        .body(bytes)
        .send()
        .await
        .expect("PUT reaches MinIO");
    assert!(put.status().is_success(), "PUT got {}", put.status());
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
            ingest: test_ingest(pool.clone()),
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

/// A JSON POST carrying the CSRF double-submit pair, as the SPA sends it.
fn post_json(uri: &str, body: &'static str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(
            axum::http::header::COOKIE,
            format!("{CSRF_COOKIE_NAME}=harness"),
        )
        .header(CSRF_HEADER_NAME, "harness")
        .body(Body::from(body))
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
        // Keep any cookie the request already carries (e.g. the CSRF cookie).
        let header = match request.headers().get(axum::http::header::COOKIE) {
            Some(existing) => format!(
                "{}; {cookie}",
                existing.to_str().expect("ASCII cookie header")
            ),
            None => cookie.to_string(),
        };
        request.headers_mut().insert(
            axum::http::header::COOKIE,
            header.parse().expect("valid cookie header"),
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
        let (_, _, probe) = table
            .iter()
            .find(|(method, path, _)| *method == route.method && *path == route.path)
            .unwrap_or_else(|| {
                panic!(
                    "{} {} is a Workspace route but has no entry in the tenant-isolation table \
                     (bin/api/src/tenant_isolation.rs::tenant_table)",
                    route.method, route.path
                )
            });

        match probe {
            Probe::Owned(setup) => {
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
                    "{} {}: owner of workspace A got {as_alice}; the cross-tenant 404 proves \
                     nothing if the route 404s for everyone",
                    route.method,
                    route.path
                );
            }
            Probe::Create { request, count } => {
                let alice_before = count(pool.clone(), alice.workspace_id).await;
                let bob_before = count(pool.clone(), bob.workspace_id).await;

                let as_bob = send(&app, request(), Some(&bob.cookie)).await;
                assert!(
                    as_bob.is_success(),
                    "{} {}: member of workspace B got {as_bob}",
                    route.method,
                    route.path
                );
                assert_eq!(
                    count(pool.clone(), bob.workspace_id).await,
                    bob_before + 1,
                    "{} {}: B's create did not land in B's workspace",
                    route.method,
                    route.path
                );
                assert_eq!(
                    count(pool.clone(), alice.workspace_id).await,
                    alice_before,
                    "{} {}: B's create reached workspace A",
                    route.method,
                    route.path
                );

                let as_alice = send(&app, request(), Some(&alice.cookie)).await;
                assert!(
                    as_alice.is_success(),
                    "{} {}: owner of workspace A got {as_alice}",
                    route.method,
                    route.path
                );
                assert_eq!(
                    count(pool.clone(), alice.workspace_id).await,
                    alice_before + 1,
                    "{} {}: A's create did not land in A's workspace",
                    route.method,
                    route.path
                );
                assert_eq!(
                    count(pool.clone(), bob.workspace_id).await,
                    bob_before + 1,
                    "{} {}: A's create reached workspace B",
                    route.method,
                    route.path
                );
            }
        }
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
