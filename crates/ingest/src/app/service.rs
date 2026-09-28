use std::sync::Arc;

use catalog::{CatalogService, NewRecording, Title, TitleError};
use kernel::{RecordingId, TakeId, UserId, WorkspaceId};
use sqlx::PgPool;

use crate::domain::{MimeType, MimeTypeError, Sources};
use crate::infra;

/// `POST /recordings`: who is recording, where, and what the take will contain. The caller has
/// already checked the owner may create recordings in the workspace.
#[derive(Debug, Clone)]
pub struct StartRecording {
    pub workspace_id: WorkspaceId,
    pub owner_id: UserId,
    pub title: Option<String>,
    pub mime_type: String,
    pub sources: Sources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartedRecording {
    pub recording_id: RecordingId,
    pub take_id: TakeId,
}

#[derive(Debug, thiserror::Error)]
pub enum StartRecordingError {
    #[error(transparent)]
    InvalidTitle(#[from] TitleError),

    #[error(transparent)]
    InvalidMimeType(#[from] MimeTypeError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct IngestService {
    pool: PgPool,
    catalog: Arc<CatalogService>,
}

impl IngestService {
    pub fn new(pool: PgPool, catalog: Arc<CatalogService>) -> Self {
        Self { pool, catalog }
    }

    /// Creates a recording (via `catalog`) and its first take in one transaction, both in the
    /// caller's workspace. The recording starts in state `recording` with `current_take` set.
    #[tracing::instrument(skip_all, fields(workspace_id = %request.workspace_id, user_id = %request.owner_id))]
    pub async fn start_recording(
        &self,
        request: StartRecording,
    ) -> Result<StartedRecording, StartRecordingError> {
        let title = Title::parse(request.title.as_deref())?;
        let mime_type = MimeType::parse(&request.mime_type)?;
        let started = StartedRecording {
            recording_id: RecordingId::new_v7(),
            take_id: TakeId::new_v7(),
        };

        let mut tx = self.pool.begin().await?;
        self.catalog
            .create_recording(
                &mut tx,
                &NewRecording {
                    id: started.recording_id,
                    workspace_id: request.workspace_id,
                    owner_id: request.owner_id,
                    title,
                    current_take: started.take_id,
                },
            )
            .await?;
        infra::insert_take(
            &mut tx,
            started.take_id,
            request.workspace_id,
            started.recording_id,
            &mime_type,
            request.sources,
        )
        .await?;
        tx.commit().await?;

        tracing::info!(recording_id = %started.recording_id, take_id = %started.take_id, "recording started");
        Ok(started)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user and their workspace, inserted directly: ingest tests only need the foreign keys.
    async fn owner_and_workspace(pool: &PgPool) -> (UserId, WorkspaceId) {
        let user_id = UserId::new_v7();
        let workspace_id = WorkspaceId::new_v7();
        sqlx::query!(
            "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Ingest Tester')",
            user_id.into_uuid(),
            format!("ingest-{user_id}@example.com"),
        )
        .execute(pool)
        .await
        .expect("insert user");
        sqlx::query!(
            "INSERT INTO workspaces (id, name, is_personal) VALUES ($1, 'Ingest', true)",
            workspace_id.into_uuid(),
        )
        .execute(pool)
        .await
        .expect("insert workspace");
        (user_id, workspace_id)
    }

    fn request(owner_id: UserId, workspace_id: WorkspaceId) -> StartRecording {
        StartRecording {
            workspace_id,
            owner_id,
            title: None,
            mime_type: "video/webm;codecs=vp9,opus".to_string(),
            sources: Sources {
                system_audio: false,
                mic: true,
                camera: false,
            },
        }
    }

    fn service(pool: &PgPool) -> IngestService {
        IngestService::new(pool.clone(), Arc::new(CatalogService::new()))
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn creates_the_recording_and_its_take_in_the_callers_workspace(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let started = service(&pool)
            .start_recording(request(owner, workspace))
            .await
            .expect("start");

        let recording = sqlx::query!(
            r#"SELECT workspace_id, owner_id, title, state::text AS "state!", current_take
               FROM recordings WHERE id = $1"#,
            started.recording_id.into_uuid(),
        )
        .fetch_one(&pool)
        .await
        .expect("recording row");
        assert_eq!(recording.workspace_id, workspace.into_uuid());
        assert_eq!(recording.owner_id, owner.into_uuid());
        assert_eq!(recording.title, "Untitled recording");
        assert_eq!(recording.state, "recording");
        assert_eq!(recording.current_take, Some(started.take_id.into_uuid()));

        let take = sqlx::query!(
            "SELECT workspace_id, recording_id, mime_type, has_system_audio, has_mic, has_camera, finalized_at
             FROM takes WHERE id = $1",
            started.take_id.into_uuid(),
        )
        .fetch_one(&pool)
        .await
        .expect("take row");
        assert_eq!(take.workspace_id, workspace.into_uuid());
        assert_eq!(take.recording_id, started.recording_id.into_uuid());
        assert_eq!(take.mime_type, "video/webm;codecs=vp9,opus");
        assert!(!take.has_system_audio && take.has_mic && !take.has_camera);
        assert!(take.finalized_at.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn invalid_input_writes_nothing(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        let ingest = service(&pool);

        let mut bad_mime = request(owner, workspace);
        bad_mime.mime_type = "audio/ogg".to_string();
        assert!(matches!(
            ingest.start_recording(bad_mime).await,
            Err(StartRecordingError::InvalidMimeType(
                MimeTypeError::Unsupported
            ))
        ));

        let mut bad_title = request(owner, workspace);
        bad_title.title = Some("x".repeat(catalog::MAX_TITLE_CHARS + 1));
        assert!(matches!(
            ingest.start_recording(bad_title).await,
            Err(StartRecordingError::InvalidTitle(TitleError::TooLong))
        ));

        let rows = sqlx::query_scalar!(
            r#"SELECT (SELECT count(*) FROM recordings) + (SELECT count(*) FROM takes) AS "n!""#
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(rows, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_failed_take_insert_leaves_no_recording_behind(pool: PgPool) {
        let (owner, workspace) = owner_and_workspace(&pool).await;
        // Make the second statement of the transaction fail, after the recording insert.
        sqlx::query!("ALTER TABLE takes ADD CONSTRAINT fail_every_take CHECK (false) NOT VALID")
            .execute(&pool)
            .await
            .expect("add failing constraint");

        let result = service(&pool)
            .start_recording(request(owner, workspace))
            .await;
        assert!(
            matches!(result, Err(StartRecordingError::Database(_))),
            "{result:?}"
        );

        let recordings = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM recordings"#)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(recordings, 0, "the recording must roll back with its take");
    }
}
