//! Private, text-only storage. No native clipboard or plugin runtime is installed.
use std::{collections::BTreeSet, fs::File, io::{Read, Write}, os::fd::AsRawFd,
    path::{Component, Path, PathBuf}};
use rustix::{fd::OwnedFd, fs::{self, AtFlags, FlockOperation, Mode, OFlags}, io::Errno};
use super::{document, model::{HistoryDocument, HistoryEntryPayload, INDEX_SCHEMA, MAX_ENTRIES},
    preview::{text_fingerprint, text_preview}, CaptureOutcome, ClipboardHistorySnapshot};

pub(super) const MAX_TEXT: usize = 1024 * 1024;
pub(super) const MAX_INDEX: usize = 8 * 1024 * 1024;
const ROOT: &str = "clipboard-history";
const CURRENT: &str = "index.json";
const STAGE: &str = "index.pending";
const DIR_FLAGS: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StoreError { Storage, Corrupt, InvalidInput, InvalidQuery, CommitUncertain }
type Result<T> = std::result::Result<T, StoreError>;
fn storage<T, E>(result: std::result::Result<T, E>) -> Result<T> { result.map_err(|_| StoreError::Storage) }

pub(super) fn valid_id(id: &str) -> bool {
    crate::manifest::valid_plugin_id(id) && matches!(
        (Path::new(id).components().next(), Path::new(id).components().nth(1)),
        (Some(Component::Normal(_)), None))
}
fn no_acl(fd: &impl AsRawFd) -> bool {
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    // Darwin NULL/ENOENT means no extended ACL. Reject all supplied ACLs.
    let acl = unsafe { acl_get_fd_np(fd.as_raw_fd(), 0x100) };
    if acl.is_null() { return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT); }
    unsafe { acl_free(acl); }
    false
}
fn local(fd: &impl AsRawFd) -> bool {
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    unsafe { libc::fstatfs(fd.as_raw_fd(), stat.as_mut_ptr()) == 0
        && stat.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0 }
}
fn check_dir(fd: &OwnedFd, private: bool) -> Result<()> {
    let stat = storage(fs::fstat(fd))?;
    if stat.st_uid != unsafe { libc::geteuid() } || !local(fd) || !no_acl(fd)
        || (if private { stat.st_mode & 0o7777 != 0o700 } else { stat.st_mode & 0o022 != 0 }) {
        return Err(StoreError::Storage);
    }
    Ok(())
}
fn check_file(file: &File) -> Result<()> {
    let stat = storage(fs::fstat(file))?;
    if fs::FileType::from_raw_mode(stat.st_mode) != fs::FileType::RegularFile
        || stat.st_uid != unsafe { libc::geteuid() } || stat.st_mode & 0o7777 != 0o600
        || stat.st_nlink != 1 || !local(file) || !no_acl(file) { return Err(StoreError::Storage); }
    Ok(())
}
fn child(parent: &OwnedFd, name: &str) -> Result<OwnedFd> {
    match fs::mkdirat(parent, name, Mode::RWXU) {
        Ok(()) => { storage(fs::fsync(parent))?; },
        Err(Errno::EXIST) => {},
        Err(_) => return Err(StoreError::Storage),
    }
    let directory = storage(fs::openat(parent, name, DIR_FLAGS, Mode::empty()))?;
    check_dir(&directory, true)?;
    Ok(directory)
}
fn same_named(parent: &OwnedFd, name: &str, held: &OwnedFd) -> Result<()> {
    let named = storage(fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW))?;
    let held = storage(fs::fstat(held))?;
    if (named.st_dev, named.st_ino) != (held.st_dev, held.st_ino) { return Err(StoreError::Storage); }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct RootIdentity { parent: (u64, u64), root: (u64, u64) }
fn identity(fd: &OwnedFd) -> Result<(u64, u64)> {
    let stat = storage(fs::fstat(fd))?;
    Ok((stat.st_dev as u64, stat.st_ino as u64))
}

pub(super) struct MacTextStore {
    location: PathBuf, id: String, parent: OwnedFd, root: OwnedFd, directory: OwnedFd,
    document: HistoryDocument, raw: Option<Vec<u8>>, poisoned: bool,
    #[cfg(test)] pub(super) fail_next: Option<FailurePoint>,
    #[cfg(test)] pub(super) before_publish: Option<Box<dyn FnOnce() + Send>>,
}

impl MacTextStore {
    pub(super) fn prepare_root(location: &Path) -> Result<RootIdentity> {
        let parent = storage(crate::atomic_file::open_data_directory(location, false))?;
        check_dir(&parent, false)?;
        let root = child(&parent, ROOT)?;
        let named = storage(crate::atomic_file::open_data_directory(location, false))?;
        if identity(&named)? != identity(&parent)? { return Err(StoreError::Storage); }
        same_named(&parent, ROOT, &root)?;
        Ok(RootIdentity { parent: identity(&parent)?, root: identity(&root)? })
    }
    pub(super) fn open(location: &Path, id: &str) -> Result<Self> {
        Self::open_expected(location, id, None)
    }
    pub(super) fn open_bound(location: &Path, id: &str, expected: RootIdentity) -> Result<Self> {
        Self::open_expected(location, id, Some(expected))
    }
    fn open_expected(location: &Path, id: &str, expected: Option<RootIdentity>) -> Result<Self> {
        if !valid_id(id) { return Err(StoreError::InvalidInput); }
        let parent = storage(crate::atomic_file::open_data_directory(location, false))?;
        check_dir(&parent, false)?;
        let root = if let Some(expected) = expected {
            let root = storage(fs::openat(&parent, ROOT, DIR_FLAGS, Mode::empty()))?;
            check_dir(&root, true)?;
            if identity(&parent)? != expected.parent || identity(&root)? != expected.root {
                return Err(StoreError::Storage);
            }
            root
        } else { child(&parent, ROOT)? };
        let directory = child(&root, id)?;
        storage(fs::flock(&directory, FlockOperation::NonBlockingLockExclusive))?;
        let mut store = Self { location: location.to_owned(), id: id.to_owned(), parent, root, directory,
            document: HistoryDocument::default(), raw: None, poisoned: false,
            #[cfg(test)] fail_next: None, #[cfg(test)] before_publish: None };
        store.validate_location()?;
        store.discard_orphan_stage()?;
        let raw = store.read_current()?;
        if let Some(raw) = &raw {
            let document = serde_json::from_slice(raw).map_err(|_| StoreError::Corrupt)?;
            validate_document(&document)?;
            store.document = document;
        }
        store.raw = raw;
        Ok(store)
    }
    fn validate_location(&self) -> Result<()> {
        if self.poisoned { return Err(StoreError::CommitUncertain); }
        let named = storage(crate::atomic_file::open_data_directory(&self.location, false))?;
        check_dir(&named, false)?;
        let a = storage(fs::fstat(&named))?;
        let b = storage(fs::fstat(&self.parent))?;
        if (a.st_dev, a.st_ino) != (b.st_dev, b.st_ino) { return Err(StoreError::Storage); }
        check_dir(&self.parent, false)?;
        check_dir(&self.root, true)?;
        check_dir(&self.directory, true)?;
        same_named(&self.parent, ROOT, &self.root)?;
        same_named(&self.root, &self.id, &self.directory)
    }
    fn discard_orphan_stage(&self) -> Result<()> {
        self.validate_location()?;
        let fd = match fs::openat(&self.directory, STAGE,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC, Mode::empty()) {
            Ok(fd) => fd, Err(Errno::NOENT) => return Ok(()), Err(_) => return Err(StoreError::Storage),
        };
        let file = File::from(fd);
        check_file(&file)?;
        let held = storage(fs::fstat(&file))?;
        let named = storage(fs::statat(&self.directory, STAGE, AtFlags::SYMLINK_NOFOLLOW))?;
        if (held.st_dev, held.st_ino) != (named.st_dev, named.st_ino) { return Err(StoreError::Storage); }
        self.validate_location()?;
        // The fixed staging namespace is never read or restored. Only a verified
        // private regular single-link orphan can be discarded under flock.
        storage(fs::unlinkat(&self.directory, STAGE, AtFlags::empty()))?;
        storage(fs::fsync(&self.directory))?;
        self.validate_location()
    }
    fn read_current(&self) -> Result<Option<Vec<u8>>> {
        self.validate_location()?;
        let fd = match fs::openat(&self.directory, CURRENT,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC, Mode::empty()) {
            Ok(fd) => fd, Err(Errno::NOENT) => return Ok(None), Err(_) => return Err(StoreError::Storage),
        };
        let file = File::from(fd);
        check_file(&file)?;
        if storage(file.metadata())?.len() > MAX_INDEX as u64 { return Err(StoreError::Corrupt); }
        let mut bytes = Vec::new();
        storage((&file).take((MAX_INDEX + 1) as u64).read_to_end(&mut bytes))?;
        if bytes.len() > MAX_INDEX { return Err(StoreError::Corrupt); }
        check_file(&file)?;
        let held = storage(fs::fstat(&file))?;
        let named = storage(fs::statat(&self.directory, CURRENT, AtFlags::SYMLINK_NOFOLLOW))?;
        if (held.st_dev, held.st_ino) != (named.st_dev, named.st_ino) { return Err(StoreError::Storage); }
        self.validate_location()?;
        Ok(Some(bytes))
    }
    fn check_current(&self) -> Result<()> {
        if self.read_current()? != self.raw { return Err(StoreError::Storage); }
        Ok(())
    }
    pub(super) fn snapshot(&self, query: Option<&str>) -> Result<ClipboardHistorySnapshot> {
        if query.is_some_and(|query| query.chars().count() > 512) { return Err(StoreError::InvalidQuery); }
        self.check_current()?;
        let query = query.unwrap_or("").trim().to_lowercase();
        Ok(ClipboardHistorySnapshot { revision: self.document.revision.clone(), entries: self.document.entries.iter()
            .filter(|entry| match &entry.payload { HistoryEntryPayload::Text { text, .. } => text.to_lowercase().contains(&query), _ => false })
            .filter_map(document::text_summary).collect() })
    }
    pub(super) fn capture(&mut self, text: String, captured_at: String) -> Result<CaptureOutcome> {
        if text.len() > MAX_TEXT || captured_at.len() > 64 { return Err(StoreError::InvalidInput); }
        self.check_current()?;
        let mut candidate = self.document.clone();
        let mutation = document::capture_text(&mut candidate, text, captured_at).map_err(|_| StoreError::Corrupt)?;
        if matches!(mutation.outcome, CaptureOutcome::Unchanged { .. }) { return Ok(mutation.outcome); }
        self.persist(candidate)?;
        Ok(mutation.outcome)
    }
    pub(super) fn remove(&mut self, id: &str) -> Result<bool> {
        self.check_current()?;
        let mut candidate = self.document.clone();
        if !document::remove(&mut candidate, id).map_err(|_| StoreError::Corrupt)? { return Ok(false); }
        self.persist(candidate)?;
        Ok(true)
    }
    pub(super) fn clear(&mut self) -> Result<()> {
        self.check_current()?;
        let mut candidate = self.document.clone();
        document::clear(&mut candidate).map_err(|_| StoreError::Corrupt)?;
        self.persist(candidate)
    }
    fn persist(&mut self, mut candidate: HistoryDocument) -> Result<()> {
        let raw = loop {
            let raw = serde_json::to_vec(&candidate).map_err(|_| StoreError::Corrupt)?;
            if raw.len() <= MAX_INDEX { break raw; }
            if candidate.entries.len() <= 1 { return Err(StoreError::InvalidInput); }
            candidate.entries.pop();
        };
        validate_document(&candidate)?;
        self.check_current()?;
        let fd = storage(fs::openat(&self.directory, STAGE, OFlags::WRONLY | OFlags::CREATE
            | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::RUSR | Mode::WUSR))?;
        let mut stage = File::from(fd);
        let identity = storage(fs::fstat(&stage))?;
        let before = (|| {
            check_file(&stage)?;
            storage(stage.write_all(&raw))?;
            storage(stage.sync_all())?;
            #[cfg(test)] if let Some(hook) = self.before_publish.take() { hook(); }
            #[cfg(test)] if self.fail_next == Some(FailurePoint::BeforeRename) {
                self.fail_next = None; return Err(StoreError::Storage);
            }
            self.check_current()?;
            check_file(&stage)?;
            let named = storage(fs::statat(&self.directory, STAGE, AtFlags::SYMLINK_NOFOLLOW))?;
            if (identity.st_dev, identity.st_ino) != (named.st_dev, named.st_ino) { return Err(StoreError::Storage); }
            storage(fs::renameat(&self.directory, STAGE, &self.directory, CURRENT))
        })();
        if let Err(error) = before {
            if fs::statat(&self.directory, STAGE, AtFlags::SYMLINK_NOFOLLOW).is_ok_and(|named|
                (identity.st_dev, identity.st_ino) == (named.st_dev, named.st_ino)) {
                let _ = fs::unlinkat(&self.directory, STAGE, AtFlags::empty());
            }
            return Err(error);
        }
        let after = (|| {
            #[cfg(test)] if self.fail_next == Some(FailurePoint::AfterRename) {
                self.fail_next = None; return Err(StoreError::Storage);
            }
            storage(fs::fsync(&self.directory))?;
            self.validate_location()?;
            if self.read_current()?.as_deref() != Some(&raw) { return Err(StoreError::Storage); }
            Ok(())
        })();
        if after.is_err() { self.poisoned = true; return Err(StoreError::CommitUncertain); }
        self.document = candidate;
        self.raw = Some(raw);
        Ok(())
    }
}

impl Drop for MacTextStore {
    fn drop(&mut self) {
        // A forked child's inherited description may outlive this object. Revoke
        // the advisory lock explicitly before closing our authoritative handle.
        let _ = fs::flock(&self.directory, FlockOperation::Unlock);
    }
}

fn decimal(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0')) { return Err(StoreError::Corrupt); }
    value.parse().map_err(|_| StoreError::Corrupt)
}
fn validate_document(document: &HistoryDocument) -> Result<()> {
    if document.schema != INDEX_SCHEMA || document.entries.len() > MAX_ENTRIES { return Err(StoreError::Corrupt); }
    decimal(&document.revision)?;
    let next_id = decimal(&document.next_id)?;
    let next_rank = decimal(&document.next_recency_rank)?;
    if next_id == 0 || next_rank == 0 { return Err(StoreError::Corrupt); }
    let mut ids = BTreeSet::new(); let mut ranks = BTreeSet::new(); let mut fingerprints = BTreeSet::new();
    let mut previous_rank = u64::MAX;
    for entry in &document.entries {
        let id = decimal(&entry.id)?; let rank = decimal(&entry.recency_rank)?;
        if id == 0 || rank == 0 || id >= next_id || rank >= next_rank || rank > previous_rank
            || !ids.insert(id) || !ranks.insert(rank) || !fingerprints.insert(&entry.fingerprint) || entry.captured_at.len() > 64 {
            return Err(StoreError::Corrupt);
        }
        previous_rank = rank;
        let HistoryEntryPayload::Text { text, text_preview: preview } = &entry.payload else { return Err(StoreError::Corrupt); };
        if text.len() > MAX_TEXT || entry.fingerprint != text_fingerprint(text) || *preview != text_preview(text) {
            return Err(StoreError::Corrupt);
        }
    }
    Ok(())
}

#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FailurePoint { BeforeRename, AfterRename }
#[cfg(test)]
#[path = "macos_store_tests.rs"]
pub(super) mod tests;
