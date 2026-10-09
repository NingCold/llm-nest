use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// One OS lock per Runtime/data directory, shared by JSON and SQLite stores.
/// The lock file remains in place; the OS releases ownership on process death.
pub struct DataDirectoryGuard {
    dir: PathBuf,
    _lock: fs::File,
}
impl DataDirectoryGuard {
    pub fn acquire(dir: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let dir = std::path::absolute(dir)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(".llmn.lock"))?;
        lock.try_lock().map_err(|error| {
            std::io::Error::other(format!(
                "data directory is already in use ({}): {error}",
                dir.display()
            ))
        })?;
        Ok(Arc::new(Self { dir, _lock: lock }))
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
