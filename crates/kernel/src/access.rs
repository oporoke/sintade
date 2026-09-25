use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A member's role in a workspace (`member_role` in Postgres). Shared-kernel type per
/// `docs/design.md`'s context map ("Tenancy | all | Shared kernel | `WorkspaceId`, `Role`,
/// `Permission`"); which permissions each role grants is decided by `tenancy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Admin, Role::Member, Role::Viewer];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown role: {0}")]
pub struct UnknownRole(pub String);

impl FromStr for Role {
    type Err = UnknownRole;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Role::ALL
            .into_iter()
            .find(|role| role.as_str() == value)
            .ok_or_else(|| UnknownRole(value.to_string()))
    }
}

/// Something a caller may do inside a workspace, checked via `tenancy`'s `authorize()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// Read anything in the workspace. Every member has it; `WorkspaceContext` checks it.
    ViewWorkspace,
    CreateRecording,
    EditRecording,
    DeleteRecording,
    ManageMembers,
    ManageBilling,
}

impl Permission {
    pub const ALL: [Permission; 6] = [
        Permission::ViewWorkspace,
        Permission::CreateRecording,
        Permission::EditRecording,
        Permission::DeleteRecording,
        Permission::ManageMembers,
        Permission::ManageBilling,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_round_trips_through_its_string_form() {
        for role in Role::ALL {
            assert_eq!(role.as_str().parse::<Role>().ok(), Some(role));
        }
        assert!("superuser".parse::<Role>().is_err());
    }
}
