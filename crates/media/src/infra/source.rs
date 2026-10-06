use std::path::Path;

use platform::{ObjectStore, StorageError};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

use crate::domain::{ChunkManifest, parse_sha256};

/// Why the take's chunks couldn't be assembled into its source file.
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    /// The manifest lists a chunk storage doesn't have. Retrying can't bring it back.
    #[error("chunk {idx} is missing from storage")]
    MissingChunk { idx: u32 },

    /// The chunk's bytes aren't what the client reported (size or SHA-256). Retrying can't
    /// fix it.
    #[error("chunk {idx} doesn't match its recorded {what}")]
    CorruptChunk { idx: u32, what: &'static str },

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("scratch disk: {0}")]
    Io(#[from] std::io::Error),
}

impl AssembleError {
    /// Bad input rather than a passing failure (ADR-0012 §7).
    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::MissingChunk { .. } | Self::CorruptChunk { .. })
    }
}

const COPY_BUFFER: usize = 256 * 1024;

/// Streams every chunk, in order, into `target` (timesliced WebM/MP4 chunks are one continuous
/// stream, so concatenating their bytes rebuilds the recording), checking each chunk's size and
/// SHA-256 against the manifest on the way (docs/design.md §10 Process steps 1–2). Returns the
/// number of bytes written.
pub async fn assemble_source(
    store: &dyn ObjectStore,
    manifest: &ChunkManifest,
    target: &Path,
) -> Result<u64, AssembleError> {
    let file = tokio::fs::File::create(target).await?;
    let mut out = BufWriter::with_capacity(COPY_BUFFER, file);
    let mut buffer = vec![0u8; COPY_BUFFER];
    let mut total = 0u64;

    for chunk in manifest.chunks() {
        let mut reader = store
            .get(&chunk.key)
            .await?
            .ok_or(AssembleError::MissingChunk { idx: chunk.idx })?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        loop {
            let read = reader.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read]).await?;
            size += read as u64;
        }
        if size != u64::from(chunk.size_bytes) {
            return Err(AssembleError::CorruptChunk {
                idx: chunk.idx,
                what: "size",
            });
        }
        let expected = parse_sha256(&chunk.sha256).ok_or(AssembleError::CorruptChunk {
            idx: chunk.idx,
            what: "SHA-256",
        })?;
        if hasher.finalize().as_slice() != expected.as_slice() {
            return Err(AssembleError::CorruptChunk {
                idx: chunk.idx,
                what: "SHA-256",
            });
        }
        total += size;
    }

    out.flush().await?;
    out.into_inner().sync_all().await?;
    Ok(total)
}

/// Streams the object at `key` into `target`. `None` if storage doesn't have it.
pub async fn download_file(
    store: &dyn ObjectStore,
    key: &str,
    target: &Path,
) -> Result<Option<u64>, AssembleError> {
    let Some(mut reader) = store.get(key).await? else {
        return Ok(None);
    };
    let mut file = tokio::fs::File::create(target).await?;
    let bytes = tokio::io::copy(&mut reader, &mut file).await?;
    file.flush().await?;
    Ok(Some(bytes))
}
