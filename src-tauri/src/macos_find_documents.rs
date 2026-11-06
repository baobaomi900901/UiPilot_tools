//! A session grant owns the exact URL selected in the native panel. It cannot be
//! reconstructed from a path, a saved preference, or a previous process's grant.

use super::*;
use objc2::Message;
use objc2_foundation::{NSHomeDirectory, NSSearchPathDirectory, NSSearchPathDomainMask};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

/// Called only while handling the user's request to show the Documents authorization panel.
pub(crate) fn documents_directory_url() -> Result<Retained<NSURL>, FindError> {
    if cfg!(feature = "macos-find-system-qa") || !cfg!(feature = "macos-find-preview") {
        return Err(FindError::Unavailable);
    }
    let urls = NSFileManager::defaultManager().URLsForDirectory_inDomains(
        NSSearchPathDirectory::DocumentDirectory, NSSearchPathDomainMask::UserDomainMask,
    );
    if urls.count() != 1 { return Err(FindError::Unavailable); }
    let url = urls.objectAtIndex(0);
    let path = url.to_file_path().ok_or(FindError::Denied)?;
    let home = PathBuf::from(NSHomeDirectory().to_string());
    let mounts = Mounts::current()?;
    // Redirected, remote or ambiguous Documents locations require a separately validated
    // policy. Never allow the panel's initial location to mount such a home on our behalf.
    if home.parent() != Some(Path::new("/Users"))
        || path.as_os_str() != home.join("Documents").as_os_str()
        || !mounts.allows_path(&home) || !mounts.allows_root(&path) {
        return Err(FindError::Denied);
    }
    let home_fd = open_directory(&home)?;
    ensure_local_fd(&home_fd)?;
    Ok(url)
}

struct PanelAccess {
    url: Retained<NSURL>,
    started: bool,
}

impl PanelAccess {
    fn new(selected: &NSURL) -> Self {
        // Apple grants non-sandboxed panel clients access through user selection as well.
        // A false return supplies no scoped-access lease; the descriptor checks below must
        // independently prove effective read access in both cases.
        // SAFETY: a successful start on this native panel URL is balanced by this guard's Drop.
        let started = unsafe { selected.startAccessingSecurityScopedResource() };
        Self { url: selected.retain(), started }
    }
}

impl Drop for PanelAccess {
    fn drop(&mut self) {
        if self.started {
            // SAFETY: this guard owns exactly one successful start call on the retained URL.
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

pub(crate) struct DocumentsGrant {
    pub(super) root: Arc<Root>,
    anchor_path: PathBuf,
    anchor_fd: File,
    identity: FindPathIdentity,
    reference: Retained<NSURL>,
    require_system_location: bool,
    revoked: AtomicBool,
    lease: Mutex<Option<PanelAccess>>,
}

impl DocumentsGrant {
    /// The caller passes the actual URL from a successful native panel response.
    pub(crate) fn from_selection(selected: &NSURL) -> Result<Arc<Self>, FindError> {
        let expected = documents_directory_url()?.to_file_path().ok_or(FindError::Denied)?;
        Self::bind_selected(&expected, selected, true)
    }

    #[cfg(test)]
    pub(crate) fn from_fixture(expected: &Path, selected: &NSURL) -> Result<Arc<Self>, FindError> {
        Self::bind_selected(expected, selected, false)
    }

    fn bind_selected(expected: &Path, selected: &NSURL, require_system_location: bool) -> Result<Arc<Self>, FindError> {
        let components: PathBuf = expected.components().collect();
        if !expected.is_absolute() || components.as_os_str() != expected.as_os_str()
            || expected.components().any(|part| part == Component::ParentDir)
            || !selected.isFileURL()
            || selected.to_file_path().as_deref().map(Path::as_os_str) != Some(expected.as_os_str()) {
            return Err(FindError::Denied);
        }
        if !Mounts::current()?.allows_root(expected) { return Err(FindError::Denied); }
        let lease = PanelAccess::new(selected);
        let anchor_path = expected.parent().and_then(Path::parent).ok_or(FindError::Denied)?;
        let anchor_fd = open_directory(anchor_path)?;
        ensure_local_fd(&anchor_fd)?;
        let relative = expected.strip_prefix(anchor_path).map_err(|_| FindError::Denied)?;
        let identity = FindPathIdentity::capture(&anchor_fd, relative).map_err(|_| FindError::Denied)?;
        if !identity.is_directory() { return Err(FindError::Denied); }
        let reference = selected.fileReferenceURL().filter(|url| url.isFileReferenceURL())
            .ok_or(FindError::Denied)?;
        let grant = Arc::new(Self {
            root: Arc::new(Root {
                path: expected.to_owned(),
                fd: open_directory(expected)?,
                #[cfg(feature = "macos-find-system-qa")]
                anchor: None,
            }),
            anchor_path: anchor_path.to_owned(), anchor_fd, identity, reference,
            require_system_location, revoked: AtomicBool::new(false), lease: Mutex::new(Some(lease)),
        });
        grant.revalidate()?;
        Ok(grant)
    }

    fn revalidate(&self) -> Result<(), FindError> {
        if self.revoked.load(Ordering::Acquire) { return Err(FindError::Stale); }
        if self.require_system_location
            && documents_directory_url()?.to_file_path().as_deref() != Some(self.root.path.as_path()) {
            return Err(FindError::Stale);
        }
        let mounts = Mounts::current()?;
        if !mounts.allows_root(&self.root.path) { return Err(FindError::Denied); }
        let current_anchor = open_directory(&self.anchor_path)?;
        let old = fstat(&self.anchor_fd).map_err(|_| FindError::Stale)?;
        let new = fstat(&current_anchor).map_err(|_| FindError::Stale)?;
        if (old.st_dev, old.st_ino) != (new.st_dev, new.st_ino) { return Err(FindError::Stale); }
        let relative = self.root.path.strip_prefix(&self.anchor_path).map_err(|_| FindError::Stale)?;
        let identity = FindPathIdentity::capture(&current_anchor, relative).map_err(|_| FindError::Denied)?;
        if !identity.same_location(&self.identity) { return Err(FindError::Stale); }
        let mut prefix = self.anchor_path.clone();
        for (index, part) in relative.components().enumerate() {
            prefix.push(part);
            if prefix == self.root.path { break; }
            let ancestor = NSURL::from_directory_path(&prefix).and_then(|url| url.fileReferenceURL())
                .filter(|url| url.isFileReferenceURL()).ok_or(FindError::Denied)?;
            let inode = identity.inode_at_depth(index + 1).ok_or(FindError::Stale)?;
            verify_reference(&ancestor, &prefix, inode, &current_anchor)?;
        }
        let directory = self.root.current_fd()?;
        let current = fstat(&directory).map_err(|_| FindError::Stale)?;
        if current.st_ino != identity.leaf_inode() || current.st_dev != new.st_dev { return Err(FindError::Stale); }
        verify_reference(&self.reference, &self.root.path, current.st_ino, &directory)?;
        if self.revoked.load(Ordering::Acquire) { return Err(FindError::Stale); }
        Ok(())
    }

    pub(crate) fn is_active(&self) -> bool {
        if self.revalidate().is_ok() { return true; }
        self.revoke();
        false
    }

    pub(super) fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// Retire admission under host state locks without calling AppKit or waiting for a worker.
    pub(crate) fn mark_revoked(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    /// Called outside host locks. The acknowledgment follows any native submission already in
    /// progress. Callback completion is not part of this critical section.
    pub(crate) fn revoke(&self) {
        self.mark_revoked();
        let mut lease = self.lease.lock().unwrap_or_else(|e| e.into_inner());
        // Keep the grant-local lock through stopAccessing so concurrent revokes are drained too.
        drop(lease.take());
    }

    pub(super) fn submit_if_active<T>(&self, submit: impl FnOnce() -> T) -> Result<T, FindError> {
        let lease = self.lease.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_revoked() || lease.is_none() { return Err(FindError::Stale); }
        let submitted = submit();
        drop(lease);
        Ok(submitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
        sync::atomic::AtomicU64,
    };

    struct Fixture {
        base: PathBuf,
        docs: PathBuf,
        shared: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "uipilot-doc-grant-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let docs = base.join("account/Documents");
            let shared = base.join("Shared");
            fs::create_dir_all(&docs).unwrap();
            fs::create_dir(&shared).unwrap();
            Self { base, docs, shared }
        }

        fn grant(&self) -> Arc<DocumentsGrant> {
            let url = NSURL::from_directory_path(&self.docs).unwrap();
            DocumentsGrant::bind_selected(&self.docs, &url, false).unwrap()
        }

        fn scope(&self, grant: Option<Arc<DocumentsGrant>>) -> IndexedLocalScope {
            let mut scope = IndexedLocalScope::bind(&[&self.shared]).unwrap();
            scope.add_documents(grant);
            scope
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) { fs::remove_dir_all(&self.base).unwrap(); }
    }

    #[test]
    fn only_exact_native_directory_selection_can_extend_the_public_scope() {
        let fixture = Fixture::new();
        let shared_url = NSURL::from_directory_path(&fixture.shared).unwrap();
        let wrong = DocumentsGrant::bind_selected(&fixture.docs, &shared_url, false);
        assert!(matches!(wrong, Err(FindError::Denied)));
        let no_grant = fixture.scope(None);
        assert_eq!(no_grant.query_paths().unwrap(), vec![fixture.shared.clone()]);
        fs::create_dir_all(fixture.docs.join("项目 🐋/资料")).unwrap();
        let file = fixture.docs.join("项目 🐋/资料/年终📄.pdf");
        fs::write(&file, b"sample").unwrap();
        let scope = fixture.scope(Some(fixture.grant()));
        let candidates = Candidates { paths: vec![file], partial: false, timed_out: false };
        let rows = authorize_batch(&scope, candidates, "pdf", &QueryCancellation::default()).unwrap().rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "年终📄.pdf");
    }

    #[test]
    fn unavailable_public_roots_do_not_prevent_a_valid_session_grant() {
        let fixture = Fixture::new();
        let missing = fixture.shared.join("missing");
        assert!(IndexedLocalScope::bind(&[&missing]).is_err());
        let scope = IndexedLocalScope::bind_with_documents(&[&missing], Some(fixture.grant())).unwrap();
        assert_eq!(scope.query_paths().unwrap(), vec![fixture.docs.clone()]);
    }

    #[test]
    fn revocation_retires_query_and_action_and_leaves_only_public_directories() {
        let fixture = Fixture::new();
        let file = fixture.docs.join("sample.pdf");
        fs::write(&file, b"x").unwrap();
        let grant = fixture.grant();
        let scope = fixture.scope(Some(grant.clone()));
        let target = authorize_row(&scope, file.clone(), FileCategory::Pdf).unwrap().unwrap().target;
        grant.revoke();
        grant.revoke();
        assert!(!grant.is_active());
        assert!(matches!(scope.query_paths(), Err(FindError::Stale)));
        let candidates = Candidates { paths: vec![file], partial: false, timed_out: false };
        assert!(matches!(authorize_batch(&scope, candidates, "all", &QueryCancellation::default()), Err(FindError::Stale)));
        assert!(submit_with(&scope, &target, FindAction::Default,
            || panic!("revoked grant admitted"), |_, _| panic!("revoked native call")).is_err());
        assert_eq!(fixture.scope(Some(grant)).query_paths().unwrap(), vec![fixture.shared.clone()]);
    }

    #[test]
    fn concurrent_revocation_at_final_admission_denies_native_submission() {
        let fixture = Fixture::new();
        let file = fixture.docs.join("report.pdf");
        fs::write(&file, b"x").unwrap();
        let grant = fixture.grant();
        let scope = fixture.scope(Some(grant.clone()));
        let target = authorize_row(&scope, file, FileCategory::Pdf).unwrap().unwrap().target;
        let (at_admission, ready) = std::sync::mpsc::channel();
        let (revoked, resume) = std::sync::mpsc::channel();
        std::thread::scope(|threads| {
            threads.spawn(move || {
                ready.recv().unwrap();
                grant.revoke();
                revoked.send(()).unwrap();
            });
            let outcome = submit_with(&scope, &target, FindAction::Default, || {
                at_admission.send(()).unwrap();
                resume.recv().unwrap();
                true
            }, |_, _| panic!("native submission after revoke"));
            assert_eq!(outcome, Err(FindError::Stale));
        });
    }

    #[test]
    fn revoke_drains_native_submission_but_not_its_pending_callback() {
        let fixture = Fixture::new();
        let file = fixture.docs.join("report.pdf");
        fs::write(&file, b"x").unwrap();
        let grant = fixture.grant();
        let scope = fixture.scope(Some(grant.clone()));
        let observer = Arc::clone(&grant);
        let target = authorize_row(&scope, file, FileCategory::Pdf).unwrap().unwrap().target;
        let (entered, started) = mpsc::channel();
        let (marked, observed) = mpsc::channel();
        let (ack, acknowledged) = mpsc::channel();
        let (callback, completion) = mpsc::channel();
        std::thread::scope(|workers| {
            workers.spawn(move || {
                started.recv().unwrap();
                grant.mark_revoked();
                marked.send(()).unwrap();
                grant.revoke();
                ack.send(()).unwrap();
                callback.send(true).unwrap();
            });
            let pending = submit_with(&scope, &target, FindAction::OpenDocument, || true, |_, outcome| {
                // This assertion catches releasing the grant mutex before the actual OS call,
                // independently of when the other thread gets scheduled.
                assert!(matches!(observer.lease.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
                entered.send(()).unwrap();
                observed.recv().unwrap();
                // The grant is retired but the OS submission is still on the stack.
                assert!(matches!(acknowledged.try_recv(), Err(mpsc::TryRecvError::Empty)));
                PendingSubmission::Awaiting(outcome, completion)
            }).unwrap();
            // Revoke must finish before the simulated callback is allowed to arrive.
            acknowledged.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(pending.confirmation(), Submission::FileOpenRequested);
        });
    }

    #[cfg(all(feature = "macos-find-preview", not(feature = "macos-find-system-qa")))]
    #[test]
    #[ignore = "requires an exclusive, indexed, pre-created scratch fixture in /Users/Shared"]
    fn scratch_documents_spotlight_grant_and_revoke() {
        use crate::macos_find_query::{gather_on_worker, ApprovedScope};
        use std::os::unix::fs::MetadataExt;

        let scratch = PathBuf::from(std::env::var_os("UIPILOT_DOCUMENTS_SMOKE_ROOT")
            .expect("supply the exclusive scratch fixture root"));
        assert_eq!(scratch.parent(), Some(Path::new("/Users/Shared")));
        assert!(scratch.file_name().unwrap().to_str().unwrap().starts_with("uipilot-find-docs-"));
        let scratch_fd = open_directory(&scratch).unwrap();
        let metadata = scratch_fd.metadata().unwrap();
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.mode() & 0o777, 0o700);
        let docs = scratch.join("home/Documents");
        let sample = docs.join("中文项目 🐋/附件/验收插画📷.png");
        let selected = NSURL::from_directory_path(&docs).unwrap();
        let grant = DocumentsGrant::bind_selected(&docs, &selected, false).unwrap();
        let scope = IndexedLocalScope::bind_with_documents(&[], Some(grant.clone())).unwrap();
        assert_eq!(scope.query_paths().unwrap(), vec![docs]);
        let cancel = QueryCancellation::default();
        let mut targets = Vec::new();
        for query in ["验收插画📷.png", "验收插画"] {
            let candidates = gather_on_worker(ApprovedScope::IndexedLocal(&scope), query, &cancel).unwrap();
            assert!(candidates.paths.contains(&sample), "Spotlight has not returned the scratch fixture");
            let batch = authorize_batch(&scope, candidates, "image", &cancel).unwrap();
            let row = batch.rows.into_iter().find(|row| row.full_path == sample.to_str().unwrap()).unwrap();
            assert_eq!(row.name, "验收插画📷.png");
            assert!(row.can_open_document);
            targets.push(row.target);
        }
        grant.revoke();
        assert!(scope.query_paths().is_err());
        for target in targets {
            assert!(submit_with(&scope, &target, FindAction::Default,
                || panic!("retired target admitted"), |_, _| panic!("Finder request")).is_err());
        }
        let late = Candidates { paths: vec![sample], partial: false, timed_out: false };
        assert!(matches!(authorize_batch(&scope, late, "image", &cancel), Err(FindError::Stale)));
    }

    #[test]
    fn directory_identity_survives_normal_edits_but_never_inherits_replacement_or_regrant() {
        let fixture = Fixture::new();
        let grant = fixture.grant();
        fs::write(fixture.docs.join("new.pdf"), b"new").unwrap();
        assert!(grant.is_active());
        let original = fixture.base.join("original");
        fs::rename(&fixture.docs, &original).unwrap();
        fs::create_dir(&fixture.docs).unwrap();
        assert!(!grant.is_active());
        fs::remove_dir(&fixture.docs).unwrap();
        fs::rename(original, &fixture.docs).unwrap();
        assert!(!grant.is_active());
    }

    #[test]
    fn refuses_package_ancestors_and_directory_alias_files_as_grants() {
        use objc2_foundation::NSURLBookmarkCreationOptions;

        let fixture = Fixture::new();
        let packaged = fixture.base.join("Host.app/Documents");
        fs::create_dir_all(&packaged).unwrap();
        let url = NSURL::from_directory_path(&packaged).unwrap();
        assert!(DocumentsGrant::from_fixture(&packaged, &url).is_err());

        let alias_path = fixture.base.join("account/alias");
        let alias = NSURL::from_file_path(&alias_path).unwrap();
        let source = NSURL::from_directory_path(&fixture.docs).unwrap();
        let bookmark = source.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::SuitableForBookmarkFile, None, None,
        ).unwrap();
        NSURL::writeBookmarkData_toURL_options_error(&bookmark, &alias, 0).unwrap();
        assert!(DocumentsGrant::from_fixture(&alias_path, &alias).is_err());
    }

    #[test]
    fn denies_symlinked_roots_and_drops_access_on_permission_loss() {
        let fixture = Fixture::new();
        let link = fixture.docs.parent().unwrap().join("link");
        symlink(&fixture.docs, &link).unwrap();
        let url = NSURL::from_directory_path(&link).unwrap();
        assert!(DocumentsGrant::bind_selected(&link, &url, false).is_err());
        let grant = fixture.grant();
        fs::set_permissions(&fixture.docs, fs::Permissions::from_mode(0)).unwrap();
        assert!(!grant.is_active());
        fs::set_permissions(&fixture.docs, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!grant.is_active());
    }
}
