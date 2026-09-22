#![deny(clippy::unwrap_used)]

mod error;
mod event;
mod id;

pub use error::AppError;
pub use event::{DomainEvent, EventEnvelope, Workspace, WorkspaceId};
pub use id::Id;
