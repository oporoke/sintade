#![deny(clippy::unwrap_used)]

mod access;
mod entitlements;
mod error;
mod event;
mod id;
mod ids;

pub use access::{Permission, Role, UnknownRole};
pub use entitlements::Entitlements;
pub use error::AppError;
pub use event::{DomainEvent, EventEnvelope};
pub use id::Id;
pub use ids::{
    Recording, RecordingId, Session, SessionId, ShareLink, ShareLinkId, Take, TakeId, User, UserId,
    Workspace, WorkspaceId,
};
