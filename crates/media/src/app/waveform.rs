use std::path::Path;

use kernel::{TakeId, WorkspaceId};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use super::service::MediaService;
use crate::domain::probe::validate;
use crate::domain::transcode::ffmpeg_deadline;
use crate::domain::waveform::{FORMAT_VERSION, PeakFolder, Waveform, bucket_ms, decode_args};
use crate::domain::{Container, mp4_key, waveform_key};
use crate::events::RenditionReady;
use crate::infra;
use crate::infra::scratch::ScratchDir;
use crate::infra::source::download_file;
use crate::infra::tools::{MediaTools, Probed, ToolError, probe, run_ffmpeg};

/// The job kind that makes a recording's waveform peaks for the editor (ADR-0039).
pub const GENERATE_WAVEFORM: &str = "GenerateWaveform";

/// The `GenerateWaveform` job's payload: which take; the peaks come from the recording's
/// original MP4.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GenerateWaveform {
    pub take_id: TakeId,
    pub workspace_id: WorkspaceId,
}

/// How a `GenerateWaveform` run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformOutcome {
    /// `waveform.json` is stored and recorded.
    Built { peaks: usize, has_audio: bool },
    /// No such take in this workspace.
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    /// The MP4 the peaks come from isn't something processing accepts.
    #[error("the MP4 can't be read for its waveform: {0}")]
    BadSource(String),

    /// FFmpeg/ffprobe failed, timed out or stalled (worth retrying).
    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// Why `GenerateWaveform` didn't produce the peaks.
#[derive(Debug, thiserror::Error)]
pub enum GenerateWaveformError {
    /// The original MP4 isn't stored yet (`ProcessTake` hasn't finished). Retrying later fixes
    /// it.
    #[error("the recording's MP4 isn't ready")]
    NotReady,

    #[error(transparent)]
    Waveform(#[from] WaveformError),

    #[error(transparent)]
    Storage(#[from] platform::StorageError),

    #[error(transparent)]
    Download(#[from] crate::infra::source::AssembleError),

    #[error(transparent)]
    Outbox(#[from] platform::OutboxError),

    #[error("the peaks can't be written: {0}")]
    Json(#[from] serde_json::Error),

    #[error("database: {0}")]
    Db(#[from] sqlx::Error),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// The peaks of `mp4`'s audio (ADR-0039): decoded by FFmpeg to raw PCM in `raw`, then folded.
/// A recording without an audio track gets a silent waveform.
pub async fn build_waveform(
    tools: &MediaTools,
    mp4: &Path,
    raw: &Path,
    declared_ms: u32,
) -> Result<Waveform, WaveformError> {
    let source = match probe(tools, mp4).await? {
        Probed::Unreadable { detail } => return Err(WaveformError::BadSource(detail)),
        Probed::Readable(report) => validate(&report, Container::Mp4)
            .map_err(|rejection| WaveformError::BadSource(rejection.to_string()))?,
    };
    // The MP4's own length is the truth; what the client declared is the fallback.
    let duration_ms = source.duration_ms.unwrap_or(declared_ms);
    if source.audio_codec.is_none() {
        return Ok(Waveform::silent(duration_ms));
    }
    run_ffmpeg(
        tools,
        decode_args(mp4, raw),
        duration_ms,
        ffmpeg_deadline(duration_ms),
    )
    .await?;
    let bucket = bucket_ms(duration_ms);
    let mut folder = PeakFolder::new(bucket);
    let mut file = tokio::fs::File::open(raw).await?;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        folder.push_bytes(&buffer[..read]);
    }
    Ok(Waveform {
        version: FORMAT_VERSION,
        duration_ms,
        bucket_ms: bucket,
        has_audio: true,
        peaks: folder.finish(duration_ms, bucket),
    })
}

impl MediaService {
    /// `GenerateWaveform` (ADR-0039): the peaks of the recording's original MP4, stored as
    /// `img/waveform.json` under the original's base and recorded as the `waveform` rendition.
    /// An edit never replaces them: the editor works in the original's time. A rerun replaces
    /// what is stored.
    #[tracing::instrument(skip_all, fields(take_id = %request.take_id, workspace_id = %request.workspace_id))]
    pub async fn generate_waveform(
        &self,
        request: GenerateWaveform,
    ) -> Result<WaveformOutcome, GenerateWaveformError> {
        let Some(job) = infra::find_job(&self.pool, request.take_id, request.workspace_id).await?
        else {
            return Ok(WaveformOutcome::NotFound);
        };
        let scratch = self.scratch.create(job.take_id, "waveform").await?;
        let result = self.generate_waveform_in(&job, &scratch).await;
        scratch.remove().await?;
        result
    }

    async fn generate_waveform_in(
        &self,
        job: &infra::MediaJobRow,
        scratch: &ScratchDir,
    ) -> Result<WaveformOutcome, GenerateWaveformError> {
        let mp4 = scratch.file("default.mp4");
        if download_file(
            self.store.as_ref(),
            &mp4_key(job.workspace_id, job.recording_id),
            &mp4,
        )
        .await?
        .is_none()
        {
            return Err(GenerateWaveformError::NotReady);
        }
        let declared_ms = u32::try_from(job.duration_ms).unwrap_or(0);
        let waveform =
            build_waveform(&self.tools, &mp4, &scratch.file("audio.raw"), declared_ms).await?;
        let json = scratch.file("waveform.json");
        tokio::fs::write(&json, serde_json::to_vec(&waveform)?).await?;
        let key = waveform_key(job.workspace_id, job.recording_id);
        let bytes = self.store.put_file(&key, &json, "application/json").await?;
        tracing::info!(
            peaks = waveform.peaks.len(),
            has_audio = waveform.has_audio,
            "waveform stored"
        );

        let mut tx = self.pool.begin().await?;
        infra::upsert_rendition(
            &mut tx,
            job.workspace_id,
            job.recording_id,
            job.take_id,
            "waveform",
            "peaks",
            &key,
            i64::try_from(bytes).unwrap_or(i64::MAX),
            serde_json::json!({
                "duration_ms": waveform.duration_ms,
                "bucket_ms": waveform.bucket_ms,
                "peaks": waveform.peaks.len(),
                "has_audio": waveform.has_audio,
            }),
        )
        .await?;
        self.outbox
            .push(
                &mut tx,
                &RenditionReady {
                    recording_id: job.recording_id,
                    take_id: job.take_id,
                    workspace_id: job.workspace_id,
                    kind: "waveform".to_string(),
                    variants: Vec::new(),
                },
            )
            .await?;
        tx.commit().await?;
        Ok(WaveformOutcome::Built {
            peaks: waveform.peaks.len(),
            has_audio: waveform.has_audio,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::waveform::peak_count;
    use crate::testing::ffmpeg_available;

    async fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sintade-{name}-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&dir).await.expect("dir");
        dir
    }

    /// A 4 s H.264 MP4: with `tone`, a 440 Hz sine (FFmpeg's, at 1/8 of full scale) for 2 s and
    /// then 2 s of silence; without, no audio track at all.
    async fn mp4(dir: &Path, tone: bool) -> PathBuf {
        let path = dir.join(if tone { "tone.mp4" } else { "mute.mp4" });
        let mut command = tokio::process::Command::new("ffmpeg");
        command.args(["-v", "error", "-y", "-f", "lavfi", "-i"]);
        command.arg("testsrc2=size=320x180:rate=30:duration=4");
        if tone {
            command.args(["-f", "lavfi", "-i"]);
            command.arg("sine=frequency=440:duration=2:sample_rate=48000,apad=whole_dur=4");
            command.args(["-c:a", "aac", "-shortest"]);
        }
        let status = command
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&path)
            .status()
            .await
            .expect("run ffmpeg");
        assert!(status.success());
        path
    }

    /// The peaks follow the sound: loud while the tone plays, silent after, one per 50 ms of
    /// the MP4's length.
    #[tokio::test]
    async fn the_peaks_follow_the_sound() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("waveform").await;
        let source = mp4(&dir, true).await;
        let waveform = build_waveform(&MediaTools::default(), &source, &dir.join("a.raw"), 0)
            .await
            .expect("waveform");
        assert!(waveform.has_audio);
        assert_eq!(waveform.bucket_ms, 50);
        assert!(
            (3_900..=4_200).contains(&waveform.duration_ms),
            "{} ms",
            waveform.duration_ms
        );
        assert_eq!(
            waveform.peaks.len() as u32,
            peak_count(waveform.duration_ms, 50)
        );
        // 1/8 of full scale is 31 of 255; AAC moves it a little.
        for (index, peak) in waveform.peaks[5..35].iter().enumerate() {
            assert!((20..=45).contains(peak), "peak {} is {peak}", index + 5);
        }
        for (index, peak) in waveform.peaks[45..75].iter().enumerate() {
            assert!(*peak <= 2, "peak {} is {peak}", index + 45);
        }
        tokio::fs::remove_dir_all(&dir).await.expect("tidy");
    }

    #[tokio::test]
    async fn a_recording_without_sound_has_a_silent_waveform() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("waveform-mute").await;
        let source = mp4(&dir, false).await;
        let waveform = build_waveform(&MediaTools::default(), &source, &dir.join("a.raw"), 0)
            .await
            .expect("waveform");
        assert!(!waveform.has_audio);
        assert!(waveform.peaks.is_empty());
        assert!(!dir.join("a.raw").exists(), "nothing was decoded");
        tokio::fs::remove_dir_all(&dir).await.expect("tidy");
    }
}
