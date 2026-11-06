//! A bookmark is an access credential. No snapshots/backups or filenames supplied
//! by a renderer are used. Only the current record is restored after a restart.
use crate::atomic_file::open_data_directory;
use rustix::{
    fd::OwnedFd,
    fs::{fchmod, flock, fstat, fsync, mkdirat, openat, renameat, statat, unlinkat,
        AtFlags, FlockOperation, Mode, OFlags},
    io::Errno,
};
use sha2::{Digest, Sha256};
use std::{fs::File, io::{self, Read, Write}, os::unix::fs::MetadataExt, path::{Path, PathBuf}};

const DIRECTORY: &str = "find-documents-access";
const CURRENT: &str = "grant";
const STAGE: &str = "grant.pending";
const MARKER: &[u8] = b"uipilot-documents-v1\0";
const LIMIT: usize = 256 * 1024;
const MAX_FILE: usize = MARKER.len() + 1 + 32 + LIMIT;
const FLAGS: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

#[derive(Eq, PartialEq)]
pub(super) enum Record { Forgotten, NeedsAuthorization, Bookmark(Vec<u8>) }

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Forgotten => "Forgotten", Self::NeedsAuthorization => "NeedsAuthorization",
            Self::Bookmark(_) => "Bookmark(<private>)" })
    }
}

pub(super) struct Store { location: PathBuf, parent: OwnedFd, directory: OwnedFd }

fn unsafe_entry() -> io::Error { io::Error::other("private store unavailable") }

impl Store {
    pub(super) fn open(data_dir: &Path) -> io::Result<Self> {
        let parent = open_data_directory(data_dir, false)?;
        let stat = fstat(&parent)?;
        let uid = unsafe { libc::geteuid() };
        if stat.st_uid != uid || stat.st_mode & 0o022 != 0 { return Err(unsafe_entry()); }
        match mkdirat(&parent, DIRECTORY, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => (),
            Err(err) => return Err(err.into()),
        }
        let directory = openat(&parent, DIRECTORY, FLAGS, Mode::empty())?;
        let stat = fstat(&directory)?;
        if stat.st_uid != uid || stat.st_mode & 0o7777 != 0o700 { return Err(unsafe_entry()); }
        // Keep one authority per app data directory even if another binary or
        // launcher instance bypasses the normal single-instance startup.
        flock(&directory, FlockOperation::NonBlockingLockExclusive)?;
        fsync(&parent)?;
        let store = Self { location: data_dir.to_owned(), parent, directory };
        store.unlink_stage()?;
        Ok(store)
    }

    fn validate_directory(&self) -> io::Result<()> {
        let held_parent = fstat(&self.parent)?;
        let named_parent = fstat(open_data_directory(&self.location, false)?)?;
        if (held_parent.st_dev, held_parent.st_ino) != (named_parent.st_dev, named_parent.st_ino) ||
            named_parent.st_uid != unsafe { libc::geteuid() } || named_parent.st_mode & 0o022 != 0 {
            return Err(unsafe_entry());
        }
        let held = fstat(&self.directory)?;
        let named = statat(&self.parent, DIRECTORY, AtFlags::SYMLINK_NOFOLLOW)?;
        if (held.st_dev, held.st_ino) != (named.st_dev, named.st_ino) ||
            held.st_uid != unsafe { libc::geteuid() } || held.st_mode & 0o7777 != 0o700 {
            return Err(unsafe_entry());
        }
        Ok(())
    }

    fn unlink_stage(&self) -> io::Result<()> {
        match unlinkat(&self.directory, STAGE, AtFlags::empty()) {
            Ok(()) => { fsync(&self.directory)?; Ok(()) }
            Err(Errno::NOENT) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) fn read(&self) -> io::Result<Record> {
        self.validate_directory()?;
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let fd = match openat(&self.directory, CURRENT, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(Record::Forgotten),
            Err(err) => return Err(err.into()),
        };
        let file = File::from(fd);
        let meta = file.metadata()?;
        if !meta.is_file() || meta.mode() & 0o7777 != 0o600 || meta.uid() != unsafe { libc::geteuid() }
            || meta.nlink() != 1 { return Err(unsafe_entry()); }
        if meta.len() > MAX_FILE as u64 { return Ok(Record::NeedsAuthorization); }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes)?;
        Ok(decode(&bytes))
    }

    pub(super) fn stage(&self, record: &Record) -> io::Result<()> {
        self.validate_directory()?;
        let bytes = encode(record)?;
        self.unlink_stage()?;
        let fd = openat(&self.directory, STAGE,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR)?;
        let commit = || -> io::Result<()> {
            fchmod(&fd, Mode::RUSR | Mode::WUSR)?;
            let mut file = File::from(fd);
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(())
        };
        let result = commit();
        if result.is_err() { let _ = self.unlink_stage(); }
        result
    }

    // The stage is never restored. Only this rename belongs in the short
    // controller admission critical section; both sync calls run outside it.
    pub(super) fn publish(&self) -> io::Result<()> {
        self.validate_directory()?;
        renameat(&self.directory, STAGE, &self.directory, CURRENT)?;
        Ok(())
    }

    pub(super) fn sync(&self) -> io::Result<()> {
        fsync(&self.directory)?;
        self.validate_directory()
    }

    pub(super) fn write(&self, record: &Record) -> io::Result<()> {
        self.stage(record)?;
        self.publish()?;
        self.confirm(record)
    }

    pub(super) fn confirm(&self, record: &Record) -> io::Result<()> {
        self.sync()?;
        if &self.read()? != record { return Err(unsafe_entry()); }
        Ok(())
    }

    pub(super) fn discard_stage(&self) -> io::Result<()> { self.unlink_stage() }

}

fn encode(record: &Record) -> io::Result<Vec<u8>> {
    let (tag, body): (u8, &[u8]) = match record {
        Record::Forgotten => (0, &[]),
        Record::NeedsAuthorization => (1, &[]),
        Record::Bookmark(data) if !data.is_empty() && data.len() <= LIMIT => (2, data),
        Record::Bookmark(_) => return Err(unsafe_entry()),
    };
    let mut bytes = MARKER.to_vec();
    bytes.push(tag);
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Record {
    let header = MARKER.len() + 1;
    if bytes.len() < header + 32 || bytes.len() > MAX_FILE || !bytes.starts_with(MARKER) {
        return Record::NeedsAuthorization;
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - 32);
    if &Sha256::digest(payload)[..] != checksum { return Record::NeedsAuthorization; }
    match (payload[header - 1], &payload[header..]) {
        (0, []) => Record::Forgotten,
        (2, data) if !data.is_empty() => Record::Bookmark(data.to_vec()),
        _ => Record::NeedsAuthorization,
    }
}

#[cfg(test)]
#[path = "macos_find_bookmark_probe.rs"]
mod probe;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::{PermissionsExt, symlink}, sync::atomic::{AtomicU64, Ordering}};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "uipilot-bookmark-store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn private_atomic_record_restores_only_current_and_durably_forgets() {
        let temp = Temp::new();
        let store = Store::open(&temp.0).unwrap();
        assert!(Store::open(&temp.0).is_err());
        let record = Record::Bookmark(vec![77; 127]);
        store.write(&record).unwrap();
        assert_eq!(store.read().unwrap(), record);
        let path = temp.0.join(DIRECTORY).join(CURRENT);
        assert_eq!(path.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(path.parent().unwrap().metadata().unwrap().mode() & 0o777, 0o700);
        store.stage(&Record::Bookmark(vec![88; 10])).unwrap(); // interrupted before authorization commit
        drop(store);
        let reopened = Store::open(&temp.0).unwrap();
        assert_eq!(reopened.read().unwrap(), record);
        assert!(!path.with_file_name(STAGE).exists());
        reopened.write(&Record::Forgotten).unwrap();
        drop(reopened);
        assert_eq!(Store::open(&temp.0).unwrap().read().unwrap(), Record::Forgotten);
        assert!(!fs::read(&path).unwrap().windows(8).any(|b| b == [77; 8]));
    }

    #[test]
    fn malformed_or_oversized_records_never_reach_the_native_parser() {
        let temp = Temp::new();
        let store = Store::open(&temp.0).unwrap();
        store.write(&Record::Bookmark(vec![1,2,3])).unwrap();
        let path = temp.0.join(DIRECTORY).join(CURRENT);
        let mut bytes = fs::read(&path).unwrap();
        bytes[MARKER.len() + 1] ^= 0xff;
        fs::write(&path, bytes).unwrap();
        assert_eq!(store.read().unwrap(), Record::NeedsAuthorization);
        File::options().write(true).open(&path).unwrap().set_len(MAX_FILE as u64 + 10).unwrap();
        assert_eq!(store.read().unwrap(), Record::NeedsAuthorization);
        assert!(store.write(&Record::Bookmark(vec![0; LIMIT+1])).is_err());
    }

    #[test]
    fn refuses_insecure_links_and_substituted_parent_before_confirming_forget() {
        let temp = Temp::new();
        let parent = temp.0.join("app");
        fs::create_dir(&parent).unwrap();
        let outside = temp.0.join("other");
        fs::write(&outside, b"untouched").unwrap();
        let store = Store::open(&parent).unwrap();
        let file = parent.join(DIRECTORY).join(CURRENT);
        symlink(&outside, &file).unwrap();
        assert!(store.read().is_err());
        store.write(&Record::Forgotten).unwrap();
        assert_eq!(fs::read(&outside).unwrap(), b"untouched");
        fs::hard_link(&file, temp.0.join("linked")).unwrap();
        assert!(store.read().is_err());
        fs::remove_file(temp.0.join("linked")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.read().is_err());
        fs::rename(&parent, temp.0.join("retired")).unwrap();
        fs::create_dir(&parent).unwrap();
        assert!(store.read().is_err());
        assert!(store.write(&Record::Forgotten).is_err());
    }
}
