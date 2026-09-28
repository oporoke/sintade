/// Longest accepted MIME type string, in bytes.
pub const MAX_MIME_TYPE_LEN: usize = 255;

/// The containers the recorder produces (docs/design.md §10) and `ProcessTake` will accept.
const CONTAINERS: [&str; 2] = ["video/webm", "video/mp4"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MimeTypeError {
    #[error("mime_type must be at most {MAX_MIME_TYPE_LEN} bytes")]
    TooLong,
    #[error("mime_type must be video/webm or video/mp4, optionally with codecs")]
    Unsupported,
}

/// The take's MIME type as `MediaRecorder` reported it, e.g. `video/webm;codecs=vp9,opus`.
/// Only the container is checked; codecs are whatever the browser chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeType(String);

impl MimeType {
    pub fn parse(raw: &str) -> Result<Self, MimeTypeError> {
        let trimmed = raw.trim();
        if trimmed.len() > MAX_MIME_TYPE_LEN {
            return Err(MimeTypeError::TooLong);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(MimeTypeError::Unsupported);
        }
        let container = trimmed
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if CONTAINERS.contains(&container.as_str()) {
            Ok(Self(trimmed.to_string()))
        } else {
            Err(MimeTypeError::Unsupported)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which sources feed the take, recorded so processing and the player know what to expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sources {
    pub system_audio: bool,
    pub mic: bool,
    pub camera: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_what_the_recorder_produces() {
        for raw in [
            "video/webm",
            "video/webm;codecs=vp9,opus",
            "video/webm;codecs=vp8",
            "video/mp4;codecs=avc1,mp4a",
            "VIDEO/MP4",
        ] {
            assert_eq!(MimeType::parse(raw).expect(raw).as_str(), raw);
        }
    }

    #[test]
    fn rejects_other_containers() {
        for raw in [
            "",
            "audio/webm",
            "video/webmx",
            "video/quicktime",
            "text/html",
            "video/webm;\u{0}codecs=vp8",
        ] {
            assert_eq!(
                MimeType::parse(raw),
                Err(MimeTypeError::Unsupported),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn rejects_overlong_values() {
        let raw = format!("video/webm;codecs={}", "a".repeat(MAX_MIME_TYPE_LEN));
        assert_eq!(MimeType::parse(&raw), Err(MimeTypeError::TooLong));
    }
}
