use kernel::{UserId, WorkspaceId};
use sqlx::PgConnection;

/// The port through which `identity` reaches workspaces, which `tenancy` owns. The allowed L2
/// edge is `tenancy → identity`, so `identity` can't call `tenancy`; instead `tenancy`
/// implements this trait and the binary injects it (ADR-0007, retiring ADR-0004's direct
/// writes). Both methods run on the caller's connection so they join `identity`'s transactions.
#[async_trait::async_trait]
pub trait WorkspaceDirectory: Send + Sync {
    async fn create_personal_workspace(
        &self,
        conn: &mut PgConnection,
        owner: UserId,
        name: &str,
    ) -> Result<WorkspaceId, sqlx::Error>;

    async fn personal_workspace_for(
        &self,
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<WorkspaceId>, sqlx::Error>;
}
