use kernel::{UserId, WorkspaceId};
use serde::Serialize;

use crate::app::service::IdentityService;
use crate::infra;

#[derive(Debug, Serialize)]
pub struct MeUser {
    pub id: UserId,
    pub email: String,
    pub display_name: String,
    pub email_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct MeWorkspace {
    pub id: WorkspaceId,
    pub name: String,
    pub role: String,
    pub is_personal: bool,
}

#[derive(Debug, Serialize)]
pub struct MeView {
    pub user: MeUser,
    pub workspaces: Vec<MeWorkspace>,
}

impl IdentityService {
    #[tracing::instrument(skip_all)]
    pub async fn me(&self, user_id: UserId) -> Result<Option<MeView>, sqlx::Error> {
        let Some(user) = infra::find_user(&self.pool, user_id).await? else {
            return Ok(None);
        };
        let workspaces = infra::list_user_workspaces(&self.pool, user_id).await?;

        Ok(Some(MeView {
            user: MeUser {
                id: user.id,
                email: user.email,
                display_name: user.display_name,
                email_verified: user.email_verified,
            },
            workspaces: workspaces
                .into_iter()
                .map(|workspace| MeWorkspace {
                    id: workspace.id,
                    name: workspace.name,
                    role: workspace.role,
                    is_personal: workspace.is_personal,
                })
                .collect(),
        }))
    }
}
