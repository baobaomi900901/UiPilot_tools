# Foundation Task 4C Validation Export Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 crate-private 的 Windows 原生保存目标选择和验证数据原子导出服务，不注册 Tauri command 或前端类型。

**Architecture:** `validation_export.rs` 用字段私有的 `ExportDestination` 把原生对话框选择转换为不可由其他模块构造的能力值。UI 线程函数只显示 `IFileSaveDialog`；独立 writer 消费该值并在未来 Task 5 的 blocking worker 中取得 snapshot、序列化和落盘。

**Tech Stack:** Rust 1.77.2、Tauri 2.11.3、Serde/serde_json、现有 `windows 0.61.3` 的 COM/Shell API、Task 4A 原子文件 helper、Task 4B stores。

**Status:** No-Go for TDD until this implementation plan is approved and Tasks 4A/4B are complete.

**Source Design:** `docs/superpowers/specs/2026-07-18-task-4c-validation-export-service-design.md`

## Global Constraints

- 前置产物是已批准并完成的 Task 4A 与 Task 4B；不新增 dependency、runtime 或通用文件 abstraction。
- 不创建 `#[tauri::command]`、`commands.rs`、`generate_handler` 变更、capability 或 `src/protocol.ts` 类型。
- TypeScript 不提供 path、filename、payload 或 JSON；只有 native dialog adapter 能构造 `ExportDestination`。
- UI 线程只执行 native modal dialog 和路径提取；snapshot、序列化、`sync_all`、`MoveFileExW` 属于 Task 5 的 `spawn_blocking` 调度边界。
- 导出必须包含已验证 research ID；缺少 ID 返回 `MissingResearchId`，不取得 validation snapshot、不创建文件。
- 错误和日志不包含目标路径、文件名、Shell display name、research ID、session ID 或临时文件名。
- 每个非平凡分支先写失败测试，再写最小实现；每个任务单独提交。
- 执行前必须验证 lightweight tag `foundation-task-4-approved-plan` 仍指向获批计划基线；不得移动或重建该 tag。

## Interfaces

Task 4C 消费：

```rust
impl SettingsStore {
    pub(crate) fn research_id(&self) -> Option<String>;
}

impl ValidationStore {
    pub(crate) fn export_snapshot(&self) -> ValidationCountsSnapshot;
}

pub(crate) fn replace_without_backup(
    destination: &Path,
    candidate: &[u8],
) -> Result<(), AtomicFileError>;
```

Task 4C 产出：

```rust
pub(crate) struct ExportDestination(PathBuf);

pub(crate) fn choose_export_destination(
    owner: HWND,
) -> Result<Option<ExportDestination>, ExportError>;

pub(crate) fn write_validation_export(
    destination: ExportDestination,
    settings: &SettingsStore,
    validation: &ValidationStore,
) -> Result<(), ExportError>;
```

The tuple field remains module-private. Task 5 can move an `ExportDestination` from chooser to writer but cannot construct one.

## Execution Baseline

Before the first RED command, verify Tasks 4A/4B left the reviewed baseline tag intact:

```powershell
$baseline = git rev-parse --verify refs/tags/foundation-task-4-approved-plan
if ($LASTEXITCODE -ne 0 -or -not $baseline) {
  throw "approved plan baseline tag missing"
}
git merge-base --is-ancestor $baseline HEAD
if ($LASTEXITCODE -ne 0) { throw "approved plan baseline is not an ancestor" }
```

Do not move or recreate the tag.

---

### Task 1: Implement the native destination capability

**Files:**
- Create: `src-tauri/src/validation_export.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/validation_export.rs`

**Interfaces:**
- Consumes: main-window `HWND` supplied by a future Rust wrapper.
- Produces: `Result<Option<ExportDestination>, ExportError>`; `None` means user cancellation only.

- [ ] **Step 1: Write failing Shell path and ownership tests**

Add `#[cfg(test)] mod validation_export;` to `lib.rs` so the RED run includes the new file. Use private closure seams for dialog HRESULT, returned `PWSTR` and deallocator. Start with exact ownership:

```rust
#[test]
fn valid_filesystem_path_is_freed_once_and_wrapped() {
    let mut wide: Vec<u16> = r"C:\Users\Test\validation.json"
        .encode_utf16()
        .chain([0])
        .collect();
    let frees = Cell::new(0);

    let destination = shell_path_with(
        |name| {
            assert_eq!(name, SIGDN_FILESYSPATH);
            Ok(PWSTR(wide.as_mut_ptr()))
        },
        |_| frees.set(frees.get() + 1),
    )
    .unwrap();

    assert_eq!(frees.get(), 1);
    assert_eq!(destination.test_path(), Path::new(r"C:\Users\Test\validation.json"));
}
```

Add tests for cancel -> `None`, non-cancel dialog error, COM `S_OK`/`S_FALSE` paired uninitialize, `RPC_E_CHANGED_MODE`, null pointer, empty string and unpaired-surrogate UTF-16. In success and every pointer-validation failure, assert the injected deallocator runs exactly once. Assert only `SIGDN_FILESYSPATH` is passed to the display-name seam and the main `HWND` is passed to `Show`.

- [ ] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
```

Expected: compile failure inside the included export module because its interfaces do not exist.

- [ ] **Step 3: Implement COM lifetime and strict path conversion**

Replace the test-only declaration with the scoped production declaration:

```rust
#[cfg_attr(not(test), allow(dead_code))]
mod validation_export;
```

Add the exact generated-API feature required by `IFileDialog::SetFileTypes` to the existing `windows` dependency; do not add another crate or unrelated Windows feature:

```toml
"Win32_UI_Shell_Common",
```

Define fixed errors and the unforgeable value:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportError {
    ComUnavailable,
    DialogFailed,
    InvalidDestination,
    MissingResearchId,
    Serialize,
    Write,
}

pub(crate) struct ExportDestination(PathBuf);

#[cfg(test)]
impl ExportDestination {
    fn test_path(&self) -> &Path {
        &self.0
    }
}
```

Give every `ExportError` variant one fixed path-free `Display` string and implement `std::error::Error`.

`CoInitializeEx(NULL, COINIT_APARTMENTTHREADED)` accepts `S_OK` and `S_FALSE`; a same-thread guard calls `CoUninitialize` once. `RPC_E_CHANGED_MODE` and every other HRESULT map to `ComUnavailable` without changing apartment.

After successful `Show(owner)`, call `GetResult`, then only `GetDisplayName(SIGDN_FILESYSPATH)`. Immediately place the returned `PWSTR` into this module-local ownership pattern before inspecting it:

```rust
struct ShellPath<F: FnOnce(*mut u16)> {
    pointer: *mut u16,
    free: Option<F>,
}

impl<F: FnOnce(*mut u16)> Drop for ShellPath<F> {
    fn drop(&mut self) {
        self.free.take().expect("Shell path deallocator missing")(self.pointer);
    }
}
```

Production supplies `|pointer| unsafe { CoTaskMemFree(Some(pointer.cast())) }`. Reject null, empty and `String::from_utf16` errors as `InvalidDestination`; do not call `from_utf16_lossy`, use display names or URLs. Only the validated `PathBuf` is moved into `ExportDestination`.

The private conversion seam has one exact signature, so tests and production use the same ownership path:

```rust
fn shell_path_with<C, F>(call: C, free: F) -> Result<ExportDestination, ExportError>
where
    C: FnOnce(SIGDN) -> Result<PWSTR, ExportError>,
    F: FnOnce(*mut u16),
{
    let raw = call(SIGDN_FILESYSPATH)?;
    let owned = ShellPath {
        pointer: raw.0,
        free: Some(free),
    };
    strict_path_from_utf16(owned.pointer).map(ExportDestination)
}
```

- [ ] **Step 4: Configure the dialog narrowly**

Create `IFileSaveDialog` with existing COM/Shell features. Set default extension `json`, one `JSON (*.json)` filter and:

```rust
FOS_FORCEFILESYSTEM
    | FOS_PATHMUSTEXIST
    | FOS_OVERWRITEPROMPT
    | FOS_NOCHANGEDIR
```

Map only `HRESULT_FROM_WIN32(ERROR_CANCELLED)` to `Ok(None)`. All other dialog/GetResult/GetDisplayName failures return fixed categories. Do not open Explorer or return the path anywhere else.

- [ ] **Step 5: Run destination tests and confirm GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Expected: dialog/path tests pass and Clippy exits 0.

- [ ] **Step 6: Commit Task 1**

```powershell
git add src-tauri/Cargo.toml src-tauri/src/validation_export.rs src-tauri/src/lib.rs
git commit -m "feat: select a native validation export target"
```

---

### Task 2: Serialize and atomically write the approved export

**Files:**
- Modify: `src-tauri/src/validation_export.rs`
- Test: `src-tauri/src/validation_export.rs`

**Interfaces:**
- Consumes: `ExportDestination`, `SettingsStore::research_id`, `ValidationStore::export_snapshot` and Task 4A no-backup writer.
- Produces: `write_validation_export`; no command/status DTO.

- [ ] **Step 1: Write failing export boundary tests**

Start with the missing-ID ordering regression. The private builder accepts a snapshot closure so the test proves it is never called:

```rust
#[test]
fn missing_research_id_stops_before_snapshot_and_write() {
    let snapshot_called = Cell::new(false);
    let result = build_export_with(None, || {
        snapshot_called.set(true);
        validation_snapshot()
    });

    assert_eq!(result, Err(ExportError::MissingResearchId));
    assert!(!snapshot_called.get());
}
```

Add a success test that serializes exactly `schemaVersion`, required `researchId` and `dailyCounts` fields. Parse with `serde_json::Value` and assert absence of `hostCrashes`, session IDs, marker, hotkey, autostart, aliases, useCounts and paths. Add writer failure tests proving a pre-existing destination remains unchanged and the returned error contains no path.

Use a private `write_validation_export_with(destination, settings, validation, replace)` closure seam. Assert success invokes `replace` exactly once with the consumed destination and serialized bytes; missing ID never invokes it. Production passes Task 4A `replace_without_backup`. Do not add a writer trait.

```rust
fn write_validation_export_with<W>(
    destination: ExportDestination,
    settings: &SettingsStore,
    validation: &ValidationStore,
    write: W,
) -> Result<(), ExportError>
where
    W: FnOnce(&Path, &[u8]) -> Result<(), AtomicFileError>;
```

- [ ] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
```

Expected: compile failure because export builder/writer are absent.

- [ ] **Step 3: Implement the exact export DTO and lock order**

Use a separate DTO; never serialize `ValidationState`:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExport {
    schema_version: u32,
    research_id: String,
    daily_counts: BTreeMap<String, DailyCounts>,
}
```

`write_validation_export` performs in this order:

```rust
let research_id = settings
    .research_id()
    .ok_or(ExportError::MissingResearchId)?;
let snapshot = validation.export_snapshot();
let bytes = serde_json::to_vec_pretty(&ValidationExport {
    schema_version: snapshot.schema_version,
    research_id,
    daily_counts: snapshot.daily_counts,
})
.map_err(|_| ExportError::Serialize)?;
replace_without_backup(&destination.0, &bytes).map_err(|_| ExportError::Write)
```

No two store locks overlap: each store method returns an owned clone before the next is called. Do not add backup, network upload, Explorer launch or path response.

- [ ] **Step 4: Run the Task 4C completion gate**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo test --manifest-path src-tauri/Cargo.toml validation_export
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
  'src-tauri/Cargo.toml',
  'src-tauri/src/atomic_file.rs',
  'src-tauri/src/lib.rs',
  'src-tauri/src/session_marker.rs',
  'src-tauri/src/settings.rs',
  'src-tauri/src/validation_data.rs',
  'src-tauri/src/validation_export.rs'
)
$changed = @(
  git diff --name-only "$baseline..HEAD"
  git diff --name-only
  git diff --cached --name-only
  git ls-files --others --exclude-standard
) | Where-Object { $_ } | Sort-Object -Unique
$unexpected = @($changed | Where-Object { $_ -notin $allowed })
if ($unexpected.Count -ne 0) {
  throw "Task 4C scope violation: $($unexpected -join ', ')"
}
git diff --check
```

Expected: default/all-features Rust gates, frontend build, security allowlist/negative fixtures, same-artifact probe smoke, executable baseline/worktree scope assertion and diff check exit 0. The allowed set includes only cumulative Task 4A/4B files plus `validation_export.rs`; no `src/protocol.ts`, command, capability, runtime dependency or Task 5 action file is allowed.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/validation_export.rs
git commit -m "feat: write validation exports atomically"
```

## Completion Gate

Task 4C is complete only after Tasks 4A/4B are complete, both Task 4C commits exist and the full gate passes on Windows 11 x64. Real dialog smoke, async command orchestration, `Cancelled`/`Exported` command results and TypeScript consumption remain Task 5/7 work.
