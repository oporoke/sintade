//! ffprobe + validation against every golden fixture (docs/design.md §18).

#[cfg(test)]
mod tests {
    use crate::domain::Container;
    use crate::domain::probe::{ProbeRejection, validate};
    use crate::infra::tools::{MediaTools, Probed, probe};
    use crate::testing::{ffmpeg_available, fixture_path};

    async fn probed(name: &str) -> Probed {
        probe(&MediaTools::default(), &fixture_path(name))
            .await
            .expect("ffprobe runs")
    }

    #[tokio::test]
    async fn every_recording_fixture_is_accepted_with_its_codecs() {
        if !ffmpeg_available().await {
            return;
        }
        let cases = [
            (
                "vp9_opus_30s.webm",
                Container::Webm,
                "vp9",
                Some("opus"),
                640,
                360,
            ),
            (
                "vp8_opus_paused.webm",
                Container::Webm,
                "vp8",
                Some("opus"),
                640,
                360,
            ),
            ("vp9_no_audio.webm", Container::Webm, "vp9", None, 640, 360),
            (
                "truncated_last_chunk.webm",
                Container::Webm,
                "vp9",
                Some("opus"),
                640,
                360,
            ),
            (
                "real_chrome_vp9_opus_10s.webm",
                Container::Webm,
                "vp9",
                Some("opus"),
                1280,
                720,
            ),
            (
                "real_firefox_vp8_opus_10s.webm",
                Container::Webm,
                "vp8",
                Some("opus"),
                1280,
                720,
            ),
            (
                "h264_aac_safari_20s.mp4",
                Container::Mp4,
                "h264",
                Some("aac"),
                640,
                360,
            ),
        ];
        for (name, container, video, audio, width, height) in cases {
            let Probed::Readable(report) = probed(name).await else {
                panic!("{name}: ffprobe should read it");
            };
            let info = validate(&report, container).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(info.video_codec, video, "{name}");
            assert_eq!(info.audio_codec.as_deref(), audio, "{name}");
            assert_eq!((info.width, info.height), (width, height), "{name}");
            assert_eq!(info.is_remuxable(), name.ends_with(".mp4"), "{name}");
        }
    }

    #[tokio::test]
    async fn not_a_video_is_unreadable() {
        if !ffmpeg_available().await {
            return;
        }
        let Probed::Unreadable { detail } = probed("not_a_video.webm").await else {
            panic!("not_a_video.webm must not probe as media");
        };
        assert!(detail.contains("Invalid data"), "{detail}");
        // And the creator sees why, in words.
        assert_eq!(
            ProbeRejection::Unreadable.to_string(),
            "the recording isn't a readable video file"
        );
    }
}
