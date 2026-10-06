//! Media's pure rules: the chunk manifest a take is processed from. No I/O here.

pub mod hls;
pub mod probe;
pub mod transcode;

use kernel::{RecordingId, TakeId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// The container a take was recorded in, from its MIME type: WebM (Chrome, Firefox) or MP4
/// (Safari).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Webm,
    Mp4,
}

impl Container {
    pub fn from_mime(mime: &str) -> Self {
        if mime.trim().to_ascii_lowercase().starts_with("video/mp4") {
            Self::Mp4
        } else {
            Self::Webm
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Webm => "video/webm",
            Self::Mp4 => "video/mp4",
        }
    }
}

/// The type to tell a viewer's browser about the stored original, so it can say whether it can
/// play it. A WebM carrying H.264 (what Chrome records when it can; ADR-0028) says so: Firefox
/// cannot play that, and a plain `video/webm` would promise it can. Everything else stays the
/// bare container, as before.
pub fn preview_content_type(mime: &str) -> String {
    let container = Container::from_mime(mime);
    let lower = mime.trim().to_ascii_lowercase();
    if container == Container::Webm && (lower.contains("h264") || lower.contains("avc1")) {
        mime.trim().to_string()
    } else {
        container.content_type().to_string()
    }
}

/// Where a recording's default MP4 lives (docs/design.md §8 storage layout).
pub fn mp4_key(workspace_id: WorkspaceId, recording_id: RecordingId) -> String {
    format!("ws/{workspace_id}/rec/{recording_id}/mp4/default.mp4")
}

/// Where a recording's HLS ladder lives: the master playlist is `master.m3u8` here and each rung
/// is a directory (docs/design.md §8 storage layout).
pub fn hls_prefix(workspace_id: WorkspaceId, recording_id: RecordingId) -> String {
    format!("ws/{workspace_id}/rec/{recording_id}/hls")
}

/// Where a recording's poster lives (docs/design.md §8 storage layout).
pub fn poster_key(workspace_id: WorkspaceId, recording_id: RecordingId) -> String {
    format!("ws/{workspace_id}/rec/{recording_id}/img/poster.jpg")
}

/// Where a take's concatenated original lives (docs/design.md §8 storage layout).
pub fn source_key(
    workspace_id: WorkspaceId,
    recording_id: RecordingId,
    take_id: TakeId,
    container: Container,
) -> String {
    format!(
        "ws/{workspace_id}/rec/{recording_id}/takes/{take_id}/source.{}",
        container.extension()
    )
}

/// One stored chunk of a take, as `TakeFinalized` listed it (ADR-0012).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestChunk {
    pub idx: u32,
    /// Object storage key of the chunk.
    pub key: String,
    pub size_bytes: u32,
    /// Lowercase hex SHA-256 the client reported (and ingest recorded) for the chunk.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("a take needs at least one chunk")]
    Empty,

    #[error("chunk_count is {chunk_count} but the manifest lists {listed} chunk(s)")]
    CountMismatch { chunk_count: u32, listed: usize },

    #[error("chunk {expected} is missing from the manifest")]
    Gap { expected: u32 },

    #[error("chunk {idx} has an invalid SHA-256")]
    BadDigest { idx: u32 },

    #[error("chunk {idx} is empty")]
    EmptyChunk { idx: u32 },
}

/// The chunks of a take, `0..n` in order, each with a well-formed SHA-256: what processing
/// downloads, verifies and concatenates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkManifest(Vec<ManifestChunk>);

impl ChunkManifest {
    /// Sorts the chunks and checks they are exactly `0..chunk_count`.
    pub fn new(mut chunks: Vec<ManifestChunk>, chunk_count: u32) -> Result<Self, ManifestError> {
        if chunk_count == 0 {
            return Err(ManifestError::Empty);
        }
        if chunks.len() != chunk_count as usize {
            return Err(ManifestError::CountMismatch {
                chunk_count,
                listed: chunks.len(),
            });
        }
        chunks.sort_by_key(|chunk| chunk.idx);
        for (expected, chunk) in (0..chunk_count).zip(&chunks) {
            if chunk.idx != expected {
                return Err(ManifestError::Gap { expected });
            }
            if chunk.size_bytes == 0 {
                return Err(ManifestError::EmptyChunk { idx: chunk.idx });
            }
            if parse_sha256(&chunk.sha256).is_none() {
                return Err(ManifestError::BadDigest { idx: chunk.idx });
            }
        }
        Ok(Self(chunks))
    }

    pub fn chunks(&self) -> &[ManifestChunk] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sum of the chunk sizes: the size of the concatenated source.
    pub fn total_bytes(&self) -> u64 {
        self.0.iter().map(|chunk| u64::from(chunk.size_bytes)).sum()
    }

    pub fn into_chunks(self) -> Vec<ManifestChunk> {
        self.0
    }
}

/// 64 lowercase or uppercase hex digits → 32 bytes.
pub fn parse_sha256(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let (pairs, _) = bytes.as_chunks::<2>();
    for (slot, [high, low]) in out.iter_mut().zip(pairs) {
        *slot = (hex_digit(*high)? << 4) | hex_digit(*low)?;
    }
    Some(out)
}

/// Bytes → lowercase hex.
#[cfg(test)]
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn hex_digit(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::preview_content_type;

    #[test]
    fn only_h264_in_webm_names_its_codecs() {
        assert_eq!(
            preview_content_type("video/webm;codecs=h264,opus"),
            "video/webm;codecs=h264,opus"
        );
        assert_eq!(
            preview_content_type("video/webm;codecs=vp9,opus"),
            "video/webm"
        );
        assert_eq!(preview_content_type("video/webm"), "video/webm");
        assert_eq!(
            preview_content_type("video/mp4;codecs=avc1,mp4a"),
            "video/mp4"
        );
    }

    use super::*;

    const HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn chunk(idx: u32) -> ManifestChunk {
        ManifestChunk {
            idx,
            key: format!("chunks/{idx:06}.webm"),
            size_bytes: 10,
            sha256: HASH.to_string(),
        }
    }

    #[test]
    fn a_manifest_is_sorted_and_contiguous() {
        let manifest = ChunkManifest::new(vec![chunk(2), chunk(0), chunk(1)], 3).expect("valid");
        let indexes: Vec<u32> = manifest.chunks().iter().map(|chunk| chunk.idx).collect();
        assert_eq!(indexes, [0, 1, 2]);
        assert_eq!(manifest.total_bytes(), 30);
    }

    #[test]
    fn gaps_counts_and_bad_digests_are_rejected() {
        assert_eq!(ChunkManifest::new(vec![], 0), Err(ManifestError::Empty));
        assert_eq!(
            ChunkManifest::new(vec![chunk(0)], 2),
            Err(ManifestError::CountMismatch {
                chunk_count: 2,
                listed: 1
            })
        );
        assert_eq!(
            ChunkManifest::new(vec![chunk(0), chunk(2)], 2),
            Err(ManifestError::Gap { expected: 1 })
        );
        let mut bad = chunk(0);
        bad.sha256 = "zz".to_string();
        assert_eq!(
            ChunkManifest::new(vec![bad], 1),
            Err(ManifestError::BadDigest { idx: 0 })
        );
        let mut empty = chunk(0);
        empty.size_bytes = 0;
        assert_eq!(
            ChunkManifest::new(vec![empty], 1),
            Err(ManifestError::EmptyChunk { idx: 0 })
        );
    }

    #[test]
    fn the_source_key_follows_the_storage_layout() {
        let (w, r, t) = (
            WorkspaceId::new_v7(),
            RecordingId::new_v7(),
            TakeId::new_v7(),
        );
        assert_eq!(
            source_key(w, r, t, Container::from_mime("video/webm;codecs=vp9,opus")),
            format!("ws/{w}/rec/{r}/takes/{t}/source.webm")
        );
        assert_eq!(
            source_key(w, r, t, Container::from_mime("video/mp4;codecs=avc1,mp4a")),
            format!("ws/{w}/rec/{r}/takes/{t}/source.mp4")
        );
        assert_eq!(Container::Mp4.content_type(), "video/mp4");
    }

    #[test]
    fn hex_digests_parse() {
        let bytes = parse_sha256(HASH).expect("valid hex");
        assert_eq!(to_hex(&bytes), HASH);
        assert_eq!(parse_sha256(&HASH.to_uppercase()), Some(bytes));
        assert_eq!(parse_sha256("abc"), None);
    }
}
