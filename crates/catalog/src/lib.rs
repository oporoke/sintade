#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{CatalogService, NewRecording};
pub use domain::{MAX_TITLE_CHARS, Title, TitleError};
