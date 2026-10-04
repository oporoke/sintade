#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{
    ABANDON_AFTER, AckChunk, AckError, AckOutcome, CHUNK_URL_TTL, FinalizeError, FinalizeOutcome,
    FinalizeTake, IngestService, MAX_MISSING_LISTED, PURGE_AFTER, PresignChunks, PresignError,
    PresignedChunk, ReceivedChunk, StartRecording, StartRecordingError, StartedRecording,
    SweepError, SweepReport, TakeStatus,
};
pub use domain::{
    ChunkRangeError, ChunkSizeError, DigestError, MAX_CHUNK_BYTES, MAX_CHUNK_INDEX,
    MAX_MIME_TYPE_LEN, MAX_PRESIGN_BATCH, MimeType, MimeTypeError, Sha256Digest, Sources,
    chunk_key,
};
