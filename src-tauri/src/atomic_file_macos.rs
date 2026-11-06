//! All settings operations are relative to the directory opened at startup.
//! Replacing a path component with a symlink cannot redirect an ongoing transaction.
use rustix::{
    fd::OwnedFd,
    fs::{mkdirat, open, openat, renameat, renameat_with, statat, unlinkat, AtFlags, FileType,
         Mode, OFlags, RenameFlags},
    io::Errno,
};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SETTINGS_MODE: Mode = Mode::RUSR.union(Mode::WUSR);
const DIRECTORY: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
pub(crate) type AtomicFileError = io::Error;

pub(crate) struct AtomicPaths {
    directory: OwnedFd,
    current: PathBuf,
    backup: PathBuf,
}

pub(crate) fn open_data_directory(directory: &Path, create: bool) -> io::Result<OwnedFd> {
    if !directory.is_absolute() { return Err(io::Error::other("expected absolute settings directory")); }
    let mut parent = open("/", DIRECTORY, Mode::empty())?;
    for component in directory.components() {
        let child = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(io::Error::other("invalid settings directory")),
        };
        parent = match openat(&parent, child, DIRECTORY, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) if create => {
                match mkdirat(&parent, child, Mode::RWXU) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(error) => return Err(error.into()),
                }
                openat(&parent, child, DIRECTORY, Mode::empty())?
            }
            Err(error) => return Err(error.into()),
        };
    }
    Ok(parent)
}

impl AtomicPaths {
    pub(crate) fn open(directory: &Path, name: &str) -> io::Result<Self> {
        let parent = open_data_directory(directory, true)?;
        Ok(Self { directory: parent, current: PathBuf::from(name), backup: PathBuf::from(format!("{name}.backup")) })
    }

    pub(crate) fn current(&self) -> &Path { &self.current }
    pub(crate) fn backup(&self) -> &Path { &self.backup }

    pub(crate) fn read_optional(&self, name: &Path) -> io::Result<Option<Vec<u8>>> {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let fd = match openat(&self.directory, name, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut file = File::from(fd);
        if !file.metadata()?.is_file() { return Err(io::Error::other("expected regular settings file")); }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub(crate) fn quarantine_invalid(&self, name: &Path) -> io::Result<()> {
        loop {
            let target = unique(name, "invalid");
            match renameat_with(&self.directory, name, &self.directory, target, RenameFlags::NOREPLACE) {
                Err(Errno::EXIST) => continue,
                result => return result.map_err(Into::into),
            }
        }
    }

    fn replace(&self, name: &Path, bytes: &[u8]) -> io::Result<()> {
        match statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile => {
                return Err(io::Error::other("expected regular settings file"));
            }
            Ok(_) | Err(Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
        let temp = unique(name, "temp");
        let fd = openat(&self.directory, &temp, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC, SETTINGS_MODE)?;
        let result = (|| -> io::Result<()> {
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            file.sync_all()?;
            renameat(&self.directory, &temp, &self.directory, name)?;
            Ok(())
        })();
        if result.is_err() { let _ = unlinkat(&self.directory, &temp, AtFlags::empty()); }
        result
    }
}

fn unique(name: &Path, label: &str) -> PathBuf {
    let mut result = name.as_os_str().to_owned();
    result.push(format!(".{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    PathBuf::from(result)
}

pub(crate) fn commit_with_backup(paths: &AtomicPaths, previous: Option<&[u8]>, candidate: &[u8]) -> io::Result<()> {
    if let Some(bytes) = previous { paths.replace(paths.backup(), bytes)?; }
    paths.replace(paths.current(), candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn links_are_refused_and_a_replaced_directory_does_not_redirect_writes() {
        let temp = std::env::temp_dir().canonicalize().unwrap().join(unique(Path::new("uipilot"), "test"));
        let owned = temp.join("owned");
        let outside = temp.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let paths = AtomicPaths::open(&owned, "settings.json").unwrap();
        fs::rename(&owned, temp.join("original")).unwrap();
        symlink(&outside, &owned).unwrap();
        assert!(AtomicPaths::open(&owned, "settings.json").is_err());
        commit_with_backup(&paths, None, b"first").unwrap();
        assert!(!outside.join("settings.json").exists());
        symlink(&outside, temp.join("original/settings.json.backup")).unwrap();
        assert!(commit_with_backup(&paths, Some(b"first"), b"second").is_err());
        assert_eq!(paths.read_optional(paths.current()).unwrap().unwrap(), b"first");
        assert!(paths.read_optional(paths.backup()).is_err());
        fs::remove_dir_all(temp).unwrap();
    }
}
