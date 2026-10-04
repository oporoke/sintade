//! Domain events published by `ingest`.

use kernel::{DomainEvent, RecordingId, TakeId, WorkspaceId};
use serde::Serialize;

/// A take's chunks are all uploaded and acked; media can process it (docs/design.md §4:
/// `TakeFinalized { take_id, chunk_count, mime }`). Written to the outbox in the same
/// transaction that marks the take finalized. It also carries the chunk manifest (ADR-0012),
/// so media can fetch and verify the chunks without reading ingest's tables.
#[derive(Debug, Clone, Serialize)]
pub struct TakeFinalized {
    pub take_id: TakeId,
    pub recording_id: RecordingId,
    #[serde(skip)]
    pub workspace_id: WorkspaceId,
    pub chunk_count: u32,
    pub duration_ms: u32,
    pub mime: String,
    /// `0..chunk_count`, ascending.
    pub chunks: Vec<FinalizedChunk>,
}

/// One chunk of a finalized take: where it is stored and what the client said it contains.
#[derive(Debug, Clone, Serialize)]
pub struct FinalizedChunk {
    pub idx: u32,
    pub key: String,
    pub size_bytes: u32,
    /// Lowercase hex.
    pub sha256: String,
}

impl DomainEvent for TakeFinalized {
    const EVENT_TYPE: &'static str = "TakeFinalized";

    fn aggregate_id(&self) -> uuid::Uuid {
        self.take_id.into_uuid()
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}
