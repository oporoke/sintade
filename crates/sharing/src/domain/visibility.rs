use serde::{Deserialize, Serialize};

/// Who may open a share link (docs/design.md §4 Sharing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Only the workspace members who can see the recording (the owner, in practice).
    Private,
    /// Members of the recording's workspace.
    Workspace,
    /// Anyone with the link.
    Link,
    /// Anyone with the link; may be listed or indexed later (V1).
    Public,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Workspace => "workspace",
            Self::Link => "link",
            Self::Public => "public",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "private" => Some(Self::Private),
            "workspace" => Some(Self::Workspace),
            "link" => Some(Self::Link),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for visibility in [
            Visibility::Private,
            Visibility::Workspace,
            Visibility::Link,
            Visibility::Public,
        ] {
            assert_eq!(Visibility::parse(visibility.as_str()), Some(visibility));
        }
        assert_eq!(Visibility::parse("everyone"), None);
    }
}
