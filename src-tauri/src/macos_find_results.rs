//! Only this module turns private Spotlight candidates into displayable rows and native actions.
//! Scope roots are supplied by the host. An immutable file-reference URL, never a displayed
//! path or a client-supplied URL, is submitted after the controller's final ticket admission.

#[path = "file_search/extensions.rs"]
mod extensions;
use extensions::FileCategory;
#[path = "macos_find_documents.rs"]
mod documents;
pub(crate) use documents::DocumentsGrant;
#[cfg(all(feature = "macos-find-preview", not(feature = "macos-find-system-qa")))]
pub(crate) use documents::documents_directory_url;
#[cfg(feature = "macos-find-system-qa")]
#[path = "macos_find_system_qa.rs"]
mod system_qa;

use crate::{
    macos_find_identity::FindPathIdentity,
    macos_find_query::{Candidates, QueryCancellation},
};
use block2::RcBlock;
use chrono::{DateTime, SecondsFormat, Utc};
use objc2::{rc::{autoreleasepool, Retained}, runtime::{AnyObject, NSObject, NSObjectProtocol}};
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{
    NSArray, NSError, NSFileManager, NSNumber, NSThread, NSURLFileIdentifierKey, NSURLVolumeURLKey, NSURLVolumeIdentifierKey, NSURLIsAliasFileKey, NSURLIsPackageKey,
    NSURLIsReadableKey, NSURLResourceKey, NSURLVolumeIsLocalKey, NSURL,
};
use rustix::fs::{fstat, open, openat, Mode, OFlags};
use std::{
    collections::HashSet,
    fs::File,
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    os::fd::AsRawFd,
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc},
    time::Duration,
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FindError { Unavailable, InvalidInput, Cancelled, TimedOut, Stale, Denied }

struct Mounts(Vec<(PathBuf, bool)>);

impl Mounts {
    fn current() -> Result<Self, FindError> {
        // MNT_NOWAIT returns kernel-cached mount information without probing remote mounts.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if !(1..=4096).contains(&count) { return Err(FindError::Unavailable); }
        let slots = count as usize + 1;
        let mut mounts: Vec<libc::statfs> = Vec::with_capacity(slots);
        // SAFETY: the allocated buffer can hold exactly the number of bytes passed here.
        let filled = unsafe {
            libc::getfsstat(mounts.as_mut_ptr(), (slots * size_of::<libc::statfs>()) as i32, libc::MNT_NOWAIT)
        };
        if filled < 1 || filled as usize >= slots { return Err(FindError::Unavailable); }
        // SAFETY: getfsstat initialized this many full statfs entries.
        unsafe { mounts.set_len(filled as usize) };
        let mut entries = Vec::new();
        for entry in mounts {
            let path_bytes: Vec<u8> = entry.f_mntonname.iter().take_while(|&&c| c != 0)
                .map(|&c| c as u8).collect();
            let path = PathBuf::from(OsString::from_vec(path_bytes));
            if !path.is_absolute() { return Err(FindError::Unavailable); }
            let fs_type = entry.f_fstypename.iter().map(|c| *c as u8).take_while(|c| *c != 0);
            let local = entry.f_flags & libc::MNT_LOCAL as u32 != 0 && !fs_type.eq(b"autofs".iter().copied());
            // Data-volume mount points can also be visible at firmlink spellings like /Users.
            if let Ok(suffix) = path.strip_prefix("/System/Volumes/Data") {
                // The data volume itself does not replace the sealed system root at "/".
                if !suffix.as_os_str().is_empty() {
                    entries.push((Path::new("/").join(suffix), local));
                }
            }
            entries.push((path, local));
        }
        Ok(Self(entries))
    }

    fn allows_path(&self, path: &Path) -> bool {
        let mut ancestors = HashSet::new();
        for (mount, local) in &self.0 {
            if path.starts_with(mount) && (!local || !ancestors.insert(mount)) {
                // A remote, autofs or multiply stacked mount cannot establish a unique local root.
                return false;
            }
        }
        !ancestors.is_empty()
    }

    fn allows_root(&self, root: &Path) -> bool {
        self.allows_path(root)
            && !self.0.iter().any(|(mount, _)| mount != root && mount.starts_with(root))
    }
}

fn local_mount_info(fd: &File) -> Result<libc::statfs, FindError> {
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fstatfs borrows this valid descriptor and writes one struct on success.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), info.as_mut_ptr()) } != 0 { return Err(FindError::Denied); }
    let info = unsafe { info.assume_init() };
    if info.f_flags & libc::MNT_LOCAL as u32 == 0 { return Err(FindError::Denied); }
    Ok(info)
}

fn ensure_local_fd(fd: &File) -> Result<(), FindError> {
    local_mount_info(fd).map(|_| ())
}

struct Root {
    path: PathBuf,
    // Keep the original mount and directory alive until the last result has been retired.
    fd: File,
    #[cfg(feature = "macos-find-system-qa")]
    anchor: Option<system_qa::MountAnchor>,
}

fn open_directory(path: &Path) -> Result<File, FindError> {
    if !path.is_absolute() { return Err(FindError::Denied); }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = open("/", flags, Mode::empty()).map_err(|_| FindError::Denied)?;
    for part in path.components() {
        match part {
            Component::RootDir => continue,
            Component::Normal(name) => fd = openat(&fd, name, flags, Mode::empty()).map_err(|_| FindError::Denied)?,
            _ => return Err(FindError::Denied),
        }
    }
    Ok(fd.into())
}

fn resource(url: &NSURL, key: &NSURLResourceKey) -> Result<Retained<AnyObject>, FindError> {
    let mut value = None;
    // SAFETY: the output is a valid object slot; each caller checks its documented dynamic type.
    unsafe { url.getResourceValue_forKey_error(&mut value, key) }.map_err(|_| FindError::Denied)?;
    value.ok_or(FindError::Denied)
}

fn url_flag(url: &NSURL, key: &NSURLResourceKey) -> Result<bool, FindError> {
    Ok(resource(url, key)?.downcast::<NSNumber>().map_err(|_| FindError::Denied)?.boolValue())
}

fn check_url(path: &Path) -> Result<Retained<NSURL>, FindError> {
    let url = NSURL::from_file_path(path).ok_or(FindError::Denied)?;
    check_resources(&url)?;
    Ok(url)
}

fn check_resources(url: &NSURL) -> Result<(), FindError> {
    // SAFETY: these are Foundation's immutable exported resource keys.
    unsafe {
        if !url_flag(url, NSURLVolumeIsLocalKey)? || url_flag(url, NSURLIsPackageKey)?
            || url_flag(url, NSURLIsAliasFileKey)? || NSFileManager::defaultManager().isUbiquitousItemAtURL(url)
            || !url_flag(url, NSURLIsReadableKey)? {
            return Err(FindError::Denied);
        }
    }
    Ok(())
}

pub(crate) struct IndexedLocalScope {
    documents: Option<Arc<DocumentsGrant>>,
    roots: Vec<Arc<Root>>,
    #[cfg(feature = "macos-find-system-qa")]
    qa_home: Option<system_qa::LocalHome>,
}

impl IndexedLocalScope {
    pub(crate) fn for_host() -> Result<Self, FindError> {
        Self::with_documents(None)
    }

    pub(crate) fn with_documents(grant: Option<Arc<DocumentsGrant>>) -> Result<Self, FindError> {
        if !cfg!(feature = "macos-find-preview") {
            return Err(FindError::Unavailable);
        }
        #[cfg(feature = "macos-find-system-qa")]
        {
            let _ = grant;
            system_qa::isolated_scope()
        }
        #[cfg(not(feature = "macos-find-system-qa"))]
        Self::bind_with_documents(&[
            Path::new("/System/Library/Fonts"), Path::new("/Library/Fonts"), Path::new("/Users/Shared"),
        ], grant)
    }

    fn add_documents(&mut self, grant: Option<Arc<DocumentsGrant>>) {
        if let Some(grant) = grant.filter(|grant| grant.is_active()) {
            self.roots.push(Arc::clone(&grant.root));
            self.documents = Some(grant);
        }
    }

    fn recheck_documents(&self) -> Result<(), FindError> {
        if self.documents.as_ref().is_some_and(|grant| !grant.is_active()) {
            return Err(FindError::Stale);
        }
        Ok(())
    }

    fn bind(paths: &[&Path]) -> Result<Self, FindError> {
        Self::bind_with_documents(paths, None)
    }

    fn bind_with_documents(paths: &[&Path], grant: Option<Arc<DocumentsGrant>>) -> Result<Self, FindError> {
        let mounts = Mounts::current()?;
        let mut roots = Vec::new();
        for &path in paths {
            if !mounts.allows_root(path) { continue; }
            let Ok(fd) = open_directory(path) else { continue };
            if ensure_local_fd(&fd).is_ok() && check_url(path).is_ok() {
                roots.push(Arc::new(Root { path: path.to_owned(), fd,
                    #[cfg(feature = "macos-find-system-qa")] anchor: None }));
            }
        }
        let mut scope = Self { roots, documents: None, #[cfg(feature = "macos-find-system-qa")] qa_home: None };
        scope.add_documents(grant);
        if scope.roots.is_empty() { return Err(FindError::Unavailable); }
        Ok(scope)
    }

    pub(crate) fn query_paths(&self) -> Result<Vec<PathBuf>, FindError> {
        self.recheck_documents()?;
        let mounts = Mounts::current()?;
        let mut paths = Vec::new();
        #[cfg(feature = "macos-find-system-qa")]
        self.recheck_qa_evidence(&mounts)?;
        for root in &self.roots {
            #[cfg(feature = "macos-find-system-qa")]
            if let Some(anchor) = &root.anchor {
                if anchor.revalidate(root, &mounts).is_err() { return Err(FindError::Stale); }
                paths.push(root.path.clone());
                // Spotlight may return the canonical Data-volume spelling of a firmlink.
                if anchor.physical_root != root.path { paths.push(anchor.physical_root.clone()); }
                continue;
            }
            if mounts.allows_root(&root.path) && root.current_fd().is_ok() {
                paths.push(root.path.clone());
            }
        }
        if paths.is_empty() { return Err(FindError::Unavailable); }
        Ok(paths)
    }

    #[cfg(feature = "macos-find-system-qa")]
    fn recheck_qa_evidence(&self, mounts: &Mounts) -> Result<(), FindError> {
        if let Some(home) = &self.qa_home {
            let current = system_qa::current_evidence()?;
            if !current.iter().eq(self.roots.iter().map(|root| &root.path)) {
                return Err(FindError::Stale);
            }
            home.revalidate(mounts)?;
        }
        Ok(())
    }

    #[cfg(feature = "macos-find-system-qa")]
    pub(crate) fn uses_system_query(&self) -> bool { self.qa_home.is_some() }

    fn routed_path(&self, path: &Path) -> Result<(&Arc<Root>, PathBuf), FindError> {
        // Metadata paths are attacker-controlled. Reject ambiguous spellings lexically
        // before reading mount data, descriptors or any resource on that path.
        let canonical: PathBuf = path.components().collect();
        if !path.is_absolute() || canonical.as_os_str() != path.as_os_str()
            || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(FindError::Denied);
        }
        for root in &self.roots {
            if path.starts_with(&root.path) {
                if self.documents.as_ref().is_some_and(|grant| Arc::ptr_eq(root, &grant.root) && grant.is_revoked()) {
                    return Err(FindError::Stale);
                }
                return Ok((root, path.to_owned()));
            }
            #[cfg(feature = "macos-find-system-qa")]
            if let Some(anchor) = &root.anchor {
                if let Ok(suffix) = path.strip_prefix(&anchor.physical_root) {
                    return Ok((root, root.path.join(suffix)));
                }
            }
        }
        Err(FindError::Denied)
    }
}

impl Root {
    fn current_fd(&self) -> Result<File, FindError> {
        let current = open_directory(&self.path)?;
        let original = fstat(&self.fd).map_err(|_| FindError::Stale)?;
        let now = fstat(&current).map_err(|_| FindError::Stale)?;
        if (original.st_dev, original.st_ino) != (now.st_dev, now.st_ino) {
            return Err(FindError::Stale);
        }
        ensure_local_fd(&current)?;
        Ok(current)
    }

    fn validate(&self, path: &Path) -> Result<(FindPathIdentity, Retained<NSURL>), FindError> {
        let mounts = Mounts::current()?;
        #[cfg(feature = "macos-find-system-qa")]
        let qa_anchor = self.anchor.as_ref();
        #[cfg(feature = "macos-find-system-qa")]
        if let Some(anchor) = qa_anchor {
            anchor.revalidate(self, &mounts)?;
            if !anchor.allows_candidate(&mounts, &self.path, path) { return Err(FindError::Denied); }
        }
        #[cfg(feature = "macos-find-system-qa")]
        let needs_scoped_check = qa_anchor.is_none();
        #[cfg(not(feature = "macos-find-system-qa"))]
        let needs_scoped_check = true;
        if needs_scoped_check && !mounts.allows_root(&self.path) { return Err(FindError::Denied); }
        let relative = path.strip_prefix(&self.path).map_err(|_| FindError::Denied)?;
        let current = self.current_fd()?;
        let identity = FindPathIdentity::capture(&current, relative).map_err(|_| FindError::Denied)?;
        #[cfg(feature = "macos-find-system-qa")]
        if let Some(anchor) = &self.anchor {
            anchor.verify_leaf(relative, &identity)?;
        }
        check_url(&self.path)?;
        let mut cursor = self.path.clone();
        for (index, part) in relative.components().enumerate() {
            let Component::Normal(name) = part else { return Err(FindError::Denied) };
            cursor.push(name);
            let url = NSURL::from_file_path(&cursor).ok_or(FindError::Denied)?;
            let reference = url.fileReferenceURL().filter(|u| u.isFileReferenceURL())
                .ok_or(FindError::Denied)?;
            // Check package/alias flags on the immutable object captured by the descriptor walk,
            // even when another process exchanges two directories during resource lookup.
            let expected_inode = identity.inode_at_depth(index + 1).ok_or(FindError::Stale)?;
            verify_reference(&reference, &cursor, expected_inode, &current)?;
            if cursor == path {
                identity.revalidate(&current).map_err(|_| FindError::Stale)?;
                return Ok((identity, reference));
            }
        }
        Err(FindError::Denied)
    }
}

fn verify_reference(reference: &NSURL, path: &Path, expected_inode: u64, root_fd: &File) -> Result<(), FindError> {
    // Use a fresh resource cache for the *reference* URL, not a path URL recreated from the
    // display string. A path round trip alone cannot detect a swapped node at that string.
    let reference_string = reference.absoluteString().ok_or(FindError::Stale)?;
    let fresh = NSURL::URLWithString(&reference_string).filter(|url| url.isFileReferenceURL()).ok_or(FindError::Stale)?;
    fresh.removeAllCachedResourceValues();
    if !objc2::available!(macos = 13.3) { return Err(FindError::Denied); }
    // SAFETY: the inode key is documented as NSNumber; volume keys below are immutable Foundation constants.
    let inode = resource(&fresh, unsafe { NSURLFileIdentifierKey })?
        .downcast::<NSNumber>().map_err(|_| FindError::Denied)?.unsignedLongLongValue();
    if inode != expected_inode { return Err(FindError::Stale); }
    check_resources(&fresh)?;

    // APFS system/data firmlinks share a logical volume in NSURL. Its persistent UUID can
    // differ from the physical snapshot UUID returned by fgetattrlist. Compare Foundation's
    // opaque volume IDs on the reference and its own volume root, then bind that volume
    // to the POSIX device of our retained root descriptor.
    let volume_url = resource(&fresh, unsafe { NSURLVolumeURLKey })?
        .downcast::<NSURL>().map_err(|_| FindError::Denied)?;
    let volume_id = resource(&fresh, unsafe { NSURLVolumeIdentifierKey })?
        .downcast::<NSObject>().map_err(|_| FindError::Denied)?;
    let volume_root_id = resource(&volume_url, unsafe { NSURLVolumeIdentifierKey })?;
    if !volume_id.isEqual(Some(&volume_root_id)) { return Err(FindError::Stale); }
    let volume_path = volume_url.filePathURL().and_then(|url| url.to_file_path()).ok_or(FindError::Denied)?;
    let volume_root = open_directory(&volume_path)?;
    ensure_local_fd(&volume_root)?;
    if fstat(&volume_root).map_err(|_| FindError::Denied)?.st_dev
        != fstat(root_fd).map_err(|_| FindError::Denied)?.st_dev {
        return Err(FindError::Stale);
    }
    let resolved = fresh.filePathURL().and_then(|url| url.to_file_path());
    if resolved.as_deref() == Some(path) { Ok(()) } else { Err(FindError::Stale) }
}

#[derive(Clone)]
pub(crate) struct AuthorizedFile {
    root: Arc<Root>,
    path: PathBuf,
    identity: Arc<FindPathIdentity>,
    reference: Retained<NSURL>,
}

pub(crate) struct AuthorizedRow {
    pub(crate) name: String,
    pub(crate) full_path: String,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_utc: String,
    pub(crate) is_directory: bool,
    pub(crate) can_open_document: bool,
    pub(crate) target: AuthorizedFile,
}

pub(crate) struct AuthorizedBatch {
    pub(crate) rows: Vec<AuthorizedRow>,
    pub(crate) partial: bool,
}

fn extension_category(path: &Path) -> Option<FileCategory> {
    FileCategory::from_extension(path.extension()?.to_str()?)
}

fn can_open_document(path: &Path, identity: &FindPathIdentity) -> bool {
    !identity.is_directory() && identity.mode & 0o111 == 0 && matches!(
        extension_category(path), Some(FileCategory::Excel | FileCategory::Word | FileCategory::Ppt | FileCategory::Pdf | FileCategory::Image | FileCategory::Video | FileCategory::Audio)
    )
}

fn authorize_row(scope: &IndexedLocalScope, path: PathBuf, category: FileCategory) -> Result<Option<AuthorizedRow>, FindError> {
    let (root, path) = scope.routed_path(&path)?;
    let (identity, reference) = root.validate(&path)?;
    let is_directory = identity.is_directory();
    let actual = if is_directory { Some(FileCategory::Folder) } else { extension_category(&path) };
    if category != FileCategory::All && Some(category) != actual {
        return Ok(None);
    }
    let name = path.file_name().and_then(|n| n.to_str()).ok_or(FindError::Denied)?.to_owned();
    let full_path = path.to_str().ok_or(FindError::Denied)?.to_owned();
    let (secs, nanos) = identity.modified;
    let modified_utc = DateTime::<Utc>::from_timestamp(secs, nanos.try_into().map_err(|_| FindError::Denied)?)
        .ok_or(FindError::Denied)?.to_rfc3339_opts(SecondsFormat::Nanos, true);
    let size_bytes = if is_directory { None } else { Some(identity.size.try_into().map_err(|_| FindError::Denied)?) };
    let can_open_document = can_open_document(&path, &identity);
    Ok(Some(AuthorizedRow {
        name, full_path, modified_utc, size_bytes, is_directory, can_open_document,
        target: AuthorizedFile { root: root.clone(), path, identity: Arc::new(identity), reference },
    }))
}

pub(crate) fn authorize_batch(scope: &IndexedLocalScope, candidates: Candidates, category: &str, cancel: &QueryCancellation) -> Result<AuthorizedBatch, FindError> {
    let category = FileCategory::parse_wire(category).ok_or(FindError::InvalidInput)?;
    cancel.check().map_err(|_| FindError::Cancelled)?;
    scope.recheck_documents()?;
    let mut rows = Vec::new();
    let mut partial = candidates.partial || candidates.paths.len() > 400;
    let mut seen = HashSet::new();
    for candidate in candidates.paths.into_iter().take(400) {
        cancel.check().map_err(|_| FindError::Cancelled)?;
        let Ok((_, path)) = scope.routed_path(&candidate) else {
            partial = true;
            continue;
        };
        if !seen.insert(path.clone()) { continue; }
        match autoreleasepool(|_| authorize_row(scope, path, category)) {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => (),
            Err(_) => partial = true,
        }
    }
    rows.sort_by(|a, b| b.target.identity.modified.cmp(&a.target.identity.modified)
        .then_with(|| a.full_path.cmp(&b.full_path)));
    partial |= rows.len() > 200;
    rows.truncate(200);
    scope.recheck_documents()?;
    #[cfg(feature = "macos-find-system-qa")]
    if scope.uses_system_query() { scope.query_paths()?; }
    cancel.check().map_err(|_| FindError::Cancelled)?;
    if candidates.timed_out && rows.is_empty() { return Err(FindError::TimedOut); }
    Ok(AuthorizedBatch { rows, partial })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FindAction { Default, OpenDocument }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Submission { FileRevealRequested, FolderOpenRequested, FileOpenRequested, Unconfirmed }

fn submit_with<T>(
    scope: &IndexedLocalScope, target: &AuthorizedFile, action: FindAction,
    admit: impl FnOnce() -> bool, submit: impl FnOnce(&NSURL, Submission) -> T,
) -> Result<T, FindError> {
    scope.recheck_documents()?;
    let (root, _) = scope.routed_path(&target.path).map_err(|_| FindError::Stale)?;
    if !Arc::ptr_eq(root, &target.root) { return Err(FindError::Stale); }
    let (current, _) = root.validate(&target.path)?;
    if current != *target.identity { return Err(FindError::Stale); }
    let submission = match action {
        FindAction::OpenDocument if can_open_document(&target.path, &current) => Submission::FileOpenRequested,
        FindAction::OpenDocument => return Err(FindError::Denied),
        FindAction::Default if current.is_directory() => Submission::FolderOpenRequested,
        FindAction::Default => Submission::FileRevealRequested,
    };
    verify_reference(&target.reference, &target.path, current.leaf_inode(), &root.fd)?;
    #[cfg(feature = "macos-find-system-qa")]
    if scope.uses_system_query() { scope.recheck_qa_evidence(&Mounts::current()?)?; }
    scope.recheck_documents()?;
    if !admit() { return Err(FindError::Stale); }
    let dispatch = || submit(&target.reference, submission);
    // Controller admission has finished. Only the grant's own mutex covers the native
    // submission, so revocation can mark it stale without taking this mutex under host locks.
    if let Some(grant) = &scope.documents {
        grant.submit_if_active(dispatch)
    } else {
        Ok(dispatch())
    }
}

enum PendingSubmission {
    Complete(Submission),
    Awaiting(Submission, mpsc::Receiver<bool>),
}

impl PendingSubmission {
    fn confirmation(self) -> Submission {
        match self {
            Self::Complete(outcome) => outcome,
            Self::Awaiting(outcome, callback) => {
                if callback.recv_timeout(Duration::from_secs(2)) == Ok(true) {
                    outcome
                } else {
                    Submission::Unconfirmed
                }
            }
        }
    }
}

pub(crate) fn submit_action(
    scope: &IndexedLocalScope, target: &AuthorizedFile, action: FindAction, admit: impl FnOnce() -> bool,
) -> Result<Submission, FindError> {
    if NSThread::isMainThread_class() { return Err(FindError::Unavailable); }
    let pending = autoreleasepool(|_| submit_with(scope, target, action, admit, |reference, submission| {
        let workspace = NSWorkspace::sharedWorkspace();
        let urls = NSArray::<NSURL>::from_slice(&[reference]);
        if submission == Submission::FileRevealRequested {
            workspace.activateFileViewerSelectingURLs(&urls);
            return PendingSubmission::Complete(submission);
        }
        let config = NSWorkspaceOpenConfiguration::configuration();
        let (tx, rx) = mpsc::channel();
        let callback = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
            let _ = tx.send(!app.is_null() && error.is_null());
        });
        if submission == Submission::FolderOpenRequested {
            let finder = NSURL::fileURLWithPath_isDirectory(&objc2_foundation::NSString::from_str("/System/Library/CoreServices/Finder.app"), true);
            workspace.openURLs_withApplicationAtURL_configuration_completionHandler(&urls, &finder, &config, Some(&callback));
        } else {
            workspace.openURL_configuration_completionHandler(reference, &config, Some(&callback));
        }
        PendingSubmission::Awaiting(submission, rx)
    }))?;
    Ok(pending.confirmation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::{symlink, PermissionsExt}, sync::atomic::{AtomicU64, Ordering}};

    struct Fixture { path: PathBuf, scope: IndexedLocalScope }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let path = parent.join(format!("uipilot-find-row-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            let scope = IndexedLocalScope::bind(&[&path]).unwrap();
            Self { path, scope }
        }

        fn batch(&self, names: &[&str], category: &str) -> AuthorizedBatch {
            let candidates = Candidates {
                paths: names.iter().map(|name| self.path.join(name)).collect(),
                timed_out: false,
                partial: false,
            };
            authorize_batch(&self.scope, candidates, category, &QueryCancellation::default()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) { fs::remove_dir_all(&self.path).unwrap(); }
    }

    #[test]
    fn exports_only_verified_rows_and_reports_skipped_candidates() {
        let fixture = Fixture::new();
        fs::write(fixture.path.join("会议 1.PDF"), b"document").unwrap();
        fs::create_dir(fixture.path.join("Folder")).unwrap();
        symlink("会议 1.PDF", fixture.path.join("fake.pdf")).unwrap();
        fs::create_dir(fixture.path.join("Fake.app")).unwrap();
        fs::write(fixture.path.join("Fake.app/child.pdf"), b"child").unwrap();
        let batch = fixture.batch(&["会议 1.PDF", "Folder", "fake.pdf", "Fake.app/child.pdf", "../outside", "missing", "会议 1.PDF"], "all");
        assert!(batch.partial);
        assert_eq!(batch.rows.len(), 2);
        let file = batch.rows.iter().find(|r| r.name == "会议 1.PDF").unwrap();
        assert_eq!(file.size_bytes, Some(8));
        assert!(file.can_open_document && file.modified_utc.ends_with('Z'));
        assert!(batch.rows.iter().find(|r| r.is_directory).unwrap().size_bytes.is_none());
        let pdf = fixture.batch(&["Folder", "会议 1.PDF"], "pdf");
        assert_eq!(pdf.rows.len(), 1);
        assert!(!pdf.partial);
    }

    #[test]
    fn replaced_objects_or_rejected_admission_never_reach_workspace() {
        let fixture = Fixture::new();
        fs::write(fixture.path.join("a.pdf"), b"x").unwrap();
        let row = fixture.batch(&["a.pdf"], "all").rows.pop().unwrap();
        assert_eq!(submit_with(&fixture.scope, &row.target, FindAction::Default, || false, |_, _| panic!("stale ticket")), Err(FindError::Stale));
        fs::rename(fixture.path.join("a.pdf"), fixture.path.join("renamed")).unwrap();
        fs::write(fixture.path.join("a.pdf"), b"replacement").unwrap();
        assert!(submit_with(&fixture.scope, &row.target, FindAction::Default, || panic!("stale identity"), |_, _| panic!("replaced file")).is_err());
    }

    #[test]
    fn action_policy_precedes_admission_and_sends_only_native_file_references() {
        let fixture = Fixture::new();
        for name in ["a.PDF", "launch.pdf", "site.webloc", "script.sh"] {
            fs::write(fixture.path.join(name), b"x").unwrap();
        }
        fs::set_permissions(fixture.path.join("launch.pdf"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir(fixture.path.join("folder")).unwrap();
        let batch = fixture.batch(&["folder", "a.PDF", "launch.pdf", "site.webloc", "script.sh"], "all");
        assert_eq!(batch.rows.len(), 5);
        for row in batch.rows {
            let expected = if row.is_directory { Submission::FolderOpenRequested } else { Submission::FileRevealRequested };
            assert_eq!(submit_with(&fixture.scope, &row.target, FindAction::Default, || true, |reference, action| {
                assert!(reference.isFileReferenceURL());
                action
            }), Ok(expected));
            let expected = if row.name == "a.PDF" { Ok(Submission::FileOpenRequested) } else { Err(FindError::Denied) };
            assert_eq!(submit_with(&fixture.scope, &row.target, FindAction::OpenDocument, || row.can_open_document, |_, action| action), expected);
        }
    }

    #[test]
    fn denied_or_cancelled_rows_never_supply_timeout_evidence() {
        let fixture = Fixture::new();
        let missing = || Candidates { paths: vec![fixture.path.join("missing")], partial: true, timed_out: true };
        let cancel = QueryCancellation::default();
        assert_eq!(authorize_batch(&fixture.scope, missing(), "pdf", &cancel).err(), Some(FindError::TimedOut));
        assert_eq!(authorize_batch(&fixture.scope, missing(), "unknown", &cancel).err(), Some(FindError::InvalidInput));
        cancel.cancel();
        assert_eq!(authorize_batch(&fixture.scope, missing(), "all", &cancel).err(), Some(FindError::Cancelled));
    }

    #[cfg(feature = "macos-find-preview")]
    #[test]
    #[ignore = "requires Spotlight and the indexed public font sample"]
    fn indexed_preview_queries_and_authorizes_the_public_font() {
        use crate::macos_find_query::{gather_on_worker, ApprovedScope};
        // Keep this probe strictly on a public system sample even when preview adds other roots.
        let scope = IndexedLocalScope::bind(&[Path::new("/System/Library/Fonts")]).unwrap();
        let cancel = QueryCancellation::default();
        let candidates = gather_on_worker(ApprovedScope::IndexedLocal(&scope), "Andale Mono.ttf", &cancel).unwrap();
        let expected = "/System/Library/Fonts/Supplemental/Andale Mono.ttf";
        assert!(candidates.paths.contains(&PathBuf::from(expected)), "bridge candidates={} partial={}", candidates.paths.len(), candidates.partial);
        let batch = authorize_batch(&scope, candidates, "all", &cancel).unwrap();
        assert!(batch.rows.iter().any(|row| row.full_path == expected));
    }

    #[test]
    fn reference_inode_must_match_even_when_resolved_path_does() {
        let fixture = Fixture::new();
        let a = fixture.path.join("a");
        fs::write(&a, b"first").unwrap();
        fs::write(fixture.path.join("b"), b"second").unwrap();
        let root = &fixture.scope.roots[0];
        let b_identity = FindPathIdentity::capture(&root.fd, Path::new("b")).unwrap();
        let reference = NSURL::from_file_path(&a).unwrap().fileReferenceURL().unwrap();
        assert_eq!(verify_reference(&reference, &a, b_identity.leaf_inode(), &root.fd), Err(FindError::Stale));
    }

    #[test]
    fn reference_device_must_match_the_retained_root() {
        let fixture = Fixture::new();
        let path = fixture.path.join("a");
        fs::write(&path, b"first").unwrap();
        let root = &fixture.scope.roots[0];
        let identity = FindPathIdentity::capture(&root.fd, Path::new("a")).unwrap();
        let reference = NSURL::from_file_path(&path).unwrap().fileReferenceURL().unwrap();
        let other_device = File::open("/dev").unwrap();
        assert_eq!(verify_reference(&reference, &path, identity.leaf_inode(), &other_device), Err(FindError::Stale));
    }

    #[test]
    fn excludes_a_real_finder_alias_without_resolving_it() {
        use objc2_foundation::NSURLBookmarkCreationOptions;

        let fixture = Fixture::new();
        let target = fixture.path.join("real.pdf");
        fs::write(&target, b"x").unwrap();
        let source = NSURL::from_file_path(target).unwrap();
        let alias = NSURL::from_file_path(fixture.path.join("alias.pdf")).unwrap();
        let bookmark = source.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::SuitableForBookmarkFile, None, None).unwrap();
        NSURL::writeBookmarkData_toURL_options_error(&bookmark, &alias, 0).unwrap();
        let batch = fixture.batch(&["alias.pdf", "real.pdf"], "pdf");
        assert!(batch.partial);
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(batch.rows[0].name, "real.pdf");
    }

    #[test]
    fn inaccessible_permissions_are_rejected_before_visibility_and_execution() {
        let fixture = Fixture::new();
        let path = fixture.path.join("denied.pdf");
        fs::write(&path, b"x").unwrap();
        let row = fixture.batch(&["denied.pdf"], "all").rows.pop().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
        assert!(fixture.batch(&["denied.pdf"], "all").rows.is_empty());
        assert!(submit_with(&fixture.scope, &row.target, FindAction::Default, || panic!("revoked"), |_, _| panic!("revoked")).is_err());
    }

    #[test]
    fn shared_preview_scope_excludes_nonlocal_ancestors_and_nested_mounts() {
        let root = Path::new("/Users/Shared");
        let local = (PathBuf::from("/"), true);
        assert!(Mounts(vec![local.clone()]).allows_root(root));
        for (mount, is_local) in [("/Users", false), ("/Users/Shared", false), ("/Users/Shared/nested", false), ("/Users/Shared/local", true)] {
            assert!(!Mounts(vec![local.clone(), (mount.into(), is_local)]).allows_root(root));
        }
        assert!(Mounts(vec![local, ("/Users/SharedOther".into(), false)]).allows_root(root));
    }

    #[test]
    fn caps_verified_rows_and_orders_equal_timestamps_by_path() {
        let fixture = Fixture::new();
        let fixed = std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + Duration::from_secs(123));
        let mut paths = Vec::new();
        for index in (0..201).rev() {
            let path = fixture.path.join(format!("{index:03}.pdf"));
            File::create(&path).unwrap().set_times(fixed).unwrap();
            paths.push(path);
        }
        let candidates = Candidates { paths, partial: false, timed_out: false };
        let batch = authorize_batch(&fixture.scope, candidates, "pdf", &QueryCancellation::default()).unwrap();
        assert!(batch.partial);
        assert_eq!(batch.rows.len(), 200);
        assert_eq!(batch.rows[0].name, "000.pdf");
        assert_eq!(batch.rows[199].name, "199.pdf");
    }
}
