use std::path::{Path, PathBuf};

use crate::domain::Container;
use crate::domain::hls::{
    BuiltRung, PlaylistSegment, Rung, master_playlist, parse_playlist, rung_args, rungs_for,
};
use crate::domain::probe::{SourceInfo, validate};
use crate::domain::transcode::{ffmpeg_deadline, x264_preset};
use crate::infra::tools::{MediaTools, Probed, ToolError, probe, run_ffmpeg};

#[derive(Debug, thiserror::Error)]
pub enum HlsError {
    /// The MP4 the ladder is cut from isn't something processing accepts.
    #[error("the MP4 can't be cut into a ladder: {0}")]
    BadSource(String),

    /// FFmpeg/ffprobe failed, timed out or stalled (worth retrying).
    #[error(transparent)]
    Tool(#[from] ToolError),

    /// What FFmpeg wrote doesn't check out.
    #[error("rung {rung} doesn't check out: {reason}")]
    BadOutput { rung: &'static str, reason: String },

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// Why `BuildHls` didn't produce a ladder.
#[derive(Debug, thiserror::Error)]
pub enum BuildHlsError {
    /// The take's MP4 isn't stored yet (`ProcessTake` hasn't finished). Retrying later fixes it.
    #[error("the recording's MP4 isn't ready")]
    NotReady,

    #[error(transparent)]
    Hls(#[from] HlsError),

    #[error(transparent)]
    Storage(#[from] platform::StorageError),

    #[error(transparent)]
    Download(#[from] crate::infra::source::AssembleError),

    #[error(transparent)]
    Queue(#[from] platform::JobQueueError),

    #[error(transparent)]
    Outbox(#[from] platform::OutboxError),

    #[error("database: {0}")]
    Db(#[from] sqlx::Error),

    #[error("payload: {0}")]
    Json(#[from] serde_json::Error),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// A rung written to disk.
#[derive(Debug, Clone)]
pub struct RungFiles {
    pub built: BuiltRung,
    pub dir: PathBuf,
    /// The segments in playback order.
    pub segments: Vec<PlaylistSegment>,
    pub duration_ms: u32,
    pub bytes: u64,
}

/// The ladder as written: every rung, and the master playlist at `master`.
#[derive(Debug, Clone)]
pub struct Ladder {
    pub rungs: Vec<RungFiles>,
    pub master: PathBuf,
}

/// How far a rung's length may be from the MP4's: a segment boundary's worth of rounding.
const DURATION_TOLERANCE_MS: u32 = 1_500;

/// §10 Process, `BuildHls`: cuts `mp4` into one fMP4 rung per height the source reaches,
/// under `out/{360p,720p,1080p}/`, and writes `out/master.m3u8`. Each rung is read back with
/// ffprobe (H.264 at the height it should have, the MP4's length) before the master lists it.
/// `expected_ms` is the declared length, used only if the MP4 doesn't say.
pub async fn build_ladder(
    tools: &MediaTools,
    mp4: &Path,
    out: &Path,
    expected_ms: u32,
) -> Result<Ladder, HlsError> {
    let source = read_source(tools, mp4).await?;
    // The MP4 is what the ladder must match; the length the client declared is only a fallback
    // for an MP4 that doesn't record one.
    let expected_ms = source.duration_ms.unwrap_or(expected_ms);
    let rungs = rungs_for(source.height);
    tracing::info!(
        height = source.height,
        rungs = ?rungs.iter().map(|rung| rung.name).collect::<Vec<_>>(),
        "building the HLS ladder"
    );

    let mut built = Vec::new();
    for rung in rungs {
        built.push(build_rung(tools, mp4, out, rung, &source, expected_ms).await?);
    }
    let master = out.join("master.m3u8");
    let listed: Vec<BuiltRung> = built.iter().map(|files| files.built.clone()).collect();
    tokio::fs::write(&master, master_playlist(&listed)).await?;
    Ok(Ladder {
        rungs: built,
        master,
    })
}

async fn read_source(tools: &MediaTools, mp4: &Path) -> Result<SourceInfo, HlsError> {
    match probe(tools, mp4).await? {
        Probed::Unreadable { detail } => Err(HlsError::BadSource(detail)),
        Probed::Readable(report) => validate(&report, Container::Mp4)
            .map_err(|rejection| HlsError::BadSource(rejection.to_string())),
    }
}

async fn build_rung(
    tools: &MediaTools,
    mp4: &Path,
    out: &Path,
    rung: Rung,
    source: &SourceInfo,
    expected_ms: u32,
) -> Result<RungFiles, HlsError> {
    let bad = |reason: String| HlsError::BadOutput {
        rung: rung.name,
        reason,
    };
    let dir = out.join(rung.name);
    tokio::fs::create_dir_all(&dir).await?;
    run_ffmpeg(
        tools,
        rung_args(
            mp4,
            &dir,
            rung,
            source.audio_codec.is_some(),
            x264_preset(expected_ms),
        ),
        expected_ms,
        ffmpeg_deadline(expected_ms),
    )
    .await?;

    let playlist = tokio::fs::read_to_string(dir.join("index.m3u8")).await?;
    let segments =
        parse_playlist(&playlist).ok_or_else(|| bad("the playlist isn't a finished VOD".into()))?;
    if !dir.join("init.mp4").is_file() {
        return Err(bad("no init segment".to_string()));
    }
    let mut bytes = tokio::fs::metadata(dir.join("init.mp4")).await?.len();
    let mut peak_bps = 0u64;
    let mut seconds = 0.0;
    for segment in &segments {
        let size = match tokio::fs::metadata(dir.join(&segment.file)).await {
            Ok(meta) => meta.len(),
            Err(_) => return Err(bad(format!("{} is missing", segment.file))),
        };
        bytes += size;
        seconds += segment.seconds;
        peak_bps = peak_bps.max(bits_per_second(size, segment.seconds));
    }

    // What a player will see: ffprobe follows the playlist through every segment.
    let report = match probe(tools, &dir.join("index.m3u8")).await? {
        Probed::Readable(report) => report,
        Probed::Unreadable { detail } => return Err(bad(detail)),
    };
    if !report
        .format
        .format_name
        .split(',')
        .any(|name| name == "hls")
    {
        return Err(bad(format!("read as {}", report.format.format_name)));
    }
    let video = report
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")
        .ok_or_else(|| bad("no video".to_string()))?;
    let (width, height) = (video.width.unwrap_or(0), video.height.unwrap_or(0));
    let wanted_height = rung.height.min(source.height) & !1;
    if video.codec_name != "h264" || height != wanted_height || width == 0 {
        return Err(bad(format!(
            "{} {width}x{height}, wanted h264 at {wanted_height} high",
            video.codec_name
        )));
    }
    if source.audio_codec.is_some()
        && !report
            .streams
            .iter()
            .any(|stream| stream.codec_type == "audio" && stream.codec_name == "aac")
    {
        return Err(bad("the AAC audio is missing".to_string()));
    }
    let duration_ms = report
        .format
        .duration
        .as_deref()
        .and_then(|text| text.parse::<f64>().ok())
        .map_or_else(|| (seconds * 1000.0) as u32, |s| (s * 1000.0) as u32);
    if expected_ms > 0 && duration_ms.abs_diff(expected_ms) > DURATION_TOLERANCE_MS {
        return Err(bad(format!(
            "{duration_ms} ms long, the MP4 is {expected_ms} ms"
        )));
    }

    Ok(RungFiles {
        built: BuiltRung {
            rung,
            width,
            height,
            average_bps: bits_per_second(bytes, seconds),
            peak_bps: peak_bps.max(bits_per_second(bytes, seconds)),
        },
        dir,
        segments,
        duration_ms,
        bytes,
    })
}

fn bits_per_second(bytes: u64, seconds: f64) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (bytes as f64 * 8.0 / seconds).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::transcode_file;
    use crate::testing::{ffmpeg_available, fixture_path};

    async fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sintade-{name}-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&dir).await.expect("dir");
        dir
    }

    /// The MP4 `ProcessTake` makes from a golden fixture.
    async fn mp4_from(fixture: &str, dir: &Path) -> PathBuf {
        let mp4 = dir.join("default.mp4");
        transcode_file(
            &MediaTools::default(),
            &fixture_path(fixture),
            Container::Webm,
            &mp4,
            1080,
            0,
        )
        .await
        .expect("mp4");
        mp4
    }

    /// A 1920x1080 H.264/AAC MP4 of `seconds`, straight from FFmpeg's test sources.
    async fn full_hd_mp4(dir: &Path, seconds: u32) -> PathBuf {
        let mp4 = dir.join("hd.mp4");
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "testsrc2=size=1920x1080:rate=30:duration={seconds}"
            ))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:duration={seconds}"))
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .args(["-c:a", "aac", "-b:a", "128k", "-ar", "48000"])
            .arg(&mp4)
            .status()
            .await
            .expect("run ffmpeg");
        assert!(status.success());
        mp4
    }

    /// ffprobe reads `path` (master or rung) and reports `(codec, height)` of its video.
    async fn probed_video(path: &Path) -> (String, u32) {
        let Probed::Readable(report) = probe(&MediaTools::default(), path).await.expect("probe")
        else {
            panic!("{} isn't readable", path.display());
        };
        assert!(report.format.format_name.contains("hls"));
        let video = report
            .streams
            .iter()
            .find(|stream| stream.codec_type == "video")
            .expect("video");
        (video.codec_name.clone(), video.height.unwrap_or(0))
    }

    #[tokio::test]
    async fn a_720p_recording_gets_360p_and_720p_and_a_master_that_validates() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("hls-720").await;
        let mp4 = mp4_from("real_chrome_vp9_opus_10s.webm", &dir).await;
        let out = dir.join("hls");
        let ladder = build_ladder(&MediaTools::default(), &mp4, &out, 0)
            .await
            .expect("ladder");

        let names: Vec<_> = ladder.rungs.iter().map(|r| r.built.rung.name).collect();
        assert_eq!(names, ["360p", "720p"]);
        let sizes: Vec<_> = ladder
            .rungs
            .iter()
            .map(|r| (r.built.width, r.built.height))
            .collect();
        assert_eq!(sizes, [(640, 360), (1280, 720)]);
        for rung in &ladder.rungs {
            assert!(rung.segments.len() >= 2, "10 s at 4 s per segment");
            assert!(rung.dir.join("init.mp4").is_file());
            assert!(rung.built.peak_bps >= rung.built.average_bps);
            assert!(rung.bytes > 0);
            assert_eq!(
                probed_video(&rung.dir.join("index.m3u8")).await,
                ("h264".to_string(), rung.built.height)
            );
        }
        // The master text lists exactly these rungs, lowest first.
        let master = tokio::fs::read_to_string(&ladder.master)
            .await
            .expect("master");
        let streams: Vec<&str> = master
            .lines()
            .filter(|l| l.ends_with("/index.m3u8"))
            .collect();
        assert_eq!(streams, ["360p/index.m3u8", "720p/index.m3u8"]);
        assert!(master.contains("RESOLUTION=1280x720"), "{master}");
        // ffprobe follows the master into its first rung.
        assert_eq!(probed_video(&ladder.master).await.0, "h264");
    }

    #[tokio::test]
    async fn a_360p_recording_gets_only_the_lowest_rung_and_never_upscales() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("hls-360").await;
        let mp4 = mp4_from("vp9_opus_30s.webm", &dir).await;
        let ladder = build_ladder(&MediaTools::default(), &mp4, &dir.join("hls"), 0)
            .await
            .expect("ladder");
        assert_eq!(ladder.rungs.len(), 1);
        assert_eq!(ladder.rungs[0].built.height, 360);
        assert!(!dir.join("hls/720p").exists());
    }

    #[tokio::test]
    async fn a_silent_recording_makes_a_ladder_without_audio() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("hls-silent").await;
        let mp4 = mp4_from("vp9_no_audio.webm", &dir).await;
        let ladder = build_ladder(&MediaTools::default(), &mp4, &dir.join("hls"), 0)
            .await
            .expect("ladder");
        assert_eq!(ladder.rungs.len(), 1);
        assert!(ladder.rungs[0].built.average_bps > 0);
    }

    #[tokio::test]
    async fn a_full_hd_recording_gets_all_three_rungs() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("hls-1080").await;
        let mp4 = full_hd_mp4(&dir, 9).await;
        let ladder = build_ladder(&MediaTools::default(), &mp4, &dir.join("hls"), 0)
            .await
            .expect("ladder");
        let heights: Vec<_> = ladder.rungs.iter().map(|r| r.built.height).collect();
        assert_eq!(heights, [360, 720, 1080]);
        assert_eq!(
            probed_video(&ladder.rungs[2].dir.join("index.m3u8")).await,
            ("h264".to_string(), 1080)
        );
        // Every rung is cut at the same places, so a player can switch between them.
        let cuts: Vec<usize> = ladder.rungs.iter().map(|r| r.segments.len()).collect();
        assert_eq!(cuts[0], cuts[1]);
        assert_eq!(cuts[1], cuts[2]);
    }

    #[tokio::test]
    async fn something_that_is_not_media_is_refused_before_any_rung() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("hls-bad").await;
        let junk = dir.join("default.mp4");
        tokio::fs::write(&junk, b"not a video")
            .await
            .expect("write");
        let error = build_ladder(&MediaTools::default(), &junk, &dir.join("hls"), 0)
            .await
            .expect_err("refused");
        assert!(matches!(error, HlsError::BadSource(_)), "{error}");
        assert!(!dir.join("hls").exists());
    }
}
