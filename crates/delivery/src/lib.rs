#![deny(clippy::unwrap_used)]

//! Playback grants: short-lived signed URLs for a recording's renditions (docs/design.md §5).
//! Stateless; the caller has already decided the viewer may watch (`sharing::decide`).

mod filename;

use std::sync::Arc;
use std::time::Duration;

use kernel::{RecordingId, WorkspaceId};
use media::RenditionReader;
use platform::{ObjectStore, StorageError};

pub use filename::download_filename;

/// How long a grant's URLs work (docs/design.md §16: presigned URLs ≤ 15 minutes).
pub const GRANT_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

/// What a viewer's player is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackGrant {
    pub mp4_url: String,
    pub poster_url: Option<String>,
    pub expires_in_s: u64,
}

/// A signed URL that saves the MP4 as a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadGrant {
    pub url: String,
    pub filename: String,
    pub expires_in_s: u64,
}

pub struct DeliveryService {
    store: Arc<dyn ObjectStore>,
    renditions: Arc<RenditionReader>,
}

impl DeliveryService {
    pub fn new(store: Arc<dyn ObjectStore>, renditions: Arc<RenditionReader>) -> Self {
        Self { store, renditions }
    }

    /// Signed URLs for the recording's MP4 (and poster), or `None` while no MP4 exists.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn grant(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<PlaybackGrant>, DeliveryError> {
        let Some(keys) = self
            .renditions
            .playback_keys(workspace_id, recording_id)
            .await?
        else {
            return Ok(None);
        };
        let mp4_url = self
            .store
            .presign_get(&keys.mp4, GRANT_TTL)
            .await?
            .to_string();
        let poster_url = match keys.poster {
            Some(key) => Some(self.store.presign_get(&key, GRANT_TTL).await?.to_string()),
            None => None,
        };
        Ok(Some(PlaybackGrant {
            mp4_url,
            poster_url,
            expires_in_s: GRANT_TTL.as_secs(),
        }))
    }

    /// A URL that downloads the recording's MP4 as `<title>.mp4`, or `None` while no MP4 exists.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn download(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        title: &str,
    ) -> Result<Option<DownloadGrant>, DeliveryError> {
        let Some(keys) = self
            .renditions
            .playback_keys(workspace_id, recording_id)
            .await?
        else {
            return Ok(None);
        };
        let filename = download_filename(title);
        let url = self
            .store
            .presign_download(&keys.mp4, GRANT_TTL, &filename)
            .await?
            .to_string();
        Ok(Some(DownloadGrant {
            url,
            filename,
            expires_in_s: GRANT_TTL.as_secs(),
        }))
    }

    /// Signed poster URLs for a page of recordings (the library's thumbnails); recordings
    /// without a poster are simply absent.
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id, recordings = recording_ids.len()))]
    pub async fn posters(
        &self,
        workspace_id: WorkspaceId,
        recording_ids: &[RecordingId],
    ) -> Result<std::collections::HashMap<RecordingId, String>, DeliveryError> {
        let mut urls = std::collections::HashMap::new();
        for (recording_id, key) in self
            .renditions
            .poster_keys(workspace_id, recording_ids)
            .await?
        {
            let url = self.store.presign_get(&key, GRANT_TTL).await?.to_string();
            urls.insert(recording_id, url);
        }
        Ok(urls)
    }

    /// Just the poster URL (a watch page for a recording that is still processing has none yet).
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn poster(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<String>, DeliveryError> {
        Ok(self
            .grant(workspace_id, recording_id)
            .await?
            .and_then(|grant| grant.poster_url))
    }
}

#[cfg(test)]
mod tests {
    use platform::S3ObjectStore;
    use sqlx::PgPool;

    use super::*;

    /// Presigning is computed locally, so no request reaches MinIO.
    fn store() -> Arc<dyn ObjectStore> {
        Arc::new(S3ObjectStore::new(
            &std::env::var("S3_ENDPOINT").expect("S3_ENDPOINT set"),
            &std::env::var("S3_BUCKET").expect("S3_BUCKET set"),
            &std::env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY set"),
            &std::env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY set"),
        ))
    }

    async fn seed(pool: &PgPool, with_mp4: bool) -> (WorkspaceId, RecordingId) {
        let user = uuid::Uuid::now_v7();
        let workspace = WorkspaceId::new_v7();
        let recording = RecordingId::new_v7();
        let take = uuid::Uuid::now_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'D')",
            user,
            format!("delivery-{user}@example.com")
        )
        .execute(pool)
        .await
        .expect("user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name) VALUES ($1, 'D')",
            workspace.into_uuid()
        )
        .execute(pool)
        .await
        .expect("workspace");
        sqlx::query!(
            "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
             VALUES ($1, $2, $3, 'D', 'ready')",
            recording.into_uuid(),
            workspace.into_uuid(),
            user
        )
        .execute(pool)
        .await
        .expect("recording");
        sqlx::query!(
            "INSERT INTO takes (id, workspace_id, recording_id, mime_type, has_system_audio,
                                has_mic, has_camera, finalized_at)
             VALUES ($1, $2, $3, 'video/webm', false, true, false, now())",
            take,
            workspace.into_uuid(),
            recording.into_uuid()
        )
        .execute(pool)
        .await
        .expect("take");
        let mut rows = vec![("thumbnail", "poster", "ws/rec/poster.jpg")];
        if with_mp4 {
            rows.push(("mp4", "default", "ws/rec/default.mp4"));
        }
        for (kind, variant, key) in rows {
            sqlx::query!(
                "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
                 VALUES ($1, $2, $3, $4, $5::text::rendition_kind, $6, $7)",
                uuid::Uuid::now_v7(),
                workspace.into_uuid(),
                recording.into_uuid(),
                take,
                kind,
                variant,
                key
            )
            .execute(pool)
            .await
            .expect("rendition");
        }
        (workspace, recording)
    }

    fn service(pool: PgPool) -> DeliveryService {
        DeliveryService::new(store(), Arc::new(RenditionReader::new(pool)))
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_grant_has_signed_urls_that_expire_in_fifteen_minutes(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        let grant = service(pool)
            .grant(workspace, recording)
            .await
            .expect("grant")
            .expect("an MP4 exists");
        assert_eq!(grant.expires_in_s, 900);
        for url in [Some(&grant.mp4_url), grant.poster_url.as_ref()]
            .into_iter()
            .flatten()
        {
            let parsed = url::Url::parse(url).expect("a URL");
            let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
            assert_eq!(query.get("X-Amz-Expires").map(|v| v.as_ref()), Some("900"));
            assert!(query.contains_key("X-Amz-Signature"));
        }
        assert!(grant.mp4_url.contains("default.mp4"));
        assert!(
            grant
                .poster_url
                .is_some_and(|url| url.contains("poster.jpg"))
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_download_url_names_the_file_and_expires(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        let grant = service(pool)
            .download(workspace, recording, "Sprint demo: Q3/Q4")
            .await
            .expect("grant")
            .expect("an MP4 exists");
        assert_eq!(grant.filename, "Sprint demo Q3 Q4.mp4");
        assert_eq!(grant.expires_in_s, 900);
        let parsed = url::Url::parse(&grant.url).expect("a URL");
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(
            query
                .get("response-content-disposition")
                .map(|v| v.as_ref()),
            Some("attachment; filename=\"Sprint demo Q3 Q4.mp4\"")
        );
        assert!(query.contains_key("X-Amz-Signature"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn posters_are_signed_per_recording_and_scoped_to_the_workspace(pool: PgPool) {
        let (workspace, with_poster) = seed(&pool, true).await;
        let (other_workspace, other_recording) = seed(&pool, true).await;
        let delivery = service(pool);
        let urls = delivery
            .posters(workspace, &[with_poster, other_recording])
            .await
            .expect("posters");
        assert_eq!(urls.len(), 1, "another workspace's poster is not signed");
        assert!(urls[&with_poster].contains("poster.jpg"));
        let none = delivery
            .posters(other_workspace, &[with_poster])
            .await
            .expect("posters");
        assert!(none.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn no_mp4_means_no_grant(pool: PgPool) {
        let (workspace, recording) = seed(&pool, false).await;
        let delivery = service(pool);
        assert!(
            delivery
                .grant(workspace, recording)
                .await
                .expect("grant")
                .is_none()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn another_workspace_gets_no_grant(pool: PgPool) {
        let (_, recording) = seed(&pool, true).await;
        let (other_workspace, _) = seed(&pool, true).await;
        let delivery = service(pool);
        assert!(
            delivery
                .grant(other_workspace, recording)
                .await
                .expect("grant")
                .is_none()
        );
    }
}
