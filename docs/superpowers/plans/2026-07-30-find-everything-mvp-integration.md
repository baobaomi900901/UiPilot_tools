# /find Everything MVP Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace production `/find` searches with the validated in-process Everything Query2 client while preserving opaque result execution and the existing file result experience.

**Architecture:** Add a backend-neutral `file_search` boundary, extract the existing Windows component-pinning and Shell-dispatch logic into one shared module, then add a lazy cached Everything adapter. Keep the legacy `FileIndex` source and execution adapter compiling, but route production `search_files` only through Everything and remove frontend file-index event and polling dependencies.

**Tech Stack:** Rust 2021, Tauri 2.11, `windows` 0.61, existing `everything-ipc-spike` Query2 library, React 19, TypeScript 7, Vitest 4, Everything 1.4 on Windows.

**Source Spec:** `docs/superpowers/specs/2026-07-30-find-everything-mvp-integration-design.md`

## Global Constraints

- Production `/find` connects only to the manually running default Everything 1.4 instance.
- Do not start, install, configure, elevate, repair, bundle, or stop Everything.
- Do not add Service, UAC, Owner SID, ACL, SDDL, multi-user, installer, or repair behavior.
- Do not add pagination, file-change events, polling, atomic refresh transactions, category filters, or sort switching.
- A search sends one Query2 request with `offset = 0`, `max_results = 200`, `request_flags = 0x155`, date-modified descending sort, a 250 ms connection timeout, and a 1 second absolute query deadline.
- Treat input as one literal filename term by encoding every Unicode scalar as `#x<HEX>:`; never expose Everything operators, wildcards, functions, macros, or advanced syntax.
- Category is fixed to `all`; sort is fixed to `modifiedDesc`; empty input is not sent.
- Never fall back to `FileIndex`; retain its source and legacy action adapter so it continues to compile.
- Authenticate every visible Everything entry under the UiPilot process token before publication; omit only the entry that cannot be authenticated.
- Revalidate every path component at execution, reject reparse points, omit `FILE_SHARE_DELETE`, and hold all handles until the Shell call returns.
- The WebView receives only display metadata and opaque `requestId`/`resultId`; authenticated actions remain Rust-only.
- `EverythingSearchState` owns the only revision high-water mark for this milestone. Successful fully authenticated batches consume a checked revision immediately before registry publication; failures consume none; superseded batches may create gaps.
- Preserve existing `require_main_window` first-statement admission and stale query/result rejection.
- Every task starts with a failing focused test, ends with focused green verification, and is committed separately only after planner review.
- Do not stage or modify the unrelated installer, resource, managed-runtime, or legacy migration document changes already present in the worktree.

## Cross-Task Interfaces

`src-tauri/src/file_search/mod.rs` owns these backend-neutral types after Tasks 1 and 2:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilePathKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionOutcome {
    FileRevealRequested,
    FolderOpenRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionError {
    SearchUnavailable,
    Stale,
    NotFound,
    OpenFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EverythingPathAction {
    identity: windows::path_auth::AuthenticatedPathIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionAction {
    Indexed(crate::file_index::OpenIndexedPath),
    Everything(EverythingPathAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileIndexStatus {
    Building,
    Ready,
    Partial,
    Rebuilding,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileResultKind {
    File,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedFileDraft {
    pub(crate) action: FileExecutionAction,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedFileBatch {
    pub(crate) index_revision: u64,
    pub(crate) items: Vec<PublishedFileDraft>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileResultItem {
    pub(crate) result_id: String,
    pub(crate) name: String,
    pub(crate) kind: FileResultKind,
    pub(crate) size_bytes: Option<String>,
    pub(crate) modified_utc: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileSearchResponse {
    pub(crate) request_id: String,
    pub(crate) index_revision: String,
    pub(crate) total: String,
    pub(crate) status: FileIndexStatus,
    pub(crate) items: Vec<FileResultItem>,
}
```

`src-tauri/src/file_search/windows/path_auth.rs` owns these shared Windows interfaces:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedPathIdentity {
    pub(crate) display_path: String,
    pub(crate) volume_guid_path: String,
    pub(crate) relative_path: String,
    pub(crate) volume_serial: u32,
    pub(crate) file_id: [u8; 16],
    pub(crate) kind: FilePathKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedPathSnapshot {
    pub(crate) identity: AuthenticatedPathIdentity,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) modified_filetime: u64,
}

pub(crate) struct LegacyPathExpectation<'a> {
    pub(crate) volume_guid_path: &'a str,
    pub(crate) volume_serial: u32,
    pub(crate) filesystem_name: &'a str,
    pub(crate) relative_path: &'a str,
    pub(crate) kind: FilePathKind,
}

pub(crate) fn authenticate_path(
    display_path: &str,
    expected_kind: FilePathKind,
) -> Result<AuthenticatedPathSnapshot, FileExecutionError>;

pub(crate) fn execute_authenticated_path(
    identity: &AuthenticatedPathIdentity,
) -> Result<FileExecutionOutcome, FileExecutionError>;

pub(crate) fn execute_legacy_indexed_path(
    expectation: LegacyPathExpectation<'_>,
) -> Result<FileExecutionOutcome, FileExecutionError>;
```

`src-tauri/src/file_search/everything.rs` owns the production search state:

```rust
pub(crate) struct EverythingSearchState {
    client: std::sync::Mutex<Option<std::sync::Arc<everything_ipc::EverythingClient>>>,
    revision: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EverythingSearchError {
    Unavailable,
    RevisionExhausted,
}

impl EverythingSearchState {
    pub(crate) fn new() -> Self;
    pub(crate) fn search(&self, query: &str) -> Result<PublishedFileBatch, EverythingSearchError>;
}

pub(crate) fn encode_literal_query(query: &str) -> Vec<u16>;
```

---

### Task 1: Extract Shared Windows Path Authentication

**Files:**
- Create: `src-tauri/src/file_search/mod.rs`
- Create: `src-tauri/src/file_search/windows/mod.rs`
- Create: `src-tauri/src/file_search/windows/path_auth.rs`
- Modify: `src-tauri/src/file_index/windows_backend.rs:139`
- Modify: `src-tauri/src/file_index/mod.rs:4106`
- Modify: `src-tauri/src/lib.rs:44`

**Interfaces:**
- Consumes: existing `OwnedHandle`, component walk, reparse checks, final-path checks, `SHOpenFolderAndSelectItems`, `ShellExecuteExW`, `VolumeIdentity`, and `OpenIndexedPath` behavior from `file_index/windows_backend.rs`.
- Produces: `FilePathKind`, `FileExecutionOutcome`, `FileExecutionError`, `AuthenticatedPathIdentity`, `AuthenticatedPathSnapshot`, `LegacyPathExpectation`, `authenticate_path`, `execute_authenticated_path`, and `execute_legacy_indexed_path` exactly as declared in `Cross-Task Interfaces`.

- [ ] **Step 1: Add failing path-authentication tests**

Create the new module files, add `mod file_search;` beside `mod file_index;`, and start `path_auth.rs` with the public signatures returning `FileExecutionError::OpenFailed`. Add deterministic tests around a private injected component walker:

```rust
#[test]
fn component_walk_uses_no_delete_share_and_rejects_reparse_or_path_substitution() {
    let opened = RefCell::new(Vec::new());
    let expected = test_identity(r"docs\report.pdf", FilePathKind::File);
    let result = walk_expected_components_with(
        &expected,
        |relative, kind, share| {
            opened.borrow_mut().push((relative.to_owned(), kind, share));
            Ok(relative.to_owned())
        },
        |relative, expected_kind, _handle| {
            Ok(test_component_observation(relative, expected_kind))
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        opened.borrow().as_slice(),
        [
            ("docs".into(), FilePathKind::Directory, ExecutionShare::ReadWrite),
            ("docs\\report.pdf".into(), FilePathKind::File, ExecutionShare::ReadWrite),
        ]
    );

    for mutation in [
        TestMutation::Reparse,
        TestMutation::OtherVolume,
        TestMutation::OtherRelativePath,
        TestMutation::WrongKind,
    ] {
        assert_eq!(run_mutated_walk(&expected, mutation), Err(FileExecutionError::Stale));
    }
}

#[test]
fn authenticated_identity_distinguishes_hard_link_paths() {
    let first = test_identity(r"links\first.txt", FilePathKind::File);
    let second = AuthenticatedPathIdentity {
        display_path: r"C:\links\second.txt".into(),
        relative_path: r"links\second.txt".into(),
        ..first.clone()
    };
    assert_eq!(first.file_id, second.file_id);
    assert_ne!(first, second);
}

#[test]
fn component_handles_live_until_shell_callback_returns() {
    let drops = Rc::new(Cell::new(0));
    execute_with_components_for_test(test_handles(Rc::clone(&drops)), || {
        assert_eq!(drops.get(), 0);
        Ok(())
    })
    .unwrap();
    assert_eq!(drops.get(), 2);
}
```

Add tests that the final `FILE_ID_INFO`, volume serial, canonical relative path, and kind are copied into `AuthenticatedPathIdentity`, and that leaf replacement, parent rename, junction/reparse insertion, and file/folder substitution return `Stale` before the injected Shell callback runs.

- [ ] **Step 2: Run the focused tests and confirm the red state**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml file_search::windows::path_auth::tests -- --nocapture
```

Expected: FAIL because the new authentication and execution functions still return `OpenFailed` and the generic walker does not exist.

- [ ] **Step 3: Move the shared handle, walk, identity, and Shell implementation**

Move, rather than copy, the existing `OwnedHandle`, execution component opening, component inspection, PIDL ownership, file reveal, directory open, and Shell callback lifetime logic into `path_auth.rs`. Implement the final-component inspection with `GetFileInformationByHandleEx` using `FileAttributeTagInfo`, `FileStandardInfo`, `FileBasicInfo`, and `FileIdInfo`:

```rust
fn read_file_id(handle: HANDLE) -> Result<[u8; 16], FileExecutionError> {
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| FileExecutionError::OpenFailed)?,
        )
    }
    .map_err(map_windows_open_error)?;
    Ok(info.FileId.Identifier)
}
```

Open every component with `FILE_FLAG_OPEN_REPARSE_POINT`, add `FILE_FLAG_BACKUP_SEMANTICS` for directories, request `FILE_LIST_DIRECTORY` for directories and `FILE_READ_ATTRIBUTES` for file leaves, and pass only `FILE_SHARE_READ | FILE_SHARE_WRITE`. Reject `FILE_ATTRIBUTE_REPARSE_POINT` on every component.

For publication, resolve the input path to its fixed-volume root, walk from that root, compare every observed canonical prefix, and return:

```rust
AuthenticatedPathSnapshot {
    identity: AuthenticatedPathIdentity {
        display_path: display_path.to_owned(),
        volume_guid_path,
        relative_path,
        volume_serial,
        file_id,
        kind,
    },
    size_bytes: (kind == FilePathKind::File).then_some(size),
    modified_filetime,
}
```

For execution, walk again, compare all identity fields, reconstruct the Shell target from the authenticated `volume_guid_path + relative_path` rather than trusting `display_path`, retain the `Vec<OwnedHandle>` in scope, invoke `SHOpenFolderAndSelectItems` for files or `ShellExecuteExW` for directories, and drop handles only after that call returns.

- [ ] **Step 4: Make the legacy index delegate to the shared helper**

Replace the old local pin/Shell block in `file_index/windows_backend.rs` with:

```rust
pub(super) fn execute_indexed_path(
    volume: &FixedVolume,
    action: &OpenIndexedPath,
) -> Result<FileExecutionOutcome, BackendError> {
    path_auth::execute_legacy_indexed_path(LegacyPathExpectation {
        volume_guid_path: &volume.identity.volume_guid_path,
        volume_serial: volume.identity.volume_serial,
        filesystem_name: &volume.identity.filesystem_name,
        relative_path: &action.relative_path,
        kind: match action.kind {
            IndexedKind::File => FilePathKind::File,
            IndexedKind::Directory => FilePathKind::Directory,
        },
    })
    .map_err(map_shared_execution_error)
}
```

Move `FileExecutionOutcome` and `FileExecutionError` definitions to `file_search/mod.rs`, import them from `file_index/mod.rs`, and preserve the current legacy error mapping. Remove the original local implementations after delegation compiles so there is one path-authentication block.

- [ ] **Step 5: Run focused legacy and new path tests**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml file_search::windows::path_auth::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml file_index::windows_backend::tests::pinned_path -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml file_execution -- --nocapture
```

Expected: all commands exit `0`; legacy indexed file reveal/folder open tests still pass through the shared helper; no copied `SHOpenFolderAndSelectItems` or `ShellExecuteExW` implementation remains in `file_index/windows_backend.rs`.

- [ ] **Step 6: Commit Task 1 after planner review**

```powershell
git add src-tauri/src/file_search/mod.rs src-tauri/src/file_search/windows/mod.rs src-tauri/src/file_search/windows/path_auth.rs src-tauri/src/file_index/windows_backend.rs src-tauri/src/file_index/mod.rs src-tauri/src/lib.rs
git commit -m "refactor: share authenticated file execution"
```

Report the commit hash, focused test output, moved symbols, and `git diff --check` result. Do not begin Task 2 until the planner approves the diff.

---

### Task 2: Introduce Backend-Neutral File Actions

**Files:**
- Modify: `src-tauri/src/file_search/mod.rs`
- Modify: `src-tauri/src/file_index/mod.rs:4217`
- Modify: `src-tauri/src/result_registry.rs:10`
- Modify: `src-tauri/src/commands.rs:462`

**Interfaces:**
- Consumes: Task 1 `AuthenticatedPathIdentity`, `execute_authenticated_path`, `FileExecutionOutcome`, and `FileExecutionError`; existing `OpenIndexedPath` and `FileIndex::execute_indexed_path`.
- Produces: `EverythingPathAction`, `FileExecutionAction`, `PublishedFileDraft`, `PublishedFileBatch`, `FileIndexStatus`, `FileResultKind`, `FileResultItem`, and `FileSearchResponse` exactly as declared in `Cross-Task Interfaces`; `ResultAction::OpenFile(FileExecutionAction)`.

- [ ] **Step 1: Add failing registry and execution-dispatch tests**

Update tests first to require both action variants:

```rust
#[test]
fn registry_resolves_indexed_and_everything_actions_as_opaque_file_actions() {
    let registry = ResultRegistry::default();
    registry.on_show("inv-1".into());
    let token = registry.begin_query(QueryDomain::File, "inv-1", 1).unwrap();
    let indexed = FileExecutionAction::Indexed(indexed_action_for_test());
    let everything = FileExecutionAction::Everything(EverythingPathAction::for_test(
        authenticated_identity_for_test(r"docs\report.pdf", [7; 16]),
    ));
    let response = registry
        .publish_if_latest(
            token,
            vec![
                ("indexed", ResultAction::OpenFile(indexed.clone())),
                ("everything", ResultAction::OpenFile(everything.clone())),
            ],
            || true,
            |request_id, items| (request_id, items),
        )
        .unwrap();
    assert_eq!(registry.resolve(&response.0, &response.1[0].1), Ok(ResultAction::OpenFile(indexed)));
    assert_eq!(registry.resolve(&response.0, &response.1[1].1), Ok(ResultAction::OpenFile(everything)));
}

#[test]
fn execute_file_action_dispatches_each_backend_once() {
    for (action, expected_backend) in [
        (FileExecutionAction::Indexed(indexed_action_for_test()), "indexed"),
        (FileExecutionAction::Everything(everything_action_for_test()), "everything"),
    ] {
        let calls = RefCell::new(Vec::new());
        let outcome = execute_file_action_with(
            action,
            |action| {
                calls.borrow_mut().push(("indexed", action.kind_for_test()));
                Ok(FileExecutionOutcome::FileRevealRequested)
            },
            |action| {
                calls.borrow_mut().push(("everything", action.kind_for_test()));
                Ok(FileExecutionOutcome::FileRevealRequested)
            },
        )
        .unwrap();
        assert_eq!(outcome, FileExecutionOutcome::FileRevealRequested);
        assert_eq!(calls.borrow().as_slice(), [(expected_backend, FilePathKind::File)]);
    }
}
```

Keep the existing serialization test and assert that neither `EverythingPathAction` fields nor any canonical identity appears in `serde_json::to_value(FileSearchResponse)`.

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests::registry_resolves_indexed_and_everything_actions_as_opaque_file_actions -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::execute_file_action_dispatches_each_backend_once -- --nocapture
```

Expected: FAIL because `FileExecutionAction`, `EverythingPathAction`, and `ResultAction::OpenFile` are not implemented.

- [ ] **Step 3: Move shared DTOs and define backend-neutral actions**

Move `FileIndexStatus`, `FileResultKind`, `FileResultItem`, and `FileSearchResponse` from `file_index/mod.rs` into `file_search/mod.rs`. Keep legacy-only `FileResultDraft` and `FileSearchBatch` in `file_index`; import or re-export the shared DTOs there so existing legacy tests retain their current meaning.

Implement action construction without exposing identity to serialization:

```rust
impl EverythingPathAction {
    pub(crate) fn new(identity: AuthenticatedPathIdentity) -> Self {
        Self { identity }
    }

    pub(crate) fn identity(&self) -> &AuthenticatedPathIdentity {
        &self.identity
    }
}
```

Change `ResultAction` to:

```rust
pub(crate) enum ResultAction {
    LaunchApplication { app_id: String, target: ApplicationLaunchTarget },
    OpenFile(FileExecutionAction),
    CopyText { plugin_id: String, generation: u64, text: String },
}
```

- [ ] **Step 4: Generalize file publication and execution**

Keep the legacy publication helper working by wrapping old actions:

```rust
let action = ResultAction::OpenFile(FileExecutionAction::Indexed(item.action.clone()));
```

Change the execution branch to pass one backend-neutral action into the file closure:

```rust
ResultAction::OpenFile(action) => {
    let outcome = execute_file(action).await?;
    let response = match outcome {
        FileExecutionOutcome::FileRevealRequested => ExecuteOutcome::FileRevealRequested,
        FileExecutionOutcome::FolderOpenRequested => ExecuteOutcome::FolderOpenRequested,
    };
    clear_and_hide()?;
    Ok(response)
}
```

Add one private dispatcher used by the production closure and tests:

```rust
fn execute_file_action_with<I, E>(
    action: FileExecutionAction,
    execute_indexed: I,
    execute_everything: E,
) -> Result<FileExecutionOutcome, FileExecutionError>
where
    I: FnOnce(OpenIndexedPath) -> Result<FileExecutionOutcome, FileExecutionError>,
    E: FnOnce(EverythingPathAction) -> Result<FileExecutionOutcome, FileExecutionError>,
{
    match action {
        FileExecutionAction::Indexed(action) => execute_indexed(action),
        FileExecutionAction::Everything(action) => execute_everything(action),
    }
}
```

The production `execute_result` closure must continue to use `spawn_blocking`; dispatch indexed actions to `FileIndex::execute_indexed_path` and Everything actions to `path_auth::execute_authenticated_path(action.identity())`.

- [ ] **Step 5: Run registry, command, and full Rust tests**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml result_registry -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml execute_result -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all commands exit `0`; application and plugin execution branches are unchanged; indexed and Everything actions both resolve only from opaque registry IDs.

- [ ] **Step 6: Commit Task 2 after planner review**

```powershell
git add src-tauri/src/file_search/mod.rs src-tauri/src/file_index/mod.rs src-tauri/src/result_registry.rs src-tauri/src/commands.rs
git commit -m "refactor: generalize file result actions"
```

Report the commit hash, action/DTO migration summary, focused and full Rust test results, and `git diff --check`. Wait for planner approval.

---

### Task 3: Add the Production Everything Search Adapter

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/file_search/mod.rs`
- Create: `src-tauri/src/file_search/everything.rs`
- Modify: `spikes/everything-ipc/tests/query_semantics.rs:1781`

**Interfaces:**
- Consumes: Task 1 `authenticate_path` and authenticated snapshots; Task 2 `EverythingPathAction`, `PublishedFileDraft`, `PublishedFileBatch`, `FileResultKind`; `everything_ipc::{EverythingClient, EverythingClientError, EverythingQuerySpec, EverythingQueryResult, EverythingSort}`.
- Produces: `EverythingSearchState`, `EverythingSearchError`, `EverythingSearchState::new`, `EverythingSearchState::search`, and `encode_literal_query` exactly as declared in `Cross-Task Interfaces`.

- [ ] **Step 1: Add the path dependency and failing adapter tests**

Add the dependency alias and required Windows time conversion feature:

```toml
everything-ipc = { package = "everything-ipc-spike", path = "../spikes/everything-ipc" }
```

Add `"Win32_System_Time"` to the existing `windows` feature list. Create `everything.rs` with stubs, then add these pure tests:

```rust
#[test]
fn literal_query_encodes_every_unicode_scalar_and_no_operator_survives() {
    assert_eq!(literal_text("a b|!*?<>\"文件😀"), "#x61:#x20:#x62:#x7C:#x21:#x2A:#x3F:#x3C:#x3E:#x22:#x6587:#x4EF6:#x1F600:");
    assert_eq!(literal_text("e\u{301}"), "#x65:#x301:");
}

#[test]
fn query_contract_is_fixed_and_authentication_preserves_order() {
    let captured = RefCell::new(None);
    let result = run_search_with(
        "report",
        &AtomicU64::new(0),
        |spec| {
            *captured.borrow_mut() = Some(spec.clone());
            Ok(query_result_for_test(205))
        },
        |item| authenticate_item_for_test(item),
    )
    .unwrap();
    let spec = captured.borrow();
    let spec = spec.as_ref().unwrap();
    assert_eq!(spec.offset, 0);
    assert_eq!(spec.max_results, 200);
    assert_eq!(spec.request_flags, 0x155);
    assert_eq!(spec.sort, EverythingSort::DateModifiedDescending);
    assert_eq!(result.items.len(), 200);
    assert_eq!(result.index_revision, 1);
}

#[test]
fn failed_query_or_authentication_batch_does_not_allocate_revision() {
    let revision = AtomicU64::new(9);
    assert_eq!(
        run_search_with("x", &revision, |_| Err(EverythingClientError::QueryTimedOut), |_| unreachable!()),
        Err(EverythingSearchError::Unavailable)
    );
    assert_eq!(revision.load(Ordering::Acquire), 9);
}

#[test]
fn revision_exhaustion_stays_failed_closed() {
    let revision = AtomicU64::new(u64::MAX);
    for _ in 0..2 {
        assert_eq!(run_successful_search_for_test(&revision), Err(EverythingSearchError::RevisionExhausted));
        assert_eq!(revision.load(Ordering::Acquire), u64::MAX);
    }
}
```

Add table-driven tests for unavailable, send failure, client closed, protocol mismatch, request-ID exhaustion, timeout, and overload. Assert the first five evict the matching cached client, timeout and overload preserve it, and a later call reconnects only after eviction. Add metadata tests where Query2 size or modified time is missing and the authenticated snapshot supplies a safe fallback.

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml file_search::everything::tests -- --nocapture
```

Expected: FAIL because the literal encoder, fixed query builder, cache policy, metadata conversion, and revision allocator are stubs.

- [ ] **Step 3: Implement literal encoding and the fixed Query2 request**

Implement literal encoding without normalization or case folding:

```rust
pub(crate) fn encode_literal_query(query: &str) -> Vec<u16> {
    let mut encoded = String::with_capacity(query.len().saturating_mul(6));
    for scalar in query.chars() {
        use std::fmt::Write as _;
        write!(&mut encoded, "#x{:X}:", u32::from(scalar)).expect("String writes are infallible");
    }
    encoded.encode_utf16().collect()
}
```

Build exactly one request:

```rust
EverythingQuerySpec {
    search: encode_literal_query(query),
    offset: 0,
    max_results: 200,
    request_flags: 0x155,
    sort: EverythingSort::DateModifiedDescending,
    deadline: Instant::now()
        .checked_add(Duration::from_secs(1))
        .ok_or(EverythingSearchError::Unavailable)?,
}
```

Reject empty input before connecting.

- [ ] **Step 4: Implement lazy caching, error eviction, authentication, and revision allocation**

Implement `EverythingSearchState::new` with an empty client slot and revision `0`. Hold the mutex only while reading, connecting, installing, or conditionally evicting the client; never hold it during `query` or path authentication.

Connect with:

```rust
EverythingClient::connect("", Duration::from_millis(250))
```

Evict only when the cached `Arc` is still pointer-equal to the client that failed. Evict on `InvalidInstance`, `IpcUnavailable`, `IpcSendFailed`, `ClientClosed`, `Protocol`, and `RequestIdExhausted`; preserve on `QueryTimedOut` and `Overloaded`.

For each returned item, derive expected kind from `attributes & 0x10`, call `authenticate_path`, omit that item on any authentication error, and preserve the original result order. Build display metadata from the authenticated path plus Query2 metadata, falling back to snapshot size and modified FILETIME when Query2 omits them. Convert FILETIME to an RFC 3339 UTC string with `FileTimeToSystemTime`; return `YYYY-MM-DDTHH:MM:SS.sssZ`, which satisfies the current frontend protocol parser.

After all entries are authenticated, allocate the revision with:

```rust
let index_revision = self
    .revision
    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1))
    .map_err(|_| EverythingSearchError::RevisionExhausted)?
    .checked_add(1)
    .ok_or(EverythingSearchError::RevisionExhausted)?;
```

Set published `total` later from `items.len()`; do not expose `EverythingQueryResult::total`.

- [ ] **Step 5: Add the isolated Everything 1.4 literal semantic gate**

Extend the existing isolated harness with an independent test oracle, not a second production encoder:

```rust
#[test]
#[ignore = "live gate: launches an isolated frozen Everything folder-index instance"]
fn real_literal_entity_queries_match_plain_filenames() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut harness = IsolatedEverything::prepare()?;
    let entries = create_literal_syntax_tree(&harness.indexed_root)?;
    harness.start(deadline)?;

    for (literal, expected_path) in entries {
        let encoded = literal
            .chars()
            .map(|scalar| format!("#x{:X}:", u32::from(scalar)))
            .collect::<String>();
        let result = harness.query(&encoded, 0, 200, deadline)?;
        assert_eq!(canonical_path_set(result.items.iter().map(|item| PathBuf::from(&item.full_path)))?, canonical_path_set([expected_path])?);
    }

    harness.shutdown()?;
    Ok(())
}
```

The fixture names must cover ASCII, spaces, Chinese text, `|`, `!`, `<`, `>`, quotes, `*`, `?`, and mixed input.

- [ ] **Step 6: Run adapter, IPC, and isolated semantic verification**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml file_search::everything::tests -- --nocapture
cargo test --manifest-path spikes/everything-ipc/Cargo.toml
cargo clippy --manifest-path spikes/everything-ipc/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/everything-ipc/Cargo.toml --test query_semantics real_literal_entity_queries_match_plain_filenames -- --ignored --exact --nocapture
```

Expected: all commands exit `0`; the isolated test launches only its frozen test instance and cleans it up; no installer or Service is touched.

- [ ] **Step 7: Commit Task 3 after planner review**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/file_search/mod.rs src-tauri/src/file_search/everything.rs spikes/everything-ipc/tests/query_semantics.rs
git commit -m "feat: add Everything file search adapter"
```

Report the commit hash, cache/eviction matrix, revision tests, isolated semantic result, and `git diff --check`. Wait for planner approval.

---

### Task 4: Route Production Commands Through Everything

**Files:**
- Modify: `src-tauri/src/commands.rs:372`
- Modify: `src-tauri/src/lib.rs:44`

**Interfaces:**
- Consumes: Task 2 backend-neutral publication/action types; Task 3 `EverythingSearchState::search` and `EverythingSearchError`.
- Produces: production `search_files` backed only by `EverythingSearchState`; unchanged Tauri command names and WebView response shape.

- [ ] **Step 1: Add failing command and state-wiring tests**

Replace old search helper expectations with tests for the frozen MVP contract:

```rust
#[test]
fn file_query_accepts_only_nonempty_all_modified_desc() {
    assert!(prepare_file_query("report".into(), "all".into(), "modifiedDesc".into(), "inv".into(), 1).is_ok());
    for invalid in [
        ("", "all", "modifiedDesc"),
        ("report", "pdf", "modifiedDesc"),
        ("report", "all", "modifiedAsc"),
    ] {
        assert_eq!(
            prepare_file_query(invalid.0.into(), invalid.1.into(), invalid.2.into(), "inv".into(), 1),
            Err(CommandError::invalid_file_query())
        );
    }
}

#[test]
fn production_file_search_uses_everything_once_and_never_legacy_index() {
    let calls = AtomicUsize::new(0);
    let response = block_on(search_files_with(
        &ready_registry("inv"),
        prepared_query("report", "inv", 1),
        move |query| {
            assert_eq!(query, "report");
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(everything_batch_for_test(7, 2))
        },
    ))
    .unwrap()
    .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(response.index_revision, "7");
    assert_eq!(response.total, "2");
    assert_eq!(response.status, FileIndexStatus::Ready);
}

#[test]
fn stale_everything_query_consumes_no_publication_slot() {
    let registry = ready_registry("inv");
    let old = registry.begin_query(QueryDomain::File, "inv", 1).unwrap();
    let _new = registry.begin_query(QueryDomain::File, "inv", 2).unwrap();
    assert!(publish_everything_search(&registry, old, everything_batch_for_test(8, 1)).is_none());
}

#[test]
fn everything_search_failures_map_to_path_free_unavailable_errors() {
    for error in [EverythingSearchError::Unavailable, EverythingSearchError::RevisionExhausted] {
        let command = map_everything_search_error(error);
        assert_eq!(command, CommandError::search_unavailable());
        assert!(!command.message.contains('\\'));
        assert!(!command.message.contains(':'));
    }
}
```

Update the source guard test so `require_main_window(&window)?;` remains the first statement and the production command body contains `everything_search.inner()` but contains none of `file_index.inner()`, `app.path()`, `app_data_dir`, or `FileIndex::search`.

Add `lib.rs` source guards requiring one `EverythingSearchState::new()`, one managed Everything state, unchanged `search_files`/`execute_result` handler names, and no new `refresh_files` command, permission, capability, or `build.rs` registration.

- [ ] **Step 2: Run focused tests and confirm the red state**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::file_query_accepts_only_nonempty_all_modified_desc -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::production_file_search_uses_everything_once_and_never_legacy_index -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml tests::production_file_search_state_commands_and_permissions_are_exact -- --nocapture
```

Expected: FAIL because production still receives `AppHandle`/`FileIndex` and calls the legacy index search.

- [ ] **Step 3: Replace the prepared query and search helper**

Use a production-only prepared value:

```rust
struct PreparedFileQuery {
    query: String,
    invocation_id: String,
    query_sequence: u64,
}
```

Preserve the current byte, scalar-count, NUL, invocation, and sequence limits, but require non-empty input and exact `category == "all"` and `sort == "modifiedDesc"`. Do not fold or normalize the query.

Change the command signature to:

```rust
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn search_files(
    window: WebviewWindow,
    registry: State<'_, ResultRegistry>,
    everything_search: State<'_, Arc<EverythingSearchState>>,
    query: String,
    category: String,
    sort: String,
    invocation_id: String,
    query_sequence: u64,
) -> Result<Option<FileSearchResponse>, CommandError> {
    require_main_window(&window)?;
    let prepared = prepare_file_query(query, category, sort, invocation_id, query_sequence)?;
    let state = Arc::clone(everything_search.inner());
    search_files_with(&registry, prepared, move |query| state.search(&query)).await
}
```

Implement `search_files_with` as a generic `FnOnce(String) -> Result<PublishedFileBatch, EverythingSearchError> + Send + 'static`, call `ResultRegistry::begin_query` before `spawn_blocking`, map worker join and backend errors to the existing unavailable errors, and publish only with the returned token.

- [ ] **Step 4: Publish authenticated drafts and preserve opaque execution**

Implement:

```rust
fn publish_everything_search(
    registry: &ResultRegistry,
    token: QueryToken,
    batch: PublishedFileBatch,
) -> Option<FileSearchResponse> {
    let total = batch.items.len();
    registry.publish_if_latest(
        token,
        batch.items.into_iter().map(|item| {
            let action = ResultAction::OpenFile(item.action.clone());
            (item, action)
        }).collect(),
        || true,
        |request_id, items| FileSearchResponse {
            request_id,
            index_revision: batch.index_revision.to_string(),
            total: total.to_string(),
            status: FileIndexStatus::Ready,
            items: items.into_iter().map(map_published_file_item).collect(),
        },
    )
}
```

Do not use Everything's database total. A stale publication returns `None`; its already allocated revision may become a gap.

- [ ] **Step 5: Manage the Everything state without removing legacy state**

In `run`, construct and manage:

```rust
let everything_search = Arc::new(file_search::everything::EverythingSearchState::new());
```

Keep the existing `FileIndex::new`, `.manage(Arc::clone(&file_index))`, and HWND installation because legacy source and execution actions remain compiled. Add exactly one `.manage(everything_search)`. Do not add commands or change capabilities, permissions, `build.rs`, installer hooks, or lifecycle setup.

- [ ] **Step 6: Run command and production-boundary verification**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml commands::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml result_registry -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml tests::production_file_search_state_commands_and_permissions_are_exact -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: all commands exit `0`; source guards prove the production search path has no `FileIndex::search` call or fallback; command/capability/build registration remains unchanged.

- [ ] **Step 7: Commit Task 4 after planner review**

```powershell
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: route find searches through Everything"
```

Report the commit hash, command signature, no-fallback evidence, full Rust gate results, and `git diff --check`. Wait for planner approval.

---

### Task 5: Simplify the Frontend to the Everything MVP Contract

**Files:**
- Modify: `src/protocol.ts:87`
- Modify: `src/main.ts:22`
- Modify: `src/launcher-core.ts:327`
- Modify: `src/launcher-view.tsx:247`
- Modify: `src/styles.css:44`
- Modify: `src/launcher.test.tsx:2445`

**Interfaces:**
- Consumes: unchanged `searchFiles` request and `FileSearchResponse`; fixed backend category `all`, sort `modifiedDesc`; unchanged opaque `executeResult` request.
- Produces: immediate explicit-query search, no file-index listener/timer, hidden category/sort controls, preserved result list/preview/Enter behavior.

- [ ] **Step 1: Replace listener and refresh tests with failing MVP tests**

Remove fake-client listener setup and add these tests before changing production code:

```tsx
it('enters find mode and searches immediately without a file-index listener', async () => {
  const fake = createFakeLauncherClient()
  const core = createLauncherCore(fake.client, publish)
  fake.emit(shown('find-immediate', '/find report'))
  await flushPromises()
  expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
  expect(fake.client.searchFiles).toHaveBeenCalledWith({
    query: 'report',
    category: 'all',
    sort: 'modifiedDesc',
    invocationId: 'find-immediate',
    querySequence: expect.any(Number),
  })
})

it('does not search an empty find query and starts on the next nonempty edit', async () => {
  const fake = createFakeLauncherClient()
  const core = createLauncherCore(fake.client, publish)
  fake.emit(shown('find-empty', '/find'))
  await flushPromises()
  expect(fake.client.searchFiles).not.toHaveBeenCalled()
  core.handleInput('/find a')
  await flushPromises()
  expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
})

it('starts no streaming or revision refresh timer', async () => {
  vi.useFakeTimers()
  const fake = createFakeLauncherClient()
  const core = createLauncherCore(fake.client, publish)
  fake.emit(shown('find-no-timer', '/find report'))
  await vi.runAllTicks()
  await vi.advanceTimersByTimeAsync(10_000)
  expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
  core.dispose()
  vi.useRealTimers()
})

it('renders no category strip or sort control and keeps preview metadata', async () => {
  render(<LauncherView core={readyFileCore()} />)
  expect(screen.queryByRole('tablist', { name: '文件类型' })).toBeNull()
  expect(screen.queryByRole('button', { name: /修改时间/ })).toBeNull()
  expect(screen.getByRole('complementary', { name: '文件预览' })).toHaveTextContent('完整路径')
})
```

Keep or rewrite existing tests for stale late responses, unavailable status, keyboard selection, and Enter. Assert Enter still calls:

```ts
expect(fake.client.executeResult).toHaveBeenCalledWith({
  requestId: 'req-0000000000000001',
  resultId: 'res-0000000000000001',
})
```

- [ ] **Step 2: Run focused frontend tests and confirm the red state**

Run:

```powershell
npm.cmd test -- src/launcher.test.tsx
```

Expected: FAIL because listener registration, timers, category strip, and sort button still exist.

- [ ] **Step 3: Remove the file-index event protocol and bridge**

Delete `LauncherClient.listenFileIndexChanged`, `FileIndexChanged`, and `parseFileIndexChanged` from `protocol.ts`. Delete this bridge entry from `main.ts`:

```ts
listenFileIndexChanged: (handler) => listen<unknown>('file-index://changed', (event) => handler(event.payload)),
```

Keep `listenShown`, `searchFiles`, and `executeResult` unchanged.

- [ ] **Step 4: Remove listener ownership, polling, and refresh timers from launcher core**

Delete listener registration state, `fileRefreshTimer`, `fileRefreshMaxTimer`, `fileStreamingPollTimer`, `fileRefreshRequired`, `ensureFileListener`, `fileIndexChanged`, `runFileRefresh`, `runFileStreamingPoll`, and their cleanup branches.

Enter file mode without waiting for an event subscription:

```ts
async function enterFileMode(query: string): Promise<void> {
  cancelApplicationSearch()
  clearFileSearchOwnership()
  fileQuery = query
  fileCategory = 'all'
  fileSort = 'modifiedDesc'
  publishFileState()
  if (query.length === 0) return
  await startFileSearch(query, 'all', 'modifiedDesc')
}
```

Leave the existing search owner token, view epoch, invocation ID, category, sort, and sequence checks in the response path. Keep `setFileCategory` and `setFileSort` in the core interface as compatibility no-ops that restore fixed state without starting a search:

```ts
function setFileCategory(_category: FileCategory): void {
  fileCategory = 'all'
}

function setFileSort(_sort: FileSort): void {
  fileSort = 'modifiedDesc'
}
```

- [ ] **Step 5: Remove category/sort interaction and adjust the file layout**

Delete category keyboard cycling and sort-toggle keyboard handling from `launcher-view.tsx`. Delete the category strip JSX and the sort button, but keep the result list, preview `<aside>`, preview toggle, status line, and Enter behavior.

Change the desktop file grid from a category-bearing layout to:

```css
.file-workspace {
  grid-template-areas:
    "query query"
    "results preview"
    "toolbar toolbar";
  grid-template-columns: minmax(0, 1fr) minmax(160px, 200px);
  grid-template-rows: 44px minmax(0, 1fr) auto;
}

@media (max-width: 600px) {
  .file-workspace {
    grid-template-areas:
      "query"
      "results"
      "preview"
      "toolbar";
  }
}
```

Remove `.file-category-strip` rules and its responsive overrides. Keep the existing preview and result responsive behavior without overlap.

- [ ] **Step 6: Run focused and full frontend verification**

Run:

```powershell
npm.cmd test -- src/launcher.test.tsx
npm.cmd test
npm.cmd run build
```

Expected: all commands exit `0`; no test or source reference to `file-index://changed`, `fileRefreshTimer`, `fileStreamingPollTimer`, `.file-category-strip`, or rendered sort controls remains; preview and opaque Enter execution tests pass.

- [ ] **Step 7: Commit Task 5 after planner review**

```powershell
git add src/protocol.ts src/main.ts src/launcher-core.ts src/launcher-view.tsx src/styles.css src/launcher.test.tsx
git commit -m "feat: simplify find for Everything search"
```

Report the commit hash, removed listener/timer/control symbols, frontend test/build results, and `git diff --check`. Wait for planner approval.

---

### Task 6: Run End-to-End Verification and Manual Acceptance

**Files:**
- Modify only files required to fix a failure caused by Tasks 1-5; every fix must receive a new failing regression test and a separate reviewed commit.

**Interfaces:**
- Consumes: the complete Rust adapter, command wiring, registry actions, shared path execution, and frontend MVP flow.
- Produces: final evidence that the approved milestone is complete without expanding scope.

- [ ] **Step 1: Verify the changed-file boundary**

Run:

```powershell
git status --short
git diff --name-only bd8c557af5ab316de7b1f0464832e57ea9692708..HEAD
git diff --check bd8c557af5ab316de7b1f0464832e57ea9692708..HEAD
```

Expected: only the plan-approved production, frontend, IPC semantic-test, and plan/spec files appear in committed diffs. Existing unrelated installer/resource files may remain dirty or untracked but are not staged or committed.

- [ ] **Step 2: Run all deterministic Rust gates**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path spikes/everything-ipc/Cargo.toml
cargo clippy --manifest-path spikes/everything-ipc/Cargo.toml --all-targets -- -D warnings
```

Expected: every command exits `0` with no warnings denied by Clippy.

- [ ] **Step 3: Run all frontend gates**

Run:

```powershell
npm.cmd test
npm.cmd run build
```

Expected: Vitest and the TypeScript/Vite production build exit `0`.

- [ ] **Step 4: Run the isolated Everything literal semantic gate**

Run:

```powershell
cargo test --manifest-path spikes/everything-ipc/Cargo.toml --test query_semantics real_literal_entity_queries_match_plain_filenames -- --ignored --exact --nocapture
```

Expected: the frozen isolated Everything 1.4 instance returns only literal filename matches for all syntax fixtures and is shut down by the harness.

- [ ] **Step 5: Run the manual default-instance `/find` smoke**

Precondition: the user manually starts the default Everything 1.4 instance and confirms its database is loaded. Start UiPilot in development mode:

```powershell
npm.cmd run tauri dev
```

Verify in `/find`:

1. A plain ASCII query returns no more than 200 results.
2. A query containing spaces, Chinese text, or `| ! < > " * ?` behaves as literal filename text.
3. Visible modified times are descending for the returned snapshot.
4. Enter on a file reveals and selects it in Explorer.
5. Enter on a folder opens it.
6. Rapid edits cannot let an older response replace the newest query.
7. Category and sort controls are absent; metadata preview remains available.
8. After manually exiting Everything, the next text edit reaches the unavailable state quickly, does not start Everything, and does not show old-index fallback results.

Record counts and pass/fail outcomes without recording local result paths.

- [ ] **Step 6: Run final source-contract scans**

Run:

```powershell
rg -n "file-index://changed|fileRefreshTimer|fileStreamingPollTimer|file-category-strip" src
rg -n "FileIndex::search|file_index\.inner\(\)|app_data_dir" src-tauri/src/commands.rs
rg -n "refresh_files|allow-refresh-files" src-tauri/src src-tauri/build.rs src-tauri/capabilities
rg -n "SHOpenFolderAndSelectItems|ShellExecuteExW" src-tauri/src/file_search src-tauri/src/file_index
```

Expected: the first three scans return no production match; the Shell APIs occur only in the shared `file_search/windows/path_auth.rs` implementation and its tests.

- [ ] **Step 7: Request final two-stage review**

Dispatch one spec-compliance reviewer against `2026-07-30-find-everything-mvp-integration-design.md`, then one code-quality reviewer against the approved implementation commits. Critical or important findings block completion and must be fixed with a regression test and a separate commit; minor findings are recorded for planner judgment.

The final report must include:

- all Task commit hashes;
- deterministic gate results;
- isolated semantic result;
- manual smoke result;
- no-fallback and no-background-refresh evidence;
- final reviewer findings and disposition;
- `git status --short` proving unrelated work was not staged.
