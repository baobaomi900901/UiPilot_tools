# Windows Launcher Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一个可运行、可测试的 Windows 11 x64 启动器基础版本，支持全局唤起、应用搜索、启动、尽力激活、基础设置、本地验证计数和键盘/辅助技术操作。

**Architecture:** 使用单一 Tauri 2 主窗口。Vanilla TypeScript 只负责输入、列表和设置界面；所有系统能力、可信路径、结果注册表和持久化都留在 Rust。前端只能回传当前结果集中的 `requestId` 与 `resultId`，Rust 再解析真实动作。应用发现只扫描 Windows 开始菜单入口，不引入数据库、前端框架或远程内容。

**Tech Stack:** Tauri 2、Vanilla TypeScript、Vite、Vitest、Rust、`windows` crate、Tauri single-instance/global-shortcut/autostart 插件。

**Source Spec:** `docs/superpowers/specs/2026-07-17-cross-platform-launcher-mvp-design.md`

**Scope Boundary:** 本计划不实现文件搜索、翻译、macOS、第三方插件、安装包签名或试用研究。本计划完成表示“启动器与应用能力可验收”，不表示完整 MVP-A 已完成。

---

## Global Constraints

- 每个任务开始前必须重读本节与 `Cross-Task Interfaces`；不得在单个任务内另造不兼容合同。
- 只允许一个本地 `main` WebView。生产构建不包含测试探针、远程页面、动态 HTML、任意网络、Shell、通用文件系统或通用进程能力。
- TypeScript 不接收或回传快捷方式路径、可执行文件路径或任意动作 payload；动作只能通过当前 Rust 注册表中的 `requestId + resultId` 解析。
- 所有隐藏路径都必须进入 Rust 的同一个 `clear_and_hide` 函数；前端没有直接隐藏窗口的 capability。
- 生产应用根只能通过 `SHGetKnownFolderPath` 查询 `FOLDERID_Programs` 与 `FOLDERID_CommonPrograms`，固定使用 `KF_FLAG_DONT_VERIFY` 和 `hToken = NULL`；不接受环境变量、配置、命令行或前端路径。测试只能向内部扫描函数注入临时根。根 reparse point 是扫描级错误；任何 reparse 子项均跳过且不跟随。
- 快捷方式和可执行文件的可信快照只驻留进程内并在启动时重建，不写入应用数据目录；磁盘只保存结构化设置、`appId -> use_count` 和验证计数。
- 只保存日期级验证计数，不保存精确行为时间、查询词、应用名称、快捷方式、可执行文件或文件路径。
- `test-instrumentation` 构建不得查询或修改正式 identifier 的 app-data，不得 load/manage settings/validation store 或创建 session marker；安全探针 smoke 必须比较运行前后的受保护文件快照。
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

`appId` 使用固定版本算法：对 `start-menu-v1\0 + rootKind + \0 + normalizedRelativeShortcutPath` 的 UTF-8 字节做 Windows CNG SHA-256，输出 `app-` 加 64 位小写十六进制。`rootKind` 固定为 `user` 或 `common`；相对路径以反斜杠连接、保留 `.lnk` 并做 Unicode `to_lowercase`，不得使用有损编码。它在重复扫描和仅大小写重命名间稳定、区分同名入口且不暴露路径；非大小写重命名、改变相对目录或切换根才产生新 ID。

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

Task 5 后续提供的 `load_settings` 返回 `SettingsView`；`save_settings` 只接受已存在 `appId` 对应的别名，不接受显示名称或路径作为键。Task 4A 只实现 crate-private store，不创建这些 command 或前端类型。

整个进程只创建一个 Tauri 托管的 `Arc<AppCache>`。Task 3 启动线程、Task 4 设置服务和 Task 5 命令都使用该实例。`settings.json` 中的 `useCounts` 只含符合固定格式的 `appId` 和 `u64`；搜索时将别名和计数装饰到内存快照的克隆，绝不把设置文件中的数据提升为路径或动作。

### Validation export

Task 4 持久化拆为三个顺序交付物；各自只执行已批准的独立计划：

- `docs/superpowers/plans/2026-07-18-task-4a-settings-persistence.md`
- `docs/superpowers/plans/2026-07-18-task-4b-validation-store.md`
- `docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md`

`SettingsStore` 和 `ValidationStore` 各自只有一个进程所有者和一个事务 mutex；成功替换 current 后才交换内存并把 `current_is_valid` 设为 true。research ID 保持可选，但非空值只能是 1 到 64 个 ASCII 字母、数字、`-`、`_`；导出缺少 ID 时固定失败且不创建文件。

验证文件只包含 schema version、日期级 `DailyCounts` 和内部 `lastReconciledSessionId`。`DailyCounts` 固定包含 `launcherInvocations`、`applicationLaunchRequests`、`activationSuccesses`、`activationRefusals`、`uncleanSessions`；不存在 `hostCrashes`。MVP-A 不安装 `SetUnhandledExceptionFilter` 或 confirmed-crash marker。

日期键固定为本地 `YYYY-MM-DD`，通过已使用的 `windows` crate 调用 `GetLocalTime` 获得，不增加日期依赖。活跃日由 `launcherInvocations > 0` 判定；该日成功动作等于 `applicationLaunchRequests + activationSuccesses`，只验证纯应用启动/激活使用频率。

事件映射固定为：窗口唤起增加 `launcherInvocations`；`LaunchRequested` 增加 `applicationLaunchRequests`；`ActivationRequested` 增加 `activationSuccesses`；`ActivationRefusedLaunchRequested` 同时增加 `activationRefusals` 与 `applicationLaunchRequests`。失败动作不增加成功动作字段。

启动时先以持久化 `lastReconciledSessionId` 对账旧 open-session marker，再原子替换为新 marker并把同一 ID 保存到内存。重复 open 失败；`record` 在 session 未打开时失败；clean exit 只删除与本进程内存 ID 匹配的 marker。`uncleanSessions` 不能自动归类为宿主崩溃。

Task 4C 的 native chooser 返回字段私有的 `ExportDestination`。Task 5 后续 async command 只在 main UI thread 显示 dialog；cancel 在 worker 派发前返回，确认后才把 destination 交给 `spawn_blocking` 完成 snapshot、序列化、`sync_all` 和原子替换。Task 4C 不修改 `src/protocol.ts` 或 command registry。

### Security probe build

安全探针通常只有一个构建入口：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-security-probe.ps1
```

该脚本内部只能执行以下 Tauri 构建命令：

```powershell
npm run tauri build -- --no-bundle --features test-instrumentation --config src-tauri/tauri.security-probe.conf.json
```

override config 把 `build.beforeBuildCommand` 固定为 `npm run build -- --mode security-probe`，因此同一次 Tauri 构建同时启用 Vite probe mode 和 Cargo `test-instrumentation` feature。脚本把 npm/Tauri 日志发送到 host，成功时 stdout 最后一行只输出探针可执行文件的绝对路径。任何测试或 smoke 都不得直接运行裸 `--features test-instrumentation` 构建。

唯一例外是 Task 1A-R 的恢复环境可靠性门禁：该环境已实证上述脚本中的
`Start-Process npm.cmd -Wait` 包装层会挂起而没有构建子进程。Task 1A-R 不修改脚本，
而是在当前 PowerShell 测试进程内把 `Path`/`PATH` 规范化为单一 `Path` 键后，直接调用
同一个 `npm.cmd run tauri build -- --no-bundle --features test-instrumentation --config
src-tauri/tauri.security-probe.conf.json`。该例外仍使用同一 override config，不是裸 feature
构建；每次必须认证原始退出码、Cargo metadata 定位的产物、UTC 时间和 SHA-256。

Task 4A 扩展 `test-security-probe.ps1`：probe 启动前后比较正式 app-data 下 `settings.json*`、`validation-data.json*`、`open-session.json*` 的名称、长度、SHA-256 和最后写入 ticks。任何 current/backup/temp/invalid 文件新增、删除或变化都使 smoke 失败。4A/4B 的 app-data 查询、store load/manage 和 session open 只能存在于 `#[cfg(not(feature = "test-instrumentation"))]` setup 分支。

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
- Create: `src-tauri/tauri.security-probe.conf.json`
- Create: `src-tauri/capabilities/main.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/security_probe.rs`
- Create: `security-probe.html`
- Create: `src/security-probe.ts`
- Create: `scripts/check-security-config.ps1`
- Create: `scripts/test-security-config.ps1`
- Create: `scripts/build-security-probe.ps1`
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
$probeConfig = Get-Content "$PSScriptRoot/../src-tauri/tauri.security-probe.conf.json" -Raw | ConvertFrom-Json
$capabilityDirectory = "$PSScriptRoot/../src-tauri/capabilities"
$capabilityFiles = @(
  Get-ChildItem $capabilityDirectory -Recurse -File |
    Where-Object { $_.Extension -in @('.json', '.json5', '.toml') }
)
$expectedCapability = [IO.Path]::GetFullPath((Join-Path $capabilityDirectory 'main.json'))
if (
  $capabilityFiles.Count -ne 1 -or
  -not [string]::Equals($capabilityFiles[0].FullName, $expectedCapability, [StringComparison]::OrdinalIgnoreCase)
) {
  throw 'Exactly one main capability file is allowed'
}
$capability = Get-Content $capabilityFiles[0].FullName -Raw | ConvertFrom-Json

if ($config.app.security.csp -ne "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'") {
  throw 'Unexpected CSP'
}
if ($config.app.security.PSObject.Properties['capabilities']) {
  throw 'Inline or explicitly enabled capabilities are not allowed'
}
$expectedProbeProperties = @('$schema', 'build') | Sort-Object
$actualProbeProperties = @($probeConfig.PSObject.Properties.Name) | Sort-Object
if (Compare-Object $expectedProbeProperties $actualProbeProperties) {
  throw 'Security probe override contains unexpected configuration'
}
$probeBuildProperties = @($probeConfig.build.PSObject.Properties.Name)
if (
  $probeBuildProperties.Count -ne 1 -or
  $probeBuildProperties[0] -ne 'beforeBuildCommand' -or
  $probeConfig.build.beforeBuildCommand -ne 'npm run build -- --mode security-probe'
) {
  throw 'Unexpected security probe build configuration'
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

Give `main` exactly the permission set in `Capability allowlist`; there is no frontend window-hide permission. `vite.config.ts` includes `security-probe.html` only in `security-probe` mode, and `security_probe.rs` creates its `security-probe` WebView only under Cargo feature `test-instrumentation`. `tauri.security-probe.conf.json` and `build-security-probe.ps1` implement the single `Security probe build` contract. `scripts/test-security-probe.ps1 -Executable <absolute-path>` never builds; it launches exactly the supplied artifact and asserts `load_settings` is rejected for the non-main label.

- [ ] **Step 5: Add a boot-only Rust and frontend entry point**

`src-tauri/src/main.rs` must call `uipilot_lib::run()`. `src-tauri/src/lib.rs` must initially build a Tauri app with no business commands. `src/main.ts` must render one text input with label `搜索应用` and no framework runtime.

- [ ] **Step 6: Verify the shell**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
npm run build
if (Test-Path dist/security-probe.html) { throw 'Production frontend contains security probe' }
cargo check --manifest-path src-tauri/Cargo.toml
$probeExe = & .\scripts\build-security-probe.ps1 | Select-Object -Last 1
if (-not (Test-Path -LiteralPath $probeExe)) { throw 'Probe executable was not produced' }
& .\scripts\test-security-probe.ps1 -Executable $probeExe
```

Expected: `security config ok`; regression fixtures reject nested JSON/TOML/JSON5 capabilities plus capability changes in the base and probe override configs; normal Vite output omits the probe; Cargo exits 0; the unique probe build produces an executable whose non-main command call is rejected.

- [ ] **Step 7: Commit**

```powershell
git add package.json package-lock.json tsconfig.json vite.config.ts index.html src scripts src-tauri
git commit -m "build: scaffold secure Tauri launcher"
```

---

## Task 1A: Close Security Gate Regression Gaps

**Files:**
- Modify: `scripts/check-security-config.ps1`
- Modify: `scripts/test-security-config.ps1`
- Modify: `scripts/test-security-probe.ps1`
- Modify: `src/security-probe.ts`
- Modify: `src-tauri/src/security_probe.rs`
- Modify: `docs/superpowers/specs/2026-07-18-security-gate-hardening-design.md`

**Interfaces:**
- Consumes: the exact Task 1 production configuration and feature-only security probe build.
- Produces: a configuration gate that rejects every unreviewed property or value, plus a probe where only the exact ACL denial exits with code `73`.

- [ ] **Step 1: Add the three failing configuration counterexamples and missing JSON fixture**

Extend `scripts/test-security-config.ps1` by restoring the canonical fixture before each mutation and asserting non-zero exit for these cases. Add the JSON fixture beside the existing JSON5/TOML fixtures so every supported capability extension is exercised:

```powershell
$nestedDirectory = New-Item -ItemType Directory -Path "$fixtureRoot/src-tauri/capabilities/nested" -Force
'{"identifier":"json-extra","windows":["main"],"permissions":["core:default"]}' |
  Set-Content -Encoding utf8 "$nestedDirectory/extra.json"
if ((Invoke-FixtureCheck).ExitCode -eq 0) { throw 'Nested JSON capability was not rejected' }
Remove-Item -LiteralPath "$nestedDirectory/extra.json"

$configPath = "$fixtureRoot/src-tauri/tauri.conf.json"
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$config.app.withGlobalTauri = $true
$config | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 $configPath
if ((Invoke-FixtureCheck).ExitCode -eq 0) { throw 'withGlobalTauri true was not rejected' }

Copy-Item "$repoRoot/src-tauri/tauri.conf.json" $configPath -Force
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$config.app.windows[0] | Add-Member -NotePropertyName url -NotePropertyValue 'https://example.com'
$config | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 $configPath
if ((Invoke-FixtureCheck).ExitCode -eq 0) { throw 'Remote window URL was not rejected' }

Copy-Item "$repoRoot/src-tauri/tauri.conf.json" $configPath -Force
$capabilityPath = "$fixtureRoot/src-tauri/capabilities/main.json"
$capability = Get-Content $capabilityPath -Raw | ConvertFrom-Json
$capability | Add-Member -NotePropertyName remote -NotePropertyValue ([pscustomobject]@{})
$capability | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 $capabilityPath
if ((Invoke-FixtureCheck).ExitCode -eq 0) { throw 'Remote capability was not rejected' }
```

- [ ] **Step 2: Run the configuration regression and confirm the first new counterexample fails**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1`

Expected: non-zero exit ending with `withGlobalTauri true was not rejected`.

- [ ] **Step 3: Replace partial checks with one exact object-property contract**

Add this shared helper to `scripts/check-security-config.ps1`:

```powershell
function Assert-ExactProperties {
  param(
    [Parameter(Mandatory)] [object] $Object,
    [Parameter(Mandatory)] [string[]] $Names,
    [Parameter(Mandatory)] [string] $Label
  )

  $actual = @($Object.PSObject.Properties.Name) | Sort-Object
  $expected = @($Names) | Sort-Object
  if (Compare-Object $expected $actual) {
    throw "$Label properties differ from the exact allowlist"
  }
}
```

Apply it to every configuration object with these exact property sets:

```powershell
Assert-ExactProperties $config @('$schema', 'productName', 'version', 'identifier', 'build', 'app', 'bundle') 'Tauri root'
Assert-ExactProperties $config.build @('frontendDist', 'devUrl', 'beforeDevCommand', 'beforeBuildCommand') 'Tauri build'
Assert-ExactProperties $config.app @('withGlobalTauri', 'windows', 'security') 'Tauri app'
Assert-ExactProperties $config.app.windows[0] @('label', 'title', 'width', 'height', 'visible', 'decorations', 'resizable', 'fullscreen') 'Main window'
Assert-ExactProperties $config.app.security @('csp') 'Tauri security'
Assert-ExactProperties $config.bundle @('active', 'targets', 'icon', 'android') 'Tauri bundle'
Assert-ExactProperties $config.bundle.android @('debugApplicationIdSuffix') 'Tauri Android bundle'
Assert-ExactProperties $capability @('$schema', 'identifier', 'description', 'windows', 'permissions') 'Main capability'
Assert-ExactProperties $probeConfig @('$schema', 'build') 'Security probe override'
Assert-ExactProperties $probeConfig.build @('beforeBuildCommand') 'Security probe build'
```

Keep the exact capability file-set and permission-set checks, then add these exact value guards:

```powershell
$expectedCsp = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'"
if (
  $config.'$schema' -ne '../node_modules/@tauri-apps/cli/config.schema.json' -or
  $config.productName -ne 'UiPilot' -or
  $config.version -ne '0.1.0' -or
  $config.identifier -ne 'com.uipilot.launcher' -or
  $config.build.frontendDist -ne '../dist' -or
  $config.build.devUrl -ne 'http://localhost:1420' -or
  $config.build.beforeDevCommand -ne 'npm run dev' -or
  $config.build.beforeBuildCommand -ne 'npm run build' -or
  $config.app.withGlobalTauri -ne $false -or
  $config.app.windows.Count -ne 1 -or
  $config.app.security.csp -ne $expectedCsp -or
  $config.bundle.active -ne $true -or
  $config.bundle.targets -ne 'all' -or
  $config.bundle.android.debugApplicationIdSuffix -ne '.debug'
) {
  throw 'Tauri configuration values differ from the exact allowlist'
}

$window = $config.app.windows[0]
if (
  $window.label -ne 'main' -or
  $window.title -ne 'UiPilot' -or
  $window.width -ne 720 -or
  $window.height -ne 420 -or
  $window.visible -ne $false -or
  $window.decorations -ne $false -or
  $window.resizable -ne $false -or
  $window.fullscreen -ne $false
) {
  throw 'Main window values differ from the exact allowlist'
}

$expectedIcons = @(
  'icons/32x32.png',
  'icons/128x128.png',
  'icons/128x128@2x.png',
  'icons/icon.icns',
  'icons/icon.ico'
)
if (Compare-Object $expectedIcons @($config.bundle.icon)) {
  throw 'Bundle icons differ from the exact allowlist'
}

if (
  $capability.'$schema' -ne '../gen/schemas/desktop-schema.json' -or
  $capability.identifier -ne 'main-capability' -or
  $capability.description -ne 'Exact permissions for the local launcher WebView' -or
  $capability.windows.Count -ne 1 -or
  $capability.windows[0] -ne 'main' -or
  $probeConfig.'$schema' -ne '../node_modules/@tauri-apps/cli/config.schema.json' -or
  $probeConfig.build.beforeBuildCommand -ne 'npm run build -- --mode security-probe'
) {
  throw 'Capability or probe values differ from the exact allowlist'
}
```

Do not add schema interpretation, JSON5 parsing, or a generic configuration framework.

- [ ] **Step 4: Run the complete configuration gate**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
```

Expected: `security config ok` and `security config regression tests ok`; all eight malicious fixtures are internally asserted non-zero.

- [ ] **Step 5: Demonstrate that a normal executable currently passes the probe oracle**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-probe.ps1 -Executable "$env:SystemRoot\System32\hostname.exe"
if ($LASTEXITCODE -ne 0) { throw 'Expected the old probe oracle counterexample to pass before the fix' }
```

Expected before the fix: `security probe ok` and exit code `0`.

- [ ] **Step 6: Make exact ACL denial the only probe success state**

Replace `src/security-probe.ts` with:

```ts
import { invoke } from '@tauri-apps/api/core'

invoke('load_settings').catch((error: unknown) => {
  if (String(error) === 'Command load_settings not allowed by ACL') {
    window.location.hash = 'acl-denied'
  }
})
```

Replace `src-tauri/src/security_probe.rs` with:

```rust
use std::{thread, time::Duration};

use tauri::{App, WebviewUrl, WebviewWindowBuilder};

const ACL_DENIED_EXIT_CODE: i32 = 73;

#[tauri::command]
pub(crate) fn load_settings() -> &'static str {
    "unexpectedly allowed"
}

pub(crate) fn setup(app: &mut App) -> tauri::Result<()> {
    let probe = WebviewWindowBuilder::new(
        app,
        "security-probe",
        WebviewUrl::App("security-probe.html".into()),
    )
    .title("UiPilot security probe")
    .visible(false)
    .build()?;

    thread::spawn(move || {
        for _ in 0..200 {
            let result = probe
                .url()
                .ok()
                .and_then(|url| url.fragment().map(str::to_owned));

            if result.as_deref() == Some("acl-denied") {
                std::process::exit(ACL_DENIED_EXIT_CODE);
            }

            thread::sleep(Duration::from_millis(50));
        }

        std::process::exit(3);
    });

    Ok(())
}
```

Use `std::process::exit` only in this feature-only probe. Tauri/Wry's Windows event-loop exit path does not preserve the requested numeric exit code, so `AppHandle::exit(73)` cannot satisfy this test contract.

Replace `scripts/test-security-probe.ps1` with:

```powershell
[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string] $Executable
)

$ErrorActionPreference = 'Stop'
$executableUri = $null
if (
  -not [Uri]::TryCreate($Executable, [UriKind]::Absolute, [ref] $executableUri) -or
  -not $executableUri.IsFile
) {
  throw 'Executable must be an absolute path'
}
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$process = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden

try {
  if (-not $process.WaitForExit(15000)) {
    throw 'Security probe timed out'
  }
  if ($process.ExitCode -ne 73) {
    throw "Security probe expected ACL denial exit code 73, got $($process.ExitCode)"
  }
}
finally {
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
  }
}

Write-Output 'security probe ok'
```

- [ ] **Step 7: Run the probe negative and positive gates**

Run:

```powershell
$ErrorActionPreference = 'Continue'
$negativeOutput = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-probe.ps1 -Executable "$env:SystemRoot\System32\hostname.exe" 2>&1
$negativeExit = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($negativeExit -eq 0) { throw "Arbitrary executable passed probe gate: $negativeOutput" }

$probeExe = & .\scripts\build-security-probe.ps1 | Select-Object -Last 1
if (-not (Test-Path -LiteralPath $probeExe)) { throw 'Probe executable was not produced' }
& .\scripts\test-security-probe.ps1 -Executable $probeExe
```

Expected: `hostname.exe` is internally asserted non-zero; the feature-only probe exits `73`, and its wrapper prints `security probe ok` with exit code `0`.

- [ ] **Step 8: Run Task 1's complete gate and commit**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
npm run build
if (Test-Path dist/security-probe.html) { throw 'Production frontend contains security probe' }
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

Expected: every command exits `0`; production output contains no probe page.

```powershell
git add docs/superpowers/specs/2026-07-18-security-gate-hardening-design.md docs/superpowers/plans/2026-07-17-windows-launcher-foundation.md scripts/check-security-config.ps1 scripts/test-security-config.ps1 scripts/test-security-probe.ps1 src/security-probe.ts src-tauri/src/security_probe.rs
git commit -m "fix: close security gate bypasses"
```

### Task 1A-R: Stabilize the Security Probe Result Channel

**Status:** direct-eval 设计 `0f5ddcc` 已批准；本实施计划待书面复审。计划 Go 前不得
创建执行基线 tag 或修改实现。

**Design:**
`docs/superpowers/specs/2026-07-18-security-probe-result-channel-reliability-design.md`

**Goal:** 窗口 build 后直接用串行 `eval_with_callback` 同时观察 DOM readiness 与 ACL
结果，建立可分类、冷启动稳定的 feature-only 探针结果通道，同时保持只有精确 ACL
拒绝可退出 `73`。

**Architecture:** 分两个独立审核检查点执行。阶段一保留一次显式 legacy URL 对照，
用 raw 退出码证明 DOM eval 候选可行；阶段二经审核授权后删除 runtime URL 查询，
只以严格 DOM 观察映射 `73`。两个阶段共享严格 parser 和串行 callback loop，但使用
不同的 phase-specific classifier。

**Files:**

- Modify and test: `src-tauri/src/security_probe.rs`
- Modify: `src/security-probe.ts`
- Unmodified legacy helper, not called by Task 1A-R: `scripts/build-security-probe.ps1`
- Read-only gate: `scripts/test-security-probe.ps1`
- Read-only gate: `scripts/check-security-config.ps1`
- Read-only gate: `scripts/test-security-config.ps1`

**Exact implementation allowlist:**

```text
src/security-probe.ts
src-tauri/src/security_probe.rs
```

No implementation commit may modify `Cargo.toml`, `package.json`, lockfiles, scripts,
Tauri configuration, capability files, production modules, Task 4 files or any other path.
The approved design and this plan are committed before the implementation baseline tag and are
therefore not part of the implementation diff.

**Phase-specific interfaces:**

R1 introduces only the following exact private diagnostic types and signatures. It must not
declare or implement `ExactAclDenied` or `classify_final`. Equivalent naming or a single
phase-switching classifier is not allowed because it would reopen the approved contract.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum DocumentReadyState {
    Loading,
    Interactive,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeState {
    InvokePending,
    InvokeSucceeded,
    AclDenied,
    InvokeOtherError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EvalError {
    ScriptFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProbeObservation {
    document_ready_state: DocumentReadyState,
    probe_state: Option<ProbeState>,
    hash: String,
    eval_error: Option<EvalError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LegacyUrlResult {
    Matched,
    Missed,
    QueryFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntermediateState {
    DocumentNotReady,
    ScriptNotStarted,
    InvokePending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeOutcome {
    LegacyUrlChannelMatched,
    DocumentNotReady,
    ProbeScriptNotStarted,
    InvokeTimeout,
    InvokeUnexpectedlyAllowed,
    InvokeOtherError,
    LegacyUrlChannelMiss,
    LegacyUrlQueryFailed,
    EvalDispatchFailed,
    EvalCallbackTimeout,
    InvalidEvalObservation,
    EvalScriptFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeDecision {
    Retry(IntermediateState),
    Exit(ProbeOutcome),
}

fn parse_observation(raw: &str) -> Result<ProbeObservation, ProbeOutcome>;

fn classify_diagnostic(
    observation: &ProbeObservation,
    legacy_url_result: Option<LegacyUrlResult>,
) -> ProbeDecision;

fn exit_code(outcome: ProbeOutcome) -> i32;
```

In R1, `exit_code` maps `LegacyUrlChannelMatched` to `73`; the remaining variants map in order
to `74` through `84` exactly as the approved design. R2.1 first adds RED references to a new
`ProbeOutcome::ExactAclDenied` variant and this new signature:

```rust
fn classify_final(observation: &ProbeObservation) -> ProbeDecision;
```

R2.3 implements both additions and maps `ExactAclDenied` to `73`. R1 must therefore pass
all-features Clippy without a future-only private variant or temporary `allow(dead_code)`.
After R2, the runtime never calls `classify_diagnostic` or `WebviewWindow::url()`.
The diagnostic classifier, `LegacyUrlResult`, the three legacy-only outcome variants and their
three `exit_code` match arms may remain only under `#[cfg(test)]`; no legacy URL code or symbol may
compile into the feature-only runtime. `allow(dead_code)` is forbidden in this module.

The browser callback returns exactly this JSON object shape:

```ts
interface ProbeObservation {
  documentReadyState: 'loading' | 'interactive' | 'complete'
  probeState:
    | 'invoke-pending'
    | 'invoke-succeeded'
    | 'acl-denied'
    | 'invoke-other-error'
    | null
  hash: string
  evalError: 'script-failed' | null
}
```

**Global constraints for Task 1A-R:**

- Do not add a command, capability, config override, dependency, result file, network path,
  environment-variable protocol, nonce, `data_directory` or production code path.
- `security-probe.ts` sets `window.location.hash = 'acl-denied'` only for the exact error
  `Command load_settings not allowed by ACL`.
- Runtime source must not use `on_page_load`, `PageLoadEvent`, a page-load channel/guard or a
  `page_load_finished` predicate. One worker owns the only 10-second monotonic deadline and the
  process exit decision immediately after the window build.
- Before any callback has returned, a synchronous eval dispatch `Err` is a startup intermediate
  and may retry after a deadline-bounded 50ms delay. After any callback has returned, a synchronous
  dispatch `Err` is terminal `81`.
- A successful dispatch creates one in-flight callback and waits only the post-dispatch remaining
  total budget. There is no independent callback timeout. While that callback is absent, dispatch
  count remains exactly `1`; reaching the total deadline is terminal `82`.
- After callback receive, close the in-flight request and re-read the same monotonic deadline
  before parsing. Recheck it after parse and before classifier/legacy lookup, then once more before
  accepting any `73`, `79` or `80` outcome. If any of these gates finds the deadline exhausted,
  return terminal `82`; a late exact ACL observation cannot become an accepted diagnostic result.
- A new eval may dispatch after 50ms only when the previous callback returned, was marked
  completed, and classified as `DocumentNotReady`, `ProbeScriptNotStarted` or `InvokePending`;
  that delay is also capped by the remaining total budget and cannot cause another dispatch after
  the deadline.
- The outer PowerShell timeout remains 15 seconds. Only `std::process::exit` is used for the
  feature-only probe's exact numeric exit.
- No Vitest file or dependency is added. The TypeScript state transition is exercised by the
  fresh native diagnostic; pure classification and callback ownership are Rust unit tests.
- The failed R1 evidence worktree at
  `D:\code\UiPilot_tools\.worktrees\security-probe-reliability` remains untouched, unstaged and
  uncommitted. Direct-eval implementation uses a new branch/worktree created from the new approved
  plan tag.

#### Approval Baseline

- [ ] **Step R0: After this plan receives Go, create the immutable execution baseline before RED**

Run:

```powershell
$approved = git rev-parse HEAD
git tag foundation-security-probe-direct-eval-approved-plan $approved
if ((git rev-parse foundation-security-probe-direct-eval-approved-plan) -ne $approved) {
  throw 'direct-eval approved-plan tag mismatch'
}
if ((git rev-parse foundation-security-probe-reliability-approved-plan) -ne `
    '157ad898c64abccfe26d1c76692011ea559b819e') {
  throw 'historical reliability approved-plan tag moved'
}
if (git status --porcelain) { throw 'worktree must be clean before RED' }

$implementationWorktree = 'D:\code\UiPilot_tools\.worktrees\security-probe-direct-eval'
if (Test-Path -LiteralPath $implementationWorktree) {
  throw "direct-eval implementation worktree path already exists: $implementationWorktree"
}
git worktree add -b codex/security-probe-direct-eval $implementationWorktree `
  foundation-security-probe-direct-eval-approved-plan
if ($LASTEXITCODE -ne 0) { throw "direct-eval worktree creation failed: $LASTEXITCODE" }
Set-Location -LiteralPath $implementationWorktree
if ((git branch --show-current) -ne 'codex/security-probe-direct-eval') {
  throw 'direct-eval implementation branch mismatch'
}
if ((git rev-parse HEAD) -ne $approved) { throw 'direct-eval implementation HEAD mismatch' }
if (git status --porcelain) { throw 'direct-eval implementation worktree is not clean' }
```

Expected: the new lightweight tag points exactly to the approved direct-eval plan commit and the
new branch/worktree is clean at that tag. All later R1/R2 commands run only from the new worktree.
Do not move or recreate this tag later. The old
`foundation-security-probe-reliability-approved-plan` remains immutable at `157ad89` as failed
page-load-route evidence and is never used by the direct-eval implementation range gates.

#### Checkpoint R1: Diagnose the Existing Result Channel

Checkpoint R1 may start only after plan Go and Step R0. It ends with a code-review decision.
Checkpoint R2 remains forbidden until R1 evidence and code receive Go.

- [ ] **Step R1.1: Write RED parser, diagnostic classifier and callback ownership tests**

Add a `#[cfg(test)]` module to `src-tauri/src/security_probe.rs` with exact-value fixtures. The
tests must include these names and assertions:

```rust
#[test]
fn diagnostic_exact_acl_and_legacy_match_is_73() {
    assert_eq!(
        exit_code(classify_diagnostic_exact(LegacyUrlResult::Matched)),
        73
    );
}

#[test]
fn diagnostic_exact_acl_and_legacy_miss_is_79() {
    assert_eq!(
        exit_code(classify_diagnostic_exact(LegacyUrlResult::Missed)),
        79
    );
}

#[test]
fn diagnostic_exact_acl_and_legacy_error_is_80() {
    assert_eq!(
        exit_code(classify_diagnostic_exact(LegacyUrlResult::QueryFailed)),
        80
    );
}

#[test]
fn parser_rejects_null_strings_arrays_invalid_json_and_unknown_fields() {
    for raw in [
        "null",
        "\"acl-denied\"",
        "[]",
        "{",
        r#"{"documentReadyState":"complete","probeState":"acl-denied","hash":"#acl-denied","evalError":null,"extra":true}"#,
    ] {
        assert_eq!(parse_observation(raw), Err(ProbeOutcome::InvalidEvalObservation));
    }
}

#[test]
fn in_flight_callback_at_total_deadline_is_terminal_and_dispatches_once() {
    let dispatches = std::sync::atomic::AtomicUsize::new(0);
    let held_sender = std::cell::RefCell::new(None);
    let outcome = run_eval_loop_with(
        test_timing(),
        |sender| {
            dispatches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held_sender.replace(Some(sender));
            Ok(())
        },
        |observation| classify_diagnostic(observation, None),
    );

    assert_eq!(outcome, ProbeOutcome::EvalCallbackTimeout);
    assert_eq!(dispatches.load(std::sync::atomic::Ordering::SeqCst), 1);
}

fn exact_acl_observation() -> ProbeObservation {
    ProbeObservation {
        document_ready_state: DocumentReadyState::Complete,
        probe_state: Some(ProbeState::AclDenied),
        hash: "#acl-denied".into(),
        eval_error: None,
    }
}

fn classify_diagnostic_exact(legacy: LegacyUrlResult) -> ProbeOutcome {
    match classify_diagnostic(&exact_acl_observation(), Some(legacy)) {
        ProbeDecision::Exit(outcome) => outcome,
        ProbeDecision::Retry(state) => panic!("exact ACL unexpectedly retried: {state:?}"),
    }
}

fn test_timing() -> ProbeTiming {
    ProbeTiming {
        total_timeout: std::time::Duration::from_millis(10),
        retry_delay: std::time::Duration::from_millis(1),
    }
}
```

Use these test helpers in the same module and add table tests proving:

1. loading with null state or exact ACL is `DocumentNotReady`, empty ready state is
   `ProbeScriptNotStarted`, and `invoke-pending` is `InvokePending`; at the deadline they become
   `74`, `75` and `76` respectively;
2. invoke success and other error with empty hash are terminal `77` and `78`; either state with a
   non-empty hash is `InvalidEvalObservation/83` for every readyState;
3. case/whitespace/encoded/extra hash variants, mismatched state, non-null `evalError`, missing
   fields and extra fields never produce `73`, `79` or `80`;
4. only `Interactive | Complete`, `Some(AclDenied)`, exact `#acl-denied` and null eval error can
   enter the legacy comparison;
5. eval dispatch error is `81`, callback timeout is `82`, invalid payload is `83`, and caught
   eval script error is `84`;
6. a late callback after terminal timeout cannot change the returned outcome;
7. after a successful dispatch, callback wait uses all and only post-dispatch remaining; if no
   callback returns by the total deadline, the result is `82` and dispatch count is exactly `1`;
8. after a completed retry observation with total remaining below 50ms, retry wait is exactly
   capped by remaining, the corresponding `74`, `75` or `76` is returned, and dispatch count
   remains exactly `1`;
9. when an injected dispatch advances the monotonic clock, the callback wait budget is recomputed
   after dispatch from the same deadline, not reused from the pre-dispatch value; if dispatch
   consumes the remaining budget, no callback wait occurs and the result is terminal `82`;
10. when callback receive advances the monotonic clock to or beyond the deadline, the result is
    terminal `82` before parse, classifier invocation or legacy URL lookup; even an exact ACL raw
    value cannot return `73`, `79` or `80`;
11. when callback arrives within budget but injected parse/classifier or legacy lookup advances the
    same clock to the deadline, the next gate returns `82`; no completed exact ACL result can bypass
    the final deadline check.

Timing cases 7 through 11 must exercise `run_eval_loop_with`, not only a standalone duration
helper. They may inject a private test clock/wait seam, but production and tests must share the same
budget calculation and state transition. Do not use wall-clock tolerance as the sole oracle.

- [ ] **Step R1.2: Run the focused RED test**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --features test-instrumentation security_probe -- --nocapture
```

Expected: compile failure because `ProbeObservation`, `classify_diagnostic`,
`run_eval_loop_with` and their helpers do not exist. A passing command means the RED test is not
testing the new contract.

- [ ] **Step R1.3: Add the feature-only frontend state machine**

Replace `src/security-probe.ts` with:

```ts
import { invoke } from '@tauri-apps/api/core'

type ProbeState =
  | 'invoke-pending'
  | 'invoke-succeeded'
  | 'acl-denied'
  | 'invoke-other-error'

declare global {
  interface Window {
    __UIPILOT_SECURITY_PROBE_STATE__?: ProbeState
  }
}

window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-pending'

try {
  void invoke('load_settings').then(
    () => {
      window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-succeeded'
    },
    (error: unknown) => {
      if (String(error) === 'Command load_settings not allowed by ACL') {
        window.__UIPILOT_SECURITY_PROBE_STATE__ = 'acl-denied'
        window.location.hash = 'acl-denied'
      } else {
        window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-other-error'
      }
    },
  )
} catch {
  window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-other-error'
}
```

Do not store or log the raw error. No other branch may write `window.location.hash`.

- [ ] **Step R1.4: Implement the strict parser and diagnostic classifier**

In `src-tauri/src/security_probe.rs`, implement the exact interfaces above and this fixed eval
expression:

```rust
const READ_OBSERVATION_SCRIPT: &str = r#"
(() => {
  try {
    return {
      documentReadyState: document.readyState,
      probeState: window.__UIPILOT_SECURITY_PROBE_STATE__ ?? null,
      hash: window.location.hash,
      evalError: null
    };
  } catch (_) {
    return {
      documentReadyState: 'loading',
      probeState: null,
      hash: '',
      evalError: 'script-failed'
    };
  }
})()
"#;
```

`parse_observation` calls only `serde_json::from_str` and maps every parse or schema error to
`InvalidEvalObservation`. The exact DOM predicate requires `Interactive | Complete`,
`Some(AclDenied)`, exact `#acl-denied`, and no `eval_error`; it has no external readiness input.

`classify_diagnostic` applies these rules in order:

```text
evalError == script-failed                           -> Exit(EvalScriptFailed)
invoke-succeeded + empty hash                        -> Exit(InvokeUnexpectedlyAllowed)
invoke-other-error + empty hash                      -> Exit(InvokeOtherError)
null + empty hash + loading                          -> Retry(DocumentNotReady)
null + empty hash + interactive/complete             -> Retry(ScriptNotStarted)
invoke-pending + empty hash                          -> Retry(InvokePending)
acl-denied + exact hash + loading                    -> Retry(DocumentNotReady)
strict ACL + legacy Matched                          -> Exit(LegacyUrlChannelMatched)
strict ACL + legacy Missed                           -> Exit(LegacyUrlChannelMiss)
strict ACL + legacy QueryFailed                      -> Exit(LegacyUrlQueryFailed)
all other state/hash/readiness combinations          -> Exit(InvalidEvalObservation)
```

`exit_code` is an exhaustive match. Do not use enum discriminants that duplicate `73`.

- [ ] **Step R1.5: Implement the direct-eval single-callback readiness loop**

The worker uses `Instant`, `sync_channel(1)`, `recv_timeout`, and a private testable helper with
this contract:

```rust
#[derive(Clone, Copy)]
struct ProbeTiming {
    total_timeout: std::time::Duration,
    retry_delay: std::time::Duration,
}

fn run_eval_loop_with<D, C>(
    timing: ProbeTiming,
    dispatch: D,
    classify: C,
) -> ProbeOutcome
where
    D: FnMut(std::sync::mpsc::SyncSender<String>) -> Result<(), ()>,
    C: FnMut(&ProbeObservation) -> ProbeDecision;
```

Production timing is exactly 10 seconds total and 50ms between bounded startup or completed-state
retries. Record the monotonic deadline immediately after the security-probe window build, then
enter `run_eval_loop_with`; there is no page-load wait and no second deadline. `dispatch` calls
`probe.eval_with_callback(READ_OBSERVATION_SCRIPT, ...)`.

Before the first returned callback, an immediate dispatch `Err` records
`StartupEvalUnavailable`, waits `min(retry_delay, remaining)`, and may retry only if the deadline
still has budget. If every startup dispatch returns `Err` until the deadline, return
`EvalDispatchFailed/81`. After any callback has returned, a later immediate dispatch `Err` is
terminal `81`.

Immediately before every dispatch, recompute remaining time. If a completed intermediate state
exists and no time remains, map it to `74`, `75` or `76`; if only startup dispatch errors exist,
return `81`. After a successful dispatch, mark exactly one callback in flight and recompute
remaining from the same deadline. If dispatch consumed the remaining budget, return
`EvalCallbackTimeout/82` without waiting. Otherwise call `recv_timeout(post_dispatch_remaining)`;
there is no independent callback timeout. Timeout or disconnect while that request is in flight is
terminal `82`, and no second eval may dispatch.

When callback receive succeeds, first close the in-flight request, then recompute the same
monotonic deadline again. If it is exhausted, return `EvalCallbackTimeout/82` before calling
`parse_observation`. Recheck after parse and before classifier or `probe.url()`, and recheck again
after any legacy lookup and before accepting a `73`, `79` or `80` outcome. Any exhausted gate
returns `82`. Only an exact result that passes every gate may leave the worker as diagnostic
success evidence; parsing, classification and URL lookup cannot reset or extend the deadline.

Observation retry sleeps only after a completed retry observation. Save that new intermediate
state, recompute remaining, and wait `min(retry_delay, remaining)`. If remaining is shorter than
the full retry delay, return that intermediate's `74`, `75` or `76` when the bounded wait ends and
do not dispatch. Only a completed full 50ms retry delay may lead to the next loop iteration. Every
deadline check, callback wait and retry wait uses the same monotonic clock; no branch may extend
the original deadline.

In checkpoint R1, when the strict DOM predicate is present, query `probe.url()` exactly once:

```rust
let legacy = match probe.url() {
    Ok(url) if url.fragment() == Some("acl-denied") => LegacyUrlResult::Matched,
    Ok(_) => LegacyUrlResult::Missed,
    Err(_) => LegacyUrlResult::QueryFailed,
};
```

Never use `.ok()`. Pass `Some(legacy)` to `classify_diagnostic`; all non-exact observations pass
`None`. The worker is the only caller of `std::process::exit(exit_code(outcome))`.

- [ ] **Step R1.6: Run focused and Rust quality gates**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --features test-instrumentation security_probe -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
$env:CARGO_INCREMENTAL = '0'
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
Remove-Item Env:CARGO_INCREMENTAL

$forbiddenReadiness = rg -n `
  "on_page_load|PageLoadEvent|page_load_finished|page_load_(channel|sender|receiver|guard)|sender_guard" `
  src-tauri/src/security_probe.rs
if ($LASTEXITCODE -eq 0) {
  throw "direct-eval runtime keeps a forbidden page-load dependency: $forbiddenReadiness"
}
if ($LASTEXITCODE -ne 1) { throw 'page-load source scan failed' }
```

Expected: focused parser/classifier/loop tests pass; the full suite has no failures; check and
Clippy exit `0`; the source scan exits exactly `1` with no forbidden readiness symbol. The existing
independent junction test may remain ignored.

- [ ] **Step R1.7: Run production and security isolation gates before the fresh diagnostic build**

Run:

```powershell
npm.cmd run build
if ($LASTEXITCODE -ne 0) { throw "production frontend build failed: $LASTEXITCODE" }
if (Test-Path dist/security-probe.html) { throw 'production frontend contains security probe' }
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1

$negative = @(& powershell -NoProfile -ExecutionPolicy Bypass `
  -File scripts/test-security-probe.ps1 `
  -Executable "$env:SystemRoot\System32\hostname.exe" 2>&1)
$negativeExit = $LASTEXITCODE
$negativeText = ($negative | ForEach-Object { $_.ToString() }) -join "`n"
$expectedNegative = 'Security probe expected ACL denial exit code 73, got 0'
if ($negativeExit -ne 1) {
  throw "hostname.exe oracle returned $negativeExit instead of 1: $negativeText"
}
if ($negativeText -notmatch [regex]::Escape($expectedNegative)) {
  throw "hostname.exe oracle did not authenticate the expected failure: $negativeText"
}
if ($negativeText -match 'security probe ok') {
  throw "hostname.exe oracle emitted a success marker: $negativeText"
}
```

Expected: production build and both security configuration gates pass; production output has no
probe page; `hostname.exe` is authenticated as the expected exit-`1`, got-`0` negative result.
Missing scripts, parse/start failures, other exits and any success marker fail the gate. Do not
launch a probe executable in this step.

- [ ] **Step R1.8: Build fresh and collect three raw diagnostic exit codes**

Run this block once. It intentionally does not invoke `test-security-probe.ps1` for the probe
because that script must reject diagnostic exits `79` and `80`.

```powershell
$ErrorActionPreference = 'Stop'
$processTarget = [EnvironmentVariableTarget]::Process
$pathValue = [Environment]::GetEnvironmentVariable('Path', $processTarget)
if ([string]::IsNullOrWhiteSpace($pathValue)) { throw 'process Path is unavailable' }
$pathKeys = @(
  [Environment]::GetEnvironmentVariables($processTarget).Keys |
    Where-Object { [string] $_ -ieq 'Path' }
)
if ($pathKeys.Count -lt 1) { throw 'process Path key is unavailable' }
foreach ($key in $pathKeys) {
  [Environment]::SetEnvironmentVariable([string] $key, $null, $processTarget)
}
[Environment]::SetEnvironmentVariable('Path', $pathValue, $processTarget)
$normalizedPathKeys = @(
  [Environment]::GetEnvironmentVariables($processTarget).Keys |
    Where-Object { [string] $_ -ieq 'Path' }
)
if ($normalizedPathKeys.Count -ne 1 -or [string] $normalizedPathKeys[0] -cne 'Path') {
  throw 'process Path/PATH keys were not normalized to one Path key'
}

$npm = (Get-Command npm.cmd -ErrorAction Stop).Source
$buildStarted = [DateTime]::UtcNow
& $npm run tauri build -- --no-bundle --features test-instrumentation `
  --config src-tauri/tauri.security-probe.conf.json
$buildExit = $LASTEXITCODE
if ($buildExit -ne 0) { throw "fresh diagnostic build failed: $buildExit" }

$metadataJson = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1
$metadataExit = $LASTEXITCODE
if ($metadataExit -ne 0) { throw "cargo metadata failed: $metadataExit" }
$metadata = $metadataJson | ConvertFrom-Json
$probeExe = Join-Path ([string] $metadata.target_directory) 'release\uipilot.exe'
if (-not (Test-Path -LiteralPath $probeExe -PathType Leaf)) {
  throw "fresh diagnostic artifact is missing: $probeExe"
}

$probe = Get-Item -LiteralPath $probeExe
if ($probe.LastWriteTimeUtc -lt $buildStarted) { throw 'diagnostic artifact is stale' }
$expectedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash
$appDataDir = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'com.uipilot.launcher'

function Get-ProtectedSnapshot([string] $Root) {
  $entries = @()
  $exists = Test-Path -LiteralPath $Root -PathType Container
  if ($exists) {
    $entries = @(Get-ChildItem -LiteralPath $Root -Force -File | Where-Object {
      $_.Name -like 'settings.json*' -or
      $_.Name -like 'validation-data.json*' -or
      $_.Name -like 'open-session.json*'
    } | Sort-Object Name | ForEach-Object {
      [pscustomobject]@{
        Name = $_.Name
        Length = $_.Length
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash
        LastWriteTicks = $_.LastWriteTimeUtc.Ticks
      }
    })
  }
  ConvertTo-Json -Compress -Depth 3 -InputObject ([pscustomobject]@{
    RootExists = $exists
    Entries = @($entries)
  })
}

function Get-ProbeTempProfiles {
  @(
    Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Force -Directory |
      Where-Object Name -Like 'uipilot-security-probe-*' |
      Sort-Object FullName |
      ForEach-Object FullName
  ) -join "`n"
}

$profilesBefore = Get-ProbeTempProfiles
$records = @()
foreach ($run in 1..3) {
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash -ne $expectedHash) {
    throw "diagnostic artifact hash changed before run $run"
  }
  $running = @(Get-Process -Name uipilot -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $probe.FullName
  })
  if ($running) { throw "probe process exists before diagnostic run $run" }
  $before = Get-ProtectedSnapshot $appDataDir
  $process = Start-Process -FilePath $probe.FullName -PassThru -WindowStyle Hidden
  try {
    if (-not $process.WaitForExit(15000)) {
      Stop-Process -Id $process.Id -Force
      throw "diagnostic run $run timed out"
    }
    $code = $process.ExitCode
  }
  finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
  }
  $after = Get-ProtectedSnapshot $appDataDir
  if ($before -cne $after) { throw "diagnostic run $run modified protected app-data" }
  if ((Get-ProbeTempProfiles) -cne $profilesBefore) {
    throw "diagnostic run $run left a temporary profile"
  }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash -ne $expectedHash) {
    throw "diagnostic artifact hash changed after run $run"
  }
  $running = @(Get-Process -Name uipilot -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $probe.FullName
  })
  if ($running) { throw "probe process remains after diagnostic run $run" }
  if ($code -notin 73, 79, 80) {
    throw "diagnostic run $run returned disallowed raw exit $code"
  }
  $category = switch ($code) {
    73 { 'LegacyUrlChannelMatched' }
    79 { 'LegacyUrlChannelMiss' }
    80 { 'LegacyUrlQueryFailed' }
  }
  $records += [pscustomobject]@{ Run = $run; ExitCode = $code; Category = $category }
}

[pscustomobject]@{
  BuildStartedUtc = $buildStarted.ToString('o')
  ArtifactUtc = $probe.LastWriteTimeUtc.ToString('o')
  Sha256 = $expectedHash
  Runs = $records
} | ConvertTo-Json -Depth 4
```

Expected: the very first un-warmed run and the next two runs all return raw `73`, `79` or `80`
against one SHA-256. Any other code, timeout, app-data change, profile/process residue or hash change
is R1 No-Go. `73` alone does not prove the legacy root cause; variation involving `79` or `80` is
direct legacy-channel instability evidence.

- [ ] **Step R1.9: Enforce R1 scope, commit, and request code review**

Run:

```powershell
$allowed = @('src/security-probe.ts', 'src-tauri/src/security_probe.rs')
$tracked = @(git diff --name-only foundation-security-probe-direct-eval-approved-plan --)
$untracked = @(git ls-files --others --exclude-standard)
$changed = @(
  @($tracked) + @($untracked) |
    Where-Object { $_ } |
    Sort-Object -Unique
)
$outside = @($changed | Where-Object { $_ -notin $allowed })
if ($outside) { throw "R1 changed files outside allowlist: $($outside -join ', ')" }
if (@($changed).Count -ne 2) { throw "R1 must change exactly two implementation files" }
if (git diff --cached --name-only) { throw 'unexpected staged files before R1 staging' }
git diff --check
git add src/security-probe.ts src-tauri/src/security_probe.rs
git diff --cached --check
git commit -m "fix: diagnose security probe result delivery"
git show --check --stat --oneline HEAD
if (git status --porcelain) { throw 'R1 worktree is not clean' }
```

Send the R1 SHA, raw three-run record, fresh artifact UTC/SHA-256, full verification output,
allowlist result and clean status for code review. Do not start R2 until the reviewer explicitly
approves R1 evidence and code.

#### Checkpoint R2: Replace the Legacy Runtime Channel

Checkpoint R2 may start only after R1 Go. It ends with a second code-review decision and does not
authorize integration into Task 4.

- [ ] **Step R2.1: Write RED tests that first reference the phase-two interface**

R1 contains neither `ProbeOutcome::ExactAclDenied` nor `classify_final`. In this RED step, add
tests that reference both names for the first time and prove the final classifier has no legacy
input and follows the final predicate:

```rust
#[test]
fn final_exact_acl_is_73_without_a_legacy_input() {
    let observation = exact_acl_observation();
    assert_eq!(
        classify_final(&observation),
        ProbeDecision::Exit(ProbeOutcome::ExactAclDenied)
    );
    assert_eq!(exit_code(ProbeOutcome::ExactAclDenied), 73);
}

#[test]
fn final_interactive_exact_acl_is_also_73() {
    let mut observation = exact_acl_observation();
    observation.document_ready_state = DocumentReadyState::Interactive;
    assert_eq!(
        classify_final(&observation),
        ProbeDecision::Exit(ProbeOutcome::ExactAclDenied)
    );
}

#[test]
fn final_requires_ready_state_exact_state_hash_and_no_eval_error() {
    for observation in final_invalid_acl_cases() {
        assert_ne!(
            classify_final(&observation),
            ProbeDecision::Exit(ProbeOutcome::ExactAclDenied)
        );
    }
}

fn final_invalid_acl_cases() -> Vec<ProbeObservation> {
    let exact = exact_acl_observation();
    let mut cases = Vec::new();

    let mut loading = exact.clone();
    loading.document_ready_state = DocumentReadyState::Loading;
    cases.push(loading);

    for hash in ["", "#ACL-DENIED", " #acl-denied", "%23acl-denied", "#acl-denied-extra"] {
        let mut observation = exact.clone();
        observation.hash = hash.into();
        cases.push(observation);
    }

    for state in [
        None,
        Some(ProbeState::InvokePending),
        Some(ProbeState::InvokeSucceeded),
        Some(ProbeState::InvokeOtherError),
    ] {
        let mut observation = exact.clone();
        observation.probe_state = state;
        cases.push(observation);
    }

    let mut script_failed = exact;
    script_failed.eval_error = Some(EvalError::ScriptFailed);
    cases.push(script_failed);
    cases
}
```

The invalid table must include `loading`, wrong/empty/encoded/case/whitespace hash,
null/pending/success/other state, and non-null `evalError`. Separate classifier tests must retain
the rule that success/other with a non-empty hash is `InvalidEvalObservation/83`.

- [ ] **Step R2.2: Run the focused RED test**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --features test-instrumentation final_ -- --nocapture
```

Expected: compile failure because R1 deliberately contains neither `ExactAclDenied` nor
`classify_final`. A passing command means the phase checkpoint was crossed early.

- [ ] **Step R2.3: Implement final classification and remove runtime URL access**

Add `ProbeOutcome::ExactAclDenied`, map it to `73`, and implement
`classify_final(observation)` with the exact phase-two predicate.
Change the runtime loop classifier to `classify_final`; when it returns `ExactAclDenied`, exit
`73` immediately. Delete all runtime calls to `probe.url()` and all runtime construction of
`LegacyUrlResult`.

Keep `classify_diagnostic`, `LegacyUrlResult`, and their evidence tests only under `#[cfg(test)]`.
Also put `#[cfg(test)]` directly on the `LegacyUrlChannelMatched`, `LegacyUrlChannelMiss` and
`LegacyUrlQueryFailed` enum variants and directly on their corresponding `exit_code` match arms.
The final non-test all-features target must compile only `ExactAclDenied` and the shared failure
variants. Do not use `allow(dead_code)` or another lint suppression. The non-test
`test-instrumentation` build must contain no legacy URL query. Do not change the frontend state
machine or timing established in R1.

- [ ] **Step R2.4: Run focused, full Rust and source-boundary gates**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --features test-instrumentation security_probe -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
$env:CARGO_INCREMENTAL = '0'
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
Remove-Item Env:CARGO_INCREMENTAL

$source = Get-Content -Raw -Encoding UTF8 src-tauri/src/security_probe.rs
if ($source -match '(allow|expect)\s*\(\s*dead_code') {
  throw 'security probe uses forbidden dead_code suppression'
}
foreach ($variant in @(
  'LegacyUrlChannelMatched',
  'LegacyUrlChannelMiss',
  'LegacyUrlQueryFailed'
)) {
  $name = [regex]::Escape($variant)
  $variantPattern = "(?m)^\s*#\[cfg\(test\)\]\r?\n\s*$name,\s*$"
  $armPattern = "(?m)^\s*#\[cfg\(test\)\]\r?\n\s*ProbeOutcome::$name\s*=>"
  if ($source -notmatch $variantPattern) {
    throw "$variant is not directly test-gated"
  }
  if ($source -notmatch $armPattern) {
    throw "$variant exit-code arm is not directly test-gated"
  }
}

$forbidden = rg -n `
  "\.url\(\)|WebviewWindow::url|data_directory|uipilot-security-probe-|on_page_load|PageLoadEvent|page_load_finished|page_load_(channel|sender|receiver|guard)|sender_guard" `
  src-tauri/src/security_probe.rs
if ($LASTEXITCODE -eq 0) { throw "final runtime keeps forbidden channel/profile code: $forbidden" }
if ($LASTEXITCODE -ne 1) { throw 'forbidden-symbol scan failed' }
```

Expected: all tests/checks pass; final non-test all-features Clippy has no dead-code warning; the
source oracle proves all three legacy variants and match arms are directly test-gated with no lint
suppression; the forbidden scan exits exactly `1` with no matches.

- [ ] **Step R2.5: Run production, configuration, negative oracle and allowlist gates**

Run:

```powershell
npm.cmd run build
if ($LASTEXITCODE -ne 0) { throw "production frontend build failed: $LASTEXITCODE" }
if (Test-Path dist/security-probe.html) { throw 'production frontend contains security probe' }
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1

$negative = @(& powershell -NoProfile -ExecutionPolicy Bypass `
  -File scripts/test-security-probe.ps1 `
  -Executable "$env:SystemRoot\System32\hostname.exe" 2>&1)
$negativeExit = $LASTEXITCODE
$negativeText = ($negative | ForEach-Object { $_.ToString() }) -join "`n"
$expectedNegative = 'Security probe expected ACL denial exit code 73, got 0'
if ($negativeExit -ne 1) {
  throw "hostname.exe oracle returned $negativeExit instead of 1: $negativeText"
}
if ($negativeText -notmatch [regex]::Escape($expectedNegative)) {
  throw "hostname.exe oracle did not authenticate the expected failure: $negativeText"
}
if ($negativeText -match 'security probe ok') {
  throw "hostname.exe oracle emitted a success marker: $negativeText"
}

$allowed = @('src/security-probe.ts', 'src-tauri/src/security_probe.rs')
$tracked = @(git diff --name-only foundation-security-probe-direct-eval-approved-plan --)
$untracked = @(git ls-files --others --exclude-standard)
$changed = @(
  @($tracked) + @($untracked) |
    Where-Object { $_ } |
    Sort-Object -Unique
)
$outside = @($changed | Where-Object { $_ -notin $allowed })
if ($outside) { throw "reliability work changed files outside allowlist: $($outside -join ', ')" }
if (@($changed).Count -ne 2) { throw 'reliability implementation allowlist is incomplete' }
git diff --check
```

Expected: production and security gates pass, `hostname.exe` is authenticated as the expected
exit-`1`, got-`0` negative result with no success marker, and the cumulative tracked plus untracked
implementation set contains exactly the two approved source files.

- [ ] **Step R2.6: Build a fresh final artifact and run the same SHA three times**

This must be the first launch of this final artifact. Do not run a probe between build completion
and the first loop iteration.

```powershell
$ErrorActionPreference = 'Stop'
$processTarget = [EnvironmentVariableTarget]::Process
$pathValue = [Environment]::GetEnvironmentVariable('Path', $processTarget)
if ([string]::IsNullOrWhiteSpace($pathValue)) { throw 'process Path is unavailable' }
$pathKeys = @(
  [Environment]::GetEnvironmentVariables($processTarget).Keys |
    Where-Object { [string] $_ -ieq 'Path' }
)
if ($pathKeys.Count -lt 1) { throw 'process Path key is unavailable' }
foreach ($key in $pathKeys) {
  [Environment]::SetEnvironmentVariable([string] $key, $null, $processTarget)
}
[Environment]::SetEnvironmentVariable('Path', $pathValue, $processTarget)
$normalizedPathKeys = @(
  [Environment]::GetEnvironmentVariables($processTarget).Keys |
    Where-Object { [string] $_ -ieq 'Path' }
)
if ($normalizedPathKeys.Count -ne 1 -or [string] $normalizedPathKeys[0] -cne 'Path') {
  throw 'process Path/PATH keys were not normalized to one Path key'
}

$npm = (Get-Command npm.cmd -ErrorAction Stop).Source
$buildStarted = [DateTime]::UtcNow
& $npm run tauri build -- --no-bundle --features test-instrumentation `
  --config src-tauri/tauri.security-probe.conf.json
$buildExit = $LASTEXITCODE
if ($buildExit -ne 0) { throw "fresh final probe build failed: $buildExit" }

$metadataJson = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1
$metadataExit = $LASTEXITCODE
if ($metadataExit -ne 0) { throw "cargo metadata failed: $metadataExit" }
$metadata = $metadataJson | ConvertFrom-Json
$probeExe = Join-Path ([string] $metadata.target_directory) 'release\uipilot.exe'
if (-not (Test-Path -LiteralPath $probeExe -PathType Leaf)) {
  throw "fresh final probe artifact is missing: $probeExe"
}
$probe = Get-Item -LiteralPath $probeExe
if ($probe.LastWriteTimeUtc -lt $buildStarted) { throw 'final probe artifact is stale' }
$expectedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash
$profilesBefore = @(
  Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Force -Directory |
    Where-Object Name -Like 'uipilot-security-probe-*' |
    Sort-Object FullName |
    ForEach-Object FullName
) -join "`n"

foreach ($run in 1..3) {
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash -ne $expectedHash) {
    throw "final probe hash changed before run $run"
  }
  $running = @(Get-Process -Name uipilot -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $probe.FullName
  })
  if ($running) { throw "probe process exists before run $run" }
  $output = & powershell -NoProfile -ExecutionPolicy Bypass `
    -File scripts/test-security-probe.ps1 -Executable $probe.FullName
  if ($LASTEXITCODE -ne 0 -or $output -notcontains 'security probe ok') {
    throw "final smoke run $run failed: $output"
  }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $probe.FullName).Hash -ne $expectedHash) {
    throw "final probe hash changed after run $run"
  }
  $running = @(Get-Process -Name uipilot -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $probe.FullName
  })
  if ($running) { throw "probe process remains after run $run" }
  $profilesAfter = @(
    Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Force -Directory |
      Where-Object Name -Like 'uipilot-security-probe-*' |
      Sort-Object FullName |
      ForEach-Object FullName
  ) -join "`n"
  if ($profilesAfter -cne $profilesBefore) { throw "run $run left a temporary profile" }
}

[pscustomobject]@{
  BuildStartedUtc = $buildStarted.ToString('o')
  ArtifactUtc = $probe.LastWriteTimeUtc.ToString('o')
  Sha256 = $expectedHash
  SmokeRuns = 3
} | ConvertTo-Json -Compress
```

Expected: the un-warmed first run and the next two runs all print `security probe ok`, all use one
unchanged SHA-256, the script proves protected app-data unchanged each time, and no matching probe
process or temporary profile remains.

- [ ] **Step R2.7: Commit the final channel and request code review**

Run:

```powershell
$allowed = @('src/security-probe.ts', 'src-tauri/src/security_probe.rs')
$tracked = @(git diff --name-only foundation-security-probe-direct-eval-approved-plan --)
$untracked = @(git ls-files --others --exclude-standard)
$changed = @(
  @($tracked) + @($untracked) |
    Where-Object { $_ } |
    Sort-Object -Unique
)
$outside = @($changed | Where-Object { $_ -notin $allowed })
if ($outside) { throw "final reliability diff exceeds allowlist: $($outside -join ', ')" }
if (@($changed).Count -ne 2) { throw 'final reliability diff must contain exactly two files' }
git diff --check
git add src-tauri/src/security_probe.rs
git diff --cached --check
git commit -m "fix: stabilize security probe result channel"
git show --check --stat --oneline HEAD
if (git status --porcelain) { throw 'R2 worktree is not clean' }
```

R2 should normally modify only the Rust file because the TypeScript state machine was committed
in R1. Stage only the files actually changed in R2; never stage the whole source directory.

Send both R1 and R2 SHAs, the phase-one raw diagnostic record, phase-two fresh 3/3 smoke record,
artifact UTC/SHA-256 values, full default/all-features/security/production gates, cumulative
allowlist result and clean worktree for code review. Do not integrate into Task 4 until the
reviewer explicitly grants code-level Go.

---

## Task 2: Lock the Result Protocol Behind a Rust Registry

**Design:** `docs/superpowers/specs/2026-07-18-result-registry-contract-design.md`

**Files:**
- Create: `src-tauri/src/model.rs`
- Create: `src-tauri/src/result_registry.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/protocol.ts`

- [ ] **Step 1: Write failing registry tests**

Add tests that prove:

1. Publishing the latest token replaces a caller-supplied fake `resultId`, assigns an opaque `requestId`, and assigns unique stable per-set `resultId` values.
2. A valid `(requestId, resultId)` resolves to a Rust-owned action, while serialized JSON contains no action, application ID, shortcut or executable path.
3. Query 1 begins, query 2 begins and publishes, then query 1 finishes: query 1 returns `None` and query 2 remains executable.
4. Query 2 reaches Rust before query 1: `begin_query(1)` returns `None` and cannot do work or publish.
5. A published result becomes stale immediately after the next valid `begin_query`, before the next result publishes.
6. A query begins, the window hides, then the query finishes: it cannot repopulate the registry.
7. An old invocation's IPC reaches Rust after a new show: its matching sequence is rejected by `invocationId` without clearing the new invocation's current result.
8. Unknown and stale IDs return fixed `RegistryError` values whose display text contains no ID or path.
9. Serialization uses `requestId` and `resultId`, has no `kind` or action field, and omits absent `subtitle` and `icon`.
10. An invalid or stale token returns `None` before ID allocation, consumes no counter values, and cannot partially replace `current`.

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

Construct `ResultRegistry` through `Default`. `begin_query` clears `current` only after active generation, invocation and strictly increasing sequence validation succeeds; rejected queries leave all state unchanged.

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
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon: Option<String>,
}
```

The private `ResultAction` map must remain non-serializable. `RegistryError` has only `StaleRequest` and `UnknownResult`, with fixed display text that includes no IDs or paths. Convert the standard-library atomic counter to fixed-format opaque strings using `req-{value:016x}` and `item-{value:016x}`; the identifiers may encode uniqueness but no business target.

`publish_if_latest` must acquire `state` once and keep that same `Mutex` guard through token validation, ID allocation, action-map construction and `current` replacement. It validates before the first `next_id` operation. An invalid token returns `None` without allocating an ID or changing state. Do not add UUID, database, cache, async-runtime or test-hook dependencies.

- [ ] **Step 4: Mirror only DTOs in TypeScript**

```ts
export interface ResultItem {
  resultId: string
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

**Source Design:** `docs/superpowers/specs/2026-07-18-start-menu-application-discovery-design.md`

**Status:** Awaiting plan review. Do not enter TDD until this Task 3 section is approved.

**Files:**
- Create: `src-tauri/src/apps/mod.rs`
- Create: `src-tauri/src/apps/discovery.rs`
- Create: `src-tauri/src/apps/rank.rs`
- Create: `src-tauri/src/apps/shortcut.rs`
- Create: `src-tauri/src/apps/cache.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/lib.rs`
- Create: `scripts/test-start-menu-boundary.ps1`

**Interfaces:**

`apps/mod.rs` owns the Rust-only record and fixed error surface. Do not serialize `Application`, `DiscoveryError` or `DiscoveryDiagnostics` to the WebView.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootKind { User, Common }

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StartMenuRoot {
    pub(crate) kind: RootKind,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Application {
    pub(crate) app_id: String,
    pub(crate) display_name: String,
    pub(crate) shortcut: PathBuf,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) icon: Option<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) use_count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiscoveryDiagnostics {
    pub(crate) missing_roots: u64,
    pub(crate) inaccessible_entries: u64,
    pub(crate) reparse_entries: u64,
    pub(crate) non_unicode_entries: u64,
    pub(crate) invalid_shortcuts: u64,
    pub(crate) unmapped_executables: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoverySnapshot {
    pub(crate) applications: Vec<Application>,
    pub(crate) diagnostics: DiscoveryDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryError {
    KnownFolderQuery,
    RootNotDirectory,
    RootUnavailable,
    RootReparsePoint,
    ComUnavailable,
    HashFailed,
    DuplicateAppId,
}
```

The fixed `Display` strings, in enum order, are `known folder query failed`, `start menu root is not a directory`, `start menu root is unavailable`, `start menu root is a reparse point`, `COM is unavailable`, `application identity hashing failed`, and `duplicate application identity`. No string includes an HRESULT or entry value. `Application.icon` is always `None` in MVP-A. `registry_entry(&Application)` creates a `ResultItem` with `icon: None` and a private `ResultAction::LaunchApplication`; no path or `app_id` enters the DTO.

`apps/mod.rs` re-exports only these crate-private production functions to later tasks; their implementations stay in the named focused modules:

```rust
pub(crate) fn discover() -> Result<DiscoverySnapshot, DiscoveryError>;
pub(crate) fn registry_entry(app: &Application) -> (ResultItem, ResultAction);
pub(crate) fn rank(applications: &[Application], query: &str) -> Vec<Application>;
```

Its private tests may call `known_folder_roots_with` and `discover_from_roots` with closures. These are test seams, not traits, configuration or Tauri commands. Root and child `io::Result<Metadata>` values pass through private `classify_root_metadata` and `classify_child_metadata` helpers so permission errors are deterministic unit-test inputs while production traversal uses the same branches.

`cache.rs` provides this process-memory surface backed by one `Mutex<()>` scan lock and one `RwLock<Vec<Application>>` snapshot:

```rust
pub(crate) struct AppCache {
    scan: Mutex<()>,
    applications: RwLock<Vec<Application>>,
}

impl AppCache {
    pub(crate) fn new() -> Self;
    #[cfg(test)]
    pub(crate) fn from_apps(applications: Vec<Application>) -> Self;
    pub(crate) fn snapshot(&self) -> Vec<Application>;
    pub(crate) fn contains(&self, app_id: &str) -> bool;
    pub(crate) fn refresh(&self) -> Result<DiscoveryDiagnostics, DiscoveryError>;
    pub(crate) fn refresh_with<F>(&self, discover: F) -> Result<DiscoveryDiagnostics, DiscoveryError>
    where
        F: FnOnce() -> Result<DiscoverySnapshot, DiscoveryError>;
}

pub(crate) fn start_initial_refresh(cache: Arc<AppCache>) -> io::Result<JoinHandle<()>> {
    start_initial_refresh_with(cache, discover)
}

fn start_initial_refresh_with<F>(
    cache: Arc<AppCache>,
    discover: F,
) -> io::Result<JoinHandle<()>>
where
    F: FnOnce() -> Result<DiscoverySnapshot, DiscoveryError> + Send + 'static,
{
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let handle = thread::Builder::new()
        .name("app-discovery".into())
        .spawn(move || {
            let _ = cache.refresh_with(|| {
                let _ = entered_tx.send(());
                discover()
            });
        })?;
    entered_rx.recv().map_err(|_| {
        io::Error::new(io::ErrorKind::Other, "app discovery worker did not start")
    })?;
    Ok(handle)
}
```

A failed refresh leaves the last good vector untouched. The cache never reads or writes `application-cache.json` because a user-modifiable path cache cannot be a trusted launch source.

`refresh_with` acquires the scan lock before invoking its discovery closure and holds it through the one snapshot replacement. `snapshot` and `contains` take only the snapshot lock; no path takes the snapshot lock before the scan lock. Initial discovery and every manual rescan therefore execute one at a time without a generation system.

`src-tauri/src/lib.rs` creates exactly one `Arc<AppCache>`, registers `Arc::clone(&app_cache)` with Tauri `manage`, and passes another clone to one named standard-library startup thread. The thread calls `AppCache::refresh`; its first failure leaves the new cache empty, while every later manual refresh failure preserves the last good snapshot. Task 4 and Task 5 must request `State<'_, Arc<AppCache>>`; they must not construct another `AppCache`.

`start_initial_refresh_with` is the private production implementation, not a `#[cfg(test)]` copy. It spawns the worker and returns only after a zero-capacity standard-library channel confirms that the worker entered the discovery closure after acquiring the scan lock. `start_initial_refresh` contains no thread logic and may only delegate to `start_initial_refresh_with(cache, discover)`. This guarantees the startup scan owns the scan lock before setup exposes commands; tests inject only the discovery closure and exercise the same spawn/handshake path as production.

Until Task 5 consumes discovery, ranking and registry conversion, declare the module exactly as `#[cfg_attr(not(test), allow(dead_code))] mod apps;`. Task 5 removes this scoped attribute when it wires the commands; do not add crate-wide or dependency-wide warning suppression.

- [ ] **Step 1: Write failing discovery, shortcut and identity tests**

Create the modules and test helpers, declare them from `src-tauri/src/lib.rs`, then add focused tests with injected temporary roots and resolver closures. The assertions must cover the approved design directly:

```rust
#[test]
fn known_folder_provider_uses_only_approved_ids_flags_and_null_token() {
    let mut calls = Vec::new();
    let roots = known_folder_roots_with(|id, flags, token| {
        calls.push((*id, flags, token));
        Ok(PathBuf::from(if *id == FOLDERID_Programs { r"C:\User" } else { r"C:\Common" }))
    }).unwrap();
    assert_eq!(roots[0].kind, RootKind::User);
    assert_eq!(roots[1].kind, RootKind::Common);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, FOLDERID_Programs);
    assert_eq!(calls[1].0, FOLDERID_CommonPrograms);
    assert!(calls.iter().all(|(_, flags, token)| *flags == KF_FLAG_DONT_VERIFY && token.is_none()));
}

#[test]
fn known_folder_failure_is_a_fixed_scan_error() {
    assert_eq!(
        known_folder_roots_with(|_, _, _| Err(())).unwrap_err(),
        DiscoveryError::KnownFolderQuery,
    );
}

#[test]
fn scans_only_regular_lnk_files_case_insensitively() {
    let snapshot = scan_fixture(&["One.lnk", "Two.LNK", "ignore.exe", "ignore.txt"]);
    assert_eq!(snapshot_titles(&snapshot), ["One", "Two"]);
}

#[test]
fn missing_root_is_empty_but_bad_existing_root_fails() {
    assert_eq!(discover_missing_fixture().unwrap().diagnostics.missing_roots, 1);
    assert_eq!(discover_non_directory_fixture().unwrap_err(), DiscoveryError::RootNotDirectory);
    assert_eq!(discover_unreadable_root_fixture().unwrap_err(), DiscoveryError::RootUnavailable);
    assert_eq!(discover_reparse_root_fixture().unwrap_err(), DiscoveryError::RootReparsePoint);
}

#[test]
fn child_reparse_and_inaccessible_entries_are_skipped() {
    let snapshot = discover_child_error_fixture().unwrap();
    assert_eq!(snapshot.diagnostics.reparse_entries, 2);
    assert_eq!(snapshot.diagnostics.inaccessible_entries, 1);
    assert!(!snapshot_titles(&snapshot).contains(&"Outside"));
}

#[test]
fn non_unicode_relative_path_is_skipped_without_lossy_text() {
    let snapshot = discover_non_unicode_fixture().unwrap();
    assert_eq!(snapshot.diagnostics.non_unicode_entries, 1);
    assert!(snapshot_titles(&snapshot).iter().all(|title| !title.contains('\u{fffd}')));
}

#[test]
fn invalid_shortcut_is_skipped_without_aborting_scan() {
    let snapshot = discover_with_one_invalid_shortcut().unwrap();
    assert_eq!(snapshot.diagnostics.invalid_shortcuts, 1);
    assert_eq!(snapshot_titles(&snapshot), ["Valid"]);
}

#[test]
fn app_id_has_fixed_vectors_and_case_only_rename_stability() {
    assert_eq!(
        app_id(RootKind::User, "Tools\\WeChat.lnk").unwrap(),
        "app-8fe952e53691106c491156368e5c9b70bd56a3fc0b2a43455a9a40c765d56f9f",
    );
    assert_eq!(
        app_id(RootKind::Common, "tools\\wechat.lnk").unwrap(),
        "app-567d9db3933c49330028523bda654c90a540288b10f56b10f6375cc6ddb1fae0",
    );
    assert_eq!(app_id(RootKind::User, "Tools\\WeChat.lnk"), app_id(RootKind::User, "tools\\wechat.LNK"));
    assert_ne!(app_id(RootKind::User, "Tools\\WeChat.lnk"), app_id(RootKind::User, "Other\\WeChat.lnk"));
}

#[test]
fn duplicate_ids_fail_without_merging() {
    assert_eq!(discover_duplicate_id_fixture().unwrap_err(), DiscoveryError::DuplicateAppId);
}

#[test]
fn target_and_absolute_root_do_not_change_app_id() {
    let first = discovered_id(RootKind::User, r"C:\FirstRoot", "Tools\\App.lnk", Some(r"C:\One.exe"));
    let second = discovered_id(RootKind::User, r"D:\OtherRoot", "Tools\\App.lnk", Some(r"D:\Two.exe"));
    assert_eq!(first, second);
}

#[test]
fn shortcut_loader_uses_raw_path_and_never_resolve() {
    let calls = load_instrumented_shortcut();
    assert_eq!(calls.get_path_flags, [SLGP_RAWPATH]);
    assert_eq!(calls.resolve_count, 0);
}

#[test]
fn unsafe_or_non_executable_raw_targets_have_no_mapping() {
    for raw in [r"%LOCALAPPDATA%\App.exe", r"App.exe", r"\\server\App.exe", r"\\?\C:\App.exe", r"C:\App.cmd"] {
        assert_eq!(validate_raw_executable(raw), None);
    }
}

#[test]
fn empty_error_and_invalid_utf16_raw_targets_have_no_mapping() {
    assert_eq!(validate_get_path_result(Ok(None)), None);
    assert_eq!(validate_get_path_result(Err(())), None);
    assert_eq!(validate_raw_executable_wide(&[0xD800, 0]), None);
}

#[test]
fn nonexistent_drive_absolute_exe_is_kept_without_io() {
    assert_eq!(validate_raw_executable(r"Z:\missing\App.exe"), Some(PathBuf::from(r"Z:\missing\App.exe")));
}

#[test]
fn result_and_diagnostics_do_not_expose_private_values() {
    let (item, action) = registry_entry(&private_application());
    assert_eq!(item.icon, None);
    let json = serde_json::to_string(&item).unwrap();
    assert!(!json.contains("app-") && !json.contains(".lnk") && !json.contains(".exe"));
    assert!(matches!(action, ResultAction::LaunchApplication { .. }));
    assert_eq!(DiscoveryDiagnostics::default(), DiscoveryDiagnostics {
        missing_roots: 0,
        inaccessible_entries: 0,
        reparse_entries: 0,
        non_unicode_entries: 0,
        invalid_shortcuts: 0,
        unmapped_executables: 0,
    });
    assert_eq!(DiscoveryError::KnownFolderQuery.to_string(), "known folder query failed");
    assert_eq!(DiscoveryError::DuplicateAppId.to_string(), "duplicate application identity");
}
```

The helpers above are test code created in the named module; they are not production APIs. Tests that cannot create an unreadable Windows entry reliably use a closure returning the corresponding `io::ErrorKind`. Construct invalid Unicode with `OsStringExt::from_wide` containing an unpaired surrogate; never embed replacement text in the fixture.

- [ ] **Step 2: Run the first focused test set and confirm failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::
```

Expected: non-zero exit because the production discovery and shortcut functions do not exist yet. Do not weaken or ignore a test to obtain the red state.

- [ ] **Step 3: Implement the minimal bounded discovery and shortcut parser**

Add a direct dependency on the already locked `windows = 0.61.3` with only the features needed here: `Win32_Foundation`, `Win32_Security_Cryptography`, `Win32_Storage_FileSystem`, `Win32_System_Com`, and `Win32_UI_Shell`. Do not add an icon, hash or fuzzy-search crate.

Production root discovery calls `SHGetKnownFolderPath` exactly twice with `FOLDERID_Programs` and `FOLDERID_CommonPrograms`, `KF_FLAG_DONT_VERIFY`, and Rust `None` for the token (`hToken = NULL`). Convert the returned UTF-16 to `OsString` without loss and release every returned `PWSTR` with `CoTaskMemFree`. Any query failure returns `KnownFolderQuery`; there is no environment or config fallback.

Use `std::fs::symlink_metadata`, `std::os::windows::fs::MetadataExt::file_attributes`, and `FILE_ATTRIBUTE_REPARSE_POINT` before every descent or file acceptance. A missing root increments `missing_roots`; an existing bad root returns its fixed hard error. Child metadata/read errors increment `inaccessible_entries`; every child reparse point increments `reparse_entries` and is skipped. Sort accepted relative paths before parsing. Only regular `.lnk` files are accepted with ASCII case-insensitive extension matching.

Normalize an accepted relative path component-by-component with `OsStr::to_str`, join using `\\`, then call Unicode `to_lowercase`. Never call `to_string_lossy` or substitute invalid code points; skip a directory subtree or file whose relative path is not losslessly representable. Hash the exact UTF-8 preimage from the approved design with Windows CNG SHA-256, close all CNG handles on every branch, and fail the complete scan on CNG failure or duplicate ID.

Initialize COM on the background scan thread. Create `IShellLinkW`, load through `IPersistFile`, and call only `GetPath(..., SLGP_RAWPATH)`. Never call `Resolve`, `canonicalize`, environment expansion, target metadata, icon APIs or target existence checks. `IPersistFile::Load` failure skips that item; COM/Shell Link creation failure aborts the scan. Accept an executable mapping only when the raw UTF-16 is lossless, contains no `%`, uses a drive-letter absolute path, is not UNC/verbatim/device namespace, and ends in `.exe` case-insensitively. Set `icon: None` for every application.

- [ ] **Step 4: Add and run the Windows junction boundary gate**

Create `scripts/test-start-menu-boundary.ps1`. It creates one process-temp test root, a separate outside directory containing the uniquely named sentinel `Outside.lnk`, and a junction inside the injected root pointing outside. It sets only `UIPILOT_TEST_USER_ROOT`, `UIPILOT_TEST_COMMON_ROOT`, and `UIPILOT_TEST_OUTSIDE_SENTINEL`, then runs the ignored test-only case:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml `
  apps::discovery::tests::junction_does_not_escape_injected_root `
  -- --ignored --exact
```

The Rust test reads those variables only under `#[cfg(test)]` and calls `discover_from_roots` with an injected resolver closure. The closure compares only `path.file_name()` with the sentinel file name and immediately panics if it receives `Outside.lnk`; all other `.lnk` inputs return `ShortcutMetadata { executable: None }` without calling `IPersistFile`. After the scan, assert `snapshot.diagnostics.reparse_entries == 1` and that the sentinel was never observed. This proves the junction was rejected before descent or shortcut parsing; an invalid fake `.lnk` cannot create a false pass.

```rust
#[test]
#[ignore = "run by scripts/test-start-menu-boundary.ps1"]
fn junction_does_not_escape_injected_root() {
    let roots = injected_roots_from_test_env();
    let sentinel_name = injected_sentinel_from_test_env().file_name().unwrap().to_owned();
    let sentinel_seen = Cell::new(false);
    let snapshot = discover_from_roots(roots, |path| {
        if path.file_name() == Some(sentinel_name.as_os_str()) {
            sentinel_seen.set(true);
            panic!("scanner reached the outside sentinel");
        }
        Ok(ShortcutMetadata { executable: None })
    }).unwrap();

    assert_eq!(snapshot.diagnostics.reparse_entries, 1);
    assert!(!sentinel_seen.get());
}
```

In `finally`, resolve and verify both cleanup roots remain children of `[IO.Path]::GetTempPath()`, remove the junction itself with `Remove-Item -LiteralPath -Force`, then remove the two temp trees with native PowerShell `Remove-Item -Recurse -Force`. Production root discovery never reads these variables.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::discovery
cargo test --manifest-path src-tauri/Cargo.toml apps::shortcut
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-menu-boundary.ps1
```

Expected: all discovery/shortcut tests pass and the boundary script exits 0.

- [ ] **Step 5: Write failing cache and ranking tests**

Add tests with an initial cached application so failure preservation is observable, plus deterministic ranking fixtures:

```rust
#[test]
fn failed_refresh_preserves_last_good_snapshot() {
    let cache = AppCache::from_apps(vec![application("Existing", 0)]);
    assert_eq!(cache.refresh_with(|| Err(DiscoveryError::KnownFolderQuery)), Err(DiscoveryError::KnownFolderQuery));
    assert_eq!(titles(&cache.snapshot()), ["Existing"]);
}

#[test]
fn successful_refresh_replaces_snapshot_once() {
    let cache = AppCache::from_apps(vec![application("Old", 0)]);
    cache.refresh_with(|| Ok(snapshot([application("New", 0)]))).unwrap();
    assert_eq!(titles(&cache.snapshot()), ["New"]);
}

#[test]
fn initial_background_refresh_populates_the_shared_instance() {
    let managed = Arc::new(AppCache::new());
    let command_state = Arc::clone(&managed);
    let handle = start_initial_refresh_with(Arc::clone(&managed), || Ok(snapshot([application("First", 0)]))).unwrap();
    handle.join().unwrap();
    assert!(Arc::ptr_eq(&managed, &command_state));
    assert_eq!(titles(&command_state.snapshot()), ["First"]);
}

#[test]
fn failed_initial_refresh_leaves_shared_instance_empty() {
    let managed = Arc::new(AppCache::new());
    let handle = start_initial_refresh_with(Arc::clone(&managed), || Err(DiscoveryError::KnownFolderQuery)).unwrap();
    handle.join().unwrap();
    assert!(managed.snapshot().is_empty());
}

#[test]
fn initial_refresh_and_later_rescan_are_serialized_and_later_result_wins() {
    let cache = Arc::new(AppCache::new());
    let (release_initial_tx, release_initial_rx) = mpsc::sync_channel(0);
    let initial = start_initial_refresh_with(Arc::clone(&cache), move || {
        release_initial_rx.recv().unwrap();
        Ok(snapshot([application("Initial", 0)]))
    }).unwrap();

    let later_cache = Arc::clone(&cache);
    let later = thread::spawn(move || {
        later_cache.refresh_with(|| Ok(snapshot([application("Later", 0)]))).unwrap();
    });
    release_initial_tx.send(()).unwrap();
    initial.join().unwrap();
    later.join().unwrap();

    assert_eq!(titles(&cache.snapshot()), ["Later"]);
}

#[test]
fn exact_prefix_contains_and_subsequence_are_ordered() {
    let apps = ranking_fixture(["企业微信", "微信", "Visual Studio Code", "Unrelated"]);
    assert_eq!(titles(&rank(&apps, "微信")), ["微信", "企业微信"]);
    assert_eq!(titles(&rank(&apps, "企业")), ["企业微信"]);
    assert_eq!(titles(&rank(&apps, "vsc")), ["Visual Studio Code"]);
}

#[test]
fn aliases_rank_without_changing_display_name() {
    let apps = fixture_with_alias("Visual Studio Code", "code");
    assert_eq!(titles(&rank(&apps, "code")), ["Visual Studio Code"]);
}

#[test]
fn use_count_then_name_then_app_id_break_ties() {
    assert_eq!(ids(&rank(&tie_fixture(), "app")), expected_tie_order());
}

#[test]
fn empty_query_is_empty_and_results_are_limited_to_twenty() {
    assert!(rank(&many_apps(25), "").is_empty());
    assert_eq!(rank(&many_apps(25), "app").len(), 20);
}

#[test]
fn duplicate_display_names_keep_distinct_ids_and_aliases() {
    let apps = duplicate_name_fixture();
    assert_ne!(apps[0].app_id, apps[1].app_id);
    assert_ne!(apps[0].aliases, apps[1].aliases);
}
```

Ranking uses the best exact, prefix, substring or character-subsequence match across display name and aliases after Unicode lowercase. For equal match class, prefer an alias match, then descending `use_count`, lowercase display name, and finally `app_id`. `rank` returns at most 20 cloned Rust records and no result for an empty query.

- [ ] **Step 6: Run the second focused test set and confirm failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::cache
cargo test --manifest-path src-tauri/Cargo.toml apps::rank
```

Expected: non-zero exit because `AppCache` and ranking are not implemented.

- [ ] **Step 7: Implement the process-memory cache and deterministic ranking**

Implement `AppCache` with one `Mutex<()>` scan lock and one `RwLock<Vec<Application>>`. `refresh` delegates to `refresh_with(discover)`. `refresh_with` holds the scan lock across discovery and the one write-lock replacement; an error leaves the vector unchanged. `snapshot` and `contains` never acquire the scan lock. Do not create a database, disk cache, watcher, async runtime or generic cache abstraction.

In `src-tauri/src/lib.rs`, create the single `Arc<AppCache>` before building Tauri, register a clone with `.manage(...)`, and start the initial refresh from the existing `.setup(...)` closure after the security-probe setup branch. `start_initial_refresh` delegates directly to the non-conditional private `start_initial_refresh_with(cache, discover)`. The shared helper uses `std::thread::Builder::new().name("app-discovery")`, the scan-lock-entry handshake, and returns its `JoinHandle`; tests join it and production detaches it. The worker may retain only the shared `Arc`; it logs at most the fixed `DiscoveryError` category and never logs names, paths or HRESULT text.

Implement the ranking contract from Step 5 with standard-library strings and iterators. Do not transliterate, correct spelling, infer semantics or add a dependency. `registry_entry` sets `ResultItem.result_id` to an empty placeholder for Task 2 to overwrite, uses the display name as `title`, sets `subtitle` and `icon` to `None`, and places `app_id`, shortcut and executable only in `ResultAction`.

- [ ] **Step 8: Run the complete Task 3 gate**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml apps::
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-menu-boundary.ps1
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
$forbidden = rg -n '%APPDATA%|%ProgramData%|SHGetFileInfo|Windows Imaging|application-cache\.json|\.Resolve\(' src-tauri/src/apps
if ($LASTEXITCODE -eq 0) { $forbidden; throw 'Forbidden Task 3 implementation found' }
if ($LASTEXITCODE -ne 1) { throw 'rg failed while checking forbidden Task 3 implementation' }
git diff --check
```

Expected: every command exits 0; tests cover all 14 approved design cases plus ranking/cache behavior. The forbidden-pattern gate finds no environment-root, icon extraction, disk path cache or `Resolve` implementation. Production code contains no test-root environment variable access outside `#[cfg(test)]`.

- [ ] **Step 9: Commit**

```powershell
git add src-tauri scripts/test-start-menu-boundary.ps1
git commit -m "feat: discover and rank Start Menu apps"
```

---

## Task 4: Persist Settings and Daily Validation Counts

**Status:** No-Go for TDD until all three implementation plans below pass written review.

- Task 4A: `docs/superpowers/plans/2026-07-18-task-4a-settings-persistence.md`
- Task 4B: `docs/superpowers/plans/2026-07-18-task-4b-validation-store.md`
- Task 4C: `docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md`

Execution order is fixed as 4A -> 4B -> 4C. Each plan has its own TDD, verification and commit gate. Approval or completion of one plan does not authorize the next unapproved plan.

No Task 5 command, action, activation, rescan or TypeScript wiring may be added while these implementation plans are under review or execution.

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
- `rescan_apps` moves discovery to a blocking runtime worker, returns only after that worker finishes, and never initializes COM on the command thread.
- `export_validation_data` runs the chooser on the main UI thread; cancel returns before any writer worker is spawned.
- A confirmed export moves only `ExportDestination + cloned AppHandle` into `spawn_blocking`; snapshot, serialization, `sync_all` and replacement all run on that worker.

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

All Task 5 commands request the same `State<'_, Arc<AppCache>>` created in Task 3; no command constructs or replaces the owner. `search_apps(query, invocationId, querySequence)` calls `begin_query`, clones `AppCache::snapshot`, asks the single `SettingsStore` to decorate only that clone with aliases and `use_count`, ranks it, and finishes through `publish_if_latest`. It returns `null` for an old invocation, superseded query or hidden generation and never calls an unconditional publish.

`execute_result(requestId, resultId)` resolves only through the registry, updates the current date according to the fixed event mapping, and after a successful launch/activation request asks `SettingsStore::increment_use_count` to atomically persist the trusted action's `app_id`. A count-write failure must not execute the application a second time or alter the already determined launch/activation outcome; report only the fixed settings-persistence category and no ID or path. Then call `clear_and_hide`; system-action failures retain the window. `hide_launcher()` calls the same `clear_and_hide` function with no frontend-supplied target. `rescan_apps()` calls `refresh` on that same `AppCache`; failure preserves the current snapshot, while success replaces it once with the two roots returned by the fixed Known Folder provider. Other settings and validation commands call their crate-private Rust services.

`export_validation_data` is a zero-argument async command. It uses Tauri's main-thread dispatcher to call `choose_export_destination(HWND)` and maps `None` to a Task 5-owned fixed `Cancelled` result without spawning the writer. For `Some(destination)`, it moves that value plus a cloned `AppHandle` into `tauri::async_runtime::spawn_blocking`; the closure obtains the managed `SettingsStore`/`ValidationStore` and calls `write_validation_export`. Success maps to Task 5-owned `Exported`. The command has no path/filename/payload input, and Task 7 adds the first matching TypeScript type.

`rescan_apps` is fixed as an async Tauri command and has no alternate synchronous entry point:

```rust
#[tauri::command]
pub async fn rescan_apps(
    cache: State<'_, Arc<AppCache>>,
) -> Result<(), AppError> {
    let cache = Arc::clone(cache.inner());
    tauri::async_runtime::spawn_blocking(move || cache.refresh())
        .await
        .map_err(|_| AppError::ScanWorkerFailed)??;
    Ok(())
}
```

`DiscoveryError` converts only to the existing fixed path-free application error. `AppError::ScanWorkerFailed` also has one fixed path-free message. `discover` initializes its COM guard after entering the `spawn_blocking` closure, and the guard calls `CoUninitialize` on that same blocking worker before returning. Do not add Tokio, another runtime, `block_on`, or a synchronous command wrapper. Tauri documents that synchronous commands execute on the main thread and async commands are preferred for heavy work: [Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/#async-commands).

When these commands consume the application module, replace `#[cfg_attr(not(test), allow(dead_code))] mod apps;` with plain `mod apps;`. No warning suppression remains after Task 5.

Register exactly the commands already declared in `build.rs`. Do not add a generic `run`, `open`, `readFile`, `writeFile`, or `request` command.

- [ ] **Step 5: Verify all command branches**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
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
- Modify: `src-tauri/src/session_marker.rs`
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
- `ExitRequested` and `WM_QUERYENDSESSION` flush but retain the open marker when exit is cancelled.
- Tray exit, `RunEvent::Exit`, and `WM_ENDSESSION(wParam != 0)` clear the marker.
- Main-window CloseRequested and ordinary hide paths do not clear the process-session marker.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml lifecycle`

Expected: compile failure because lifecycle module does not exist.

- [ ] **Step 3: Implement one-window lifecycle**

Install the single-instance plugin first in the builder. Register `Alt+Space` through the global-shortcut plugin. On invocation, generate the opaque monotonically increasing `invocationId`, call `ResultRegistry::on_show(invocationId.clone())`, center the fixed-size window near the top of the current monitor, show it, focus it, and emit the same `invocationId` in `launcher://shown` with a Rust `Instant` measurement in test builds.

Create a tray menu with only `打开设置` and `退出`. Focus loss and window close call `clear_and_hide` without touching the process marker. Tray `退出`, Tauri run events and a minimal Windows session-end message hook implement the exact clean-exit transitions in `Validation export`; the hook never blocks shutdown. Apply autostart changes only after the corresponding plugin call succeeds, then persist settings.

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
- Any non-empty text increments `querySequence` and invokes only `search_apps(query, invocationId, querySequence)`.
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

`scripts/smoke-launcher.ps1 -Executable <absolute-path> [-RequireSecurityProbe]` must run exactly the supplied executable, verify it starts once, verify a second process exits, and confirm the first process remains alive. With `-RequireSecurityProbe`, it additionally requires the probe window and delegates its permission assertion to `test-security-probe.ps1`; absence of the page is a failure. It must not launch arbitrary applications automatically; application activation remains a manual checklist because it changes desktop state.

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

`scripts/benchmark-launcher.ps1 -Executable <absolute-path>` must use exactly the supplied instrumented executable, Windows UI Automation and one external `Stopwatch` clock for the hot-launch measurement. Run 5 warmups followed by 200 measured `Alt+Space` invocations and assert `shortcut_sent_external -> input_focus_observed_external` P95 is at most 1 second. With the deterministic 500-application in-memory cache, run 1,000 fixed queries through the actual input/DOM path and assert `query_input_ui -> app_results_committed_ui` P95 is at most 100 ms. The report must include all reference-environment fields required by section 10.1 and must not contain query text or application names.

- [ ] **Step 4: Run the complete automated gate**

Run:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
npm run tauri build -- --no-bundle
$productionExe = (Resolve-Path 'src-tauri/target/release/uipilot.exe').Path
& .\scripts\smoke-launcher.ps1 -Executable $productionExe
$probeExe = & .\scripts\build-security-probe.ps1 | Select-Object -Last 1
if (-not (Test-Path -LiteralPath $probeExe)) { throw 'Probe executable was not produced' }
& .\scripts\smoke-launcher.ps1 -Executable $probeExe -RequireSecurityProbe
& .\scripts\benchmark-launcher.ps1 -Executable $probeExe
git diff --check
```

Expected: every command exits 0. Manual checks are recorded with Windows build, WebView2 version, application used, result, and allowed-difference classification.

The probe test, probe smoke and benchmark must all receive the exact path emitted by `build-security-probe.ps1`. The gate must contain no direct `tauri build --features test-instrumentation` command.

- [ ] **Step 5: Review against the frozen scope**

Confirm all statements are true before marking the plan complete:

- There is one local-only WebView and one trusted action registry.
- No command accepts a file path, executable path, URL, shell fragment, or arbitrary payload from TypeScript.
- App discovery stays within `FOLDERID_Programs` and `FOLDERID_CommonPrograms` returned by the fixed Known Folder provider.
- Activation ambiguity and refusal follow the frozen fallback policy.
- The implementation introduces only launcher and application capabilities defined by this plan.
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

This plan is complete only when Tasks 1-8 pass on Windows 11 x64 and the launcher-only acceptance evidence is recorded. Foundation 完成不等于 MVP-A；下一必需交付物是签名安装包与发布验证计划。
