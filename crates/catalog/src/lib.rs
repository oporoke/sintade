#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{
    CatalogService, LibraryItem, LibraryPage, ManageError, Measured, NewRecording, PurgeReport,
    RecordingManager, RecordingOwner, Reopened, TRASH_RETENTION, WatchInfo,
};
pub use domain::{Chapter, ChapterError, ChapterList, MAX_CHAPTER_TITLE_CHARS, MAX_CHAPTERS};
pub use domain::{Cursor, LIBRARY_PAGE_SIZE, MAX_TITLE_CHARS, Title, TitleError};
