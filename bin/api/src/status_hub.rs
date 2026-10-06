//! The SSE hub (docs/design.md §5: "listens on Postgres `NOTIFY` and pushes processing status to
//! connected clients"). One task per API process listens on the outbox channel and turns the
//! events that change what a recording looks like (`RecordingReady`, `ProcessingFailed`,
//! `RenditionReady`) into a [`Change`]; every open status stream wakes on the ones for its
//! recording and re-reads the recording's status. A `NOTIFY` carries only the event type, so the
//! hub reads the new `outbox_events` rows itself, by id, and also on a timer: a notification
//! lost to a reconnect costs at most [`CATCH_UP`] of delay, never a missed change.

use std::sync::Arc;
use std::time::Duration;

use kernel::{RecordingId, WorkspaceId};
use sqlx::PgPool;
use tokio::sync::broadcast;

/// A recording's status may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub workspace_id: WorkspaceId,
    pub recording_id: RecordingId,
}

/// Event types that change a recording's status.
const STATUS_EVENTS: [&str; 3] = ["RecordingReady", "ProcessingFailed", "RenditionReady"];
/// Re-read the outbox at least this often, notified or not.
const CATCH_UP: Duration = Duration::from_secs(5);
const RECONNECT_AFTER: Duration = Duration::from_secs(2);
/// A stream that falls this far behind skips ahead (it re-reads the status anyway).
const BACKLOG: usize = 256;

pub struct StatusHub {
    changes: broadcast::Sender<Change>,
    /// What feeds the hub, started by the first subscriber: an API process with no status stream
    /// open holds no extra database connection.
    source: Option<(PgPool, std::sync::Once)>,
}

impl StatusHub {
    /// A hub with nothing feeding it.
    #[cfg(test)]
    pub fn detached() -> Arc<Self> {
        Arc::new(Self {
            changes: broadcast::channel(BACKLOG).0,
            source: None,
        })
    }

    /// A hub fed from the outbox of `pool`'s database, once something subscribes.
    pub fn new(pool: PgPool) -> Arc<Self> {
        Arc::new(Self {
            changes: broadcast::channel(BACKLOG).0,
            source: Some((pool, std::sync::Once::new())),
        })
    }

    /// Subscribe *before* reading a recording's status, so a change in between is not lost.
    /// Must be called inside the Tokio runtime.
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        let receiver = self.changes.subscribe();
        if let Some((pool, once)) = &self.source {
            once.call_once(|| {
                tokio::spawn(run(pool.clone(), self.changes.clone()));
            });
        }
        receiver
    }
}

async fn run(pool: PgPool, changes: broadcast::Sender<Change>) {
    let mut last =
        match sqlx::query_scalar!(r#"SELECT COALESCE(MAX(id), 0) AS "id!" FROM outbox_events"#)
            .fetch_one(&pool)
            .await
        {
            Ok(id) => id,
            Err(error) => {
                tracing::error!(%error, "status hub: cannot read the outbox; live status is off");
                return;
            }
        };
    loop {
        let mut listener = match platform::listen(&pool, platform::NOTIFY_CHANNEL).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "status hub: cannot listen; retrying");
                tokio::time::sleep(RECONNECT_AFTER).await;
                continue;
            }
        };
        loop {
            if let Err(error) = publish_new(&pool, &changes, &mut last).await {
                tracing::warn!(%error, "status hub: cannot read the outbox");
            }
            match tokio::time::timeout(CATCH_UP, listener.recv()).await {
                Ok(Ok(_)) | Err(_) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "status hub: listener lost; reconnecting");
                    break;
                }
            }
        }
        tokio::time::sleep(RECONNECT_AFTER).await;
    }
}

/// Publishes the status-changing events after `last`, and moves `last` past everything read.
async fn publish_new(
    pool: &PgPool,
    changes: &broadcast::Sender<Change>,
    last: &mut i64,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, event_type, aggregate_id, payload->>'workspace_id' AS workspace_id
        FROM outbox_events WHERE id > $1 ORDER BY id
        "#,
        *last,
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        *last = row.id;
        if !STATUS_EVENTS.contains(&row.event_type.as_str()) {
            continue;
        }
        let Some(workspace_id) = row
            .workspace_id
            .and_then(|text| text.parse::<uuid::Uuid>().ok())
        else {
            continue;
        };
        // Nobody listening is fine.
        let _ = changes.send(Change {
            workspace_id: WorkspaceId::from_uuid(workspace_id),
            recording_id: RecordingId::from_uuid(row.aggregate_id),
        });
    }
    Ok(())
}
