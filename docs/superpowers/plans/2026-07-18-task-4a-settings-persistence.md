# Foundation Task 4A Settings Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Windows 原子文件协议、唯一 `SettingsStore`、应用别名和 `useCounts` 持久化，不注册任何 Tauri command。

**Architecture:** `atomic_file.rs` 提供 crate-private 的同目录 temp、同步和原子替换函数；`settings.rs` 在一个 `Mutex<SettingsState>` 内完成完整读改写事务。Rust setup 只在非 `test-instrumentation` 构建加载并托管一个 store，Task 5/6 后续复用它。

**Tech Stack:** Rust 1.77.2、Tauri 2.11.3、Serde/serde_json、标准库文件 API、现有 `windows 0.61.3` 的 `MoveFileExW`。

**Status:** No-Go for TDD until this implementation plan is approved.

**Source Design:** `docs/superpowers/specs/2026-07-18-task-4a-settings-persistence-design.md`

## Global Constraints

- 仅支持当前 Windows 11 x64 Foundation 范围；不新增 crate 或 npm dependency。
- 不创建 Tauri command，不修改 capability、`invoke_handler` 或 `src/protocol.ts`。
- 不应用 hotkey/autostart 副作用，不实现搜索、动作、隐藏、重扫或验证计数。
- `test-instrumentation` 构建不得查询 app-data path、加载/隔离设置或 manage `SettingsStore`；安全探针前后 settings/validation/marker 文件集合、内容和元数据必须相同。
- 所有磁盘路径由 Rust 从 Tauri application data directory 构造；前端不提供路径或文件名。
- 错误和日志只包含固定类别，不包含路径、临时文件名、`appId`、research ID 或别名。
- 每个非平凡分支先写失败测试，再写最小实现；每个任务单独提交。
- 三份计划批准后、任何 TDD 命令前，执行者必须在批准计划提交上创建且不得移动 lightweight tag `foundation-task-4-approved-plan`；完成门禁以该 tag 到 HEAD 加工作区计算完整范围。

## Interfaces

Task 4A 消费 Task 3 的唯一 `Arc<AppCache>`：

```rust
impl AppCache {
    pub(crate) fn snapshot(&self) -> Vec<Application>;
    pub(crate) fn contains(&self, app_id: &str) -> bool;
}
```

Task 4A 产出：

```rust
pub(crate) struct AtomicPaths {
    current: PathBuf,
    backup: PathBuf,
}

impl AtomicPaths {
    pub(crate) fn new(directory: &Path, file_name: &str) -> Self;
    pub(crate) fn current(&self) -> &Path;
    pub(crate) fn backup(&self) -> &Path;
}

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

```rust
impl SettingsStore {
    pub(crate) fn load(app_data_dir: &Path) -> Result<Self, SettingsError>;
    pub(crate) fn decorate_applications(&self, applications: &mut [Application]);
    pub(crate) fn update_user_settings(
        &self,
        update: SettingsUpdate,
        cache: &AppCache,
    ) -> Result<(), SettingsError>;
    pub(crate) fn increment_use_count(
        &self,
        app_id: &str,
        cache: &AppCache,
    ) -> Result<(), SettingsError>;
    pub(crate) fn research_id(&self) -> Option<String>;
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> Settings;
}
```

## Execution Baseline

After written approval and before the first RED command, pin the approved plan commit once:

```powershell
$approved = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw "cannot resolve approved plan commit" }
$tagName = 'foundation-task-4-approved-plan'
$tagExists = git tag --list $tagName
if ($LASTEXITCODE -ne 0) { throw "could not inspect approved plan tag" }
if ($tagExists) {
  $existing = git rev-parse "refs/tags/$tagName"
  if ($LASTEXITCODE -ne 0 -or $existing -ne $approved) {
    throw "foundation-task-4-approved-plan already points elsewhere"
  }
} else {
  git tag foundation-task-4-approved-plan $approved
  if ($LASTEXITCODE -ne 0) { throw "could not create approved plan tag" }
}
```

Do not move or recreate this tag during Tasks 4A/4B/4C. If an approved plan changes, stop execution and revise/re-review the baseline contract before continuing.

---

### Task 1: Add the atomic byte-file helper

**Files:**
- Create: `src-tauri/src/atomic_file.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/atomic_file.rs`

**Interfaces:**
- Consumes: host-constructed `Path` values only.
- Produces: the exact `AtomicPaths`, read, quarantine, backup commit and no-backup replace interfaces above.

- [ ] **Step 1: Write failing atomic protocol tests**

Add `#[cfg(test)] mod atomic_file;` to `lib.rs` so the RED run cannot ignore the new file. Add module-local tests using a unique `std::env::temp_dir()` directory and a `Drop` cleanup guard. The first regression must prove the state transition that both stores depend on:

```rust
#[test]
fn second_commit_preserves_first_commit_as_backup() {
    let dir = TestDir::new("atomic-two-commits");
    let paths = AtomicPaths::new(dir.path(), "settings.json");

    commit_with_backup(&paths, None, br#"{"value":1}"#).unwrap();
    commit_with_backup(
        &paths,
        Some(br#"{"value":1}"#),
        br#"{"value":2}"#,
    )
    .unwrap();

    assert_eq!(fs::read(paths.current()).unwrap(), br#"{"value":2}"#);
    assert_eq!(fs::read(paths.backup()).unwrap(), br#"{"value":1}"#);
}
```

Add focused tests for candidate-temp failure, backup-temp failure, backup move failure and current move failure. Use the private closure seam `commit_with(paths, previous, candidate, write_synced, replace)` so each test injects one failing call while delegating other calls to the real helper. Assert the approved disk state matrix and that temp files are never loaded as current/backup.

The private seam is fixed as:

```rust
fn commit_with<W, R>(
    paths: &AtomicPaths,
    previous: Option<&[u8]>,
    candidate: &[u8],
    mut write_synced: W,
    mut replace: R,
) -> Result<(), AtomicFileError>
where
    W: FnMut(&Path, &[u8]) -> io::Result<()>,
    R: FnMut(&Path, &Path, MOVE_FILE_FLAGS) -> io::Result<()>;
```

Production `commit_with_backup` delegates to this seam with the real write/sync and `MoveFileExW` functions. No trait or second filesystem implementation is added.

- [ ] **Step 2: Run the focused tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml atomic_file
```

Expected: compile failure inside the included `atomic_file` module because its interfaces do not exist.

- [ ] **Step 3: Implement the minimal Windows helper**

Replace the test-only module declaration with the scoped production declaration:

```rust
#[cfg_attr(not(test), allow(dead_code))]
mod atomic_file;
```

Implement fixed, path-free errors and exact replace flags:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicFileError {
    Read,
    CandidateWrite,
    BackupWrite,
    BackupReplace,
    CurrentReplace,
    InvalidQuarantine,
}

fn replace_flags() -> MOVE_FILE_FLAGS {
    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
}
```

Give every `AtomicFileError` variant one fixed path-free `Display` string and implement `std::error::Error`.

`write_synced` must use `OpenOptions::new().write(true).create_new(true)`, `write_all`, `sync_all`, then drop the handle before replacement. Temp and `.invalid-*` names use only the destination base name, `std::process::id()` and one process-global `AtomicU64`; current, backup and every temp remain siblings. Convert paths to NUL-terminated UTF-16 without lossy APIs and call `MoveFileExW`; never set `MOVEFILE_COPY_ALLOWED`.

`commit_with_backup` must perform exactly:

```rust
write_synced(current_temp, candidate)?;
if let Some(previous) = previous {
    write_synced(backup_temp, previous)?;
    replace(backup_temp, paths.backup(), replace_flags())?;
}
replace(current_temp, paths.current(), replace_flags())?;
```

On failure, best-effort remove only temps created by that call and return the primary fixed category. `read_optional` maps only `NotFound` to `Ok(None)`; every other read error is `Read`. `quarantine_invalid` moves to a unique sibling without overwrite. `replace_without_backup` performs candidate temp write/sync/close followed by one current replacement.

- [ ] **Step 4: Run atomic tests and confirm GREEN**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml atomic_file
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: atomic tests pass and Clippy exits 0.

- [ ] **Step 5: Commit Task 1**

```powershell
git add src-tauri/src/atomic_file.rs src-tauri/src/lib.rs
git commit -m "feat: add atomic settings file protocol"
```

---

### Task 2: Implement `SettingsStore`

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/settings.rs`

**Interfaces:**
- Consumes: Task 1 atomic helper and Task 3 `AppCache`/`Application`.
- Produces: the exact `SettingsStore` methods defined above; no command DTO.

- [ ] **Step 1: Write failing settings validation and transaction tests**

Add `#[cfg(test)] mod settings;` to `lib.rs` so the RED run includes the new file. Define test builders for one valid app whose ID is `app-` plus 64 lowercase hex characters. Add this required preservation regression:

```rust
#[test]
fn user_update_preserves_use_counts_and_absent_aliases() {
    let cache = AppCache::from_apps(vec![application(APP_A)]);
    let store = store_with(Settings {
        aliases: BTreeMap::from([
            (APP_A.into(), vec!["old".into()]),
            (APP_ABSENT.into(), vec!["keep".into()]),
        ]),
        use_counts: BTreeMap::from([(APP_A.into(), 7)]),
        ..Settings::default()
    });

    store
        .update_user_settings(
            SettingsUpdate {
                hotkey: "Alt+Space".into(),
                autostart: false,
                research_id: Some("study_01".into()),
                aliases: BTreeMap::from([(APP_A.into(), vec!["new".into()])]),
            },
            &cache,
        )
        .unwrap();

    let value = store.snapshot();
    assert_eq!(value.use_counts[APP_A], 7);
    assert_eq!(value.aliases[APP_ABSENT], ["keep"]);
}
```

Add tests for defaults, current -> backup -> defaults loading, invalid-file quarantine, malformed `appId`, temporarily absent valid IDs, unknown update keys, unknown count increments, checked overflow, decoration by stable ID and duplicate display names. Test research ID `A`, a 64-byte allowed value, and rejection of `Some("")`, whitespace, non-ASCII and 65 bytes in both load and update paths.

Add the two-thread `Barrier` regression from the approved design: two increments of the same known ID finish at 2 in memory and current. Add defaults/backup recovery tests where the first successful save sets `current_is_valid = true` and the second save writes the first value to backup.

- [ ] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml settings
```

Expected: compile failure inside the included `settings` module because the persisted types/store do not exist.

- [ ] **Step 3: Implement the persisted structures and validators**

Replace the test-only module declaration with `#[cfg_attr(not(test), allow(dead_code))] mod settings;`; Task 5 removes the scoped allowance when it consumes the service.

Use the approved shapes exactly:

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) use_counts: BTreeMap<String, u64>,
}

pub(crate) struct SettingsUpdate {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
}

struct SettingsState {
    value: Settings,
    current_is_valid: bool,
}

pub(crate) struct SettingsStore {
    paths: AtomicPaths,
    state: Mutex<SettingsState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsError {
    Storage,
    Serialize,
    InvalidUpdate,
    UnknownApplication,
    CountOverflow,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".into(),
            autostart: false,
            research_id: None,
            aliases: BTreeMap::new(),
            use_counts: BTreeMap::new(),
        }
    }
}

impl From<AtomicFileError> for SettingsError {
    fn from(_: AtomicFileError) -> Self {
        Self::Storage
    }
}
```

Give every `SettingsError` variant one fixed path-free `Display` string and implement `std::error::Error` so setup can use `?` without exposing an underlying path.

Implement research ID validation with standard-library byte checks only:

```rust
fn valid_research_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
```

Implement `valid_app_id` as length 68, prefix `app-`, followed by exactly 64 lowercase ASCII hex bytes. Use the same validators after deserialization and before an update. An invalid current/backup is quarantined through Task 1; only both missing/invalid yields defaults.

`SettingsStore::load` first calls `fs::create_dir_all(app_data_dir)` and maps failure to `SettingsError::Storage`; it then constructs only `settings.json`/backup paths under that directory. It must never create or accept another root.

- [ ] **Step 4: Implement the single-lock mutations**

Each mutation acquires `Mutex<SettingsState>` once, clones `guard.value`, validates and mutates the candidate, serializes old/candidate values, then calls:

```rust
let previous = guard
    .current_is_valid
    .then_some(previous_bytes.as_slice());
commit_with_backup(&self.paths, previous, &candidate_bytes)?;
*guard = SettingsState {
    value: candidate,
    current_is_valid: true,
};
```

`update_user_settings` copies only editable fields, merges aliases by the approved current-cache rule and preserves every `use_count`. `increment_use_count` first requires `cache.contains(app_id)`, then uses `checked_add`. `decorate_applications` only copies aliases/counts for matching IDs in the supplied clone. No method stores or returns a trusted path.

- [ ] **Step 5: Run settings tests and confirm GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml settings
cargo test --manifest-path src-tauri/Cargo.toml atomic_file
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all focused tests pass and Clippy exits 0.

- [ ] **Step 6: Commit Task 2**

```powershell
git add src-tauri/src/settings.rs src-tauri/src/lib.rs
git commit -m "feat: persist launcher settings"
```

---

### Task 3: Load and manage the unique settings store

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `scripts/test-security-probe.ps1`
- Test: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `SettingsStore::load` and Tauri `Manager::path`/`manage`.
- Produces: one process-owned managed `SettingsStore` for Task 5/6.

- [ ] **Step 1: Write a failing shared setup-helper test**

Write a unit test that calls the following planned private helper, but do not define the helper yet:

```rust
let store = load_settings_store(test_dir.path()).unwrap();
assert_eq!(store.snapshot(), Settings::default());
```

Test that the helper loads defaults into a unique temp directory and that a later reload reads the committed current file. Do not instantiate Tauri in the unit test; the production `manage` assertion is the single-owner gate.

- [ ] **Step 2: Run the setup test and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml load_settings_store
```

Expected: compile failure because the helper is absent.

- [ ] **Step 3: Wire setup without commands**

Add the production helper through the same path tested in RED:

```rust
#[cfg(any(test, not(feature = "test-instrumentation")))]
fn load_settings_store(
    app_data_dir: &std::path::Path,
) -> Result<settings::SettingsStore, settings::SettingsError> {
    settings::SettingsStore::load(app_data_dir)
}
```

Guard the `tauri::Manager` import with `#[cfg(not(feature = "test-instrumentation"))]`; use fully qualified paths in the helper as above so all-features non-test builds leave no helper-only imports. Query app-data, load and manage only inside the same production branch:

```rust
#[cfg(not(feature = "test-instrumentation"))]
{
    let app_data_dir = _app.path().app_data_dir()?;
    let settings = load_settings_store(&app_data_dir)?;
    assert!(_app.manage(settings), "settings store already managed");
}
```

Keep the existing single `Arc<AppCache>` and initial refresh unchanged. Do not add `generate_handler`, a command function, `src/protocol.ts` edits or plugin calls. Keep only scoped `#[cfg_attr(not(test), allow(dead_code))]` annotations required until Task 5 consumes the interfaces; Task 5 must remove them.

Extend the existing security-probe smoke rather than adding a second script. Add an optional app-data parameter whose default exactly follows the production identifier:

```powershell
[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string] $Executable,

  [string] $AppDataDir = (Join-Path `
    [Environment]::GetFolderPath('ApplicationData') `
    'com.uipilot.launcher')
)
```

Before launching the supplied executable and after it exits 73, snapshot every file in that directory whose name matches `settings.json*`, `validation-data.json*` or `open-session.json*`. Each sorted entry contains only relative name, length, SHA-256 and `LastWriteTimeUtc.Ticks`; serialize the array with `ConvertTo-Json -Compress`. If before/after strings differ, throw `security probe modified protected app data`. This must catch new/deleted current, backup, temp and invalid siblings. Existing executable validation, timeout and exact exit-73 assertions remain unchanged.

Add this exact helper and call `$before = Get-ProtectedAppDataSnapshot $AppDataDir` immediately before `Start-Process`:

```powershell
function Get-ProtectedAppDataSnapshot([string] $Root) {
  $patterns = @(
    'settings.json*',
    'validation-data.json*',
    'open-session.json*'
  )
  $entries = @()
  if (Test-Path -LiteralPath $Root) {
    $entries = @(
      Get-ChildItem -LiteralPath $Root -File | Where-Object {
        $name = $_.Name
        @($patterns | Where-Object { $name -like $_ }).Count -ne 0
      } | Sort-Object Name | ForEach-Object {
        [pscustomobject]@{
          Name = $_.Name
          Length = $_.Length
          Sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
          LastWriteTicks = $_.LastWriteTimeUtc.Ticks
        }
      }
    )
  }
  ConvertTo-Json -Compress -Depth 3 -InputObject @($entries)
}

```

Insert `$before = Get-ProtectedAppDataSnapshot $AppDataDir` immediately before the existing `Start-Process`. At the end of its existing `finally`, after the conditional `Stop-Process`, add the comparison so timeout, wrong exit and success paths all verify app-data immutability:

```powershell
$after = Get-ProtectedAppDataSnapshot $AppDataDir
if ($before -cne $after) {
  throw 'security probe modified protected app data'
}
```

- [ ] **Step 4: Run the Task 4A completion gate**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo test --manifest-path src-tauri/Cargo.toml atomic_file
cargo test --manifest-path src-tauri/Cargo.toml settings
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
npm run build
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
$probeExe = & .\scripts\build-security-probe.ps1 | Select-Object -Last 1
if ($LASTEXITCODE -ne 0 -or -not $probeExe) { throw "security probe build failed" }
& .\scripts\test-security-probe.ps1 -Executable $probeExe
if ($LASTEXITCODE -ne 0) { throw "security probe smoke failed" }

$baseline = git rev-parse --verify refs/tags/foundation-task-4-approved-plan
if ($LASTEXITCODE -ne 0) { throw "approved plan baseline tag missing" }
git merge-base --is-ancestor $baseline HEAD
if ($LASTEXITCODE -ne 0) { throw "approved plan baseline is not an ancestor" }
$allowed = @(
  'scripts/test-security-probe.ps1',
  'src-tauri/src/atomic_file.rs',
  'src-tauri/src/lib.rs',
  'src-tauri/src/settings.rs'
)
$changed = @(
  git diff --name-only "$baseline..HEAD"
  git diff --name-only
  git diff --cached --name-only
  git ls-files --others --exclude-standard
) | Where-Object { $_ } | Sort-Object -Unique
$unexpected = @($changed | Where-Object { $_ -notin $allowed })
if ($unexpected.Count -ne 0) {
  throw "Task 4A scope violation: $($unexpected -join ', ')"
}
git diff --check
```

Expected: all default/all-features Rust gates, frontend production build, security allowlist/negative fixtures, the exact probe artifact smoke, executable baseline/worktree scope assertion and diff check exit 0. Probe smoke proves settings, validation-data and marker families are unchanged.

- [ ] **Step 5: Commit Task 3**

```powershell
git add scripts/test-security-probe.ps1 src-tauri/src/lib.rs
git commit -m "feat: manage the settings store"
```

## Completion Gate

Task 4A is complete only after all three task commits and the full gate pass on Windows 11 x64. Completion means atomic settings persistence exists; it does not authorize Task 4B, Task 4C, Task 5 wiring or TDD for any unapproved plan.
