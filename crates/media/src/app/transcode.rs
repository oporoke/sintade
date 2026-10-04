use std::path::Path;

use crate::domain::Container;
use crate::domain::probe::{ProbeRejection, SourceInfo, validate};
use crate::domain::transcode::{Mp4Plan, ffmpeg_deadline, mp4_args, poster_args};
use crate::infra::tools::{MediaTools, Probed, ToolError, probe, run, run_ffmpeg};

/// What making the MP4 produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodeReport {
    pub source: SourceInfo,
    pub plan: Mp4Plan,
    /// The MP4 as ffprobe reads it back.
    pub mp4: SourceInfo,
    pub mp4_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    /// The source isn't something processing accepts (permanent).
    #[error(transparent)]
    Rejected(#[from] ProbeRejection),

    /// FFmpeg/ffprobe failed, timed out or stalled (worth retrying).
    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error("the MP4 FFmpeg wrote doesn't check out: {0}")]
    BadOutput(String),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// §10 Process steps 3–4 on local files: validate `input` with ffprobe, then write the
/// fast-start MP4 to `output` (a remux for Safari's H.264/AAC, otherwise a transcode capped at
/// `max_height`), and check the result reads back as H.264 with the right size.
/// `expected_ms` is the recording's length, for progress and the deadline.
pub async fn transcode_file(
    tools: &MediaTools,
    input: &Path,
    container: Container,
    output: &Path,
    max_height: u32,
    expected_ms: u32,
) -> Result<TranscodeReport, TranscodeError> {
    let source = match probe(tools, input).await? {
        Probed::Unreadable { detail } => {
            tracing::warn!(%detail, "ffprobe can't read the source");
            return Err(ProbeRejection::Unreadable.into());
        }
        Probed::Readable(report) => validate(&report, container)?,
    };
    let plan = Mp4Plan::choose(&source, max_height);
    tracing::info!(
        video = %source.video_codec,
        audio = source.audio_codec.as_deref().unwrap_or("none"),
        width = source.width,
        height = source.height,
        ?plan,
        "source validated; making the MP4"
    );
    run_ffmpeg(
        tools,
        mp4_args(input, output, &source, plan),
        expected_ms,
        ffmpeg_deadline(expected_ms),
    )
    .await?;

    let mp4 = match probe(tools, output).await? {
        Probed::Readable(report) => validate(&report, Container::Mp4)
            .map_err(|rejection| TranscodeError::BadOutput(rejection.to_string()))?,
        Probed::Unreadable { detail } => return Err(TranscodeError::BadOutput(detail)),
    };
    if mp4.video_codec != "h264" || mp4.height > max_height {
        return Err(TranscodeError::BadOutput(format!(
            "{} {}x{}",
            mp4.video_codec, mp4.width, mp4.height
        )));
    }
    let mp4_bytes = tokio::fs::metadata(output).await?.len();
    Ok(TranscodeReport {
        source,
        plan,
        mp4,
        mp4_bytes,
    })
}

/// The poster as written: its picture size and file size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosterReport {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// A poster frame is quick: one decode and one JPEG.
const POSTER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// §10 Process step 5: the poster JPEG from the finished MP4.
pub async fn make_poster(
    tools: &MediaTools,
    mp4: &Path,
    output: &Path,
    duration_ms: u32,
) -> Result<PosterReport, TranscodeError> {
    let mut command = tokio::process::Command::new(&tools.ffmpeg);
    command.args(poster_args(mp4, output, duration_ms));
    let result = run(command, POSTER_TIMEOUT).await?;
    if !result.status.success() {
        return Err(TranscodeError::Tool(ToolError::Failed {
            tool: tools.ffmpeg.clone(),
            status: result.status.to_string(),
            stderr_tail: String::from_utf8_lossy(&result.stderr)
                .lines()
                .last()
                .unwrap_or_default()
                .to_string(),
        }));
    }
    let Probed::Readable(report) = probe(tools, output).await? else {
        return Err(TranscodeError::BadOutput(
            "the poster isn't readable".to_string(),
        ));
    };
    let stream = report
        .streams
        .first()
        .ok_or_else(|| TranscodeError::BadOutput("the poster has no picture".to_string()))?;
    Ok(PosterReport {
        width: stream.width.unwrap_or(0),
        height: stream.height.unwrap_or(0),
        bytes: tokio::fs::metadata(output).await?.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ffmpeg_available, fixture_path, is_fast_start};

    #[tokio::test]
    async fn a_poster_is_cut_from_the_mp4() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = std::env::temp_dir().join(format!("sintade-poster-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&dir).await.expect("dir");
        let mp4 = dir.join("default.mp4");
        let report = transcode_file(
            &MediaTools::default(),
            &fixture_path("real_chrome_vp9_opus_10s.webm"),
            Container::Webm,
            &mp4,
            1080,
            10_000,
        )
        .await
        .expect("mp4");
        let poster = dir.join("poster.jpg");
        let made = make_poster(
            &MediaTools::default(),
            &mp4,
            &poster,
            report.mp4.duration_ms.unwrap_or(0),
        )
        .await
        .expect("poster");
        assert_eq!((made.width, made.height), (1280, 720));
        let bytes = tokio::fs::read(&poster).await.expect("read");
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "a JPEG");
        assert_eq!(made.bytes, bytes.len() as u64);
        tokio::fs::remove_dir_all(&dir).await.expect("tidy");
    }

    async fn make(name: &str, container: Container, max_height: u32) -> (TranscodeReport, Vec<u8>) {
        let out = std::env::temp_dir().join(format!("sintade-mp4-{}.mp4", uuid::Uuid::now_v7()));
        let report = transcode_file(
            &MediaTools::default(),
            &fixture_path(name),
            container,
            &out,
            max_height,
            30_000,
        )
        .await
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        let bytes = tokio::fs::read(&out).await.expect("read mp4");
        tokio::fs::remove_file(&out).await.expect("tidy");
        (report, bytes)
    }

    /// Day 46: every recording fixture becomes a fast-start H.264 MP4 of the right size and
    /// length; Safari's is a remux.
    #[tokio::test]
    async fn every_fixture_becomes_a_fast_start_h264_mp4() {
        if !ffmpeg_available().await {
            return;
        }
        // (fixture, container, plan, audio, width, height, duration range in ms)
        let cases = [
            (
                "vp9_opus_30s.webm",
                Container::Webm,
                false,
                Some("aac"),
                640,
                360,
                29_500..30_600,
            ),
            (
                "vp8_opus_paused.webm",
                Container::Webm,
                false,
                Some("aac"),
                640,
                360,
                12_500..13_600,
            ),
            (
                "vp9_no_audio.webm",
                Container::Webm,
                false,
                None,
                640,
                360,
                9_500..10_600,
            ),
            (
                "truncated_last_chunk.webm",
                Container::Webm,
                false,
                Some("aac"),
                640,
                360,
                5_000..10_000,
            ),
            (
                "real_chrome_vp9_opus_10s.webm",
                Container::Webm,
                false,
                Some("aac"),
                1280,
                720,
                9_000..11_500,
            ),
            (
                "real_firefox_vp8_opus_10s.webm",
                Container::Webm,
                false,
                Some("aac"),
                1280,
                720,
                9_000..11_500,
            ),
            (
                "h264_aac_safari_20s.mp4",
                Container::Mp4,
                true,
                Some("aac"),
                640,
                360,
                19_500..20_600,
            ),
        ];
        for (name, container, remux, audio, width, height, duration) in cases {
            let (report, bytes) = make(name, container, 1080).await;
            assert_eq!(report.plan == Mp4Plan::Remux, remux, "{name}");
            assert_eq!(report.mp4.video_codec, "h264", "{name}");
            assert_eq!(report.mp4.audio_codec.as_deref(), audio, "{name}");
            assert_eq!(
                (report.mp4.width, report.mp4.height),
                (width, height),
                "{name}"
            );
            let ms = report.mp4.duration_ms.expect("an MP4 records its duration");
            assert!(
                duration.contains(&ms),
                "{name}: {ms} ms not in {duration:?}"
            );
            assert!(is_fast_start(&bytes), "{name}: moov must precede mdat");
            assert_eq!(report.mp4_bytes, bytes.len() as u64, "{name}");
        }
    }

    #[tokio::test]
    async fn the_plan_height_scales_the_mp4_down() {
        if !ffmpeg_available().await {
            return;
        }
        let (report, _) = make("real_chrome_vp9_opus_10s.webm", Container::Webm, 360).await;
        assert_eq!((report.mp4.width, report.mp4.height), (640, 360));
        // Safari's 360p stays a remux under a 360p cap; under a lower one it's transcoded.
        let (remux, _) = make("h264_aac_safari_20s.mp4", Container::Mp4, 360).await;
        assert_eq!(remux.plan, Mp4Plan::Remux);
        let (scaled, _) = make("h264_aac_safari_20s.mp4", Container::Mp4, 240).await;
        assert_eq!(scaled.plan, Mp4Plan::Transcode { max_height: 240 });
        assert_eq!(scaled.mp4.height, 240);
    }

    #[tokio::test]
    async fn not_a_video_is_rejected_before_ffmpeg_runs() {
        if !ffmpeg_available().await {
            return;
        }
        let result = transcode_file(
            &MediaTools::default(),
            &fixture_path("not_a_video.webm"),
            Container::Webm,
            &std::env::temp_dir().join("never-written.mp4"),
            1080,
            1_000,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(TranscodeError::Rejected(ProbeRejection::Unreadable))
            ),
            "{result:?}"
        );
    }
}
