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
use std::sync::Arc;

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
    /// The route lists the caller's workspace. `seed` puts a recording with a recognisable
    /// title in the given workspace and returns it; the harness checks the owner's listing has
    /// it and another workspace's listing does not.
    Listing {
        request: fn() -> Request<Body>,
        seed: fn(PgPool, WorkspaceId) -> BoxFuture<String>,
    },
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
            "/api/v1/recordings/{recording_id}/retry",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = failed_recording_in(&pool, workspace_id).await;
                    post_json(&format!("/api/v1/recordings/{recording_id}/retry"), "")
                })
            }),
        ),
        (
            Method::GET,
            "/api/v1/recordings/{recording_id}/events",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    get(&format!("/api/v1/recordings/{recording_id}/events"))
                })
            }),
        ),
        (
            Method::PATCH,
            "/api/v1/recordings/{recording_id}",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    json_request(
                        Method::PATCH,
                        &format!("/api/v1/recordings/{recording_id}"),
                        r#"{"title":"Renamed"}"#,
                    )
                })
            }),
        ),
        (
            Method::DELETE,
            "/api/v1/recordings/{recording_id}",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    json_request(
                        Method::DELETE,
                        &format!("/api/v1/recordings/{recording_id}"),
                        "",
                    )
                })
            }),
        ),
        (
            Method::GET,
            "/api/v1/recordings/{recording_id}/chapters",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    get(&format!("/api/v1/recordings/{recording_id}/chapters"))
                })
            }),
        ),
        (
            Method::PUT,
            "/api/v1/recordings/{recording_id}/chapters",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    json_request(
                        Method::PUT,
                        &format!("/api/v1/recordings/{recording_id}/chapters"),
                        r#"{"chapters":[{"start_ms":0,"title":"Intro"}]}"#,
                    )
                })
            }),
        ),
        (
            Method::GET,
            "/api/v1/recordings",
            Probe::Listing {
                request: || get("/api/v1/recordings"),
                seed: |pool, workspace_id| {
                    Box::pin(async move {
                        let id = ready_recording_in(&pool, workspace_id).await;
                        id.to_string()
                    })
                },
            },
        ),
        (
            Method::GET,
            "/api/v1/recordings/{recording_id}/download",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = downloadable_recording_in(&pool, workspace_id).await;
                    get(&format!("/api/v1/recordings/{recording_id}/download"))
                })
            }),
        ),
        (
            Method::POST,
            "/api/v1/recordings/{recording_id}/links",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let recording_id = ready_recording_in(&pool, workspace_id).await;
                    post_json(&format!("/api/v1/recordings/{recording_id}/links"), "{}")
                })
            }),
        ),
        (
            Method::GET,
            "/api/v1/recordings/{recording_id}/links",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let (recording_id, _) = link_in(&pool, workspace_id).await;
                    get(&format!("/api/v1/recordings/{recording_id}/links"))
                })
            }),
        ),
        (
            Method::PATCH,
            "/api/v1/recordings/{recording_id}/links/{link_id}",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let (recording_id, link_id) = link_in(&pool, workspace_id).await;
                    json_request(
                        Method::PATCH,
                        &format!("/api/v1/recordings/{recording_id}/links/{link_id}"),
                        r#"{"allow_download":true}"#,
                    )
                })
            }),
        ),
        (
            Method::DELETE,
            "/api/v1/recordings/{recording_id}/links/{link_id}",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let (recording_id, link_id) = link_in(&pool, workspace_id).await;
                    json_request(
                        Method::DELETE,
                        &format!("/api/v1/recordings/{recording_id}/links/{link_id}"),
                        "",
                    )
                })
            }),
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
        (
            Method::GET,
            "/api/v1/takes/{take_id}/status",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let take_id = take_owned_by(&pool, workspace_id).await;
                    get(&format!("/api/v1/takes/{take_id}/status"))
                })
            }),
        ),
        (
            Method::POST,
            "/api/v1/takes/{take_id}/finalize",
            Probe::Owned(|pool, workspace_id| {
                Box::pin(async move {
                    let take_id = take_owned_by(&pool, workspace_id).await;
                    put_chunk(&pool, take_id, 0, b"x").await;
                    ack_as_owner(&pool, workspace_id, take_id, 0, 1).await;
                    post_json(
                        &format!("/api/v1/takes/{take_id}/finalize"),
                        r#"{"chunk_count":1,"duration_ms":2000}"#,
                    )
                })
            }),
        ),
    ]
}

async fn workspace_owner(pool: &PgPool, workspace_id: WorkspaceId) -> UserId {
    UserId::from_uuid(
        sqlx::query_scalar!(
            "SELECT user_id FROM memberships WHERE workspace_id = $1 AND role = 'owner'",
            workspace_id.into_uuid(),
        )
        .fetch_one(pool)
        .await
        .expect("workspace owner"),
    )
}

/// Acks chunk `idx` (already PUT) through the service, as the owner.
async fn ack_as_owner(
    pool: &PgPool,
    workspace_id: WorkspaceId,
    take_id: TakeId,
    idx: u32,
    size: u64,
) {
    test_ingest(pool.clone())
        .ack_chunk(ingest::AckChunk {
            workspace_id,
            user_id: workspace_owner(pool, workspace_id).await,
            take_id,
            idx,
            size_bytes: size,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
        })
        .await
        .expect("ack");
}

/// Starts a recording in `workspace_id` as its owner, returning the take.
async fn take_owned_by(pool: &PgPool, workspace_id: WorkspaceId) -> TakeId {
    let owner = workspace_owner(pool, workspace_id).await;
    test_ingest(pool.clone())
        .start_recording(ingest::StartRecording {
            workspace_id,
            owner_id: owner,
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

/// A `ready` recording in `workspace_id`, owned by its owner.
async fn ready_recording_in(pool: &PgPool, workspace_id: WorkspaceId) -> kernel::RecordingId {
    let owner = workspace_owner(pool, workspace_id).await;
    let recording_id = kernel::RecordingId::new_v7();
    sqlx::query!(
        "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
         VALUES ($1, $2, $3, 'Ready', 'ready')",
        recording_id.into_uuid(),
        workspace_id.into_uuid(),
        owner.into_uuid(),
    )
    .execute(pool)
    .await
    .expect("recording");
    recording_id
}

/// A `ready` recording in `workspace_id` with an MP4 rendition.
async fn downloadable_recording_in(
    pool: &PgPool,
    workspace_id: WorkspaceId,
) -> kernel::RecordingId {
    let recording_id = ready_recording_in(pool, workspace_id).await;
    let take_id = uuid::Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                            has_mic, has_camera, finalized_at)
         VALUES ($1, $2, $3, 'video/webm', false, true, false, now())",
        take_id,
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
    )
    .execute(pool)
    .await
    .expect("take");
    sqlx::query!(
        "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
         VALUES ($1, $2, $3, $4, 'mp4', 'default', 'ws/rec/default.mp4')",
        uuid::Uuid::now_v7(),
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        take_id,
    )
    .execute(pool)
    .await
    .expect("rendition");
    recording_id
}

/// A recording in `workspace_id` with one live share link.
async fn link_in(
    pool: &PgPool,
    workspace_id: WorkspaceId,
) -> (kernel::RecordingId, kernel::ShareLinkId) {
    let recording_id = ready_recording_in(pool, workspace_id).await;
    let link_id = kernel::ShareLinkId::new_v7();
    sqlx::query!(
        "INSERT INTO share_links (id, workspace_id, recording_id, slug)
         VALUES ($1, $2, $3, $4)",
        link_id.into_uuid(),
        workspace_id.into_uuid(),
        recording_id.into_uuid(),
        format!(
            "{:0>12}",
            link_id.into_uuid().simple().to_string().split_at(12).0
        ),
    )
    .execute(pool)
    .await
    .expect("link");
    (recording_id, link_id)
}

/// A recording in `workspace_id` that processing gave up on: `failed`, with the take's job.
async fn failed_recording_in(pool: &PgPool, workspace_id: WorkspaceId) -> kernel::RecordingId {
    let take_id = take_owned_by(pool, workspace_id).await;
    let take = sqlx::query!(
        "SELECT recording_id, mime_type FROM takes WHERE id = $1",
        take_id.into_uuid()
    )
    .fetch_one(pool)
    .await
    .expect("take");
    sqlx::query!(
        "UPDATE recordings SET state = 'failed' WHERE id = $1",
        take.recording_id
    )
    .execute(pool)
    .await
    .expect("fail the recording");
    sqlx::query!(
        r#"INSERT INTO media_jobs (take_id, workspace_id, recording_id, mime_type, duration_ms,
                                   chunks, state)
           VALUES ($1, $2, $3, $4, 1000, '[]'::jsonb, 'failed')"#,
        take_id.into_uuid(),
        workspace_id.into_uuid(),
        take.recording_id,
        take.mime_type,
    )
    .execute(pool)
    .await
    .expect("media job");
    kernel::RecordingId::from_uuid(take.recording_id)
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
            billing: Arc::new(billing::BillingService::new()),
            pool: pool.clone(),
            identity: test_identity(pool.clone()),
            tenancy: test_tenancy(pool.clone()),
            ingest: test_ingest(pool.clone()),
            retry: Arc::new(media::RetryService::new(
                pool.clone(),
                Arc::new(catalog::CatalogService::new()),
            )),
            catalog: Arc::new(catalog::CatalogService::new()),
            recordings: Arc::new(catalog::RecordingManager::new(
                pool.clone(),
                test_store(),
                test_clock(),
            )),
            delivery: Arc::new(delivery::DeliveryService::new(
                test_store(),
                Arc::new(media::RenditionReader::new(pool.clone())),
                test_clock(),
                b"test-manifest-key-test-manifest!".to_vec(),
            )),
            sharing: Arc::new(sharing::SharingService::new(
                pool.clone(),
                Arc::new(catalog::CatalogService::new()),
                test_clock(),
            )),
            status_hub: crate::status_hub::StatusHub::detached(),
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
    json_request(Method::POST, uri, body)
}

fn json_request(method: Method, uri: &str, body: &'static str) -> Request<Body> {
    Request::builder()
        .method(method)
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

async fn send(app: &Router, request: Request<Body>, cookie: Option<&str>) -> StatusCode {
    send_full(app, request, cookie).await.0
}

/// Like `send`, with the response body as text.
async fn send_full(
    app: &Router,
    mut request: Request<Body>,
    cookie: Option<&str>,
) -> (StatusCode, String) {
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
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("router call succeeds");
    let status = response.status();
    // A live stream (SSE) never ends: its status and headers are the answer; dropping the
    // response closes it.
    let is_stream = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"text/event-stream"));
    if is_stream {
        return (status, String::new());
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
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
            Access::Viewer => {
                // An unknown slug is `404`; the path being mounted shows as `405` for a method
                // it doesn't take (an unmounted path is `404` for every method).
                assert_eq!(
                    status,
                    StatusCode::NOT_FOUND,
                    "{} {}",
                    route.method,
                    route.path
                );
                let wrong_method = Request::builder()
                    .method(Method::DELETE)
                    .uri(concrete_path(route.path))
                    .body(Body::empty())
                    .expect("valid request");
                assert_eq!(
                    send(&app, wrong_method, None).await,
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{} {} is in the route table but not mounted",
                    route.method,
                    route.path
                );
            }
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
                let request = setup(pool.clone(), alice.workspace_id).await;
                let before = snapshot(&pool, alice.workspace_id).await;
                let as_bob = send(&app, request, Some(&bob.cookie)).await;
                assert_eq!(
                    as_bob,
                    StatusCode::NOT_FOUND,
                    "{} {}: workspace B reached workspace A's resource",
                    route.method,
                    route.path
                );
                assert_eq!(
                    snapshot(&pool, alice.workspace_id).await,
                    before,
                    "{} {}: workspace B's refused request still changed workspace A's data",
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
            Probe::Listing { request, seed } => {
                let marker = seed(pool.clone(), alice.workspace_id).await;
                let (as_bob, bob_body) = send_full(&app, request(), Some(&bob.cookie)).await;
                assert!(
                    as_bob.is_success(),
                    "{} {}: member of workspace B got {as_bob}",
                    route.method,
                    route.path
                );
                assert!(
                    !bob_body.contains(&marker),
                    "{} {}: workspace B's listing shows workspace A's recording",
                    route.method,
                    route.path
                );
                let (as_alice, alice_body) = send_full(&app, request(), Some(&alice.cookie)).await;
                assert!(
                    as_alice.is_success(),
                    "{} {}: {as_alice}",
                    route.method,
                    route.path
                );
                assert!(
                    alice_body.contains(&marker),
                    "{} {}: the owner's listing misses their own recording; the check above \
                     proves nothing without it",
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

/// Every `Viewer` route, against a private link in workspace A: anonymous visitors and
/// members of workspace B get `404`; A's owner gets through.
#[sqlx::test(migrations = "../../migrations")]
async fn viewer_routes_hide_private_links_from_other_tenants(pool: PgPool) {
    let app = app(&pool);
    let alice = tenant(&pool, "a").await;
    let bob = tenant(&pool, "b").await;
    let (recording_id, link_id) = link_in(&pool, alice.workspace_id).await;
    // The playlist routes need a ladder to let even the owner through.
    crate::routes::testkit::ladder(&pool, alice.workspace_id, recording_id).await;
    crate::routes::testkit::sprite(&pool, alice.workspace_id, recording_id).await;
    let slug = sqlx::query_scalar!(
        "UPDATE share_links SET visibility = 'private' WHERE id = $1 RETURNING slug",
        link_id.into_uuid()
    )
    .fetch_one(&pool)
    .await
    .expect("private link");

    let viewer_routes: Vec<Route> = table_with_probe()
        .into_iter()
        .filter(|route| route.access == Access::Viewer)
        .collect();
    assert!(!viewer_routes.is_empty());
    let before = snapshot(&pool, alice.workspace_id).await;
    for route in &viewer_routes {
        let request = || {
            Request::builder()
                .method(route.method.clone())
                .uri(
                    route
                        .path
                        .replace("{slug}", &slug)
                        .replace("{rung}", "720p"),
                )
                .body(Body::empty())
                .expect("valid request")
        };
        for (who, cookie) in [
            ("anonymous", None),
            ("workspace B", Some(bob.cookie.as_str())),
        ] {
            assert_eq!(
                send(&app, request(), cookie).await,
                StatusCode::NOT_FOUND,
                "{} {}: {who} reached a private link of workspace A",
                route.method,
                route.path
            );
        }
        assert_eq!(
            snapshot(&pool, alice.workspace_id).await,
            before,
            "{} {}: refused viewers changed workspace A's data",
            route.method,
            route.path
        );
        let as_owner = send(&app, request(), Some(&alice.cookie)).await;
        assert!(
            as_owner != StatusCode::NOT_FOUND && as_owner != StatusCode::UNAUTHORIZED,
            "{} {}: the owner got {as_owner}; the 404s above prove nothing otherwise",
            route.method,
            route.path
        );
    }
}

/// Every row a workspace owns, as one string, so a test can tell whether anything changed.
async fn snapshot(pool: &PgPool, workspace_id: WorkspaceId) -> String {
    let ws = workspace_id.into_uuid();
    let mut out = String::new();
    macro_rules! table {
        ($name:literal, $query:expr) => {{
            let rows: Vec<serde_json::Value> = sqlx::query_scalar($query)
                .bind(ws)
                .fetch_all(pool)
                .await
                .expect(concat!("snapshot ", $name));
            out.push_str(&format!("{}: {}\n", $name, serde_json::Value::Array(rows)));
        }};
    }
    table!(
        "recordings",
        "SELECT to_jsonb(t) FROM recordings t WHERE workspace_id = $1 ORDER BY id"
    );
    table!(
        "takes",
        "SELECT to_jsonb(t) FROM takes t WHERE workspace_id = $1 ORDER BY id"
    );
    table!(
        "chunks",
        "SELECT to_jsonb(t) FROM chunks t WHERE workspace_id = $1 ORDER BY take_id, idx"
    );
    table!(
        "share_links",
        "SELECT to_jsonb(t) FROM share_links t WHERE workspace_id = $1 ORDER BY id"
    );
    table!(
        "renditions",
        "SELECT to_jsonb(t) FROM renditions t WHERE workspace_id = $1 ORDER BY id"
    );
    table!(
        "media_jobs",
        "SELECT to_jsonb(t) FROM media_jobs t WHERE workspace_id = $1 ORDER BY take_id"
    );
    table!(
        "memberships",
        "SELECT to_jsonb(t) FROM memberships t WHERE workspace_id = $1 ORDER BY user_id"
    );
    table!(
        "outbox_events",
        "SELECT payload FROM outbox_events WHERE payload->>'workspace_id' = $1::uuid::text ORDER BY id"
    );
    out
}

/// The source of the function a route is mounted with: its signature (`fn name(…) -> …`) and
/// body, found by name in the handler's module file.
fn handler_source(route: &Route) -> (String, String) {
    let segments: Vec<&str> = route.handler_name.split("::").collect();
    let name = segments.last().copied().expect("a handler name");
    let module = segments[segments.len() - 2];
    let file = if module == "app" {
        "src/app.rs".to_string()
    } else {
        format!("src/routes/{module}.rs")
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let start = source
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("{} is not defined in {path:?}", route.handler_name));
    let rest = &source[start..];
    let signature_end = rest.find("{\n").unwrap_or(rest.len());
    let body_end = rest.find("\n}\n").unwrap_or(rest.len());
    (
        rest[..signature_end].to_string(),
        rest[signature_end..body_end].to_string(),
    )
}

/// A route is classified by what its handler actually takes, so a handler that reads workspace
/// data can't be filed under `Public` or `Session` and escape the isolation tests:
/// `WorkspaceContext` → `Workspace`, `MaybeSession` → `Viewer`, `SessionClaims` → `Session`.
#[test]
fn every_route_is_classified_by_what_its_handler_takes() {
    for route in routes::table() {
        let (signature, _) = handler_source(&route);
        let taken = if signature.contains("WorkspaceContext") {
            Access::Workspace
        } else if signature.contains("MaybeSession") {
            Access::Viewer
        } else if signature.contains("SessionClaims") {
            Access::Session
        } else {
            Access::Public
        };
        assert_eq!(
            route.access, taken,
            "{} {} ({}) is classified {:?} but its handler takes the extractors of {:?}",
            route.method, route.path, route.handler_name, route.access, taken
        );
    }
}

/// Routes that change state behind a session must check the CSRF token. These are the
/// exceptions, each with the reason.
const NO_CSRF: &[(&str, &str)] = &[
    (
        "/api/v1/auth/refresh",
        "keyed by the refresh cookie, SameSite=Lax, no body",
    ),
    (
        "/api/v1/auth/logout",
        "keyed by the refresh cookie; idempotent",
    ),
];

#[test]
fn every_state_changing_session_route_checks_csrf() {
    for route in routes::table() {
        let mutating = matches!(
            route.method,
            Method::POST | Method::PATCH | Method::PUT | Method::DELETE
        );
        if !mutating || route.access == Access::Public {
            continue;
        }
        if NO_CSRF.iter().any(|(path, _)| *path == route.path) {
            continue;
        }
        let (_, body) = handler_source(&route);
        assert!(
            body.contains("verify_csrf("),
            "{} {} ({}) changes state for a signed-in caller but never calls verify_csrf",
            route.method,
            route.path,
            route.handler_name
        );
    }
}
