use std::path::{Path, PathBuf};

use crate::domain::Container;
use crate::domain::probe::validate;
use crate::domain::sprite::{
    COLUMNS, PREVIEW_WIDTH, ROWS, TILE_WIDTH, animated_webp_size, interval_seconds, preview_args,
    sheet_count, sheet_name, sprite_args, sprite_vtt, tile_count,
};
use crate::domain::transcode::ffmpeg_deadline;
use crate::infra::tools::{MediaTools, Probed, ToolError, probe, run_ffmpeg};

#[derive(Debug, thiserror::Error)]
pub enum SpriteError {
    /// The MP4 the sprite is cut from isn't something processing accepts.
    #[error("the MP4 can't be sampled: {0}")]
    BadSource(String),

    /// FFmpeg/ffprobe failed, timed out or stalled (worth retrying).
    #[error(transparent)]
    Tool(#[from] ToolError),

    /// What FFmpeg wrote doesn't check out.
    #[error("the {what} doesn't check out: {reason}")]
    BadOutput { what: &'static str, reason: String },

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

/// Why `GenerateSprite` didn't produce a sprite.
#[derive(Debug, thiserror::Error)]
pub enum GenerateSpriteError {
    /// The take's MP4 isn't stored yet (`ProcessTake` hasn't finished). Retrying later fixes it.
    #[error("the recording's MP4 isn't ready")]
    NotReady,

    #[error(transparent)]
    Sprite(#[from] SpriteError),

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

/// The sprite and the preview as written under the scratch directory.
#[derive(Debug, Clone)]
pub struct SpriteSet {
    pub interval_s: u32,
    pub tiles: u32,
    pub tile_height: u32,
    /// `sprite_0.jpg`… in order.
    pub sheets: Vec<PathBuf>,
    pub vtt: PathBuf,
    pub preview: PathBuf,
    pub preview_height: u32,
}

/// `GenerateSprite`: samples `mp4` into tiled sheets and `sprite.vtt`, and makes the animated
/// preview, all in `out`. Everything is read back (sheet size, the preview's WebP animation
/// header) before it is reported.
pub async fn build_sprite(
    tools: &MediaTools,
    mp4: &Path,
    out: &Path,
    declared_ms: u32,
) -> Result<SpriteSet, SpriteError> {
    let source = match probe(tools, mp4).await? {
        Probed::Unreadable { detail } => return Err(SpriteError::BadSource(detail)),
        Probed::Readable(report) => validate(&report, Container::Mp4)
            .map_err(|rejection| SpriteError::BadSource(rejection.to_string()))?,
    };
    // The MP4's own length is the truth; what the client declared is the fallback.
    let duration_ms = source.duration_ms.unwrap_or(declared_ms);
    let interval_s = interval_seconds(duration_ms);
    let tiles = tile_count(duration_ms, interval_s);
    tokio::fs::create_dir_all(out).await?;

    let deadline = ffmpeg_deadline(duration_ms);
    run_ffmpeg(
        tools,
        sprite_args(mp4, out, interval_s),
        duration_ms,
        deadline,
    )
    .await?;
    let wanted = sheet_count(tiles);
    let mut sheets = Vec::new();
    for index in 0..wanted {
        let sheet = out.join(sheet_name(index));
        if !sheet.is_file() {
            return Err(SpriteError::BadOutput {
                what: "sprite",
                reason: format!("{} is missing", sheet_name(index)),
            });
        }
        sheets.push(sheet);
    }
    if out.join(sheet_name(wanted)).exists() {
        return Err(SpriteError::BadOutput {
            what: "sprite",
            reason: format!("more than the {wanted} sheet(s) a {duration_ms} ms recording needs"),
        });
    }
    let tile_height = read_tile_height(tools, &sheets[0]).await?;

    let vtt = out.join("sprite.vtt");
    tokio::fs::write(&vtt, sprite_vtt(duration_ms, interval_s, tile_height)).await?;

    let preview = out.join("preview.webp");
    run_ffmpeg(
        tools,
        preview_args(mp4, &preview, duration_ms),
        duration_ms,
        deadline,
    )
    .await?;
    let preview_height = read_preview(&preview).await?;

    Ok(SpriteSet {
        interval_s,
        tiles,
        tile_height,
        sheets,
        vtt,
        preview,
        preview_height,
    })
}

/// A sheet is 10 x 10 tiles of `TILE_WIDTH`; its height says how tall a tile is.
async fn read_tile_height(tools: &MediaTools, sheet: &Path) -> Result<u32, SpriteError> {
    let bad = |reason: String| SpriteError::BadOutput {
        what: "sprite",
        reason,
    };
    let Probed::Readable(report) = probe(tools, sheet).await? else {
        return Err(bad("the sheet isn't readable".to_string()));
    };
    let stream = report
        .streams
        .first()
        .ok_or_else(|| bad("the sheet has no picture".to_string()))?;
    let (width, height) = (stream.width.unwrap_or(0), stream.height.unwrap_or(0));
    if width != COLUMNS * TILE_WIDTH || height == 0 || height % ROWS != 0 {
        return Err(bad(format!("a sheet of {width}x{height}")));
    }
    Ok(height / ROWS)
}

/// The preview must be an animated WebP of the right width: a RIFF/WEBP file with an `ANIM`
/// header and frames.
async fn read_preview(preview: &Path) -> Result<u32, SpriteError> {
    let bad = |reason: String| SpriteError::BadOutput {
        what: "preview",
        reason,
    };
    let bytes = tokio::fs::read(preview).await?;
    let Some((width, height)) = animated_webp_size(&bytes) else {
        return Err(bad("not an animated WebP".to_string()));
    };
    let has = |tag: &[u8]| bytes.windows(4).any(|window| window == tag);
    if !has(b"ANIM") || !has(b"ANMF") {
        return Err(bad("no animation frames".to_string()));
    }
    if width != PREVIEW_WIDTH {
        return Err(bad(format!("{width} px wide")));
    }
    Ok(height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sprite::{Cue, PREVIEW_FRAMES, parse_vtt};
    use crate::testing::ffmpeg_available;

    async fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sintade-{name}-{}", uuid::Uuid::now_v7()));
        tokio::fs::create_dir_all(&dir).await.expect("dir");
        dir
    }

    /// A 640x360 MP4 whose picture is a flat colour that changes every second: second `n` is
    /// `COLORS[n % COLORS.len()]`. Where a thumbnail came from can be read off its pixels.
    const COLORS: [[u8; 3]; 6] = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [255, 0, 255],
        [0, 255, 255],
    ];

    async fn coloured_mp4(dir: &Path, seconds: u32) -> PathBuf {
        let mut command = tokio::process::Command::new("ffmpeg");
        command.args(["-v", "error", "-y"]);
        let mut graph = String::new();
        for second in 0..seconds {
            let [r, g, b] = COLORS[second as usize % COLORS.len()];
            command.args(["-f", "lavfi", "-i"]).arg(format!(
                "color=c=0x{r:02x}{g:02x}{b:02x}:s=640x360:r=30:d=1"
            ));
            graph.push_str(&format!("[{second}:v]"));
        }
        graph.push_str(&format!("concat=n={seconds}:v=1:a=0[v]"));
        let mp4 = dir.join("colours.mp4");
        let status = command
            .args(["-filter_complex", &graph, "-map", "[v]"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "15"])
            .arg(&mp4)
            .status()
            .await
            .expect("run ffmpeg");
        assert!(status.success());
        mp4
    }

    /// The average colour of a rectangle of a sheet, read with FFmpeg.
    async fn colour_at(sheet: &Path, cue: &Cue) -> [u8; 3] {
        // Sample the middle of the tile, away from JPEG edge bleed.
        let (w, h) = (cue.w / 2, cue.h / 2);
        let (x, y) = (cue.x + cue.w / 4, cue.y + cue.h / 4);
        let output = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(sheet)
            .args(["-vf", &format!("crop={w}:{h}:{x}:{y},scale=1:1")])
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .await
            .expect("run ffmpeg");
        assert!(output.status.success(), "{:?}", output.stderr);
        [output.stdout[0], output.stdout[1], output.stdout[2]]
    }

    fn near(actual: [u8; 3], wanted: [u8; 3]) -> bool {
        actual
            .iter()
            .zip(wanted)
            .all(|(a, w)| (i32::from(*a) - i32::from(w)).abs() < 60)
    }

    /// Day 81's Check: every cue's tile shows the picture from the cue's start time.
    #[tokio::test]
    async fn sprite_frames_align_with_their_timestamps() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("sprite-align").await;
        let mp4 = coloured_mp4(&dir, 12).await;
        let set = build_sprite(&MediaTools::default(), &mp4, &dir.join("out"), 0)
            .await
            .expect("sprite");
        assert_eq!((set.interval_s, set.tiles, set.sheets.len()), (1, 12, 1));
        assert_eq!(set.tile_height, 90, "640x360 at 160 wide");

        let vtt = tokio::fs::read_to_string(&set.vtt).await.expect("vtt");
        let cues = parse_vtt(&vtt).expect("cues");
        assert_eq!(cues.len(), 12);
        for (index, cue) in cues.iter().enumerate() {
            assert_eq!(cue.start_ms, index as u32 * 1000);
            assert_eq!((cue.w, cue.h), (160, 90));
            let sheet = set.sheets[0].parent().expect("dir").join(&cue.sheet);
            let colour = colour_at(&sheet, cue).await;
            let wanted = COLORS[index % COLORS.len()];
            assert!(
                near(colour, wanted),
                "cue {index} at {} ms is {colour:?}, the picture there is {wanted:?}",
                cue.start_ms
            );
        }
    }

    #[tokio::test]
    async fn a_long_recording_is_sampled_more_widely_across_sheets() {
        if !ffmpeg_available().await {
            return;
        }
        // 130 s at one frame per second needs two sheets.
        let dir = scratch("sprite-sheets").await;
        let mp4 = dir.join("long.mp4");
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x180:rate=5:duration=130")
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4)
            .status()
            .await
            .expect("run ffmpeg");
        assert!(status.success());
        let set = build_sprite(&MediaTools::default(), &mp4, &dir.join("out"), 0)
            .await
            .expect("sprite");
        assert_eq!((set.interval_s, set.tiles, set.sheets.len()), (1, 130, 2));
        let cues =
            parse_vtt(&tokio::fs::read_to_string(&set.vtt).await.expect("vtt")).expect("vtt");
        assert_eq!(cues.len(), 130);
        assert_eq!(cues[100].sheet, "sprite_1.jpg");
        assert_eq!((cues[100].x, cues[100].y), (0, 0));
        assert_eq!(cues[129].end_ms, 130_000);
    }

    #[tokio::test]
    async fn the_preview_is_an_animated_webp() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("sprite-preview").await;
        let mp4 = coloured_mp4(&dir, 12).await;
        let set = build_sprite(&MediaTools::default(), &mp4, &dir.join("out"), 0)
            .await
            .expect("sprite");
        let bytes = tokio::fs::read(&set.preview).await.expect("preview");
        let frames = bytes.windows(4).filter(|w| *w == b"ANMF").count();
        assert_eq!(frames, PREVIEW_FRAMES as usize, "one frame per sample");
        assert_eq!(set.preview_height, 180, "640x360 at 320 wide");
        assert!(bytes.len() < 200_000, "{} bytes", bytes.len());
    }

    #[tokio::test]
    async fn something_that_is_not_media_is_refused() {
        if !ffmpeg_available().await {
            return;
        }
        let dir = scratch("sprite-bad").await;
        let junk = dir.join("default.mp4");
        tokio::fs::write(&junk, b"nope").await.expect("write");
        let error = build_sprite(&MediaTools::default(), &junk, &dir.join("out"), 0)
            .await
            .expect_err("refused");
        assert!(matches!(error, SpriteError::BadSource(_)), "{error}");
    }
}
