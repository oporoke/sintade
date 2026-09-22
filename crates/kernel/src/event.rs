use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::id::Id;

pub struct Workspace;
pub type WorkspaceId = Id<Workspace>;

pub trait DomainEvent: Serialize {
    const EVENT_TYPE: &'static str;

    fn aggregate_id(&self) -> Uuid;
    fn workspace_id(&self) -> WorkspaceId;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub id: Uuid,
    pub event_type: &'static str,
    pub aggregate_id: Uuid,
    pub workspace_id: WorkspaceId,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
    pub version: u16,
    pub data: T,
}
