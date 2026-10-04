use crate::id::Id;

pub struct User;
pub type UserId = Id<User>;

pub struct Workspace;
pub type WorkspaceId = Id<Workspace>;

pub struct Session;
pub type SessionId = Id<Session>;

pub struct Recording;
pub type RecordingId = Id<Recording>;

pub struct Take;
pub type TakeId = Id<Take>;

pub struct ShareLink;
pub type ShareLinkId = Id<ShareLink>;
