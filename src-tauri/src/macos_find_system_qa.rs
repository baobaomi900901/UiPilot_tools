//! Host-only isolated system-scope QA. The operator supplies previously verified directory
//! boundaries in the QA process environment. No client can nominate or broaden those roots.

use super::*;
use std::{
    ffi::{CStr, OsStr},
    os::unix::ffi::OsStrExt,
    ptr,
};

const DATA_MOUNT: &str = "/System/Volumes/Data";
const MAX_VERIFIED_ROOTS: usize = 8;

fn canonical_absolute(path: &Path) -> bool {
    let raw = path.as_os_str().as_bytes();
    raw.starts_with(b"/") && raw.len() <= 1024 && !raw.contains(&0)
        && !raw[1..].split(|byte| *byte == b'/')
            .any(|part| matches!(part, b"" | b"." | b".."))
}

fn verified_roots(raw: &OsStr) -> Result<Vec<PathBuf>, FindError> {
    if raw.as_bytes().len() > MAX_VERIFIED_ROOTS * 1025 { return Err(FindError::Denied); }
    let mut roots = Vec::new();
    for path in std::env::split_paths(raw) {
        if roots.len() >= MAX_VERIFIED_ROOTS || !canonical_absolute(&path)
            || roots.iter().any(|other: &PathBuf| other.starts_with(&path) || path.starts_with(other)) {
            return Err(FindError::Denied);
        }
        roots.push(path);
    }
    if roots.is_empty() { return Err(FindError::Denied); }
    Ok(roots)
}

fn account_home() -> Result<PathBuf, FindError> {
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0i8; 16 * 1024];
    let mut found = ptr::null_mut();
    // SAFETY: libc writes the passwd record into entry and its strings into buffer;
    // both remain live until the home path has been copied.
    let status = unsafe {
        libc::getpwuid_r(libc::geteuid(), entry.as_mut_ptr(), buffer.as_mut_ptr(), buffer.len(), &mut found)
    };
    if status != 0 || found.is_null() { return Err(FindError::Unavailable); }
    let raw = unsafe { (*found).pw_dir };
    if raw.is_null() { return Err(FindError::Unavailable); }
    let home = PathBuf::from(OsStr::from_bytes(unsafe { CStr::from_ptr(raw) }.to_bytes()));
    if std::env::var_os("HOME").as_deref() != Some(home.as_os_str())
        || Path::new(&objc2_foundation::NSHomeDirectory().to_string()) != home {
        return Err(FindError::Denied);
    }
    Ok(home)
}

fn ordinary_home(home: &Path, mounts: &Mounts) -> bool {
    canonical_absolute(home) && home.parent() == Some(Path::new("/Users"))
        && mounts.allows_path(home)
}

#[derive(Eq, PartialEq)]
struct MountId {
    path: PathBuf,
    fsid: [u8; 8],
}

impl MountId {
    fn from_fd(fd: &File) -> Result<Self, FindError> {
        let info = local_mount_info(fd)?;
        let bytes = info.f_mntonname.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        let path = PathBuf::from(OsString::from_vec(bytes));
        if !path.is_absolute() { return Err(FindError::Denied); }
        // Darwin fsid_t is exactly two initialized i32s. Compare its opaque eight bytes.
        let fsid = unsafe { std::mem::transmute::<libc::fsid_t, [u8; 8]>(info.f_fsid) };
        Ok(Self { path, fsid })
    }
}

pub(super) struct LocalHome {
    path: PathBuf,
    id: MountId,
    fd: File,
}

impl LocalHome {
    fn bind(path: PathBuf, mounts: &Mounts) -> Result<Self, FindError> {
        if !ordinary_home(&path, mounts) { return Err(FindError::Denied); }
        let fd = open_directory(&path)?;
        let id = MountId::from_fd(&fd)?;
        if !mounts.allows_path(&id.path) { return Err(FindError::Denied); }
        Ok(Self { path, id, fd })
    }

    pub(super) fn revalidate(&self, mounts: &Mounts) -> Result<(), FindError> {
        if account_home()? != self.path || !ordinary_home(&self.path, mounts)
            || !mounts.allows_path(&self.id.path) {
            return Err(FindError::Denied);
        }
        let current = open_directory(&self.path)?;
        let old = fstat(&self.fd).map_err(|_| FindError::Stale)?;
        let new = fstat(&current).map_err(|_| FindError::Stale)?;
        if MountId::from_fd(&current)? != self.id || (old.st_dev, old.st_ino) != (new.st_dev, new.st_ino) {
            return Err(FindError::Stale);
        }
        Ok(())
    }
}

fn physical_spelling(mount: &Path, root: &Path) -> Result<PathBuf, FindError> {
    if root.starts_with(mount) {
        Ok(root.to_owned())
    } else if mount == Path::new(DATA_MOUNT) {
        Ok(mount.join(root.strip_prefix("/").map_err(|_| FindError::Denied)?))
    } else {
        Err(FindError::Denied)
    }
}

fn physical_root(mount: &Path, root: &Path) -> Result<PathBuf, FindError> {
    let result = physical_spelling(mount, root)?;
    if result == mount { return Err(FindError::Denied); }
    Ok(result)
}

fn covers_home(root: &Path, physical_root: &Path, home: &Path, physical_home: &Path) -> bool {
    [home, physical_home].iter().any(|path| path.starts_with(root) || path.starts_with(physical_root))
}

fn on_same_mount(mounts: &Mounts, root: &Path, candidate: &Path) -> bool {
    mounts.allows_path(candidate) && candidate.starts_with(root)
        && !mounts.0.iter().any(|(nested, _)| nested != root && nested.starts_with(root)
            && candidate.starts_with(nested))
}

pub(super) struct MountAnchor {
    mount: MountId,
    fd: File,
    pub(super) physical_root: PathBuf,
    original_chain: FindPathIdentity,
}

impl MountAnchor {
    fn bind(path: &Path, root_fd: &File, mounts: &Mounts) -> Result<Self, FindError> {
        let mount = MountId::from_fd(root_fd)?;
        if !mounts.allows_path(&mount.path) { return Err(FindError::Denied); }
        let physical_root = physical_root(&mount.path, path)?;
        if !on_same_mount(mounts, &mount.path, &physical_root) { return Err(FindError::Denied); }
        let fd = open_directory(&mount.path)?;
        if MountId::from_fd(&fd)? != mount { return Err(FindError::Stale); }
        let relative = physical_root.strip_prefix(&mount.path).map_err(|_| FindError::Denied)?;
        let chain = FindPathIdentity::capture(&fd, relative).map_err(|_| FindError::Denied)?;
        let root_stat = fstat(root_fd).map_err(|_| FindError::Denied)?;
        let mount_stat = fstat(&fd).map_err(|_| FindError::Denied)?;
        if !chain.is_directory() || chain.leaf_inode() != root_stat.st_ino || mount_stat.st_dev != root_stat.st_dev {
            return Err(FindError::Denied);
        }
        let mut prefix = mount.path.clone();
        for part in relative.components() {
            prefix.push(part);
            check_url(&prefix)?;
        }
        Ok(Self { mount, fd, physical_root, original_chain: chain })
    }

    pub(super) fn revalidate(&self, root: &Root, mounts: &Mounts) -> Result<(), FindError> {
        if !mounts.allows_path(&root.path)
            || !on_same_mount(mounts, &self.mount.path, &self.physical_root) {
            return Err(FindError::Denied);
        }
        let current = root.current_fd()?;
        if MountId::from_fd(&current)? != self.mount { return Err(FindError::Stale); }
        let relative = self.physical_root.strip_prefix(&self.mount.path).map_err(|_| FindError::Stale)?;
        let chain = FindPathIdentity::capture(&self.fd, relative).map_err(|_| FindError::Stale)?;
        if !chain.same_location(&self.original_chain) { return Err(FindError::Stale); }
        let mut prefix = self.mount.path.clone();
        for part in relative.components() {
            prefix.push(part);
            check_url(&prefix)?;
        }
        Ok(())
    }

    pub(super) fn allows_candidate(&self, mounts: &Mounts, root: &Path, path: &Path) -> bool {
        let Ok(suffix) = path.strip_prefix(root) else { return false; };
        on_same_mount(mounts, root, path)
            && on_same_mount(mounts, &self.physical_root, &self.physical_root.join(suffix))
    }

    pub(super) fn verify_leaf(&self, suffix: &Path, identity: &FindPathIdentity) -> Result<(), FindError> {
        let physical = self.physical_root.join(suffix);
        let relative = physical.strip_prefix(&self.mount.path).map_err(|_| FindError::Stale)?;
        let physical_identity = FindPathIdentity::capture(&self.fd, relative).map_err(|_| FindError::Stale)?;
        if identity.leaf_inode() != physical_identity.leaf_inode() { return Err(FindError::Stale); }
        Ok(())
    }
}

pub(super) fn current_evidence() -> Result<Vec<PathBuf>, FindError> {
    if std::env::var_os("UIPILOT_FIND_ISOLATED_QA").as_deref() != Some(OsStr::new("1")) {
        return Err(FindError::Unavailable);
    }
    let raw = std::env::var_os("UIPILOT_FIND_QA_VERIFIED_ROOTS").ok_or(FindError::Denied)?;
    verified_roots(&raw)
}

pub(super) fn isolated_scope() -> Result<IndexedLocalScope, FindError> {
    let paths = current_evidence()?;
    let mounts = Mounts::current()?;
    let home = LocalHome::bind(account_home()?, &mounts)?;
    let physical_home = physical_spelling(&home.id.path, &home.path)?;
    let mut roots: Vec<Arc<Root>> = Vec::new();
    for path in paths {
        // Reject broad roots in either spelling before probing them.
        if covers_home(&path, &path, &home.path, &physical_home) || !mounts.allows_path(&path) {
            return Err(FindError::Denied);
        }
        let fd = open_directory(&path)?;
        let anchor = MountAnchor::bind(&path, &fd, &mounts)?;
        // Recheck using the verified mount of this root, independent of its input spelling.
        if covers_home(&path, &anchor.physical_root, &home.path, &physical_home) {
            return Err(FindError::Denied);
        }
        for existing in &roots {
            let old = existing.anchor.as_ref().ok_or(FindError::Denied)?;
            if anchor.physical_root.starts_with(&old.physical_root)
                || old.physical_root.starts_with(&anchor.physical_root) { return Err(FindError::Denied); }
        }
        roots.push(Arc::new(Root { path, fd, anchor: Some(anchor) }));
    }
    Ok(IndexedLocalScope { roots, qa_home: Some(home) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_overlapping_and_broad_operator_evidence_is_rejected_without_io() {
        for value in ["", "/", "relative", "/Users/a/../b", "/foo//bar", "/foo/", "/x:/x/y", "file:///x", "/x:"] {
            assert!(verified_roots(OsStr::new(value)).is_err(), "{value}");
        }
        let many = (0..9).map(|i| format!("/root{i}")).collect::<Vec<_>>().join(":");
        assert!(verified_roots(OsStr::new(&many)).is_err());
        assert_eq!(verified_roots(OsStr::new("/Users/test/Projects")).unwrap().len(), 1);
    }

    #[test]
    fn whole_home_and_its_ancestors_are_denied_in_both_firmlink_spellings() {
        let home = Path::new("/Users/qa");
        let physical = physical_spelling(Path::new(DATA_MOUNT), home).unwrap();
        for broad in ["/Users", "/Users/qa", DATA_MOUNT, "/System/Volumes/Data/Users", "/System/Volumes/Data/Users/qa"] {
            assert!(covers_home(Path::new(broad), Path::new(broad), home, &physical));
        }
        // The real mount-derived spelling also wins if an unrelated input spelling hid the alias.
        assert!(covers_home(Path::new("/other/spelling"), &physical, home, &physical));
        for narrow in ["/Users/qa/Projects", "/Users/qa2", "/System/Volumes/Data/Users/qa/Projects"] {
            assert!(!covers_home(Path::new(narrow), Path::new(narrow), home, &physical));
        }
    }

    #[test]
    fn local_mount_routing_handles_firmlinks_remote_homes_and_crossings() {
        let roots = Mounts(vec![("/".into(), true), (DATA_MOUNT.into(), true),
            ("/Users/shared/net".into(), false), ("/Users/remote".into(), false),
            ("/Users/test/Projects/extra".into(), true)]);
        let stacked = Mounts(vec![("/".into(), true), ("/Users/test".into(), true), ("/Users/test".into(), true)]);
        assert!(!ordinary_home(Path::new("/Users/test"), &stacked));
        assert!(ordinary_home(Path::new("/Users/test"), &roots));
        assert!(!ordinary_home(Path::new("/Users/remote"), &roots));
        assert!(!ordinary_home(Path::new("/Network/test"), &roots));
        let root = Path::new("/Users/test/Projects");
        assert_eq!(physical_root(Path::new(DATA_MOUNT), root).unwrap(), Path::new("/System/Volumes/Data/Users/test/Projects"));
        assert!(on_same_mount(&roots, root, &root.join("file")));
        assert!(!on_same_mount(&roots, root, &root.join("extra/file")));
        assert!(!on_same_mount(&roots, root, Path::new("/Users/remote/doc")));
        assert!(physical_root(Path::new("/Volumes/local"), root).is_err());
    }

    #[test]
    fn binds_actual_local_mount_anchor_and_denies_unverified_siblings() {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root_path = parent.join(format!("uipilot-find-qa-{}", std::process::id()));
        std::fs::create_dir(&root_path).unwrap();
        let fd = open_directory(&root_path).unwrap();
        let mounts = Mounts::current().unwrap();
        let anchor = MountAnchor::bind(&root_path, &fd, &mounts).unwrap();
        let physical = anchor.physical_root.clone();
        let root = Arc::new(Root { path: root_path.clone(), fd, anchor: Some(anchor) });
        // No real home lookup, user-directory probe or system-wide Spotlight request.
        let scope = IndexedLocalScope { roots: vec![root], qa_home: None };
        let doc = root_path.join("test.pdf");
        std::fs::write(&doc, b"document").unwrap();
        let (_, logical) = scope.routed_path(&physical.join("test.pdf")).unwrap();
        assert_eq!(logical, doc);
        assert!(scope.routed_path(&parent.join("private/outside.pdf")).is_err());
        assert!(scope.routed_path(&root_path.join("x/../private")).is_err());
        let row = authorize_row(&scope, doc, FileCategory::All).unwrap().unwrap();
        assert!(row.can_open_document);
        std::fs::remove_dir_all(root_path).unwrap();
    }
}
