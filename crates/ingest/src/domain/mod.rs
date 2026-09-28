use kernel::{RecordingId, TakeId, WorkspaceId};

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

    /// The file extension for stored chunks: the container, `webm` or `mp4`.
    pub fn extension(&self) -> &'static str {
        extension_for(&self.0)
    }
}

fn extension_for(mime_type: &str) -> &'static str {
    if mime_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("video/mp4")
    {
        "mp4"
    } else {
        "webm"
    }
}

/// Most chunk URLs one presign call may return (`?count=`, docs/design.md §9).
pub const MAX_PRESIGN_BATCH: u32 = 10;

/// Highest chunk index accepted: 2 s chunks, so about 55 hours. Keys pad to 6 digits.
pub const MAX_CHUNK_INDEX: u32 = 99_999;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChunkRangeError {
    #[error("count must be between 1 and {MAX_PRESIGN_BATCH}")]
    BadCount,
    #[error("chunk index must be at most {MAX_CHUNK_INDEX}")]
    IndexTooLarge,
}

/// Checks a presign request's `first..first+count` range; returns the indexes.
pub fn chunk_range(first: u32, count: u32) -> Result<std::ops::Range<u32>, ChunkRangeError> {
    if count == 0 || count > MAX_PRESIGN_BATCH {
        return Err(ChunkRangeError::BadCount);
    }
    let end = first
        .checked_add(count)
        .ok_or(ChunkRangeError::IndexTooLarge)?;
    if end - 1 > MAX_CHUNK_INDEX {
        return Err(ChunkRangeError::IndexTooLarge);
    }
    Ok(first..end)
}

/// Where a chunk lives in object storage (docs/design.md §8): under the recording's prefix,
/// so deleting a recording is one prefix delete.
pub fn chunk_key(
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    take_id: TakeId,
    idx: u32,
    mime_type: &str,
) -> String {
    format!(
        "ws/{workspace_id}/rec/{recording_id}/takes/{take_id}/chunks/{idx:06}.{}",
        extension_for(mime_type)
    )
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
    fn chunk_keys_follow_the_storage_layout() {
        let workspace = WorkspaceId::new_v7();
        let recording = RecordingId::new_v7();
        let take = TakeId::new_v7();
        assert_eq!(
            chunk_key(workspace, recording, take, 7, "video/webm;codecs=vp9,opus"),
            format!("ws/{workspace}/rec/{recording}/takes/{take}/chunks/000007.webm")
        );
        assert!(
            chunk_key(workspace, recording, take, 0, "video/mp4;codecs=avc1")
                .ends_with("/000000.mp4")
        );
    }

    #[test]
    fn presign_ranges_are_bounded() {
        assert_eq!(chunk_range(0, 1), Ok(0..1));
        assert_eq!(chunk_range(5, MAX_PRESIGN_BATCH), Ok(5..15));
        assert_eq!(chunk_range(0, 0), Err(ChunkRangeError::BadCount));
        assert_eq!(
            chunk_range(0, MAX_PRESIGN_BATCH + 1),
            Err(ChunkRangeError::BadCount)
        );
        assert_eq!(
            chunk_range(MAX_CHUNK_INDEX, 1),
            Ok(MAX_CHUNK_INDEX..MAX_CHUNK_INDEX + 1)
        );
        assert_eq!(
            chunk_range(MAX_CHUNK_INDEX, 2),
            Err(ChunkRangeError::IndexTooLarge)
        );
        assert_eq!(
            chunk_range(u32::MAX, 2),
            Err(ChunkRangeError::IndexTooLarge)
        );
    }

    #[test]
    fn rejects_overlong_values() {
        let raw = format!("video/webm;codecs={}", "a".repeat(MAX_MIME_TYPE_LEN));
        assert_eq!(MimeType::parse(&raw), Err(MimeTypeError::TooLong));
    }
}
