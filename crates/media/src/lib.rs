#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;
#[cfg(test)]
mod testing;

pub use app::{
    BUILD_HLS, BuildHls, BuildHlsError, EnqueueError, GENERATE_SPRITE, GenerateSprite,
    GenerateSpriteError, HlsError, HlsOutcome, MediaService, PROCESS_TAKE, PlaybackKeys,
    ProcessError, ProcessOutcome, ProcessTake, RenditionReader, RequestLadderError, RetryError,
    RetryService, SourceKey, SpriteError, SpriteOutcome, TakeFinalizedData, TakeFinalizedMessage,
    TranscodeError, TranscodeReport, transcode_file,
};
pub use domain::edl::{
    Edl, EdlError, EdlJsonError, EdlRanges, KeptRange, MAX_RANGES, MIN_RANGE_MS,
};
pub use domain::probe::SourceInfo;
pub use domain::transcode::Mp4Plan;
pub use domain::{ChunkManifest, Container, ManifestChunk, ManifestError};
pub use infra::scratch::{ScratchDir, ScratchSpace};
pub use infra::tools::MediaTools;
