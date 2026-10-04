mod slug;
mod visibility;

pub use slug::{SLUG_LEN, Slug};
pub use visibility::Visibility;

use kernel::{RecordingId, ShareLinkId, WorkspaceId};
use time::OffsetDateTime;

/// A share link as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareLinkView {
    pub id: ShareLinkId,
    pub workspace_id: WorkspaceId,
    pub recording_id: RecordingId,
    pub slug: String,
    pub visibility: Visibility,
    pub allow_download: bool,
    pub expires_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl ShareLinkView {
    /// Whether the link still resolves at `now`: not revoked, not expired.
    pub fn is_active(&self, now: OffsetDateTime) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|expires| expires > now)
    }
}
