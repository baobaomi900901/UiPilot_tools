# Foundation Task 4B Validation Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现唯一 `ValidationStore`、日级并发计数、open-session marker 和 exactly-once 启动对账，只统计 `uncleanSessions`。

**Architecture:** `validation_data.rs` 复用 Task 4A 原子文件 helper，以一个 `Mutex<ValidationStoreState>` 管理持久化来源和当前 session 身份；`session_marker.rs` 只处理随机 ID、marker 文件和严格所有权。Rust setup 在任何验证事件或未来 command 暴露前完成 load/reconcile/open。

**Tech Stack:** Rust 1.77.2、Tauri 2.11.3、Serde/serde_json、现有 `windows 0.61.3` 的 `GetLocalTime`、Windows CNG 和 `MoveFileExW`。

**Status:** No-Go for TDD until this implementation plan is approved and Task 4A is complete.

**Source Design:** `docs/superpowers/specs/2026-07-18-task-4b-validation-store-design.md`

## Global Constraints

- 前置产物是已批准并完成的 Task 4A；只复用其 `atomic_file.rs`，不复制原子写入协议。
- 不新增 dependency，不创建 Tauri command，不修改 capability、`invoke_handler` 或 `src/protocol.ts`。
- 不安装 `SetUnhandledExceptionFilter`，不使用 `catch_unwind` 生成证据，不创建 confirmed-crash marker，不保存或导出 `hostCrashes`。
- 只保存 schema、日期级计数和内部 `lastReconciledSessionId`；不保存精确时间、查询、应用、路径或 session ID 到日志/导出。
- `record`、clear、reconcile 和 clean exit 全部使用同一个 store mutex；不嵌套获取 `SettingsStore` 或 `AppCache` 锁。
- 每个非平凡分支先写失败测试，再写最小实现；每个任务单独提交。

## Interfaces

Task 4B 消费 Task 4A：

```rust
pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AtomicFileError>;
pub(crate) fn quarantine_invalid(path: &Path) -> Result<(), AtomicFileError>;
pub(crate) fn commit_with_backup(
    paths: &AtomicPaths,
    previous: Option<&[u8]>,
    candidate: &[u8],
) -> Result<(), AtomicFileError>;
pub(crate) fn replace_without_backup(
    destination: &Path,
    candidate: &[u8],
) -> Result<(), AtomicFileError>;
```

Task 4B 产出：

```rust
impl ValidationStore {
    pub(crate) fn load(app_data_dir: &Path) -> Result<Self, ValidationError>;
    pub(crate) fn record(&self, event: ValidationEvent) -> Result<(), ValidationError>;
    pub(crate) fn clear_daily_counts(&self) -> Result<(), ValidationError>;
    pub(crate) fn export_snapshot(&self) -> ValidationCountsSnapshot;
    pub(crate) fn reconcile_and_open_session(&self) -> Result<(), ValidationError>;
    pub(crate) fn mark_clean_exit(&self) -> Result<(), ValidationError>;
}
```

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationEvent {
    LauncherInvoked,
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested,
}
```

---

### Task 1: Implement persisted validation counts

**Files:**
- Create: `src-tauri/src/validation_data.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/validation_data.rs`

**Interfaces:**
- Consumes: Task 4A atomic load/quarantine/commit functions.
- Produces: validated daily state, `record`, clear and snapshot; session methods remain unimplemented until Task 2.

- [ ] **Step 1: Write failing validation-state tests**

Add `#[cfg(test)] mod validation_data;` to `lib.rs` so the RED run includes the new file. Start with the exact event mapping:

```rust
#[test]
fn activation_refusal_counts_refusal_and_launch_request_once() {
    let store = open_test_store("2026-07-18");

    store
        .record(ValidationEvent::ActivationRefusedLaunchRequested)
        .unwrap();

    let day = &store.export_snapshot().daily_counts["2026-07-18"];
    assert_eq!(day.activation_refusals, 1);
    assert_eq!(day.application_launch_requests, 1);
    assert_eq!(day.activation_successes, 0);
}
```

Add tests for all four events, same-day/cross-day aggregation, `checked_add` overflow, clear preserving `last_reconciled_session_id`, `record` returning `SessionNotOpen`, strict date keys, malformed current/backup recovery and path-free errors. Strict dates must reject wrong separators/widths, month 0/13, day 0, impossible month days and invalid leap days without adding a date crate.

Add two `Barrier`-started record threads and assert both increments survive in memory/current. Add defaults/backup recovery followed by two writes; the first sets `current_is_valid = true`, the second stores the first committed validation value as backup.

Use module-private `record_with(event, date_provider, persist)` and `clear_with(persist)` closure seams. For each operation, make `persist` return every Task 4A `AtomicFileError` category in turn. Every case must keep the approved old in-memory/current state; Task 4A tests remain responsible for the physical candidate-write/backup-write/backup-replace/current-replace disk matrix.

Serialize `export_snapshot()` in a test and assert it contains only `schemaVersion` and `dailyCounts`, with no last reconciled ID, current session ID, marker, path or raw input.

- [ ] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_data
```

Expected: compile failure inside the included `validation_data` module because its persisted types/store do not exist.

- [ ] **Step 3: Add the exact persisted types**

Replace the test-only module declaration with `#[cfg_attr(not(test), allow(dead_code))] mod validation_data;`; Task 5 removes the scoped allowance when it consumes the service.

Add only the required Windows feature:

```toml
"Win32_System_SystemInformation",
```

Implement the approved shapes:

```rust
pub(crate) const VALIDATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyCounts {
    pub(crate) launcher_invocations: u64,
    pub(crate) application_launch_requests: u64,
    pub(crate) activation_successes: u64,
    pub(crate) activation_refusals: u64,
    pub(crate) unclean_sessions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationState {
    schema_version: u32,
    daily_counts: BTreeMap<String, DailyCounts>,
    last_reconciled_session_id: Option<String>,
}

struct ValidationStoreState {
    value: ValidationState,
    current_is_valid: bool,
    current_session_id: Option<String>,
}

pub(crate) struct ValidationStore {
    paths: ValidationPaths,
    state: Mutex<ValidationStoreState>,
}

struct ValidationPaths {
    data: AtomicPaths,
    marker: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidationCountsSnapshot {
    pub(crate) schema_version: u32,
    pub(crate) daily_counts: BTreeMap<String, DailyCounts>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationError {
    Storage,
    Serialize,
    InvalidDate,
    CounterOverflow,
    SessionNotOpen,
    SessionAlreadyOpen,
    SessionOwnershipLost,
    SessionRandom,
}

impl From<AtomicFileError> for ValidationError {
    fn from(_: AtomicFileError) -> Self {
        Self::Storage
    }
}
```

Give every `ValidationError` variant one fixed path-free `Display` string and implement `std::error::Error`. `ValidationState::default()` must set schema version 1 with empty counts and no last reconciled ID.

`ValidationStore::load` follows current -> backup -> defaults through Task 4A. Loading current sets `current_is_valid = true`; backup/defaults sets false; every process initializes `current_session_id = None`. Validate schema version, every date key and `lastReconciledSessionId` format (`session-` plus 32 lowercase hex bytes). Invalid files are quarantined; permission and other I/O errors fail.

- [ ] **Step 4: Implement single-lock count transactions**

Production date uses `GetLocalTime` and fixed `YYYY-MM-DD`; tests inject a private date-provider closure. Implement leap-year/month-day validation with standard library arithmetic.

For `record`, reject a missing `current_session_id`, clone `guard.value`, mutate with `checked_add`, serialize old/candidate, persist while holding the same lock, then update without changing the current session:

```rust
*guard = ValidationStoreState {
    value: candidate,
    current_is_valid: true,
    current_session_id: existing_session_id,
};
```

The private seams have exact persistence callbacks and production delegates them to `commit_with_backup`:

```rust
fn record_with<D, P>(
    &self,
    event: ValidationEvent,
    date_provider: D,
    persist: P,
) -> Result<(), ValidationError>
where
    D: FnOnce() -> Result<String, ValidationError>,
    P: FnOnce(&AtomicPaths, Option<&[u8]>, &[u8]) -> Result<(), AtomicFileError>;

fn clear_with<P>(&self, persist: P) -> Result<(), ValidationError>
where
    P: FnOnce(&AtomicPaths, Option<&[u8]>, &[u8]) -> Result<(), AtomicFileError>;
```

`clear_daily_counts` only clears the map and preserves `last_reconciled_session_id` plus the in-memory session ID. `export_snapshot` clones only schema/daily counts in a short lock and exposes no session fields.

- [ ] **Step 5: Run focused tests and confirm GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_data
cargo test --manifest-path src-tauri/Cargo.toml atomic_file
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: focused tests pass and Clippy exits 0.

- [ ] **Step 6: Commit Task 1**

```powershell
git add src-tauri/Cargo.toml src-tauri/src/validation_data.rs src-tauri/src/lib.rs
git commit -m "feat: persist daily validation counts"
```

---

### Task 2: Add exactly-once session reconciliation

**Files:**
- Create: `src-tauri/src/session_marker.rs`
- Modify: `src-tauri/src/validation_data.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/session_marker.rs`
- Test: `src-tauri/src/validation_data.rs`

**Interfaces:**
- Consumes: `ValidationStoreState`, Task 4A marker replace/quarantine functions and existing Windows CNG feature.
- Produces: `reconcile_and_open_session` and ownership-safe `mark_clean_exit`.

- [ ] **Step 1: Write failing marker ownership tests**

Add `#[cfg(test)] mod session_marker;` to `lib.rs` so the RED run includes the new file. Add this repeated-open regression:

```rust
#[test]
fn repeated_open_fails_without_touching_marker_or_state() {
    let store = loaded_test_store();
    store.reconcile_and_open_session().unwrap();
    let before = store.test_state();
    let marker_before = fs::read(store.marker_path()).unwrap();

    assert_eq!(
        store.reconcile_and_open_session(),
        Err(ValidationError::SessionAlreadyOpen),
    );
    assert_eq!(store.test_state(), before);
    assert_eq!(fs::read(store.marker_path()).unwrap(), marker_before);
}
```

The module-local test interface is exact and is not compiled into production:

```rust
#[cfg(test)]
impl ValidationStore {
    fn test_state(&self) -> (ValidationState, bool, Option<String>);
    fn marker_path(&self) -> &Path;
}
```

Add tests for missing/malformed marker, stale marker incrementing once, already-reconciled marker not incrementing twice, and all four crash boundaries: before validation persist, after persist/before marker replace, after marker replace and after clean deletion. Reopen from disk repeatedly and assert exactly-once.

Add clean-exit tests for matching ID deletion, missing/damaged/mismatched marker returning `SessionOwnershipLost` without deletion or ID clearing, and a second clean call succeeding without disk access. Assert record fails before open and after successful clean.

Add a clear test with an open session: `dailyCounts` becomes empty while both `lastReconciledSessionId` and the exact current marker bytes remain unchanged. Simulate force termination by dropping the first store without clean exit, reload from the same directory, and assert the next open increments `uncleanSessions` once.

- [ ] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml session_marker
cargo test --manifest-path src-tauri/Cargo.toml reconcile_and_open_session
```

Expected: compile failure inside the included marker module because marker/session methods do not exist.

- [ ] **Step 3: Implement opaque session IDs and marker I/O**

Replace the test-only declaration with `#[cfg_attr(not(test), allow(dead_code))] mod session_marker;`; Task 6 removes the scoped allowance when it consumes clean-exit integration.

Use Windows CNG system RNG only:

```rust
fn new_session_id() -> Result<String, ValidationError> {
    let mut bytes = [0_u8; 16];
    let status = unsafe {
        BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    };
    if status.is_err() {
        return Err(ValidationError::SessionRandom);
    }
    let mut id = String::with_capacity(40);
    id.push_str("session-");
    for byte in bytes {
        write!(id, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(id)
}
```

`SessionMarker` contains only schema version, ID and local date. Write `open-session.json` through `replace_without_backup`. Malformed marker JSON/schema/ID/date is quarantined with a fixed category and does not increment `uncleanSessions`. Tests inject fixed IDs and I/O closures; production never accepts an ID/date/path from frontend.

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionMarker {
    schema_version: u32,
    session_id: String,
    local_date: String,
}
```

- [ ] **Step 4: Implement the locked reconciliation state machine**

Hold `Mutex<ValidationStoreState>` for the entire operation. Before any file read, reject `current_session_id.is_some()` with `SessionAlreadyOpen`. Then execute exactly:

```rust
if stale_marker_id != guard.value.last_reconciled_session_id {
    let mut candidate = guard.value.clone();
    increment_unclean_for_marker_date(&mut candidate, marker_date)?;
    candidate.last_reconciled_session_id = Some(stale_marker_id.clone());
    persist_validation_candidate(&mut guard, candidate)?;
}

let new_id = id_provider()?;
replace_marker(&new_id, current_local_date()?)?;
guard.current_session_id = Some(new_id);
```

If validation persist succeeds and marker replacement fails, keep memory synchronized with the persisted validation state but leave `current_session_id = None`. The local session starts only after marker replacement succeeds.

`mark_clean_exit` holds the same lock. `None` is idempotent success without marker access. For `Some(id)`, read a structurally valid marker and require an exact ID match; missing/malformed/mismatch returns `SessionOwnershipLost` without deletion or ID clearing. Delete the matching marker, then set the in-memory ID to `None`.

- [ ] **Step 5: Run marker and validation tests and confirm GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml session_marker
cargo test --manifest-path src-tauri/Cargo.toml validation_data
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all session/count tests pass and Clippy exits 0.

- [ ] **Step 6: Commit Task 2**

```powershell
git add src-tauri/src/session_marker.rs src-tauri/src/validation_data.rs src-tauri/src/lib.rs
git commit -m "feat: reconcile unclean validation sessions"
```

---

### Task 3: Open and manage the validation session during setup

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Tauri app-data path and `ValidationStore::load/reconcile_and_open_session`.
- Produces: one managed, open `ValidationStore` for Task 5/6.

- [ ] **Step 1: Write a failing production-path helper test**

Write a unit test that calls the following planned private helper, but do not define the helper yet:

```rust
let _store = load_and_open_validation_store(test_dir.path()).unwrap();
assert!(test_dir.path().join("open-session.json").exists());
```

Test that it creates a marker before returning. Task 2 already injects marker-replace failure through the shared reconciliation path and proves no half-open store is returned; do not duplicate that failure seam in `lib.rs`.

- [ ] **Step 2: Run the helper test and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml load_and_open_validation_store
```

Expected: compile failure because the helper is absent.

- [ ] **Step 3: Wire setup before exposure**

Add the production helper through the same path tested in RED:

```rust
fn load_and_open_validation_store(
    app_data_dir: &Path,
) -> Result<ValidationStore, ValidationError> {
    let store = ValidationStore::load(app_data_dir)?;
    store.reconcile_and_open_session()?;
    Ok(store)
}
```

After obtaining the same app-data directory used by Task 4A, load/open and manage exactly one store:

```rust
let validation = load_and_open_validation_store(&app_data_dir)?;
assert!(_app.manage(validation), "validation store already managed");
```

This must happen before any future command registration or visible launcher event can record data. Do not wire tray exit, `RunEvent`, `WM_ENDSESSION` or record calls; Task 6 and Task 5 own those integrations.

- [ ] **Step 4: Run the Task 4B completion gate**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml session_marker
cargo test --manifest-path src-tauri/Cargo.toml validation_data
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run build
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
$forbidden = rg -n "SetUnhandledExceptionFilter|hostCrashes|host_crashes|confirmed-crash" src-tauri src
if ($LASTEXITCODE -eq 0) { throw "forbidden crash instrumentation found`n$forbidden" }
if ($LASTEXITCODE -ne 1) { throw "forbidden-symbol scan failed" }
git diff --check
```

Expected: tests/build/checks and the wrapped forbidden-symbol scan exit 0; no command, capability, `src/protocol.ts`, crash marker or Task 5 action file is added.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/Cargo.toml src-tauri/src
git commit -m "feat: open the validation session at startup"
```

## Completion Gate

Task 4B is complete only after Task 4A is complete, all three Task 4B commits exist and the full gate passes on Windows 11 x64. Completion exposes only a crate-private store; Task 5 command/event recording and Task 6 clean-exit wiring remain separate work.
