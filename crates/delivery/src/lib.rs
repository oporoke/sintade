#![deny(clippy::unwrap_used)]

//! Playback grants: short-lived signed URLs for a recording's renditions (docs/design.md §5).
//! Stateless; the caller has already decided the viewer may watch (`sharing::decide`).

mod filename;
mod hls;
mod manifest_token;

use std::sync::Arc;
use std::time::Duration;

use kernel::{RecordingId, WorkspaceId};
use media::RenditionReader;
use platform::{Clock, ObjectStore, StorageError};

pub use filename::download_filename;
pub use manifest_token::{ManifestSigner, TokenError};

/// How long a grant's URLs work (docs/design.md §16: presigned URLs ≤ 15 minutes).
pub const GRANT_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Which file a grant points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackKind {
    /// The fast-start MP4: seekable, plays everywhere.
    Mp4,
    /// The original recording, while the MP4 is still being made: plays at once in browsers
    /// that support its format, but isn't indexed for seeking.
    Preview,
}

/// What a viewer's player is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackGrant {
    pub kind: PlaybackKind,
    pub url: String,
    /// The MIME type of `url` (`video/mp4`, or the original's: `video/webm`).
    pub content_type: String,
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

/// What asking for a rung playlist can give.
#[derive(Debug, PartialEq, Eq)]
pub enum RungPlaylist {
    /// No such rung, or no ladder.
    NotFound,
    /// The token is missing, forged, for another recording or expired.
    Refused(TokenError),
    Playlist(String),
}

pub struct DeliveryService {
    store: Arc<dyn ObjectStore>,
    renditions: Arc<RenditionReader>,
    clock: Arc<dyn Clock>,
    signer: ManifestSigner,
}

impl DeliveryService {
    /// `manifest_key` signs the playlist tokens (derive it from the session secret, don't use the
    /// secret itself).
    pub fn new(
        store: Arc<dyn ObjectStore>,
        renditions: Arc<RenditionReader>,
        clock: Arc<dyn Clock>,
        manifest_key: Vec<u8>,
    ) -> Self {
        Self {
            store,
            renditions,
            clock,
            signer: ManifestSigner::new(manifest_key),
        }
    }

    /// Signed URLs for the recording: its MP4 when it exists, otherwise (if `allow_preview`) the
    /// original, so a viewer can watch seconds after the recording stops. `None` when there is
    /// nothing playable.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn grant(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        allow_preview: bool,
    ) -> Result<Option<PlaybackGrant>, DeliveryError> {
        let Some(keys) = self
            .renditions
            .playback_keys(workspace_id, recording_id)
            .await?
        else {
            return Ok(None);
        };
        let (kind, key, content_type) = match (&keys.mp4, &keys.source) {
            (Some(mp4), _) => (PlaybackKind::Mp4, mp4.clone(), "video/mp4".to_string()),
            (None, Some(source)) if allow_preview => (
                PlaybackKind::Preview,
                source.key.clone(),
                source.content_type.clone(),
            ),
            _ => return Ok(None),
        };
        if kind == PlaybackKind::Mp4 {
            // The first view asks for the adaptive ladder. A failure here must not stop the
            // viewer watching the MP4.
            if let Err(error) = self
                .renditions
                .request_ladder(workspace_id, recording_id)
                .await
            {
                tracing::warn!(%error, "playback: could not request the HLS ladder");
            }
        }
        let url = self.store.presign_get(&key, GRANT_TTL).await?.to_string();
        let poster_url = match keys.poster {
            Some(key) => Some(self.store.presign_get(&key, GRANT_TTL).await?.to_string()),
            None => None,
        };
        Ok(Some(PlaybackGrant {
            kind,
            url,
            content_type,
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
        let Some(mp4) = self
            .renditions
            .playback_keys(workspace_id, recording_id)
            .await?
            .and_then(|keys| keys.mp4)
        else {
            return Ok(None);
        };
        let filename = download_filename(title);
        let url = self
            .store
            .presign_download(&mp4, GRANT_TTL, &filename)
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

    /// Whether the recording's HLS ladder exists yet (it is built after the first view).
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn has_ladder(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<bool, DeliveryError> {
        Ok(self
            .renditions
            .hls_prefix(workspace_id, recording_id)
            .await?
            .is_some())
    }

    /// Whether the recording's scrub sprite exists yet.
    pub async fn has_sprite(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<bool, DeliveryError> {
        Ok(self
            .renditions
            .sprite_vtt_key(workspace_id, recording_id)
            .await?
            .is_some())
    }

    /// The master playlist with a token (valid [`GRANT_TTL`]) on each rung it names. `None`
    /// before the ladder exists.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn hls_master(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<String>, DeliveryError> {
        let Some(prefix) = self
            .renditions
            .hls_prefix(workspace_id, recording_id)
            .await?
        else {
            return Ok(None);
        };
        let Some(master) = self.read_playlist(&format!("{prefix}/master.m3u8")).await? else {
            return Ok(None);
        };
        let expires_at = self.clock.now().unix_timestamp() + GRANT_TTL.as_secs() as i64;
        let token = self.signer.issue(recording_id, expires_at);
        Ok(Some(hls::tokenize_master(&master, &token)))
    }

    /// One rung's playlist with every segment (and the init segment) signed, but only until
    /// `token` expires, so no segment URL outlives the master fetch that started the session by
    /// more than [`GRANT_TTL`].
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn hls_rung(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        rung: &str,
        token: Option<&str>,
    ) -> Result<RungPlaylist, DeliveryError> {
        if !hls::RUNGS.contains(&rung) {
            return Ok(RungPlaylist::NotFound);
        }
        let now = self.clock.now().unix_timestamp();
        let expires_at = match token
            .ok_or(TokenError::Invalid)
            .and_then(|token| self.signer.verify(token, recording_id, now))
        {
            Ok(expires_at) => expires_at,
            Err(error) => return Ok(RungPlaylist::Refused(error)),
        };
        let ttl = Duration::from_secs(
            u64::try_from(expires_at - now)
                .unwrap_or(1)
                .clamp(1, GRANT_TTL.as_secs()),
        );
        let Some(prefix) = self
            .renditions
            .hls_prefix(workspace_id, recording_id)
            .await?
        else {
            return Ok(RungPlaylist::NotFound);
        };
        let Some(playlist) = self
            .read_playlist(&format!("{prefix}/{rung}/index.m3u8"))
            .await?
        else {
            return Ok(RungPlaylist::NotFound);
        };
        let Some(files) = hls::referenced_files(&playlist) else {
            tracing::error!("hls: a stored playlist references a path");
            return Ok(RungPlaylist::NotFound);
        };
        let mut urls = std::collections::HashMap::new();
        for file in files {
            if let std::collections::hash_map::Entry::Vacant(entry) = urls.entry(file) {
                let key = format!("{prefix}/{rung}/{}", entry.key());
                let url = self.store.presign_get(&key, ttl).await?.to_string();
                entry.insert(url);
            }
        }
        Ok(RungPlaylist::Playlist(hls::rewrite(&playlist, &urls)))
    }

    /// The scrub sprite's `sprite.vtt` with each sheet signed for 15 minutes; `None` until the
    /// sprite exists.
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn sprite_vtt(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<String>, DeliveryError> {
        let Some(key) = self
            .renditions
            .sprite_vtt_key(workspace_id, recording_id)
            .await?
        else {
            return Ok(None);
        };
        let Some(vtt) = self.read_playlist(&key).await? else {
            return Ok(None);
        };
        let Some(files) = hls::sprite_files(&vtt) else {
            tracing::error!("sprite: a stored vtt references a path");
            return Ok(None);
        };
        let prefix = key.rsplit_once('/').map_or("", |(prefix, _)| prefix);
        let mut urls = std::collections::HashMap::new();
        for file in files {
            if let std::collections::hash_map::Entry::Vacant(entry) = urls.entry(file) {
                let url = self
                    .store
                    .presign_get(&format!("{prefix}/{}", entry.key()), GRANT_TTL)
                    .await?
                    .to_string();
                entry.insert(url);
            }
        }
        Ok(Some(hls::rewrite_sprite(&vtt, &urls)))
    }

    /// A playlist is small; anything past 1 MiB is not one.
    async fn read_playlist(&self, key: &str) -> Result<Option<String>, DeliveryError> {
        use tokio::io::AsyncReadExt;
        let Some(reader) = self.store.get(key).await? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        reader
            .take(1024 * 1024)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| StorageError::Request(error.to_string()))?;
        Ok(String::from_utf8(bytes).ok())
    }

    /// Just the poster URL (a watch page for a recording that is still processing has none yet).
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn poster(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
    ) -> Result<Option<String>, DeliveryError> {
        Ok(self
            .grant(workspace_id, recording_id, true)
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
        DeliveryService::new(
            store(),
            Arc::new(RenditionReader::new(pool)),
            Arc::new(platform::SystemClock),
            b"test-manifest-key-test-manifest!".to_vec(),
        )
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_grant_has_signed_urls_that_expire_in_fifteen_minutes(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        let grant = service(pool)
            .grant(workspace, recording, false)
            .await
            .expect("grant")
            .expect("an MP4 exists");
        assert_eq!(grant.kind, PlaybackKind::Mp4);
        assert_eq!(grant.content_type, "video/mp4");
        assert_eq!(grant.expires_in_s, 900);
        for url in [Some(&grant.url), grant.poster_url.as_ref()]
            .into_iter()
            .flatten()
        {
            let parsed = url::Url::parse(url).expect("a URL");
            let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
            assert_eq!(query.get("X-Amz-Expires").map(|v| v.as_ref()), Some("900"));
            assert!(query.contains_key("X-Amz-Signature"));
        }
        assert!(grant.url.contains("default.mp4"));
        assert!(
            grant
                .poster_url
                .is_some_and(|url| url.contains("poster.jpg"))
        );
    }

    /// Marks the seeded recording's take as processed, as `ProcessTake` would.
    async fn mark_processed(pool: &PgPool, workspace: WorkspaceId, recording: RecordingId) {
        sqlx::query!(
            "INSERT INTO media_jobs (take_id, workspace_id, recording_id, mime_type, duration_ms,
                                    chunks, state)
             SELECT id, workspace_id, recording_id, 'video/webm', 1000, '[]', 'done'
             FROM takes WHERE recording_id = $1 AND workspace_id = $2",
            recording.into_uuid(),
            workspace.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("media job");
    }

    async fn ladder_jobs(pool: &PgPool) -> i64 {
        sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM jobs WHERE kind = 'BuildHls'"#)
            .fetch_one(pool)
            .await
            .expect("jobs")
    }

    /// Day 80's Check: an unviewed recording has no ladder job (so no HLS objects); the first
    /// view asks for one, and many views ask once.
    #[sqlx::test(migrations = "../../migrations")]
    async fn the_first_view_asks_for_the_ladder_once(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        mark_processed(&pool, workspace, recording).await;
        assert_eq!(ladder_jobs(&pool).await, 0, "unviewed: nothing queued");

        let delivery = service(pool.clone());
        for _ in 0..3 {
            delivery
                .grant(workspace, recording, false)
                .await
                .expect("grant")
                .expect("an MP4 exists");
        }
        assert_eq!(ladder_jobs(&pool).await, 1);
        let payload = sqlx::query_scalar!(r#"SELECT payload FROM jobs WHERE kind = 'BuildHls'"#)
            .fetch_one(&pool)
            .await
            .expect("payload");
        assert_eq!(payload["workspace_id"], workspace.to_string());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_recording_with_a_ladder_or_without_an_mp4_asks_for_nothing(pool: PgPool) {
        // Has a ladder already.
        let (workspace, built) = seed(&pool, true).await;
        mark_processed(&pool, workspace, built).await;
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
             SELECT $1, workspace_id, recording_id, id, 'hls', 'master', 'hls/master.m3u8'
             FROM takes WHERE recording_id = $2",
            uuid::Uuid::now_v7(),
            built.into_uuid(),
        )
        .execute(&pool)
        .await
        .expect("ladder");
        // Still processing: no MP4, so nothing is played, nothing asked for.
        let (other, processing) = seed(&pool, false).await;
        mark_processed(&pool, other, processing).await;

        let delivery = service(pool.clone());
        delivery
            .grant(workspace, built, false)
            .await
            .expect("grant");
        delivery
            .grant(other, processing, false)
            .await
            .expect("grant");
        assert_eq!(ladder_jobs(&pool).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_view_never_asks_for_another_workspaces_ladder(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        mark_processed(&pool, workspace, recording).await;
        let delivery = service(pool.clone());
        let elsewhere = WorkspaceId::new_v7();
        assert!(
            delivery
                .grant(elsewhere, recording, false)
                .await
                .expect("grant")
                .is_none()
        );
        assert_eq!(ladder_jobs(&pool).await, 0);
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
    async fn no_mp4_and_no_source_means_no_grant(pool: PgPool) {
        let (workspace, recording) = seed(&pool, false).await;
        let delivery = service(pool);
        for allow_preview in [false, true] {
            assert!(
                delivery
                    .grant(workspace, recording, allow_preview)
                    .await
                    .expect("grant")
                    .is_none()
            );
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_source_plays_while_the_mp4_is_still_being_made(pool: PgPool) {
        let (workspace, recording) = seed(&pool, false).await;
        let take = sqlx::query_scalar!(
            "SELECT id FROM takes WHERE recording_id = $1",
            recording.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("take");
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key, meta)
             VALUES ($1, $2, $3, $4, 'source', 'default', 'ws/rec/takes/t/source.webm',
                     '{\"content_type\": \"video/webm\"}')",
            uuid::Uuid::now_v7(),
            workspace.into_uuid(),
            recording.into_uuid(),
            take
        )
        .execute(&pool)
        .await
        .expect("source");
        let delivery = service(pool);

        // Only when the caller allows a preview…
        assert!(
            delivery
                .grant(workspace, recording, false)
                .await
                .expect("grant")
                .is_none()
        );
        let grant = delivery
            .grant(workspace, recording, true)
            .await
            .expect("grant")
            .expect("the source is playable");
        assert_eq!(grant.kind, PlaybackKind::Preview);
        assert_eq!(grant.content_type, "video/webm");
        assert!(grant.url.contains("source.webm"));
        assert!(grant.poster_url.is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_mp4_wins_once_it_exists(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        let grant = service(pool)
            .grant(workspace, recording, true)
            .await
            .expect("grant")
            .expect("grant");
        assert_eq!(grant.kind, PlaybackKind::Mp4);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn another_workspace_gets_no_grant(pool: PgPool) {
        let (_, recording) = seed(&pool, true).await;
        let (other_workspace, _) = seed(&pool, true).await;
        let delivery = service(pool);
        assert!(
            delivery
                .grant(other_workspace, recording, true)
                .await
                .expect("grant")
                .is_none()
        );
    }

    async fn put(store: &Arc<dyn ObjectStore>, key: &str, bytes: &[u8]) {
        let dir = std::env::temp_dir().join(format!("delivery-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let file = dir.join("f");
        std::fs::write(&file, bytes).expect("file");
        store
            .put_file(key, &file, "application/octet-stream")
            .await
            .expect("put");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Day 86 Check: "segment URLs expire; no hotlinking". Against the real bucket: a rung
    /// playlist is only served with a genuine, unexpired token; its segments are signed only until
    /// that token expires; and the bare object URL (the hotlink) is refused.
    #[sqlx::test(migrations = "../../migrations")]
    async fn segment_urls_expire_with_the_token_and_the_bare_url_is_refused(pool: PgPool) {
        let (workspace, recording) = seed(&pool, true).await;
        let store = store();
        let prefix = format!("test/{workspace}/rec/{recording}/hls");
        put(
            &store,
            &format!("{prefix}/master.m3u8"),
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n720p/index.m3u8\n",
        )
        .await;
        put(
            &store,
            &format!("{prefix}/720p/index.m3u8"),
            b"#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\nseg_0000.m4s\n#EXT-X-ENDLIST\n",
        )
        .await;
        put(&store, &format!("{prefix}/720p/init.mp4"), b"init").await;
        put(&store, &format!("{prefix}/720p/seg_0000.m4s"), b"segment").await;
        let take = sqlx::query_scalar!(
            "SELECT id FROM takes WHERE recording_id = $1",
            recording.into_uuid()
        )
        .fetch_one(&pool)
        .await
        .expect("take");
        sqlx::query!(
            "INSERT INTO renditions (id, workspace_id, recording_id, take_id, kind, variant, storage_key)
             VALUES ($1, $2, $3, $4, 'hls', 'master', $5)",
            uuid::Uuid::now_v7(),
            workspace.into_uuid(),
            recording.into_uuid(),
            take,
            format!("{prefix}/master.m3u8"),
        )
        .execute(&pool)
        .await
        .expect("master rendition");
        let service = DeliveryService::new(
            store,
            Arc::new(RenditionReader::new(pool)),
            Arc::new(platform::SystemClock),
            b"test-manifest-key-test-manifest!".to_vec(),
        );

        // The master hands out a token; without one, or with a forged one, there is no rung.
        let master = service
            .hls_master(workspace, recording)
            .await
            .expect("master")
            .expect("ladder");
        assert!(master.contains("720p/index.m3u8?t="), "{master}");
        for token in [None, Some("junk")] {
            assert_eq!(
                service
                    .hls_rung(workspace, recording, "720p", token)
                    .await
                    .expect("rung"),
                RungPlaylist::Refused(TokenError::Invalid)
            );
        }

        // A token with two seconds left signs segments for two seconds.
        let now = platform::SystemClock.now().unix_timestamp();
        let short = service.signer.issue(recording, now + 2);
        let RungPlaylist::Playlist(playlist) = service
            .hls_rung(workspace, recording, "720p", Some(&short))
            .await
            .expect("rung")
        else {
            panic!("a genuine token gets the playlist");
        };
        let segment = playlist
            .lines()
            .find(|line| line.contains("seg_0000.m4s"))
            .expect("segment line")
            .to_string();
        let http = reqwest::Client::new();
        let fetched = http.get(&segment).send().await.expect("get");
        assert_eq!(fetched.status().as_u16(), 200);
        assert_eq!(fetched.bytes().await.expect("body").as_ref(), b"segment");

        // The bare object URL, the way a hotlink would name it, is refused.
        let bare = segment.split('?').next().expect("url").to_string();
        assert_eq!(
            http.get(&bare).send().await.expect("get").status().as_u16(),
            403
        );
        // Tampering with the signed URL (another object, same signature) is refused too.
        let other = segment.replace("seg_0000.m4s", "init.mp4");
        assert_eq!(
            http.get(&other)
                .send()
                .await
                .expect("get")
                .status()
                .as_u16(),
            403
        );

        // Once the token has expired, so has every URL it signed, and the token itself.
        tokio::time::sleep(Duration::from_secs(4)).await;
        assert_eq!(
            http.get(&segment)
                .send()
                .await
                .expect("get")
                .status()
                .as_u16(),
            403
        );
        assert_eq!(
            service
                .hls_rung(workspace, recording, "720p", Some(&short))
                .await
                .expect("rung"),
            RungPlaylist::Refused(TokenError::Expired)
        );
    }
}
