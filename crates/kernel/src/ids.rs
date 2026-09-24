use crate::id::Id;

pub struct User;
pub type UserId = Id<User>;

pub struct Workspace;
pub type WorkspaceId = Id<Workspace>;

pub struct Session;
pub type SessionId = Id<Session>;
