//! Live status (docs/design.md §9 `GET /recordings/{id}/events`): a Server-Sent Events stream of
//! a recording's status. It sends the current status at once, then again whenever the status hub
//! hears the recording changed (ready, failed, a rendition finished). Nothing is polled.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, KeepAliveStream, Sse};
use kernel::{AppError, RecordingId, WorkspaceId};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::error::{ApiError, Problem};
use crate::status_hub::Change;
use crate::workspace_context::WorkspaceContext;

/// A stream is closed after this long; the browser's `EventSource` reconnects on its own, which
/// re-checks the session and the link instead of trusting them for a whole day.
const MAX_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// What a recording looks like right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RecordingStatus {
    /// `uploading`, `processing`, `ready` or `failed`.
    pub state: String,
    /// The adaptive (HLS) ladder exists.
    pub hls: bool,
    /// The scrub sprite and its `sprite.vtt` exist.
    pub sprite: bool,
    /// The animated preview exists.
    pub preview: bool,
}

type Frames = Sse<KeepAliveStream<ReceiverStream<Result<Event, Infallible>>>>;

/// The status of a recording, or `None` if it is gone (deleted, trashed, another workspace's).
async fn status_of(
    state: &AppState,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Result<Option<RecordingStatus>, sqlx::Error> {
    let mut conn = state.pool.acquire().await?;
    let Some(info) = state
        .catalog
        .watch_info(&mut conn, recording_id, workspace_id)
        .await?
    else {
        return Ok(None);
    };
    drop(conn);
    let renditions = media::RenditionReader::new(state.pool.clone())
        .available(workspace_id, recording_id)
        .await?;
    let has = |kind: &str, variant: &str| renditions.iter().any(|(k, v)| k == kind && v == variant);
    Ok(Some(RecordingStatus {
        state: info.state,
        hls: has("hls", "master"),
        sprite: has("sprite", "vtt"),
        preview: has("preview", "default"),
    }))
}

/// Opens the stream. The caller has already checked the viewer may see the recording; `404`s
/// for everything else are theirs to produce.
pub(crate) fn stream(
    state: AppState,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
) -> Frames {
    // Subscribe before the first read, so a change between the two is not lost.
    let changes = state.status_hub.subscribe();
    let (frames, receiver) = mpsc::channel(8);
    tokio::spawn(pump(state, workspace_id, recording_id, changes, frames));
    Sse::new(ReceiverStream::new(receiver)).keep_alive(KeepAlive::default())
}

async fn pump(
    state: AppState,
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    mut changes: broadcast::Receiver<Change>,
    frames: mpsc::Sender<Result<Event, Infallible>>,
) {
    let deadline = tokio::time::sleep(MAX_LIFETIME);
    tokio::pin!(deadline);
    let mut last: Option<RecordingStatus> = None;
    loop {
        match status_of(&state, workspace_id, recording_id).await {
            Ok(Some(status)) => {
                if last.as_ref() != Some(&status) {
                    let Ok(event) = Event::default().event("status").json_data(&status) else {
                        return;
                    };
                    if frames.send(Ok(event)).await.is_err() {
                        return;
                    }
                    last = Some(status);
                }
            }
            Ok(None) => {
                let _ = frames
                    .send(Ok(Event::default().event("gone").data("")))
                    .await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "status stream: cannot read the status");
                return;
            }
        }
        // Wait for a change to this recording.
        loop {
            tokio::select! {
                change = changes.recv() => match change {
                    Ok(change) if change == (Change { workspace_id, recording_id }) => break,
                    Ok(_) => {}
                    // Missed some: the status read is the truth anyway.
                    Err(broadcast::error::RecvError::Lagged(_)) => break,
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                () = &mut deadline => return,
                () = frames.closed() => return,
            }
        }
    }
}

/// Live status of one of the caller's recordings.
#[utoipa::path(
    get,
    path = "/api/v1/recordings/{recording_id}/events",
    tag = "recordings",
    params(("recording_id" = uuid::Uuid, Path, description = "The recording")),
    responses(
        (status = 200, description = "A `text/event-stream` of `status` events, each a RecordingStatus; a `gone` event ends it when the recording disappears", body = RecordingStatus, content_type = "text/event-stream"),
        (status = 401, description = "No valid session", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "No such recording in the caller's workspace", body = Problem, content_type = "application/problem+json"),
    )
)]
#[tracing::instrument(skip_all, fields(workspace_id = %ctx.workspace_id))]
pub async fn recording_events(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path(recording_id): Path<RecordingId>,
) -> Result<Frames, ApiError> {
    let known = status_of(&state, ctx.workspace_id, recording_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "recording events: lookup failed");
            ApiError::from(AppError::Internal("status unavailable".to_string()))
        })?;
    if known.is_none() {
        return Err(AppError::NotFound.into());
    }
    Ok(stream(state, ctx.workspace_id, recording_id))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::{Method, StatusCode};
    use sqlx::PgPool;

    use crate::routes::testkit::{call, caller, link, next_frame, open_stream, recording};

    const WAIT: Duration = Duration::from_secs(10);

    /// What the worker does when a recording becomes ready or gains a rendition: change the
    /// rows, then write the event (and notify) in the same transaction.
    async fn changed(
        pool: &PgPool,
        workspace: kernel::WorkspaceId,
        recording: kernel::RecordingId,
        event_type: &str,
    ) {
        let mut tx = pool.begin().await.expect("tx");
        sqlx::query!(
            "INSERT INTO outbox_events (event_type, aggregate_id, payload) VALUES ($1, $2, $3)",
            event_type,
            recording.into_uuid(),
            serde_json::json!({ "workspace_id": workspace.to_string() }),
        )
        .execute(&mut *tx)
        .await
        .expect("event");
        sqlx::query!(
            "SELECT pg_notify($1, $2)",
            platform::NOTIFY_CHANNEL,
            event_type
        )
        .execute(&mut *tx)
        .await
        .expect("notify");
        tx.commit().await.expect("commit");
    }

    /// Day 82's Check: a stream open on a processing recording reports it ready, and then each
    /// rendition, without being asked again.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_recording_stream_pushes_each_change(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let uri = format!("/api/v1/recordings/{id}/events");
        let (status, mut stream) = open_stream(&pool, Some(&owner), &uri).await;
        assert_eq!(status, StatusCode::OK);

        let first = next_frame(&mut stream, WAIT).await.expect("first frame");
        assert!(first.contains("event: status"), "{first}");
        assert!(first.contains(r#""state":"processing""#), "{first}");
        assert!(first.contains(r#""sprite":false"#), "{first}");

        // The worker finishes: state and event commit together.
        sqlx::query!(
            "UPDATE recordings SET state = 'ready' WHERE id = $1",
            id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("ready");
        changed(&pool, owner.workspace_id, id, "RecordingReady").await;
        let ready = next_frame(&mut stream, WAIT).await.expect("ready frame");
        assert!(ready.contains(r#""state":"ready""#), "{ready}");

        // A later rendition arrives.
        let take = sqlx::query_scalar!(
            "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                                has_mic, has_camera, finalized_at)
             VALUES ($1, $2, $3, 'video/webm', false, false, false, now()) RETURNING id",
            uuid::Uuid::now_v7(),
            owner.workspace_id.into_uuid(),
            id.into_uuid(),
        )
        .fetch_one(&pool)
        .await
        .expect("take");
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
             VALUES ($1, $2, $3, $4, 'sprite', 'vtt', 'img/sprite.vtt')",
            uuid::Uuid::now_v7(),
            owner.workspace_id.into_uuid(),
            id.into_uuid(),
            take,
        )
        .execute(&pool)
        .await
        .expect("rendition");
        changed(&pool, owner.workspace_id, id, "RenditionReady").await;
        let sprite = next_frame(&mut stream, WAIT).await.expect("sprite frame");
        assert!(sprite.contains(r#""sprite":true"#), "{sprite}");
        assert!(sprite.contains(r#""state":"ready""#), "{sprite}");

        // Events about other recordings, or of other kinds, send nothing.
        let other = recording(&pool, &owner, "processing").await;
        changed(&pool, owner.workspace_id, other, "RecordingReady").await;
        changed(&pool, owner.workspace_id, id, "TakeFinalized").await;
        assert_eq!(
            next_frame(&mut stream, Duration::from_millis(1500)).await,
            None
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_stream_is_for_the_owner_workspace_only(pool: PgPool) {
        let alice = caller(&pool).await;
        let bob = caller(&pool).await;
        let id = recording(&pool, &alice, "processing").await;
        let uri = format!("/api/v1/recordings/{id}/events");
        assert_eq!(
            open_stream(&pool, Some(&bob), &uri).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            open_stream(&pool, None, &uri).await.0,
            StatusCode::UNAUTHORIZED
        );
        let missing = format!("/api/v1/recordings/{}/events", uuid::Uuid::now_v7());
        assert_eq!(
            open_stream(&pool, Some(&alice), &missing).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_viewer_follows_a_link_and_a_revoked_one_is_gone(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let (slug, link_id) = link(&pool, &owner, id, "link").await;
        let uri = format!("/api/v1/s/{slug}/events");
        let (status, mut stream) = open_stream(&pool, None, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let first = next_frame(&mut stream, WAIT).await.expect("frame");
        assert!(first.contains(r#""state":"processing""#), "{first}");

        // Private again: strangers are refused like any missing link.
        let reply = call(
            &pool,
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/recordings/{id}/links/{link_id}"),
            Some(serde_json::json!({"visibility": "private"})),
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert_eq!(
            open_stream(&pool, None, &uri).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_trashed_recording_ends_its_stream(pool: PgPool) {
        let owner = caller(&pool).await;
        let id = recording(&pool, &owner, "processing").await;
        let uri = format!("/api/v1/recordings/{id}/events");
        let (_, mut stream) = open_stream(&pool, Some(&owner), &uri).await;
        next_frame(&mut stream, WAIT).await.expect("first");
        sqlx::query!(
            "UPDATE recordings SET trashed_at = now() WHERE id = $1",
            id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("trash");
        changed(&pool, owner.workspace_id, id, "RenditionReady").await;
        let gone = next_frame(&mut stream, WAIT).await.expect("gone");
        assert!(gone.contains("event: gone"), "{gone}");
        assert_eq!(next_frame(&mut stream, WAIT).await, None, "the stream ends");
    }
}
