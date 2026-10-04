//! How a source becomes the default MP4 (docs/design.md §10 Process step 4): the FFmpeg
//! arguments, decided from what ffprobe found. Pure: no processes here.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use super::probe::SourceInfo;

/// How the MP4 is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp4Plan {
    /// Already H.264/AAC (Safari) and within the plan's height: copy the streams into a
    /// fast-start MP4, no re-encode.
    Remux,
    /// Everything else (VP8/VP9/AV1, Opus): H.264/AAC, scaled down to at most `max_height`.
    Transcode { max_height: u32 },
}

impl Mp4Plan {
    pub fn choose(info: &SourceInfo, max_height: u32) -> Self {
        if info.is_remuxable() && info.height <= max_height {
            Self::Remux
        } else {
            Self::Transcode { max_height }
        }
    }
}

/// `ffmpeg` arguments that turn `input` into a fast-start MP4 at `output` (the design's
/// command, plus what MediaRecorder output needs):
///
/// - `-movflags +faststart` puts the index first, so playback and seeking start before the
///   whole file has downloaded.
/// - Variable frame rate is kept (`-fps_mode vfr`): screen recordings repeat frames rarely and
///   a pause leaves a timestamp gap that must stay a gap, not become duplicated frames.
/// - Audio across a pause gap is filled with silence (`aresample=async=1`) so sound stays in
///   sync, then loudness-normalised; loudnorm works at 192 kHz, so the output is set to 48 kHz.
/// - The height is capped (`max_height`, the plan's resolution) and kept even, as H.264 4:2:0
///   requires; the width follows the aspect ratio (`-2`).
pub fn mp4_args(input: &Path, output: &Path, info: &SourceInfo, plan: Mp4Plan) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(input.as_os_str().to_owned());
    let mut push = |values: &[&str]| args.extend(values.iter().map(OsString::from));
    push(&["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn"]);
    match plan {
        Mp4Plan::Remux => push(&["-c", "copy"]),
        Mp4Plan::Transcode { max_height } => {
            let scale = format!("scale=-2:trunc(min(ih\\,{max_height})/2)*2");
            push(&[
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-pix_fmt",
                "yuv420p",
                "-fps_mode",
                "vfr",
                "-vf",
                &scale,
            ]);
            if info.audio_codec.is_some() {
                push(&[
                    "-c:a",
                    "aac",
                    "-b:a",
                    "128k",
                    "-af",
                    "aresample=async=1:first_pts=0,loudnorm",
                    "-ar",
                    "48000",
                ]);
            }
        }
    }
    push(&["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"]);
    args.push(output.as_os_str().to_owned());
    args
}

/// How long an FFmpeg run may take before it's killed: 3x the recording's length (the target
/// is 0.5x, §11), and never less than 10 minutes, so a short take on a busy worker isn't cut
/// off.
pub fn ffmpeg_deadline(duration_ms: u32) -> Duration {
    Duration::from_millis(u64::from(duration_ms) * 3).max(Duration::from_secs(10 * 60))
}

/// One `-progress` block from FFmpeg: how far into the output it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub out_time_ms: u64,
    pub done: bool,
}

/// Folds one `key=value` line of FFmpeg's `-progress` output into `progress`. Returns `true`
/// when the line ends a block (`progress=continue|end`).
pub fn read_progress_line(line: &str, progress: &mut Progress) -> bool {
    let Some((key, value)) = line.trim().split_once('=') else {
        return false;
    };
    match key {
        // Microseconds, despite the name of `out_time_ms` (an FFmpeg quirk); prefer `_us`.
        "out_time_us" => {
            if let Ok(us) = value.parse::<i64>() {
                progress.out_time_ms = u64::try_from(us / 1000).unwrap_or(0);
            }
            false
        }
        "progress" => {
            progress.done = value == "end";
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(video: &str, audio: Option<&str>, height: u32) -> SourceInfo {
        SourceInfo {
            video_codec: video.to_string(),
            audio_codec: audio.map(str::to_string),
            width: height * 16 / 9,
            height,
            duration_ms: None,
        }
    }

    fn joined(args: &[OsString]) -> String {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn safari_h264_within_the_plan_is_remuxed() {
        assert_eq!(
            Mp4Plan::choose(&info("h264", Some("aac"), 720), 1080),
            Mp4Plan::Remux
        );
        // Over the plan's height it is transcoded (and scaled) instead.
        assert_eq!(
            Mp4Plan::choose(&info("h264", Some("aac"), 1440), 1080),
            Mp4Plan::Transcode { max_height: 1080 }
        );
        assert_eq!(
            Mp4Plan::choose(&info("vp9", Some("opus"), 720), 1080),
            Mp4Plan::Transcode { max_height: 1080 }
        );
    }

    #[test]
    fn a_remux_copies_streams_into_a_fast_start_mp4() {
        let args = joined(&mp4_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            &info("h264", Some("aac"), 720),
            Mp4Plan::Remux,
        ));
        assert!(args.contains("-c copy"), "{args}");
        assert!(args.contains("-movflags +faststart"), "{args}");
        assert!(!args.contains("libx264"), "{args}");
        assert!(args.ends_with("out.mp4"), "{args}");
    }

    #[test]
    fn a_transcode_makes_h264_aac_capped_and_even() {
        let args = joined(&mp4_args(
            Path::new("in.webm"),
            Path::new("out.mp4"),
            &info("vp9", Some("opus"), 1440),
            Mp4Plan::Transcode { max_height: 1080 },
        ));
        for expected in [
            "-c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p",
            "-vf scale=-2:trunc(min(ih\\,1080)/2)*2",
            "-c:a aac -b:a 128k",
            "loudnorm",
            "-ar 48000",
            "-movflags +faststart",
            "-progress pipe:1",
        ] {
            assert!(args.contains(expected), "missing {expected:?} in {args}");
        }
    }

    #[test]
    fn a_silent_source_gets_no_audio_settings() {
        let args = joined(&mp4_args(
            Path::new("in.webm"),
            Path::new("out.mp4"),
            &info("vp9", None, 360),
            Mp4Plan::Transcode { max_height: 1080 },
        ));
        assert!(!args.contains("-c:a"), "{args}");
        assert!(!args.contains("loudnorm"), "{args}");
    }

    #[test]
    fn deadlines_scale_with_length_with_a_floor() {
        assert_eq!(ffmpeg_deadline(10_000), Duration::from_secs(600));
        assert_eq!(ffmpeg_deadline(30 * 60_000), Duration::from_secs(90 * 60));
    }

    #[test]
    fn progress_lines_fold_into_blocks() {
        let mut progress = Progress::default();
        assert!(!read_progress_line("frame=120", &mut progress));
        assert!(!read_progress_line("out_time_us=4000000", &mut progress));
        assert!(read_progress_line("progress=continue", &mut progress));
        assert_eq!(
            progress,
            Progress {
                out_time_ms: 4_000,
                done: false
            }
        );
        assert!(!read_progress_line("out_time_us=N/A", &mut progress));
        assert!(read_progress_line("progress=end", &mut progress));
        assert!(progress.done);
        assert!(!read_progress_line("garbage", &mut progress));
    }
}
