#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{CatalogService, Measured, NewRecording, RecordingOwner, Reopened, WatchInfo};
pub use domain::{MAX_TITLE_CHARS, Title, TitleError};
