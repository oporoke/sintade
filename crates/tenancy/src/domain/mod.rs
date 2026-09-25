use kernel::{Permission, Role};

/// The role → permission matrix, following README §5's draft (TODO: Verify, ADR-0007): owners
/// do everything, admins everything but billing, members create and edit, viewers only read.
/// "Own vs others' recordings" needs the resource, so it is checked per resource, not here.
pub fn grants(role: Role, permission: Permission) -> bool {
    match role {
        Role::Owner => true,
        Role::Admin => permission != Permission::ManageBilling,
        Role::Member => matches!(
            permission,
            Permission::ViewWorkspace
                | Permission::CreateRecording
                | Permission::EditRecording
                | Permission::DeleteRecording
        ),
        Role::Viewer => permission == Permission::ViewWorkspace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_can_view_its_workspace() {
        for role in Role::ALL {
            assert!(grants(role, Permission::ViewWorkspace), "{role} must view");
        }
    }

    #[test]
    fn owner_has_every_permission() {
        for permission in Permission::ALL {
            assert!(grants(Role::Owner, permission));
        }
    }

    #[test]
    fn only_owner_manages_billing() {
        for role in [Role::Admin, Role::Member, Role::Viewer] {
            assert!(!grants(role, Permission::ManageBilling), "{role} must not");
        }
    }

    #[test]
    fn members_cannot_manage_members_but_admins_can() {
        assert!(grants(Role::Admin, Permission::ManageMembers));
        assert!(!grants(Role::Member, Permission::ManageMembers));
    }

    #[test]
    fn viewer_is_read_only() {
        for permission in Permission::ALL {
            assert_eq!(
                grants(Role::Viewer, permission),
                permission == Permission::ViewWorkspace
            );
        }
    }
}
