use std::sync::Arc;

use catalog::CatalogService;
use kernel::{RecordingId, ShareLinkId, UserId, WorkspaceId};
use platform::{Clock, Outbox, OutboxError};
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::domain::{ShareLinkView, Slug, Visibility};
use crate::events::LinkCreated;
use crate::infra;

/// Slug collisions are astronomically unlikely (71 bits); a few redraws cover them anyway.
const SLUG_ATTEMPTS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    /// No such recording or link in the caller's workspace.
    #[error("not found")]
    NotFound,

    #[error("the expiry must be in the future")]
    ExpiryInPast,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("outbox error: {0}")]
    Outbox(#[from] OutboxError),

    #[error("could not draw an unused slug")]
    SlugExhausted,
}

#[derive(Debug, Clone, Copy)]
pub struct NewLink {
    pub visibility: Visibility,
    pub allow_download: bool,
    pub expires_at: Option<OffsetDateTime>,
}

/// A partial update: `None` leaves a field alone; `expires_at: Some(None)` clears the expiry.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinkPatch {
    pub visibility: Option<Visibility>,
    pub allow_download: Option<bool>,
    pub expires_at: Option<Option<OffsetDateTime>>,
}

pub struct SharingService {
    pool: PgPool,
    catalog: Arc<CatalogService>,
    outbox: Outbox,
    clock: Arc<dyn Clock>,
}

impl SharingService {
    pub fn new(pool: PgPool, catalog: Arc<CatalogService>, clock: Arc<dyn Clock>) -> Self {
        Self {
            pool,
            catalog,
            outbox: Outbox::new(clock.clone()),
            clock,
        }
    }

    /// Creates a link for a recording in the caller's workspace, with a fresh 71-bit slug.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, recording_id = %recording_id))]
    pub async fn create_link(
        &self,
        workspace_id: WorkspaceId,
        actor: UserId,
        recording_id: RecordingId,
        new: NewLink,
    ) -> Result<ShareLinkView, SharingError> {
        self.check_expiry(new.expires_at)?;
        let mut tx = self.pool.begin().await?;
        if !self
            .catalog
            .exists_in_workspace(&mut tx, recording_id, workspace_id)
            .await?
        {
            return Err(SharingError::NotFound);
        }
        let id = ShareLinkId::new_v7();
        let mut slug = None;
        for _ in 0..SLUG_ATTEMPTS {
            let candidate = Slug::generate();
            if infra::insert(
                &mut tx,
                id,
                workspace_id,
                recording_id,
                candidate.as_str(),
                new.visibility,
                new.allow_download,
                new.expires_at,
            )
            .await?
            {
                slug = Some(candidate);
                break;
            }
        }
        if slug.is_none() {
            return Err(SharingError::SlugExhausted);
        }
        self.outbox
            .push(
                &mut tx,
                &LinkCreated {
                    link_id: id,
                    recording_id,
                    workspace_id,
                    created_by: actor,
                    visibility: new.visibility.as_str().to_string(),
                },
            )
            .await?;
        let links = infra::list_for_recording(&mut tx, workspace_id, recording_id).await?;
        tx.commit().await?;
        links
            .into_iter()
            .find(|link| link.id == id)
            .ok_or(SharingError::NotFound)
    }

    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, recording_id = %recording_id))]
    pub async fn list_links(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Vec<ShareLinkView>, SharingError> {
        let mut conn = self.pool.acquire().await?;
        if !self
            .catalog
            .exists_in_workspace(&mut conn, recording_id, workspace_id)
            .await?
        {
            return Err(SharingError::NotFound);
        }
        Ok(infra::list_for_recording(&mut conn, workspace_id, recording_id).await?)
    }

    /// Changes a live link's policy. A revoked link can't be changed (`NotFound`).
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, link_id = %link_id))]
    pub async fn update_policy(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        link_id: ShareLinkId,
        patch: LinkPatch,
    ) -> Result<ShareLinkView, SharingError> {
        if let Some(Some(expires_at)) = patch.expires_at {
            self.check_expiry(Some(expires_at))?;
        }
        let mut conn = self.pool.acquire().await?;
        infra::update(
            &mut conn,
            workspace_id,
            recording_id,
            link_id,
            patch.visibility,
            patch.allow_download,
            patch.expires_at,
        )
        .await?
        .ok_or(SharingError::NotFound)
    }

    /// Revokes a link; it stops resolving at once (no cache sits in front of `find_by_slug`).
    /// Idempotent for a link of the recording.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, link_id = %link_id))]
    pub async fn revoke(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        link_id: ShareLinkId,
    ) -> Result<(), SharingError> {
        let mut conn = self.pool.acquire().await?;
        if infra::revoke(&mut conn, workspace_id, recording_id, link_id).await? {
            Ok(())
        } else {
            Err(SharingError::NotFound)
        }
    }

    /// The live (not revoked, not expired) link behind a public slug.
    #[tracing::instrument(skip_all)]
    pub async fn resolve(&self, slug: &str) -> Result<Option<ShareLinkView>, SharingError> {
        let Some(slug) = Slug::parse(slug) else {
            return Ok(None);
        };
        let mut conn = self.pool.acquire().await?;
        let found = infra::find_by_slug(&mut conn, slug.as_str()).await?;
        let now = self.clock.now();
        Ok(found.filter(|link| link.is_active(now)))
    }

    fn check_expiry(&self, expires_at: Option<OffsetDateTime>) -> Result<(), SharingError> {
        match expires_at {
            Some(at) if at <= self.clock.now() => Err(SharingError::ExpiryInPast),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashSet;
    use std::time::Duration;

    use super::*;

    pub(crate) struct Seeded {
        pub workspace: WorkspaceId,
        pub owner: UserId,
        pub recording: RecordingId,
    }

    /// A user, a workspace and a `ready` recording in it.
    pub(crate) async fn seed(pool: &PgPool) -> Seeded {
        let owner = UserId::new_v7();
        let workspace = WorkspaceId::new_v7();
        let recording = RecordingId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Sharer')",
            owner.into_uuid(),
            format!("sharing-{owner}@example.com"),
        )
        .execute(pool)
        .await
        .expect("user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name) VALUES ($1, 'Sharing Test')",
            workspace.into_uuid()
        )
        .execute(pool)
        .await
        .expect("workspace");
        sqlx::query!(
            "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
             VALUES ($1, $2, $3, 'Shared', 'ready')",
            recording.into_uuid(),
            workspace.into_uuid(),
            owner.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("recording");
        Seeded {
            workspace,
            owner,
            recording,
        }
    }

    pub(crate) fn service(pool: PgPool) -> SharingService {
        SharingService::new(
            pool,
            Arc::new(CatalogService::new()),
            Arc::new(platform::SystemClock),
        )
    }

    fn link() -> NewLink {
        NewLink {
            visibility: Visibility::Link,
            allow_download: false,
            expires_at: None,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn every_link_gets_its_own_slug(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool.clone());
        let mut slugs = HashSet::new();
        for _ in 0..25 {
            let created = sharing
                .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
                .await
                .expect("create");
            assert_eq!(created.slug.len(), crate::SLUG_LEN);
            assert!(slugs.insert(created.slug));
        }
        // A second recording shares the one global slug space.
        let listed = sharing
            .list_links(seeded.workspace, seeded.recording)
            .await
            .expect("list");
        assert_eq!(listed.len(), 25);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_duplicate_slug_is_refused_by_the_database(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool.clone());
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");
        let mut conn = pool.acquire().await.expect("conn");
        let inserted = infra::insert(
            &mut conn,
            ShareLinkId::new_v7(),
            seeded.workspace,
            seeded.recording,
            &created.slug,
            Visibility::Link,
            false,
            None,
        )
        .await
        .expect("insert");
        assert!(!inserted, "the slug is unique");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn revoke_takes_effect_immediately(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool);
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");
        assert!(
            sharing
                .resolve(&created.slug)
                .await
                .expect("resolve")
                .is_some()
        );

        sharing
            .revoke(seeded.workspace, seeded.recording, created.id)
            .await
            .expect("revoke");
        assert!(
            sharing
                .resolve(&created.slug)
                .await
                .expect("resolve")
                .is_none(),
            "a revoked link stops resolving at once"
        );
        // Revoking again is fine; changing a revoked link is not.
        sharing
            .revoke(seeded.workspace, seeded.recording, created.id)
            .await
            .expect("idempotent");
        let patched = sharing
            .update_policy(
                seeded.workspace,
                seeded.recording,
                created.id,
                LinkPatch {
                    visibility: Some(Visibility::Public),
                    ..LinkPatch::default()
                },
            )
            .await;
        assert!(matches!(patched, Err(SharingError::NotFound)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_changes_only_what_it_is_given(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool);
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");
        let in_a_day = platform::SystemClock.now() + Duration::from_secs(86_400);

        let updated = sharing
            .update_policy(
                seeded.workspace,
                seeded.recording,
                created.id,
                LinkPatch {
                    visibility: Some(Visibility::Workspace),
                    allow_download: Some(true),
                    expires_at: Some(Some(in_a_day)),
                },
            )
            .await
            .expect("update");
        assert_eq!(updated.visibility, Visibility::Workspace);
        assert!(updated.allow_download);
        assert!(updated.expires_at.is_some());
        assert_eq!(updated.slug, created.slug);

        let cleared = sharing
            .update_policy(
                seeded.workspace,
                seeded.recording,
                created.id,
                LinkPatch {
                    expires_at: Some(None),
                    ..LinkPatch::default()
                },
            )
            .await
            .expect("clear expiry");
        assert_eq!(cleared.visibility, Visibility::Workspace, "untouched");
        assert!(cleared.expires_at.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_expired_link_stops_resolving(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool.clone());
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");
        sqlx::query!(
            "UPDATE share_links SET expires_at = now() - interval '1 second' WHERE id = $1",
            created.id.into_uuid()
        )
        .execute(&pool)
        .await
        .expect("expire");
        assert!(
            sharing
                .resolve(&created.slug)
                .await
                .expect("resolve")
                .is_none()
        );

        let past = sharing
            .create_link(
                seeded.workspace,
                seeded.owner,
                seeded.recording,
                NewLink {
                    expires_at: Some(platform::SystemClock.now() - Duration::from_secs(5)),
                    ..link()
                },
            )
            .await;
        assert!(matches!(past, Err(SharingError::ExpiryInPast)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn other_workspaces_cannot_touch_the_link(pool: PgPool) {
        let seeded = seed(&pool).await;
        let other = seed(&pool).await;
        let sharing = service(pool);
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");

        // The other workspace can't create, list, change or revoke it.
        assert!(matches!(
            sharing
                .create_link(other.workspace, other.owner, seeded.recording, link())
                .await,
            Err(SharingError::NotFound)
        ));
        assert!(matches!(
            sharing.list_links(other.workspace, seeded.recording).await,
            Err(SharingError::NotFound)
        ));
        assert!(matches!(
            sharing
                .revoke(other.workspace, seeded.recording, created.id)
                .await,
            Err(SharingError::NotFound)
        ));
        assert!(matches!(
            sharing
                .update_policy(
                    other.workspace,
                    seeded.recording,
                    created.id,
                    LinkPatch::default()
                )
                .await,
            Err(SharingError::NotFound)
        ));
        assert!(
            sharing
                .resolve(&created.slug)
                .await
                .expect("resolve")
                .is_some()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn creating_a_link_writes_link_created(pool: PgPool) {
        let seeded = seed(&pool).await;
        let sharing = service(pool.clone());
        let created = sharing
            .create_link(seeded.workspace, seeded.owner, seeded.recording, link())
            .await
            .expect("create");
        let events = sqlx::query_scalar!(
            "SELECT count(*) AS \"n!\" FROM outbox_events WHERE event_type = 'LinkCreated' AND aggregate_id = $1",
            created.id.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(events, 1);
    }
}
