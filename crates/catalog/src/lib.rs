#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{
    CatalogService, LibraryItem, LibraryPage, Measured, NewRecording, RecordingOwner, Reopened,
    WatchInfo,
};
pub use domain::{Cursor, LIBRARY_PAGE_SIZE, MAX_TITLE_CHARS, Title, TitleError};
