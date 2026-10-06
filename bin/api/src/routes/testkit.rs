//! Shared helpers for route tests: real users with sessions, and calls through the full router.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use kernel::{RecordingId, UserId, WorkspaceId};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::app::build_router;
use crate::app::tests::{
    test_clock, test_identity, test_ingest, test_rate_limiter, test_store, test_tenancy,
};
use crate::csrf::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};
use crate::session::ACCESS_COOKIE_NAME;

pub struct Caller {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub cookie: String,
}

/// Registers and logs in a fresh user (with a personal workspace of their own).
pub async fn caller(pool: &PgPool) -> Caller {
    let identity = test_identity(pool.clone());
    let email = format!("testkit-{}@example.com", uuid::Uuid::now_v7());
    let password = "correct-horse-battery-staple-42".to_string();
    identity
        .register(identity::RegisterRequest {
            email: email.clone(),
            password: password.clone(),
            display_name: "Tester".to_string(),
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

/// A recording in the caller's workspace in `state`.
pub async fn recording(pool: &PgPool, caller: &Caller, state: &str) -> RecordingId {
    let id = RecordingId::new_v7();
    sqlx::query!(
        "INSERT INTO recordings (id, workspace_id, owner_id, title, state, duration_ms, width, height)
         VALUES ($1, $2, $3, 'Mine', $4::text::recording_state, 12000, 1280, 720)",
        id.into_uuid(),
        caller.workspace_id.into_uuid(),
        caller.user_id.into_uuid(),
        state,
    )
    .execute(pool)
    .await
    .expect("recording");
    id
}

/// An MP4 and poster rendition for a recording (the take row they hang off included).
pub async fn renditions(pool: &PgPool, caller: &Caller, recording: RecordingId) {
    let take = uuid::Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                            has_mic, has_camera, finalized_at)
         VALUES ($1, $2, $3, 'video/webm', false, true, false, now())",
        take,
        caller.workspace_id.into_uuid(),
        recording.into_uuid(),
    )
    .execute(pool)
    .await
    .expect("take");
    for (kind, variant, key) in [
        ("mp4", "default", "ws/rec/default.mp4"),
        ("thumbnail", "poster", "ws/rec/poster.jpg"),
    ] {
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
             VALUES ($1, $2, $3, $4, $5::text::rendition_kind, $6, $7)",
            uuid::Uuid::now_v7(),
            caller.workspace_id.into_uuid(),
            recording.into_uuid(),
            take,
            kind,
            variant,
            key,
        )
        .execute(pool)
        .await
        .expect("rendition");
    }
}

/// Only the original (`source.webm`) of a recording, as it is while the MP4 is being made.
pub async fn source_only(pool: &PgPool, caller: &Caller, recording: RecordingId) {
    let take = uuid::Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                            has_mic, has_camera, finalized_at)
         VALUES ($1, $2, $3, 'video/webm', false, true, false, now())",
        take,
        caller.workspace_id.into_uuid(),
        recording.into_uuid(),
    )
    .execute(pool)
    .await
    .expect("take");
    sqlx::query!(
        "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key, meta)
         VALUES ($1, $2, $3, $4, 'source', 'default', 'ws/rec/takes/t/source.webm',
                 '{\"content_type\": \"video/webm\"}')",
        uuid::Uuid::now_v7(),
        caller.workspace_id.into_uuid(),
        recording.into_uuid(),
        take,
    )
    .execute(pool)
    .await
    .expect("source");
}

pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
    pub cache_control: Option<String>,
    pub content_type: Option<String>,
    /// The body as text, for responses that aren't JSON (a playlist).
    pub text: String,
}

/// One request through the whole router. `caller: None` is an anonymous visitor.
pub async fn call(
    pool: &PgPool,
    caller: Option<&Caller>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Reply {
    let app = build_router(
        pool.clone(),
        test_identity(pool.clone()),
        test_tenancy(pool.clone()),
        test_ingest(pool.clone()),
        test_store(),
        test_rate_limiter(pool.clone()),
        test_clock(),
        "http://localhost:4200",
    );
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    request = match caller {
        Some(caller) => request
            .header(
                "cookie",
                format!("{}; {CSRF_COOKIE_NAME}=token", caller.cookie),
            )
            .header(CSRF_HEADER_NAME, "token"),
        None => request,
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
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cache_control,
        content_type,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// A built HLS ladder (a 360p and a 720p rung, one segment each) for a recording (given a take if
/// it has none): the playlists go to the real test bucket and the renditions point at them.
/// Returns the storage prefix.
pub async fn ladder(pool: &PgPool, workspace_id: WorkspaceId, recording: RecordingId) -> String {
    let store = crate::app::tests::test_store();
    let prefix = format!("test/{workspace_id}/rec/{recording}/hls");
    let dir = std::env::temp_dir().join(format!("ladder-{recording}"));
    std::fs::create_dir_all(&dir).expect("scratch");
    let put = async |name: &str, text: &str| {
        let file = dir.join("f");
        std::fs::write(&file, text).expect("file");
        store
            .put_file(
                &format!("{prefix}/{name}"),
                &file,
                "application/octet-stream",
            )
            .await
            .expect("put");
    };
    put(
        "master.m3u8",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=640x360\n360p/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2,RESOLUTION=1280x720\n720p/index.m3u8\n",
    )
    .await;
    for rung in ["360p", "720p"] {
        put(
            &format!("{rung}/index.m3u8"),
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\nseg_0000.m4s\n#EXT-X-ENDLIST\n",
        )
        .await;
    }
    let _ = std::fs::remove_dir_all(&dir);
    let existing = sqlx::query_scalar!(
        "SELECT id FROM takes WHERE recording_id = $1",
        recording.into_uuid()
    )
    .fetch_optional(pool)
    .await
    .expect("take lookup");
    let take = match existing {
        Some(take) => take,
        None => {
            let take = uuid::Uuid::now_v7();
            sqlx::query!(
                "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                                    has_mic, has_camera, finalized_at)
                 VALUES ($1, $2, $3, 'video/webm', false, true, false, now())",
                take,
                workspace_id.into_uuid(),
                recording.into_uuid(),
            )
            .execute(pool)
            .await
            .expect("take");
            take
        }
    };
    sqlx::query!(
        "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
         VALUES ($1, $2, $3, $4, 'hls', 'master', $5)",
        uuid::Uuid::now_v7(),
        workspace_id.into_uuid(),
        recording.into_uuid(),
        take,
        format!("{prefix}/master.m3u8"),
    )
    .execute(pool)
    .await
    .expect("hls rendition");
    prefix
}

/// Creates a link on `recording` as `owner` and returns its slug.
pub async fn link(
    pool: &PgPool,
    owner: &Caller,
    recording: RecordingId,
    visibility: &str,
) -> (String, String) {
    let reply = call(
        pool,
        Some(owner),
        Method::POST,
        &format!("/api/v1/recordings/{recording}/links"),
        Some(serde_json::json!({"visibility": visibility})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    (
        reply.body["slug"].as_str().expect("slug").to_string(),
        reply.body["id"].as_str().expect("id").to_string(),
    )
}

/// A streaming GET through the whole router (SSE): the status, and the body to read frames from.
pub async fn open_stream(
    pool: &PgPool,
    caller: Option<&Caller>,
    uri: &str,
) -> (StatusCode, axum::body::BodyDataStream) {
    let app = build_router(
        pool.clone(),
        test_identity(pool.clone()),
        test_tenancy(pool.clone()),
        test_ingest(pool.clone()),
        test_store(),
        test_rate_limiter(pool.clone()),
        test_clock(),
        "http://localhost:4200",
    );
    let mut request = Request::builder().method(Method::GET).uri(uri);
    if let Some(caller) = caller {
        request = request.header("cookie", caller.cookie.clone());
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("call");
    (response.status(), response.into_body().into_data_stream())
}

/// The next SSE frame (up to the blank line) from `stream`, or `None` if nothing came in
/// `within`. Keep-alive comments are skipped.
pub async fn next_frame(
    stream: &mut axum::body::BodyDataStream,
    within: std::time::Duration,
) -> Option<String> {
    use tokio_stream::StreamExt;
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if let Some(end) = text.find("\n\n") {
            let frame: String = text[..end].to_string();
            text = text[end + 2..].to_string();
            if !frame.starts_with(':') {
                return Some(frame);
            }
            continue;
        }
        let chunk = tokio::time::timeout_at(deadline, stream.next())
            .await
            .ok()??;
        text.push_str(&String::from_utf8_lossy(&chunk.expect("chunk")));
    }
}
