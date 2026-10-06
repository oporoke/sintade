//! Domain events published by `media` (docs/design.md §9 event table).

use kernel::{DomainEvent, RecordingId, TakeId, UserId, WorkspaceId};
use serde::Serialize;

/// The recording's fast-start MP4 and poster exist and it is `ready` (§4: `Ready` as soon as
/// the MP4 exists). Messaging sends the "ready" email from it (Day 48). Written in the same
/// transaction as the state change and the rendition rows.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingReady {
    pub recording_id: RecordingId,
    pub take_id: TakeId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub owner_id: UserId,
    pub title: String,
    pub duration_ms: u32,
    pub width: u32,
    pub height: u32,
}

impl DomainEvent for RecordingReady {
    const EVENT_TYPE: &'static str = "RecordingReady";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.recording_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

/// Processing gave up on the recording; it is `failed` and can be retried (Day 48). `reason`
/// is creator-facing.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessingFailed {
    pub recording_id: RecordingId,
    pub take_id: TakeId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub owner_id: UserId,
    pub title: String,
    pub reason: String,
}

impl DomainEvent for ProcessingFailed {
    const EVENT_TYPE: &'static str = "ProcessingFailed";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.recording_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

/// A rendition other than the first MP4 now exists (the HLS ladder, later the sprite and
/// captions): live status and the player pick it up from here (Day 82). Written in the same
/// transaction as the rendition rows.
#[derive(Debug, Clone, Serialize)]
pub struct RenditionReady {
    pub recording_id: RecordingId,
    pub take_id: TakeId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    /// The rendition kind: `hls`.
    pub kind: String,
    /// The rungs a ladder holds, lowest first; empty for a single-file rendition.
    pub variants: Vec<String>,
}

impl DomainEvent for RenditionReady {
    const EVENT_TYPE: &'static str = "RenditionReady";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.recording_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}
