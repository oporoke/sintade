use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use kernel::{AppError, Permission, Role, UserId, WorkspaceId};
use tenancy::AuthorizeError;

use crate::app::AppState;
use crate::error::ApiError;
use crate::session::SessionClaims;

/// The caller's session resolved against a workspace: who they are, which workspace they're
/// acting in, and their role there. Any handler touching workspace-owned resources takes this
/// and filters every query by `workspace_id`. The role is re-read from `memberships` on each
/// request (via `TenancyService::authorize`), so removal or a role change applies immediately.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no workspace-scoped route until Day 33; the tenant-isolation harness uses it"
    )
)]
pub struct WorkspaceContext {
    #[cfg_attr(
        test,
        expect(dead_code, reason = "the test probe route only needs the workspace")
    )]
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub role: Role,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no workspace-scoped route until Day 33; the tenant-isolation harness uses it"
    )
)]
impl WorkspaceContext {
    /// Checks a finer-grained permission after the extractor has already established
    /// membership. A member lacking it gets `403`: they can see the workspace exists.
    pub fn require(&self, permission: Permission) -> Result<(), ApiError> {
        if tenancy::grants(self.role, permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden("insufficient role".to_string()).into())
        }
    }
}

impl FromRequestParts<AppState> for WorkspaceContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let claims = SessionClaims::from_request_parts(parts, state).await?;
        let role = state
            .tenancy
            .authorize(
                claims.user_id,
                Permission::ViewWorkspace,
                claims.workspace_id,
            )
            .await?;
        Ok(Self {
            user_id: claims.user_id,
            workspace_id: claims.workspace_id,
            role,
        })
    }
}

impl From<AuthorizeError> for ApiError {
    fn from(error: AuthorizeError) -> Self {
        match error {
            AuthorizeError::NotFound => AppError::NotFound.into(),
            AuthorizeError::Forbidden { .. } => {
                AppError::Forbidden("insufficient role".to_string()).into()
            }
            AuthorizeError::UnknownRole(_) | AuthorizeError::Database(_) => {
                tracing::error!(%error, "authorize failed");
                AppError::Internal("authorization unavailable".to_string()).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    fn context(role: Role) -> WorkspaceContext {
        WorkspaceContext {
            user_id: UserId::new_v7(),
            workspace_id: WorkspaceId::new_v7(),
            role,
        }
    }

    #[test]
    fn require_passes_when_the_role_grants_the_permission() {
        assert!(
            context(Role::Member)
                .require(Permission::EditRecording)
                .is_ok()
        );
    }

    #[test]
    fn require_is_403_when_the_role_lacks_the_permission() {
        let error = context(Role::Viewer)
            .require(Permission::EditRecording)
            .expect_err("viewer cannot edit");
        assert_eq!(
            error.into_response().status(),
            axum::http::StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn non_membership_maps_to_404_not_403() {
        let response = ApiError::from(AuthorizeError::NotFound).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
