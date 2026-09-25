use kernel::{Role, UserId, WorkspaceId};
use sqlx::{PgConnection, PgExecutor};

pub async fn insert_workspace(
    conn: &mut PgConnection,
    id: WorkspaceId,
    name: &str,
    is_personal: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO workspaces (id, name, is_personal) VALUES ($1, $2, $3)",
        id.into_uuid(),
        name,
        is_personal,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn insert_membership(
    conn: &mut PgConnection,
    workspace_id: WorkspaceId,
    user_id: UserId,
    role: Role,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, $3::text::member_role)",
        workspace_id.into_uuid(),
        user_id.into_uuid(),
        role.as_str(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn personal_workspace_id_for_user(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<Option<WorkspaceId>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT w.id
        FROM workspaces w
        JOIN memberships m ON m.workspace_id = w.id
        WHERE m.user_id = $1 AND w.is_personal = true
        "#,
        user_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await
    .map(|maybe_id| maybe_id.map(WorkspaceId::from_uuid))
}

pub struct MembershipRecord {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub role: String,
    pub is_personal: bool,
}

pub async fn list_memberships_for_user(
    executor: impl PgExecutor<'_>,
    user_id: UserId,
) -> Result<Vec<MembershipRecord>, sqlx::Error> {
    sqlx::query!(
        r#"
        SELECT w.id, w.name, w.is_personal, m.role::text as "role!"
        FROM workspaces w
        JOIN memberships m ON m.workspace_id = w.id
        WHERE m.user_id = $1
        ORDER BY w.created_at
        "#,
        user_id.into_uuid(),
    )
    .fetch_all(executor)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| MembershipRecord {
                workspace_id: WorkspaceId::from_uuid(row.id),
                name: row.name,
                role: row.role,
                is_personal: row.is_personal,
            })
            .collect()
    })
}

/// The caller's role in `workspace_id`, or `None` if they aren't a member. Scoped by both
/// `workspace_id` and `user_id` -- this is the query every tenant check bottoms out in.
pub async fn find_role(
    executor: impl PgExecutor<'_>,
    workspace_id: WorkspaceId,
    user_id: UserId,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT role::text as "role!" FROM memberships WHERE workspace_id = $1 AND user_id = $2"#,
        workspace_id.into_uuid(),
        user_id.into_uuid(),
    )
    .fetch_optional(executor)
    .await
}
