//! Opt-in, isolated account acceptance. Both native control queries and the one system
//! query use a fresh fixture token. Nothing here submits a request to Finder.

use super::*;
use crate::{
    macos_find_query::{configured_for_indexed_local, gather_on_worker, ApprovedScope, GatherError},
    result_registry::{QueryDomain, ResultAction, ResultRegistries},
};
use std::{
    cell::Cell,
    ffi::OsStr,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    time::Instant,
};

type Check<T> = Result<T, &'static str>;

thread_local! {
    // Model withdrawal without mutating a process environment shared with Foundation.
    // This override can only remove authority, never supply or widen an accepted root.
    static WITHDRAWN: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn apply_probe_withdrawal(raw: Option<OsString>) -> Option<OsString> {
    if WITHDRAWN.get() { None } else { raw }
}

fn require(ok: bool, message: &'static str) -> Check<()> {
    if ok { Ok(()) } else { Err(message) }
}

fn passed(stage: &'static str) -> Check<()> {
    writeln!(std::io::stdout().lock(), "PASS {stage}").map_err(|_| "could not report probe stage")
}

struct Fixture {
    root: PathBuf,
    token: String,
}

impl Fixture {
    fn parse(path: &Path) -> Check<Self> {
        require(path.parent() == Some(Path::new("/Users/Shared")) && canonical_absolute(path),
            "invalid fixture location")?;
        let token = path.file_name().and_then(OsStr::to_str)
            .and_then(|name| name.strip_prefix("uipilot-system-qa-")).ok_or("invalid fixture name")?;
        require(token.len() == 32 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && token.bytes().any(|byte| byte != token.as_bytes()[0]), "invalid random fixture token")?;
        Ok(Self { root: path.to_owned(), token: token.to_owned() })
    }

    fn filename(&self) -> String { format!("UiPilot-{}-双样本.png", self.token) }
}

// Hold the directory and the created inode until both index controls and the system
// query finish. Pre-existing entries cannot be opened for writing or deleted.
struct CreatedPng {
    dir: File,
    name: String,
    file: File,
}

impl CreatedPng {
    fn create(dir: File, name: &str, bytes: &[u8]) -> Check<Self> {
        let fd = openat(&dir, name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL |
            OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::RUSR | Mode::WUSR)
            .map_err(|_| "cannot exclusively create synthetic PNG")?;
        let mut sample = Self { dir, name: name.to_owned(), file: File::from(fd) };
        sample.file.write_all(bytes).map_err(|_| "cannot write synthetic PNG")?;
        // Spotlight can observe the finished file after its writer closes. Keep a
        // read descriptor solely to identify the exact inode during cleanup.
        let reader = File::from(openat(&sample.dir, name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK |
            OFlags::CLOEXEC, Mode::empty()).map_err(|_| "cannot pin synthetic PNG")?);
        let writer_id = fstat(&sample.file).map_err(|_| "synthetic PNG identity unavailable")?;
        let reader_id = fstat(&reader).map_err(|_| "synthetic PNG identity unavailable")?;
        require((writer_id.st_dev, writer_id.st_ino) == (reader_id.st_dev, reader_id.st_ino), "synthetic PNG changed")?;
        sample.file = reader;
        Ok(sample)
    }
}

impl Drop for CreatedPng {
    fn drop(&mut self) {
        use rustix::fs::{statat, unlinkat, AtFlags};
        if let (Ok(held), Ok(current)) = (fstat(&self.file), statat(&self.dir, self.name.as_str(), AtFlags::SYMLINK_NOFOLLOW)) {
            if (held.st_dev, held.st_ino) == (current.st_dev, current.st_ino) {
                let _ = unlinkat(&self.dir, self.name.as_str(), AtFlags::empty());
            }
        }
    }
}

fn private_child(root: &File, name: &str, uid: u32) -> Check<File> {
    let dir = File::from(openat(root, name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty()).map_err(|_| "cannot open private fixture directory")?);
    let meta = dir.metadata().map_err(|_| "fixture identity unavailable")?;
    require(meta.uid() == uid && meta.mode() & 0o7777 == 0o700 && no_extended_acl(&dir)
        && meta.dev() == root.metadata().map_err(|_| "fixture identity unavailable")?.dev()
        && ensure_local_fd(&dir).is_ok(), "fixture directories must be local, QA-owned, mode 0700 and free of ACLs")?;
    Ok(dir)
}

struct Sample {
    directory: PathBuf,
    physical: PathBuf,
    logical_file: PathBuf,
    physical_file: PathBuf,
    device: u64,
    inode: u64,
}

impl Sample {
    fn read(directory: PathBuf, filename: &str, uid: u32, mounts: &Mounts) -> Check<Self> {
        require(mounts.allows_root(&directory), "fixture directory crosses a mount")?;
        let dir = open_directory(&directory).map_err(|_| "fixture directory unavailable")?;
        let meta = dir.metadata().map_err(|_| "fixture directory unavailable")?;
        require(meta.uid() == uid && meta.mode() & 0o7777 == 0o700 && no_extended_acl(&dir),
            "fixture directories must be QA-owned, mode 0700 and free of extended ACLs")?;
        let mount = MountId::from_fd(&dir).map_err(|_| "fixture mount unavailable")?;
        let physical = physical_root(&mount.path, &directory).map_err(|_| "fixture firmlink cannot be verified")?;
        let identity = FindPathIdentity::capture(&dir, Path::new(filename)).map_err(|_| "sample identity unavailable")?;
        require(!identity.is_directory(), "sample must be a regular PNG")?;
        let fd = openat(&dir, filename, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty()).map_err(|_| "cannot open the synthetic PNG")?;
        let mut file = File::from(fd);
        let meta = file.metadata().map_err(|_| "cannot identify the synthetic PNG")?;
        require(meta.is_file() && meta.uid() == uid && meta.nlink() == 1 && meta.ino() == identity.leaf_inode()
            && meta.dev() == dir.metadata().map_err(|_| "fixture directory unavailable")?.dev(), "unsafe sample identity")?;
        let mut header = [0; 8];
        file.read_exact(&mut header).map_err(|_| "sample has no PNG signature")?;
        require(&header == b"\x89PNG\r\n\x1a\n", "sample has no PNG signature")?;
        Ok(Self { logical_file: directory.join(filename), physical_file: physical.join(filename),
            directory, physical, device: meta.dev(), inode: meta.ino() })
    }

    fn matches(&self, path: &Path) -> bool {
        path == self.logical_file || path == self.physical_file
    }

    fn indexed(&self, text: &str, cancel: &QueryCancellation) -> Check<bool> {
        // A control stays strictly within one directory, including its physical spelling.
        let mut spellings = vec![self.directory.as_path()];
        if self.directory != self.physical { spellings.push(self.physical.as_path()); }
        for path in spellings {
            let scan = match gather_on_worker(ApprovedScope::Directory(path), text, cancel) {
                Ok(scan) => scan,
                Err(GatherError::TimedOut) => continue,
                Err(_) => return Err("single-directory Spotlight service failed"),
            };
            require(scan.paths.iter().all(|p| self.matches(p)), "control matched an unexpected object")?;
            if !scan.timed_out && !scan.paths.is_empty() { return Ok(true); }
        }
        Ok(false)
    }
}

fn wait_for_controls(allowed: &Sample, outside: &Sample, token: &str, cancel: &QueryCancellation) -> Check<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut ready = [false; 2];
    loop {
        for (index, sample) in [allowed, outside].into_iter().enumerate() {
            if !ready[index] { ready[index] = sample.indexed(token, cancel)?; }
        }
        if ready == [true, true] { return Ok(()); }
        if Instant::now() >= deadline {
            return Err(match ready {
                [false, false] => "index controls missing both samples; system query skipped",
                [true, false] => "outside sample missing from index control; system query skipped",
                _ => "allowed sample missing from index control; system query skipped",
            });
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[derive(Default)]
struct Calls { admission: Cell<usize>, submit: Cell<usize> }

impl Calls {
    fn attempt(&self, scope: &IndexedLocalScope, target: &AuthorizedFile, admit: impl FnOnce() -> bool) -> Result<bool, FindError> {
        submit_with(scope, target, FindAction::OpenDocument,
            || { self.admission.set(self.admission.get() + 1); admit() },
            |reference, kind| {
                self.submit.set(self.submit.get() + 1);
                kind == Submission::FileOpenRequested && reference.isFileReferenceURL()
            })
    }
}

fn run_on_worker() -> Check<()> {
    // Check all user-supplied declarations before touching fixture directories or Spotlight.
    require(std::env::var_os("UIPILOT_FIND_ISOLATED_QA").as_deref() == Some(OsStr::new("1")),
        "isolated QA opt-in required")?;
    let declared = std::env::var("UIPILOT_FIND_QA_SMOKE_ACCOUNT").map_err(|_| "dedicated QA account declaration required")?;
    let fixture = Fixture::parse(Path::new(&std::env::var_os("UIPILOT_FIND_QA_SMOKE_FIXTURE")
        .ok_or("exclusive scratch fixture required")?))?;
    let account = qa_account().map_err(|_| "this account is not eligible for system QA")?;
    require(account.name == declared, "declared account does not match the dedicated QA account")?;
    let allowed_dir = fixture.root.join("allowed");
    let evidence = current_evidence().map_err(|_| "explicit verified root required")?;
    require(evidence == [allowed_dir.as_path()], "verified roots must contain only the allowed fixture directory")?;
    let mounts = Mounts::current().map_err(|_| "mount table unavailable")?;
    require(mounts.allows_root(&fixture.root), "fixture mount is not eligible")?;
    let root = open_directory(&fixture.root).map_err(|_| "exclusive fixture unavailable")?;
    let root_meta = root.metadata().map_err(|_| "exclusive fixture unavailable")?;
    require(root_meta.uid() == account.uid && root_meta.mode() & 0o7777 == 0o700 && no_extended_acl(&root),
        "expected a QA-owned mode-0700 fixture without extended ACLs")?;
    let scope = IndexedLocalScope::for_host().map_err(|_| "host rejected the isolated scope or account home")?;
    require(scope.uses_system_query(), "host did not select the system backend")?;
    let filename = fixture.filename();
    let allowed_child = private_child(&root, "allowed", account.uid)?;
    let outside_child = private_child(&root, "outside", account.uid)?;
    use base64::{engine::general_purpose::STANDARD, Engine};
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .map_err(|_| "invalid embedded PNG")?;
    let _samples = [CreatedPng::create(allowed_child, &filename, &png)?,
                    CreatedPng::create(outside_child, &filename, &png)?];
    let allowed = Sample::read(allowed_dir, &filename, account.uid, &mounts)?;
    let outside = Sample::read(fixture.root.join("outside"), &filename, account.uid, &mounts)?;
    require((allowed.device, allowed.inode) != (outside.device, outside.inode), "the two samples must be distinct objects")?;

    let cancel = QueryCancellation::default();
    wait_for_controls(&allowed, &outside, &fixture.token, &cancel)?;
    passed("directory index controls allowed=1 outside=1")?;
    require(configured_for_indexed_local(&scope, &fixture.token), "native system scope identifier mismatch")?;
    let registries = ResultRegistries::default();
    let registry = registries.find();
    let invocation = "isolated-find-probe";
    registry.try_on_show(invocation.to_owned()).map_err(|_| "Find registry unavailable")?;
    let query_owner = registry.begin_query(QueryDomain::File, invocation, 1).ok_or("Find query admission failed")?;
    // Exactly one global query, with the token rather than any real filename. A global miss
    // after positive controls is a coverage failure, never grounds for a fallback scan.
    let candidates = gather_on_worker(ApprovedScope::IndexedLocal(&scope), &fixture.token, &cancel)
        .map_err(|_| "system Spotlight query failed after both index controls passed")?;
    require(!candidates.paths.is_empty(), "system scope returned no allowed match after positive controls")?;
    require(candidates.paths.iter().all(|path| allowed.matches(path)), "system query exposed an unapproved candidate")?;
    let batch = authorize_batch(&scope, candidates, "image", &cancel)
        .map_err(|_| "system candidate authorization failed")?;
    let entries = batch.rows.into_iter().enumerate()
        .map(|(index, row)| (row, ResultAction::MacFindRow(index))).collect();
    let (request, rows) = registry.publish_if_latest(query_owner, entries, || true, |id, rows| (id, rows))
        .ok_or("Find registry rejected publication")?;
    require(rows.len() == 1, "system publication must contain one verified row")?;
    let (result, row) = &rows[0];
    let (action, ticket) = registry.resolve_with_ticket(&request, result).map_err(|_| "published row has no current ticket")?;
    require(matches!(action, ResultAction::MacFindRow(0)), "wrong published action")?;
    require(row.name == filename && row.full_path == allowed.logical_file.to_str().ok_or("invalid fixture spelling")?
        && row.target.identity.leaf_inode() == allowed.inode && row.can_open_document && !row.is_directory,
        "published row did not identify the expected synthetic PNG")?;
    let target = row.target.clone();

    // Replay the two proven control hits through production authorization in case a provider
    // returned the excluded path. This check cannot satisfy the actual system query above.
    let replay = || Candidates { paths: vec![allowed.logical_file.clone(), outside.logical_file.clone()],
        timed_out: false, partial: false };
    let replayed = authorize_batch(&scope, replay(), "image", &cancel).map_err(|_| "control replay failed")?;
    require(replayed.partial && replayed.rows.len() == 1 && replayed.rows[0].full_path == row.full_path,
        "outside fixture survived file authorization")?;
    let positive = Calls::default();
    require(positive.attempt(&scope, &target, || registry.is_execution_ticket_current(&ticket)) == Ok(true)
        && positive.admission.get() == 1 && positive.submit.get() == 1, "positive mock dispatch did not submit exactly once")?;
    require(registry.retire_result_set_if_current(&ticket) && !registry.is_execution_ticket_current(&ticket),
        "first ticket did not retire after mock dispatch")?;
    passed("system query authorization approved_rows=1 outside_rows=0 mock_submissions=1")?;

    // Give withdrawal its own unconsumed ticket on the same retained target. There is no
    // second Spotlight query; this publication just isolates the negative admission case.
    let owner = registry.begin_query(QueryDomain::File, invocation, 2).ok_or("negative ticket query failed")?;
    let (request, mut slots) = registry.publish_if_latest(owner, vec![(target, ResultAction::MacFindRow(0))],
        || true, |request, slots| (request, slots)).ok_or("negative ticket publication failed")?;
    let (id, target) = slots.pop().ok_or("negative publication has no target")?;
    let (action, fresh_ticket) = registry.resolve_with_ticket(&request, &id).map_err(|_| "could not resolve the second ticket")?;
    require(matches!(action, ResultAction::MacFindRow(0)) && fresh_ticket != ticket, "second ticket reused a consumed action")?;
    let current_ticket = || registry.is_execution_ticket_current(&fresh_ticket);

    // Withdraw host evidence only on this worker; never change process environment, TCC
    // settings, mounted volumes or the operator's fixture. Once observed, the old scope
    // must stay retired even when an independent future query could see valid evidence.
    let forbidden = Calls::default();
    // Keep this ticket current for the first negative attempt so a stale registry token
    // cannot mask a failure in the independent native scope check.
    require(current_ticket(), "ticket retired before authority withdrawal")?;
    let denied = withdraw(|| {
        forbidden.attempt(&scope, &target, current_ticket).is_err() && scope.query_paths().is_err()
            && authorize_batch(&scope, replay(), "image", &cancel).is_err() && current_ticket()
    });
    require(denied && forbidden.attempt(&scope, &target, current_ticket).is_err()
        && forbidden.admission.get() == 0 && forbidden.submit.get() == 0,
        "withdrawn authority allowed stale admission or dispatch")?;
    registry.invalidate_domain(QueryDomain::File).map_err(|_| "could not retire the old Find ticket")?;
    require(!current_ticket(), "old Find ticket survived retirement")?;
    passed("withdrawn authority old_target_admissions=0 mock_submissions=0")
}

pub(crate) fn run() -> Result<(), String> {
    let worker = std::thread::Builder::new().name("find-system-qa".into())
        .spawn(|| autoreleasepool(|_| run_on_worker())).map_err(|_| "could not start dedicated worker".to_owned())?;
    worker.join().map_err(|_| "system QA worker failed".to_owned())?.map_err(str::to_owned)
}

fn withdraw<T>(operation: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset { fn drop(&mut self) { WITHDRAWN.set(self.0); } }
    let _reset = Reset(WITHDRAWN.replace(true));
    operation()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_creation_refuses_links_and_cleanup_leaves_replacements_alone() {
        use std::{fs, os::unix::fs::symlink};
        let parent = std::env::temp_dir().canonicalize().unwrap().join(format!("uipilot-qa-png-{}", std::process::id()));
        fs::create_dir(&parent).unwrap();
        let original = parent.join("original");
        let name = parent.join("sample.png");
        fs::write(&original, b"unchanged").unwrap();
        symlink(&original, &name).unwrap();
        assert!(CreatedPng::create(open_directory(&parent).unwrap(), "sample.png", b"PNG").is_err());
        assert_eq!(fs::read(&original).unwrap(), b"unchanged");
        fs::remove_file(&name).unwrap();
        let owned = CreatedPng::create(open_directory(&parent).unwrap(), "sample.png", b"created").unwrap();
        fs::remove_file(&name).unwrap();
        fs::write(&name, b"replacement").unwrap();
        drop(owned);
        assert_eq!(fs::read(&name).unwrap(), b"replacement");
        fs::remove_file(&name).unwrap();
        drop(CreatedPng::create(open_directory(&parent).unwrap(), "sample.png", b"created").unwrap());
        assert!(!name.exists());
        fs::remove_file(original).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn rejects_unscoped_or_malformed_fixture_declarations_without_io() {
        let good = "/Users/Shared/uipilot-system-qa-0123456789abcdef0123456789abcdef";
        assert!(Fixture::parse(Path::new(good)).is_ok());
        for path in ["/Users/Shared", "/tmp/foreign", "/Users/Shared/uipilot-system-qa-short",
            "/Users/Shared/uipilot-system-qa-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "/Users/Shared/uipilot-system-qa-0123456789abcdef0123456789abcdef/nested"] {
            assert!(Fixture::parse(Path::new(path)).is_err());
        }
    }

    #[test]
    fn denied_evidence_retires_an_existing_target_without_dispatch_or_a_system_query() {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "uipilot-qa-revoked-target-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        let mut scope = IndexedLocalScope::bind(&[&path]).unwrap();
        let sample = path.join("item.png");
        std::fs::write(&sample, b"synthetic").unwrap();
        let target = authorize_row(&scope, sample, FileCategory::Image).unwrap().unwrap().target;

        // No live account/home lookup is possible here: missing evidence is rejected
        // before home access, and the second attempt must reject the retired scope first.
        let fd = open_directory(&path).unwrap();
        scope.qa_home = Some(LocalHome {
            account: QaAccount { name: String::from("uipilot-qa-fixture"), uid: 0, home: path.clone() },
            id: MountId::from_fd(&fd).unwrap(), fd,
        });
        let forbidden = Calls::default();
        assert!(withdraw(|| forbidden.attempt(&scope, &target, || true)).is_err());
        assert_eq!(forbidden.attempt(&scope, &target, || true), Err(FindError::Stale));
        assert_eq!((forbidden.admission.get(), forbidden.submit.get()), (0, 0));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn probe_authority_can_only_remove_operator_evidence() {
        let input = || Some(OsString::from("/synthetic"));
        assert!(apply_probe_withdrawal(None).is_none());
        assert!(withdraw(|| apply_probe_withdrawal(input())).is_none());
        assert_eq!(apply_probe_withdrawal(input()), input());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires a dedicated local QA account with two indexed synthetic samples"]
fn isolated_system_scope_publish_and_revoke() -> Result<(), String> { run() }
