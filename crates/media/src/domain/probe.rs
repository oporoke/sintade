//! What ffprobe says about a source file, and whether processing accepts it (docs/design.md
//! §10 Process step 3: "validate container, codecs, duration; reject anything unexpected").

use serde::Deserialize;

use super::Container;

/// `ffprobe -print_format json -show_format -show_streams`, the parts media reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProbeReport {
    #[serde(default)]
    pub format: ProbeFormat,
    #[serde(default)]
    pub streams: Vec<ProbeStream>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProbeFormat {
    #[serde(default)]
    pub format_name: String,
    /// Seconds as a decimal string; absent or "N/A" for MediaRecorder WebM (no duration in
    /// the header).
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProbeStream {
    #[serde(default)]
    pub codec_type: String,
    #[serde(default)]
    pub codec_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The accepted source, as processing will treat it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub width: u32,
    pub height: u32,
    /// From the container, when it records one (MP4 does; MediaRecorder WebM doesn't).
    pub duration_ms: Option<u32>,
}

impl SourceInfo {
    /// Already H.264 (+ AAC or silent): the MP4 can be a remux, no transcode (Safari).
    pub fn is_remuxable(&self) -> bool {
        self.video_codec == "h264"
            && self
                .audio_codec
                .as_deref()
                .is_none_or(|codec| codec == "aac")
    }
}

/// Why a source was refused. The message is shown to the creator (via `ProcessingFailed`), so
/// it says what's wrong, not how ffprobe put it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeRejection {
    #[error("the recording isn't a readable video file")]
    Unreadable,

    #[error("the recording is in an unexpected format ({found})")]
    UnexpectedContainer { found: String },

    #[error("the recording has no video")]
    NoVideo,

    #[error("the recording's video uses an unsupported codec ({codec})")]
    UnsupportedVideoCodec { codec: String },

    #[error("the recording's audio uses an unsupported codec ({codec})")]
    UnsupportedAudioCodec { codec: String },

    #[error("the recording's picture size ({width}x{height}) isn't supported")]
    BadDimensions { width: u32, height: u32 },
}

/// Codecs browsers' MediaRecorder produces (docs/design.md §10 Record: VP9/VP8 + Opus in WebM,
/// H.264 + AAC in MP4); AV1 for newer Chrome.
const VIDEO_CODECS: [&str; 4] = ["vp9", "vp8", "av1", "h264"];
const AUDIO_CODECS: [&str; 3] = ["opus", "vorbis", "aac"];
/// Larger than any screen we expect (8K is 7680x4320); smaller than FFmpeg's own limits.
const MAX_SIDE: u32 = 8192;

/// Accepts the source if it is the container the take declared, with one usable video stream
/// and audio (if any) in a codec browsers record.
pub fn validate(report: &ProbeReport, declared: Container) -> Result<SourceInfo, ProbeRejection> {
    let format = report.format.format_name.as_str();
    let container_ok = match declared {
        // ffprobe calls both Matroska and WebM "matroska,webm".
        Container::Webm => format
            .split(',')
            .any(|name| name == "matroska" || name == "webm"),
        Container::Mp4 => format.split(',').any(|name| name == "mp4" || name == "mov"),
    };
    if !container_ok {
        return Err(ProbeRejection::UnexpectedContainer {
            found: if format.is_empty() {
                "unknown".to_string()
            } else {
                format.to_string()
            },
        });
    }

    let video = report
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")
        .ok_or(ProbeRejection::NoVideo)?;
    if !VIDEO_CODECS.contains(&video.codec_name.as_str()) {
        return Err(ProbeRejection::UnsupportedVideoCodec {
            codec: video.codec_name.clone(),
        });
    }
    let (width, height) = (video.width.unwrap_or(0), video.height.unwrap_or(0));
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(ProbeRejection::BadDimensions { width, height });
    }

    let audio_codec = match report
        .streams
        .iter()
        .find(|stream| stream.codec_type == "audio")
    {
        Some(audio) if AUDIO_CODECS.contains(&audio.codec_name.as_str()) => {
            Some(audio.codec_name.clone())
        }
        Some(audio) => {
            return Err(ProbeRejection::UnsupportedAudioCodec {
                codec: audio.codec_name.clone(),
            });
        }
        None => None,
    };

    Ok(SourceInfo {
        video_codec: video.codec_name.clone(),
        audio_codec,
        width,
        height,
        duration_ms: report.format.duration.as_deref().and_then(seconds_to_ms),
    })
}

/// "20.023242" → 20023. `None` for "N/A", negatives and nonsense.
pub fn seconds_to_ms(seconds: &str) -> Option<u32> {
    let value: f64 = seconds.trim().parse().ok()?;
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) / 1000.0 {
        return None;
    }
    Some((value * 1000.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(json: &str) -> ProbeReport {
        serde_json::from_str(json).expect("valid ffprobe json")
    }

    const CHROME: &str = r#"{
        "streams": [
            {"index": 0, "codec_name": "vp9", "codec_type": "video", "width": 1280, "height": 720},
            {"index": 1, "codec_name": "opus", "codec_type": "audio", "channels": 2}
        ],
        "format": {"filename": "source.webm", "format_name": "matroska,webm"}
    }"#;

    const SAFARI: &str = r#"{
        "streams": [
            {"codec_name": "h264", "codec_type": "video", "width": 640, "height": 360},
            {"codec_name": "aac", "codec_type": "audio"}
        ],
        "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "20.023242"}
    }"#;

    #[test]
    fn a_chrome_recording_is_accepted_without_a_duration() {
        let info = validate(&report(CHROME), Container::Webm).expect("accepted");
        assert_eq!(
            info,
            SourceInfo {
                video_codec: "vp9".to_string(),
                audio_codec: Some("opus".to_string()),
                width: 1280,
                height: 720,
                duration_ms: None,
            }
        );
        assert!(!info.is_remuxable());
    }

    #[test]
    fn a_safari_recording_is_accepted_and_remuxable() {
        let info = validate(&report(SAFARI), Container::Mp4).expect("accepted");
        assert_eq!(info.duration_ms, Some(20_023));
        assert!(info.is_remuxable());
    }

    #[test]
    fn a_silent_h264_recording_is_remuxable() {
        let silent = report(
            r#"{"streams": [{"codec_name": "h264", "codec_type": "video", "width": 2, "height": 2}],
                "format": {"format_name": "mov,mp4"}}"#,
        );
        assert!(
            validate(&silent, Container::Mp4)
                .expect("ok")
                .is_remuxable()
        );
    }

    #[test]
    fn the_declared_container_must_match() {
        assert_eq!(
            validate(&report(SAFARI), Container::Webm),
            Err(ProbeRejection::UnexpectedContainer {
                found: "mov,mp4,m4a,3gp,3g2,mj2".to_string()
            })
        );
        assert!(matches!(
            validate(&ProbeReport::default(), Container::Webm),
            Err(ProbeRejection::UnexpectedContainer { .. })
        ));
    }

    #[test]
    fn missing_or_unexpected_streams_are_rejected() {
        let audio_only = report(
            r#"{"streams": [{"codec_name": "opus", "codec_type": "audio"}],
                "format": {"format_name": "matroska,webm"}}"#,
        );
        assert_eq!(
            validate(&audio_only, Container::Webm),
            Err(ProbeRejection::NoVideo)
        );
        let theora = report(
            r#"{"streams": [{"codec_name": "theora", "codec_type": "video", "width": 2, "height": 2}],
                "format": {"format_name": "matroska,webm"}}"#,
        );
        assert_eq!(
            validate(&theora, Container::Webm),
            Err(ProbeRejection::UnsupportedVideoCodec {
                codec: "theora".to_string()
            })
        );
        let mp3 = report(
            r#"{"streams": [{"codec_name": "vp8", "codec_type": "video", "width": 2, "height": 2},
                            {"codec_name": "mp3", "codec_type": "audio"}],
                "format": {"format_name": "matroska,webm"}}"#,
        );
        assert_eq!(
            validate(&mp3, Container::Webm),
            Err(ProbeRejection::UnsupportedAudioCodec {
                codec: "mp3".to_string()
            })
        );
        let zero = report(
            r#"{"streams": [{"codec_name": "vp8", "codec_type": "video", "width": 0, "height": 720}],
                "format": {"format_name": "matroska,webm"}}"#,
        );
        assert_eq!(
            validate(&zero, Container::Webm),
            Err(ProbeRejection::BadDimensions {
                width: 0,
                height: 720
            })
        );
    }

    #[test]
    fn durations_parse_from_seconds() {
        assert_eq!(seconds_to_ms("20.023242"), Some(20_023));
        assert_eq!(seconds_to_ms("0"), Some(0));
        assert_eq!(seconds_to_ms("N/A"), None);
        assert_eq!(seconds_to_ms("-1"), None);
        assert_eq!(seconds_to_ms("inf"), None);
    }
}
