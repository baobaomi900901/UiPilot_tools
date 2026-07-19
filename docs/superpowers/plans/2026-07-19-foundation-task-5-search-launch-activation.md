# Foundation Task 5 Search, Launch, and Best-Effort Activation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** `f204c0c45050de979beb7311cf52a3e5c2c57ee8` received TaskCodeGo No-Go only for module-wide warning suppression. The settings DTO and shared-hide corrections are Go. Corrective lint/source-oracle TDD is authorized; merge, push, trial, signing, and release remain prohibited.

**Goal:** 在 Windows 11 x64 上复用现有 `ResultRegistry`、`AppCache`、`SettingsStore`、`ValidationStore` 和冻结 DTO，交付应用搜索、可信 `.lnk` 启动、唯一进程下的尽力窗口激活，以及八个窄 Tauri command wrapper。

**Architecture:** `commands.rs` 只做统一 caller guard、独立 settings read/write DTO 投影、唯一 `clear_and_hide`、状态编排和线程边界；`apps/action.rs` 只做动作策略；`apps/windows_backend.rs` 只做受信任 Rust action 到 Win32 API 的适配。前端执行动作只提交 `requestId/resultId`；`appId` 只允许出现在 settings view/update，绝不进入动作定位。无新 trait、crate、通用执行器或未来扩展点。

**Tech Stack:** Rust 1.77.2、Tauri 2.11.3、现有 `windows` 0.61.3、现有标准库同步原语与 Tauri async runtime。

**Source Design:** `docs/superpowers/specs/2026-07-19-foundation-task-5-search-launch-activation-design.md` in this same lint-boundary documentation revision.

**Implementation / Trust Baseline:** `2788e9a275e0406e70d7597a4a78da274d8c55c0`. This is the reviewed clean local `main` after approved Task 4C fast-forward integration. `origin/main` is intentionally behind and is not a substitute baseline.

**Reviewed Implementation Head:** `f204c0c45050de979beb7311cf52a3e5c2c57ee8` on `codex/foundation-task-5`. Corrective work resumes only from this clean head in the existing implementation worktree; it does not create a replacement baseline, tag, branch, or worktree.

**Release State:** `TaskSecurityReviewRequired` and `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001`. Task 5 does not inherit Task 4C's trust-zero-change exception. A later code review must explicitly decide whether the implementation may be reported as `TaskCodeGo + ReleaseSecurityBlocked`.

**Supersession:** After written Plan Go, this document replaces only the Task 5 implementation section in `docs/superpowers/plans/2026-07-17-windows-launcher-foundation.md`. It does not change Task 6, 7, or 8 ownership.

---

## Global Constraints

- The original implementation worktree prerequisite at `2788e9a275e0406e70d7597a4a78da274d8c55c0` is already satisfied. Before corrective RED, confirm the existing `codex/foundation-task-5` worktree is clean at `f204c0c45050de979beb7311cf52a3e5c2c57ee8`. Do not create a replacement worktree or implement in the current docs worktree.
- The implementation allowlist is exact:

```text
src-tauri/Cargo.toml
src-tauri/src/lib.rs
src-tauri/src/commands.rs
src-tauri/src/apps/action.rs
src-tauri/src/apps/windows_backend.rs
src-tauri/src/apps/mod.rs
src-tauri/src/settings.rs
src-tauri/src/result_registry.rs
src-tauri/src/session_marker.rs
src-tauri/src/validation_data.rs
```

- Reuse without modification: `src-tauri/src/model.rs`, `src-tauri/src/apps/cache.rs`, `src-tauri/src/apps/discovery.rs`, `src-tauri/src/apps/rank.rs`, and `src-tauri/src/validation_export.rs`. `result_registry.rs`, `session_marker.rs`, and `validation_data.rs` may receive only the exact Task 6 item-level temporary attributes frozen below.
- Keep the following byte-identical to the implementation/trust baseline:

```text
src-tauri/build.rs
src-tauri/Cargo.lock
src-tauri/capabilities/main.json
src-tauri/permissions/**
src-tauri/tauri.conf.json
src-tauri/tauri.security-probe.conf.json
src-tauri/src/security_probe.rs
security-probe.html
src/security-probe.ts
scripts/check-security-config.ps1
scripts/test-security-config.ps1
scripts/build-security-probe.ps1
scripts/test-security-probe.ps1
package.json
package-lock.json
vite.config.ts
tsconfig.json
index.html
```

- Do not modify TypeScript/UI, lifecycle/global shortcut/tray/autostart behavior, `/find`, plugins, macOS, installer, signing, or release behavior.
- Do not run the runtime positive probe or touch the failed probe worktree. Static security config checks and build-output isolation checks remain required.
- Each nontrivial behavior starts with one smallest failing test, then the focused test, then the full task test set. Do not launch arbitrary real applications from automated tests.
- No production adapter trait. Use private closure/function seams only where tests must observe Windows calls or post-action ordering.
- A security trust-input checkpoint is mandatory before code review. Any change outside the allowlist, any frozen-file byte change, any extra Windows feature, or any need to alter security trust inputs is an immediate stop and review escalation.
- The original Task 0-6 steps remain the implementation provenance. Corrective execution after written revised Design/Plan Go runs only the amended RED/GREEN contracts in Tasks 3-5, then repeats Task 6 Steps 3-6 and Step 8 from the first command/checkpoint line. Do not repeat the already committed Step 7 wiring. Add two auditable commits without rewriting `b42baee`: one settings DTO correction and one shared hide correction.
- The post-`f204c0c` correction adds one auditable commit without rewriting history: source-oracle RED, minimal lint/cfg GREEN, and the P2 test-expression cleanup. It does not implement Task 6 lifecycle behavior.

## Frozen Interfaces

### Command surface and caller guard

Production registers exactly these eight commands:

```rust
search_apps(query, invocation_id, query_sequence) -> Result<Option<SearchResponse>, CommandError>
execute_result(request_id, result_id) -> Result<ExecuteOutcome, CommandError>
load_settings() -> Result<SettingsView, CommandError>
save_settings(settings: UserSettingsUpdate) -> Result<(), CommandError>
rescan_apps() -> Result<(), CommandError>                 // async command
export_validation_data() -> Result<ExportOutcome, CommandError> // async command
clear_validation_data() -> Result<(), CommandError>
hide_launcher() -> Result<(), CommandError>
```

Tauri-injected `WebviewWindow`, `AppHandle`, and managed state are not frontend parameters. Every wrapper calls one crate-private guard as its first executable statement:

```rust
fn require_main_window(window: &tauri::WebviewWindow) -> Result<(), CommandError> {
    (window.label() == "main")
        .then_some(())
        .ok_or_else(CommandError::invalid_caller)
}
```

No registry/cache/store read, adapter call, worker/main-thread dispatch, or window mutation may precede the guard. The feature-only handler remains exactly the existing `security_probe::load_settings`; production commands are never registered in a `test-instrumentation` build.

### DTOs and fixed errors

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppAliasTarget {
    app_id: String,
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsView {
    hotkey: String,
    autostart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    research_id: Option<String>,
    applications: Vec<AppAliasTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserSettingsUpdate {
    hotkey: String,
    autostart: bool,
    research_id: Option<String>,
    aliases: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum ExecuteOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested { message: &'static str },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum ExportOutcome {
    Cancelled,
    Exported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: &'static str,
}
```

Only these fixed error codes are allowed: `invalidCaller`, `staleRequest`, `unknownResult`, `applicationEntryUnavailable`, `settingsFailed`, `validationFailed`, `windowFailed`, `scanFailed`, `scanWorkerFailed`, `mainThreadDispatchFailed`, `exportFailed`, and `exportWorkerFailed`. Code/message/log output must not contain query, ID, application name, PID, HWND, HRESULT, path, or raw system error text.

`SettingsView.applications` projects every application in the current unique `AppCache` snapshot, in snapshot order,
including empty aliases and duplicate display names with distinct `appId` values. It exposes only `appId`,
`displayName`, optional `icon`, and aliases. Temporarily absent store aliases are omitted from the view but remain in
the store. `UserSettingsUpdate.aliases` is the only save identity input; `displayName` and `icon` are never accepted
as update keys, and no settings DTO exposes shortcut, executable, path, action payload, or `useCounts`.
`SettingsView.researchId` is omitted for `None` and is the exact string for `Some`; it is never JSON `null`.
`UserSettingsUpdate.researchId` input continues to accept missing or `null` as `None`.

### Shared clear-and-hide interface

```rust
pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    clear_and_hide_with(
        || registry.hide_and_clear(),
        || window.hide().map_err(|_| ()),
    )
}

fn clear_and_hide_with<C, H>(clear: C, hide: H) -> Result<(), CommandError>
where
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
{
    clear();
    hide().map_err(|_| CommandError::window_failed())
}
```

Task 5 owns exactly this helper. `execute_result` calls it immediately and exactly once after a successful system
action; `hide_launcher` calls it after the main-caller guard. System-action failure never calls it. Task 6 consumes
the same crate-private helper for every lifecycle hide path. No caller may directly clear and later call the helper,
which would double-clear and advance the registry generation twice.

### Native action result

`apps/action.rs` owns only this domain result and a private closure seam; it must not depend on Tauri or stores:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApplicationActionOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested,
}

pub(crate) fn execute_application(
    action: &crate::result_registry::ResultAction,
) -> Result<ApplicationActionOutcome, ApplicationActionError>;
```

`commands.rs` maps this outcome to the frozen `ExecuteOutcome` and `ValidationEvent`. The trusted `app_id`, shortcut, and optional executable originate only from the resolved `ResultAction`.

## Test Contract Map

The approved 15 groups are not a final-only checklist. They are assigned to RED phases below:

| Group | RED phase | Contract |
|---|---|---|
| 1 | Task 3 | Search old invocation, sequence/hide races, empty query, 20 cap, private action |
| 2 | Tasks 3-4 | Stale/unknown result and forged settings ID never reach Windows seam |
| 3 | Task 1 | ToolHelp zero/one/multiple and same-basename indeterminate |
| 4 | Task 1 | Ordinal-ignore-case comparisons; no canonicalize/metadata/traversal |
| 5 | Task 1 | Snapshot/process RAII and unique handle lifetime |
| 6 | Task 1 | Eligible first HWND, callback always `TRUE`, full enumeration |
| 7 | Task 1 | Exit/PID/window races and enumeration uncertainty fall back |
| 8 | Tasks 1-2 | Activation true/false and launch failure behavior |
| 9 | Tasks 4-5 | One immediate shared helper call, registry-before-window, fixed error priority |
| 10 | Task 4 | Three outcomes to four approved counters; no failure/query recording |
| 11 | Task 5 | Rescan worker boundary and old-cache preservation |
| 12 | Task 5 | Main-thread chooser, cancellation, blocking writer |
| 13 | Tasks 3 and 5 | Exact ordered targets/aliases, optional researchId JSON, absent/count preservation |
| 14 | Tasks 3 and 6 | Eight guards, exact manifest/capability parity, no path/PID/HWND input |
| 15 | Tasks 0 and 6 | A' feature delta and frozen trust inputs |

---

## Task 0: Establish the Approved Implementation / Trust Baseline

**Files:** None. This task creates only the later isolated branch/worktree after written Plan Go.

- [ ] **Step 1: Confirm every prerequisite before creating the worktree**

Record all of the following in the implementation thread: Design Go at `763632816ef5a75bf5cfcbd7ebcbcecdeaf098e0`, Plan Go at the exact future plan commit, Task 4C Code Go, and clean local `main` at the baseline below.

Run from the main repository:

```powershell
$baseline = '2788e9a275e0406e70d7597a4a78da274d8c55c0'
git rev-parse main
git status --short
git show --no-patch --format='%H %s' $baseline
```

Expected: `git rev-parse main` equals `$baseline`; status is empty. If not, stop. Do not substitute `origin/main`, rebase, merge, cherry-pick, or create a tag.

- [ ] **Step 2: Create one managed Task 5 worktree at the exact baseline**

Use the environment's managed-worktree mechanism. The resulting branch must use a new `codex/` name and its initial `HEAD` must equal `$baseline`.

```powershell
git rev-parse HEAD
git status --short --branch
```

Expected: exact baseline SHA and clean status.

- [ ] **Step 3: Capture the trust baseline hashes without generating permissions**

```powershell
$baseline = '2788e9a275e0406e70d7597a4a78da274d8c55c0'
git ls-tree -r $baseline -- src-tauri/build.rs src-tauri/Cargo.lock src-tauri/capabilities src-tauri/permissions src-tauri/tauri.conf.json src-tauri/tauri.security-probe.conf.json src-tauri/src/security_probe.rs security-probe.html src/security-probe.ts scripts package.json package-lock.json vite.config.ts tsconfig.json index.html
```

Keep the output in the implementation review record, not a new repository file. Do not run Cargo during this baseline capture.

- [ ] **Step 4: Prepare the frozen Node dependency tree only when absent**

The new worktree may not contain `node_modules`. From the authenticated repository root, use only the lockfile-defined install and immediately prove that the frozen manifests did not change:

```powershell
$baseline = '2788e9a275e0406e70d7597a4a78da274d8c55c0'
if (-not (Test-Path -LiteralPath node_modules -PathType Container)) {
  npm.cmd ci --ignore-scripts --no-audit --no-fund
  if ($LASTEXITCODE -ne 0) { throw 'npm ci failed; report the blocker without choosing another install method' }
}
git diff --exit-code $baseline -- package.json package-lock.json
if ($LASTEXITCODE -ne 0) { throw 'Node dependency preparation changed frozen manifests' }
```

If this exact command cannot support the later build, stop and report it. Do not substitute `npm install`, another package manager, script enablement, or a lockfile rewrite.

---

## Task 1: Implement the Windows Native Adapter Fail-Safely

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/apps/windows_backend.rs`
- Modify: `src-tauri/src/apps/mod.rs`

- [ ] **Step 1: RED - freeze process uniqueness and indeterminate semantics**

Add private model/seam tests in `windows_backend.rs`. The table-driven cases must assert:

```rust
struct ProcessFixture {
    pid: u32,
    basename_utf16: &'static [u16],
    full_path_utf16: Result<&'static [u16], ()>,
}

// Expected decisions after complete enumeration:
// []                                      => NativeActivation::Unavailable
// [one exact full-path match]             => inspect that PID's windows
// [two exact full-path matches]            => NativeActivation::Unavailable
// [same basename whose query fails]        => NativeActivation::Indeterminate
// [one exact + one same-basename failure]  => NativeActivation::Indeterminate
// [different basename whose query fails]   => still evaluate exact candidates;
//                                             it is never opened in production
// [one exact match, then Process32NextW fails with a nonterminal error]
//                                          => NativeActivation::Indeterminate
```

Add fixed ASCII and non-ASCII case vectors. Basename and full-path comparison must pass the original UTF-16 slices directly to `CompareStringOrdinal(left, right, TRUE)` and accept only `CSTR_EQUAL`. Tests and implementation prohibit `to_lowercase`, `eq_ignore_ascii_case`, `to_string_lossy`, UTF-8 conversion, or Unicode normalization. `Win32_Globalization` is already in the baseline resolved feature graph; do not change the approved direct Cargo feature delta.

Make the seam expose no filesystem call. There must be no `canonicalize`, target `metadata`, directory walk, network resolution, or environment expansion in the implementation.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::windows_backend::tests::process -- --nocapture
```

Expected RED: module/helper is absent or assertions fail.

- [ ] **Step 2: GREEN - add only the A' features and ToolHelp enumeration**

Extend the existing `windows` feature list without removing/reordering Task 4C features:

```toml
  "Win32_System_Diagnostics_ToolHelp",
  "Win32_System_Threading",
  "Win32_UI_WindowsAndMessaging",
```

Do not add `Win32_System_ProcessStatus`, another feature, crate, version, or lockfile update. `Win32_System_Diagnostics_ToolHelp` must be the only feature newly entering the resolved graph; the other two make existing transitive features explicit because product code directly uses their APIs.

Implement:

- `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)` plus complete `Process32FirstW`/`Process32NextW` enumeration.
- Reject `INVALID_HANDLE_VALUE`; initialize `PROCESSENTRY32W.dwSize` before the first call.
- Treat `Process32NextW == FALSE` as normal completion only when `GetLastError() == ERROR_NO_MORE_FILES`. Any other last-error value, including a failure after one exact match was already seen, makes the entire result indeterminate.
- Basename prefilter from `PROCESSENTRY32W.szExeFile` before `OpenProcess`.
- `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)` and `QueryFullProcessImageNameW` only for same-basename candidates.
- Exact basename and full-path comparison by `CompareStringOrdinal(..., TRUE)` over the original UTF-16; never approximate it through lowercasing or lossy text conversion.
- Treat an insufficient/failed `QueryFullProcessImageNameW` buffer result as indeterminate; do not compare a truncated path.
- An owned RAII `HANDLE` wrapper that closes every valid snapshot/process handle exactly once.
- Retain the unique matching process handle through the activation decision so PID reuse cannot be silently promoted.

Any snapshot/enumeration/query failure or any unqueryable same-basename candidate yields `NativeActivation::Indeterminate`. `windows_backend` does not launch a shortcut or map a domain outcome; fallback ownership belongs only to `apps/action.rs`.

- [ ] **Step 3: RED - freeze complete EnumWindows behavior**

Add a private enumeration seam test whose callback input includes PID, visible, owner, tool-window, and property-query success. Assert:

```rust
// First eligible HWND is recorded.
// Every callback return is TRUE, including the first and later eligible HWNDs.
// Later candidates never replace the first Z-order match.
// Any target-related property uncertainty invalidates the recorded HWND.
// EnumWindows failure invalidates the recorded HWND.
```

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::windows_backend::tests::window -- --nocapture
```

Expected RED: window enumeration is not implemented.

- [ ] **Step 4: GREEN - enumerate and activate one safe window**

Only after exactly one full-path matching process, call `EnumWindows`. A candidate requires matching PID, `IsWindowVisible`, no `GW_OWNER`, and no `WS_EX_TOOLWINDOW`. The callback records only the first match and always returns `TRUE` to finish enumeration. Never return `FALSE` as “found”.

Call `SetForegroundWindow` exactly once only after complete successful enumeration with no target-related uncertainty. The primitive maps true to `NativeActivation::Activated` and false to `NativeActivation::Refused`. It never launches a shortcut.

- [ ] **Step 5: RED/GREEN - freeze the two native primitives and their call counts**

The only production surface exported by `windows_backend` is equivalent to:

```rust
pub(crate) enum NativeActivation {
    Activated,
    Refused,
    Unavailable,
    Indeterminate,
}

pub(crate) fn try_activate(executable: &Path) -> NativeActivation;
pub(crate) fn launch_shortcut(shortcut: &Path) -> Result<(), NativeActionError>;
```

Add primitive-level tests for process exit, PID/window race, no eligible window, failed/tail-failed enumeration, vanished HWND, activation true, activation false, and `ShellExecuteW` failure. `try_activate` must make zero `ShellExecuteW` calls on every branch. `launch_shortcut` makes exactly one `ShellExecuteW` call using the registry-owned `.lnk`, default open operation, no arguments, and no working directory; a result greater than 32 means request accepted.

Required results:

```text
unique eligible + foreground true  -> NativeActivation::Activated; zero launch calls
unique eligible + foreground false -> NativeActivation::Refused; zero launch calls
zero/multiple/no-window/race        -> NativeActivation::Unavailable; zero launch calls
snapshot/query/tail uncertainty     -> NativeActivation::Indeterminate; zero launch calls
launch primitive success/failure    -> exactly one ShellExecuteW call
```

- [ ] **Step 6: Verify and commit the adapter slice**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml apps::windows_backend -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml
git diff --exit-code 2788e9a275e0406e70d7597a4a78da274d8c55c0 -- src-tauri/Cargo.lock
git diff --check
git status --short
git add src-tauri/Cargo.toml src-tauri/src/apps/windows_backend.rs src-tauri/src/apps/mod.rs
git commit -m "feat: add fail-closed Windows action primitives"
```

Expected: tests/check pass; lockfile diff is empty; only the three listed files are committed.

---

## Task 2: Add the Minimal Application Action Policy

**Files:**
- Create: `src-tauri/src/apps/action.rs`
- Modify: `src-tauri/src/apps/mod.rs`

- [ ] **Step 1: RED - freeze the policy without a backend trait**

Use a private closure seam with exactly the information already present in `ResultAction`. This is the only fallback/outcome state machine. Tests cover:

```rust
// executable None: launch shortcut once; never enumerate/activate.
// unique eligible window + activation true: activation outcome; never launch.
// activation refused: launch once; refusal+launch outcome.
// zero/multiple/indeterminate/race: launch once; ordinary launch outcome.
// launch failure: fixed domain error; no retry.
```

The seam accepts one private `FnMut(&Path) -> NativeActivation` activation primitive and one private `FnMut(&Path) -> Result<(), NativeActionError>` launch primitive. Assert the activation/launch call counts on every row. It must not become a public trait or general command runner.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::action::tests -- --nocapture
```

Expected RED: `action.rs` is absent.

- [ ] **Step 2: GREEN - map the native adapter to three domain outcomes**

Implement only `ApplicationActionOutcome`, fixed `ApplicationActionError`, `execute_application`, and the private test seam. `apps/action.rs` exclusively maps executable `None` and `NativeActivation::{Activated, Refused, Unavailable, Indeterminate}` to launch/no-launch and the three domain outcomes. `windows_backend` must not return `ApplicationActionOutcome` or perform fallback. Do not add Tauri/store dependencies, retries, polling, elevation, thread-input attachment, process termination, or focus verification.

- [ ] **Step 3: Verify and commit**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml apps::action -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml apps::windows_backend -- --nocapture
git diff --check
git add src-tauri/src/apps/action.rs src-tauri/src/apps/mod.rs
git commit -m "feat: define application action policy"
```

---

## Task 3: Add Guarded Search and Settings Commands

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/settings.rs`

- [ ] **Step 1: RED - freeze the common caller guard before state access**

Create `commands.rs` tests around a private command-core seam that records state reads and side effects. Exercise all eight command names in a table. For caller label other than exact `main`, assert `invalidCaller` and an empty trace.

The wrapper body shape is mandatory:

```rust
#[tauri::command]
pub(crate) fn load_settings(
    window: tauri::WebviewWindow,
    settings: tauri::State<'_, SettingsStore>,
    cache: tauri::State<'_, Arc<AppCache>>,
) -> Result<SettingsView, CommandError> {
    require_main_window(&window)?;
    // state use begins only here
    load_settings_core(&settings, &cache)
}
```

No test helper may make a wrapper appear guarded while the production wrapper reads injected state first.

- [ ] **Step 2: RED - freeze search ordering and races**

Add tests for old invocation, non-increasing sequence, older query publishing after a newer query, hide during ranking, empty query, 20-result cap, and Rust-private action storage. The core must follow exactly:

```rust
let token = match registry.begin_query(&invocation_id, query_sequence) {
    Some(token) => token,
    None => return Ok(None),
};
let mut applications = cache.snapshot();
settings.decorate_applications(&mut applications);
let entries = apps::rank(&applications, &query)
    .iter()
    .map(apps::registry_entry)
    .collect();
Ok(registry.publish_if_latest(token, entries))
```

The invalid/old path must not read cache or settings. Search performs no rescan or disk traversal and serializes no `appId`, shortcut, or executable.

- [ ] **Step 3: GREEN - implement only `search_apps` and its fixed errors**

Implement only the search core/wrapper and fixed errors required by Steps 1-2. During corrective execution from
`b42baee`, do not add or modify `SettingsView`, `AppAliasTarget`, `UserSettingsUpdate`, `load_settings`, or
`save_settings` in this step. The settings RED below must run against the uncorrected bidirectional DTO first.

- [ ] **Step 4: RED - freeze settings DTOs, exact projection, and roundtrip before implementation**

Write tests that reference the independent `SettingsView`, `AppAliasTarget`, and `UserSettingsUpdate` types before
implementing them. Seed the store with `APP_DUPLICATE_A -> ["seed alias"]`,
`APP_ABSENT -> ["absent alias"]`, and nonzero use counts for both IDs. The current `AppCache` snapshot order is
exactly `APP_EMPTY`, `APP_DUPLICATE_A`, `APP_DUPLICATE_B`, with these expected targets:

```text
0: appId=APP_EMPTY, displayName="Empty App", icon=None, aliases=[]
1: appId=APP_DUPLICATE_A, displayName="Duplicate App", icon=Some("icon-a"), aliases=["seed alias"]
2: appId=APP_DUPLICATE_B, displayName="Duplicate App", icon=None, aliases=[]
```

Assert the complete ordered `applications` vector equals those three targets, not only its IDs or length. Also
assert:

```text
APP_ABSENT is not exposed in SettingsView
researchId=None omits the JSON field entirely; researchId=Some("study_01") emits that exact string
Task 7 can construct a complete aliases map keyed by every returned appId; save succeeds
final SettingsStore snapshot retains APP_DUPLICATE_A=["seed alias"] and APP_ABSENT=["absent alias"]
all seeded useCounts are byte-for-byte/value-for-value unchanged after save
serialized SettingsView has no shortcut/executable/path/useCounts and never emits researchId:null
forged format or unknown new appId fails the entire save
failed save leaves settings unchanged and records zero Windows seam calls
```

Expected RED from `b42baee`: compile failure because the three split DTO types do not exist, or assertion failure
because the old DTO omits empty/current targets and serializes `None` research ID as `null`. Do not weaken the test
to accept the old `aliases`-only response.

- [ ] **Step 5: GREEN - split DTOs and project all current applications**

In `settings.rs`, remove only `#[cfg(test)]` from the existing `SettingsStore::snapshot`; do not change its body, update transaction, validation, or persistence.

In `commands.rs`, add the exact independent `SettingsView`, `AppAliasTarget`, and `UserSettingsUpdate` definitions
from Frozen Interfaces, including `skip_serializing_if = "Option::is_none"` only on
`SettingsView.research_id`. Call both snapshots only after the caller guard. Iterate the current cache snapshot in
its existing order and create one `AppAliasTarget` per application, looking up aliases by `app_id` and defaulting to an empty
vector. Copy only `app_id`, `display_name`, safe optional `icon`, and aliases; do not copy private paths or
`use_count`. Convert `UserSettingsUpdate` to existing `SettingsUpdate` and call `update_user_settings`; this existing
store method clones the old candidate and therefore preserves aliases for temporarily absent applications. `appId`
must never be passed to `apps::action` or `windows_backend`.

- [ ] **Step 6: Run focused tests and commit**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::caller -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::search -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::settings -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml settings -- --nocapture
git diff --check
git add src-tauri/src/commands.rs
git commit -m "fix: restore settings application targets"
```

Expected corrective commit scope: only `src-tauri/src/commands.rs`. `src-tauri/src/settings.rs` already exposes the
approved snapshot and its Task 4 transaction remains unchanged.

---

## Task 4: Execute Registered Results and Preserve Post-Action Ordering

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: RED - stale and unknown IDs stop before every side effect**

Add tests where `ResultRegistry::resolve` returns `StaleRequest` or `UnknownResult`. Assert fixed error mapping and zero Windows adapter, validation, settings, registry invalidation, and hide calls. The command accepts only `requestId/resultId`; no `appId`, path, PID, HWND, arguments, or payload field is allowed.

- [ ] **Step 2: RED - freeze immediate shared hide and fixed error priority**

Drive a private `execute_result_core` with one `clear_and_hide` closure plus the existing action/validation/settings
seams. Cover every single-failure position and all combinations of `windowFailed`, `validationFailed`, and
`settingsFailed`. Required trace after a successful system action:

```text
resolve
system-action
clear-and-hide.registry
clear-and-hide.window
validation-record
settings-increment
```

Assertions:

- `clear_and_hide` runs exactly once immediately after system action success and invalidates before window hide.
- A window hide failure leaves the registry inactive/empty and still calls validation and settings exactly once.
- Validation failure still calls settings increment; no failure repeats helper or system action.
- Final error uses fixed priority `validationFailed > settingsFailed > windowFailed`, independent of occurrence time.
- The system action runs exactly once in every success/persistence-failure combination.
- System-action failure preserves registry/window and performs no count or helper call.
- An already active result mapping cannot resolve again as soon as helper returns, including simulated focus/show
  failure where window hide returns `windowFailed`.

- [ ] **Step 3: RED - freeze outcome/counter mapping**

Assert the only mappings are:

```rust
ApplicationActionOutcome::LaunchRequested
    => (ExecuteOutcome::LaunchRequested, ValidationEvent::LaunchRequested),
ApplicationActionOutcome::ActivationRequested
    => (ExecuteOutcome::ActivationRequested, ValidationEvent::ActivationRequested),
ApplicationActionOutcome::ActivationRefusedLaunchRequested
    => (
        ExecuteOutcome::ActivationRefusedLaunchRequested {
            message: "Windows 拒绝了前台切换，已发送启动请求",
        },
        ValidationEvent::ActivationRefusedLaunchRequested,
    ),
```

The existing validation store maps these to `applicationLaunchRequests`, `activationSuccesses`, and `activationRefusals + applicationLaunchRequests`. Query content and failed actions are never recorded. `LauncherInvoked` remains Task 6.

- [ ] **Step 4: GREEN - implement `execute_result` with the shared helper**

Define the exact crate-private `clear_and_hide(registry, window)` from Frozen Interfaces plus a private closure seam
that proves registry-before-window without constructing a Tauri window. Resolve the registry-owned action, copy only
its trusted `app_id`, and execute through `apps::execute_application`. On success, call `clear_and_hide` immediately
and save its optional window error; then call validation and settings once each. Return
`validation_error.or(settings_error).or(window_error)` so the fixed business priority is independent of occurrence
time. Do not call helper for system-action failure and do not add rollback, compensation, retry, or a second clear.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::execute -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml validation_data -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml settings -- --nocapture
git diff --check
```

Do not commit the helper correction yet: Task 5 Step 3 must first replace `hide_launcher` with the same production
helper. The combined correction is committed once in Task 5 Step 4.

---

## Task 5: Add Blocking Rescan, Export, Clear, and Hide Wrappers

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: RED/GREEN - rescan only in a blocking worker**

Add a seam test that records the caller thread and worker execution. `rescan_apps` clones the unique managed `Arc<AppCache>`, calls `tauri::async_runtime::spawn_blocking`, and awaits it. Map discovery failure to `scanFailed` and join failure to `scanWorkerFailed`; existing `AppCache::refresh` preserves the previous snapshot on discovery failure.

- [ ] **Step 2: RED/GREEN - export chooser and writer stay on their approved threads**

Add tests for dispatcher failure, chooser cancel, chooser failure, writer success, writer failure, and join failure. Freeze this flow:

```text
main caller guard
run_on_main_thread -> obtain main HWND -> choose_export_destination(HWND) once
None -> Cancelled, no writer
Some(owned destination) -> spawn_blocking once
worker obtains managed SettingsStore + ValidationStore from cloned AppHandle
worker calls write_validation_export once
success -> Exported
```

Use `tauri::async_runtime::channel(1)`; do not add a Tokio dependency. The frontend supplies no owner, path, filename, or export payload. No store lock is held while the dialog is open.

- [ ] **Step 3: RED/GREEN - all hide paths consume the one shared helper**

`clear_validation_data` calls only the existing daily-count clear path and never removes/changes the session marker.
After its common caller guard, `hide_launcher` calls only `clear_and_hide(&registry, &window)`; remove the separate
private `hide_launcher_with` state machine and every direct clear-plus-later-hide production path.

The shared closure-core tests must prove:

```text
helper call count = 1
trace = [registry-hide-and-clear, window-hide]
window hide failure -> windowFailed and registry inactive/empty
existing active mapping + simulated Task 6 focus/show failure -> mapping immediately cannot resolve
execute_result system-action failure -> helper call count = 0
```

`hide_launcher` never calls `mark_clean_exit`. Task 6 later imports this same crate-private helper for focus loss,
close, modal recovery, and show/focus/emit failures; it must not copy the sequence.

- [ ] **Step 4: Re-run all eight-command guard tests and commit**

The table test from Task 3 must now invoke every final wrapper and prove non-main callers cause zero state access/side effects.

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml commands::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml validation_export -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml apps::cache -- --nocapture
git diff --check
git add src-tauri/src/commands.rs
git commit -m "fix: unify launcher hide paths"
```

Expected corrective commit scope: only `src-tauri/src/commands.rs`, containing both the Task 4 execute path and Task
5 `hide_launcher` consumption of the one helper. No lifecycle/Task 6 product code is added here.

---

## Task 6: Wire the Production Handler and Run the Security Trust-Input Checkpoint

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/result_registry.rs`
- Modify: `src-tauri/src/session_marker.rs`
- Modify: `src-tauri/src/validation_data.rs`
- Modify: `src-tauri/src/commands.rs` (P2 test expression only)
- Review only: all allowlisted and frozen files

- [ ] **Step 1: RED - freeze production/feature handler separation**

Keep the existing handler oracle and add one source oracle that fails on every module-wide `allow(dead_code)` /
`allow(unused_imports)`. It must require all product module declarations to use exact
`#[cfg(any(test, not(feature = "test-instrumentation")))]`, and require only six exact item-level attributes for the
five Task 6 logical APIs frozen in the design. The normal builder still manages exactly one `ResultRegistry` and
registers the eight production commands, while `test-instrumentation` registers only
`security_probe::load_settings`. The normal handler must be:

```rust
tauri::generate_handler![
    commands::search_apps,
    commands::execute_result,
    commands::load_settings,
    commands::save_settings,
    commands::rescan_apps,
    commands::export_validation_data,
    commands::clear_validation_data,
    commands::hide_launcher,
]
```

Do not change `build.rs` or `capabilities/main.json`; they already declare exactly these eight commands. Task 6 later calls `on_show` on this same managed registry.

- [ ] **Step 2: GREEN - wire only the production module/state/handler**

For corrective GREEN, remove every module-level warning suppression and the obsolete Task 2 comment. Compile all
product modules under `any(test, not(feature = "test-instrumentation"))`; keep product cache creation, management,
store setup, and initial refresh only in normal production. The feature-only non-test probe target compiles no product
module and registers only its probe command. Add the exact conditionally-scoped `dead_code` item attribute to
`ResultRegistry::on_show`, `read_marker_for_clean`, `ValidationEvent::LauncherInvoked`,
`ValidationError::SessionOwnershipLost`, `mark_clean_exit`, and `mark_clean_exit_with`; no impl/enum/module/crate or
`unused_imports` allowance is permitted. Rename the P2 settings test and assert its real contract directly without a
fake `windows_calls` counter. Do not add capability, permission, lifecycle, window-show, shortcut, tray, or autostart
changes.

Run the focused source oracle before GREEN and record the expected failure caused by the existing module-wide allows
and missing exact item attributes. After GREEN, rerun that test plus default/all-features check before the full gate.

- [ ] **Step 3: Verify all Rust behavior before the trust review**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
git diff --check
```

Expected: all pass. Both default and all-features Clippy are required. If and only if a Clippy invocation hits the known rustc 1.95 incremental ICE, rerun that same failed invocation with `CARGO_INCREMENTAL=0`; do not weaken lint flags or skip the other mode. These commands may refresh generated permission mtimes/stat; if blobs are identical, close that as index/stat noise. Never accept a blob change and do not commit generated permissions.

- [ ] **Step 4: Run static security/build isolation checks, never the runtime positive probe**

```powershell
npm.cmd test -- --passWithNoTests
npm.cmd run build
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
if (Test-Path -LiteralPath dist\security-probe.html -PathType Leaf) {
  throw 'security-probe.html leaked into production dist'
}
$probeLeaks = @(
  Get-ChildItem -LiteralPath dist -Force -Recurse |
    Where-Object { -not $_.PSIsContainer -and $_.Name -like 'security-probe*' }
)
if ($probeLeaks) { throw "security probe leaked into production dist: $($probeLeaks.FullName -join ', ')" }
```

Expected: the no-test-yet frontend command, production build, real-repository security check, and malicious-fixture regression pass; production `dist` excludes `security-probe.html` and every related probe artifact. `test-security-config.ps1` does not replace the preceding real-repository check because its fixture copies only `main.json`. Do not run `scripts/build-security-probe.ps1`, `scripts/test-security-probe.ps1`, or claim runtime ACL positive evidence.

- [ ] **Step 5: Run one fail-closed trust-input checkpoint**

Run this single inline checkpoint from the implementation worktree root before staging the final wiring commit. Do not extract it into a reusable checker or add a fixture framework:

```powershell
$ErrorActionPreference = 'Stop'
$baseline = '2788e9a275e0406e70d7597a4a78da274d8c55c0'
$allowed = @(
  'src-tauri/Cargo.toml',
  'src-tauri/src/lib.rs',
  'src-tauri/src/commands.rs',
  'src-tauri/src/apps/action.rs',
  'src-tauri/src/apps/windows_backend.rs',
  'src-tauri/src/apps/mod.rs',
  'src-tauri/src/settings.rs',
  'src-tauri/src/result_registry.rs',
  'src-tauri/src/session_marker.rs',
  'src-tauri/src/validation_data.rs'
)
$frozen = @(
  'src-tauri/build.rs',
  'src-tauri/Cargo.lock',
  'src-tauri/capabilities/main.json',
  'src-tauri/permissions',
  'src-tauri/tauri.conf.json',
  'src-tauri/tauri.security-probe.conf.json',
  'src-tauri/src/security_probe.rs',
  'security-probe.html',
  'src/security-probe.ts',
  'scripts/check-security-config.ps1',
  'scripts/test-security-config.ps1',
  'scripts/build-security-probe.ps1',
  'scripts/test-security-probe.ps1',
  'package.json',
  'package-lock.json',
  'vite.config.ts',
  'tsconfig.json',
  'index.html'
)
$trustDirectories = @('src-tauri/capabilities', 'src-tauri/permissions')

$rootText = (& git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $rootText) { throw 'cannot authenticate worktree root' }
$root = [IO.Path]::GetFullPath($rootText).TrimEnd('\')
$current = [IO.Path]::GetFullPath((Get-Location).ProviderPath).TrimEnd('\')
if (-not [string]::Equals($root, $current, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'trust checkpoint must run from the authenticated worktree root'
}
$rootItem = Get-Item -LiteralPath $root -Force
if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
  throw 'authenticated worktree root is not a plain directory'
}
git merge-base --is-ancestor $baseline HEAD
if ($LASTEXITCODE -ne 0) { throw 'approved baseline is not an ancestor of HEAD' }

$auditPaths = @($allowed + $frozen | Sort-Object -Unique)
$indexFlags = @(git ls-files -v -- $auditPaths)
if ($LASTEXITCODE -ne 0) { throw 'cannot inspect tracked trust paths' }
$unsafeIndexFlags = @($indexFlags | Where-Object { $_ -cmatch '^[a-zS] ' })
if ($unsafeIndexFlags) {
  throw "assume-unchanged/skip-worktree is forbidden on audited paths: $($unsafeIndexFlags -join ', ')"
}

$directoryAuditPaths = @('src-tauri/permissions')
foreach ($relative in $auditPaths) {
  $full = Join-Path $root $relative
  if (-not (Test-Path -LiteralPath $full)) { throw "audited path is missing: $relative" }
  $item = Get-Item -LiteralPath $full -Force
  $expectsDirectory = $relative -in $directoryAuditPaths
  if ($expectsDirectory -ne $item.PSIsContainer) { throw "audited path has the wrong type: $relative" }
  $cursor = $item
  while ($null -ne $cursor) {
    if (($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "reparse point is forbidden in audited path ancestry: $relative"
    }
    if ($cursor -ne $item -and $cursor -isnot [IO.DirectoryInfo]) {
      throw "non-directory parent found in audited path ancestry: $relative"
    }
    if ([string]::Equals($cursor.FullName.TrimEnd('\'), $root, [StringComparison]::OrdinalIgnoreCase)) { break }
    $cursor = if ($cursor -is [IO.DirectoryInfo]) { $cursor.Parent } else { $cursor.Directory }
  }
  if ($null -eq $cursor) { throw "audited path escapes authenticated worktree: $relative" }
}

$baselineTrust = @(git ls-tree -r --name-only $baseline -- $trustDirectories)
if ($LASTEXITCODE -ne 0) { throw 'cannot read baseline trust inventory' }
$baselineTrustSet = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($relative in $baselineTrust) { [void] $baselineTrustSet.Add($relative) }
$workingTrust = @(
  foreach ($directory in $trustDirectories) {
    $fullDirectory = Join-Path $root $directory
    $directoryItem = Get-Item -LiteralPath $fullDirectory -Force
    if (-not $directoryItem.PSIsContainer -or ($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "trust directory is not a plain directory: $directory"
    }
    Get-ChildItem -LiteralPath $fullDirectory -Force -Recurse | ForEach-Object {
      if (($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "reparse point is forbidden in trust inventory: $($_.FullName)"
      }
      if (-not $_.PSIsContainer) {
        $candidate = [IO.Path]::GetFullPath($_.FullName)
        $rootPrefix = "$root\"
        if (-not $candidate.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
          throw "trust candidate escapes authenticated worktree: $candidate"
        }
        $candidate.Substring($rootPrefix.Length).Replace('\', '/')
      }
    }
  }
) | Sort-Object -Unique
$unexpectedTrust = @($workingTrust | Where-Object { -not $baselineTrustSet.Contains($_) })
$workingTrustSet = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($relative in $workingTrust) { [void] $workingTrustSet.Add($relative) }
$missingTrust = @($baselineTrust | Where-Object { -not $workingTrustSet.Contains($_) })
if ($unexpectedTrust -or $missingTrust) {
  throw "capability/permission inventory differs from baseline; unexpected=[$($unexpectedTrust -join ', ')], missing=[$($missingTrust -join ', ')]"
}

$committedChanges = @(git diff --name-only "$baseline...HEAD")
if ($LASTEXITCODE -ne 0) { throw 'cannot inspect committed change set' }
$stagedChanges = @(git diff --cached --name-only)
if ($LASTEXITCODE -ne 0) { throw 'cannot inspect staged change set' }
$workingChanges = @(git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'cannot inspect working-tree change set' }
$untracked = @(git ls-files --others --exclude-standard)
if ($LASTEXITCODE -ne 0) { throw 'cannot inspect untracked change set' }
$changed = @($committedChanges + $stagedChanges + $workingChanges + $untracked) |
  Where-Object { $_ } |
  Sort-Object -Unique
$unexpected = $changed | Where-Object { $_ -notin $allowed }
if ($unexpected) { throw "Task 5 changed files outside allowlist: $($unexpected -join ', ')" }

if ($untracked) { throw "non-ignored untracked files are forbidden: $($untracked -join ', ')" }

git diff --exit-code $baseline HEAD -- $frozen
if ($LASTEXITCODE -ne 0) { throw 'committed frozen trust input changed' }
git diff --cached --exit-code -- $frozen
if ($LASTEXITCODE -ne 0) { throw 'staged frozen trust input changed' }
git diff --exit-code -- $frozen
if ($LASTEXITCODE -ne 0) { throw 'working-tree frozen trust input changed' }
```

Expected: no exception. The root and baseline are authenticated; every allowed/frozen audited path exists, only `src-tauri/permissions` is a directory, every other audited path is a plain file, and every final component and ancestor through the authenticated root has the expected type and is not a reparse point; audited tracked files have no assume-unchanged/skip-worktree flags; capability/permission inventory exactly matches baseline even for ignored or hidden candidates; baseline-to-HEAD, index, and worktree tracked changes stay within the allowlist; non-ignored untracked files are absent; all frozen content remains byte-identical.

- [ ] **Step 6: Review the four security-sensitive change surfaces explicitly**

Create no new manifest file. In the code-review request, attach baseline-to-HEAD diffs for:

```text
src-tauri/Cargo.toml                  trust input: exact A' features only
src-tauri/src/lib.rs                  trust input: production handler/state wiring
src-tauri/src/commands.rs             trust input: all production command implementations
src-tauri/src/apps/windows_backend.rs native adapter: process/window/path-sensitive Win32 calls
```

Also report:

- `Win32_System_Diagnostics_ToolHelp` is the only newly resolved Windows feature.
- `Win32_System_Threading` and `Win32_UI_WindowsAndMessaging` are now explicit.
- `Win32_System_ProcessStatus`, new crates, versions, and lock changes are absent.
- All eight wrappers use the same guard before state/side effects.
- No frontend command input contains path/PID/HWND/arguments. Search/execute never expose or accept `appId`;
  only `SettingsView.applications` and `UserSettingsUpdate.aliases` contain it for settings identity.
- `execute_result`, `hide_launcher`, and the frozen Task 6 lifecycle interface consume the same one-call
  `clear_and_hide`; successful action error priority is `validationFailed > settingsFailed > windowFailed`.
- EnumWindows callback always returns `TRUE` and full enumeration is required.
- Same-basename query uncertainty prohibits activation.
- `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` remains.

- [ ] **Step 7: Commit wiring**

This step is retained as provenance for the original implementation. The post-`f204c0c` lint correction uses one new
commit and does not rewrite any prior commit:

```powershell
git add src-tauri/src/lib.rs src-tauri/src/commands.rs src-tauri/src/result_registry.rs src-tauri/src/session_marker.rs src-tauri/src/validation_data.rs
git commit -m "fix: narrow task 5 lint suppressions"
git status --short --branch
git log --oneline 2788e9a275e0406e70d7597a4a78da274d8c55c0..HEAD
```

- [ ] **Step 8: Enforce commit-level consistency, then request independent code/security review**

```powershell
$baseline = '2788e9a275e0406e70d7597a4a78da274d8c55c0'
git diff --check "$baseline..HEAD"
if ($LASTEXITCODE -ne 0) { throw 'baseline-to-HEAD diff check failed' }
$commits = @(git rev-list --reverse "$baseline..HEAD")
foreach ($commit in $commits) {
  git show --check --oneline --no-renames $commit
  if ($LASTEXITCODE -ne 0) { throw "commit diff check failed: $commit" }
}
$status = @(git status --porcelain=v1)
if ($status) { throw "final worktree is not clean: $($status -join ', ')" }
```

Expected: baseline range and every individual commit pass whitespace/error checks; staged, unstaged, and untracked state are all empty.

Send the review thread: branch, worktree, baseline, HEAD SHA, exact changed files, all validation results, clean state, trust checkpoint output, and explicit not-done items. Request a written decision on `TaskCodeGo + ReleaseSecurityBlocked`; do not infer it from passing tests or Task 4C precedent.

---

## Manual Smoke After Code Go Only

These checks are prohibited before implementation Plan Go and should normally wait for code review direction. They never remove the release block.

- [ ] Search from the real launcher and confirm visible results expose no private path/appId.
- [ ] Launch one human-selected installed application whose trusted shortcut has no executable mapping.
- [ ] With exactly one matching process and one ordinary top-level window, request activation once.
- [ ] With activation refused or no eligible window, confirm a single trusted-shortcut launch fallback.
- [ ] Confirm non-main caller attempts are rejected without state change if an approved local harness exists.
- [ ] Keep `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001`; do not run or repair the failed runtime positive probe in this task.

## Completion Report Template

Every review handoff must include:

```text
Branch:
Worktree:
Implementation/trust baseline: 2788e9a275e0406e70d7597a4a78da274d8c55c0
HEAD SHA:
Exact changed files:
Validation commands and results:
Frozen-file comparison:
Working tree state:
Security state: TaskSecurityReviewRequired
Release state: ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001
Explicitly not done: Task 6/7/8, UI, /find, plugins, macOS, installer,
runtime positive probe, tag movement, merge, push, release
Requested decision: TaskCodeGo + ReleaseSecurityBlocked Go/No-Go
```
