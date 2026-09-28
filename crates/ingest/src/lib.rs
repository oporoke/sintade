#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{
    AckChunk, AckError, AckOutcome, CHUNK_URL_TTL, IngestService, PresignChunks, PresignError,
    PresignedChunk, StartRecording, StartRecordingError, StartedRecording,
};
pub use domain::{
    ChunkRangeError, ChunkSizeError, DigestError, MAX_CHUNK_BYTES, MAX_CHUNK_INDEX,
    MAX_MIME_TYPE_LEN, MAX_PRESIGN_BATCH, MimeType, MimeTypeError, Sha256Digest, Sources,
    chunk_key,
};
