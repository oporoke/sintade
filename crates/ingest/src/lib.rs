#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{IngestService, StartRecording, StartRecordingError, StartedRecording};
pub use domain::{MAX_MIME_TYPE_LEN, MimeType, MimeTypeError, Sources};
