//! A private, bounded file medium. No plugin or JSON authorization lives here.
//! The caller must hold its genuine data-call lease across every operation.
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FlockOperation, Mode, OFlags},
    io::Errno,
};
use std::{
    cell::Cell,
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

pub(super) const MAX_DOCUMENT: usize = super::storage_document::STORAGE_QUOTA_BYTES + 256;
const LOCK: &str = ".storage.lock";
#[derive(Clone, Copy)]
pub(super) enum FileKind {
    Storage,
    Settings,
    Lifecycle,
}
impl FileKind {
    fn current(self) -> &'static str {
        match self {
            Self::Storage => "storage.json",
            Self::Settings => "settings.json",
            Self::Lifecycle => "plugins.json",
        }
    }
    fn stage(self) -> &'static str {
        match self {
            Self::Storage => "storage.pending",
            Self::Settings => "settings.pending",
            Self::Lifecycle => "plugins.pending",
        }
    }
}
const DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileError {
    Storage,
    Corrupt,
    CommitUncertain,
}
type Result<T> = std::result::Result<T, FileError>;
fn checked<T, E>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|_| FileError::Storage)
}
fn no_acl(fd: &impl AsRawFd) -> bool {
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    let acl = unsafe { acl_get_fd_np(fd.as_raw_fd(), 0x100) };
    if acl.is_null() {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
    }
    unsafe {
        acl_free(acl);
    }
    false
}
fn private(fd: &impl AsRawFd, directory: bool) -> Result<()> {
    check_private(fd, directory, false)
}
fn check_private(fd: &impl AsRawFd, directory: bool, readable_parent: bool) -> Result<()> {
    // statfs and ACL inspect the held descriptor, never a second pathname.
    let mut volume = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe {
        libc::fstatfs(fd.as_raw_fd(), volume.as_mut_ptr()) != 0
            || volume.assume_init().f_flags & libc::MNT_LOCAL as u32 == 0
    } || !no_acl(fd)
    {
        return Err(FileError::Storage);
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(FileError::Storage);
    }
    let stat = unsafe { stat.assume_init() };
    let expected = if directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    };
    let mode = if directory { 0o700 } else { 0o600 };
    if stat.st_uid != unsafe { libc::geteuid() }
        || stat.st_mode & libc::S_IFMT != expected
        || if readable_parent {
            stat.st_mode & 0o7022 != 0 || stat.st_mode & 0o700 != 0o700
        } else {
            stat.st_mode & 0o7777 != mode
        }
        || (!directory && stat.st_nlink != 1)
    {
        return Err(FileError::Storage);
    }
    Ok(())
}
fn identity(fd: &impl rustix::fd::AsFd) -> Result<(u64, u64)> {
    let s = checked(fs::fstat(fd))?;
    Ok((s.st_dev as u64, s.st_ino as u64))
}
fn same(parent: &OwnedFd, name: &str, held: &impl rustix::fd::AsFd) -> Result<()> {
    let named = checked(fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW))?;
    if (named.st_dev as u64, named.st_ino as u64) != identity(held)? {
        return Err(FileError::Storage);
    }
    Ok(())
}
pub(super) struct PrivateRoot {
    path: PathBuf,
    directory: OwnedFd,
    lock: File,
}
impl PrivateRoot {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let directory = checked(crate::atomic_file::open_data_directory(path, false))?;
        private(&directory, true)?;
        let lock = File::from(checked(fs::openat(
            &directory,
            LOCK,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ))?);
        private(&lock, false)?;
        checked(fs::flock(&lock, FlockOperation::NonBlockingLockExclusive))?;
        let root = Self {
            path: path.to_owned(),
            directory,
            lock,
        };
        root.validate()?;
        checked(fs::fsync(&root.directory))?;
        Ok(root)
    }
    pub(super) fn validate(&self) -> Result<()> {
        let named = checked(crate::atomic_file::open_data_directory(&self.path, false))?;
        private(&named, true)?;
        private(&self.directory, true)?;
        private(&self.lock, false)?;
        if identity(&named)? != identity(&self.directory)? {
            return Err(FileError::Storage);
        }
        same(&self.directory, LOCK, &self.lock)
    }
}
impl Drop for PrivateRoot {
    fn drop(&mut self) {
        let _ = fs::flock(&self.lock, FlockOperation::Unlock);
    }
}
pub(super) struct PrivateFile {
    kind: FileKind,
    owner: String,
    directory: OwnedFd,
    poisoned: bool,
    current: Cell<Option<Option<(u64, u64)>>>,
    #[cfg(test)]
    pub(super) failure: Option<Failure>,
    #[cfg(test)]
    pub(super) before_publish: Option<Box<dyn FnOnce() + Send>>,
}
impl PrivateFile {
    pub(super) fn open(root: &PrivateRoot, owner: &str, create: bool) -> Result<Option<Self>> {
        Self::open_kind(root, owner, create, FileKind::Storage)
    }
    pub(super) fn open_kind(
        root: &PrivateRoot,
        owner: &str,
        create: bool,
        kind: FileKind,
    ) -> Result<Option<Self>> {
        if !super::manifest::valid_plugin_id(owner) {
            return Err(FileError::Storage);
        }
        root.validate()?;
        let directory = match fs::openat(&root.directory, owner, DIR, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) if !create => return Ok(None),
            Err(Errno::NOENT) => {
                checked(fs::mkdirat(&root.directory, owner, Mode::RWXU))?;
                checked(fs::fsync(&root.directory))?;
                checked(fs::openat(&root.directory, owner, DIR, Mode::empty()))?
            }
            Err(_) => return Err(FileError::Storage),
        };
        let file = Self {
            kind,
            owner: owner.into(),
            directory,
            poisoned: false,
            current: Cell::new(None),
            #[cfg(test)]
            failure: None,
            #[cfg(test)]
            before_publish: None,
        };
        file.validate(root)?;
        file.discard_orphan(root)?;
        Ok(Some(file))
    }
    fn validate(&self, root: &PrivateRoot) -> Result<()> {
        if self.poisoned {
            return Err(FileError::CommitUncertain);
        }
        root.validate()?;
        private(&self.directory, true)?;
        same(&root.directory, &self.owner, &self.directory)
    }
    fn discard_orphan(&self, root: &PrivateRoot) -> Result<()> {
        self.validate(root)?;
        let fd = match fs::openat(&self.directory, self.kind.stage(), FILE, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(()),
            Err(_) => return Err(FileError::Storage),
        };
        let stage = File::from(fd);
        private(&stage, false)?;
        same(&self.directory, self.kind.stage(), &stage)?;
        self.validate(root)?;
        checked(fs::unlinkat(
            &self.directory,
            self.kind.stage(),
            AtFlags::empty(),
        ))?;
        checked(fs::fsync(&self.directory))?;
        self.validate(root)
    }
    fn bind_current(&self, value: Option<(u64, u64)>) -> Result<()> {
        if self.current.get().is_some_and(|expected| expected != value) {
            return Err(FileError::Storage);
        }
        self.current.set(Some(value));
        Ok(())
    }
    pub(super) fn read(&self, root: &PrivateRoot) -> Result<Option<Vec<u8>>> {
        self.validate(root)?;
        let fd = match fs::openat(&self.directory, self.kind.current(), FILE, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => {
                self.validate(root)?;
                self.bind_current(None)?;
                return Ok(None);
            }
            Err(_) => return Err(FileError::Storage),
        };
        let file = File::from(fd);
        private(&file, false)?;
        let before = checked(fs::fstat(&file))?;
        if before.st_size < 0 || before.st_size as u64 > MAX_DOCUMENT as u64 {
            return Err(FileError::Corrupt);
        }
        let mut bytes = Vec::new();
        checked(
            (&file)
                .take((MAX_DOCUMENT + 1) as u64)
                .read_to_end(&mut bytes),
        )?;
        if bytes.len() > MAX_DOCUMENT {
            return Err(FileError::Corrupt);
        }
        private(&file, false)?;
        let after = checked(fs::fstat(&file))?;
        if (
            before.st_size,
            before.st_mtime,
            before.st_mtime_nsec,
            before.st_ctime,
            before.st_ctime_nsec,
        ) != (
            after.st_size,
            after.st_mtime,
            after.st_mtime_nsec,
            after.st_ctime,
            after.st_ctime_nsec,
        ) || bytes.len() as i64 != after.st_size
        {
            return Err(FileError::Storage);
        }
        same(&self.directory, self.kind.current(), &file)?;
        self.validate(root)?;
        self.bind_current(Some(identity(&file)?))?;
        Ok(Some(bytes))
    }
    pub(super) fn commit(
        &mut self,
        root: &PrivateRoot,
        previous: Option<&[u8]>,
        bytes: &[u8],
    ) -> Result<()> {
        if bytes.len() > MAX_DOCUMENT {
            return Err(FileError::Corrupt);
        }
        if self.read(root)?.as_deref() != previous {
            return Err(FileError::Storage);
        }
        let fd = checked(fs::openat(
            &self.directory,
            self.kind.stage(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ))?;
        let mut stage = File::from(fd);
        let stage_identity = identity(&stage)?;
        let before = (|| {
            private(&stage, false)?;
            checked(stage.write_all(bytes))?;
            checked(stage.sync_all())?;
            #[cfg(test)]
            if let Some(hook) = self.before_publish.take() {
                hook();
            }
            #[cfg(test)]
            if self.failure == Some(Failure::BeforeRename) {
                self.failure = None;
                return Err(FileError::Storage);
            }
            if self.read(root)?.as_deref() != previous {
                return Err(FileError::Storage);
            }
            private(&stage, false)?;
            same(&self.directory, self.kind.stage(), &stage)?;
            checked(fs::renameat(
                &self.directory,
                self.kind.stage(),
                &self.directory,
                self.kind.current(),
            ))
        })();
        if let Err(error) = before {
            // Never clean an untrusted replacement or a detached root.
            if self.validate(root).is_ok()
                && private(&stage, false).is_ok()
                && same(&self.directory, self.kind.stage(), &stage).is_ok()
            {
                let _ = fs::unlinkat(&self.directory, self.kind.stage(), AtFlags::empty());
                let _ = fs::fsync(&self.directory);
            }
            return Err(error);
        }
        self.current.set(Some(Some(stage_identity)));
        let after = (|| {
            #[cfg(test)]
            if self.failure == Some(Failure::AfterRename) {
                self.failure = None;
                return Err(FileError::Storage);
            }
            checked(fs::fsync(&self.directory))?;
            if self.read(root)?.as_deref() != Some(bytes) {
                return Err(FileError::Storage);
            }
            Ok(())
        })();
        if after.is_err() {
            self.poisoned = true;
            return Err(FileError::CommitUncertain);
        }
        Ok(())
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    BeforeRename,
    AfterRename,
}

/// Only the trusted manager supplies fixed child names. Validate the existing
/// parent before creating anything; never chmod or follow an existing object.
pub(super) fn ensure_private_child(parent: &Path, name: &'static str) -> Result<PathBuf> {
    ensure_child(parent, name, false)
}
pub(super) fn ensure_management_root(appdata: &Path) -> Result<PathBuf> {
    ensure_child(appdata, "plugin-management-v1", true)
}
fn ensure_child(parent: &Path, name: &'static str, readable_parent: bool) -> Result<PathBuf> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(FileError::Storage);
    }
    let parent_fd = checked(crate::atomic_file::open_data_directory(parent, false))?;
    check_private(&parent_fd, true, readable_parent)?;
    match fs::mkdirat(&parent_fd, name, Mode::RWXU) {
        Ok(()) | Err(Errno::EXIST) => {}
        Err(_) => return Err(FileError::Storage),
    }
    let child = checked(fs::openat(&parent_fd, name, DIR, Mode::empty()))?;
    private(&child, true)?;
    same(&parent_fd, name, &child)?;
    let named_parent = checked(crate::atomic_file::open_data_directory(parent, false))?;
    check_private(&named_parent, true, readable_parent)?;
    if identity(&named_parent)? != identity(&parent_fd)? {
        return Err(FileError::Storage);
    }
    checked(fs::fsync(&parent_fd))?;
    Ok(parent.join(name))
}
