use kernel::UserId;

use crate::app::me::MeUser;
use crate::app::service::IdentityService;
use crate::domain::{DisplayName, DisplayNameError};
use crate::infra;

#[derive(Debug, thiserror::Error)]
pub enum UpdateProfileError {
    #[error(transparent)]
    InvalidDisplayName(#[from] DisplayNameError),

    #[error("user not found")]
    NotFound,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct UpdateProfileRequest {
    pub display_name: String,
}

impl IdentityService {
    /// Profile settings (Day 19): today only the display name. Email changes need their own
    /// re-verification flow and are not part of this.
    #[tracing::instrument(skip_all, fields(user_id = %user_id))]
    pub async fn update_profile(
        &self,
        user_id: UserId,
        request: UpdateProfileRequest,
    ) -> Result<MeUser, UpdateProfileError> {
        let display_name = DisplayName::parse(&request.display_name)?;
        infra::update_display_name(&self.pool, user_id, display_name.as_str()).await?;
        self.me(user_id).await?.ok_or(UpdateProfileError::NotFound)
    }

    /// "Log out everywhere": revokes every refresh session the user has. Access cookies already
    /// issued stay valid until they expire (`ACCESS_TOKEN_TTL`, 15 min) -- they are stateless,
    /// the same trade-off a password reset already makes.
    #[tracing::instrument(skip_all, fields(user_id = %user_id))]
    pub async fn logout_all(&self, user_id: UserId) -> Result<(), sqlx::Error> {
        infra::revoke_all_sessions_for_user(&self.pool, user_id, self.clock.now()).await
    }
}
