use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use kernel::TakeId;

/// The worker's scratch area (local disk): one directory per job, removed when the job ends,
/// whatever the outcome. Leftovers from a crashed worker are swept at startup.
#[derive(Debug, Clone)]
pub struct ScratchSpace {
    root: PathBuf,
}

impl ScratchSpace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A fresh, empty directory for processing `take_id`. A directory left by an earlier
    /// attempt at the same take is replaced.
    pub async fn create(&self, take_id: TakeId) -> io::Result<ScratchDir> {
        let path = self.root.join(take_id.to_string());
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        tokio::fs::create_dir_all(&path).await?;
        Ok(ScratchDir { path: Some(path) })
    }

    /// Removes job directories untouched for longer than `older_than` (a worker that died
    /// mid-job can't clean up after itself). Returns how many were removed.
    pub async fn sweep_leftovers(&self, older_than: Duration) -> io::Result<usize> {
        let mut entries = match tokio::fs::read_dir(&self.root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let now = SystemTime::now();
        let mut removed = 0;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or_default();
            if metadata.is_dir() && age > older_than {
                tokio::fs::remove_dir_all(entry.path()).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// One job's scratch directory. [`ScratchDir::remove`] deletes it; if the job's future is
/// dropped or panics first, `Drop` deletes it instead.
#[derive(Debug)]
pub struct ScratchDir {
    path: Option<PathBuf>,
}

impl ScratchDir {
    pub fn path(&self) -> &Path {
        self.path.as_deref().unwrap_or_else(|| Path::new(""))
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path().join(name)
    }

    pub async fn remove(mut self) -> io::Result<()> {
        match self.path.take() {
            Some(path) => match tokio::fs::remove_dir_all(&path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
            None => Ok(()),
        }
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Some(path) = self.path.take()
            && let Err(error) = std::fs::remove_dir_all(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %path.display(), "could not remove scratch dir");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("sintade-scratch-test-{}", uuid::Uuid::now_v7()))
    }

    #[tokio::test]
    async fn a_job_dir_is_created_empty_and_removed() {
        let space = ScratchSpace::new(temp_root());
        let take = TakeId::new_v7();
        let dir = space.create(take).await.expect("create");
        tokio::fs::write(dir.file("source.webm"), b"bytes")
            .await
            .expect("write");
        // A second attempt at the same take starts clean.
        let again = space.create(take).await.expect("recreate");
        assert!(!again.file("source.webm").exists());
        let path = again.path().to_path_buf();
        again.remove().await.expect("remove");
        assert!(!path.exists());
        drop(dir); // already gone: dropping is harmless
        tokio::fs::remove_dir_all(space.root()).await.expect("tidy");
    }

    #[tokio::test]
    async fn dropping_a_job_dir_removes_it() {
        let space = ScratchSpace::new(temp_root());
        let dir = space.create(TakeId::new_v7()).await.expect("create");
        let path = dir.path().to_path_buf();
        drop(dir);
        assert!(!path.exists());
        tokio::fs::remove_dir_all(space.root()).await.expect("tidy");
    }

    #[tokio::test]
    async fn leftovers_older_than_the_cutoff_are_swept() {
        let space = ScratchSpace::new(temp_root());
        let dir = space.create(TakeId::new_v7()).await.expect("create");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir); // as if the worker died mid-job
        assert_eq!(
            space
                .sweep_leftovers(Duration::from_secs(3600))
                .await
                .expect("sweep"),
            0
        );
        assert!(path.exists(), "a fresh dir may belong to a running job");
        assert_eq!(
            space.sweep_leftovers(Duration::ZERO).await.expect("sweep"),
            1
        );
        assert!(!path.exists());
        tokio::fs::remove_dir_all(space.root()).await.expect("tidy");
    }
}
