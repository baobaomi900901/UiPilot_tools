//! Descriptor-relative identity checks for host-owned Find candidates.
//!
//! The caller supplies the current, independently validated scope root on both passes.
//! An identity match is only one prerequisite for publishing or acting on a row.
//! The scope provider must check local-volume eligibility, packages, aliases, effective
//! access, scope revision and the native file-reference URL separately.

use rustix::fs::{accessat, fstat, openat, statat, Access, AtFlags, FileType, Mode, OFlags, Stat};
use std::{
    ffi::OsStr,
    fs::File,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

// Darwin <sys/stat.h>. Dataless provider objects must never be opened or hydrated by Find.
const SF_DATALESS: u32 = 0x4000_0000;

#[derive(Debug, Eq, PartialEq)]
struct ObjectId {
    device: i32,
    inode: u64,
    birth: (i64, i64),
    generation: u32,
}

impl From<&Stat> for ObjectId {
    fn from(stat: &Stat) -> Self {
        Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            birth: (stat.st_birthtime, stat.st_birthtime_nsec),
            generation: stat.st_gen,
        }
    }
}

#[derive(Eq, PartialEq)]
pub(crate) struct FindPathIdentity {
    relative: PathBuf,
    // Root, intermediate directories and leaf, in traversal order.
    chain: Vec<ObjectId>,
    kind: FileType,
    changed: (i64, i64),
    pub(crate) modified: (i64, i64),
    pub(crate) size: i64,
    pub(crate) mode: u16,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
    InvalidPath,
    Ineligible,
    Unavailable,
    Changed,
}

impl From<rustix::io::Errno> for IdentityError {
    fn from(_: rustix::io::Errno) -> Self {
        Self::Unavailable
    }
}

impl FindPathIdentity {
    pub(crate) fn capture(root: &File, relative: &Path) -> Result<Self, IdentityError> {
        // Validate raw components before Path can normalize interior "." or separators.
        let bytes = relative.as_os_str().as_bytes();
        let components = bytes.split(|byte| *byte == b'/');
        if bytes.contains(&0) || components.clone().any(|part| matches!(part, b"" | b"." | b"..")) {
            return Err(IdentityError::InvalidPath);
        }

        let root_stat = fstat(root)?;
        if FileType::from_raw_mode(root_stat.st_mode) != FileType::Directory {
            return Err(IdentityError::Ineligible);
        }
        let mut chain = vec![ObjectId::from(&root_stat)];
        let mut directory = rustix::io::fcntl_dupfd_cloexec(root, 0)?;
        let mut components = components.map(OsStr::from_bytes);
        let leaf = components.next_back().ok_or(IdentityError::InvalidPath)?;
        for component in components {
            directory = openat(
                &directory,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            let stat = fstat(&directory)?;
            if stat.st_dev != root_stat.st_dev || stat.st_flags & SF_DATALESS != 0 {
                return Err(IdentityError::Ineligible);
            }
            chain.push(ObjectId::from(&stat));
        }

        // Metadata only. Opening even a regular candidate for reading can hydrate a placeholder.
        let stat = statat(&directory, leaf, AtFlags::SYMLINK_NOFOLLOW)?;
        let kind = FileType::from_raw_mode(stat.st_mode);
        if stat.st_dev != root_stat.st_dev || stat.st_flags & SF_DATALESS != 0
            || !matches!(kind, FileType::Directory | FileType::RegularFile)
        {
            return Err(IdentityError::Ineligible);
        }
        let mut required = Access::READ_OK;
        if kind == FileType::Directory {
            required |= Access::EXEC_OK;
        }
        accessat(&directory, leaf, required, AtFlags::EACCESS | AtFlags::SYMLINK_NOFOLLOW)?;
        chain.push(ObjectId::from(&stat));
        Ok(Self {
            relative: relative.to_owned(),
            chain,
            kind,
            changed: (stat.st_ctime, stat.st_ctime_nsec),
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            size: stat.st_size,
            mode: stat.st_mode,
        })
    }

    pub(crate) fn same_location(&self, other: &Self) -> bool {
        self.chain == other.chain && self.kind == other.kind
    }

    pub(crate) fn inode_at_depth(&self, depth: usize) -> Option<u64> {
        self.chain.get(depth).map(|object| object.inode)
    }

    pub(crate) fn leaf_inode(&self) -> u64 {
        self.chain.last().expect("capture always records root and leaf").inode
    }

    pub(crate) fn is_directory(&self) -> bool {
        self.kind == FileType::Directory
    }

    pub(crate) fn revalidate(&self, current_root: &File) -> Result<(), IdentityError> {
        if Self::capture(current_root, &self.relative)? == *self {
            Ok(())
        } else {
            Err(IdentityError::Changed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(PathBuf, File);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "uipilot-find-id-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root.clone(), File::open(&root).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn revalidates_directory_and_file_with_literal_characters() {
        let root = Fixture::new();
        fs::create_dir(root.0.join("资料")).unwrap();
        let relative = Path::new("资料/会议 *'?.txt");
        fs::write(root.0.join(relative), b"v1").unwrap();
        for path in [Path::new("资料"), relative] {
            let identity = FindPathIdentity::capture(&root.1, path).unwrap();
            assert_eq!(identity.revalidate(&root.1), Ok(()));
        }
    }

    #[test]
    fn rejects_traversal_and_special_objects() {
        let root = Fixture::new();
        for path in ["", "/absolute", "./name", "a/./b", "a/../b", "a//b", "a/", "a\0b"] {
            assert_eq!(
                FindPathIdentity::capture(&root.1, Path::new(path)).err(),
                Some(IdentityError::InvalidPath)
            );
        }
        symlink("missing", root.0.join("link")).unwrap();
        assert_eq!(
            FindPathIdentity::capture(&root.1, Path::new("link")).err(),
            Some(IdentityError::Ineligible)
        );
        let devices = File::open("/dev").unwrap();
        assert_eq!(
            FindPathIdentity::capture(&devices, Path::new("null")).err(),
            Some(IdentityError::Ineligible)
        );
    }

    #[test]
    fn rejects_a_mount_at_the_leaf_or_intermediate_directory() {
        // macOS mounts devfs at /dev. This probes only metadata and opens no device.
        let system_root = File::open("/").unwrap();
        for path in ["dev", "dev/null"] {
            assert_eq!(
                FindPathIdentity::capture(&system_root, Path::new(path)).err(),
                Some(IdentityError::Ineligible)
            );
        }
    }

    #[test]
    fn detects_leaf_replacements_and_removal() {
        let root = Fixture::new();
        let leaf = root.0.join("a");
        fs::write(&leaf, b"first").unwrap();
        let identity = FindPathIdentity::capture(&root.1, Path::new("a")).unwrap();
        fs::rename(&leaf, root.0.join("original")).unwrap();
        fs::write(&leaf, b"second").unwrap();
        assert_eq!(identity.revalidate(&root.1), Err(IdentityError::Changed));
        fs::remove_file(&leaf).unwrap();
        assert_eq!(identity.revalidate(&root.1), Err(IdentityError::Unavailable));
        symlink("original", &leaf).unwrap();
        assert_eq!(identity.revalidate(&root.1), Err(IdentityError::Ineligible));
    }

    #[test]
    fn rejects_replaced_parent_even_when_the_leaf_keeps_its_inode() {
        let root = Fixture::new();
        let relative = Path::new("dir/leaf");
        fs::create_dir(root.0.join("dir")).unwrap();
        fs::create_dir(root.0.join("replacement")).unwrap();
        fs::write(root.0.join(relative), b"x").unwrap();
        fs::hard_link(root.0.join(relative), root.0.join("replacement/leaf")).unwrap();
        let identity = FindPathIdentity::capture(&root.1, relative).unwrap();
        fs::rename(root.0.join("dir"), root.0.join("old")).unwrap();
        symlink("old", root.0.join("dir")).unwrap();
        assert!(identity.revalidate(&root.1).is_err());
        fs::remove_file(root.0.join("dir")).unwrap();
        fs::rename(root.0.join("replacement"), root.0.join("dir")).unwrap();
        assert_eq!(identity.revalidate(&root.1), Err(IdentityError::Changed));
    }

    #[test]
    fn rejects_replaced_scope_root_with_identical_leaf() {
        let original = Fixture::new();
        let replacement = Fixture::new();
        fs::write(original.0.join("a"), b"x").unwrap();
        fs::hard_link(original.0.join("a"), replacement.0.join("a")).unwrap();
        let identity = FindPathIdentity::capture(&original.1, Path::new("a")).unwrap();
        assert_eq!(identity.revalidate(&replacement.1), Err(IdentityError::Changed));
    }
}
