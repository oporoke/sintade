#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{
    CHUNK_URL_TTL, IngestService, PresignChunks, PresignError, PresignedChunk, StartRecording,
    StartRecordingError, StartedRecording,
};
pub use domain::{
    ChunkRangeError, MAX_CHUNK_INDEX, MAX_MIME_TYPE_LEN, MAX_PRESIGN_BATCH, MimeType,
    MimeTypeError, Sources, chunk_key,
};
