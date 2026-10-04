use std::sync::Arc;

use identity::IdentityService;
use kernel::UserId;
use messaging::{Recipient, RecipientDirectory};

/// Adapts `identity` (which owns users) to the port `messaging` sends through.
pub struct IdentityRecipients {
    identity: Arc<IdentityService>,
}

impl IdentityRecipients {
    pub fn new(identity: Arc<IdentityService>) -> Self {
        Self { identity }
    }
}

#[async_trait::async_trait]
impl RecipientDirectory for IdentityRecipients {
    async fn recipient(&self, user_id: UserId) -> Result<Option<Recipient>, String> {
        self.identity
            .me(user_id)
            .await
            .map(|user| {
                user.map(|user| Recipient {
                    email: user.email,
                    display_name: user.display_name,
                })
            })
            .map_err(|error| error.to_string())
    }
}
