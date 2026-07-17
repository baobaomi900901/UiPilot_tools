# Windows Launcher Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一个可运行、可测试的 Windows 11 x64 启动器基础版本，支持全局唤起、应用搜索、启动、尽力激活、基础设置、本地验证计数和键盘/辅助技术操作。

**Architecture:** 使用单一 Tauri 2 主窗口。Vanilla TypeScript 只负责输入、列表和设置界面；所有系统能力、可信路径、结果注册表和持久化都留在 Rust。前端只能回传当前结果集中的 `requestId` 与 `resultId`，Rust 再解析真实动作。应用发现只扫描 Windows 开始菜单入口，不引入数据库、前端框架或远程内容。

**Tech Stack:** Tauri 2、Vanilla TypeScript、Vite、Vitest、Rust、`windows` crate、Tauri single-instance/global-shortcut/autostart 插件。

**Source Spec:** `docs/superpowers/specs/2026-07-17-cross-platform-launcher-mvp-design.md`

**Scope Boundary:** 本计划不实现 `/find`、翻译、macOS、第三方插件、安装包签名或试用研究。文件搜索只有在 `2026-07-17-systemindex-spike.md` 得出 Go 后才能另写正式实现计划。本计划完成表示“启动器与应用能力可验收”，不表示完整 MVP-A 已完成。

---

## Global Constraints

- 每个任务开始前必须重读本节与 `Cross-Task Interfaces`；不得在单个任务内另造不兼容合同。
- 只允许一个本地 `main` WebView。生产构建不包含测试探针、远程页面、动态 HTML、任意网络、Shell、通用文件系统或通用进程能力。
- TypeScript 不接收或回传快捷方式路径、可执行文件路径或任意动作 payload；动作只能通过当前 Rust 注册表中的 `requestId + resultId` 解析。
- 所有隐藏路径都必须进入 Rust 的同一个 `clear_and_hide` 函数；前端没有直接隐藏窗口的 capability。
- 应用扫描只允许两个开始菜单根，拒绝任何 symlink 或带 `FILE_ATTRIBUTE_REPARSE_POINT` 的目录后再递归。
- 只保存日期级验证计数，不保存精确行为时间、查询词、应用名称、快捷方式、可执行文件或文件路径。
- 每个非平凡分支先写一个会失败的最小测试，再实现并运行该任务列出的完整验证命令。
- 任务若必须改变本节接口，先修改本计划并重新审核；不能让实现者自行补合同。

## Cross-Task Interfaces

### Query ownership

Rust 每次显示窗口时生成新的 opaque `invocationId` 并随 `launcher://shown` 发给前端。前端保存它、把 `querySequence` 重置为 0，每次普通应用查询先递增，再调用：

```ts
search_apps({ query: string, invocationId: string, querySequence: number }): Promise<SearchResponse | null>
```

Rust 注册表内部持有当前 generation、窗口 active 状态、最新序号和最多一份结果集：

```rust
struct RegistryState {
    generation: u64,
    active: bool,
    active_invocation_id: Option<String>,
    latest_query_sequence: u64,
    current: Option<ResultSet>,
}

struct QueryToken {
    generation: u64,
    query_sequence: u64,
}
```

`begin_query(invocation_id, sequence)` 只接受 active generation、匹配当前 `invocationId` 且严格大于当前值的序号并返回 token。`publish_if_latest(token, entries)` 只有在 generation 仍相同、窗口仍 active 且 token 序号仍为最新时才替换 `current`；否则返回 `None` 且不修改注册表。窗口隐藏会递增 generation、设为 inactive、清除 active invocation 并清空结果；窗口显示会递增 generation、写入新的 invocation、设为 active 并把最新序号重置为 0。这样旧查询晚完成、旧 IPC 在下一次唤起后才到达 Rust，以及隐藏后的在途查询都不能重新发布。

成功响应固定为：

```ts
interface SearchResponse {
  requestId: string
  items: ResultItem[]
}
```

`null` 只表示该查询已被更新序号或隐藏 generation 淘汰，前端直接忽略。

### Window lifecycle

```rust
fn clear_and_hide(window: &WebviewWindow, registry: &ResultRegistry) -> Result<(), AppError>;
```

`hide_launcher` 命令、失焦、关闭请求和动作成功都调用这个函数。动作失败不隐藏。`Esc` 只能 invoke `hide_launcher`。

### Application identity and settings

`appId` 使用固定版本算法：对 `start-menu-v1\0 + rootKind + \0 + normalizedRelativeShortcutPath` 做 Windows CNG SHA-256，输出 `app-` 加小写十六进制。它在同一快捷方式的重复扫描间稳定、区分同名入口，且不暴露原始路径。应用移动后视为新入口。

```ts
interface AppAliasTarget {
  appId: string
  displayName: string
  icon?: string
  aliases: string[]
}

interface SettingsView {
  hotkey: string
  autostart: boolean
  researchId?: string
  applications: AppAliasTarget[]
}
```

`load_settings` 返回 `SettingsView`；`save_settings` 只接受已存在 `appId` 对应的别名，不接受显示名称或路径作为键。

### Validation export

```rust
const VALIDATION_SCHEMA_VERSION: u32 = 1;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyCounts {
    launcher_invocations: u64,
    application_launch_requests: u64,
    activation_successes: u64,
    activation_refusals: u64,
    file_search_sessions: u64,
    file_location_requests: u64,
    file_found: u64,
    file_not_found: u64,
    file_cancelled: u64,
    host_crashes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExport {
    schema_version: u32,
    research_id: Option<String>,
    daily_counts: BTreeMap<String, DailyCounts>,
}
```

日期键固定为本地 `YYYY-MM-DD`，通过已使用的 `windows` crate 调用 `GetLocalTime` 获得，不增加日期依赖。活跃日由 `launcherInvocations > 0` 判定；该日成功动作等于 `applicationLaunchRequests + activationSuccesses + fileLocationRequests`。文件字段在 Foundation 阶段保持 0，供通过 Spike 后的文件计划沿用。

事件映射固定为：窗口唤起增加 `launcherInvocations`；`LaunchRequested` 增加 `applicationLaunchRequests`；`ActivationRequested` 增加 `activationSuccesses`；`ActivationRefusedLaunchRequested` 同时增加 `activationRefusals` 与 `applicationLaunchRequests`；未关闭的上次会话增加其记录日期的 `hostCrashes`。失败动作不增加成功动作字段。

### Capability allowlist

`main.json` 的 permissions 必须与以下集合完全相等，不能多也不能少：

```text
allow-search-apps
allow-execute-result
allow-load-settings
allow-save-settings
allow-rescan-apps
allow-export-validation-data
allow-clear-validation-data
allow-hide-launcher
core:event:allow-listen
core:event:allow-unlisten
```

Rust 侧使用的 single-instance、global-shortcut、autostart 和原生窗口 API 不向 WebView 授权。

---

## Task 1: Scaffold a Secure Tauri Shell

**Files:**
- Create: `package.json`
- Create: `package-lock.json`
- Create: `tsconfig.json`
- Create: `vite.config.ts`
- Create: `index.html`
- Create: `src/main.ts`
- Create: `src/styles.css`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/main.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/security_probe.rs`
- Create: `security-probe.html`
- Create: `src/security-probe.ts`
- Create: `scripts/check-security-config.ps1`
- Create: `scripts/test-security-probe.ps1`

- [ ] **Step 1: Initialize the smallest supported toolchain**

Run:

```powershell
npm init -y
npm pkg set type=module scripts.dev="vite --port 1420" scripts.build="tsc --noEmit && vite build" scripts.test="vitest run" scripts.tauri="tauri"
npm install @tauri-apps/api@^2
npm install -D @tauri-apps/cli@^2 typescript vite vitest jsdom
npm run tauri init -- --ci --force --app-name UiPilot --window-title UiPilot --frontend-dist ../dist --dev-url http://localhost:1420 --before-dev-command "npm run dev" --before-build-command "npm run build"
npm run tauri add single-instance
npm run tauri add global-shortcut
npm run tauri add autostart
```

Expected: npm and Cargo manifests exist; Tauri plugins are present in both manifests.

Define an empty Cargo feature named `test-instrumentation`. Security probes, deterministic benchmark fixtures, and local performance traces must compile only when this feature is enabled.

- [ ] **Step 2: Write a security configuration check that fails on generated defaults**

Create `scripts/check-security-config.ps1` with checks for these exact invariants:

```powershell
$ErrorActionPreference = 'Stop'
$config = Get-Content "$PSScriptRoot/../src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$capabilityFiles = @(Get-ChildItem "$PSScriptRoot/../src-tauri/capabilities" -File -Filter '*.json')
if ($capabilityFiles.Count -ne 1 -or $capabilityFiles[0].Name -ne 'main.json') {
  throw 'Exactly one main capability file is allowed'
}
$capability = Get-Content $capabilityFiles[0].FullName -Raw | ConvertFrom-Json

if ($config.app.security.csp -ne "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'") {
  throw 'Unexpected CSP'
}
if ($config.app.windows.Count -ne 1 -or $config.app.windows[0].label -ne 'main') {
  throw 'Only the main WebView is allowed'
}
if ($capability.windows.Count -ne 1 -or $capability.windows[0] -ne 'main') {
  throw 'Capability must target only the main window'
}
$expectedPermissions = @(
  'allow-search-apps',
  'allow-execute-result',
  'allow-load-settings',
  'allow-save-settings',
  'allow-rescan-apps',
  'allow-export-validation-data',
  'allow-clear-validation-data',
  'allow-hide-launcher',
  'core:event:allow-listen',
  'core:event:allow-unlisten'
) | Sort-Object
if ($capability.permissions | Where-Object { $_ -isnot [string] }) {
  throw 'Scoped permission objects are not allowed'
}
$actualPermissions = @($capability.permissions) | Sort-Object
if (Compare-Object $expectedPermissions $actualPermissions) {
  throw 'Capability permission set differs from the exact allowlist'
}
Write-Output 'security config ok'
```

- [ ] **Step 3: Run the check and confirm it fails**

Run: `powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1`

Expected: non-zero exit with `Unexpected CSP` or a capability-scope error.

- [ ] **Step 4: Configure one local-only WebView and an explicit command allowlist**

Set `tauri.conf.json` to one undecorated, hidden-on-start, non-resizable `main` window with a fixed launcher size, no remote URL, and the CSP asserted above. Keep `withGlobalTauri` disabled.

Configure `src-tauri/build.rs` so registered commands participate in Tauri's permission system:

```rust
fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "search_apps",
                "execute_result",
                "load_settings",
                "save_settings",
                "rescan_apps",
                "export_validation_data",
                "clear_validation_data",
                "hide_launcher",
            ]),
        ),
    )
    .expect("failed to build Tauri application");
}
```

Give `main` exactly the permission set in `Capability allowlist`; there is no frontend window-hide permission. `vite.config.ts` includes `security-probe.html` only in `security-probe` mode, and `security_probe.rs` creates its `security-probe` WebView only under Cargo feature `test-instrumentation`. `scripts/test-security-probe.ps1` builds both modes, proves the production dist/binary omit the probe, then launches the instrumented build and asserts `load_settings` is rejected for the non-main label.

- [ ] **Step 5: Add a boot-only Rust and frontend entry point**

`src-tauri/src/main.rs` must call `uipilot_lib::run()`. `src-tauri/src/lib.rs` must initially build a Tauri app with no business commands. `src/main.ts` must render one text input with label `搜索应用` and no framework runtime.

- [ ] **Step 6: Verify the shell**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-probe.ps1
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: `security config ok`; TypeScript/Vite build succeeds; Cargo exits 0.

- [ ] **Step 7: Commit**

```powershell
git add package.json package-lock.json tsconfig.json vite.config.ts index.html src scripts src-tauri
git commit -m "build: scaffold secure Tauri launcher"
```

---

## Task 2: Lock the Result Protocol Behind a Rust Registry

**Files:**
- Create: `src-tauri/src/model.rs`
- Create: `src-tauri/src/result_registry.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/protocol.ts`

- [ ] **Step 1: Write failing registry tests**

Add tests that prove:

1. Publishing the latest token assigns an opaque `requestId` and stable per-set `resultId` values.
2. A valid `(requestId, resultId)` resolves to a Rust-owned action.
3. Query 1 begins, query 2 begins and publishes, then query 1 finishes: query 1 returns `None` and query 2 remains executable.
4. Query 2 reaches Rust before query 1: `begin_query(1)` returns `None` and cannot do work or publish.
5. A query begins, the window hides, then the query finishes: it cannot repopulate the registry.
6. An old invocation's IPC reaches Rust after a new show: its matching sequence is rejected by `invocationId`.
7. Unknown and stale IDs fail without exposing a path.

Use this public contract in the test:

```rust
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResultAction {
    LaunchApplication {
        app_id: String,
        shortcut: PathBuf,
        executable: Option<PathBuf>,
    },
}

pub(crate) struct ResultRegistry {
    next_id: AtomicU64,
    state: Mutex<RegistryState>,
}

impl ResultRegistry {
    pub(crate) fn on_show(&self, invocation_id: String);

    pub(crate) fn begin_query(
        &self,
        invocation_id: &str,
        query_sequence: u64,
    ) -> Option<QueryToken>;

    pub(crate) fn publish_if_latest(
        &self,
        token: QueryToken,
        entries: Vec<(ResultItem, ResultAction)>,
    ) -> Option<SearchResponse>;

    pub(crate) fn resolve(
        &self,
        request_id: &str,
        result_id: &str,
    ) -> Result<ResultAction, RegistryError>;

    pub(crate) fn hide_and_clear(&self);
}
```

- [ ] **Step 2: Run the Rust tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml result_registry`

Expected: compile failure because the modules and types do not exist.

- [ ] **Step 3: Implement the minimal serializable response**

Use `serde(rename_all = "camelCase")` and exactly this wire shape:

```rust
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub(crate) request_id: String,
    pub(crate) items: Vec<ResultItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultItem {
    pub(crate) result_id: String,
    pub(crate) kind: ResultKind,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) icon: Option<String>,
}
```

The private `ResultAction` map must remain non-serializable. Convert the standard-library atomic counter to fixed-format opaque strings such as `req-0000000000000001` and `item-0000000000000001`; the identifiers may encode uniqueness but no business target. Do not add UUID, database, cache, or async-runtime dependencies.

- [ ] **Step 4: Mirror only DTOs in TypeScript**

```ts
export type ResultKind = 'application' | 'file' | 'status'

export interface ResultItem {
  resultId: string
  kind: ResultKind
  title: string
  subtitle?: string
  icon?: string
}

export interface SearchResponse {
  requestId: string
  items: ResultItem[]
}
```

- [ ] **Step 5: Verify protocol tests and serialization names**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml result_registry
npm run build
```

Expected: all registry tests pass; TypeScript compiles.

- [ ] **Step 6: Commit**

```powershell
git add src src-tauri
git commit -m "feat: add trusted result registry"
```

---

## Task 3: Discover and Rank Start Menu Applications

**Files:**
- Create: `src-tauri/src/apps/mod.rs`
- Create: `src-tauri/src/apps/discovery.rs`
- Create: `src-tauri/src/apps/rank.rs`
- Create: `src-tauri/src/apps/shortcut.rs`
- Create: `src-tauri/src/apps/cache.rs`
- Create: `src-tauri/src/apps/icon.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Create: `scripts/test-start-menu-boundary.ps1`

- [ ] **Step 1: Write failing discovery and ranking tests**

Cover these cases with fake Start Menu roots and a fake shortcut resolver:

```rust
#[test]
fn scans_only_lnk_files_from_configured_roots() {}

#[test]
fn exact_prefix_contains_and_subsequence_are_ordered() {}

#[test]
fn aliases_participate_without_changing_display_name() {}

#[test]
fn recent_use_breaks_equal_score_ties() {}

#[test]
fn empty_query_returns_no_results() {}

#[test]
fn limits_results_to_twenty() {}

#[test]
fn cache_never_exposes_shortcut_or_executable_paths_in_result_items() {}

#[test]
fn icon_extraction_failure_uses_the_local_generic_icon() {}

#[test]
fn duplicate_display_names_have_distinct_stable_app_ids() {}

#[test]
fn aliases_are_bound_to_app_id_not_display_name() {}
```

The ranking fixture must include `企业微信`, `微信`, `Visual Studio Code`, and unrelated entries. Assert that `企业` and `微信` find the expected applications through exact/prefix/contains/subsequence rules only.

`scripts/test-start-menu-boundary.ps1` must create a temporary allowed root, a separate outside directory containing a fake `.lnk`, and a directory junction inside the allowed root pointing outside. It runs the focused scanner test and proves the outside entry is absent. The script verifies both temporary paths are under the process temp directory before cleanup and removes them in `finally`.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml apps::`

Expected: compile failure because the app modules do not exist.

- [ ] **Step 3: Implement bounded Start Menu discovery**

Scan only:

```text
%APPDATA%\Microsoft\Windows\Start Menu\Programs
%ProgramData%\Microsoft\Windows\Start Menu\Programs
```

Use `std::fs::read_dir` recursion, skip inaccessible entries, accept only `.lnk`, and never scan arbitrary drives. Before descending into any directory, read Windows attributes and reject symlinks plus every directory carrying `FILE_ATTRIBUTE_REPARSE_POINT`; do not resolve the target to decide whether it looks safe. Represent the resolved record as:

```rust
pub(crate) struct Application {
    pub(crate) app_id: String,
    pub(crate) display_name: String,
    pub(crate) shortcut: PathBuf,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) icon_png_base64: Option<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) use_count: u64,
}
```

Resolve `.lnk` metadata through `IShellLinkW` and `IPersistFile` from the `windows` crate. Failure to resolve an executable is allowed; the shortcut remains a valid launch entry and simply skips activation mapping.

Generate `app_id` exactly as specified in `Application identity and settings` with Windows CNG SHA-256. Detect duplicate IDs during a scan and fail the rescan instead of merging entries. The app ID may enter `AppAliasTarget` and trusted Rust actions, but shortcut and executable paths never enter a frontend DTO.

- [ ] **Step 4: Cache application metadata and icons without widening frontend access**

Write `application-cache.json` atomically in the application data directory and rebuild it on first launch or manual rescan. Extract a small PNG icon from the shell entry with `SHGetFileInfoW`, convert it through Windows Imaging Component, release every `HICON`, and store only base64 PNG bytes in the cache. Return `format!("data:image/png;base64,{encoded}")` or a bundled generic icon; never return an icon path, shortcut path, or executable path to TypeScript.

- [ ] **Step 5: Implement deterministic fuzzy scoring**

Normalize with Unicode lowercase without transliteration. Score in this order: exact, prefix, substring, subsequence. Break ties by alias match, then descending `use_count`, then display name. Return no more than 20 items. Do not add a fuzzy-search dependency.

- [ ] **Step 6: Run tests and clippy**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::
powershell -ExecutionPolicy Bypass -File scripts/test-start-menu-boundary.ps1
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all app tests pass; clippy exits 0.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri scripts/test-start-menu-boundary.ps1
git commit -m "feat: discover and rank Start Menu apps"
```

---

## Task 4: Persist Settings and Daily Validation Counts

**Files:**
- Create: `src-tauri/src/settings.rs`
- Create: `src-tauri/src/validation_data.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/protocol.ts`

- [ ] **Step 1: Write failing persistence tests**

Use a unique directory under `std::env::temp_dir()` and remove it at the end of each test. Prove:

- Missing settings produce `Alt+Space`, autostart disabled, and empty aliases.
- Save uses a sibling temporary file and atomic rename.
- Invalid current JSON is quarantined with a `.invalid` suffix and the last valid `.backup` file is loaded.
- Defaults are used only when neither the current nor backup file is valid.
- Validation counters never contain query text, application names, or paths.
- Two events on the same local date update one `DailyCounts`; the next date creates a second entry.
- Export has `schemaVersion: 1`, optional `researchId`, and date-keyed `dailyCounts` with every field in the cross-task contract.
- Three distinct active dates and per-day successful-action totals can be calculated from the exported structure.
- An unclosed prior session increments `hostCrashes` on the recorded session date; a graceful exit does not.
- Duplicate display names retain separate aliases through their distinct `appId` values.
- Saving an alias for an unknown `appId` is rejected.
- Clear resets all aggregates.
- Export opens a native save dialog only after the user clicks Export and accepts no destination path from TypeScript.

- [ ] **Step 2: Run tests and confirm failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml settings
cargo test --manifest-path src-tauri/Cargo.toml validation_data
```

Expected: compile failure because persistence modules do not exist.

- [ ] **Step 3: Implement two structured JSON files**

Store `settings.json` and `validation-data.json` under Tauri's application data directory. Persist through `create temp -> write -> sync_all -> preserve previous valid file as .backup -> rename`. Keep these settings only:

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
}
```

The aliases map key is `appId`; validate it against the current application cache before saving. `load_settings` joins stored aliases with the current cache and returns the `SettingsView` contract, so same-name applications remain independently editable.

Persist `VALIDATION_SCHEMA_VERSION` and the `BTreeMap<String, DailyCounts>` shape from the cross-task contract. Obtain the local date with `GetLocalTime`; do not store a timestamp. At startup, record the current date as an open session. On graceful tray exit clear it. If the next startup finds an open session, increment `hostCrashes` for that saved date before opening the new session. Do not record raw inputs, result titles, executable names, shortcut paths, or precise behavior times.

Implement export with Windows `IFileSaveDialog` inside the zero-argument Rust command. Write the aggregate JSON only after the user confirms the native dialog; return only success/cancel/error status to TypeScript. Do not grant dialog or filesystem capabilities to the WebView.

- [ ] **Step 4: Verify persistence behavior**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml settings
cargo test --manifest-path src-tauri/Cargo.toml validation_data
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: persistence tests and full Rust suite pass.

- [ ] **Step 5: Commit**

```powershell
git add src src-tauri
git commit -m "feat: persist settings and local validation counts"
```

---

## Task 5: Implement Search, Launch, and Best-Effort Activation Commands

**Files:**
- Create: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/apps/action.rs`
- Create: `src-tauri/src/apps/windows_backend.rs`
- Modify: `src-tauri/src/apps/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing behavior tests against a fake Windows backend**

Define this boundary so every decision branch is testable without opening real applications:

```rust
pub(crate) trait WindowsBackend {
    fn running_processes(&self, executable: &Path) -> Result<Vec<u32>, ActionError>;
    fn visible_windows(&self, process_id: u32) -> Result<Vec<WindowRef>, ActionError>;
    fn activate(&self, window: WindowRef) -> Result<bool, ActionError>;
    fn launch_shortcut(&self, shortcut: &Path) -> Result<(), ActionError>;
}
```

Test the exact policy:

- No executable mapping: launch shortcut.
- No running process: launch shortcut.
- Multiple matching processes: launch shortcut without activation.
- One process but no visible top-level window: launch shortcut.
- One process and multiple visible windows: activate the highest z-order window.
- Activation returns true: report activation success and do not launch.
- Activation returns false: launch shortcut and return the Windows-refusal warning.
- Activation API errors: return a categorized error; do not claim success.
- A cached shortcut that no longer exists keeps the launcher open and returns the rescan instruction.
- Unknown or stale result IDs never reach the backend.
- A command-level barrier makes query 1 finish after query 2; query 1 returns `null`, query 2 remains in the registry, and Enter executes query 2.
- Hiding during an in-flight query makes its publish return `null` and leaves the registry empty.
- `hide_launcher` clears the registry before attempting to hide; if hide fails it returns an error and the registry remains cleared.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml apps::action`

Expected: compile failure because action modules do not exist.

- [ ] **Step 3: Implement the policy and real Windows adapter**

Use native APIs through the `windows` crate:

- Process enumeration and executable comparison remain in Rust.
- `EnumWindows`, visibility checks, owner/process ID and z-order determine candidates.
- `SetForegroundWindow` is treated as a request whose Boolean result is authoritative for the branch, not proof that focus changed.
- `ShellExecuteW` opens the trusted `.lnk` path already stored in `ResultRegistry`.
- Never poll for 500 ms, kill processes, elevate, inject input, or accept a path from the WebView.

Return a serializable outcome:

```rust
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum ExecuteOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested { message: &'static str },
}
```

- [ ] **Step 4: Register narrow Tauri commands**

`search_apps(query, invocationId, querySequence)` calls `begin_query` before doing cache/ranking work and finishes through `publish_if_latest`; it returns `null` for an old invocation, superseded query or hidden generation. It never calls an unconditional publish. `execute_result(requestId, resultId)` resolves only through the registry, updates the current date according to the fixed event mapping and updates recent-use count only after a successful launch/activation request, then calls `clear_and_hide`; failures retain the window. `hide_launcher()` calls the same `clear_and_hide` function with no frontend-supplied target. `rescan_apps()` refreshes only the two Start Menu roots. Settings and validation commands call their Rust services.

Register exactly the commands already declared in `build.rs`. Do not add a generic `run`, `open`, `readFile`, `writeFile`, or `request` command.

- [ ] **Step 5: Verify all command branches**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
```

Expected: all tests pass; clippy and security check exit 0.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri
git commit -m "feat: launch and activate trusted app results"
```

---

## Task 6: Wire Lifecycle, Hotkey, Tray, and Autostart

**Files:**
- Create: `src-tauri/src/lifecycle.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Write failing lifecycle state tests**

Separate window-state decisions from Tauri handles and test:

- Global hotkey toggles hidden -> shown and emits a new `invocationId`.
- Repeated hotkey while visible focuses the existing window rather than creating another.
- Escape command, focus loss, and close request all route through `clear_and_hide`, not process exit.
- Every hide path clears the current `ResultRegistry` mapping.
- Single-instance activation reuses the main window.
- Invalid/conflicting configured hotkey preserves the previous registration and returns a visible error.
- Autostart remains disabled until explicitly enabled.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml lifecycle`

Expected: compile failure because lifecycle module does not exist.

- [ ] **Step 3: Implement one-window lifecycle**

Install the single-instance plugin first in the builder. Register `Alt+Space` through the global-shortcut plugin. On invocation, generate the opaque monotonically increasing `invocationId`, call `ResultRegistry::on_show(invocationId.clone())`, center the fixed-size window near the top of the current monitor, show it, focus it, and emit the same `invocationId` in `launcher://shown` with a Rust `Instant` measurement in test builds.

Create a tray menu with only `打开设置` and `退出`. Focus loss and window close call `clear_and_hide`; tray `退出` first marks the validation session clean, then terminates the process. Apply autostart changes only after the corresponding plugin call succeeds, then persist settings.

- [ ] **Step 4: Verify lifecycle logic and compile the desktop binary**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml lifecycle
npm run build
npm run tauri build -- --no-bundle
```

Expected: tests pass and a debug-independent executable is produced without installer bundling.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri
git commit -m "feat: add launcher lifecycle and global hotkey"
```

---

## Task 7: Build the Keyboard-First Launcher UI

**Files:**
- Create: `src/launcher.ts`
- Create: `src/launcher.test.ts`
- Modify: `src/main.ts`
- Modify: `src/styles.css`
- Modify: `index.html`
- Modify: `src/protocol.ts`

- [ ] **Step 1: Write failing DOM tests**

Use Vitest with jsdom and an injected command client. Cover:

- Input focuses and selects its content after `launcher://shown`.
- Empty input clears results without invoking Rust.
- Normal text increments `querySequence` and invokes only `search_apps(query, invocationId, querySequence)`.
- `/find` displays `文件搜索尚未启用` and invokes no file or generic command in this plan.
- Any other slash-prefixed command displays `未知命令` and never falls through to app search.
- ArrowUp/ArrowDown wrap within the fixed result set.
- Enter sends only current `requestId` and selected `resultId`.
- Escape invokes only `hide_launcher`; no TypeScript code calls a direct window hide API.
- Successful execution relies on Rust to clear and hide; failed execution keeps the window open and announces the error.
- A late response for an older query and a Rust `null` superseded response are ignored.
- Each `launcher://shown` event resets `querySequence` to 0 before accepting input.
- Results use `role="listbox"`, items use `role="option"`, and `aria-activedescendant` tracks selection.
- Errors update a `role="status"` live region and are not conveyed by color alone.
- The input uses combobox semantics and points `aria-controls` at the listbox.
- Icon-only controls have an accessible name and tooltip.
- The layout follows `prefers-color-scheme` and does not expose a custom theme setting.

- [ ] **Step 2: Run tests and confirm failure**

Run: `npm test`

Expected: tests fail because launcher state and DOM behavior are not implemented.

- [ ] **Step 3: Implement a small state controller**

Keep state explicit and framework-free:

```ts
export interface LauncherState {
  query: string
  invocationId?: string
  requestId?: string
  items: ResultItem[]
  selectedIndex: number
  pendingSequence: number
  status: string
}
```

Use the monotonically increasing frontend `pendingSequence` both as `querySequence` sent to Rust and as a final UI stale-response guard. Render text with `textContent`, never `innerHTML`. Use a generic local application icon when `icon` is absent. Do not load remote images or expose local paths.

- [ ] **Step 4: Implement settings in the same WebView**

Use an unframed settings view, not a second WebView. Include research ID, hotkey input, autostart toggle, aliases editor keyed by `AppAliasTarget.appId`, rescan button, validation export/clear buttons, and a back control. A DOM test with duplicate display names must edit one app without changing the other's aliases. Display persistence/plugin errors inline and in the live region.

- [ ] **Step 5: Verify UI behavior**

Run:

```powershell
npm test
npm run build
```

Expected: all DOM tests pass; production frontend build succeeds.

- [ ] **Step 6: Commit**

```powershell
git add index.html src
git commit -m "feat: add keyboard-first launcher interface"
```

---

## Task 8: Add Performance Boundaries and Complete Launcher Verification

**Files:**
- Create: `src-tauri/src/performance.rs`
- Create: `scripts/smoke-launcher.ps1`
- Create: `scripts/benchmark-launcher.ps1`
- Modify: `src-tauri/src/lifecycle.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/main.ts`
- Create: `README.md`

- [ ] **Step 1: Write failing timing-domain tests**

Prove the emitted records use suffixed names and never subtract timestamps from different domains. The launcher-only events in this plan are:

```text
shortcut_sent_external
input_focus_observed_external
shortcut_received_rust
show_event_emitted_rust
show_event_received_ui
input_interactive_ui
query_input_ui
app_results_committed_ui
```

Each record contains an `invocationId` or frontend query sequence, event name, elapsed value in its own process, build mode, OS build, CPU, memory, and WebView2 version where available. Production builds keep aggregate validation counts but do not write performance traces. Only `_external` timestamps are subtracted from `_external`, `_rust` from `_rust`, and `_ui` from `_ui`; launcher end-to-end and application-search P95 are calculated exactly from the pairs frozen in section 10 of the spec.

- [ ] **Step 2: Run the timing tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml performance`

Expected: compile failure because the performance module does not exist.

- [ ] **Step 3: Implement test-build-only traces and a manual smoke script**

`scripts/smoke-launcher.ps1` must verify that the built executable starts once, a second process exits, and the first process remains alive. It must not launch arbitrary applications automatically; application activation remains a manual checklist because it changes desktop state.

Document these manual Windows 11 checks in `README.md`:

1. `Alt+Space` opens and focuses one launcher window.
2. `企业` and `微信` find installed matching Start Menu entries when present.
3. Arrow keys select; Enter launches an unopened app.
4. A uniquely mapped running desktop app takes the activation branch.
5. An ambiguous mapping takes the launch branch.
6. Escape and focus loss hide the launcher.
7. Tray settings and exit work.
8. Visible focus, list selection, live error status, and a Windows Narrator smoke pass are recorded.
9. No network request appears during ordinary use.

`scripts/benchmark-launcher.ps1` must use Windows UI Automation and one external `Stopwatch` clock for the hot-launch measurement. Run 5 warmups followed by 200 measured `Alt+Space` invocations and assert `shortcut_sent_external -> input_focus_observed_external` P95 is at most 1 second. In a `test-instrumentation` build with a deterministic 500-application in-memory cache, run 1,000 fixed queries through the actual input/DOM path and assert `query_input_ui -> app_results_committed_ui` P95 is at most 100 ms. The report must include all reference-environment fields required by section 10.1 and must not contain query text or application names.

- [ ] **Step 4: Run the complete automated gate**

Run:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
npm run tauri build -- --no-bundle
npm run tauri build -- --no-bundle --features test-instrumentation
powershell -ExecutionPolicy Bypass -File scripts/smoke-launcher.ps1
powershell -ExecutionPolicy Bypass -File scripts/benchmark-launcher.ps1
git diff --check
```

Expected: every command exits 0. Manual checks are recorded with Windows build, WebView2 version, application used, result, and allowed-difference classification.

The smoke script must also launch the test-only `security-probe` WebView and assert its `load_settings` invocation is rejected. This assertion is required even though the static capability check already limits the window label.

- [ ] **Step 5: Review against the frozen scope**

Confirm all statements are true before marking the plan complete:

- There is one local-only WebView and one trusted action registry.
- No command accepts a file path, executable path, URL, shell fragment, or arbitrary payload from TypeScript.
- App discovery stays within the two Start Menu roots.
- Activation ambiguity and refusal follow the frozen fallback policy.
- `/find` is visibly gated and has no production backend.
- No translation, network, plugin, macOS, signing, or pilot code was introduced.
- Keyboard and Narrator smoke results are recorded.
- Windows 11 launcher and app-search performance sample counts and P95 values are recorded with one clock domain per subtraction.

- [ ] **Step 6: Commit**

```powershell
git add README.md scripts src src-tauri
git commit -m "test: verify Windows launcher foundation"
```

---

## Completion Gate

This plan is complete only when Tasks 1-8 pass on Windows 11 x64 and the launcher-only acceptance evidence is recorded. The next production planning decision is determined by the SystemIndex Spike:

- Spike Go: write a separate `/find` implementation plan, then a release/signing plan.
- Spike No-Go: keep `/find` disabled, return to architecture review, and do not claim MVP-A completion.
