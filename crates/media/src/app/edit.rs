//! Edits (docs/design.md §10, `RenderEdit`): submitting an edit queues a render; the render cuts
//! the kept ranges out of the original's MP4, makes the rest of the version from the result under
//! `edits/{version}/`, and then swaps it in for what viewers get (ADR-0036, ADR-0038).

use catalog::Measured;
use kernel::{RecordingId, UserId, WorkspaceId};
use platform::{JobQueue, JobQueueError};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::domain::edl::{Edl, EdlError, EdlJsonError};
use crate::domain::probe::{SourceInfo, validate};
use crate::domain::render::{DURATION_TOLERANCE_MS, render_args};
use crate::domain::transcode::{ffmpeg_deadline, x264_preset};
use crate::domain::{
    Container, edit_base, hls_prefix_in, img_prefix_in, mp4_key, mp4_key_in, poster_key_in,
};
use crate::events::EditApplied;
use crate::infra;
use crate::infra::source::download_file;
use crate::infra::tools::{Probed, ToolError, probe, run_ffmpeg};

use super::hls::HlsError;
use super::service::{MediaService, StoredRendition, record_renditions};
use super::sprite::SpriteError;
use super::transcode::{TranscodeError, make_poster};

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
    /// The edit's renditions replaced the recording's: it is what viewers get now.
    Applied {
        duration_ms: u32,
        bytes: u64,
        /// HLS rungs rebuilt; 0 when the recording had no ladder yet (the next view asks).
        rungs: usize,
    },
    /// A newer edit of the recording was applied first: this one is kept, never shown.
    Superseded,
    /// Already applied, superseded or given up on (a duplicate job).
    AlreadyFinished,
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

    #[error("the poster: {0}")]
    Poster(#[from] TranscodeError),

    #[error(transparent)]
    Sprite(#[from] SpriteError),

    #[error(transparent)]
    Hls(#[from] HlsError),

    #[error(transparent)]
    Storage(#[from] platform::StorageError),

    #[error(transparent)]
    Download(#[from] crate::infra::source::AssembleError),

    #[error(transparent)]
    Outbox(#[from] platform::OutboxError),

    #[error("database: {0}")]
    Db(#[from] sqlx::Error),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

impl MediaService {
    /// `RenderEdit` (ADR-0036, ADR-0038): makes the edit into a version of the recording and,
    /// only once every part of it is stored, makes it the version viewers get. The recording is
    /// `processing` meanwhile and `ready` again after; until the swap viewers keep the old
    /// version. The edit's MP4 is cut from the original's MP4 and checked against the kept
    /// ranges; its poster, sprite, preview and (if the recording has one) HLS ladder are made
    /// from it, all under `edits/{version}/`. A retry reuses an MP4 already stored. A failure on
    /// the job's last attempt marks the edit `failed`, deletes what it stored, and leaves the
    /// recording as it was.
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
        if !matches!(edit.status.as_str(), "queued" | "rendering" | "rendered") {
            return Ok(RenderOutcome::AlreadyFinished);
        }
        if !self.begin_edit(&edit).await? {
            return Ok(RenderOutcome::Superseded);
        }
        let scratch = self
            .scratch
            .create(edit.take_id, &format!("edit-{}", edit.version))
            .await?;
        let result = self.apply_in(&edit, &scratch).await;
        scratch.remove().await?;
        if let Err(error) = &result
            && last_attempt
        {
            self.give_up(&edit, &error.to_string()).await?;
        }
        result
    }

    /// `Ready → Processing` as the edit starts. `false` (and the edit superseded) if a newer edit
    /// of the recording is applied already.
    async fn begin_edit(&self, edit: &infra::EditRow) -> Result<bool, RenderEditError> {
        let mut tx = self.pool.begin().await?;
        infra::lock_versions(&mut tx, edit.recording_id).await?;
        if infra::newer_applied(&mut *tx, edit.workspace_id, edit.recording_id, edit.version)
            .await?
        {
            infra::mark_superseded(&mut *tx, edit.id, edit.workspace_id).await?;
            self.settle(&mut tx, edit).await?;
            tx.commit().await?;
            tracing::info!(
                version = edit.version,
                "a newer edit is applied; superseded"
            );
            return Ok(false);
        }
        self.catalog
            .mark_reprocessing(&mut tx, edit.recording_id, edit.workspace_id)
            .await?;
        if edit.status != "rendered" {
            infra::mark_rendering(&mut *tx, edit.id, edit.workspace_id).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// `Processing → Ready` (measurements as they are), unless another edit of the recording is
    /// still on its way: the last one to finish makes it `ready`. Under the versions lock.
    async fn settle(
        &self,
        conn: &mut PgConnection,
        edit: &infra::EditRow,
    ) -> Result<(), sqlx::Error> {
        if !infra::others_pending(&mut *conn, edit.workspace_id, edit.recording_id, edit.id).await?
        {
            self.catalog
                .restore_ready(conn, edit.recording_id, edit.workspace_id)
                .await?;
        }
        Ok(())
    }

    /// The last attempt failed: the edit is `failed`, what it stored is deleted (nothing points
    /// at it), and the recording keeps the version it had.
    async fn give_up(&self, edit: &infra::EditRow, reason: &str) -> Result<(), RenderEditError> {
        let mut tx = self.pool.begin().await?;
        infra::lock_versions(&mut tx, edit.recording_id).await?;
        infra::mark_failed(&mut *tx, edit.id, edit.workspace_id, reason).await?;
        self.settle(&mut tx, edit).await?;
        tx.commit().await?;
        tracing::warn!(
            version = edit.version,
            "edit given up; the recording keeps its version"
        );
        // The trailing slash keeps version 1 from matching version 10.
        let prefix = format!(
            "{}/",
            edit_base(edit.workspace_id, edit.recording_id, edit.version)
        );
        if let Err(error) = self.store.delete_prefix(&prefix).await {
            tracing::warn!(%error, "could not delete a failed edit's files");
        }
        Ok(())
    }

    async fn apply_in(
        &self,
        edit: &infra::EditRow,
        scratch: &crate::infra::scratch::ScratchDir,
    ) -> Result<RenderOutcome, RenderEditError> {
        let base = edit_base(edit.workspace_id, edit.recording_id, edit.version);
        let mp4 = scratch.file("edit.mp4");
        let (rendered, bytes, stored_key) = self.rendered_mp4(edit, &base, scratch, &mp4).await?;
        let duration_ms = rendered
            .duration_ms
            .ok_or_else(|| RenderEditError::BadOutput("no duration".to_string()))?;

        // What else the version plays with, made from its own MP4 and stored beside it.
        let poster_path = scratch.file("poster.jpg");
        let poster = make_poster(&self.tools, &mp4, &poster_path, duration_ms).await?;
        let poster_key = poster_key_in(&base);
        self.store
            .put_file(&poster_key, &poster_path, "image/jpeg")
            .await?;
        let set = super::sprite::build_sprite(&self.tools, &mp4, &scratch.file("img"), duration_ms)
            .await?;
        let mut rows = self.store_sprite(&img_prefix_in(&base), &set).await?;
        // The ladder is remade only if the recording has one (it is built after the first view);
        // otherwise the next view asks for one, cut from the new MP4.
        let rungs = if infra::has_ladder(
            &self.pool,
            edit.workspace_id,
            edit.recording_id,
            edit.take_id,
        )
        .await?
        {
            let ladder =
                super::hls::build_ladder(&self.tools, &mp4, &scratch.file("hls"), duration_ms)
                    .await?;
            rows.extend(self.store_ladder(&hls_prefix_in(&base), &ladder).await?);
            ladder.rungs.len()
        } else {
            0
        };
        rows.push(StoredRendition {
            kind: "mp4",
            variant: "default".to_string(),
            key: stored_key,
            bytes,
            meta: serde_json::json!({
                "duration_ms": duration_ms,
                "width": rendered.width,
                "height": rendered.height,
                "video_codec": rendered.video_codec,
                "audio_codec": rendered.audio_codec,
                "edit_version": edit.version,
            }),
        });
        rows.push(StoredRendition {
            kind: "thumbnail",
            variant: "poster".to_string(),
            key: poster_key,
            bytes: poster.bytes,
            meta: serde_json::json!({ "width": poster.width, "height": poster.height }),
        });

        // The swap: one transaction, so viewers get the old version or the new one, never a mix.
        let mut tx = self.pool.begin().await?;
        infra::lock_versions(&mut tx, edit.recording_id).await?;
        if infra::newer_applied(&mut *tx, edit.workspace_id, edit.recording_id, edit.version)
            .await?
        {
            infra::mark_superseded(&mut *tx, edit.id, edit.workspace_id).await?;
            self.settle(&mut tx, edit).await?;
            tx.commit().await?;
            tracing::info!(
                version = edit.version,
                "a newer edit was applied first; superseded"
            );
            return Ok(RenderOutcome::Superseded);
        }
        infra::delete_derived_renditions(
            &mut tx,
            edit.workspace_id,
            edit.recording_id,
            edit.take_id,
        )
        .await?;
        record_renditions(
            &mut tx,
            edit.workspace_id,
            edit.recording_id,
            edit.take_id,
            &rows,
        )
        .await?;
        infra::mark_applied(&mut *tx, edit.id, edit.workspace_id).await?;
        let measured = Measured {
            duration_ms: i32::try_from(duration_ms).unwrap_or(i32::MAX),
            width: i32::try_from(rendered.width).unwrap_or(0),
            height: i32::try_from(rendered.height).unwrap_or(0),
            size_bytes: i64::try_from(bytes).unwrap_or(i64::MAX),
        };
        self.catalog
            .remeasure(&mut tx, edit.recording_id, edit.workspace_id, measured)
            .await?;
        self.settle(&mut tx, edit).await?;
        self.outbox
            .push(
                &mut tx,
                &EditApplied {
                    recording_id: edit.recording_id,
                    take_id: edit.take_id,
                    workspace_id: edit.workspace_id,
                    edit_id: edit.id,
                    version: edit.version,
                    duration_ms,
                },
            )
            .await?;
        tx.commit().await?;
        tracing::info!(
            version = edit.version,
            duration_ms,
            bytes,
            rungs,
            "edit applied"
        );
        Ok(RenderOutcome::Applied {
            duration_ms,
            bytes,
            rungs,
        })
    }

    /// The edit's MP4 at `output`, checked, with its size and storage key: the one an earlier
    /// attempt stored, or cut now from the original's MP4 and stored at `{base}/mp4/default.mp4`.
    async fn rendered_mp4(
        &self,
        edit: &infra::EditRow,
        base: &str,
        scratch: &crate::infra::scratch::ScratchDir,
        output: &std::path::Path,
    ) -> Result<(SourceInfo, u64, String), RenderEditError> {
        let edl = Edl::from_json(&edit.edl, edit.source_duration_ms)?;
        let kept_ms = edl.kept_ms();
        if edit.status == "rendered"
            && let Some(key) = &edit.output_key
            && let Some(bytes) = download_file(self.store.as_ref(), key, output).await?
        {
            let rendered = self.check_rendered(output, kept_ms).await?;
            return Ok((rendered, bytes, key.clone()));
        }

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
        run_ffmpeg(
            &self.tools,
            render_args(
                &input,
                output,
                &edl,
                source.audio_codec.is_some(),
                x264_preset(kept_ms),
            ),
            kept_ms,
            ffmpeg_deadline(kept_ms),
        )
        .await?;
        let rendered = self.check_rendered(output, kept_ms).await?;
        let duration_ms = rendered.duration_ms.unwrap_or(kept_ms);

        let out_key = mp4_key_in(base);
        let bytes = self.store.put_file(&out_key, output, "video/mp4").await?;
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
        Ok((rendered, bytes, out_key))
    }

    /// A rendered MP4 that is H.264, readable, and as long as the kept ranges.
    async fn check_rendered(
        &self,
        output: &std::path::Path,
        kept_ms: u32,
    ) -> Result<SourceInfo, RenderEditError> {
        let rendered = match probe(&self.tools, output).await? {
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
        Ok(rendered)
    }
}
