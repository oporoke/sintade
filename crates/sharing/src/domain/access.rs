use super::{ShareLinkView, Visibility};

/// Who is opening a share link, as far as access is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewer {
    /// Signed in?
    pub signed_in: bool,
    /// Signed in and a member of the recording's workspace.
    pub workspace_member: bool,
    /// Signed in and the recording's owner.
    pub owner: bool,
}

impl Viewer {
    pub const ANONYMOUS: Self = Self {
        signed_in: false,
        workspace_member: false,
        owner: false,
    };
}

/// What a link lets a viewer do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Signing in could change the answer (a workspace link opened anonymously).
    LoginRequired,
    /// Not for this viewer. Callers answer `404`, never `403`: a private recording's link must
    /// not reveal that it exists (CLAUDE.md rule 5).
    Hidden,
}

/// Whether `viewer` may open `link`. The link must already be live (not revoked or expired);
/// `SharingService::resolve` guarantees that.
///
/// | visibility | anonymous | signed in | member | owner |
/// | --- | --- | --- | --- | --- |
/// | `private` | hidden | hidden | hidden | allow |
/// | `workspace` | login | hidden | allow | allow |
/// | `link`, `public` | allow | allow | allow | allow |
pub fn decide(link: &ShareLinkView, viewer: Viewer) -> Decision {
    match link.visibility {
        Visibility::Link | Visibility::Public => Decision::Allow,
        Visibility::Workspace => {
            if viewer.workspace_member || viewer.owner {
                Decision::Allow
            } else if viewer.signed_in {
                Decision::Hidden
            } else {
                Decision::LoginRequired
            }
        }
        Visibility::Private => {
            if viewer.owner {
                Decision::Allow
            } else {
                Decision::Hidden
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{RecordingId, ShareLinkId, WorkspaceId};
    use time::OffsetDateTime;

    use super::*;

    fn link(visibility: Visibility) -> ShareLinkView {
        ShareLinkView {
            id: ShareLinkId::new_v7(),
            workspace_id: WorkspaceId::new_v7(),
            recording_id: RecordingId::new_v7(),
            slug: "abcdefghijkl".to_string(),
            visibility,
            allow_download: false,
            expires_at: None,
            revoked_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    const OTHER: Viewer = Viewer {
        signed_in: true,
        workspace_member: false,
        owner: false,
    };
    const MEMBER: Viewer = Viewer {
        signed_in: true,
        workspace_member: true,
        owner: false,
    };
    const OWNER: Viewer = Viewer {
        signed_in: true,
        workspace_member: true,
        owner: true,
    };

    #[test]
    fn the_access_table() {
        use Decision::{Allow, Hidden, LoginRequired};
        let cases = [
            (Visibility::Private, [Hidden, Hidden, Hidden, Allow]),
            (Visibility::Workspace, [LoginRequired, Hidden, Allow, Allow]),
            (Visibility::Link, [Allow, Allow, Allow, Allow]),
            (Visibility::Public, [Allow, Allow, Allow, Allow]),
        ];
        for (visibility, expected) in cases {
            let link = link(visibility);
            let got = [Viewer::ANONYMOUS, OTHER, MEMBER, OWNER].map(|viewer| decide(&link, viewer));
            assert_eq!(got, expected, "{visibility:?}");
        }
    }
}
