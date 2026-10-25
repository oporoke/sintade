//! Edits (docs/design.md §10, `RenderEdit`): submitting an edit queues a render; the render cuts
//! the kept ranges out of the recording's MP4 and stores the result under `edits/{version}/`.
//! Replacing what viewers watch with an edit is a later step (ADR-0036).

use kernel::{RecordingId, UserId, WorkspaceId};
use platform::{JobQueue, JobQueueError};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::edl::{Edl, EdlError, EdlJsonError};
use crate::domain::probe::validate;
use crate::domain::render::{DURATION_TOLERANCE_MS, edit_mp4_key, render_args};
use crate::domain::transcode::{ffmpeg_deadline, x264_preset};
use crate::domain::{Container, mp4_key};
use crate::infra;
use crate::infra::source::download_file;
use crate::infra::tools::{Probed, ToolError, probe, run_ffmpeg};

use super::service::MediaService;

/// The job kind that renders a saved edit.
pub const RENDER_EDIT: &str = "RenderEdit";

/// The `RenderEdit` job's payload: which edit; everything else is on its row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RenderEdit {
    pub edit_id: Uuid,
    pub workspace_id: WorkspaceId,
}

/// A saved edit, as the API tells the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmittedEdit {
    pub edit_id: Uuid,
    pub version: u32,
    /// How long the edited recording will be.
    pub kept_ms: u32,
    pub source_duration_ms: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SubmitEditError {
    /// No such recording in the workspace (also what another workspace's recording looks like).
    #[error("recording not found")]
    NotFound,

    /// The recording has no MP4 yet (still processing, or failed): there is nothing to cut.
    #[error("the recording isn't ready to edit")]
    NotReady,

    #[error(transparent)]
    Invalid(#[from] EdlError),

    /// The ranges keep the whole recording.
    #[error("the edit keeps everything, so it changes nothing")]
    NoChange,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Queue(#[from] JobQueueError),

    #[error("could not encode the job: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Saving edits. Like [`super::RetryService`] it only records and queues: the API needs no
/// ffmpeg, scratch space or storage for it.
pub struct EditService {
    pool: PgPool,
}

impl EditService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Validates `ranges` against the recording's MP4, saves them as the next version and queues
    /// `RenderEdit`, in one transaction (two edits at once get different versions).
    #[tracing::instrument(skip_all, fields(recording_id = %recording_id, workspace_id = %workspace_id))]
    pub async fn submit(
        &self,
        workspace_id: WorkspaceId,
        recording_id: RecordingId,
        user_id: UserId,
        ranges: &[[u32; 2]],
    ) -> Result<SubmittedEdit, SubmitEditError> {
        let mut tx = self.pool.begin().await?;
        if !infra::lock_recording(&mut tx, workspace_id, recording_id).await? {
            return Err(SubmitEditError::NotFound);
        }
        let Some(source) = infra::source_mp4(&mut tx, workspace_id, recording_id).await? else {
            return Err(SubmitEditError::NotReady);
        };
        let edl = Edl::new(ranges, source.duration_ms)?;
        if edl.is_identity() {
            return Err(SubmitEditError::NoChange);
        }
        let version = infra::next_version(&mut tx, workspace_id, recording_id).await?;
        let edit_id = Uuid::now_v7();
        infra::insert_edit(
            &mut tx,
            edit_id,
            workspace_id,
            recording_id,
            source.take_id,
            version,
            &edl.to_json(),
            source.duration_ms,
            user_id,
        )
        .await?;
        let payload = serde_json::to_value(RenderEdit {
            edit_id,
            workspace_id,
        })?;
        JobQueue::enqueue_in(&mut tx, RENDER_EDIT, payload).await?;
        tx.commit().await?;
        Ok(SubmittedEdit {
            edit_id,
            version,
            kept_ms: edl.kept_ms(),
            source_duration_ms: source.duration_ms,
        })
    }
}

/// How a `RenderEdit` run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOutcome {
    Rendered {
        duration_ms: u32,
        bytes: u64,
    },
    /// Already rendered (a duplicate job).
    AlreadyRendered,
    /// No such edit in this workspace (the recording was deleted meanwhile).
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderEditError {
    /// The recording's MP4 isn't stored. Retrying later may fix it.
    #[error("the recording's MP4 isn't stored")]
    NotReady,

    #[error("the stored edit can't be read: {0}")]
    StoredEdit(#[from] EdlJsonError),

    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error("the MP4 can't be read: {0}")]
    BadSource(String),

    /// What FFmpeg wrote doesn't check out.
    #[error("the rendered MP4 doesn't check out: {0}")]
    BadOutput(String),

    #[error(transparent)]
    Storage(#[from] platform::StorageError),

    #[error(transparent)]
    Download(#[from] crate::infra::source::AssembleError),

    #[error("database: {0}")]
    Db(#[from] sqlx::Error),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

impl MediaService {
    /// `RenderEdit`: cuts the edit's kept ranges out of the recording's MP4 and joins them into
    /// `edits/{version}/default.mp4`. The rendered file's length is checked against the ranges.
    /// A failure on the job's last attempt marks the edit `failed`.
    #[tracing::instrument(skip_all, fields(edit_id = %request.edit_id, workspace_id = %request.workspace_id))]
    pub async fn render_edit(
        &self,
        request: RenderEdit,
        last_attempt: bool,
    ) -> Result<RenderOutcome, RenderEditError> {
        let Some(edit) =
            infra::find_edit(&self.pool, request.edit_id, request.workspace_id).await?
        else {
            return Ok(RenderOutcome::NotFound);
        };
        if !infra::mark_rendering(&self.pool, edit.id, edit.workspace_id).await? {
            return Ok(RenderOutcome::AlreadyRendered);
        }
        let scratch = self.scratch.create(edit.take_id).await?;
        let result = self.render_in(&edit, &scratch).await;
        scratch.remove().await?;
        if let Err(error) = &result
            && last_attempt
        {
            infra::mark_failed(&self.pool, edit.id, edit.workspace_id, &error.to_string()).await?;
        }
        result
    }

    async fn render_in(
        &self,
        edit: &infra::EditRow,
        scratch: &crate::infra::scratch::ScratchDir,
    ) -> Result<RenderOutcome, RenderEditError> {
        let edl = Edl::from_json(&edit.edl, edit.source_duration_ms)?;
        let input = scratch.file("default.mp4");
        let key = mp4_key(edit.workspace_id, edit.recording_id);
        if download_file(self.store.as_ref(), &key, &input)
            .await?
            .is_none()
        {
            return Err(RenderEditError::NotReady);
        }
        let source = match probe(&self.tools, &input).await? {
            Probed::Readable(report) => validate(&report, Container::Mp4)
                .map_err(|rejection| RenderEditError::BadSource(rejection.to_string()))?,
            Probed::Unreadable { detail } => return Err(RenderEditError::BadSource(detail)),
        };
        let kept_ms = edl.kept_ms();
        let output = scratch.file("edit.mp4");
        run_ffmpeg(
            &self.tools,
            render_args(
                &input,
                &output,
                &edl,
                source.audio_codec.is_some(),
                x264_preset(kept_ms),
            ),
            kept_ms,
            ffmpeg_deadline(kept_ms),
        )
        .await?;

        let rendered = match probe(&self.tools, &output).await? {
            Probed::Readable(report) => validate(&report, Container::Mp4)
                .map_err(|rejection| RenderEditError::BadOutput(rejection.to_string()))?,
            Probed::Unreadable { detail } => return Err(RenderEditError::BadOutput(detail)),
        };
        let duration_ms = rendered
            .duration_ms
            .ok_or_else(|| RenderEditError::BadOutput("no duration".to_string()))?;
        if duration_ms.abs_diff(kept_ms) > DURATION_TOLERANCE_MS {
            return Err(RenderEditError::BadOutput(format!(
                "{duration_ms} ms rendered for {kept_ms} ms kept"
            )));
        }
        if rendered.video_codec != "h264" {
            return Err(RenderEditError::BadOutput(rendered.video_codec));
        }

        let out_key = edit_mp4_key(edit.workspace_id, edit.recording_id, edit.version);
        let bytes = self.store.put_file(&out_key, &output, "video/mp4").await?;
        infra::mark_rendered(
            &self.pool,
            edit.id,
            edit.workspace_id,
            &out_key,
            duration_ms,
            i64::try_from(bytes).unwrap_or(i64::MAX),
        )
        .await?;
        tracing::info!(
            version = edit.version,
            duration_ms,
            kept_ms,
            bytes,
            "edit rendered"
        );
        Ok(RenderOutcome::Rendered { duration_ms, bytes })
    }
}
