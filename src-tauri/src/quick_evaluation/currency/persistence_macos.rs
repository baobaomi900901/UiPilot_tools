//! Disposable currency cache adapter over the Mac directory-relative store.
//! Keeps the shared cache's failure semantics without weakening no-follow IO.
use std::{path::{Path, PathBuf}, sync::Arc};
use crate::atomic_file;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AtomicFileError { Read, CandidateWrite }

#[derive(Clone)]
pub(super) struct AtomicPaths {
    directory: Option<Arc<atomic_file::AtomicPaths>>,
    current: PathBuf,
    backup: PathBuf,
}
impl AtomicPaths {
    pub(super) fn new(root: &Path, name: &str) -> Self {
        Self {
            directory: atomic_file::AtomicPaths::open(root, name).ok().map(Arc::new),
            current: root.join(name), backup: root.join(format!("{name}.backup")),
        }
    }
    pub(super) fn available(&self) -> bool { self.directory.is_some() }
    pub(super) fn current(&self) -> &Path { &self.current }
    pub(super) fn backup(&self) -> &Path { &self.backup }
}
pub(super) fn read_optional_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, AtomicFileError> {
    let parent = path.parent().ok_or(AtomicFileError::Read)?;
    let name = path.file_name().and_then(|s| s.to_str()).ok_or(AtomicFileError::Read)?;
    let paths = atomic_file::AtomicPaths::open(parent, name).map_err(|_| AtomicFileError::Read)?;
    paths.read_optional_bounded(paths.current(), limit).map_err(|_| AtomicFileError::Read)
}
pub(super) fn commit_with_backup(paths: &AtomicPaths, previous: Option<&[u8]>, candidate: &[u8]) -> Result<(), AtomicFileError> {
    let directory = paths.directory.as_ref().ok_or(AtomicFileError::CandidateWrite)?;
    atomic_file::commit_with_backup(directory, previous, candidate).map_err(|_| AtomicFileError::CandidateWrite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink, sync::atomic::{AtomicU64, Ordering}};
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!("uipilot-currency-io-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap(); Self(root)
        }
    }
    impl Drop for TestDir { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }


    #[test]
    fn bounded_cache_reads_reject_oversized_files_and_links() {
        let temp = TestDir::new();
        let root = &temp.0;
        let path = root.join("cache.json");
        fs::write(&path, b"12345").unwrap();
        assert_eq!(read_optional_bounded(&path, 5).unwrap().unwrap(), b"12345");
        assert_eq!(read_optional_bounded(&path, 4), Err(AtomicFileError::Read));
        let link = root.join("link.json"); symlink(&path, &link).unwrap();
        assert_eq!(read_optional_bounded(&link, 10), Err(AtomicFileError::Read));
    }
    #[test]
    fn pending_persistence_keeps_original_directory_after_path_replacement() {
        let temp = TestDir::new();
        let root = &temp.0;
        let owned = root.join("owned"); let moved = root.join("moved"); let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        let paths = AtomicPaths::new(&owned, "cache.json");
        let pending = paths.clone();
        commit_with_backup(&paths, None, b"old").unwrap();
        fs::rename(&owned, &moved).unwrap(); symlink(&outside, &owned).unwrap();
        commit_with_backup(&pending, Some(b"old"), b"new").unwrap();
        assert_eq!(fs::read(moved.join("cache.json")).unwrap(), b"new");
        assert_eq!(fs::read(moved.join("cache.json.backup")).unwrap(), b"old");
        assert!(!outside.join("cache.json").exists());
        assert!(!AtomicPaths::new(&owned, "cache.json").available());
    }
}
