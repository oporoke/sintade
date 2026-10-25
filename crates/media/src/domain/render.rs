//! How an edit becomes an MP4 (docs/design.md §10, `RenderEdit`): the FFmpeg arguments that keep
//! the EDL's ranges of the recording's MP4 and join them. Pure: no processes here.

use std::ffi::OsString;
use std::path::Path;

use super::edl::Edl;

/// Seconds with millisecond precision, the way `trim` reads them.
fn seconds(ms: u32) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// The `-filter_complex` graph: each kept range is trimmed out of the video (and the audio, when
/// there is any), its timestamps restarted at zero, and all of them concatenated in order.
pub fn filter_graph(edl: &Edl, with_audio: bool) -> String {
    let mut graph = String::new();
    let mut joined = String::new();
    for (index, range) in edl.ranges().iter().enumerate() {
        let (start, end) = (seconds(range.start_ms), seconds(range.end_ms));
        graph.push_str(&format!(
            "[0:v:0]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{index}];"
        ));
        joined.push_str(&format!("[v{index}]"));
        if with_audio {
            graph.push_str(&format!(
                "[0:a:0]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{index}];"
            ));
            joined.push_str(&format!("[a{index}]"));
        }
    }
    let audio = u8::from(with_audio);
    let outputs = if with_audio { "[v][a]" } else { "[v]" };
    graph.push_str(&format!(
        "{joined}concat=n={}:v=1:a={audio}{outputs}",
        edl.ranges().len()
    ));
    graph
}

/// `ffmpeg` arguments that write the edited recording to `output`: H.264 (and AAC when the
/// source has audio) in a fast-start MP4, like the default MP4 it is cut from.
pub fn render_args(
    input: &Path,
    output: &Path,
    edl: &Edl,
    with_audio: bool,
    preset: &str,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(input.as_os_str().to_owned());
    let graph = filter_graph(edl, with_audio);
    let mut push = |values: &[&str]| args.extend(values.iter().map(OsString::from));
    push(&["-filter_complex", &graph, "-map", "[v]"]);
    if with_audio {
        push(&["-map", "[a]"]);
    }
    push(&[
        "-sn",
        "-dn",
        "-c:v",
        "libx264",
        "-preset",
        preset,
        "-crf",
        "23",
        "-pix_fmt",
        "yuv420p",
        "-fps_mode",
        "vfr",
        // A keyframe at least every 2 s of media time, so seeking stays quick after a join.
        "-force_key_frames",
        "expr:gte(t,n_forced*2)",
    ]);
    if with_audio {
        push(&["-c:a", "aac", "-b:a", "128k", "-ar", "48000"]);
    }
    push(&["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"]);
    args.push(output.as_os_str().to_owned());
    args
}

/// How far the rendered duration may be from the kept ranges' total and still count as equal:
/// frames and audio packets are whole, so a join can be a frame or two off.
pub const DURATION_TOLERANCE_MS: u32 = 120;

#[cfg(test)]
mod tests {
    use super::*;

    fn edl() -> Edl {
        Edl::new(&[[2_000, 10_500], [12_250, 20_000]], 40_000).expect("valid")
    }

    #[test]
    fn each_kept_range_is_trimmed_restarted_and_joined() {
        assert_eq!(
            filter_graph(&edl(), true),
            "[0:v:0]trim=start=2.000:end=10.500,setpts=PTS-STARTPTS[v0];\
             [0:a:0]atrim=start=2.000:end=10.500,asetpts=PTS-STARTPTS[a0];\
             [0:v:0]trim=start=12.250:end=20.000,setpts=PTS-STARTPTS[v1];\
             [0:a:0]atrim=start=12.250:end=20.000,asetpts=PTS-STARTPTS[a1];\
             [v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"
        );
    }

    #[test]
    fn a_silent_recording_has_no_audio_chain() {
        let graph = filter_graph(&edl(), false);
        assert!(!graph.contains("atrim"), "{graph}");
        assert!(graph.ends_with("[v0][v1]concat=n=2:v=1:a=0[v]"), "{graph}");
    }

    #[test]
    fn the_arguments_write_a_fast_start_h264_mp4() {
        let args: Vec<String> = render_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            &edl(),
            true,
            "veryfast",
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w == pair);
        assert!(has(["-c:v", "libx264"]) && has(["-c:a", "aac"]));
        assert!(has(["-map", "[v]"]) && has(["-map", "[a]"]));
        assert!(has(["-movflags", "+faststart"]));
        assert_eq!(args.last().map(String::as_str), Some("out.mp4"));
        let silent = render_args(
            Path::new("in.mp4"),
            Path::new("o.mp4"),
            &edl(),
            false,
            "veryfast",
        );
        assert!(!silent.iter().any(|arg| arg == "aac"));
    }

    #[test]
    fn each_version_plays_from_its_own_base() {
        use crate::domain::{
            base_of_mp4, edit_base, hls_prefix_in, img_prefix_in, mp4_key, mp4_key_in,
            poster_key_in, recording_base,
        };
        let (w, r) = (kernel::WorkspaceId::new_v7(), kernel::RecordingId::new_v7());
        let base = edit_base(w, r, 3);
        assert_eq!(base, format!("ws/{w}/rec/{r}/edits/3"));
        assert_eq!(
            mp4_key_in(&base),
            format!("ws/{w}/rec/{r}/edits/3/mp4/default.mp4")
        );
        assert_eq!(hls_prefix_in(&base), format!("ws/{w}/rec/{r}/edits/3/hls"));
        assert_eq!(img_prefix_in(&base), format!("ws/{w}/rec/{r}/edits/3/img"));
        assert_eq!(
            poster_key_in(&base),
            format!("ws/{w}/rec/{r}/edits/3/img/poster.jpg")
        );
        // The original keeps the layout it always had.
        assert_eq!(mp4_key(w, r), format!("ws/{w}/rec/{r}/mp4/default.mp4"));
        // Either MP4 leads back to its base; nothing else does.
        assert_eq!(base_of_mp4(&mp4_key_in(&base)), Some(base.as_str()));
        assert_eq!(
            base_of_mp4(&mp4_key(w, r)),
            Some(recording_base(w, r).as_str())
        );
        assert_eq!(base_of_mp4(&format!("{base}/img/poster.jpg")), None);
    }
}
