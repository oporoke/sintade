use std::sync::Arc;

use platform::{Clock, JobQueue};

use crate::app::workspaces::WorkspaceDirectory;
use sqlx::PgPool;

pub struct IdentityService {
    pub(super) pool: PgPool,
    pub(super) queue: JobQueue,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) public_base_url: String,
    pub(super) session_secret: Vec<u8>,
    pub(super) workspaces: Arc<dyn WorkspaceDirectory>,
}

impl IdentityService {
    pub fn new(
        pool: PgPool,
        queue: JobQueue,
        clock: Arc<dyn Clock>,
        public_base_url: String,
        session_secret: Vec<u8>,
        workspaces: Arc<dyn WorkspaceDirectory>,
    ) -> Self {
        Self {
            pool,
            queue,
            clock,
            public_base_url,
            session_secret,
            workspaces,
        }
    }

    /// Best-effort enqueue: logs and swallows failure rather than propagating it, matching
    /// register's existing behavior -- an undeliverable notification email should never fail
    /// the request that triggered it.
    pub(super) async fn enqueue_email(&self, to: &str, subject: &str, body: &str) {
        let payload = serde_json::json!({
            "to": to,
            "subject": subject,
            "body": body,
        });
        if let Err(error) = self.queue.enqueue("SendEmail", payload).await {
            tracing::error!(%error, "failed to enqueue email");
        }
    }
}
