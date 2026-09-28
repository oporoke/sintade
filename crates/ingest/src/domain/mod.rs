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

/// Largest chunk accepted, in bytes (README §4.4: max chunk 16 MB). A 2 s slice at the
/// recorder's bitrates is well under 1 MB.
pub const MAX_CHUNK_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChunkSizeError {
    #[error("size_bytes must be between 1 and {MAX_CHUNK_BYTES}")]
    OutOfRange,
}

/// Checks a claimed chunk size.
pub fn chunk_size(size_bytes: u64) -> Result<u64, ChunkSizeError> {
    if (1..=MAX_CHUNK_BYTES).contains(&size_bytes) {
        Ok(size_bytes)
    } else {
        Err(ChunkSizeError::OutOfRange)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DigestError {
    #[error("sha256 must be 64 hex characters")]
    Malformed,
}

/// A chunk's SHA-256, sent by the client as 64 hex characters (any case).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn parse_hex(raw: &str) -> Result<Self, DigestError> {
        let raw = raw.as_bytes();
        if raw.len() != 64 {
            return Err(DigestError::Malformed);
        }
        let mut bytes = [0u8; 32];
        let (pairs, _) = raw.as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_value(*high)? << 4) | hex_value(*low)?;
        }
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

fn hex_value(digit: u8) -> Result<u8, DigestError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(DigestError::Malformed),
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
    fn digests_round_trip_through_hex() {
        let hex = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
        let digest = Sha256Digest::parse_hex(hex).expect("valid");
        assert_eq!(digest.to_hex(), hex.to_ascii_lowercase());
        assert_eq!(digest.as_bytes()[0], 0xe3);
        assert_eq!(Sha256Digest::from_bytes(digest.as_bytes()), Some(digest));
        for bad in [
            "",
            "e3b0",
            &"g".repeat(64),
            &"a".repeat(63),
            &"a".repeat(65),
        ] {
            assert_eq!(
                Sha256Digest::parse_hex(bad),
                Err(DigestError::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn chunk_sizes_are_bounded() {
        assert_eq!(chunk_size(1), Ok(1));
        assert_eq!(chunk_size(MAX_CHUNK_BYTES), Ok(MAX_CHUNK_BYTES));
        assert_eq!(chunk_size(0), Err(ChunkSizeError::OutOfRange));
        assert_eq!(
            chunk_size(MAX_CHUNK_BYTES + 1),
            Err(ChunkSizeError::OutOfRange)
        );
    }

    #[test]
    fn rejects_overlong_values() {
        let raw = format!("video/webm;codecs={}", "a".repeat(MAX_MIME_TYPE_LEN));
        assert_eq!(MimeType::parse(&raw), Err(MimeTypeError::TooLong));
    }
}
