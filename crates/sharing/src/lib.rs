#![deny(clippy::unwrap_used)]

mod app;
mod domain;
pub mod events;
mod infra;

pub use app::{LinkPatch, NewLink, SharingError, SharingService};
pub use domain::{SLUG_LEN, ShareLinkView, Slug, Visibility};
