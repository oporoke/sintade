//! The HLS ladder (docs/design.md §10 Process, `BuildHls`): which rungs a recording gets, the
//! FFmpeg arguments for one rung, and the master playlist. Pure: no processes here.

use std::ffi::OsString;
use std::path::Path;

/// Seconds per segment (the design's `-hls_time 4`).
pub const SEGMENT_SECONDS: u32 = 4;

/// One rung of the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rung {
    /// Directory and `variant` name: `360p`.
    pub name: &'static str,
    pub height: u32,
    /// Video bitrate ceiling in kbit/s. CRF 23 sets the quality; this keeps a busy screen
    /// (scrolling, video) from sending the rung's size past what its row of the ladder is for.
    pub max_kbps: u32,
}

pub const LADDER: [Rung; 3] = [
    Rung {
        name: "360p",
        height: 360,
        max_kbps: 800,
    },
    Rung {
        name: "720p",
        height: 720,
        max_kbps: 2_800,
    },
    Rung {
        name: "1080p",
        height: 1080,
        max_kbps: 5_000,
    },
];

/// The rungs a source of `source_height` gets: every rung that isn't taller than the source
/// (no upscaling). A source shorter than 360 px still gets the lowest rung, at its own height,
/// so there is always something to play.
pub fn rungs_for(source_height: u32) -> Vec<Rung> {
    let rungs: Vec<Rung> = LADDER
        .into_iter()
        .filter(|rung| rung.height <= source_height)
        .collect();
    if rungs.is_empty() {
        vec![LADDER[0]]
    } else {
        rungs
    }
}

/// `ffmpeg` arguments that cut `mp4` into one rung under `dir`: `index.m3u8`, `init.mp4` and
/// `seg_0000.m4s`… (the design's command, plus):
///
/// - a keyframe every segment, forced by time (screen recordings are variable frame rate, so a
///   frame-count GOP would drift), and no scene-cut keyframes: every segment starts on a
///   keyframe and the rungs' segments line up, so the player can switch between them;
/// - `independent_segments`: each segment decodes on its own;
/// - audio is copied: the MP4 already holds the 128 kb/s AAC (48 kHz, loudness-normalised) that
///   every rung wants, so it isn't encoded three more times.
pub fn rung_args(
    mp4: &Path,
    dir: &Path,
    rung: Rung,
    has_audio: bool,
    preset: &str,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(mp4.as_os_str().to_owned());
    let scale = format!("scale=-2:trunc(min(ih\\,{})/2)*2", rung.height);
    let maxrate = format!("{}k", rung.max_kbps);
    let bufsize = format!("{}k", rung.max_kbps * 2);
    let keyframes = format!("expr:gte(t,n_forced*{SEGMENT_SECONDS})");
    let segment_seconds = SEGMENT_SECONDS.to_string();
    let mut push = |values: &[&str]| args.extend(values.iter().map(OsString::from));
    push(&["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn"]);
    push(&[
        "-c:v",
        "libx264",
        "-preset",
        preset,
        "-crf",
        "23",
        "-maxrate",
        &maxrate,
        "-bufsize",
        &bufsize,
        "-pix_fmt",
        "yuv420p",
        "-vf",
        &scale,
        "-force_key_frames",
        &keyframes,
        "-sc_threshold",
        "0",
    ]);
    if has_audio {
        push(&["-c:a", "copy"]);
    }
    push(&[
        "-f",
        "hls",
        "-hls_time",
        &segment_seconds,
        "-hls_playlist_type",
        "vod",
        "-hls_segment_type",
        "fmp4",
        "-hls_flags",
        "independent_segments",
        "-hls_fmp4_init_filename",
        "init.mp4",
        "-progress",
        "pipe:1",
        "-nostats",
    ]);
    args.push(OsString::from("-hls_segment_filename"));
    args.push(dir.join("seg_%04d.m4s").into_os_string());
    args.push(dir.join("index.m3u8").into_os_string());
    args
}

/// A segment as a rung's playlist lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistSegment {
    pub file: String,
    pub seconds: f64,
}

/// What a rung's `index.m3u8` lists: its segments in order. `None` if it isn't a finished VOD
/// playlist (no `#EXTM3U`, no `#EXT-X-ENDLIST`, or an `#EXTINF` without a file).
pub fn parse_playlist(text: &str) -> Option<Vec<PlaylistSegment>> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next()? != "#EXTM3U" {
        return None;
    }
    let mut segments = Vec::new();
    let mut pending: Option<f64> = None;
    let mut ended = false;
    for line in lines {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let seconds = rest.split(',').next()?.parse::<f64>().ok()?;
            pending = Some(seconds);
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if !line.starts_with('#') {
            segments.push(PlaylistSegment {
                file: line.to_string(),
                seconds: pending.take()?,
            });
        }
    }
    (ended && pending.is_none() && !segments.is_empty()).then_some(segments)
}

/// A finished rung, as the master playlist needs to know it.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltRung {
    pub rung: Rung,
    pub width: u32,
    pub height: u32,
    /// Total bits over total seconds.
    pub average_bps: u64,
    /// The busiest segment's bits over its seconds.
    pub peak_bps: u64,
}

/// `master.m3u8`, rungs lowest first (a player with no bandwidth estimate starts at the top of
/// the list; hls.js's `startLevel` and Safari both begin low and climb).
/// `TODO: Verify` CODECS: left out, so a player probes the first segment instead of reading it
/// here; every rung is H.264 High/AAC-LC, which every target browser plays.
pub fn master_playlist(rungs: &[BuiltRung]) -> String {
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n");
    for built in rungs {
        text.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={},AVERAGE-BANDWIDTH={},RESOLUTION={}x{}\n{}/index.m3u8\n",
            built.peak_bps, built.average_bps, built.width, built.height, built.rung.name
        ));
    }
    text
}

/// Content type for an object under `hls/`, by file name.
pub fn content_type(file: &str) -> &'static str {
    if file.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if file.ends_with(".m4s") {
        "video/iso.segment"
    } else {
        "video/mp4"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(args: &[OsString]) -> String {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_rung_is_never_taller_than_the_source() {
        let names = |height| {
            rungs_for(height)
                .iter()
                .map(|rung| rung.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(1080), ["360p", "720p", "1080p"]);
        assert_eq!(names(1440), ["360p", "720p", "1080p"]);
        assert_eq!(names(900), ["360p", "720p"]);
        assert_eq!(names(720), ["360p", "720p"]);
        assert_eq!(names(360), ["360p"]);
        // Too small for any rung: the lowest, at the source's own height.
        assert_eq!(names(240), ["360p"]);
    }

    #[test]
    fn a_rung_is_fragmented_mp4_cut_on_keyframes() {
        let args = joined(&rung_args(
            Path::new("default.mp4"),
            Path::new("/scratch/720p"),
            LADDER[1],
            true,
            "veryfast",
        ));
        for expected in [
            "-i default.mp4",
            "-c:v libx264 -preset veryfast -crf 23 -maxrate 2800k -bufsize 5600k",
            "-vf scale=-2:trunc(min(ih\\,720)/2)*2",
            "-force_key_frames expr:gte(t,n_forced*4) -sc_threshold 0",
            "-c:a copy",
            "-f hls -hls_time 4 -hls_playlist_type vod -hls_segment_type fmp4",
            "-hls_flags independent_segments",
            "-hls_fmp4_init_filename init.mp4",
            "-hls_segment_filename /scratch/720p/seg_%04d.m4s /scratch/720p/index.m3u8",
        ] {
            assert!(args.contains(expected), "missing {expected:?} in {args}");
        }
    }

    #[test]
    fn a_silent_source_gets_no_audio_settings() {
        let args = joined(&rung_args(
            Path::new("in.mp4"),
            Path::new("360p"),
            LADDER[0],
            false,
            "veryfast",
        ));
        assert!(!args.contains("-c:a"), "{args}");
    }

    const PLAYLIST: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:4\n\
        #EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"init.mp4\"\n\
        #EXTINF:4.000000,\nseg_0000.m4s\n#EXTINF:2.500000,\nseg_0001.m4s\n#EXT-X-ENDLIST\n";

    #[test]
    fn a_finished_playlist_lists_its_segments() {
        assert_eq!(
            parse_playlist(PLAYLIST),
            Some(vec![
                PlaylistSegment {
                    file: "seg_0000.m4s".to_string(),
                    seconds: 4.0
                },
                PlaylistSegment {
                    file: "seg_0001.m4s".to_string(),
                    seconds: 2.5
                },
            ])
        );
    }

    #[test]
    fn an_unfinished_or_broken_playlist_is_refused() {
        assert_eq!(
            parse_playlist(&PLAYLIST.replace("#EXT-X-ENDLIST\n", "")),
            None
        );
        assert_eq!(parse_playlist(&PLAYLIST.replace("#EXTM3U\n", "")), None);
        assert_eq!(parse_playlist("#EXTM3U\n#EXT-X-ENDLIST\n"), None);
        // A segment line with no #EXTINF before it.
        assert_eq!(
            parse_playlist("#EXTM3U\nseg_0000.m4s\n#EXT-X-ENDLIST\n"),
            None
        );
        // An #EXTINF with no file after it.
        assert_eq!(
            parse_playlist("#EXTM3U\n#EXTINF:4.0,\n#EXT-X-ENDLIST\n"),
            None
        );
    }

    #[test]
    fn the_master_lists_rungs_lowest_first_with_what_was_measured() {
        let built = |rung: Rung, width, height, average_bps, peak_bps| BuiltRung {
            rung,
            width,
            height,
            average_bps,
            peak_bps,
        };
        let master = master_playlist(&[
            built(LADDER[0], 640, 360, 300_000, 450_000),
            built(LADDER[1], 1280, 720, 900_000, 1_400_000),
        ]);
        assert_eq!(
            master,
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
             #EXT-X-STREAM-INF:BANDWIDTH=450000,AVERAGE-BANDWIDTH=300000,RESOLUTION=640x360\n\
             360p/index.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=1400000,AVERAGE-BANDWIDTH=900000,RESOLUTION=1280x720\n\
             720p/index.m3u8\n"
        );
    }

    #[test]
    fn content_types_follow_the_file() {
        assert_eq!(content_type("index.m3u8"), "application/vnd.apple.mpegurl");
        assert_eq!(content_type("seg_0001.m4s"), "video/iso.segment");
        assert_eq!(content_type("init.mp4"), "video/mp4");
    }
}
