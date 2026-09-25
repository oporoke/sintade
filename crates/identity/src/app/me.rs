use kernel::UserId;
use serde::Serialize;

use crate::app::service::IdentityService;
use crate::infra;

/// The identity half of `GET /me`. Workspaces come from `tenancy`; the handler composes both.
#[derive(Debug, Serialize)]
pub struct MeUser {
    pub id: UserId,
    pub email: String,
    pub display_name: String,
    pub email_verified: bool,
}

impl IdentityService {
    #[tracing::instrument(skip_all)]
    pub async fn me(&self, user_id: UserId) -> Result<Option<MeUser>, sqlx::Error> {
        Ok(infra::find_user(&self.pool, user_id)
            .await?
            .map(|user| MeUser {
                id: user.id,
                email: user.email,
                display_name: user.display_name,
                email_verified: user.email_verified,
            }))
    }
}
