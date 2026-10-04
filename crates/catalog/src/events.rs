//! Domain events published by `catalog` (docs/design.md §9 event table).

use kernel::{DomainEvent, RecordingId, UserId, WorkspaceId};
use serde::Serialize;

/// A recording moved to the trash: its share links stop working at once (the watch page only
/// resolves live recordings) and it is purged after 30 days.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingTrashed {
    pub recording_id: RecordingId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub trashed_by: UserId,
}

impl DomainEvent for RecordingTrashed {
    const EVENT_TYPE: &'static str = "RecordingTrashed";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.recording_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

/// A trashed recording was deleted for good: its rows and everything under its storage prefix.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingPurged {
    pub recording_id: RecordingId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub objects_deleted: u64,
}

impl DomainEvent for RecordingPurged {
    const EVENT_TYPE: &'static str = "RecordingPurged";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.recording_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}
