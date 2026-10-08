//! The scrub sprite and the animated preview (docs/design.md §8 storage layout, `GenerateSprite`):
//! how often to sample, how the thumbnails tile into sheets, the `sprite.vtt` that maps time to
//! a tile, and the FFmpeg arguments. Pure: no processes here.

use std::ffi::OsString;
use std::path::Path;

/// Thumbnail width in pixels; the height follows the aspect ratio (kept even).
pub const TILE_WIDTH: u32 = 160;
pub const COLUMNS: u32 = 10;
pub const ROWS: u32 = 10;
pub const TILES_PER_SHEET: u32 = COLUMNS * ROWS;
/// Most thumbnails a recording gets (three sheets): a longer recording is sampled less often.
pub const MAX_TILES: u32 = 3 * TILES_PER_SHEET;

/// Frames in the animated preview, and how long each one shows.
pub const PREVIEW_FRAMES: u32 = 8;
pub const PREVIEW_FRAME_MS: u32 = 500;
pub const PREVIEW_WIDTH: u32 = 320;

/// Whole seconds between thumbnails: every second for a short recording, wider for a long one
/// so the count stays within [`MAX_TILES`].
pub fn interval_seconds(duration_ms: u32) -> u32 {
    let seconds = duration_ms.div_ceil(1000).max(1);
    seconds.div_ceil(MAX_TILES).max(1)
}

/// How many thumbnails `duration_ms` yields at `interval_s`.
pub fn tile_count(duration_ms: u32, interval_s: u32) -> u32 {
    duration_ms.div_ceil(interval_s * 1000).max(1)
}

pub fn sheet_count(tiles: u32) -> u32 {
    tiles.div_ceil(TILES_PER_SHEET)
}

pub fn sheet_name(index: u32) -> String {
    format!("sprite_{index}.jpg")
}

/// `ffmpeg` arguments that turn `mp4` into the sprite sheets `sheets_dir/sprite_{n}.jpg`: one
/// thumbnail every `interval_s` (the first at 0), tiled 10 by 10.
pub fn sprite_args(mp4: &Path, sheets_dir: &Path, interval_s: u32) -> Vec<OsString> {
    let filter = format!("fps=1/{interval_s},scale={TILE_WIDTH}:-2,tile={COLUMNS}x{ROWS}");
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(mp4.as_os_str().to_owned());
    args.extend(
        [
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            "-fps_mode",
            "passthrough",
            "-q:v",
            "5",
            "-start_number",
            "0",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(sheets_dir.join("sprite_%d.jpg").into_os_string());
    args
}

/// `ffmpeg` arguments for the animated preview: [`PREVIEW_FRAMES`] frames spread evenly over the
/// recording, each shown for [`PREVIEW_FRAME_MS`], looping forever.
pub fn preview_args(mp4: &Path, output: &Path, duration_ms: u32) -> Vec<OsString> {
    let seconds = f64::from(duration_ms.max(1)) / 1000.0;
    // N frames over the whole recording; a very short one repeats its frames, never fails.
    let filter = format!(
        "fps={PREVIEW_FRAMES}/{seconds:.3},scale={PREVIEW_WIDTH}:-2,\
         settb=1/1000,setpts=N*{PREVIEW_FRAME_MS}"
    );
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(mp4.as_os_str().to_owned());
    args.extend(
        [
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            // `passthrough` keeps every frame at its own time; Ubuntu's FFmpeg 6.1 drops most of
            // them under `vfr`, which left CI with 4 frames instead of 8.
            "-fps_mode",
            "passthrough",
            "-frames:v",
            &PREVIEW_FRAMES.to_string(),
            "-c:v",
            "libwebp_anim",
            "-q:v",
            "55",
            "-loop",
            "0",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(output.as_os_str().to_owned());
    args
}

/// The canvas size of an animated WebP, from its `VP8X` header (ffprobe can't read the size of an
/// animated WebP). `None` if the bytes aren't an animated WebP.
pub fn animated_webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" || bytes.get(12..16)? != b"VP8X" {
        return None;
    }
    // Flags byte: bit 1 (0x02) is "animation".
    if bytes.get(20)? & 0x02 == 0 {
        return None;
    }
    let le24 = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };
    Some((le24(24)? + 1, le24(27)? + 1))
}

/// Where a thumbnail is: its sheet and the rectangle on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePlace {
    pub sheet: u32,
    pub x: u32,
    pub y: u32,
}

pub fn tile_place(index: u32, tile_height: u32) -> TilePlace {
    let in_sheet = index % TILES_PER_SHEET;
    TilePlace {
        sheet: index / TILES_PER_SHEET,
        x: (in_sheet % COLUMNS) * TILE_WIDTH,
        y: (in_sheet / COLUMNS) * tile_height,
    }
}

/// `HH:MM:SS.mmm`.
pub fn vtt_time(ms: u32) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// `sprite.vtt`: one cue per thumbnail, `sheet.jpg#xywh=x,y,w,h`, which is what a player's
/// hover preview reads. The sheets are named relative to the VTT, so both sit under `img/`.
/// The last cue ends at the recording's end.
pub fn sprite_vtt(duration_ms: u32, interval_s: u32, tile_height: u32) -> String {
    let step = interval_s * 1000;
    let mut text = String::from("WEBVTT\n");
    for index in 0..tile_count(duration_ms, interval_s) {
        let start = index * step;
        let end = (start + step).min(duration_ms.max(start + 1));
        let place = tile_place(index, tile_height);
        text.push_str(&format!(
            "\n{} --> {}\n{}#xywh={},{},{},{}\n",
            vtt_time(start),
            vtt_time(end),
            sheet_name(place.sheet),
            place.x,
            place.y,
            TILE_WIDTH,
            tile_height
        ));
    }
    text
}

/// One cue of a sprite VTT, parsed back (for checks).
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub sheet: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[cfg(test)]
fn parse_time(text: &str) -> Option<u32> {
    let (clock, millis) = text.split_once('.')?;
    let mut parts = clock.split(':');
    let (h, m, s): (u32, u32, u32) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    Some(((h * 60 + m) * 60 + s) * 1000 + millis.parse::<u32>().ok()?)
}

#[cfg(test)]
pub fn parse_vtt(text: &str) -> Option<Vec<Cue>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "WEBVTT" {
        return None;
    }
    let mut cues = Vec::new();
    while let Some(line) = lines.next() {
        let Some((start, end)) = line.split_once(" --> ") else {
            continue;
        };
        let target = lines.next()?;
        let (sheet, rect) = target.split_once("#xywh=")?;
        let mut numbers = rect.split(',').map(|n| n.parse::<u32>().ok());
        cues.push(Cue {
            start_ms: parse_time(start)?,
            end_ms: parse_time(end)?,
            sheet: sheet.to_string(),
            x: numbers.next()??,
            y: numbers.next()??,
            w: numbers.next()??,
            h: numbers.next()??,
        });
    }
    Some(cues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_recordings_are_sampled_every_second_long_ones_less() {
        assert_eq!(interval_seconds(0), 1);
        assert_eq!(interval_seconds(10_000), 1);
        assert_eq!(interval_seconds(300_000), 1);
        assert_eq!(interval_seconds(300_001), 2);
        // One hour: 3600 / 300 = 12 s.
        assert_eq!(interval_seconds(3_600_000), 12);
        for ms in [1, 59_999, 600_000, 3_600_000, 36_000_000] {
            let interval = interval_seconds(ms);
            assert!(tile_count(ms, interval) <= MAX_TILES, "{ms}");
        }
    }

    #[test]
    fn tiles_and_sheets_are_counted_up() {
        assert_eq!(tile_count(10_000, 1), 10);
        assert_eq!(tile_count(10_001, 1), 11);
        assert_eq!(tile_count(0, 1), 1);
        assert_eq!(sheet_count(1), 1);
        assert_eq!(sheet_count(100), 1);
        assert_eq!(sheet_count(101), 2);
    }

    #[test]
    fn tiles_run_left_to_right_then_down_then_to_the_next_sheet() {
        let h = 90;
        assert_eq!(
            tile_place(0, h),
            TilePlace {
                sheet: 0,
                x: 0,
                y: 0
            }
        );
        assert_eq!(
            tile_place(1, h),
            TilePlace {
                sheet: 0,
                x: 160,
                y: 0
            }
        );
        assert_eq!(
            tile_place(10, h),
            TilePlace {
                sheet: 0,
                x: 0,
                y: 90
            }
        );
        assert_eq!(
            tile_place(99, h),
            TilePlace {
                sheet: 0,
                x: 1440,
                y: 810
            }
        );
        assert_eq!(
            tile_place(100, h),
            TilePlace {
                sheet: 1,
                x: 0,
                y: 0
            }
        );
        assert_eq!(
            tile_place(215, h),
            TilePlace {
                sheet: 2,
                x: 800,
                y: 90
            }
        );
    }

    #[test]
    fn times_are_vtt_clock_times() {
        assert_eq!(vtt_time(0), "00:00:00.000");
        assert_eq!(vtt_time(61_500), "00:01:01.500");
        assert_eq!(vtt_time(3_725_042), "01:02:05.042");
    }

    #[test]
    fn the_vtt_has_one_cue_per_tile_ending_at_the_recording_end() {
        let vtt = sprite_vtt(2_500, 1, 90);
        assert_eq!(
            vtt,
            "WEBVTT\n\
             \n00:00:00.000 --> 00:00:01.000\nsprite_0.jpg#xywh=0,0,160,90\n\
             \n00:00:01.000 --> 00:00:02.000\nsprite_0.jpg#xywh=160,0,160,90\n\
             \n00:00:02.000 --> 00:00:02.500\nsprite_0.jpg#xywh=320,0,160,90\n"
        );
        let cues = parse_vtt(&sprite_vtt(250_000, 1, 90)).expect("vtt");
        assert_eq!(cues.len(), 250);
        assert_eq!(cues[150].sheet, "sprite_1.jpg");
        assert_eq!((cues[150].x, cues[150].y), (0, 90 * 5));
        // Cues are contiguous.
        for pair in cues.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
    }

    #[test]
    fn the_vtt_reads_back() {
        let cues = parse_vtt(&sprite_vtt(5_000, 2, 100)).expect("vtt");
        assert_eq!(cues.len(), 3);
        assert_eq!(
            cues[2],
            Cue {
                start_ms: 4_000,
                end_ms: 5_000,
                sheet: "sprite_0.jpg".to_string(),
                x: 320,
                y: 0,
                w: 160,
                h: 100
            }
        );
        assert_eq!(parse_vtt("nope"), None);
    }

    #[test]
    fn an_animated_webp_header_gives_its_canvas_size() {
        let mut header = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        header.push(0x02); // flags: animation
        header.extend([0, 0, 0]); // reserved
        header.extend([0x3f, 0x01, 0x00]); // width - 1 = 319
        header.extend([0xb3, 0x00, 0x00]); // height - 1 = 179
        assert_eq!(animated_webp_size(&header), Some((320, 180)));
        header[20] = 0; // a still image
        assert_eq!(animated_webp_size(&header), None);
        assert_eq!(animated_webp_size(b"not a webp at all, not at all"), None);
    }

    fn joined(args: &[OsString]) -> String {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_sprite_is_tiled_thumbnails_at_the_interval() {
        let args = joined(&sprite_args(Path::new("d.mp4"), Path::new("/s"), 12));
        assert!(
            args.contains("-vf fps=1/12,scale=160:-2,tile=10x10"),
            "{args}"
        );
        assert!(args.contains("-start_number 0"), "{args}");
        assert!(args.ends_with("/s/sprite_%d.jpg"), "{args}");
    }

    #[test]
    fn the_preview_is_a_looping_animated_webp() {
        let args = joined(&preview_args(
            Path::new("d.mp4"),
            Path::new("p.webp"),
            10_000,
        ));
        assert!(
            args.contains("fps=8/10.000,scale=320:-2,settb=1/1000,setpts=N*500"),
            "{args}"
        );
        assert!(args.contains("-frames:v 8 -c:v libwebp_anim"), "{args}");
        assert!(args.contains("-loop 0"), "{args}");
    }
}
