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

#[derive(Debug, Eq, PartialEq)]
pub(super) struct QaAccount {
    pub(super) name: String,
    pub(super) uid: u32,
    pub(super) home: PathBuf,
}

fn qa_name(name: &str) -> bool {
    name == "uipilot-qa" || name.strip_prefix("uipilot-qa-").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

fn verified_account(uid: u32, effective_uid: u32, record_uid: u32, name: &str, home: &Path) -> Result<QaAccount, FindError> {
    if uid == 0 || effective_uid != uid || record_uid != uid || !qa_name(name)
        || home.as_os_str() != Path::new("/Users").join(name).as_os_str() {
        return Err(FindError::Denied);
    }
    Ok(QaAccount { name: name.into(), uid, home: home.to_owned() })
}

fn standard_membership(status: libc::c_int, member: libc::c_int) -> Result<(), FindError> {
    match (status, member) {
        (0, 0) => Ok(()),
        (0, 1) => Err(FindError::Denied),
        _ => Err(FindError::Unavailable),
    }
}

fn require_standard_account(uid: u32, primary_gid: libc::gid_t) -> Result<(), FindError> {
    unsafe extern "C" {
        fn mbr_uid_to_uuid(uid: libc::uid_t, uuid: *mut u8) -> libc::c_int;
        fn mbr_gid_to_uuid(gid: libc::gid_t, uuid: *mut u8) -> libc::c_int;
        fn mbr_check_membership(user: *const u8, group: *const u8, member: *mut libc::c_int) -> libc::c_int;
    }
    let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut buffer = vec![0i8; 16 * 1024];
    let mut found = ptr::null_mut();
    // SAFETY: getgrnam_r writes into the supplied record and scratch buffer;
    // only the GID is used while that storage is alive.
    let result = unsafe { libc::getgrnam_r(c"admin".as_ptr(), group.as_mut_ptr(),
        buffer.as_mut_ptr(), buffer.len(), &mut found) };
    if result != 0 || found.is_null() { return Err(FindError::Unavailable); }
    let admin_gid = unsafe { (*found).gr_gid };
    // A process launched with inherited or elevated group credentials must not
    // pass merely because its password record describes a standard account.
    let (gid, egid) = unsafe { (libc::getgid(), libc::getegid()) };
    if gid != primary_gid || egid != gid || gid == 0 || gid == admin_gid { return Err(FindError::Denied); }
    let count = unsafe { libc::getgroups(0, ptr::null_mut()) };
    if !(0..=1024).contains(&count) { return Err(FindError::Unavailable); }
    let mut groups = vec![0; count as usize];
    // SAFETY: provide space for count group IDs and check the returned length.
    let actual = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    if actual < 0 || actual > count { return Err(FindError::Unavailable); }
    if groups[..actual as usize].iter().any(|&group| group == 0 || group == admin_gid) {
        return Err(FindError::Denied);
    }
    let mut user_uuid = [0; 16];
    let mut group_uuid = [0; 16];
    let mut member = -1;
    // SAFETY: membership.h writes UUIDs into 16-byte arrays and one result
    // into member. Its lookup includes direct and nested admin membership.
    unsafe {
        if mbr_uid_to_uuid(uid, user_uuid.as_mut_ptr()) != 0 ||
            mbr_gid_to_uuid(admin_gid, group_uuid.as_mut_ptr()) != 0 {
            return Err(FindError::Unavailable);
        }
        let status = mbr_check_membership(user_uuid.as_ptr(), group_uuid.as_ptr(), &mut member);
        standard_membership(status, member)
    }
}

pub(super) fn qa_account() -> Result<QaAccount, FindError> {
    // SAFETY: these libc calls take no arguments and return the process user IDs.
    let (uid, euid) = unsafe { (libc::getuid(), libc::geteuid()) };
    if uid == 0 || uid != euid { return Err(FindError::Denied); }
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0i8; 16 * 1024];
    let mut found = ptr::null_mut();
    // SAFETY: libc writes the record into entry and strings into buffer. Both
    // remain live until the verified short name and home path have been copied.
    let status = unsafe {
        libc::getpwuid_r(euid, entry.as_mut_ptr(), buffer.as_mut_ptr(), buffer.len(), &mut found)
    };
    if status != 0 || found.is_null() { return Err(FindError::Unavailable); }
    let record = unsafe { &*found };
    if record.pw_name.is_null() || record.pw_dir.is_null() { return Err(FindError::Unavailable); }
    let name = unsafe { CStr::from_ptr(record.pw_name) }.to_str().map_err(|_| FindError::Denied)?;
    let home = Path::new(OsStr::from_bytes(unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes()));
    let account = verified_account(uid, euid, record.pw_uid, name, home)?;
    require_standard_account(uid, record.pw_gid)?;
    if std::env::var_os("HOME").as_deref() != Some(home.as_os_str())
        || Path::new(&objc2_foundation::NSHomeDirectory().to_string()).as_os_str() != home.as_os_str() {
        return Err(FindError::Denied);
    }
    Ok(account)
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
    account: QaAccount,
    id: MountId,
    fd: File,
}

impl LocalHome {
    pub(super) fn bind(account: QaAccount, mounts: &Mounts) -> Result<Self, FindError> {
        if !ordinary_home(&account.home, mounts) { return Err(FindError::Denied); }
        let fd = open_directory(&account.home)?;
        let stat = fstat(&fd).map_err(|_| FindError::Denied)?;
        if stat.st_uid != account.uid || stat.st_mode & 0o022 != 0 { return Err(FindError::Denied); }
        let id = MountId::from_fd(&fd)?;
        if !mounts.allows_path(&id.path) { return Err(FindError::Denied); }
        Ok(Self { account, id, fd })
    }

    pub(super) fn revalidate(&self, mounts: &Mounts) -> Result<(), FindError> {
        if qa_account()? != self.account || !ordinary_home(&self.account.home, mounts)
            || !mounts.allows_path(&self.id.path) {
            return Err(FindError::Denied);
        }
        let current = open_directory(&self.account.home)?;
        let old = fstat(&self.fd).map_err(|_| FindError::Stale)?;
        let new = fstat(&current).map_err(|_| FindError::Stale)?;
        if new.st_uid != self.account.uid || new.st_mode & 0o022 != 0 || MountId::from_fd(&current)? != self.id
            || (old.st_dev, old.st_ino) != (new.st_dev, new.st_ino) {
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

const SHARED: &str = "/Users/Shared";

fn shared_fixture(name: &OsStr) -> bool {
    name.as_bytes().strip_prefix(b"uipilot-system-qa-").is_some_and(|uuid| {
        uuid.len() == 32 && uuid.iter().all(u8::is_ascii_hexdigit)
    })
}

#[derive(Eq, PartialEq)]
enum RootPolicy { Home, Shared }

fn root_policy(path: &Path, home: &Path) -> Result<RootPolicy, FindError> {
    if !canonical_absolute(path) { return Err(FindError::Denied); }
    if path != home && path.starts_with(home) { return Ok(RootPolicy::Home); }
    let mut relative = path.strip_prefix(SHARED).map_err(|_| FindError::Denied)?.components();
    match (relative.next(), relative.next()) {
        (Some(Component::Normal(fixture)), Some(Component::Normal(allowed)))
            if shared_fixture(fixture) && allowed == "allowed" => Ok(RootPolicy::Shared),
        _ => Err(FindError::Denied),
    }
}

// A mode-0700 fixture can still have an extended ACL granting other users
// access. Require operator-created Shared fixture directories to have no ACL.
pub(super) fn no_extended_acl(fd: &File) -> bool {
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    const ACL_TYPE_EXTENDED: libc::c_int = 0x100;
    // SAFETY: the descriptor is live. Darwin returns NULL/ENOENT for no ACL;
    // a returned ACL belongs to the caller and is freed exactly once.
    let acl = unsafe { acl_get_fd_np(fd.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() { return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT); }
    unsafe { acl_free(acl); }
    false
}

fn check_component(current: &File, uid: u32, policy: &RootPolicy) -> Result<(), FindError> {
    let stat = fstat(current).map_err(|_| FindError::Denied)?;
    if stat.st_uid != uid || stat.st_mode & 0o022 != 0 { return Err(FindError::Denied); }
    if *policy == RootPolicy::Shared && (stat.st_mode & 0o7777 != 0o700 || !no_extended_acl(current)) {
        return Err(FindError::Denied);
    }
    Ok(())
}

impl LocalHome {
    fn open_root(&self, path: &Path, mounts: &Mounts) -> Result<File, FindError> {
        let policy = root_policy(path, &self.account.home)?;
        let base = if policy == RootPolicy::Home { &self.account.home } else { Path::new(SHARED) };
        if !mounts.allows_root(path) { return Err(FindError::Denied); }
        // Verify each ancestor before opening its children. In particular, an
        // unrelated user's Shared fixture cannot cause a probe inside its tree.
        let mut current = match policy {
            RootPolicy::Home => rustix::io::fcntl_dupfd_cloexec(&self.fd, 0).map_err(|_| FindError::Denied)?.into(),
            RootPolicy::Shared => {
                let base_fd = open_directory(base)?;
                let stat = fstat(&base_fd).map_err(|_| FindError::Denied)?;
                if stat.st_uid != 0 || (stat.st_mode & 0o022 != 0 && stat.st_mode & 0o1000 == 0) {
                    return Err(FindError::Denied);
                }
                base_fd
            }
        };
        let base_mount = MountId::from_fd(&current)?;
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        for part in path.strip_prefix(base).map_err(|_| FindError::Denied)?.components() {
            let Component::Normal(name) = part else { return Err(FindError::Denied); };
            current = openat(&current, name, flags, Mode::empty()).map_err(|_| FindError::Denied)?.into();
            check_component(&current, self.account.uid, &policy)?;
            if MountId::from_fd(&current)? != base_mount { return Err(FindError::Denied); }
        }
        let physical_root = physical_spelling(&base_mount.path, path)?;
        let physical_base = physical_spelling(&base_mount.path, base)?;
        if physical_root == physical_base || !physical_root.starts_with(&physical_base) {
            return Err(FindError::Denied);
        }
        Ok(current)
    }

    pub(super) fn check_bound_root(&self, root: &Root, mounts: &Mounts) -> Result<(), FindError> {
        let walked = self.open_root(&root.path, mounts)?;
        let actual = fstat(&walked).map_err(|_| FindError::Denied)?;
        let bound = fstat(&root.fd).map_err(|_| FindError::Denied)?;
        if (actual.st_dev, actual.st_ino) != (bound.st_dev, bound.st_ino) { return Err(FindError::Stale); }
        Ok(())
    }
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
    let raw = std::env::var_os("UIPILOT_FIND_QA_VERIFIED_ROOTS");
    #[cfg(any(test, feature = "macos-find-qa-probe"))]
    let raw = probe::apply_probe_withdrawal(raw);
    let raw = raw.ok_or(FindError::Denied)?;
    verified_roots(&raw)
}

pub(super) fn isolated_scope() -> Result<IndexedLocalScope, FindError> {
    let account = qa_account()?;
    let paths = current_evidence()?;
    for path in &paths { root_policy(path, &account.home)?; }
    let mounts = Mounts::current()?;
    let home = LocalHome::bind(account, &mounts)?;
    let mut roots: Vec<Arc<Root>> = Vec::new();
    for path in paths {
        // Reject broad roots in either spelling before probing them.
        if !mounts.allows_path(&path) { return Err(FindError::Denied); }
        let fd = home.open_root(&path, &mounts)?;
        let anchor = MountAnchor::bind(&path, &fd, &mounts)?;
        for existing in &roots {
            let old = existing.anchor.as_ref().ok_or(FindError::Denied)?;
            if anchor.physical_root.starts_with(&old.physical_root)
                || old.physical_root.starts_with(&anchor.physical_root) { return Err(FindError::Denied); }
        }
        roots.push(Arc::new(Root { path, fd, anchor: Some(anchor) }));
    }
    Ok(IndexedLocalScope { roots, documents: None, qa_home: Some(home), qa_retired: false.into() })
}

#[cfg(any(test, feature = "macos-find-qa-probe"))]
#[path = "macos_find_system_qa_probe.rs"]
pub(crate) mod probe;

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
    fn only_real_non_root_dedicated_account_names_and_matching_homes_are_eligible() {
        for name in ["uipilot-qa", "uipilot-qa-test01"] {
            assert!(verified_account(501, 501, 501, name, &Path::new("/Users").join(name)).is_ok());
        }
        for name in ["owner", "uipilot-qa-", "uipilot-qa-TEST", "uipilot-qa-foo-bar", "uipilot-qax"] {
            assert!(verified_account(501, 501, 501, name, &Path::new("/Users").join(name)).is_err());
        }
        assert_eq!(standard_membership(0, 0), Ok(()));
        assert_eq!(standard_membership(0, 1), Err(FindError::Denied));
        assert!(standard_membership(0, -1).is_err());
        assert!(standard_membership(libc::ENOENT, 0).is_err());
        let home = Path::new("/Users/uipilot-qa");
        for ids in [(0,0,0),(501,0,0),(501,501,502)] {
            assert!(verified_account(ids.0, ids.1, ids.2, "uipilot-qa", home).is_err());
        }
        assert!(verified_account(501,501,501,"uipilot-qa",Path::new("/Users/personal")).is_err());
    }

    #[test]
    fn root_policy_refuses_broad_foreign_or_physical_evidence_without_io() {
        let home = Path::new("/Users/uipilot-qa");
        assert!(root_policy(&home.join("Projects"), home) == Ok(RootPolicy::Home));
        let fixture = Path::new(SHARED).join("uipilot-system-qa-abcdef0123456789abcdef0123456789");
        assert!(root_policy(&fixture.join("allowed"), home) == Ok(RootPolicy::Shared));
        for denied in [home.to_owned(), "/Users".into(), "/Users/another-user/project".into(),
            "/System/Library/Fonts".into(), SHARED.into(), fixture.clone(), fixture.join("outside"),
            Path::new(DATA_MOUNT).join("Users/uipilot-qa/Projects"),
            Path::new(DATA_MOUNT).join("Users/Shared").join(fixture.file_name().unwrap()).join("allowed"),
            Path::new(SHARED).join("uipilot-system-qa-short/allowed"), home.join("Projects/../private")] {
            assert!(root_policy(&denied, home).is_err(), "{denied:?}");
        }
    }

    #[test]
    fn shared_fixture_rechecks_owner_mode_and_positive_acl_on_held_descriptor() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().canonicalize().unwrap()
            .join(format!("uipilot-qa-component-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let fd = open_directory(&root).unwrap();
        let uid = fstat(&fd).unwrap().st_uid;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(check_component(&fd, uid, &RootPolicy::Shared).is_ok());
        assert!(check_component(&fd, uid.wrapping_add(1), &RootPolicy::Shared).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(check_component(&fd, uid, &RootPolicy::Shared).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Only this synthetic directory receives an ACL. Its POSIX mode still
        // looks private, but a fresh acquisition or revalidation must reject it.
        assert!(std::process::Command::new("/bin/chmod").args(["+a", "everyone allow read"])
            .arg(&root).status().unwrap().success());
        assert!(check_component(&fd, uid, &RootPolicy::Shared).is_err());
        std::fs::remove_dir(root).unwrap();
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
        let scope = IndexedLocalScope { roots: vec![root], documents: None, qa_home: None, qa_retired: false.into() };
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
