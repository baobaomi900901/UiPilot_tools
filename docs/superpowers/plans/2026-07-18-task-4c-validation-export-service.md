# Foundation Task 4C Validation Export Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 crate-private 的 Windows 原生保存目标选择和验证数据原子导出服务，不注册 Tauri command 或前端类型。

**Architecture:** `validation_export.rs` 用字段私有的 `ExportDestination` 把原生对话框选择转换为不可由其他模块构造的能力值。UI 线程函数只显示 `IFileSaveDialog`；独立 writer 消费该值并在未来 Task 5 的 blocking worker 中取得 snapshot、序列化和落盘。

**Tech Stack:** Rust 1.77.2、Tauri 2.11.3、Serde/serde_json、现有 `windows 0.61.3` 的 COM/Shell API、Task 4A 原子文件 helper、Task 4B stores。

**Status:** No-Go for resumed Task 4C execution until this gate-decoupled plan revision is approved.
Tasks 4A/4B and Task 4C Task 1 are complete; Task 2 remains an uncommitted frozen diff.

**Source Design:** `docs/superpowers/specs/2026-07-18-task-4c-validation-export-service-design.md`

**Security Gate Design:**
`docs/superpowers/specs/2026-07-18-security-probe-gate-decoupling-design.md`

## Global Constraints

- 前置产物是已批准并完成的 Task 4A 与 Task 4B；不新增 dependency、runtime 或通用文件 abstraction。
- 不创建 `#[tauri::command]`、`commands.rs`、`generate_handler` 变更、capability 或 `src/protocol.ts` 类型。
- TypeScript 不提供 path、filename、payload 或 JSON；只有 native dialog adapter 能构造 `ExportDestination`。
- UI 线程只执行 native modal dialog 和路径提取；snapshot、序列化、`sync_all`、`MoveFileExW` 属于 Task 5 的 `spawn_blocking` 调度边界。
- 导出必须包含已验证 research ID；缺少 ID 返回 `MissingResearchId`，不取得 validation snapshot、不创建文件。
- 错误和日志不包含目标路径、文件名、Shell display name、research ID、session ID 或临时文件名。
- 每个非平凡分支先写失败测试，再写最小实现；每个任务单独提交。
- 执行前必须验证 lightweight tag `foundation-task-4-approved-plan` 仍指向获批计划基线；不得移动或重建该 tag。
- runtime positive probe 不再属于 Task 4C completion gate。Task 4C 可以得到 `TaskCodeGo`，
  但必须同时报告 `ReleaseSecurityBlocked` 和 `SEC-RUNTIME-PROBE-001`；不得声称 runtime ACL、
  Foundation 或 MVP-A release ready。
- 不新增通用 trust checker/fixture、PowerShell 7 dependency 或 security waiver 参数。

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

This plan is reviewed in an independent docs branch. After plan Go and before resuming Task 4C, the
coordinator performs exactly these non-destructive preparation steps:

1. Create immutable lightweight tag `foundation-task-4c-trust-baseline` at
   `6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`. If the tag already exists, it must resolve to that
   SHA; never move or recreate it.
2. Create immutable lightweight evidence tag `foundation-task-4c-gate-plan` at the final docs commit
   named by the written plan-Go message. Record that exact commit SHA; never move or recreate the tag.
3. In `codex/foundation-task-4`, first verify `HEAD` is exactly `6b2348e...`, the index is empty,
   `PlanPath` has no staged or worktree modification, and `src-tauri/src/validation_export.rs` is the
   only unstaged path. Only after those checks, restore the final approved plan blob into the index and
   worktree with this exact command; do not cherry-pick the docs commit:

   ```powershell
   git restore --source refs/tags/foundation-task-4c-gate-plan --staged --worktree -- docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md
   if ($LASTEXITCODE -ne 0) { throw 'approved plan blob restore failed' }
   ```

4. After restore, verify the staged set is exactly `PlanPath`, the unstaged set is exactly
   `src-tauri/src/validation_export.rs`, and the staged PlanPath blob equals the PlanPath blob at
   `foundation-task-4c-gate-plan`. Create one new plan-only commit on the Task 4 branch; do not claim
   it has the same commit identity as the docs evidence commit.
5. The resulting Task 4C `HEAD` must have baseline SHA `6b2348e...` as its direct parent, and its
   commit diff must contain only
   `docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md`. After commit, the index
   is empty and `src-tauri/src/validation_export.rs` remains the only unstaged path.

No tag creation or plan integration occurs before this plan receives written Go. The completion gate
below authenticates the prepared state again rather than trusting the preparation report.

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

- [x] **Step 1: Write failing Shell path and ownership tests**

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

- [x] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
```

Expected: compile failure inside the included export module because its interfaces do not exist.

- [x] **Step 3: Implement COM lifetime and strict path conversion**

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

- [x] **Step 4: Configure the dialog narrowly**

Create `IFileSaveDialog` with existing COM/Shell features. Set default extension `json`, one `JSON (*.json)` filter and:

```rust
FOS_FORCEFILESYSTEM
    | FOS_PATHMUSTEXIST
    | FOS_OVERWRITEPROMPT
    | FOS_NOCHANGEDIR
```

Map only `HRESULT_FROM_WIN32(ERROR_CANCELLED)` to `Ok(None)`. All other dialog/GetResult/GetDisplayName failures return fixed categories. Do not open Explorer or return the path anywhere else.

- [x] **Step 5: Run destination tests and confirm GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Expected: dialog/path tests pass and Clippy exits 0.

- [x] **Step 6: Commit Task 1**

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

- [x] **Step 1: Write failing export boundary tests**

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

- [x] **Step 2: Run focused tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml validation_export
```

Expected: compile failure because export builder/writer are absent.

- [x] **Step 3: Implement the exact export DTO and lock order**

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
$ErrorActionPreference = 'Stop'
$GateWorkingDirectory = [IO.Path]::GetFullPath(
  'D:\code\UiPilot_tools\.worktrees\foundation-task-4'
)
$BaselineSha = '6b2348e3694f4a8f45df97d84ccf6f1de3c6d516'
$ApprovedPlanSha = 'c1e4d9c649d4710113206f630fc9d594ae8d2b7e'
$GatePlanTag = 'foundation-task-4c-gate-plan'
$PlanPath = 'docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md'
$ExportPath = 'src-tauri/src/validation_export.rs'
$GatePlanEvidenceSha = $null
$TrustPathspecArguments = @(
  'src-tauri/capabilities/'
  'src-tauri/permissions/'
  'src-tauri/tauri*.conf.json'
  'src-tauri/tauri*.conf.json5'
  'src-tauri/tauri*.conf.toml'
  'scripts/check-security-config.ps1'
  'scripts/test-security-config.ps1'
  'scripts/build-security-probe.ps1'
  'scripts/test-security-probe.ps1'
  'security-probe.html'
  'src/security-probe.ts'
  'src-tauri/src/security_probe.rs'
  'src-tauri/src/lib.rs'
  'src-tauri/src/main.rs'
  'src-tauri/Cargo.toml'
  'src-tauri/Cargo.lock'
  'src-tauri/build.rs'
  'vite.config.*'
  'package.json'
  'package-lock.json'
  'npm-shrinkwrap.json'
  'pnpm-lock.yaml'
  'yarn.lock'
  'bun.lock'
  'bun.lockb'
  'tsconfig*.json'
  'index.html'
  '.gitignore'
  '.gitattributes'
  '.gitmodules'
) -join ' '

if (-not [IO.Directory]::Exists($GateWorkingDirectory)) {
  throw 'Task 4C worktree is missing'
}
$GateRootAttributes = [IO.File]::GetAttributes($GateWorkingDirectory)
if (
  ($GateRootAttributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
  ($GateRootAttributes -band [IO.FileAttributes]::Directory) -eq 0
) {
  throw 'Task 4C worktree root is not a plain directory'
}
$CurrentWorkingDirectory = [IO.Path]::GetFullPath((Get-Location).ProviderPath)
if (-not [StringComparer]::OrdinalIgnoreCase.Equals(
  $CurrentWorkingDirectory,
  $GateWorkingDirectory
)) {
  throw 'Run the Task 4C gate from the fixed Task 4 worktree root'
}
$gitCommands = @(Get-Command git.exe -CommandType Application -ErrorAction Stop)
if ($gitCommands.Count -ne 1) {
  throw 'Expected exactly one resolved git.exe application'
}
$GitExe = [IO.Path]::GetFullPath($gitCommands[0].Source)
$ExpectedGitExe = [IO.Path]::GetFullPath('C:\Program Files\Git\cmd\git.exe')
if (
  -not [IO.Path]::IsPathRooted($GitExe) -or
  -not [IO.File]::Exists($GitExe) -or
  -not [StringComparer]::OrdinalIgnoreCase.Equals($GitExe, $ExpectedGitExe) -or
  -not [StringComparer]::OrdinalIgnoreCase.Equals(
    [IO.Path]::GetFileName($GitExe),
    'git.exe'
  )
) {
  throw 'Resolved Git application is invalid'
}
$StrictUtf8 = New-Object Text.UTF8Encoding($false, $true)

function Invoke-GitRaw {
  param(
    [Parameter(Mandatory = $true)]
    [string] $Arguments
  )

  $startInfo = New-Object Diagnostics.ProcessStartInfo
  $startInfo.FileName = $GitExe
  $startInfo.Arguments = $Arguments
  $startInfo.WorkingDirectory = $GateWorkingDirectory
  $startInfo.UseShellExecute = $false
  $startInfo.RedirectStandardOutput = $true
  $startInfo.CreateNoWindow = $true

  $process = New-Object Diagnostics.Process
  $process.StartInfo = $startInfo
  $buffer = New-Object IO.MemoryStream
  try {
    if (-not $process.Start()) {
      throw 'Git process did not start'
    }
    # Do not access StandardOutput through StreamReader anywhere in this gate.
    $process.StandardOutput.BaseStream.CopyTo($buffer)
    $process.WaitForExit()
    [pscustomobject]@{
      ExitCode = $process.ExitCode
      Bytes = $buffer.ToArray()
    }
  }
  finally {
    $buffer.Dispose()
    $process.Dispose()
  }
}

function Require-GitSuccess {
  param(
    [Parameter(Mandatory = $true)]
    $Result,
    [Parameter(Mandatory = $true)]
    [string] $Purpose
  )

  if ($Result.ExitCode -ne 0) {
    throw "Git failed for $Purpose with exit code $($Result.ExitCode)"
  }
}

function Convert-StrictUtf8 {
  param([byte[]] $Bytes)

  try {
    $StrictUtf8.GetString($Bytes)
  }
  catch {
    throw 'Git output is not valid UTF-8'
  }
}

function Convert-NulTokens {
  param([byte[]] $Bytes)

  if ($Bytes.Count -eq 0) {
    return
  }
  if ($Bytes[$Bytes.Count - 1] -ne 0) {
    throw 'Git NUL output is not terminated'
  }

  $start = 0
  for ($index = 0; $index -lt $Bytes.Count; $index++) {
    if ($Bytes[$index] -ne 0) {
      continue
    }
    if ($index -eq $start) {
      throw 'Git NUL output contains an empty token'
    }
    $length = $index - $start
    $tokenBytes = New-Object byte[] $length
    [Array]::Copy($Bytes, $start, $tokenBytes, 0, $length)
    Convert-StrictUtf8 $tokenBytes
    $start = $index + 1
  }
}

function Assert-GitPath {
  param([string] $Path)

  if (
    [string]::IsNullOrEmpty($Path) -or
    [IO.Path]::IsPathRooted($Path) -or
    $Path.StartsWith('/') -or
    $Path.Contains('\')
  ) {
    throw 'Git returned an unsafe path'
  }
  foreach ($character in $Path.ToCharArray()) {
    if ([char]::IsControl($character)) {
      throw 'Git path contains a control character'
    }
  }
  $segments = @($Path.Split('/'))
  if (
    $segments.Count -eq 0 -or
    @($segments | Where-Object { $_ -eq '' -or $_ -eq '.' -or $_ -eq '..' }).Count -ne 0
  ) {
    throw 'Git path contains an unsafe segment'
  }
  $Path
}

function Get-GitText {
  param(
    [string] $Arguments,
    [string] $Purpose
  )

  $result = Invoke-GitRaw $Arguments
  Require-GitSuccess $result $Purpose
  $text = Convert-StrictUtf8 $result.Bytes
  if ($text.Contains([char]0)) {
    throw "Unexpected NUL in $Purpose"
  }
  $trimmed = $text.TrimEnd([char[]]@(13, 10))
  if ($trimmed.Contains([char]13) -or $trimmed.Contains([char]10)) {
    throw "Multiple lines returned for $Purpose"
  }
  $trimmed
}

function Get-NameStatusEntries {
  param(
    [string] $Arguments,
    [string] $Purpose
  )

  $result = Invoke-GitRaw $Arguments
  Require-GitSuccess $result $Purpose
  $tokens = @(Convert-NulTokens $result.Bytes)
  if (($tokens.Count % 2) -ne 0) {
    throw "Malformed name-status output for $Purpose"
  }
  for ($index = 0; $index -lt $tokens.Count; $index += 2) {
    if ($tokens[$index] -cnotmatch '^[AMDTUXB]$') {
      throw "Unexpected diff status for $Purpose"
    }
    $path = Assert-GitPath $tokens[$index + 1]
    [pscustomobject]@{
      Status = $tokens[$index]
      Path = $path
    }
  }
}

function Assert-SingleModifiedEntry {
  param(
    [object[]] $Entries,
    [string] $ExpectedPath,
    [string] $Purpose
  )

  if (
    $Entries.Count -ne 1 -or
    $Entries[0].Status -cne 'M' -or
    -not [StringComparer]::Ordinal.Equals($Entries[0].Path, $ExpectedPath)
  ) {
    throw "$Purpose must contain exactly one M entry for $ExpectedPath"
  }
}

function Get-LsFilesPaths {
  param(
    [string] $Arguments,
    [string] $Purpose
  )

  $result = Invoke-GitRaw $Arguments
  Require-GitSuccess $result $Purpose
  foreach ($token in @(Convert-NulTokens $result.Bytes)) {
    Assert-GitPath $token
  }
}

function Assert-NoTrustIndexFlags {
  param([string] $Arguments)

  $result = Invoke-GitRaw $Arguments
  Require-GitSuccess $result 'trust index flags'
  foreach ($record in @(Convert-NulTokens $result.Bytes)) {
    if ($record.Length -lt 3 -or $record[1] -ne ' ') {
      throw 'Malformed ls-files -v record'
    }
    $tag = $record[0]
    $upperTag = [char]::ToUpperInvariant($tag)
    if ('HSMRCK?'.IndexOf([string]$upperTag, [StringComparison]::Ordinal) -lt 0) {
      throw 'Unknown ls-files -v status'
    }
    $path = Assert-GitPath $record.Substring(2)
    if ([char]::IsLower($tag) -or $tag -ceq 'S') {
      throw "Trust path has an index bypass flag: $path"
    }
  }
}

function Assert-ExactPaths {
  param(
    [string[]] $Actual,
    [string[]] $Expected,
    [string] $Purpose
  )

  $actualSorted = @($Actual | Sort-Object -CaseSensitive -Unique)
  $expectedSorted = @($Expected | Sort-Object -CaseSensitive -Unique)
  if ($actualSorted.Count -ne $expectedSorted.Count) {
    throw "$Purpose path count mismatch"
  }
  for ($index = 0; $index -lt $expectedSorted.Count; $index++) {
    if (-not [StringComparer]::Ordinal.Equals(
      $actualSorted[$index],
      $expectedSorted[$index]
    )) {
      throw "$Purpose path mismatch"
    }
  }
}

function Assert-NoReparseComponents {
  param(
    [string] $RelativePath,
    [bool] $MustExist,
    [bool] $FinalMustBeDirectory
  )

  $normalized = Assert-GitPath $RelativePath
  $segments = @($normalized.Split('/'))
  $current = $GateWorkingDirectory
  for ($index = -1; $index -lt $segments.Count; $index++) {
    if ($index -ge 0) {
      $current = [IO.Path]::Combine($current, $segments[$index])
    }
    try {
      $attributes = [IO.File]::GetAttributes($current)
    }
    catch [IO.FileNotFoundException] {
      if (-not $MustExist -and $index -eq ($segments.Count - 1)) {
        return
      }
      throw 'Expected path component is missing'
    }
    catch [IO.DirectoryNotFoundException] {
      if (-not $MustExist -and $index -eq ($segments.Count - 1)) {
        return
      }
      throw 'Expected path component is missing'
    }
    if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw 'A checked path component is a reparse point'
    }
    $isDirectory = ($attributes -band [IO.FileAttributes]::Directory) -ne 0
    if ($index -lt ($segments.Count - 1) -and -not $isDirectory) {
      throw 'A checked parent component is not a directory'
    }
    if ($index -eq ($segments.Count - 1) -and $FinalMustBeDirectory -and -not $isDirectory) {
      throw 'A trust root is not a directory'
    }
    if ($index -eq ($segments.Count - 1) -and $MustExist -and -not $FinalMustBeDirectory -and $isDirectory) {
      throw 'A checked file candidate is a directory'
    }
  }
}

function Assert-Task4CScope {
  $topLevel = Get-GitText 'rev-parse --show-toplevel' 'worktree root'
  $topLevelFull = [IO.Path]::GetFullPath($topLevel)
  if (-not [StringComparer]::OrdinalIgnoreCase.Equals(
    $topLevelFull,
    $GateWorkingDirectory
  )) {
    throw 'Git worktree root mismatch'
  }

  $tagSha = Get-GitText (
    'rev-parse --verify refs/tags/foundation-task-4c-trust-baseline^{}'
  ) 'Task 4C trust baseline tag'
  if (-not [StringComparer]::Ordinal.Equals($tagSha, $BaselineSha)) {
    throw 'Task 4C trust baseline tag moved'
  }
  $ancestor = Invoke-GitRaw (
    'merge-base --is-ancestor 6b2348e3694f4a8f45df97d84ccf6f1de3c6d516 HEAD'
  )
  Require-GitSuccess $ancestor 'Task 4C baseline ancestry'

  $approvedPlanTag = Get-GitText (
    'rev-parse --verify refs/tags/foundation-task-4-approved-plan^{}'
  ) 'Foundation Task 4 approved plan tag'
  if (-not [StringComparer]::Ordinal.Equals($approvedPlanTag, $ApprovedPlanSha)) {
    throw 'Foundation Task 4 approved plan tag moved'
  }
  $approvedPlanAncestor = Invoke-GitRaw (
    'merge-base --is-ancestor c1e4d9c649d4710113206f630fc9d594ae8d2b7e HEAD'
  )
  Require-GitSuccess $approvedPlanAncestor 'Foundation Task 4 plan ancestry'

  $gatePlanSha = Get-GitText (
    'rev-parse --verify refs/tags/foundation-task-4c-gate-plan^{}'
  ) 'Task 4C gate-plan evidence tag'
  $gatePlanBlob = Get-GitText (
    'rev-parse refs/tags/foundation-task-4c-gate-plan:docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md'
  ) 'approved gate-plan blob'
  $headPlanBlob = Get-GitText (
    'rev-parse HEAD:docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md'
  ) 'integrated gate-plan blob'
  if (-not [StringComparer]::Ordinal.Equals($gatePlanBlob, $headPlanBlob)) {
    throw 'Integrated Task 4C plan content differs from the approved evidence tag'
  }
  $script:GatePlanEvidenceSha = $gatePlanSha

  $parentSha = Get-GitText 'rev-parse HEAD^' 'Task 4C plan parent'
  if (-not [StringComparer]::Ordinal.Equals($parentSha, $BaselineSha)) {
    throw 'Task 4C plan commit is not directly based on the approved baseline'
  }
  $planCommitPaths = @(Get-LsFilesPaths (
    'diff-tree --no-commit-id --name-only -r -z HEAD'
  ) 'integrated plan commit')
  Assert-ExactPaths $planCommitPaths @($PlanPath) 'integrated plan commit'

  $restrictedCommitted = @(Get-NameStatusEntries (
    'diff --no-renames --name-status -z 6b2348e3694f4a8f45df97d84ccf6f1de3c6d516 HEAD -- ' +
    $TrustPathspecArguments
  ) 'committed trust diff')
  $restrictedIndex = @(Get-NameStatusEntries (
    'diff --cached --no-renames --name-status -z HEAD -- ' +
    $TrustPathspecArguments
  ) 'index trust diff')
  $restrictedWorktree = @(Get-NameStatusEntries (
    'diff --no-renames --name-status -z -- ' +
    $TrustPathspecArguments
  ) 'worktree trust diff')
  if (@($restrictedCommitted + $restrictedIndex + $restrictedWorktree).Count -ne 0) {
    throw 'Task 4C changed a frozen trust input'
  }

  $fullCommitted = @(Get-NameStatusEntries (
    'diff --no-renames --name-status -z 6b2348e3694f4a8f45df97d84ccf6f1de3c6d516 HEAD --'
  ) 'committed task diff')
  $fullIndex = @(Get-NameStatusEntries (
    'diff --cached --no-renames --name-status -z HEAD --'
  ) 'index task diff')
  $fullWorktree = @(Get-NameStatusEntries (
    'diff --no-renames --name-status -z --'
  ) 'worktree task diff')
  $untracked = @(Get-LsFilesPaths (
    'ls-files --others --exclude-standard -z'
  ) 'untracked task paths')
  $untrackedTrust = @(Get-LsFilesPaths (
    'ls-files --others --exclude-standard -z -- ' + $TrustPathspecArguments
  ) 'untracked trust paths')
  $ignoredTrust = @(Get-LsFilesPaths (
    'ls-files --others --ignored --exclude-standard -z -- ' +
    $TrustPathspecArguments
  ) 'ignored trust paths')
  if ($untrackedTrust.Count -ne 0 -or $ignoredTrust.Count -ne 0) {
    throw 'Task 4C contains an untracked or ignored trust input'
  }
  Assert-SingleModifiedEntry $fullCommitted $PlanPath 'committed task layer'
  if ($fullIndex.Count -ne 0) {
    throw 'Task 4C index must be empty'
  }
  Assert-SingleModifiedEntry $fullWorktree $ExportPath 'worktree task layer'
  if ($untracked.Count -ne 0) {
    throw 'Task 4C must not contain untracked paths'
  }

  Assert-NoTrustIndexFlags (
    'ls-files -v -z -- ' + $TrustPathspecArguments
  )

  foreach ($trustRoot in @(
    'src-tauri'
    'src-tauri/capabilities'
    'src-tauri/permissions'
    'scripts'
    'src'
  )) {
    Assert-NoReparseComponents $trustRoot $false $true
  }
  $trackedTrust = @(Get-LsFilesPaths (
    'ls-files -z -- ' + $TrustPathspecArguments
  ) 'tracked trust paths')
  foreach ($candidate in @($trackedTrust + $untracked + $ignoredTrust)) {
    Assert-NoReparseComponents $candidate $true $false
  }
  Assert-NoReparseComponents $PlanPath $true $false
  Assert-NoReparseComponents $ExportPath $true $false
}

# Authenticate scope before executing repository-owned scripts or build wiring.
Assert-Task4CScope

cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
cargo check --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'default cargo check failed' }
cargo check --manifest-path src-tauri/Cargo.toml --all-features
if ($LASTEXITCODE -ne 0) { throw 'all-features cargo check failed' }
cargo test --manifest-path src-tauri/Cargo.toml validation_export
if ($LASTEXITCODE -ne 0) { throw 'focused validation export tests failed' }
cargo test --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'full Rust tests failed' }
cargo test --manifest-path src-tauri/Cargo.toml --all-features
if ($LASTEXITCODE -ne 0) { throw 'all-features Rust tests failed' }
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { throw 'default Clippy failed' }
$env:CARGO_INCREMENTAL = '0'
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { throw 'all-features Clippy failed' }

$securityConfigOutput = @(
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
)
$securityConfigExit = $LASTEXITCODE
$securityConfigOutput | ForEach-Object { Write-Output $_ }
if (
  $securityConfigExit -ne 0 -or
  $securityConfigOutput -notcontains 'security config ok'
) {
  throw 'static security config gate failed'
}
$securityFixtureOutput = @(
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
)
$securityFixtureExit = $LASTEXITCODE
$securityFixtureOutput | ForEach-Object { Write-Output $_ }
if (
  $securityFixtureExit -ne 0 -or
  $securityFixtureOutput -notcontains 'security config regression tests ok'
) {
  throw 'security config regression fixtures failed'
}

npm.cmd run build
if ($LASTEXITCODE -ne 0) { throw 'production frontend build failed' }
if (Test-Path -LiteralPath 'dist/security-probe.html') {
  throw 'production build contains the security probe page'
}

# Re-authenticate after every repository-owned script and build has run.
Assert-Task4CScope
$diffCheck = Invoke-GitRaw 'diff --check'
Require-GitSuccess $diffCheck 'final diff check'

Write-Output 'TaskCodeGo candidate: Task 4C gates passed'
Write-Output 'ReleaseSecurityBlocked: SEC-RUNTIME-PROBE-001'
Write-Output "GatePlanEvidence: $GatePlanTag=$GatePlanEvidenceSha"
```

Expected: every native command's raw exit code is checked; the immutable baseline, approved plan
evidence tag/blob, plan-only HEAD, restricted trust view, index flags, ignored candidates and every
reparse component pass before and after repository-owned commands. The committed layer is exactly one
M entry for the plan, the index is empty, the worktree layer is exactly one M entry for the export, and
nonignored untracked is empty. Rust, static security fixtures and the production build pass, production
contains no probe page, and the fixed final output reports TaskCodeGo candidate,
ReleaseSecurityBlocked: SEC-RUNTIME-PROBE-001 and the evidence-tag SHA. The runtime positive probe is
not run and is not claimed as passing. No src/protocol.ts, command, capability, dependency or Task 5
action file is allowed.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/validation_export.rs
git commit -m "feat: write validation exports atomically"
```

## Completion Gate

Task 4C is complete only after Tasks 4A/4B are complete, both Task 4C commits exist and the full gate passes on Windows 11 x64. Real dialog smoke, async command orchestration, `Cancelled`/`Exported` command results and TypeScript consumption remain Task 5/7 work.
