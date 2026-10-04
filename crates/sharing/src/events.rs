//! Domain events published by `sharing` (docs/design.md §9 event table).

use kernel::{DomainEvent, RecordingId, ShareLinkId, UserId, WorkspaceId};
use serde::Serialize;

/// A share link was created for a recording.
#[derive(Debug, Clone, Serialize)]
pub struct LinkCreated {
    pub link_id: ShareLinkId,
    pub recording_id: RecordingId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub created_by: UserId,
    pub visibility: String,
}

impl DomainEvent for LinkCreated {
    const EVENT_TYPE: &'static str = "LinkCreated";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.link_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}
