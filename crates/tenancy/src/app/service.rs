use kernel::{Permission, Role, UnknownRole, UserId, WorkspaceId};
use sqlx::{PgConnection, PgPool};

use crate::domain::grants;
use crate::infra;

/// Why `authorize()` refused. `NotFound` covers both "no such workspace" and "not a member":
/// the caller must not learn which (CLAUDE.md rule 5 -- inaccessible resources are `404`).
#[derive(Debug, thiserror::Error)]
pub enum AuthorizeError {
    #[error("workspace not found")]
    NotFound,

    #[error("role {role} lacks {permission:?}")]
    Forbidden { role: Role, permission: Permission },

    #[error(transparent)]
    UnknownRole(#[from] UnknownRole),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct Membership {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub role: Role,
    pub is_personal: bool,
}

pub struct TenancyService {
    pool: PgPool,
}

impl TenancyService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a personal workspace with `owner` as its owner. Runs on the caller's connection
    /// rather than opening its own transaction, because registration must create the user and
    /// their workspace atomically (US-01) -- `identity` owns that transaction.
    #[tracing::instrument(skip_all, fields(user_id = %owner))]
    pub async fn create_personal_workspace(
        &self,
        conn: &mut PgConnection,
        owner: UserId,
        name: &str,
    ) -> Result<WorkspaceId, sqlx::Error> {
        let workspace_id = WorkspaceId::new_v7();
        infra::insert_workspace(conn, workspace_id, name, true).await?;
        infra::insert_membership(conn, workspace_id, owner, Role::Owner).await?;
        Ok(workspace_id)
    }

    #[tracing::instrument(skip_all, fields(user_id = %user_id))]
    pub async fn personal_workspace_for(
        &self,
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<WorkspaceId>, sqlx::Error> {
        infra::personal_workspace_id_for_user(&mut *conn, user_id).await
    }

    #[tracing::instrument(skip_all, fields(user_id = %user_id))]
    pub async fn list_memberships(
        &self,
        user_id: UserId,
    ) -> Result<Vec<Membership>, AuthorizeError> {
        infra::list_memberships_for_user(&self.pool, user_id)
            .await?
            .into_iter()
            .map(|record| {
                Ok(Membership {
                    workspace_id: record.workspace_id,
                    name: record.name,
                    role: record.role.parse()?,
                    is_personal: record.is_personal,
                })
            })
            .collect()
    }

    /// The single tenant check: is `user_id` a member of `workspace_id` whose role grants
    /// `permission`? Returns the role on success so callers can make finer decisions. The role
    /// is read from `memberships` on every call, never trusted from the session cookie, so a
    /// removed member or a downgraded role takes effect on the very next request.
    #[tracing::instrument(skip_all, fields(user_id = %user_id, workspace_id = %workspace_id))]
    pub async fn authorize(
        &self,
        user_id: UserId,
        permission: Permission,
        workspace_id: WorkspaceId,
    ) -> Result<Role, AuthorizeError> {
        let role: Role = infra::find_role(&self.pool, workspace_id, user_id)
            .await?
            .ok_or(AuthorizeError::NotFound)?
            .parse()?;
        if grants(role, permission) {
            Ok(role)
        } else {
            Err(AuthorizeError::Forbidden { role, permission })
        }
    }
}

/// `identity`'s port, implemented here over the allowed `tenancy → identity` edge (ADR-0007).
#[async_trait::async_trait]
impl identity::WorkspaceDirectory for TenancyService {
    async fn create_personal_workspace(
        &self,
        conn: &mut PgConnection,
        owner: UserId,
        name: &str,
    ) -> Result<WorkspaceId, sqlx::Error> {
        TenancyService::create_personal_workspace(self, conn, owner, name).await
    }

    async fn personal_workspace_for(
        &self,
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<WorkspaceId>, sqlx::Error> {
        TenancyService::personal_workspace_for(self, conn, user_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn insert_user(pool: &PgPool) -> UserId {
        let user_id = UserId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Tenancy Tester')",
            user_id.into_uuid(),
            format!("tenancy-{}@example.com", user_id),
        )
        .execute(pool)
        .await
        .expect("insert user");
        user_id
    }

    async fn personal_workspace(
        service: &TenancyService,
        pool: &PgPool,
        owner: UserId,
    ) -> WorkspaceId {
        let mut conn = pool.acquire().await.expect("connection");
        service
            .create_personal_workspace(&mut conn, owner, "Personal")
            .await
            .expect("create workspace")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn owner_is_authorized_in_their_own_workspace(pool: PgPool) {
        let service = TenancyService::new(pool.clone());
        let owner = insert_user(&pool).await;
        let workspace = personal_workspace(&service, &pool, owner).await;

        let role = service
            .authorize(owner, Permission::ManageBilling, workspace)
            .await
            .expect("owner authorized");
        assert_eq!(role, Role::Owner);
        let mut conn = pool.acquire().await.expect("connection");
        assert_eq!(
            service
                .personal_workspace_for(&mut conn, owner)
                .await
                .expect("query"),
            Some(workspace)
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_non_member_gets_not_found_not_forbidden(pool: PgPool) {
        let service = TenancyService::new(pool.clone());
        let alice = insert_user(&pool).await;
        let bob = insert_user(&pool).await;
        let alices_workspace = personal_workspace(&service, &pool, alice).await;

        let result = service
            .authorize(bob, Permission::ViewWorkspace, alices_workspace)
            .await;
        assert!(
            matches!(result, Err(AuthorizeError::NotFound)),
            "{result:?}"
        );

        let unknown = service
            .authorize(alice, Permission::ViewWorkspace, WorkspaceId::new_v7())
            .await;
        assert!(
            matches!(unknown, Err(AuthorizeError::NotFound)),
            "{unknown:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_viewer_is_forbidden_from_editing(pool: PgPool) {
        let service = TenancyService::new(pool.clone());
        let owner = insert_user(&pool).await;
        let viewer = insert_user(&pool).await;
        let workspace = personal_workspace(&service, &pool, owner).await;
        let mut conn = pool.acquire().await.expect("connection");
        infra::insert_membership(&mut conn, workspace, viewer, Role::Viewer)
            .await
            .expect("add viewer");

        assert_eq!(
            service
                .authorize(viewer, Permission::ViewWorkspace, workspace)
                .await
                .expect("viewer can view"),
            Role::Viewer
        );
        let result = service
            .authorize(viewer, Permission::EditRecording, workspace)
            .await;
        assert!(
            matches!(
                result,
                Err(AuthorizeError::Forbidden {
                    role: Role::Viewer,
                    ..
                })
            ),
            "{result:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_memberships_returns_only_the_callers_workspaces(pool: PgPool) {
        let service = TenancyService::new(pool.clone());
        let alice = insert_user(&pool).await;
        let bob = insert_user(&pool).await;
        let alices = personal_workspace(&service, &pool, alice).await;
        personal_workspace(&service, &pool, bob).await;

        let memberships = service.list_memberships(alice).await.expect("list");
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].workspace_id, alices);
        assert_eq!(memberships[0].role, Role::Owner);
        assert!(memberships[0].is_personal);
    }
}
