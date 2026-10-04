//! The pipeline against every golden fixture: duration, codecs, seekability (docs/design.md
//! §18 "golden-file tests"; CI runs these with `MEDIA_REQUIRE_FFMPEG=1`).

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::app::transcode::make_poster;
    use crate::app::transcode_file;
    use crate::domain::Container;
    use crate::infra::tools::MediaTools;
    use crate::testing::{decodes_fully, ffmpeg_available, fixture_path, is_fast_start};

    struct Golden {
        name: &'static str,
        container: Container,
        audio: Option<&'static str>,
        size: (u32, u32),
        /// The MP4's length as ffprobe reports it, in ms.
        duration_ms: std::ops::Range<u32>,
    }

    fn goldens() -> Vec<Golden> {
        let webm = Container::Webm;
        vec![
            Golden {
                name: "vp9_opus_30s.webm",
                container: webm,
                audio: Some("aac"),
                size: (640, 360),
                duration_ms: 29_500..30_600,
            },
            Golden {
                name: "vp8_opus_paused.webm",
                container: webm,
                audio: Some("aac"),
                size: (640, 360),
                duration_ms: 12_500..13_600,
            },
            Golden {
                name: "vp9_no_audio.webm",
                container: webm,
                audio: None,
                size: (640, 360),
                duration_ms: 9_500..10_600,
            },
            Golden {
                name: "truncated_last_chunk.webm",
                container: webm,
                audio: Some("aac"),
                size: (640, 360),
                duration_ms: 5_000..10_000,
            },
            Golden {
                name: "real_chrome_vp9_opus_10s.webm",
                container: webm,
                audio: Some("aac"),
                size: (1280, 720),
                duration_ms: 9_000..11_500,
            },
            Golden {
                name: "real_firefox_vp8_opus_10s.webm",
                container: webm,
                audio: Some("aac"),
                size: (1280, 720),
                duration_ms: 9_000..11_500,
            },
            Golden {
                name: "h264_aac_safari_20s.mp4",
                container: Container::Mp4,
                audio: Some("aac"),
                size: (640, 360),
                duration_ms: 19_500..20_600,
            },
        ]
    }

    /// Decodes one frame starting at `at_ms`; true if a frame came out.
    async fn seek_decodes_a_frame(mp4: &Path, at_ms: u32) -> bool {
        let out = std::env::temp_dir().join(format!("sintade-seek-{}.jpg", uuid::Uuid::now_v7()));
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-ss"])
            .arg(format!("{:.3}", f64::from(at_ms) / 1000.0))
            .arg("-i")
            .arg(mp4)
            .args(["-frames:v", "1"])
            .arg(&out)
            .status()
            .await
            .expect("run ffmpeg");
        let wrote = tokio::fs::metadata(&out)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        let _ = tokio::fs::remove_file(&out).await;
        status.success() && wrote
    }

    /// Longest gap between video keyframes, in seconds (a seek never lands further than this
    /// from a decodable frame).
    async fn longest_keyframe_gap(mp4: &Path) -> f64 {
        let output = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "packet=pts_time,flags",
                "-of",
                "csv=p=0",
            ])
            .arg(mp4)
            .output()
            .await
            .expect("run ffprobe");
        let text = String::from_utf8_lossy(&output.stdout);
        let mut keys: Vec<f64> = text
            .lines()
            .filter_map(|line| {
                let (pts, flags) = line.split_once(',')?;
                flags.starts_with('K').then(|| pts.parse().ok())?
            })
            .collect();
        assert!(!keys.is_empty(), "no keyframes in {mp4:?}");
        keys.sort_by(f64::total_cmp);
        keys.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
    }

    #[tokio::test]
    async fn every_fixture_is_covered_by_a_golden() {
        let dir = fixture_path("README.md");
        let dir = dir.parent().expect("fixtures dir");
        let known: Vec<&str> = goldens().iter().map(|g| g.name).collect();
        for entry in std::fs::read_dir(dir).expect("read fixtures") {
            let name = entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string();
            let media = name.ends_with(".webm") || name.ends_with(".mp4");
            if media && name != "not_a_video.webm" {
                assert!(
                    known.contains(&name.as_str()),
                    "{name} has no golden expectations"
                );
            }
        }
    }

    #[tokio::test]
    async fn every_fixture_produces_a_playable_seekable_mp4_and_poster() {
        if !ffmpeg_available().await {
            return;
        }
        let tools = MediaTools::default();
        for golden in goldens() {
            let name = golden.name;
            let dir = std::env::temp_dir().join(format!("sintade-golden-{}", uuid::Uuid::now_v7()));
            tokio::fs::create_dir_all(&dir).await.expect("dir");
            let mp4 = dir.join("default.mp4");
            let report = transcode_file(
                &tools,
                &fixture_path(name),
                golden.container,
                &mp4,
                1080,
                30_000,
            )
            .await
            .unwrap_or_else(|error| panic!("{name}: {error}"));

            // Duration and codecs.
            let ms = report.mp4.duration_ms.expect("an MP4 records its duration");
            assert!(
                golden.duration_ms.contains(&ms),
                "{name}: {ms} ms not in {:?}",
                golden.duration_ms
            );
            assert_eq!(report.mp4.video_codec, "h264", "{name}");
            assert_eq!(report.mp4.audio_codec.as_deref(), golden.audio, "{name}");
            assert_eq!((report.mp4.width, report.mp4.height), golden.size, "{name}");

            // Seekability: index first, keyframes close together, every frame decodes,
            // and seeks to the start, middle and near the end each yield a frame.
            let bytes = tokio::fs::read(&mp4).await.expect("read mp4");
            assert!(is_fast_start(&bytes), "{name}: moov must precede mdat");
            let gap = longest_keyframe_gap(&mp4).await;
            // Transcodes force a keyframe every 2 s of media (plus the 3 s pause gap in
            // `vp8_opus_paused`, where no frames exist); a remux keeps the source's own.
            let limit = if report.plan == crate::domain::transcode::Mp4Plan::Remux {
                10.0
            } else {
                5.0
            };
            assert!(
                gap <= limit,
                "{name}: keyframes {gap:.1} s apart (limit {limit})"
            );
            assert!(
                decodes_fully(&mp4).await,
                "{name}: the MP4 must decode without errors"
            );
            for at in [0, ms / 2, ms.saturating_sub(1_500)] {
                assert!(
                    seek_decodes_a_frame(&mp4, at).await,
                    "{name}: seek to {at} ms gave no frame"
                );
            }

            // Poster.
            let poster = dir.join("poster.jpg");
            let made = make_poster(&tools, &mp4, &poster, ms)
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                (made.width, made.height),
                golden.size,
                "{name}: poster size"
            );
            let jpeg = tokio::fs::read(&poster).await.expect("poster");
            assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF], "{name}: a JPEG");

            tokio::fs::remove_dir_all(&dir).await.expect("tidy");
        }
    }
}
