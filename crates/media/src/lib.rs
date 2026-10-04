#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;
#[cfg(test)]
mod testing;

pub use app::{
    EnqueueError, MediaService, PROCESS_TAKE, ProcessError, ProcessOutcome, ProcessTake,
    TakeFinalizedData, TakeFinalizedMessage,
};
pub use domain::{ChunkManifest, ManifestChunk, ManifestError};
pub use infra::scratch::{ScratchDir, ScratchSpace};
pub use infra::tools::MediaTools;
