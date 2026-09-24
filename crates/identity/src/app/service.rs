use std::sync::Arc;

use platform::{Clock, JobQueue};
use sqlx::PgPool;

pub struct IdentityService {
    pub(super) pool: PgPool,
    pub(super) queue: JobQueue,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) public_base_url: String,
    pub(super) session_secret: Vec<u8>,
}

impl IdentityService {
    pub fn new(
        pool: PgPool,
        queue: JobQueue,
        clock: Arc<dyn Clock>,
        public_base_url: String,
        session_secret: Vec<u8>,
    ) -> Self {
        Self {
            pool,
            queue,
            clock,
            public_base_url,
            session_secret,
        }
    }
}
