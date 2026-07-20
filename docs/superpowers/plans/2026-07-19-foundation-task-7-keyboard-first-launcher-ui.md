# Foundation Task 7 Keyboard-First Launcher UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Windows MVP keyboard-first launcher and settings UI as a thin React 19 / Ant Design 6 view over one independently tested frontend ownership core, then connect it to the frozen eight-command and `launcher://shown` contracts only after Task 6 Code Go.

**Architecture:** `launcher-core.ts` owns all protocol, async, invocation, search, IME, settings, and operation-token state behind one exact injected client. `native-input.ts` is the only raw text-event trust adapter, and `launcher-view.tsx` renders immutable snapshots with controlled AntD inputs without receiving Tauri. Phase A is package/core/view-only from Task 5 baseline `1dafdac`; R3 proposes one correlated zero-payload end boundary plus exact-value state idempotence before the real `main.ts` adapter/final evidence.

**Tech Stack:** Node `v22.21.1`, npm `10.9.4`, TypeScript `7.0.2`, Vite `8.1.5`, Vitest `4.1.10`, jsdom `29.1.1`, React `19.2.7`, ReactDOM `19.2.7`, Ant Design `6.5.1`, Tauri JS API `2.11.1`.

## Status And Review Gates

- R1 diagnostic status: Pass A No-Go against `a036c00f9ee00b876631732f65f49a9af30a77e5`; Pass B correctly not run.
- R2 Design/Plan/Security Go: revoked after the failed post-GREEN WebView2 gate; its four-file diff was restored without a
  commit. R3 Design/Plan/Code/Security Go and Task 8 Code/Security Go are complete.
- Phase-A Code/Dependency Go is bound to `f7016634046303b32a750d0942fb549f777028d6`; its dependency evidence remains
  unchanged.
- Task 6 Code Go/local integration is bound to `a8626e72e97a5caa924333e6d6545efe9cd2e6d0`.
- Current clean product HEAD is `16018e56486bcd4efcd1a2c81798ebc9223025e7`.
- Pass A proved the same-IME trusted non-composing tail and later independent same-value ordinary input have identical
  permitted metadata; natural cancel restoration was not proven. This is design input, not production-pass evidence.
- Local TaskCodeGo and main integration remain No-Go until the revised minimal local gate and separate review complete.
- `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` remains. Do not run or repair the runtime positive probe.
- No merge to `main`, push, signing, trial, release, tag creation, evidence-worktree cleanup, or Task 6/security worktree change is authorized by this plan.
- Latest scope disposition: Task 8 Code/Security Go remains bound to clean product HEAD
  `16018e56486bcd4efcd1a2c81798ebc9223025e7`. All custom Task 9 performance, CDP, Job cleanup, and diagnostic
  infrastructure is permanently non-executable failed evidence. Only the minimal local code gate in Task 9 may be
  considered after this revision receives written Plan/Security Go; release/QA acceptance remains blocked.

Tasks 0-8 below retain the reviewed implementation sequence as historical context. Do not rerun, amend, or rewrite its
commits. Task 7 records the completed R1 diagnostic and failed R2 gate; Task 7R3 and Task 8 record the approved product
path ending at `16018e56486bcd4efcd1a2c81798ebc9223025e7`.

## Global Constraints

- Product changes are limited to exactly these ten paths:

```text
package.json
package-lock.json
tsconfig.json
src/launcher-core.ts
src/native-input.ts
src/launcher-view.tsx
src/launcher.test.tsx
src/main.ts
src/protocol.ts
src/styles.css
```

- `index.html`, `vite.config.ts`, all `src-tauri/**`, Cargo files, capabilities, permissions, Tauri config/CSP, security-probe/config/scripts, and every path outside the allowlist remain byte-identical to the applicable baseline.
- Add exactly `react 19.2.7`, `react-dom 19.2.7`, and `antd 6.5.1` as exact production dependencies; add exactly `@types/react 19.2.17` and `@types/react-dom 19.2.3` as exact development dependencies. Add no React plugin, router, state/query library, Testing Library, animation package, direct icon dependency, or other package.
- Keep `withGlobalTauri: false` and the existing CSP. AntD/React receives no Tauri client. Only `src/main.ts` may import `@tauri-apps/api`.
- Use only `ConfigProvider`, `App`, `Input`, `Form`, `Checkbox`, `Button`, `List`, `Alert`, `Spin`, and `theme` as AntD value imports. Disable AntD motion. Do not use AutoComplete, Select, Card, Modal, Popconfirm, notification/message globals, remote assets/fonts, or direct `@ant-design/icons` imports.
- Search and execution use only `invocationId`, `querySequence`, `requestId`, and `resultId`. `appId` exists only in core-private settings mappings and `UserSettingsUpdate.aliases`; it never reaches DOM text, IDs, attributes, React keys, logs, or action calls.
- The only Tauri event is `launcher://shown`; the only commands are the existing eight exact names and argument objects. No Tauri window API or direct `.hide()` is permitted.
- R3 permits one zero-payload `compositionBoundary` only when a same-target end closes the exact binding-local active
  session created by a trusted start. Only trusted input supplies text. Boundary/end data, DOM end value, timers,
  microtasks, adjacency/order/delay, `inputType` classification, suppression, tombstones, native bridges, and generic
  event frameworks remain forbidden. Exact-value commits are state no-ops; unchanged-query rerun belongs to Enter.
- No raw thrown/backend message, query, alias, result text, app ID, path, user value, or WebView2 text is logged or persisted.
- Keep commits small and in the task order below. Do not amend or rewrite a reviewed commit.

Local presentation strings are exact and never use `CommandError.message` or thrown values:

| Condition | Exact local text |
|---|---|
| `staleRequest`, `unknownResult` | `搜索结果已过期，请重新搜索。` |
| `applicationEntryUnavailable` | `应用入口不可用，请重新扫描。` |
| `settingsFailed` | `设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。` |
| `validationFailed` | `验证数据操作失败。` |
| `windowFailed` | `窗口操作失败。` |
| `scanFailed`, `scanWorkerFailed` | `重新扫描失败。` |
| `mainThreadDispatchFailed`, `exportFailed`, `exportWorkerFailed` | `导出失败。` |
| `invalidCaller`, unknown shape/code, initialization failure | `操作不可用，请重试。` |
| no search results | `未找到应用` |
| local activation-refused notice | `Windows 拒绝了前台切换，已发送启动请求` |
| lifecycle `settingsFailed` notice | `快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。` |
| lifecycle `validationFailed` notice | `本地验证数据操作失败。` |

## Frozen Dependency Evidence

The authorized disposable resolution ran twice from copies of baseline `package.json` and `package-lock.json` with this effective configuration:

```text
Node=v22.21.1
npm=10.9.4
registry=https://registry.npmmirror.com/
package-lock=true
package-lock-only=true
ignore-scripts=true
legacy-peer-deps=false
strict-peer-deps=false
install-links=false
workspaces=null
omit/include empty
```

Commands used in each disposable copy:

```powershell
npm.cmd pkg set "dependencies.react=19.2.7" "dependencies.react-dom=19.2.7" "dependencies.antd=6.5.1" "devDependencies.@types/react=19.2.17" "devDependencies.@types/react-dom=19.2.3"
npm.cmd install --package-lock-only --package-lock=true --ignore-scripts --no-audit --no-fund --registry=https://registry.npmmirror.com/ --legacy-peer-deps=false --strict-peer-deps=false --install-links=false
```

Evidence result:

| Artifact/check | Baseline | Candidate/result |
|---|---|---|
| `package.json` SHA-256 | `22AF7D24B7FEDF3C2064D27F6531E61DA718A24BA30ABD9F772AB34925D06C31` | `B6DEDB7563EFEC6ACF8C4E50CB2DFAA567BCC6140A554A87AC0903C5C122B005` |
| `package-lock.json` SHA-256 | `60AA9748F715772DE37FD39FB3645811D0B80981268CA3253DBC3B4F50E45C06` | `783B63D3F591E40F3F9D3BE6A85AEE2B18C5E0A83982DD8BC8763218CF05EE22` |
| lock entries | 1 root + 149 registry | 1 root + 219 registry |
| non-root delta | - | 70 added, 0 removed, 0 changed |
| entry types | 0 link/file/workspace/other | 0 link/file/workspace/other |
| added licenses | - | 70 MIT, 0 missing |
| added lifecycle scripts | - | 0 |
| baseline lifecycle flag | `fsevents` only, unchanged optional baseline package | unchanged; installs remain scripts-disabled |
| source/integrity | baseline mirror | all 70 use `registry.npmmirror.com` and `sha512` |
| `npm ci --ignore-scripts` | - | exit 0, 159 installed packages |
| `npm ls --all` | - | exit 0; 160 parseable paths including root |
| official full audit | 0 high / 0 critical | 0 high / 0 critical |
| official `--omit=dev` audit | 0 high / 0 critical | 0 high / 0 critical |
| no-plugin TSX proof | - | `tsc --noEmit && vite build` exit 0; Vite transformed 1461 modules |

The official npm endpoint is used only for the read-only advisory query because the configured mirror returns `404 NOT_IMPLEMENTED` for audit. It must not rewrite the configured registry, package source, npm config, package files, or lock.

## File Responsibility Map

| Path | Responsibility |
|---|---|
| `package.json` / `package-lock.json` | Exact reviewed React/AntD runtime and React type resolution only. |
| `tsconfig.json` | Add only `"jsx": "react-jsx"`. |
| `src/protocol.ts` | Frozen DTOs, strict shown-payload parser, classified text records, exact client and core public types. |
| `src/launcher-core.ts` | Single cached-snapshot state owner; readiness, shown/search/execute/hide/settings/IME ownership. No DOM, React, AntD, or Tauri imports. |
| `src/native-input.ts` | Real native input/composition listener binding and trust classification. No client or state machine. |
| `src/launcher-view.tsx` | Thin `useSyncExternalStore` React/AntD rendering, refs, focus/scroll effects, and local intents. No Tauri import. |
| `src/launcher.test.tsx` | Pure-core, native-boundary, React view, source-boundary, and real-adapter tests using existing Vitest/jsdom. |
| `src/styles.css` | Dense unframed layout, AntD token-compatible local CSS, zoom, forced-colors, long-text, focus. |
| `src/main.ts` | Phase-B-only exact Tauri client, view-ready startup order, React root, and idempotent teardown. |

## Contract-To-Task Map

| Design contract | Plan task |
|---|---|
| Exact dependencies, lock, TSX without plugin | Task 1 |
| DTO/event validation, cached snapshot, readiness, shown/search/null/stale ownership | Task 2 |
| Native trust and IME draft/boundary/ordinary ownership | Task 3 history, failed Task 7R2, and candidate Task 7R3 |
| Execute/hide token and settings operations/alias preservation | Task 4 |
| AntD rendering, keyboard/ARIA/focus/scroll/long text/no ID leakage | Task 5 |
| Phase-A build, source and dependency gates | Task 6 |
| Task 6 integration, R1/R2 No-Go evidence, and R3 production gate | Tasks 7, 7R2, and 7R3 |
| Exact real Tauri adapter and listener-before-load | Task 8 |
| Final build, bundle/performance/Narrator/security/trust review | Task 9 |

---

### Task 0: Authenticate Plan Go And Create The Isolated Implementation Worktree

**Files:** None.

**Interfaces:**
- Consumes: written Plan Go naming the exact commit containing this file, Design Go `337cee60`, and clean `main` `1dafdac`.
- Produces: clean `codex/foundation-task-7` at `D:\code\UiPilot_tools\.worktrees\foundation-task-7`, still at `1dafdac`.

- [ ] **Step 1: Authenticate the two existing owner contexts before creating anything**

Run from `D:\code\UiPilot_tools`:

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$baseline = '1dafdacbe921a25fe331633662ea1a000140dcdf'
$designEvidence = '337cee60cdbe0979eef25f2ef3a512920acec0a7'
$mainRoot = (& git rev-parse --show-toplevel).Trim()
Assert-NativeExit 'resolve main root'
if ($mainRoot -ne 'D:/code/UiPilot_tools') { throw 'wrong main root' }
$mainHead = (& git rev-parse HEAD).Trim()
Assert-NativeExit 'resolve main baseline'
if ($mainHead -ne $baseline) { throw 'main baseline moved' }
$mainStatus = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'read main status'
if ($mainStatus.Count -ne 0) { throw 'main is dirty' }
if (-not $env:TASK7_PLAN_GO_SHA -or $env:TASK7_PLAN_GO_SHA -notmatch '^[0-9a-f]{40}$') {
  throw 'TASK7_PLAN_GO_SHA must be the exact SHA named by written Plan Go'
}
git cat-file -e "$designEvidence`:docs/superpowers/specs/2026-07-19-foundation-task-7-keyboard-first-launcher-ui-design.md"
if ($LASTEXITCODE -ne 0) { throw 'approved Task 7 design evidence is unavailable' }
git cat-file -e "$env:TASK7_PLAN_GO_SHA`:docs/superpowers/plans/2026-07-19-foundation-task-7-keyboard-first-launcher-ui.md"
if ($LASTEXITCODE -ne 0) { throw 'approved Task 7 plan evidence is unavailable' }
if (Test-Path -LiteralPath 'D:\code\UiPilot_tools\.worktrees\foundation-task-7') {
  throw 'Task 7 implementation worktree already exists'
}
git show-ref --verify --quiet refs/heads/codex/foundation-task-7
if ($LASTEXITCODE -eq 0) { throw 'Task 7 implementation branch already exists' }
if ($LASTEXITCODE -ne 1) { throw 'cannot authenticate Task 7 branch absence' }
```

Expected: all assertions pass and neither branch nor worktree exists.

- [ ] **Step 2: Create exactly one implementation worktree from the frozen baseline**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
git worktree add -b codex/foundation-task-7 D:\code\UiPilot_tools\.worktrees\foundation-task-7 1dafdacbe921a25fe331633662ea1a000140dcdf
Assert-NativeExit 'create Task 7 worktree'
Set-Location D:\code\UiPilot_tools\.worktrees\foundation-task-7
$implementationHead = (& git rev-parse HEAD).Trim()
Assert-NativeExit 'resolve implementation baseline'
if ($implementationHead -ne '1dafdacbe921a25fe331633662ea1a000140dcdf') { throw 'wrong implementation baseline' }
$implementationBranch = (& git branch --show-current).Trim()
Assert-NativeExit 'resolve implementation branch'
if ($implementationBranch -ne 'codex/foundation-task-7') { throw 'wrong implementation branch' }
$implementationStatus = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'read implementation status'
if ($implementationStatus.Count -ne 0) { throw 'new implementation worktree is dirty' }
```

Expected: clean isolated worktree, no product edit, no tag, and no main change.

---

### Task 1: Apply The Reviewed Dependency And TSX Delta

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `tsconfig.json`

**Interfaces:**
- Consumes: frozen dependency evidence in this plan.
- Produces: exact React/AntD resolution and TypeScript automatic JSX transform; no application behavior.

- [ ] **Step 1: Apply only the five exact package entries and one compiler option**

Use `apply_patch` so the root maps become:

```json
"dependencies": {
  "@tauri-apps/api": "^2.11.1",
  "@tauri-apps/plugin-autostart": "^2.5.1",
  "@tauri-apps/plugin-global-shortcut": "^2.3.2",
  "antd": "6.5.1",
  "react": "19.2.7",
  "react-dom": "19.2.7"
},
"devDependencies": {
  "@tauri-apps/cli": "^2.11.4",
  "@types/react": "19.2.17",
  "@types/react-dom": "19.2.3",
  "jsdom": "^29.1.1",
  "typescript": "^7.0.2",
  "vite": "^8.1.5",
  "vitest": "^4.1.10"
}
```

Add exactly this `compilerOptions` member after `isolatedModules`:

```json
"jsx": "react-jsx"
```

Normalize the structured manifest with the installed npm version, then require that its diff still contains only the five approved entries:

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd pkg fix
Assert-NativeExit 'npm pkg fix'
git diff -- package.json
Assert-NativeExit 'git diff package.json'
```

- [ ] **Step 2: Generate the exact lock with scripts disabled**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$expectedConfig = [ordered]@{
  registry = 'https://registry.npmmirror.com/'
  'package-lock' = 'true'
  'legacy-peer-deps' = 'false'
  'strict-peer-deps' = 'false'
  'install-links' = 'false'
  workspaces = 'null'
  omit = ''
  include = ''
}
foreach ($entry in $expectedConfig.GetEnumerator()) {
  $actual = npm.cmd config get $entry.Key
  Assert-NativeExit "npm config get $($entry.Key)"
  if ($actual.Trim() -ne $entry.Value) { throw "npm config drifted: $($entry.Key)" }
}
if (@(Get-ChildItem Env: | Where-Object { $_.Name -like 'npm_config_*' }).Count) { throw 'unapproved npm_config environment override' }
npm.cmd install --package-lock-only --package-lock=true --ignore-scripts --no-audit --no-fund --registry=https://registry.npmmirror.com/ --legacy-peer-deps=false --strict-peer-deps=false --install-links=false
Assert-NativeExit 'npm package-lock-only install'
```

Expected: exit 0; no `node_modules` requirement; no lifecycle script runs.

- [ ] **Step 3: Run the frozen package/lock oracle**

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$nodeVersion = node --version
Assert-NativeExit 'node --version'
if ($nodeVersion.Trim() -ne 'v22.21.1') { throw 'Node version drifted' }
$npmVersion = npm.cmd --version
Assert-NativeExit 'npm --version'
if ($npmVersion.Trim() -ne '10.9.4') { throw 'npm version drifted' }
$packageHash = (Get-FileHash package.json -Algorithm SHA256).Hash
$lockHash = (Get-FileHash package-lock.json -Algorithm SHA256).Hash
if ($packageHash -ne 'B6DEDB7563EFEC6ACF8C4E50CB2DFAA567BCC6140A554A87AC0903C5C122B005') { throw 'package.json drifted' }
if ($lockHash -ne '783B63D3F591E40F3F9D3BE6A85AEE2B18C5E0A83982DD8BC8763218CF05EE22') { throw 'package-lock.json drifted' }
@'
const lock = require('./package-lock.json')
const entries = Object.entries(lock.packages)
if (lock.lockfileVersion !== 3 || entries.length !== 220) throw new Error('lock inventory drifted')
const root = lock.packages['']
if (!root || root.dependencies.antd !== '6.5.1' || root.dependencies.react !== '19.2.7' ||
    root.dependencies['react-dom'] !== '19.2.7' || root.devDependencies['@types/react'] !== '19.2.17' ||
    root.devDependencies['@types/react-dom'] !== '19.2.3') throw new Error('root dependency map drifted')
for (const [path, entry] of entries) {
  if (path === '') continue
  if (entry.link || typeof entry.resolved !== 'string' || !entry.resolved.startsWith('https://registry.npmmirror.com/') ||
      typeof entry.integrity !== 'string' || !entry.integrity.startsWith('sha512-')) {
    throw new Error(`unapproved lock entry: ${path}`)
  }
}
'@ | node
Assert-NativeExit 'package-lock Node oracle'
```

Expected: hashes and all 219 non-root registry entries match; zero link/file/workspace/Git type.

- [ ] **Step 4: Install and rerun supply-chain checks**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
function Assert-Audit([string]$label, [object[]]$lines) {
  try { $audit = ($lines -join "`n") | ConvertFrom-Json } catch { throw "$label returned invalid JSON" }
  if ($null -eq $audit.metadata -or $null -eq $audit.metadata.vulnerabilities) { throw "$label omitted vulnerability metadata" }
  $names = @($audit.metadata.vulnerabilities.PSObject.Properties.Name)
  if ('high' -notin $names -or 'critical' -notin $names) { throw "$label omitted high/critical counts" }
  if ([int]$audit.metadata.vulnerabilities.high -ne 0 -or [int]$audit.metadata.vulnerabilities.critical -ne 0) {
    throw "$label reported high/critical vulnerabilities"
  }
}
npm.cmd ci --package-lock=true --ignore-scripts --no-audit --no-fund --registry=https://registry.npmmirror.com/ --legacy-peer-deps=false --strict-peer-deps=false --install-links=false
Assert-NativeExit 'npm ci'
npm.cmd ls --all
Assert-NativeExit 'npm ls --all'
$fullAuditJson = @(npm.cmd audit --audit-level=high --registry=https://registry.npmjs.org/ --json)
Assert-NativeExit 'npm audit full'
Assert-Audit 'npm audit full' $fullAuditJson
$prodAuditJson = @(npm.cmd audit --omit=dev --audit-level=high --registry=https://registry.npmjs.org/ --json)
Assert-NativeExit 'npm audit omit=dev'
Assert-Audit 'npm audit omit=dev' $prodAuditJson
```

Expected: all commands exit 0; both audits report `high: 0`, `critical: 0`; no package/lock diff after `npm ci`.

- [ ] **Step 5: Commit only the dependency/compiler delta**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
git diff --check
Assert-NativeExit 'git diff --check'
git add package.json package-lock.json tsconfig.json
Assert-NativeExit 'git add dependency delta'
$staged = @(git diff --cached --name-only)
Assert-NativeExit 'git staged path inventory'
if (@(Compare-Object -CaseSensitive @('package-lock.json','package.json','tsconfig.json') @($staged | Sort-Object -CaseSensitive -Unique)).Count) {
  throw "dependency staged scope drifted: $($staged -join ', ')"
}
git commit -m "build: add reviewed React launcher dependencies"
Assert-NativeExit 'git commit dependency delta'
```

Expected staged paths: exactly the three files above.

---

### Task 2: Build The Cached-Snapshot Protocol And Launcher Core

**Files:**
- Modify: `src/protocol.ts`
- Create: `src/launcher-core.ts`
- Create: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: exact eight-command/event DTOs frozen below.
- Produces: `createLauncherCore(client)`, stable `getSnapshot`/`subscribe`, `start`, `shown`, `text`, `retireControl`, `keyDown`, `requestHide`, and `destroy` methods used by later tasks.

- [ ] **Step 1: Write RED tests for protocol, store identity, startup, shown, search, execute, and hide**

Create `src/launcher.test.tsx` with one local fake client and deferred Promise helper. The first test group must execute these rows:

```ts
const storeRows = [
  ['repeat getSnapshot', 'sameRef', 0],
  ['one accepted shown', 'newRef', 1],
  ['invalid shown', 'sameRef', 0],
  ['stale search completion', 'sameRef', 0],
] as const

const shownRows = [
  ['launcher', '', 0, 0],
  ['launcher', 'calc', 1, 1],
  ['settings', 'calc', 0, 0],
] as const
```

Assert directly, without snapshots:

```ts
expect(core.getSnapshot()).toBe(core.getSnapshot())
expect(core.getSnapshot).toBe(core.getSnapshot)
expect(core.subscribe).toBe(core.subscribe)
const unsubscribe = core.subscribe(listener)
unsubscribe()
unsubscribe()
expect(listener).toHaveBeenCalledTimes(expectedNotifications)
```

Also prove:

- listener registration resolves before the fake client's first `loadSettings` call;
- listener registration failure makes zero load calls and publishes the fixed initialization status once;
- load failure after listener registration keeps launcher search available and exposes only the fixed settings reload path;
- destroy while listener registration is pending makes zero load calls and immediately invokes a late unlisten once; repeated destroy/unsubscribe is no-op;
- `retireControl(unknownControl)` is a same-snapshot/zero-notification/zero-client-call no-op and repeated retirement is idempotent;
- a shown callback fired while load is pending is accepted;
- invalid extra/missing/wrong-type event fields are zero-effect;
- protocol property assertions reject `path`, `executable`, `useCounts`, PID, HWND, action, and any ninth command/event name;
- each valid shown replaces invocation, increments epoch, resets sequence/request/results/selection, preserves only committed query, and applies notice priority;
- preserved non-empty launcher query sends one sequence-`1` search; empty query sends zero; settings sends zero;
- ordinary empty edit increments sequence and clears state with zero search; non-empty sends exact `{ query, invocationId, querySequence }`;
- search start clears ordinary status and mapped results; current `null` releases pending without replacing state; stale response/error/null is zero-effect;
- current non-empty response selects index 0; ArrowUp/ArrowDown wrap; no-results arrows are no-op;
- Enter sends only `{ requestId, resultId }` once; execute success never calls hide; a second pending Enter is no-op;
- launcher/settings Escape and settings close share one hide owner; current rejection preserves view/query/focus and fixed status; stale completion cannot release newer ownership.

- [ ] **Step 2: Run RED and authenticate the missing modules**

```powershell
npm.cmd test -- --run src/launcher.test.tsx
```

Expected: FAIL because `./launcher-core` and the added protocol exports/interfaces do not exist; baseline `ResultItem` / `SearchResponse` continue to load, and the failure is never `running 0 tests`.

- [ ] **Step 3: Add the exact protocol surface**

Historical Phase-A note: do not rerun or edit the reviewed Task 2 commit. The displayed union is Phase-A history; R2 was
restored after its failed gate, and Task 7R3 defines the only candidate replacement. All non-IME DTO/client seams remain
exact.

```ts
export type ControlKey = number
export type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }

export interface LauncherClient {
  listenShown(handler: (payload: unknown) => void): Promise<() => void>
  searchApps(input: { query: string; invocationId: string; querySequence: number }): Promise<SearchResponse | null>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  loadSettings(): Promise<SettingsView>
  saveSettings(input: { settings: UserSettingsUpdate }): Promise<void>
  rescanApps(): Promise<void>
  exportValidationData(): Promise<ExportOutcome>
  clearValidationData(): Promise<void>
  hideLauncher(): Promise<void>
}

export interface ViewResult {
  key: number
  title: string
  subtitle?: string
}

export interface LauncherSnapshot {
  view: 'launcher' | 'settings'
  viewEpoch: number
  invocationId?: string
  queryControl: ControlKey
  query: string
  queryControlValue: string
  querySequence: number
  results: readonly ViewResult[]
  selectedIndex: number
  searchPending: boolean
  executePending: boolean
  hidePending: boolean
  shownNotice?: string
  status: string
}
```

Copy `ResultItem`, `SearchResponse`, `AppAliasTarget`, `SettingsView`, `UserSettingsUpdate`, `ExecuteOutcome`, `ExportOutcome`, `CommandErrorCode`, `CommandError`, `ShowTarget`, `LifecycleNotice`, and `LauncherShown` verbatim from Design Go. Add one strict `parseLauncherShown(value: unknown): LauncherShown | null` that accepts exactly two targets times the three notice values (`null`, `settingsFailed`, `validationFailed`) and rejects missing, extra, inherited, array, and wrong-type fields.

- [ ] **Step 4: Implement one cached-snapshot core, not a reducer framework**

Create `src/launcher-core.ts` with this public shape:

```ts
export interface LauncherCore {
  readonly getSnapshot: () => LauncherSnapshot
  readonly subscribe: (listener: () => void) => () => void
  readonly start: () => Promise<void>
  readonly failInitialization: () => void
  readonly shown: (payload: unknown) => void
  readonly text: (record: ClassifiedTextRecord) => void
  readonly retireControl: (control: ControlKey) => void
  readonly keyDown: (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean) => void
  readonly requestHide: () => Promise<void>
  readonly destroy: () => void
}

export function createLauncherCore(client: LauncherClient): LauncherCore
```

Keep one private mutable model and one cached frozen snapshot. Route every real mutation through exactly:

```ts
function publish(mutated: boolean): void {
  if (!mutated) return
  snapshot = Object.freeze(projectSnapshot(model))
  for (const listener of [...listeners]) listener()
}
```

`projectSnapshot` allocates and freezes every exposed nested object/array as well as the top object; it never exposes a mutable model collection. Construct `getSnapshot` and `subscribe` once. `subscribe` adds one listener and returns a closure with a private `active` boolean so repeated unsubscribe is a no-op. Batch every logical transition before one `publish(true)`. Invalid, stale, untrusted, and equal no-op transitions call no publish. Keep `resultId` and `requestId` in private model mappings; snapshot result keys are local numbers only.

Use exact local fixed UI strings from Design Go. A thrown value is decoded only by checking an own string `code` in the frozen allowlist; ignore its `message` and every other field.

- [ ] **Step 5: Run GREEN and commit the core checkpoint**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx
Assert-NativeExit 'core focused test'
npm.cmd run build
Assert-NativeExit 'core production build'
git diff --check
Assert-NativeExit 'core diff check'
git add src/protocol.ts src/launcher-core.ts src/launcher.test.tsx
Assert-NativeExit 'core git add'
git commit -m "feat: add launcher protocol and ownership core"
Assert-NativeExit 'core git commit'
```

Expected: focused tests and production build pass; commit scope is exactly three files.

---

### Task 3: Add IME Ownership And The Native Trust Adapter

This task is reviewed Phase-A history and is not executable. Do not rewrite its commit. Its value-bearing
`compositionStart`/`compositionUpdate`/`compositionEnd`, suppression, and tombstone behavior was not production-approved;
R2 failed and was restored, and blocked Task 7R3 is the only candidate replacement.

**Files:**
- Create: `src/native-input.ts`
- Modify: `src/protocol.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: `ControlKey`, `ClassifiedTextRecord`, and `LauncherCore.text` from Task 2.
- Produces: the existing `bindNativeTextInput(input, control, emit)` function consumed by the view.

The historical public function shape remains:

```ts
export function bindNativeTextInput(
  input: HTMLInputElement,
  control: ControlKey,
  emit: (record: ClassifiedTextRecord) => void,
): () => void
```

The Phase-A implementation and tests are evidence for corrective RED, not a production model to preserve. R2 was restored
after its failed gate; Task 7R3 proposes the four-file replacement without rerunning a historical task.

---

### Task 4: Complete Settings, Execute, Hide, And Async Ownership

**Files:**
- Modify: `src/protocol.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: cached snapshot, client, IME control keys, and fixed DTOs.
- Produces: settings projection and exact operation methods consumed by the React view.

- [ ] **Step 1: Add the exact view-only settings types and core methods to RED imports**

Extend the public snapshot without exposing `appId` or backend result IDs:

```ts
export interface AliasControlView {
  key: ControlKey
  value: string
}

export interface ApplicationAliasView {
  key: ControlKey
  displayName: string
  aliases: readonly AliasControlView[]
}

export interface SettingsSnapshot {
  hotkey: AliasControlView
  researchId: AliasControlView
  autostart: boolean
  applications: readonly ApplicationAliasView[]
  readOnly: boolean
  operation?: 'load' | 'save' | 'rescan' | 'export' | 'clear'
  clearConfirmation: boolean
  needsReload: boolean
}
```

At this task, extend `LauncherSnapshot` with exactly:

```ts
settings?: SettingsSnapshot
```

Add these exact methods to `LauncherCore`:

```ts
readonly setAutostart: (checked: boolean) => void
readonly addAlias: (application: ControlKey) => void
readonly removeAlias: (application: ControlKey, alias: ControlKey) => void
readonly saveSettings: () => Promise<void>
readonly reloadSettings: () => Promise<void>
readonly rescanApps: () => Promise<void>
readonly exportValidation: () => Promise<void>
readonly beginClearValidation: () => void
readonly cancelClearValidation: () => void
readonly confirmClearValidation: () => Promise<void>
```

- [ ] **Step 2: Write RED settings projection and call-count tests**

Use a fixture with:

```ts
const settingsFixture: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  applications: [
    { appId: 'private-app-id-a', displayName: '同名应用', aliases: ['alpha'] },
    { appId: 'private-app-id-b', displayName: '同名应用', aliases: [] },
  ],
}
```

Assert the snapshot contains two display entries with distinct local keys, A's seed alias and B's blank first editor, but serialized DOM/snapshot JSON contains neither private ID. Editing duplicate B changes only its private vector. Save must call exactly:

```ts
expect(client.saveSettings).toHaveBeenCalledWith({
  settings: {
    hotkey: 'Alt+Space',
    autostart: false,
    aliases: { 'private-app-id-a': ['alpha'], 'private-app-id-b': ['beta'] },
  },
})
```

Cover all exact ownership rows:

- settings target loads all current applications, including empty aliases and duplicate display names;
- a settings shown event received while initial load is pending renders loading; load failure leaves launcher search usable and exposes one explicit settings reload;
- duplicate display names receive only local display suffixes `(1)`, `(2)` while private IDs remain distinct;
- absent `researchId` remains absent on save; non-empty value is exact;
- save success marks stale, reloads once, and replaces form only after reload; save failure preserves edits, marks stale/read-only, and performs no automatic reload;
- rescan success reloads once; rescan failure keeps editable form; post-rescan load failure keeps old form read-only;
- stale save/rescan completion only sets `needsReload`, releases its exact token, and makes no form/status/focus/follow-up change;
- `needsReload` disables edit/add/remove/save/rescan until current reload succeeds; close/reload/export/clear remain available;
- export cancelled/success/error, inline clear confirm/cancel/error, and focus ownership have exact call counts;
- only one settings operation runs; a new shown invalidates visible continuation but not the global operation token;
- settings `关闭` and either-view non-composing Escape call the one hide owner; there is no local launcher switch;
- research ID forwards max length 64 and `[A-Za-z0-9_-]{1,64}` while allowing empty; hotkey remains unparsed;
- execute failure preserves query/view and fixed local status; exact `activationRefusedLaunchRequested` success sets the process-local one-shot notice even after a newer view epoch; later notice priority follows Design Go.
- alias/application removal and form-replacement rows create an active composition owner on the field being removed;
  each operation retires it before model deletion, late composing/ordinary input records for that key are zero-effect, and
  a simultaneously owned unrelated control still finalizes normally;
- form-replacement rows repeat those three ownership states on old hotkey/research/alias controls, then apply a current successful reload/rescan projection; every old control is retired before the old form map is replaced, late records are zero-effect, and unrelated current-view ownership is preserved;
- repeated removal/replacement retirement is idempotent, and every replacement application/alias/text control key is
  strictly fresh and greater than all retired keys, so no composition owner can transfer.

The tests must retain old keys and send late records after deletion/replacement; assert same snapshot reference, zero notification/client call, and no mutation of the fresh control. Add a source-order assertion for both `removeAlias` and the form replacement helper: the exact `retireControl(oldKey)` call must occur before the corresponding map/array deletion or replacement assignment, so lookup-based late-record rejection cannot mask omitted retirement.

- [ ] **Step 3: Run RED and authenticate missing settings methods**

```powershell
npm.cmd test -- --run src/launcher.test.tsx -t "settings|execute|hide"
```

Expected: FAIL for the new methods/state; never zero tests.

- [ ] **Step 4: Implement settings with one private ID map**

Keep `SettingsView` and current form inside the existing core model. For every loaded application allocate stable monotonic local application/alias control keys and store `appId` only in a private `Map<ControlKey, string>`. A removed/replaced control calls `retireControl(control)` before its model entry disappears; a newly added alias gets a fresh key. Saving walks the complete current application projection, drops values exactly equal to `''`, and builds `Record<appId, string[]>`; do not trim, split, deduplicate, parse, or merge hidden absent aliases.

Use one settings operation token and the exact `settingsNeedsReload` state machine from Design Go. Never retry, compensate, or overlap a client call. Decode only fixed error codes.

- [ ] **Step 5: Run GREEN and commit**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx -t "settings|execute|hide"
Assert-NativeExit 'settings focused test'
npm.cmd test -- --run src/launcher.test.tsx
Assert-NativeExit 'settings full test'
npm.cmd run build
Assert-NativeExit 'settings production build'
git add src/protocol.ts src/launcher-core.ts src/launcher.test.tsx
Assert-NativeExit 'settings git add'
git commit -m "feat: add launcher settings ownership"
Assert-NativeExit 'settings git commit'
```

Expected: three-file commit and all tests/build pass.

---

### Task 5: Render The Thin React And Ant Design View

**Files:**
- Create: `src/launcher-view.tsx`
- Modify: `src/launcher.test.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `LauncherCore`, immutable `LauncherSnapshot`, local control keys, and `bindNativeTextInput`.
- Produces: `LauncherView` and `LauncherViewReady` callback used by Task 8.

- [ ] **Step 1: Write RED React DOM/accessibility tests without Testing Library**

Mount with React 19 `createRoot` and `act`, query the real DOM, and assert:

- `LauncherView` renders `ConfigProvider -> App ->` one private surface containing one controlled AntD Input and one explicit list;
- a test-only `window.matchMedia` implementation is called with exact `'(prefers-color-scheme: dark)'`; initial light/dark selects exact `theme.defaultAlgorithm`/`theme.darkAlgorithm`, keeps `token.motion === false`, a real-shaped `change` notification updates the selected algorithm once, and unmount removes the same listener once;
- launcher input is disabled before the first valid launcher shown event and enabled afterward; empty query renders no recommendation or instructional empty state;
- query input has exact combobox semantics: `aria-autocomplete=list`, `aria-controls`, `aria-expanded`, and current local `aria-activedescendant`;
- result container/rows have exact listbox/option/selected semantics, local IDs only, focus remains in input, arrows wrap, and selection change calls `scrollIntoView({ block: 'nearest' })`;
- launcher shown focuses/selects the input; settings shown focuses the heading; composing keys are no-op;
- non-composing Escape prevents native default and routes only to the shared core hide owner; it never switches view locally;
- one polite atomic status region gives lifecycle notice priority, then fixed ordinary status;
- a current empty result renders exact `未找到应用`; current null leaves ordinary status empty; stale null/error changes nothing;
- settings uses Form/Input/Checkbox/Button, persistent labels, exact `关闭` text/accessibility name, and inline clear confirmation;
- no DOM text/attribute/id/form name/React key serialization contains fixture app IDs, result IDs, paths, or icons;
- markup-like titles render literal text and no `dangerouslySetInnerHTML` exists;
- long Chinese/Latin values remain present and controls expose busy/disabled state without structural reflow;
- ref replacement/unmount calls the idempotent native `unbind()` first and then `core.retireControl(control)` exactly once for that binding; it does not destroy the core or own Tauri unlisten;
- `viewReady` fires once only after the real AntD `InputRef.input` exists and native listeners are bound.

Add source assertions that `launcher-view.tsx` imports only the approved AntD values, imports no `@tauri-apps/api` or `@ant-design/icons`, and contains none of `AutoComplete`, `Select`, `Card`, `Modal`, `Popconfirm`, `notification`, `message`, or `dangerouslySetInnerHTML`.

For both ref replacement and unmount, instrument the two cleanup seams and require the exact one-shot order:

```ts
expect(cleanupOrder).toEqual(['native-unbind', `retire:${control}`])
expect(core.destroy).not.toHaveBeenCalled()
```

The test-only AntD partial mock must retain the real exported `theme` algorithms and wrap the real `ConfigProvider` while recording its `theme` prop. Use one controlled `MediaQueryList` seam:

```ts
const scheme = installMatchMedia(false)
const mounted = mountLauncherView()
expect(window.matchMedia).toHaveBeenCalledWith('(prefers-color-scheme: dark)')
expect(lastConfig().algorithm).toBe(theme.defaultAlgorithm)
expect(lastConfig().token?.motion).toBe(false)
act(() => scheme.emit(true))
expect(lastConfig().algorithm).toBe(theme.darkAlgorithm)
mounted.unmount()
expect(scheme.remove).toHaveBeenCalledTimes(1)
expect(scheme.remove.mock.calls[0]).toEqual(['change', scheme.add.mock.calls[0][1]])
```

Run a separate initial-dark row. `installMatchMedia` is test-only; it implements only `matches`, `media`, and paired `addEventListener`/`removeEventListener` needed by production.

- [ ] **Step 2: Run RED**

```powershell
npm.cmd test -- --run src/launcher.test.tsx -t "React view|accessibility|source boundary"
```

Expected: FAIL because `launcher-view.tsx` does not exist; never zero tests.

- [ ] **Step 3: Implement the smallest view**

Create:

```ts
export interface LauncherViewProps {
  core: LauncherCore
  onReady: (result: 'ready' | 'failed') => void
}

export function LauncherView({ core, onReady }: LauncherViewProps): React.JSX.Element
```

Use `useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)` with no React state mirror or context. `LauncherView` is the composition root: it renders `ConfigProvider -> App ->` the private view surface, so `src/main.ts` never owns theme state. Create one mount-stable `MediaQueryList` from exact `window.matchMedia('(prefers-color-scheme: dark)')`, initialize one presentation-only boolean from `matches`, subscribe to its `change` event, and remove the same listener on unmount. Select exact `dark ? theme.darkAlgorithm : theme.defaultAlgorithm` and keep `token: { motion: false }`; do not mirror this boolean into the protocol core or create a second store.

Import `bindNativeTextInput` directly; a controlled text input's React `onChange` is inert and never calls the core. Bind the underlying native input in a ref/effect and return an idempotent cleanup that executes exactly `unbind(); core.retireControl(control)` in that order. Signal `ready` exactly once after successful binding or `failed` exactly once if binding throws; never include the thrown value. Use layout/effects only for the frozen focus/select/nearest-scroll actions and guard each by current epoch/local key.

Render result rows explicitly; AntD must not own filtering, selection, popup, active descendant, or IME. Use local keys from the snapshot. AntD `Form` is layout/submit markup only: do not create `Form.useForm` state and do not give `Form.Item` a data-store `name`; controlled Inputs receive only core-local `id`/HTML `name`. Do not render `icon` URLs; use one CSS `aria-hidden` generic mark. Use `Alert`/`Spin` only inside the one feedback region and disable global motion in ConfigProvider.

- [ ] **Step 4: Replace the baseline CSS with the approved dense layout**

Use AntD tokens plus local CSS for the fixed `720 x 420` surface: search/status/result bands, one settings scroll region, `min-width: 0`, `overflow-wrap: anywhere`, stable control heights, visible focus, `prefers-color-scheme`, `forced-colors`, and vertical scrolling at 100/150/200% zoom. Keep radius at 6px or less, no cards/hero/gradient/decorative blobs/animation, no viewport-scaled font, negative letter spacing, horizontal overflow, or remote font.

- [ ] **Step 5: Run GREEN and commit**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx -t "React view|accessibility|source boundary"
Assert-NativeExit 'React view focused test'
npm.cmd test -- --run src/launcher.test.tsx
Assert-NativeExit 'React view full test'
npm.cmd run build
Assert-NativeExit 'React view production build'
git add src/launcher-view.tsx src/launcher.test.tsx src/styles.css
Assert-NativeExit 'React view git add'
git commit -m "feat: render Ant Design launcher views"
Assert-NativeExit 'React view git commit'
```

Expected: exact three-file commit; DOM tests and build pass.

---

### Task 6: Complete The Pre-Task-6 Phase-A Gate

**Files:** No new product file. Fixes may touch only the nine Phase-A allowlist paths; `src/main.ts` remains byte-identical to `1dafdac`.

**Interfaces:**
- Consumes: Tasks 1-5 commits.
- Produces: independently reviewed package/core/native/view checkpoint; no real lifecycle claim.

- [ ] **Step 1: Run the complete frontend gate from a fresh install**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
function Assert-Audit([string]$label, [object[]]$lines) {
  try { $audit = ($lines -join "`n") | ConvertFrom-Json } catch { throw "$label returned invalid JSON" }
  if ($null -eq $audit.metadata -or $null -eq $audit.metadata.vulnerabilities) { throw "$label omitted vulnerability metadata" }
  $names = @($audit.metadata.vulnerabilities.PSObject.Properties.Name)
  if ('high' -notin $names -or 'critical' -notin $names) { throw "$label omitted high/critical counts" }
  if ([int]$audit.metadata.vulnerabilities.high -ne 0 -or [int]$audit.metadata.vulnerabilities.critical -ne 0) {
    throw "$label reported high/critical vulnerabilities"
  }
}
npm.cmd ci --package-lock=true --ignore-scripts --no-audit --no-fund --registry=https://registry.npmmirror.com/ --legacy-peer-deps=false --strict-peer-deps=false --install-links=false
Assert-NativeExit 'npm ci'
npm.cmd test
Assert-NativeExit 'npm test'
npm.cmd run build
Assert-NativeExit 'npm build'
npm.cmd ls --all
Assert-NativeExit 'npm ls --all'
$fullAuditJson = @(npm.cmd audit --audit-level=high --registry=https://registry.npmjs.org/ --json)
Assert-NativeExit 'npm audit full'
Assert-Audit 'npm audit full' $fullAuditJson
$prodAuditJson = @(npm.cmd audit --omit=dev --audit-level=high --registry=https://registry.npmjs.org/ --json)
Assert-NativeExit 'npm audit omit=dev'
Assert-Audit 'npm audit omit=dev' $prodAuditJson
if (Test-Path dist\security-probe.html) { throw 'production dist contains security probe HTML' }
if (@(Get-ChildItem dist -Recurse -Force | Where-Object { $_.Name -match 'security-probe' }).Count) {
  throw 'production dist contains security probe artifact'
}
```

Expected: all commands exit 0, audits remain 0 high/critical, no probe output.

- [ ] **Step 2: Run exact Phase-A source and scope oracles**

```powershell
$baseline = '1dafdacbe921a25fe331633662ea1a000140dcdf'
$expected = @(
  'package.json','package-lock.json','tsconfig.json','src/launcher-core.ts','src/native-input.ts',
  'src/launcher-view.tsx','src/launcher.test.tsx','src/protocol.ts','src/styles.css'
) | Sort-Object -CaseSensitive -Unique
$changed = @(& git diff --name-only "$baseline..HEAD")
if ($LASTEXITCODE -ne 0) { throw 'Phase A changed-path inventory failed' }
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
$scopeDelta = @(Compare-Object -CaseSensitive $expected $changed)
if ($scopeDelta.Count) { throw "Phase A exact path set drifted: $($scopeDelta -join '; ')" }
if (@($changed | Where-Object { $_ -ceq 'src/main.ts' }).Count -ne 0) { throw 'Phase A src/main.ts count is not zero' }
if (@($changed | Where-Object { $_ -ceq 'src/launcher-core.test.ts' }).Count -ne 0) { throw 'split core test file is forbidden' }
if (Test-Path -LiteralPath 'src/launcher-core.test.ts') { throw 'split core test file exists anywhere in Phase A worktree' }
$view = Get-Content -Raw -Encoding utf8 src/launcher-view.tsx
$core = Get-Content -Raw -Encoding utf8 src/launcher-core.ts
$native = Get-Content -Raw -Encoding utf8 src/native-input.ts
foreach ($forbidden in @('@tauri-apps/api','@ant-design/icons','dangerouslySetInnerHTML')) {
  if ($view.Contains($forbidden)) { throw "view source boundary failed: $forbidden" }
}
if ($view -cmatch '(?m)\b(?:AutoComplete|Select|Card|Modal|Popconfirm)\b') { throw 'forbidden AntD component' }
if ($core -match '(?m)(?:from\s+|import\()\s*["''](?:react|antd|@tauri-apps/api)') { throw 'core imports presentation/Tauri' }
foreach ($required in @('isTrusted','inputType','insertCompositionText','addEventListener','removeEventListener')) {
  if (-not $native.Contains($required)) { throw "native trust boundary missing: $required" }
}
git diff --check "$baseline..HEAD"
if ($LASTEXITCODE -ne 0) { throw 'Phase A diff check failed' }
```

Expected: the committed diff is exactly the nine Phase-A paths, `src/launcher.test.tsx` is the sole test path, `src/main.ts` count is zero, `src/launcher-core.test.ts` is absent, and core/view/native boundaries remain intact.

- [ ] **Step 3: Request written Phase-A Code/Dependency Go and wait for both dependencies**

Send the audit thread the branch, worktree, baseline, exact Phase-A HEAD, ordered commits/files, dependency hashes/70-entry evidence, fresh command results, exact nine-path comparison, and clean status. Explicitly state that Vite still uses the baseline `main.ts` entry, so Phase A makes no UI bundle/performance or real-lifecycle claim. Request written **Task 7 Phase-A Code Go + Dependency Go** bound to that exact HEAD; do not request final Task 7 Code Go.

Task 7 must hold both written prerequisites before Task 7 below begins: (1) Phase-A Code/Dependency Go naming exact `TASK7_PHASE_A_GO_SHA`, and (2) Task 6 Code Go plus approved local integration naming exact `TASK6_MAIN_SHA`. A report without either written Go is not authorization to integrate.

---

### Task 7: Completed R1 Pre-TDD Diagnostic Evidence (Non-Executable)

**Files:**
- Historical temporary uncommitted modification: `src/main.ts` (restored; do not reapply under this revision)
- No committed product file.

**Interfaces:**
- Consumed: diagnostic-only Go at `a036c00`, Phase-A Go `f7016634`, Task 6 integration `a8626e7`, and clean merge
  `28f058be`.
- Produced: Pass A No-Go evidence and clean recovery at `28f058be`; no product source or passing production-adapter claim.

- [x] **Step 1: Authenticated the exact clean recovery point and reviewed ancestors**

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$phaseA = 'f7016634046303b32a750d0942fb549f777028d6'
$task6 = 'a8626e72e97a5caa924333e6d6545efe9cd2e6d0'
$recovery = '28f058be94d4fadb0b490b08f4bb5f99a77c08f0'
$head = (& git rev-parse HEAD).Trim()
Assert-NativeExit 'resolve recovery HEAD'
if ($head -ne $recovery) { throw 'product worktree is not at the approved R1 recovery point' }
$task7Status = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'read recovery status'
if ($task7Status.Count -ne 0) { throw 'Task 7 recovery worktree is dirty' }
git merge-base --is-ancestor $phaseA HEAD
Assert-NativeExit 'Phase-A Go ancestry'
git merge-base --is-ancestor $task6 HEAD
Assert-NativeExit 'Task 6 integration ancestry'
$parents = @(& git show -s --format=%P HEAD)
Assert-NativeExit 'resolve recovery parents'
if ($parents.Count -ne 1 -or ($parents[0] -split ' ') -notcontains $phaseA -or ($parents[0] -split ' ') -notcontains $task6) {
  throw 'recovery commit does not have the two approved parents'
}
$task5 = '1dafdacbe921a25fe331633662ea1a000140dcdf'
foreach ($path in @(
  'package.json','package-lock.json','tsconfig.json','index.html','vite.config.ts','src/main.ts','src/protocol.ts',
  'src/styles.css','security-probe.html','src/security-probe.ts'
)) {
  $before = (& git rev-parse "$task5`:$path").Trim()
  Assert-NativeExit "resolve Task 5 blob $path"
  $after = (& git rev-parse "$task6`:$path").Trim()
  Assert-NativeExit "resolve Task 6 blob $path"
  if ($before -ne $after) { throw "Task 6 changed Task 7-owned/frozen frontend input: $path" }
}
```

Expected: clean exact recovery commit with the reviewed Phase-A and Task 6 parents; every pre-existing
frontend/package/protocol blob and both frozen security-probe blobs remain identical to `1dafdac`. No merge is rerun.

- [x] **Step 2: Applied and removed the temporary reachable R1 diagnostic harness**

The following is historical audit evidence only; do not rerun it. Pass A mounted the approved view/core unchanged and added read-only
native capture listeners for `beforeinput`, `input`, and composition events. It records only sequence/type/class/
`isTrusted`/`inputType`/`InputEvent.isComposing`/control category plus reviewed non-text fields. It does not emit a new
record, suppress an event, or change core/client behavior. Use this gate client:

```ts
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { createLauncherCore, type LauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import type { ClassifiedTextRecord, LauncherClient, SettingsView } from './protocol'

const gateClient: LauncherClient = {
  listenShown: async (handler) => {
    const { listen } = await import('@tauri-apps/api/event')
    return listen<unknown>('launcher://shown', (event) => handler(event.payload))
  },
  loadSettings: async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<SettingsView>('load_settings')
  },
  searchApps: async () => null,
  executeResult: async () => ({ status: 'launchRequested' }),
  saveSettings: async () => undefined,
  rescanApps: async () => undefined,
  exportValidationData: async () => ({ status: 'cancelled' }),
  clearValidationData: async () => undefined,
  hideLauncher: async () => undefined,
}
```

Wrap only the classified core entry point; never record `record.value` or the numeric key:

```ts
function classifiedTraceCore(core: LauncherCore): LauncherCore {
  return Object.freeze({
    ...core,
    text(record: ClassifiedTextRecord) {
      const category = record.control === core.getSnapshot().queryControl ? 'launcher' : 'settings'
      console.info(JSON.stringify({ kind: record.kind, category }))
      core.text(record)
    },
  })
}
```

After the view-ready callback, add these capture listeners; they never write `data`, `target.id`, or `target.value`:

```ts
let rawSequence = 0
const rawTypes = ['compositionstart', 'compositionupdate', 'compositionend', 'beforeinput', 'input'] as const
function traceRaw(event: Event): void {
  if (!(event.target instanceof HTMLInputElement)) return
  const launcher = event.target.getAttribute('role') === 'combobox'
  console.info(JSON.stringify({
    sequence: ++rawSequence,
    category: launcher ? 'launcher' : 'settings',
    type: event.type,
    eventClass: event.constructor.name,
    isTrusted: event.isTrusted,
    inputType: event instanceof InputEvent ? event.inputType : null,
    isComposing: event instanceof InputEvent ? event.isComposing : null,
    cancelable: event.cancelable,
    bubbles: event.bubbles,
    composed: event.composed,
    targetRangeCount: event instanceof InputEvent && typeof event.getTargetRanges === 'function'
      ? event.getTargetRanges().length
      : null,
  }))
}
for (const type of rawTypes) document.addEventListener(type, traceRaw, true)
```

Pass A installed no classifier/core candidate. It collected the raw matrix only. Pass B was not run because no permitted
non-text discriminator existed; that stop is final evidence for R2 and cannot be reopened by adding inference.

Create the root explicitly from the existing element; do not modify `index.html`:

```ts
const appElement = document.querySelector<HTMLElement>('#app')
if (!appElement) throw new Error('Missing application root')
const root = createRoot(appElement)
```

The harness startup order is exact:

```ts
const core = createLauncherCore(gateClient)
const viewReady = new Promise<void>((resolve, reject) => {
  root.render(createElement(LauncherView, {
    core: classifiedTraceCore(core),
    onReady: (result) => result === 'ready' ? resolve() : reject(new Error('view initialization failed')),
  }))
})
await viewReady
await core.start() // listen resolves first; then no-argument load_settings
```

No Tauri API other than event `listen` and core `invoke('load_settings')` may appear. The local fake methods above must never call Tauri. Do not log event payload, settings result, `data`, input value, query, alias, application name, ID, or backend payload.

- [x] **Step 3: Built and ran Pass A; stopped before Pass B**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd run build
Assert-NativeExit 'metadata harness build'
npm.cmd run tauri dev
```

The interactive `tauri dev` process may start only after the checked build succeeds. If it exits before the evidence sequence is complete, the gate fails; its later operator-requested shutdown is not treated as build evidence.

The normal external Task 6 launcher request and real tray settings entry exercised launcher/settings natural commits,
cancel, and independent same-value edits. Sanitized evidence retained only versions, sequence, category, event kind/class,
`isTrusted`, `inputType`, `InputEvent.isComposing`, reviewed non-text fields, classified kind/count, and fixed booleans.
It retained no text/data/value/query/alias/ID/timestamp/backend payload.

Pass A found that a same-IME trusted non-composing `insertText` tail and later independent same-value ordinary
`insertText` have identical permitted metadata. Settings natural cancel also returned
`cancelRestoredKnownValue=false`. Therefore Pass A is No-Go, Pass B remained unrun, and no discriminator/boundary route is
available. Do not add timer, microtask, value/order heuristic, suppression/tombstone, native bridge, dependency, command,
or permission.

- [x] **Step 4: Removed the harness and proved zero residue**

Before restore, require the only worktree diff to be `src/main.ts` and manually inspect it as the known harness. Then:

```powershell
$dirty = @(& git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'metadata harness diff inventory failed' }
if ($dirty.Count -ne 1 -or $dirty[0] -ne 'src/main.ts') { throw "unexpected metadata-harness diff: $($dirty -join ', ')" }
git restore --worktree -- src/main.ts
if ($LASTEXITCODE -ne 0) { throw 'metadata harness restore failed' }
$harnessStatus = @(& git status --porcelain=v1 --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'metadata harness status failed' }
if ($harnessStatus.Count -ne 0) { throw 'metadata harness was not fully removed' }
```

Result: clean product worktree at `28f058be`, clean docs at `a036c00`, no diagnostic listeners/processes, and no committed
harness/capability/config/security change. R2 later failed its post-GREEN gate and was restored; corrective TDD now
requires new written R3 Design/Plan/Security Go.

---

### Task 7R2: Historical Failed Unified Trusted Input Attempt

**Historical and permanently non-executable:** Steps 1-5 ran under the former R2 Go. The production gate disproved the
model because natural commits repeatedly had no trusted non-composing tail. The four-file diff was recorded as patch-id
`2c94cefaef42706394289e79af0119744eb986c4` and restored without a stash or commit. Steps 6/7 are revoked. The code and
commands below remain audit history only and must not be rerun.

**Files:**
- Modify: `src/protocol.ts`
- Modify: `src/native-input.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`
- Temporary uncommitted modification: `src/main.ts` for the production-adapter rerun only

**Interfaces:**
- Consumes: written R2 Design/Plan/Security Go bound to the clean recovery SHA and this unified trusted-input model.
- Produces: one append-only corrective commit; no dependency, command, permission, Rust, or real-adapter change.

- [x] **Step 1: Authenticate the written diagnostic checkpoint and clean recovery HEAD**

Require the audit response to grant R2 Design/Plan/Security Go and name the exact recovery SHA. Then run:

```powershell
$ErrorActionPreference = 'Stop'
$recovery = '28f058be94d4fadb0b490b08f4bb5f99a77c08f0'
$head = (& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'R2 recovery HEAD resolution failed' }
if ($head -ne $recovery) { throw 'R2 corrective TDD did not start at approved recovery HEAD' }
$status = @(& git status --porcelain=v1 --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'R2 recovery status failed' }
if ($status.Count) { throw 'R2 recovery worktree is dirty' }
```

- [x] **Step 2: Add the corrective RED before changing protocol/core/adapter**

Modify only `src/launcher.test.tsx`. Add exact `describe('R2 unified trusted input', ...)` rows before changing protocol,
core, or adapter. Run the five behavioral rows below once for the launcher query control and once for a settings alias
control: complete commit tail, cancel non-composing input, standalone untrusted end, no-tail lifecycle retirement, and a
later independent same-value ordinary edit.

- Type assertions require only zero-text `compositionStart`, value-bearing `compositionInput`, and value-bearing
  `ordinaryInput`; they require the obsolete update/end/boundary variants to be `never`.
- Launcher commit: start retires the old search and initializes committed draft; trusted composing input updates only
  `queryControlValue`; the first trusted non-composing tail clears the owner, commits once, increments sequence once, and
  makes exactly one applicable search.
- Launcher cancel: after a composing draft, one trusted non-composing `deleteContentBackward` runs the same ordinary path
  once. It is not inferred as cancel and is never suppressed. A later same-value `insertText` is one additional edit and
  applicable search.
- Settings commit/cancel rows use alias and hotkey controls: composing input changes draft with zero Rust calls; trusted
  non-composing input commits locally once with zero search/execute/hide/save; later same-value input commits once more.
- Standalone untrusted raw `compositionend`, duplicate untrusted end, and every other synthetic raw event emit zero
  records and make zero snapshot/client change through the real jsdom listener boundary. For each control category,
  establish the core owner directly, dispatch the raw end through the bound element, then prove a later direct matching
  composing input still updates that owner before ordinary input clears it; the end did not clear or commit ownership.
- No-start trusted-classified composing input, stale generation, retired control, and replaced control preserve snapshot
  identity and make zero calls.
- No-tail launcher shown and view unbind restore committed control text and discard draft. No-tail settings removal/form
  replacement retires before deletion, rejects late records, and preserves unrelated ownership. Cleanup itself makes zero
  composition-caused search/settings commit; normal shown auto-search remains governed by its existing contract. After
  shown clears the owner, a trusted non-composing input from the still-current query binding is a normal edit, not stale;
  only an old retired/replaced binding is rejected.
- For launcher and settings, `core.keyDown(key, true)` with Enter/arrows/Escape preserves the exact snapshot and makes zero
  client calls. Then send a separate trusted non-composing `deleteContentBackward`: it takes the ordinary path exactly
  once, making one applicable launcher search or one settings-local commit with zero Rust calls. Non-composing Escape
  continues to invoke only the shared `hide_launcher` owner. No script-only DOM mutation can enter core state without a
  trusted InputEvent.

```powershell
npm.cmd test -- --run src/launcher.test.tsx -t "R2 unified trusted input"
if ($LASTEXITCODE -ne 1) { throw 'R2 RED did not fail exactly as expected' }
```

Expected: named tests execute and fail on the old value-bearing start/update/end and suppression/tombstone behavior,
never zero tests or an unrelated module-load failure. Inspect the failures before GREEN.

- [x] **Step 3: Implement the minimum four-file GREEN**

In `src/protocol.ts`, replace the IME union with exactly:

```ts
export type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
```

In `src/native-input.ts`, attach only `compositionstart` and `input` listeners. Same-target trusted start emits the
zero-text record. Same-target trusted input emits `compositionInput` when `event.isComposing`, otherwise
`ordinaryInput`; its value comes from that bound input and its input type is payload only. Every untrusted/wrong-target/
post-unbind event emits nothing. No update/end listener or CompositionEvent value/data read remains. Unbind removes the
two listeners idempotently.

In `src/launcher-core.ts`, replace suppression/tombstone/end branches with one owner
`{ control, viewEpoch, invocationId, generation, lastTrustedDraft }`. Start initializes from committed state and retires
the old search. Matching composing input replaces draft only. Every ordinary input clears a matching owner then calls the
existing `commitControl` once, even when values are equal; with no owner it remains the same ordinary path. Shown,
replacement, and retirement restore committed draft before invalidating ownership. Do not add a timer, microtask,
value/order heuristic, generalized event framework, or new public API.

- [x] **Step 4: Run focused/full GREEN, type/build checks, and exact source oracles**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx -t "R2 unified trusted input|native trust|IME ownership"
Assert-NativeExit 'R2 focused tests'
npm.cmd test
Assert-NativeExit 'R2 full tests'
npm.cmd run build
Assert-NativeExit 'R2 production build'
$changed = @(& git diff --name-only)
Assert-NativeExit 'R2 changed path inventory'
$expected = @('src/launcher-core.ts','src/launcher.test.tsx','src/native-input.ts','src/protocol.ts') |
  Sort-Object -CaseSensitive -Unique
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
if (@(Compare-Object -CaseSensitive $expected $changed).Count) { throw 'R2 corrective path set drifted' }
$protocol = Get-Content -Raw -Encoding utf8 src/protocol.ts
$native = Get-Content -Raw -Encoding utf8 src/native-input.ts
foreach ($forbidden in @('compositionUpdate','compositionEnd','compositionBoundary')) {
  if ($protocol.Contains($forbidden)) { throw "R2 protocol retained $forbidden" }
}
foreach ($required in @('compositionStart','compositionInput','ordinaryInput')) {
  if (-not $protocol.Contains($required)) { throw "R2 protocol missing $required" }
}
foreach ($required in @('isTrusted','isComposing','compositionstart','addEventListener','removeEventListener')) {
  if (-not $native.Contains($required)) { throw "R2 native source missing $required" }
}
foreach ($forbidden in @('insertCompositionText','compositionupdate','compositionend','compositionUpdate','compositionEnd','compositionBoundary')) {
  if ($native.Contains($forbidden)) { throw "R2 native source retained $forbidden" }
}
git diff --check
Assert-NativeExit 'R2 diff check'
```

- [x] **Step 5: Rerun the complete WebView2 matrix through production adapter/core (No-Go)**

Temporarily apply the reviewed Task 7 harness to `src/main.ts` without a parallel classifier. Keep sanitized raw trace plus
`classifiedTraceCore`. For launcher and settings, repeat natural commit, natural cancel, no-tail shown/unbind, standalone
untrusted end, and later independent same-value ordinary input. Required counts are exact: composing input changes draft
with zero applicable call; the first trusted non-composing input makes one ordinary edit and one normally applicable call;
no-tail cleanup and untrusted end make zero composition-caused edit/call; later same-value input adds one edit/call.
No-start/stale/retired/replaced-control cases remain zero-effect. Any mismatch stops without commit and returns to written
design/security review.

Restore only `src/main.ts`, then require the dirty set to return to the exact four corrective files:

```powershell
$expected = @('src/launcher-core.ts','src/launcher.test.tsx','src/native-input.ts','src/protocol.ts') |
  Sort-Object -CaseSensitive -Unique
git restore --worktree -- src/main.ts
if ($LASTEXITCODE -ne 0) { throw 'R2 production harness restore failed' }
$changed = @(& git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'R2 post-harness inventory failed' }
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
if (@(Compare-Object -CaseSensitive $expected $changed).Count) { throw 'R2 post-harness path set drifted' }
```

- [ ] **Step 6: Revoked - do not commit the invalid R2 diff**

Not executed. The invalid diff was restored to the recovery HEAD without a stash or commit. No R2 product artifact may be
reconstructed from the historical steps above.

- [ ] **Step 7: Revoked - no committed R2 gate or code/security request**

Not executed. No R2 commit, committed gate, Code Go request, or Security Go request exists. Task 8 remains blocked by R3.

---

### Task 7R3: Finalize Trusted Draft With One Correlated Boundary

**Non-executable pending review:** this candidate requires new written R3 Design Go, Plan Go, and Security Go bound to the
clean recovery HEAD. The prior R2 approvals do not transfer. No checkbox below may run before all three approvals.

**Files:**
- Modify: `src/protocol.ts`
- Modify: `src/native-input.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`
- Temporary uncommitted modification: `src/main.ts` for the production WebView2 matrix only

**Interfaces:**
- Consumes: clean recovery `28f058be94d4fadb0b490b08f4bb5f99a77c08f0`, existing `ControlKey`, the existing
  binding-local listener closure, and the existing core composition owner.
- Produces: one append-only four-file corrective commit only after the real gate passes. It adds no dependency, public
  API, command, permission, Rust/native bridge, or final Tauri adapter.

- [ ] **Step 1: Authenticate new written R3 approvals and the clean recovery**

```powershell
$ErrorActionPreference = 'Stop'
$recovery = '28f058be94d4fadb0b490b08f4bb5f99a77c08f0'
$head = (& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'R3 recovery HEAD resolution failed' }
if ($head -cne $recovery) { throw "R3 must start at clean recovery $recovery, got $head" }
$status = @(& git status --porcelain=v1 --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'R3 recovery status failed' }
if ($status.Count) { throw 'R3 recovery worktree is dirty' }
```

The review response must explicitly grant R3 Design/Plan/Security Go and preserve the exact four-file product scope.

- [ ] **Step 2: Add all R3 RED rows before implementation**

Modify only `src/launcher.test.tsx`. Add `describe('R3 correlated composition boundary', ...)` with the following exact
launcher and settings rows:

1. **No-tail boundary commit:** committed value is `calc`; trusted start plus composing input stores draft `测试` with
   zero search/settings commit. A zero-payload matching boundary commits `测试` once. Launcher increments sequence once and
   calls search once; settings mutates locally once and makes zero Rust calls.
2. **Boundary then same-value tail:** after row 1, ordinary input `测试` preserves the exact snapshot reference and call
   counts. A duplicate boundary is also zero-effect.
3. **Ordinary before boundary:** matching ordinary input commits the draft value once and clears ownership; the later
   boundary preserves snapshot identity and counts.
4. **Cancel delete then end:** trusted start/draft followed by ordinary `deleteContentBackward` value `cal` commits once;
   launcher searches `cal` once and settings mutates locally once/Rust zero. The later boundary is zero-effect. Direct
   `keyDown('Escape', true)` before that input preserves snapshot/client counts; non-composing Escape still uses only the
   shared hide owner.
5. **Ownership rejection:** no-start, duplicate, wrong-control, stale epoch/invocation/generation, retired, replaced, and
   unbound boundary records preserve snapshot identity and make zero calls. Retiring an unfinished owner restores its
   committed value; late input/end records remain zero-effect.
6. **Stored-draft sentinel:** after trusted composing input stores `测试`, change only a test DOM/control sentinel to a
   different value, then send the zero-payload boundary. The committed value is still `测试`; the boundary has no field
   capable of carrying the sentinel.
7. **Lifecycle discard:** shown, unbind, replacement, and settings-field removal before boundary restore committed state,
   clear ownership, and make a late end zero-effect.
8. **Later edits:** a later same-value ordinary input with already matching visible text preserves snapshot identity. If
   an unfinished draft differs but ordinary input restores the current committed value, publish only that control
   restoration once and make zero search/settings mutation. A later different value publishes one snapshot and makes one
   applicable launcher search or one settings-local mutation. Unchanged-query rerun remains an Enter test, not an input
   test.

Add protocol type assertions for exactly:

```ts
export type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'compositionBoundary'; control: ControlKey }
```

The boundary variant must reject `value`, `data`, and `inputType`. Raw jsdom tests prove untrusted synthetic start/input
cannot establish or supply text, standalone/no-start end emits nothing, wrong-target end emits nothing, and post-unbind
end emits nothing. Because jsdom cannot create `isTrusted === true`, it must not gain an injected trust switch; the
positive binding-active end path is proven by the production WebView2 gate and the exact source oracle below.

```powershell
npm.cmd test -- --run src/launcher.test.tsx -t "R3 correlated composition boundary"
if ($LASTEXITCODE -ne 1) { throw 'R3 RED did not fail exactly as expected' }
```

Expected: named assertions execute and fail on the missing boundary/idempotence behavior; never zero tests, a missing
module, or an unrelated Phase-A failure.

- [ ] **Step 3: Implement the minimum four-file GREEN**

In `src/protocol.ts`, use the exact four-variant union from Step 2.

In each `bindNativeTextInput` closure, add one local boolean active flag. A trusted `CompositionEvent` `compositionstart`
from the same bound control and target sets it and emits the zero-text start. A trusted same-target
`InputEvent` supplies the only values:
while `isComposing`, emit composing
input only when active; otherwise clear active first and emit ordinary input. Add one same-target `compositionend`
listener that does nothing unless active; when active, set active to false before emitting only
`{ kind: 'compositionBoundary', control }`. Never read end data or current DOM value. Unbind sets active false before
removing all three listeners. No-start, duplicate, wrong-target, and post-unbind end remain zero-effect.

In `src/launcher-core.ts`, keep one owner
`{ control, viewEpoch, invocationId, generation, lastTrustedDraft }`. Matching composing input updates only draft. A
matching boundary clears ownership first and calls the existing control commit with only `lastTrustedDraft`. Matching
ordinary input clears ownership first and commits only its trusted input value. The shared commit path first synchronizes
the visible control draft. An exact committed-value match then skips committed-state mutation, sequence increment, search,
and settings mutation; publish only if that visible synchronization changed the draft. Keep shown/unbind/replacement/
retire discard behavior. Do not add a timer, microtask, adjacency/order/delay
rule, `inputType` classifier, suppression marker, tombstone, exported helper, or new public API.

- [ ] **Step 4: Run GREEN and the exact four-file/source gates**

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx -t "R3 correlated composition boundary|native trust|IME ownership"
Assert-NativeExit 'R3 focused tests'
npm.cmd test
Assert-NativeExit 'R3 full tests'
npm.cmd run build
Assert-NativeExit 'R3 production build'
$expected = @('src/launcher-core.ts','src/launcher.test.tsx','src/native-input.ts','src/protocol.ts') |
  Sort-Object -CaseSensitive -Unique
$changed = @(& git diff --name-only)
Assert-NativeExit 'R3 changed path inventory'
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
if (@(Compare-Object -CaseSensitive $expected $changed).Count) { throw 'R3 corrective path set drifted' }
$protocol = Get-Content -Raw -Encoding utf8 src/protocol.ts
$native = Get-Content -Raw -Encoding utf8 src/native-input.ts
$core = Get-Content -Raw -Encoding utf8 src/launcher-core.ts
foreach ($required in @('compositionStart','compositionInput','ordinaryInput','compositionBoundary')) {
  if (-not $protocol.Contains($required)) { throw "R3 protocol missing $required" }
}
foreach ($required in @('isTrusted','isComposing','compositionstart','compositionend','addEventListener','removeEventListener')) {
  if (-not $native.Contains($required)) { throw "R3 native boundary missing $required" }
}
foreach ($forbidden in @('compositionUpdate','compositionEnd','setTimeout','queueMicrotask','tombstone','suppression')) {
  if ($protocol.Contains($forbidden) -or $native.Contains($forbidden) -or $core.Contains($forbidden)) {
    throw "R3 forbidden source token $forbidden"
  }
}
git diff --check
Assert-NativeExit 'R3 diff check'
```

The focused source test must extract the `compositionend` listener body and prove: active is checked; active is cleared
before the boundary callback; the callback literal contains only `kind` and `control`; and the body contains no `.data`
or `.value`. It must also assert exact listener add/remove counts for start/input/end and idempotent unbind. Do not add a
general source scanner.

- [ ] **Step 5: Run the complete R3 production WebView2 matrix before commit**

Temporarily apply the reviewed, uncommitted `src/main.ts` harness. Use the real AntD inputs, production adapter/core,
existing `launcher://shown` listener, and no-argument `load_settings`; all other clients remain local fixed counters. Log
only OS/WebView versions, event kind/class, `isTrusted`, `inputType`, `isComposing`, control category, record kind, fixed
counts, and fixed booleans. Never log text, values, query, alias, IDs, payload, or timestamps.

For launcher and settings, prove all rows below with exact before/after counts:

1. Real Microsoft Pinyin no-tail commit emits trusted start/composing input, then the observed untrusted end. The adapter
   emits one boundary, core commits `lastTrustedDraft` once, and launcher makes one applicable search/settings makes one
   local mutation with zero Rust calls.
2. While a real trusted session is active, script changes only DOM current value to a sentinel and dispatches a same-target
   synthetic end. The boundary commits the prior trusted draft, never the sentinel; the later natural end is duplicate
   zero-effect. The script cannot create an owner or supply text.
3. After boundary commit, a separately generated trusted same-value ordinary input makes zero additional mutation/call;
   a trusted different value makes exactly one normal edit/call.
4. Natural cancel emits trusted non-composing `deleteContentBackward` before end; it commits once and the later end is
   zero-effect. The composing keydown itself invokes neither hide nor another command.
5. Ordinary commit before end clears ownership and commits once; the later end is zero-effect.
6. Duplicate/no-start/wrong-target/unbound/stale/retired/replaced-control end is zero-effect.
7. Shown/unbind/replacement/retire before end discards draft, restores committed value, and makes late end zero-effect.

Any mismatch restores `src/main.ts`, stops all owned processes, and returns to written design/security review without a
commit. On success restore `src/main.ts` and require the dirty set to be exactly the four R3 files.

- [ ] **Step 6: Commit the passing R3 correction once**

```powershell
$expected = @('src/launcher-core.ts','src/launcher.test.tsx','src/native-input.ts','src/protocol.ts') |
  Sort-Object -CaseSensitive -Unique
$changed = @(& git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'R3 precommit path inventory failed' }
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
if (@(Compare-Object -CaseSensitive $expected $changed).Count) {
  throw 'R3 precommit scope mismatch'
}
git add src/protocol.ts src/native-input.ts src/launcher-core.ts src/launcher.test.tsx
if ($LASTEXITCODE -ne 0) { throw 'R3 git add failed' }
$staged = @(& git diff --cached --name-only)
if ($LASTEXITCODE -ne 0) { throw 'R3 staged inventory failed' }
$staged = @($staged | Sort-Object -CaseSensitive -Unique)
if (@(Compare-Object -CaseSensitive $expected $staged).Count) {
  throw 'R3 staged scope mismatch'
}
git commit -m "fix: finalize trusted launcher composition"
if ($LASTEXITCODE -ne 0) { throw 'R3 corrective commit failed' }
```

- [ ] **Step 7: Run the committed R3 gate and request code/security review**

From the committed clean worktree, rerun Step 4, the full Step 5 production matrix,
`git diff --check 28f058be94d4fadb0b490b08f4bb5f99a77c08f0..HEAD`, `git show --check --format= HEAD`, exact four-path comparison, and
clean staged/unstaged/untracked status. Send the audit thread the new R3 Design/Plan/Security Go SHAs, recovery and product
HEAD, RED evidence, tests/build, sanitized production matrix, dependency-no-change proof, exact commit/file set, and
clean status. Request written R3 Code Go and Security Go. Do not enter Task 8 while review is pending.

---

### Task 8: Add The Exact Real Tauri Adapter And Startup

Task 8 and Task 9 remain non-executable until Task 7R3 receives written Code/Security Go and its production WebView2 gate
passes.

**Files:**
- Modify: `src/main.ts`
- Modify: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: written R3 Code/Security Go plus passing production WebView2 gate, `LauncherClient`, `LauncherCore`,
  `LauncherView`, and exact Task 6 event/readiness.
- Produces: the only production Tauri adapter and real React root.

- [ ] **Step 1: Add RED module-mocked adapter/startup tests**

Use Vitest module mocks for only `@tauri-apps/api/event` and `@tauri-apps/api/core`, create the existing `#app` element, dynamically import `main.ts`, and prove:

- React root/view-ready completes before `listen` is called;
- resolved `listen('launcher://shown', handler)` occurs before the first no-argument `invoke('load_settings')`;
- mount/ref/native-listener/listen failure produces zero load calls and fixed local initialization error;
- an event emitted while load is pending reaches the core;
- exact invoke table is:

```ts
const invokeRows = [
  ['search_apps', [{ query: 'calc', invocationId: 'inv-1', querySequence: 1 }]],
  ['execute_result', [{ requestId: 'req-1', resultId: 'result-1' }]],
  ['load_settings', []],
  ['save_settings', [{ settings: update }]],
  ['rescan_apps', []],
  ['export_validation_data', []],
  ['clear_validation_data', []],
  ['hide_launcher', []],
] as const
```

- `pagehide`/teardown removes native listeners, calls Tauri unlisten once, unmounts React once, and repeated teardown is no-op;
- source contains exactly the eight command strings and one event string, no Tauri window import/API, no direct hide, no path/PID/HWND/appId action argument.

- [ ] **Step 2: Run RED**

```powershell
npm.cmd test -- --run src/launcher.test.tsx -t "real adapter|startup"
```

Expected: FAIL because baseline `main.ts` is still the vanilla DOM shell; never zero tests.

- [ ] **Step 3: Implement the one literal client and startup sequence**

Replace the vanilla DOM construction with one literal `LauncherClient`. Use static imports from `@tauri-apps/api/core` and `@tauri-apps/api/event`; the temporary gate's dynamic imports do not enter production. Each method is one typed `invoke` call using the exact table above; each `[]` row calls `invoke('name')` with one JavaScript argument total, never a second `undefined`/`{}` argument. No generic command bus or Tauri client reaches React. `listenShown` maps only `event.payload` to the core callback.

Create the core and root, render the `LauncherView` composition root (which owns `ConfigProvider -> App`), and resolve/reject the one view-ready Promise from its fixed `ready | failed` result. On `failed`, call `core.failInitialization()` and make zero listener/load calls. On `ready`, await `core.start()`; expected listener/load failures are decoded inside the core without throwing raw values. Keep the root visible state Rust-owned. Add one idempotent local teardown used by `pagehide`; it calls `core.destroy()` before `root.unmount()`.

- [ ] **Step 4: Run GREEN and commit only adapter/test**

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
npm.cmd test -- --run src/launcher.test.tsx -t "real adapter|startup"
Assert-NativeExit 'real adapter focused test'
npm.cmd test
Assert-NativeExit 'real adapter full test'
npm.cmd run build
Assert-NativeExit 'real adapter production build'
git add src/main.ts src/launcher.test.tsx
Assert-NativeExit 'real adapter git add'
git commit -m "feat: connect launcher to Tauri lifecycle"
Assert-NativeExit 'real adapter git commit'
```

Expected: exact two-file commit, all frontend tests/build pass.

---

### Task 9: Run The Final Local Code Gate; Keep Release/QA Blocked

**Files:** No source change. Any failure stops and returns for written review; this gate does not authorize a product fix.

**Interfaces:**
- Consumes: Task 6 baseline ancestor, Tasks 1-8 plus Task 7R3 commit and passing R3 production metadata gate.
- Produces: local TaskCodeGo request only; release/QA remains blocked.

**Permanent scope disposition:** The custom Job cleanup preflight, historical performance runner, temporary measurement
wrapper, CDP collectors/probes, and every associated diagnostic are failed evidence and must never be executed, repaired,
or resumed. Steps 3 and 3A and their fenced content remain only for audit history. They cannot satisfy this local gate and
cannot be inherited by a later agent as authorization.

The only executable Task 9 path after written Plan/Security Go is Step 1, Step 2, Step 4, and Step 5 below. It makes no
release-executable, runtime-performance, process-cleanup, accessibility, zero-network, installer, signing, trial, or
release claim.

- [ ] **Step 1: Run fresh frontend and inherited Rust/config gates**

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$expectedHead = '16018e56486bcd4efcd1a2c81798ebc9223025e7'
$head = (& git rev-parse HEAD).Trim()
Assert-NativeExit 'local gate HEAD'
if ($head -cne $expectedHead) { throw 'local gate product HEAD drifted' }
$sourceStatus = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'pre-local-gate source status'
if ($sourceStatus.Count) { throw 'source is not clean before local code gate' }
npm.cmd test -- --run src/launcher.test.tsx -t "R3 correlated composition boundary|native trust|IME ownership"
Assert-NativeExit 'R3 focused tests'
npm.cmd test -- --run src/launcher.test.tsx -t "real adapter|startup"
Assert-NativeExit 'adapter focused tests'
npm.cmd test
Assert-NativeExit 'frontend full tests'
npm.cmd run build
Assert-NativeExit 'frontend production build'
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
Assert-NativeExit 'cargo test default'
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
Assert-NativeExit 'cargo test all-features'
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
Assert-NativeExit 'cargo clippy default'
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
Assert-NativeExit 'cargo clippy all-features'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-security-config.ps1
Assert-NativeExit 'security config checker'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
Assert-NativeExit 'security config regression'
$sourceStatus = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'post-local-gate source status'
if ($sourceStatus.Count) { throw 'source is not clean after local code gate' }
```

Expected: focused R3 and adapter rows, the full frontend suite, Vite build, inherited Rust/Clippy checks, and both static
security-config checks pass from and return to a clean product worktree. This is local code evidence only; no Tauri
release executable is built or authenticated.

- [ ] **Step 2: Measure the exact local production bundle**

```powershell
@'
const fs = require('node:fs')
const path = require('node:path')
const zlib = require('node:zlib')
const root = path.resolve('dist')
function walk(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const absolute = path.join(directory, entry.name)
    return entry.isDirectory() ? walk(absolute) : [absolute]
  })
}
function attributes(tag) {
  const values = {}
  for (const match of tag.matchAll(/([:\w-]+)\s*=\s*(["'])(.*?)\2/g)) values[match[1].toLowerCase()] = match[3]
  return values
}
function localFile(reference) {
  if (/^(?:[a-z]+:)?\/\//i.test(reference) || reference.startsWith('data:')) throw new Error(`remote initial reference: ${reference}`)
  const clean = decodeURIComponent(reference.split(/[?#]/, 1)[0]).replace(/^\/+/, '')
  const absolute = path.resolve(root, clean)
  if (absolute !== root && !absolute.startsWith(`${root}${path.sep}`)) throw new Error(`initial reference escaped dist: ${reference}`)
  if (!fs.statSync(absolute).isFile()) throw new Error(`initial reference missing: ${reference}`)
  return absolute
}
const files = walk(root)
const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8')
const tags = html.match(/<(?:script|link)\b[^>]*>/gi) ?? []
const js = []
const css = []
for (const tag of tags) {
  const attrs = attributes(tag)
  if (/^<script\b/i.test(tag) && attrs.src) {
    if (!attrs.src.split(/[?#]/, 1)[0].endsWith('.js')) throw new Error(`unexpected initial script: ${attrs.src}`)
    js.push(localFile(attrs.src))
  }
  if (/^<link\b/i.test(tag) && (attrs.rel ?? '').toLowerCase().split(/\s+/).includes('stylesheet')) {
    if (!attrs.href?.split(/[?#]/, 1)[0].endsWith('.css')) throw new Error(`unexpected stylesheet: ${attrs.href}`)
    css.push(localFile(attrs.href))
  }
}
if (js.length !== 1 || new Set(js).size !== 1) throw new Error(`unexpected initial JS inventory: ${js.length}`)
if (!css.length) throw new Error('no initial CSS reference')
if (new Set(css).size !== css.length) throw new Error('duplicate initial CSS reference')
const initial = new Set([...js, ...css])
const emittedChunks = files.filter(file => file.endsWith('.js') || file.endsWith('.css'))
const unreferenced = emittedChunks.filter(file => !initial.has(file))
if (unreferenced.length || initial.size !== emittedChunks.length) {
  throw new Error(`dynamic/unreferenced JS/CSS emitted: ${unreferenced.map(file => path.relative(root, file)).join(',')}`)
}
const maps = files.filter(file => file.endsWith('.map'))
if (maps.length) throw new Error(`source maps emitted: ${maps.join(',')}`)
function measure(group) {
  const entries = group.map(file => {
    const bytes = fs.readFileSync(file)
    return { file: path.relative(root, file).replaceAll('\\', '/'), raw: bytes.length, gzip: zlib.gzipSync(bytes, { level: 9 }).length }
  })
  return { files: entries, total: entries.reduce((sum, entry) => ({ raw: sum.raw + entry.raw, gzip: sum.gzip + entry.gzip }), { raw: 0, gzip: 0 }) }
}
const sizes = { js: measure(js), css: measure(css) }
console.log(JSON.stringify(sizes, null, 2))
if (sizes.js.total.raw > 900 * 1024 || sizes.js.total.gzip > 300 * 1024) throw new Error('JS bundle threshold exceeded')
if (sizes.css.total.raw > 120 * 1024 || sizes.css.total.gzip > 30 * 1024) throw new Error('CSS bundle threshold exceeded')
const cssText = css.map(file => fs.readFileSync(file, 'utf8')).join('\n')
if (/(?:src|href)=["'](?:https?:)?\/\//i.test(html) || /(?:url\(|@import\s+)[^;]*(?:https?:)?\/\//i.test(cssText)) {
  throw new Error('remote runtime asset reference emitted')
}
'@ | node
if ($LASTEXITCODE -ne 0) { throw 'initial bundle Node oracle failed' }
if (Test-Path dist\security-probe.html) { throw 'production dist contains security probe HTML' }
if (@(Get-ChildItem dist -Recurse -Force | Where-Object { $_.Name -match 'security-probe' }).Count) {
  throw 'production dist contains security probe artifact'
}
```

Expected: `dist/index.html` authenticates one initial JS reference and every initial CSS reference; every emitted JS/CSS file is one of those references, so dynamic/unreferenced chunks fail. Print each referenced file's raw/level-9 gzip size plus JS/CSS totals before threshold checks; no source map/remote/probe artifact.

- [x] **Step 3: Permanently closed failed performance/accessibility infrastructure**

On the agreed Windows 11 reference host, record Windows build, CPU, memory, storage, power mode, and WebView2 version. Retain only aggregate timings/environment metadata:

- 30 clean process starts: `performance.timeOrigin` to mounted view + native listeners + registered shown listener, P95 <= `750 ms`.
- Exactly 5 warmups, then 100 empty-query and 100 preserved-query launcher events (205 real Task 6 requests total): callback entry to focused enabled input and next painted frame, each P95 <= `100 ms`.
- Existing external launcher and search P95 remain separately reported; do not mix clocks.
- At 100/150/200% zoom, verify no horizontal overflow/overlap/clipped focused control and Arrow navigation keeps active option visible.
- With Narrator, verify combobox, result count, active title/subtitle, settings heading, lifecycle notice, errors, labels, and `关闭`.
- Verify forced-colors visible focus/borders and long Chinese/Latin/markup-like text.
- Keep DevTools network recording active and require zero remote asset/font/request entries; namespace strings inside bundled libraries are not network evidence.

The `TASK7_PERF_WORKFLOW` below is now frozen historical evidence and **must not be executed**. Its formal preflight under
plan `4e9af0789974577521d444043890f2ecbaf59eeb` failed with
`Cleanup=FAIL category=preflight count=1 / child postdates authenticated parent exit cutoff` before query seed or any
sample. Do not modify or retry that timestamp/parent-PID runner. Step 3, query seed, 30/205, and runtime
accessibility/network claims remain release/QA No-Go. The separate static local Step 4 trust checkpoint remains part of
the executable local code gate.

The bounded suspended-child diagnostic after the `8ad87b11cc8bdd4b59785847a7b77a1d45426a64` checkpoint failure is
diagnostic input only. Expected and queried paths were both length `57`; their first exact UTF-16 difference was casing
at index `4` (`73` versus `105`), while ordinal-ignore-case, extended-prefix and trailing-separator normalization,
regular/non-reparse type, and executable file SHA-256 all agreed. The prior mismatch was not reproduced. Keep the
existing ordinal-ignore-case full-path authentication unchanged; do not add canonicalization, basename-only, hash-only,
or another path probe.

- [x] **Step 3A: Permanently closed failed Job cleanup checkpoint**

The following fence is permanently non-executable failed evidence. It may not be run under any prior or future
`TASK7_CLEANUP_PLAN_GO_SHA`, repaired, or used to reopen the historical performance workflow.

<!-- TASK7_JOB_PREFLIGHT_BEGIN -->
```powershell
$ErrorActionPreference = 'Stop'
$repo = 'D:\code\UiPilot_tools\.worktrees\foundation-task-7'
$docs = 'D:\code\UiPilot_tools\.worktrees\foundation-task-7-design'
$productSha = '16018e56486bcd4efcd1a2c81798ebc9223025e7'
$planSha = [Environment]::GetEnvironmentVariable('TASK7_CLEANUP_PLAN_GO_SHA', 'Process')
if ($planSha -cnotmatch '^[0-9a-f]{40}$') { throw 'cleanup Plan Go SHA is missing' }
$port = 9227
$browserArgsName = 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'
$originalBrowserArgs = [Environment]::GetEnvironmentVariable($browserArgsName, 'Process')
$originalTarget = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'Process')
$originalIncremental = [Environment]::GetEnvironmentVariable('CARGO_INCREMENTAL', 'Process')
$originalJobs = [Environment]::GetEnvironmentVariable('CARGO_BUILD_JOBS', 'Process')
$targetPath = Join-Path $env:TEMP ("uipilot-task7-job-target-$([guid]::NewGuid().ToString('N'))")
$fixturePaths = [Collections.Generic.List[string]]::new()
$activeJob = $null
$exactExeForFinal = $null
$preflightEvidence = $null
$originalFailure = $null
$cleanupFailures = [Collections.Generic.List[string]]::new()

function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}

function Add-CleanupFailure([Collections.Generic.List[string]]$failures, [string]$label, $errorRecord) {
  $message = "${label}: $($errorRecord.Exception.Message)"
  if (-not $failures.Contains($message)) { [void]$failures.Add($message) }
}

function Get-CombinedFailureMessage([Exception]$original, [Collections.Generic.List[string]]$cleanupFailures) {
  if ($cleanupFailures.Count -lt 1) { throw 'combined failure requires cleanup evidence' }
  "$($original.Message) | cleanup failed count=$($cleanupFailures.Count) | $([string]::Join(' | ', @($cleanupFailures)))"
}

$browserOnlyException = [InvalidOperationException]::new('TASK7_BROWSER_ONLY_SENTINEL')
$browserOnlyRecord = [Management.Automation.ErrorRecord]::new(
  $browserOnlyException, 'task7-browser-only', [Management.Automation.ErrorCategory]::NotSpecified, $null)
$browserOnlyFailures = [Collections.Generic.List[string]]::new()
if ($null -ne $browserOnlyRecord) {
  Add-CleanupFailure $browserOnlyFailures 'browser arguments restore' $browserOnlyRecord
}
$browserOnlyMessage = Get-CombinedFailureMessage $browserOnlyException $browserOnlyFailures
if ($browserOnlyMessage -cne 'TASK7_BROWSER_ONLY_SENTINEL | cleanup failed count=1 | browser arguments restore: TASK7_BROWSER_ONLY_SENTINEL') {
  throw 'browser-only failure preservation self-test failed'
}

$launchCombinedException = [InvalidOperationException]::new('TASK7_LAUNCH_COMBINED_SENTINEL')
$browserCombinedException = [InvalidOperationException]::new('TASK7_BROWSER_COMBINED_SENTINEL')
$browserCombinedRecord = [Management.Automation.ErrorRecord]::new(
  $browserCombinedException, 'task7-browser-combined', [Management.Automation.ErrorCategory]::NotSpecified, $null)
$jobCombinedException = [InvalidOperationException]::new('TASK7_JOB_CLEANUP_SENTINEL')
$jobCombinedRecord = [Management.Automation.ErrorRecord]::new(
  $jobCombinedException, 'task7-job-cleanup', [Management.Automation.ErrorCategory]::NotSpecified, $null)
$combinedFailures = [Collections.Generic.List[string]]::new()
if ($null -ne $browserCombinedRecord) {
  Add-CleanupFailure $combinedFailures 'browser arguments restore' $browserCombinedRecord
}
Add-CleanupFailure $combinedFailures 'primary Job cleanup' $jobCombinedRecord
$combinedMessage = Get-CombinedFailureMessage $launchCombinedException $combinedFailures
if ($combinedMessage -cne 'TASK7_LAUNCH_COMBINED_SENTINEL | cleanup failed count=2 | browser arguments restore: TASK7_BROWSER_COMBINED_SENTINEL | primary Job cleanup: TASK7_JOB_CLEANUP_SENTINEL') {
  throw 'launch/browser/Job failure preservation self-test failed'
}

$productHead = @(& git -C $repo rev-parse HEAD)
Assert-NativeExit 'cleanup product HEAD'
$productStatus = @(& git -C $repo status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'cleanup product status'
$docsHead = @(& git -C $docs rev-parse HEAD)
Assert-NativeExit 'cleanup docs HEAD'
$docsStatus = @(& git -C $docs status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'cleanup docs status'
if ($productHead.Count -ne 1 -or $productHead[0] -cne $productSha -or $productStatus.Count -ne 0 -or
    $docsHead.Count -ne 1 -or $docsHead[0] -cne $planSha -or $docsStatus.Count -ne 0) {
  throw 'cleanup baseline or worktree state drifted'
}
if ($null -ne $originalBrowserArgs -or $null -ne $originalTarget -or
    $null -ne $originalIncremental -or $null -ne $originalJobs) {
  throw 'cleanup preflight requires absent browser/Cargo override environment'
}
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$targetFull = [IO.Path]::GetFullPath($targetPath)
$tempItem = Get-Item -LiteralPath $tempRoot -Force
if ($tempItem -isnot [IO.DirectoryInfo] -or ($tempItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
    -not $targetFull.StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
    (Test-Path -LiteralPath $targetFull)) {
  throw 'cleanup TEMP target authentication failed'
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class Task7OwnedJob {
    public const uint CREATE_SUSPENDED = 0x00000004;
    public const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    public const int JobObjectBasicAccountingInformation = 1;
    public const int JobObjectExtendedLimitInformation = 9;
    public const uint PROCESS_TERMINATE = 0x0001;
    public const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    public const uint SYNCHRONIZE = 0x00100000;
    public const uint WAIT_OBJECT_0 = 0;
    public const uint WAIT_TIMEOUT = 258;
    public const uint STILL_ACTIVE = 259;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct STARTUPINFO {
        public uint cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public uint dwX;
        public uint dwY;
        public uint dwXSize;
        public uint dwYSize;
        public uint dwXCountChars;
        public uint dwYCountChars;
        public uint dwFillAttribute;
        public uint dwFlags;
        public ushort wShowWindow;
        public ushort cbReserved2;
        public IntPtr lpReserved2;
        public IntPtr hStdInput;
        public IntPtr hStdOutput;
        public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION {
        public IntPtr hProcess;
        public IntPtr hThread;
        public uint dwProcessId;
        public uint dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct IO_COUNTERS {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
        public long TotalUserTime;
        public long TotalKernelTime;
        public long ThisPeriodTotalUserTime;
        public long ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount;
        public uint TotalProcesses;
        public uint ActiveProcesses;
        public uint TotalTerminatedProcesses;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetInformationJobObject(
        IntPtr job, int informationClass,
        ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION information, uint length);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool QueryInformationJobObject(
        IntPtr job, int informationClass,
        out JOBOBJECT_BASIC_ACCOUNTING_INFORMATION information, uint length, IntPtr returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CreateProcessW(
        string applicationName, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles,
        uint creationFlags, IntPtr environment, string currentDirectory,
        ref STARTUPINFO startupInfo, out PROCESS_INFORMATION processInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint ResumeThread(IntPtr thread);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint access, bool inheritHandle, uint processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool QueryFullProcessImageNameW(
        IntPtr process, uint flags, StringBuilder path, ref uint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@

if ([IntPtr]::Size -ne 8 -or
    [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+STARTUPINFO]) -ne 104 -or
    [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+PROCESS_INFORMATION]) -ne 24 -or
    [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+JOBOBJECT_EXTENDED_LIMIT_INFORMATION]) -ne 144 -or
    [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+JOBOBJECT_BASIC_ACCOUNTING_INFORMATION]) -ne 48) {
  throw 'cleanup Win32 structure size drifted'
}

function Close-OwnedHandle([IntPtr]$handle, [string]$label) {
  if ($handle -eq [IntPtr]::Zero) { return }
  if (-not [Task7OwnedJob]::CloseHandle($handle)) {
    throw "$label CloseHandle failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
}

function Get-ExactExecutableRows([string]$path) {
  @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
    $null -ne $_.ExecutablePath -and [string]::Equals($_.ExecutablePath, $path, [StringComparison]::OrdinalIgnoreCase)
  })
}

function Get-PortListeners {
  @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq $port })
}

function Get-JobAccounting($job) {
  $accounting = [Task7OwnedJob+JOBOBJECT_BASIC_ACCOUNTING_INFORMATION]::new()
  $size = [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+JOBOBJECT_BASIC_ACCOUNTING_INFORMATION])
  if (-not [Task7OwnedJob]::QueryInformationJobObject(
      $job.Handle, [Task7OwnedJob]::JobObjectBasicAccountingInformation,
      [ref]$accounting, [uint32]$size, [IntPtr]::Zero)) {
    throw "QueryInformationJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  $accounting
}

function New-OwnedJob {
  $handle = [Task7OwnedJob]::CreateJobObjectW([IntPtr]::Zero, $null)
  if ($handle -eq [IntPtr]::Zero) {
    throw "CreateJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  try {
    $limits = [Task7OwnedJob+JOBOBJECT_EXTENDED_LIMIT_INFORMATION]::new()
    $limits.BasicLimitInformation.LimitFlags = [Task7OwnedJob]::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    $size = [Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+JOBOBJECT_EXTENDED_LIMIT_INFORMATION])
    if (-not [Task7OwnedJob]::SetInformationJobObject(
        $handle, [Task7OwnedJob]::JobObjectExtendedLimitInformation, [ref]$limits, [uint32]$size)) {
      throw "SetInformationJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    [pscustomobject]@{
      Handle = $handle
      Processes = [Collections.Generic.List[object]]::new()
      Primary = $null
      Stopped = $false
      Closed = $false
    }
  } catch {
    $original = $_
    $localCleanupFailures = [Collections.Generic.List[string]]::new()
    try { Close-OwnedHandle $handle 'job setup' } catch { Add-CleanupFailure $localCleanupFailures 'job setup handle close' $_ }
    if ($localCleanupFailures.Count) {
      throw (Get-CombinedFailureMessage $original.Exception $localCleanupFailures)
    }
    throw $original
  }
}

function Get-OwnedProcessPath([IntPtr]$processHandle) {
  $capacity = [uint32]32768
  $buffer = [Text.StringBuilder]::new([int]$capacity)
  if (-not [Task7OwnedJob]::QueryFullProcessImageNameW($processHandle, 0, $buffer, [ref]$capacity)) {
    throw "QueryFullProcessImageName failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  [IO.Path]::GetFullPath($buffer.ToString())
}

function Start-SuspendedOwnedProcess($job, [string]$application, [string]$arguments) {
  $exactApplication = [IO.Path]::GetFullPath($application)
  $startup = [Task7OwnedJob+STARTUPINFO]::new()
  $startup.cb = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][Task7OwnedJob+STARTUPINFO])
  $information = [Task7OwnedJob+PROCESS_INFORMATION]::new()
  $commandLine = [Text.StringBuilder]::new(('"' + $exactApplication + '"' + $arguments))
  if (-not [Task7OwnedJob]::CreateProcessW(
      $exactApplication, $commandLine, [IntPtr]::Zero, [IntPtr]::Zero, $false,
      [Task7OwnedJob]::CREATE_SUSPENDED, [IntPtr]::Zero, (Split-Path -Parent $exactApplication),
      [ref]$startup, [ref]$information)) {
    throw "CreateProcessW suspended failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  $assigned = $false
  try {
    if (-not [Task7OwnedJob]::AssignProcessToJobObject($job.Handle, $information.hProcess)) {
      throw "AssignProcessToJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $assigned = $true
    $isMember = $false
    if (-not [Task7OwnedJob]::IsProcessInJob($information.hProcess, $job.Handle, [ref]$isMember) -or -not $isMember) {
      throw "assigned process membership failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    if (-not [string]::Equals(
        (Get-OwnedProcessPath $information.hProcess), $exactApplication, [StringComparison]::OrdinalIgnoreCase)) {
      throw 'created process executable path drifted'
    }
    $resumeResult = [Task7OwnedJob]::ResumeThread($information.hThread)
    if ($resumeResult -eq [uint32]::MaxValue) {
      throw "ResumeThread failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    Close-OwnedHandle $information.hThread 'created thread'
    $information.hThread = [IntPtr]::Zero
    $record = [pscustomobject]@{
      Pid = [int]$information.dwProcessId
      Handle = $information.hProcess
      Application = $exactApplication
      Closed = $false
    }
    $job.Processes.Add($record)
    $record
  } catch {
    $original = $_
    $localCleanupFailures = [Collections.Generic.List[string]]::new()
    try {
      if ($information.hProcess -eq [IntPtr]::Zero) { throw 'failed launch has no retained process handle' }
      if ($assigned) {
        if (-not [Task7OwnedJob]::TerminateJobObject($job.Handle, 197)) {
          throw "failed launch TerminateJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
      } elseif (-not [Task7OwnedJob]::TerminateProcess($information.hProcess, 197)) {
        throw "failed launch TerminateProcess failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
      }
    } catch { Add-CleanupFailure $localCleanupFailures 'failed launch termination' $_ }
    try {
      if ($information.hProcess -eq [IntPtr]::Zero) { throw 'failed launch has no waitable process handle' }
      $failedWait = [Task7OwnedJob]::WaitForSingleObject($information.hProcess, 2000)
      if ($failedWait -ne [Task7OwnedJob]::WAIT_OBJECT_0) {
        throw "failed launch process wait result=$failedWait win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
      }
    } catch { Add-CleanupFailure $localCleanupFailures 'failed launch process wait' $_ }
    if ($information.hThread -ne [IntPtr]::Zero) {
      try { Close-OwnedHandle $information.hThread 'failed thread' } catch { Add-CleanupFailure $localCleanupFailures 'failed thread close' $_ }
    }
    if ($information.hProcess -ne [IntPtr]::Zero) {
      try { Close-OwnedHandle $information.hProcess 'failed process' } catch { Add-CleanupFailure $localCleanupFailures 'failed process close' $_ }
    }
    if ($localCleanupFailures.Count) {
      throw (Get-CombinedFailureMessage $original.Exception $localCleanupFailures)
    }
    throw $original
  }
}

function Wait-OwnedProcess($record, [uint32]$milliseconds, [switch]$RequireZeroExit) {
  $wait = [Task7OwnedJob]::WaitForSingleObject($record.Handle, $milliseconds)
  if ($wait -ne [Task7OwnedJob]::WAIT_OBJECT_0) {
    throw "owned process wait failed result=$wait win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  [uint32]$exitCode = [Task7OwnedJob]::STILL_ACTIVE
  if (-not [Task7OwnedJob]::GetExitCodeProcess($record.Handle, [ref]$exitCode)) {
    throw "GetExitCodeProcess failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  if ($RequireZeroExit -and $exitCode -ne 0) { throw "owned process exited nonzero code=$exitCode" }
  $exitCode
}

function Assert-HandleInOwnedJob([IntPtr]$processHandle, $job, [switch]$RequireLive) {
  $isMember = $false
  if (-not [Task7OwnedJob]::IsProcessInJob($processHandle, $job.Handle, [ref]$isMember) -or -not $isMember) {
    throw "process is outside authenticated Job win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  if ($RequireLive -and [Task7OwnedJob]::WaitForSingleObject($processHandle, 0) -ne [Task7OwnedJob]::WAIT_TIMEOUT) {
    throw 'authenticated Job process is not live'
  }
}

function Assert-PidInOwnedJob([int]$ownedPid, $job, [switch]$RequireLive) {
  $handle = [Task7OwnedJob]::OpenProcess(
    [Task7OwnedJob]::PROCESS_QUERY_LIMITED_INFORMATION -bor [Task7OwnedJob]::SYNCHRONIZE,
    $false, [uint32]$ownedPid)
  if ($handle -eq [IntPtr]::Zero) {
    throw "OpenProcess membership failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  try { Assert-HandleInOwnedJob $handle $job -RequireLive:$RequireLive } finally { Close-OwnedHandle $handle 'membership process' }
}

function Assert-OwnedJobPorts($job) {
  $listeners = @(Get-PortListeners)
  if ($listeners.Count -lt 1) { throw 'owned Job has no debug listener' }
  foreach ($listener in $listeners) { Assert-PidInOwnedJob ([int]$listener.OwningProcess) $job -RequireLive }
}

function Assert-OwnedJobForeground($job) {
  $window = [Task7OwnedJob]::GetForegroundWindow()
  if ($window -eq [IntPtr]::Zero) { throw 'owned Job has no foreground window' }
  [uint32]$foregroundPid = 0
  [void][Task7OwnedJob]::GetWindowThreadProcessId($window, [ref]$foregroundPid)
  if ($foregroundPid -eq 0) { throw 'foreground PID is zero' }
  Assert-PidInOwnedJob ([int]$foregroundPid) $job -RequireLive
}

function Stop-OwnedJob($job, [string]$exactExe) {
  if ($null -eq $job -or $job.Stopped) { return }
  $job.Stopped = $true
  $failures = [Collections.Generic.List[string]]::new()
  try {
    try {
      $before = Get-JobAccounting $job
      if ($before.ActiveProcesses -gt 0 -and -not [Task7OwnedJob]::TerminateJobObject($job.Handle, 197)) {
        throw "TerminateJobObject failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
      }
    } catch { Add-CleanupFailure $failures 'job termination' $_ }
    try {
      $deadline = [DateTime]::UtcNow.AddSeconds(5)
      do {
        $after = Get-JobAccounting $job
        if ($after.ActiveProcesses -eq 0) { break }
        Start-Sleep -Milliseconds 25
      } while ([DateTime]::UtcNow -lt $deadline)
      if ($after.ActiveProcesses -ne 0) { throw "job active-process deadline count=$($after.ActiveProcesses)" }
    } catch { Add-CleanupFailure $failures 'job active-zero' $_ }
    foreach ($record in $job.Processes) {
      try {
        if ([Task7OwnedJob]::WaitForSingleObject($record.Handle, 2000) -ne [Task7OwnedJob]::WAIT_OBJECT_0) {
          throw 'retained process handle did not signal'
        }
      } catch { Add-CleanupFailure $failures 'retained process wait' $_ }
    }
    if ($null -ne $exactExe) {
      try {
        $exact = @(Get-ExactExecutableRows $exactExe).Count
        $listeners = @(Get-PortListeners).Count
        if ($exact -ne 0 -or $listeners -ne 0) { throw "owned Job residue exact=$exact listen=$listeners" }
      } catch { Add-CleanupFailure $failures 'owned Job residue' $_ }
    }
  } finally {
    foreach ($record in $job.Processes) {
      if (-not $record.Closed) {
        try { Close-OwnedHandle $record.Handle 'retained process' } catch { Add-CleanupFailure $failures 'process handle close' $_ }
        $record.Closed = $true
      }
    }
    if (-not $job.Closed) {
      try { Close-OwnedHandle $job.Handle 'owned Job' } catch { Add-CleanupFailure $failures 'Job handle close' $_ }
      $job.Closed = $true
    }
  }
  if ($failures.Count) { throw "owned Job cleanup failed count=$($failures.Count)" }
}

function Start-OwnedJobPrimary([string]$exactExe) {
  if (@(Get-ExactExecutableRows $exactExe).Count -ne 0 -or @(Get-PortListeners).Count -ne 0) {
    throw 'owned Job primary requires zero exact executable and listener'
  }
  $job = New-OwnedJob
  $launchFailure = $null
  $browserRestoreFailure = $null
  try {
    try {
      [Environment]::SetEnvironmentVariable($browserArgsName, "--remote-debugging-port=$port", 'Process')
      $job.Primary = Start-SuspendedOwnedProcess $job $exactExe ''
    } catch {
      $launchFailure = $_
    } finally {
      try {
        [Environment]::SetEnvironmentVariable($browserArgsName, $originalBrowserArgs, 'Process')
      } catch {
        $browserRestoreFailure = $_
      }
    }
    if ($null -ne $launchFailure) { throw $launchFailure }
    if ($null -ne $browserRestoreFailure) { throw $browserRestoreFailure }
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
      Assert-HandleInOwnedJob $job.Primary.Handle $job -RequireLive
      $listeners = @(Get-PortListeners)
      $accounting = Get-JobAccounting $job
      if ($listeners.Count -gt 0 -and $accounting.ActiveProcesses -ge 2) {
        Assert-OwnedJobPorts $job
        return $job
      }
      Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'owned Job WebView2 readiness deadline'
  } catch {
    $original = if ($null -ne $launchFailure) { $launchFailure } else { $_ }
    $localCleanupFailures = [Collections.Generic.List[string]]::new()
    if ($null -ne $browserRestoreFailure) {
      Add-CleanupFailure $localCleanupFailures 'browser arguments restore' $browserRestoreFailure
    }
    try { Stop-OwnedJob $job $exactExe } catch { Add-CleanupFailure $localCleanupFailures 'primary Job cleanup' $_ }
    if ($localCleanupFailures.Count) {
      throw (Get-CombinedFailureMessage $original.Exception $localCleanupFailures)
    }
    throw $original
  }
}

function Invoke-OwnedSecondary($job, [string]$exactExe) {
  $before = @(Get-ExactExecutableRows $exactExe)
  if ($before.Count -ne 1 -or [int]$before[0].ProcessId -ne $job.Primary.Pid) {
    throw 'owned Job primary changed before secondary'
  }
  $secondary = Start-SuspendedOwnedProcess $job $exactExe ''
  [void](Wait-OwnedProcess $secondary 10000 -RequireZeroExit)
  $after = @(Get-ExactExecutableRows $exactExe)
  if ($after.Count -ne 1 -or [int]$after[0].ProcessId -ne $job.Primary.Pid) {
    throw 'owned secondary left an extra exact executable'
  }
}

function New-FixtureScript([string]$body) {
  $path = Join-Path $env:TEMP ("uipilot-task7-job-fixture-$([guid]::NewGuid().ToString('N')).ps1")
  if (Test-Path -LiteralPath $path) { throw 'unique Job fixture path exists' }
  [IO.File]::WriteAllText($path, $body, [Text.UTF8Encoding]::new($false))
  $item = Get-Item -LiteralPath $path -Force
  if ($item -isnot [IO.FileInfo] -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Job fixture is not a regular file'
  }
  $fixturePaths.Add($path)
  $path
}

$powershellExe = [IO.Path]::GetFullPath((Get-Command powershell.exe -ErrorAction Stop).Source)

try {
  $earlyFixture = New-FixtureScript "throw 'TASK7_EARLY_FIXTURE_BODY_MUST_NOT_RUN'"
  $earlyOriginal = $null
  $earlyCleanupFailures = [Collections.Generic.List[string]]::new()
  try {
    throw 'TASK7_EARLY_FIXTURE_ORIGINAL_SENTINEL'
  } catch {
    $earlyOriginal = $_
  } finally {
    foreach ($path in $fixturePaths) {
      try {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
      } catch {
        Add-CleanupFailure $earlyCleanupFailures 'early fixture file cleanup' $_
      }
    }
  }
  if ($null -eq $earlyOriginal -or
      $earlyOriginal.Exception.Message -cne 'TASK7_EARLY_FIXTURE_ORIGINAL_SENTINEL' -or
      $earlyCleanupFailures.Count -ne 0 -or (Test-Path -LiteralPath $earlyFixture)) {
    throw 'early fixture failure cleanup regression failed'
  }
  Write-Output 'EarlyFixtureFailureCleanupRegression=PASS count=1'

$positiveJob = $null
$positiveScript = New-FixtureScript @'
$child = Start-Process powershell.exe -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 30
'@
try {
  $positiveJob = New-OwnedJob
  [void](Start-SuspendedOwnedProcess $positiveJob $powershellExe " -NoProfile -ExecutionPolicy Bypass -File `"$positiveScript`"")
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    $positiveAccounting = Get-JobAccounting $positiveJob
    if ($positiveAccounting.TotalProcesses -ge 2 -and $positiveAccounting.ActiveProcesses -ge 2) { break }
    Start-Sleep -Milliseconds 25
  } while ([DateTime]::UtcNow -lt $deadline)
  if ($positiveAccounting.TotalProcesses -lt 2 -or $positiveAccounting.ActiveProcesses -lt 2) {
    throw 'positive Job child inheritance fixture failed'
  }
} finally {
  if ($null -ne $positiveJob) { Stop-OwnedJob $positiveJob $null }
}

$sentinel = Join-Path $env:TEMP ("uipilot-task7-job-sentinel-$([guid]::NewGuid().ToString('N')).txt")
$fixturePaths.Add($sentinel)
$negativeScript = New-FixtureScript ("[IO.File]::WriteAllText('" + $sentinel.Replace("'", "''") + "','ran')")
$fakeJob = [pscustomobject]@{ Handle = [IntPtr]::Zero; Processes = [Collections.Generic.List[object]]::new() }
$assignmentFailure = $null
try {
  [void](Start-SuspendedOwnedProcess $fakeJob $powershellExe " -NoProfile -ExecutionPolicy Bypass -File `"$negativeScript`"")
} catch { $assignmentFailure = $_ }
if ($null -eq $assignmentFailure -or (Test-Path -LiteralPath $sentinel) -or
    @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      ([string]$_.CommandLine).IndexOf($negativeScript, [StringComparison]::OrdinalIgnoreCase) -ge 0
    }).Count -ne 0) {
  throw 'suspended assignment-failure fixture executed or left a process'
}

$jobA = $null
$jobB = $null
$wrongJobScript = New-FixtureScript 'Start-Sleep -Seconds 30'
try {
  $jobA = New-OwnedJob
  $jobB = New-OwnedJob
  $jobARecord = Start-SuspendedOwnedProcess $jobA $powershellExe " -NoProfile -ExecutionPolicy Bypass -File `"$wrongJobScript`""
  $inWrongJob = $false
  if (-not [Task7OwnedJob]::IsProcessInJob($jobARecord.Handle, $jobB.Handle, [ref]$inWrongJob) -or $inWrongJob) {
    throw 'wrong-Job membership fixture was accepted'
  }
  if (-not [Task7OwnedJob]::TerminateJobObject($jobB.Handle, 197)) {
    throw "empty wrong Job termination failed win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
  }
  if ([Task7OwnedJob]::WaitForSingleObject($jobARecord.Handle, 0) -ne [Task7OwnedJob]::WAIT_TIMEOUT) {
    throw 'wrong Job terminated a process it did not own'
  }
} finally {
  if ($null -ne $jobB) { Stop-OwnedJob $jobB $null }
  if ($null -ne $jobA) { Stop-OwnedJob $jobA $null }
}

  [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', $targetFull, 'Process')
  [Environment]::SetEnvironmentVariable('CARGO_INCREMENTAL', '0', 'Process')
  [Environment]::SetEnvironmentVariable('CARGO_BUILD_JOBS', '1', 'Process')
  Set-Location -LiteralPath $repo
  & npm.cmd run tauri build -- --no-bundle
  Assert-NativeExit 'cleanup preflight release build'
  $exactExe = Join-Path $targetFull 'release\uipilot.exe'
  $exeItem = Get-Item -LiteralPath $exactExe -Force
  if ($exeItem -isnot [IO.FileInfo] -or ($exeItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'cleanup preflight executable authentication failed'
  }
  $exactExeForFinal = $exeItem.FullName
  $activeJob = Start-OwnedJobPrimary $exeItem.FullName
  Invoke-OwnedSecondary $activeJob $exeItem.FullName
  $focusDeadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    try { Assert-OwnedJobForeground $activeJob; $focusOwned = $true } catch { $focusOwned = $false }
    if ($focusOwned) { break }
    Start-Sleep -Milliseconds 50
  } while ([DateTime]::UtcNow -lt $focusDeadline)
  if (-not $focusOwned) { throw 'owned Job foreground deadline' }
  Assert-OwnedJobPorts $activeJob
  $realAccounting = Get-JobAccounting $activeJob
  if ($realAccounting.TotalProcesses -lt 2 -or $realAccounting.ActiveProcesses -lt 2) {
    throw 'real WebView2 processes were not accounted to the Job'
  }
  $sentinelFailure = $null
  try { throw 'TASK7_JOB_CLEANUP_PREFLIGHT_SENTINEL' } catch { $sentinelFailure = $_ }
  Stop-OwnedJob $activeJob $exeItem.FullName
  $activeJob = $null
  if ($sentinelFailure.Exception.Message -cne 'TASK7_JOB_CLEANUP_PREFLIGHT_SENTINEL') {
    throw 'real Job cleanup did not preserve sentinel'
  }
  $preflightEvidence = [pscustomobject]@{
    JobFixtures = 3
    AssignmentBeforeResume = $true
    PositiveTotalProcessesAtLeast = 2
    WrongJobRejected = $true
    RealTotalProcessesAtLeast = 2
    ExactExecutableRemaining = @(Get-ExactExecutableRows $exeItem.FullName).Count
    ListenPortRemaining = @(Get-PortListeners).Count
  }
} catch {
  $originalFailure = $_
} finally {
  if ($null -ne $activeJob) {
    try { Stop-OwnedJob $activeJob $exeItem.FullName } catch { Add-CleanupFailure $cleanupFailures 'real Job cleanup' $_ }
    $activeJob = $null
  }
  try { [Environment]::SetEnvironmentVariable($browserArgsName, $originalBrowserArgs, 'Process') } catch { Add-CleanupFailure $cleanupFailures 'browser arguments' $_ }
  try { [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', $originalTarget, 'Process') } catch { Add-CleanupFailure $cleanupFailures 'CARGO_TARGET_DIR' $_ }
  try { [Environment]::SetEnvironmentVariable('CARGO_INCREMENTAL', $originalIncremental, 'Process') } catch { Add-CleanupFailure $cleanupFailures 'CARGO_INCREMENTAL' $_ }
  try { [Environment]::SetEnvironmentVariable('CARGO_BUILD_JOBS', $originalJobs, 'Process') } catch { Add-CleanupFailure $cleanupFailures 'CARGO_BUILD_JOBS' $_ }
  foreach ($path in $fixturePaths) {
    try { if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force } } catch { Add-CleanupFailure $cleanupFailures 'fixture file cleanup' $_ }
  }
  try {
    if (Test-Path -LiteralPath $targetFull) {
      $targetItem = Get-Item -LiteralPath $targetFull -Force
      if ($targetItem -isnot [IO.DirectoryInfo] -or ($targetItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
          -not [IO.Path]::GetFullPath($targetItem.FullName).StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'cleanup target type/path drifted'
      }
      Remove-Item -LiteralPath $targetItem.FullName -Recurse -Force
    }
  } catch { Add-CleanupFailure $cleanupFailures 'target cleanup' $_ }
  try {
    $finalProduct = @(& git -C $repo status --porcelain=v1 --untracked-files=all)
    Assert-NativeExit 'cleanup final product status'
    $finalDocs = @(& git -C $docs status --porcelain=v1 --untracked-files=all)
    Assert-NativeExit 'cleanup final docs status'
    $finalExact = if ($null -eq $exactExeForFinal) { 0 } else { @(Get-ExactExecutableRows $exactExeForFinal).Count }
    if ($finalProduct.Count -ne 0 -or $finalDocs.Count -ne 0 -or $finalExact -ne 0 -or @(Get-PortListeners).Count -ne 0 -or
        (Test-Path -LiteralPath $targetFull) -or @($fixturePaths | Where-Object { Test-Path -LiteralPath $_ }).Count -ne 0) {
      throw 'cleanup preflight left source/process/port/TEMP residue'
    }
  } catch { Add-CleanupFailure $cleanupFailures 'final residue verification' $_ }
}

if ($cleanupFailures.Count) {
  Write-Output "Cleanup=FAIL category=job-preflight count=$($cleanupFailures.Count)"
}
if ($null -ne $originalFailure) {
  if ($cleanupFailures.Count) { throw (Get-CombinedFailureMessage $originalFailure.Exception $cleanupFailures) }
  throw $originalFailure
}
if ($cleanupFailures.Count) {
  throw "cleanup failed count=$($cleanupFailures.Count) | $([string]::Join(' | ', @($cleanupFailures)))"
}
$preflightEvidence | Add-Member -NotePropertyName Cleanup -NotePropertyValue 'PASS'
$preflightEvidence | Format-List
```
<!-- TASK7_JOB_PREFLIGHT_END -->

Expected: the failure-preservation self-test, early-failure cleanup regression, and all three native no-product fixtures
pass; the clean-source build succeeds in one new TEMP target, and the real Job owns
the primary/secondary/WebView2 listener and foreground processes, Job accounting reaches active zero after one explicit
termination, the fixed sentinel remains primary, and final source/environment/process/Listen-port/TEMP state is clean.
Stop and return for written Design/Plan/Security review after this output; do not continue into the historical workflow.

After written Go, execute the fence only through this provenance launcher. It reads the approved Git blob rather than
trusting mutable working text, authenticates the exact docs path and source contract, writes one UTF-8-no-BOM TEMP
runner, checks the raw child exit immediately, and removes the runner on success or failure:

```powershell
$ErrorActionPreference = 'Stop'
$docs = 'D:\code\UiPilot_tools\.worktrees\foundation-task-7-design'
$expectedBranch = 'codex/foundation-task-7-design'
$relativePlan = 'docs/superpowers/plans/2026-07-19-foundation-task-7-keyboard-first-launcher-ui.md'
$goSha = [Environment]::GetEnvironmentVariable('TASK7_CLEANUP_PLAN_GO_SHA', 'Process')
$runnerPath = $null
$launcherFailure = $null
$cleanupFailure = $null

function Invoke-CheckedGit([string[]]$arguments, [string]$label) {
  $result = @(& git -C $docs @arguments)
  $exit = $LASTEXITCODE
  if ($exit -ne 0) { throw "$label failed with exit $exit" }
  @($result)
}

if ($goSha -cnotmatch '^[0-9a-f]{40}$') { throw 'cleanup Plan Go SHA is missing' }
$brokenScalarIndex = (Write-Output 'TASK7_PROVENANCE_SCALAR_FIXTURE')[0]
$approvedScalarRows = @(Write-Output 'TASK7_PROVENANCE_SCALAR_FIXTURE')
$approvedScalarIndex = $approvedScalarRows[0]
if ($brokenScalarIndex -isnot [char] -or [string]$brokenScalarIndex -cne 'T' -or
    $approvedScalarRows.Count -ne 1 -or $approvedScalarIndex -isnot [string] -or
    $approvedScalarIndex -cne 'TASK7_PROVENANCE_SCALAR_FIXTURE') {
  throw 'cleanup provenance scalar/array fixture failed'
}
Write-Output 'PlanProvenanceScalarArrayFixture=PASS count=1'
$rootRows = @(Invoke-CheckedGit @('rev-parse', '--show-toplevel') 'docs root')
if ($rootRows.Count -ne 1) { throw 'cleanup docs root row count drifted' }
$root = [IO.Path]::GetFullPath([string]$rootRows[0])
if (-not [string]::Equals($root, [IO.Path]::GetFullPath($docs), [StringComparison]::OrdinalIgnoreCase)) {
  throw 'cleanup docs root drifted'
}
$rootItem = Get-Item -LiteralPath $root -Force
if ($rootItem -isnot [IO.DirectoryInfo] -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
  throw 'cleanup docs root type drifted'
}
$branch = @(Invoke-CheckedGit @('symbolic-ref', '--short', 'HEAD') 'docs branch')
$head = @(Invoke-CheckedGit @('rev-parse', 'HEAD') 'docs HEAD')
$status = @(Invoke-CheckedGit @('status', '--porcelain=v1', '--untracked-files=all') 'docs status')
if ($branch.Count -ne 1 -or $branch[0] -cne $expectedBranch -or
    $head.Count -ne 1 -or $head[0] -cne $goSha -or $status.Count -ne 0) {
  throw 'cleanup approved docs branch/HEAD/status drifted'
}

$cursor = $rootItem
$segments = @($relativePlan -split '/')
for ($index = 0; $index -lt $segments.Count; $index++) {
  $matches = @(Get-ChildItem -LiteralPath $cursor.FullName -Force | Where-Object { $_.Name -ceq $segments[$index] })
  if ($matches.Count -ne 1) { throw 'cleanup plan case-sensitive path authentication failed' }
  $cursor = $matches[0]
  if (($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      ($index -lt $segments.Count - 1 -and $cursor -isnot [IO.DirectoryInfo]) -or
      ($index -eq $segments.Count - 1 -and $cursor -isnot [IO.FileInfo])) {
    throw 'cleanup plan path type/reparse authentication failed'
  }
}
$workingPlan = [IO.Path]::GetFullPath($cursor.FullName)
if (-not $workingPlan.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) {
  throw 'cleanup plan escaped docs root'
}
$approvedObject = @(Invoke-CheckedGit @('rev-parse', "${goSha}:$relativePlan") 'approved plan object')
$workingObject = @(Invoke-CheckedGit @('hash-object', "--path=$relativePlan", $workingPlan) 'working plan object')
if ($approvedObject.Count -ne 1 -or $workingObject.Count -ne 1 -or
    $approvedObject[0] -cne $workingObject[0]) {
  throw 'cleanup working plan blob differs from approved blob'
}
$flags = @(Invoke-CheckedGit @('ls-files', '-v', '--', $relativePlan) 'plan index flags')
if ($flags.Count -ne 1 -or $flags[0] -cmatch '^[a-zS] ') { throw 'cleanup plan has unsafe index flags' }

$approvedLines = @(Invoke-CheckedGit @('show', "${goSha}:$relativePlan") 'approved plan content')
$approvedText = [string]::Join("`n", $approvedLines)
$approvedPowerShellFences = [regex]::Matches($approvedText, '(?ms)^```powershell\r?\n(?<body>.*?)\r?\n```\s*$')
$approvedLaunchers = @($approvedPowerShellFences | Where-Object { $_.Groups['body'].Value.Contains('function Invoke-CheckedGit') })
if ($approvedLaunchers.Count -ne 1) { throw 'cleanup approved plan must contain one provenance launcher' }
$launcherSource = $approvedLaunchers[0].Groups['body'].Value
$launcherTokens = $null
$launcherErrors = $null
$launcherAst = [Management.Automation.Language.Parser]::ParseInput(
  $launcherSource, [ref]$launcherTokens, [ref]$launcherErrors)
if ($launcherErrors.Count -ne 0) { throw 'cleanup provenance launcher PowerShell AST failed' }
$directGitIndexes = @($launcherAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.IndexExpressionAst] -and
    $node.Target -is [Management.Automation.Language.ParenExpressionAst] -and
    $node.Target.Extent.Text.Contains('Invoke-CheckedGit')
}, $true))
if ($directGitIndexes.Count -ne 0) { throw 'cleanup direct scalar Git indexing remains' }
$launcherAssignments = @($launcherAst.FindAll({
  param($node) $node -is [Management.Automation.Language.AssignmentStatementAst]
}, $true))
$rootRowsAssignments = @($launcherAssignments | Where-Object {
  $_.Extent.Text -ceq '$rootRows = @(Invoke-CheckedGit @(''rev-parse'', ''--show-toplevel'') ''docs root'')'
})
$rootAssignments = @($launcherAssignments | Where-Object {
  $_.Extent.Text -ceq '$root = [IO.Path]::GetFullPath([string]$rootRows[0])'
})
$rootCountChecks = @($launcherAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.IfStatementAst] -and
    $node.Extent.Text.Contains('$rootRows.Count -ne 1')
}, $true))
if ($rootRowsAssignments.Count -ne 1 -or $rootAssignments.Count -ne 1 -or $rootCountChecks.Count -ne 1) {
  throw 'cleanup exact one-row root validation drifted'
}
$scalarFixtureOutput = @($launcherAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'Write-Output' -and
    $node.Extent.Text -ceq "Write-Output 'PlanProvenanceScalarArrayFixture=PASS count=1'"
}, $true))
$scalarFixtureToken = 'TASK7_PROVENANCE_' + 'SCALAR_FIXTURE'
if ($scalarFixtureOutput.Count -ne 1 -or
    [regex]::Matches($launcherSource, [regex]::Escape($scalarFixtureToken)).Count -ne 3) {
  throw 'cleanup provenance scalar/array fixture source drifted'
}
$pattern = '(?s)<!-- TASK7_JOB_PREFLIGHT_BEGIN -->\s*```powershell\r?\n(?<workflow>.*?)\r?\n```\s*<!-- TASK7_JOB_PREFLIGHT_END -->'
$matches = [regex]::Matches($approvedText, $pattern)
if ($matches.Count -ne 1) { throw 'cleanup approved plan must contain one Job preflight fence' }
$workflow = $matches[0].Groups['workflow'].Value
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($workflow, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw 'cleanup Job preflight PowerShell AST failed' }
$functions = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $true))
$requiredFunctions = @(
  'Assert-NativeExit', 'Add-CleanupFailure', 'Get-CombinedFailureMessage', 'Close-OwnedHandle', 'Get-ExactExecutableRows', 'Get-PortListeners',
  'Get-JobAccounting', 'New-OwnedJob', 'Get-OwnedProcessPath', 'Start-SuspendedOwnedProcess', 'Wait-OwnedProcess',
  'Assert-HandleInOwnedJob', 'Assert-PidInOwnedJob', 'Assert-OwnedJobPorts', 'Assert-OwnedJobForeground',
  'Stop-OwnedJob', 'Start-OwnedJobPrimary', 'Invoke-OwnedSecondary', 'New-FixtureScript'
)
foreach ($name in $requiredFunctions) {
  if (@($functions | Where-Object { $_.Name -ceq $name }).Count -ne 1) {
    throw "cleanup required function drifted: $name"
  }
}
if ($functions.Count -ne $requiredFunctions.Count) { throw 'cleanup unexpected function added' }
function Get-FunctionText([string]$name) {
  $matchedFunctions = @($functions | Where-Object { $_.Name -ceq $name })
  $matchedFunctions[0].Extent.Text
}
$startText = Get-FunctionText 'Start-SuspendedOwnedProcess'
$assignAt = $startText.IndexOf('[Task7OwnedJob]::AssignProcessToJobObject', [StringComparison]::Ordinal)
$resumeAt = $startText.IndexOf('[Task7OwnedJob]::ResumeThread', [StringComparison]::Ordinal)
if ($assignAt -lt 0 -or $resumeAt -lt 0 -or $assignAt -ge $resumeAt) {
  throw 'cleanup assign-before-resume ownership order drifted'
}
$portCommands = @($ast.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Get-NetTCPConnection'
}, $true))
if ($portCommands.Count -ne 1 -or $portCommands[0].Extent.Text -cnotmatch '(?s)-State\s+Listen') {
  throw 'cleanup Listen-port inventory drifted'
}
$fixtureCalls = @($ast.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'New-FixtureScript'
}, $true))
if ($fixtureCalls.Count -ne 4) { throw 'cleanup fixture inventory drifted' }
$firstFixtureCall = @($fixtureCalls | Sort-Object { $_.Extent.StartOffset })[0]
$buildCommands = @($ast.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'npm.cmd' -and
    $node.Extent.Text.Contains('run tauri build -- --no-bundle')
}, $true))
if ($buildCommands.Count -ne 1) { throw 'cleanup build command inventory drifted' }
$firstFixtureOwners = [Collections.Generic.List[object]]::new()
$cursor = $firstFixtureCall.Parent
while ($null -ne $cursor) {
  if ($cursor -is [Management.Automation.Language.TryStatementAst]) { $firstFixtureOwners.Add($cursor) }
  $cursor = $cursor.Parent
}
$buildOwners = [Collections.Generic.List[object]]::new()
$cursor = $buildCommands[0].Parent
while ($null -ne $cursor) {
  if ($cursor -is [Management.Automation.Language.TryStatementAst]) { $buildOwners.Add($cursor) }
  $cursor = $cursor.Parent
}
if ($firstFixtureOwners.Count -ne 1 -or $buildOwners.Count -ne 1 -or
    -not [object]::ReferenceEquals($firstFixtureOwners[0], $buildOwners[0]) -or
    $null -eq $firstFixtureOwners[0].Finally -or
    -not $firstFixtureOwners[0].Finally.Extent.Text.Contains('foreach ($path in $fixturePaths)') -or
    -not $firstFixtureOwners[0].Finally.Extent.Text.Contains("Add-CleanupFailure `$cleanupFailures 'fixture file cleanup' `$_")) {
  throw 'cleanup fixture/build outer ownership drifted'
}
$earlyFailureOutputs = @($ast.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'Write-Output' -and
    $node.Extent.Text -ceq "Write-Output 'EarlyFixtureFailureCleanupRegression=PASS count=1'"
}, $true))
$earlyFailureToken = 'TASK7_EARLY_FIXTURE_' + 'ORIGINAL_SENTINEL'
if ($earlyFailureOutputs.Count -ne 1 -or
    [regex]::Matches($workflow, [regex]::Escape($earlyFailureToken)).Count -ne 2) {
  throw 'cleanup early fixture failure regression drifted'
}
$exactCounts = @{
  'CreateProcessW' = 3
  'AssignProcessToJobObject' = 3
  'ResumeThread' = 3
  'TerminateProcess' = 3
  'CREATE_SUSPENDED' = 2
  'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE' = 2
}
foreach ($entry in $exactCounts.GetEnumerator()) {
  if ([regex]::Matches($workflow, [regex]::Escape([string]$entry.Key)).Count -ne [int]$entry.Value) {
    throw "cleanup native source count drifted: $($entry.Key)"
  }
}
foreach ($forbidden in @(
    'CREATE_BREAKAWAY_FROM_JOB', 'ParentProcessId', 'CreationDate', 'ExitCutoffUtc',
    'Discover-OwnedDescendants', 'Stop-Process', 'taskkill', '.Kill(', 'CompletionPort')) {
  if ($workflow.IndexOf($forbidden, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
    throw "cleanup forbidden ownership token present: $forbidden"
  }
}
foreach ($discardedNativeResult in @(
    '\[void\]\s*\[Task7OwnedJob\]::TerminateJobObject',
    '\[void\]\s*\[Task7OwnedJob\]::TerminateProcess',
    '\[void\]\s*\[Task7OwnedJob\]::WaitForSingleObject')) {
  if ([regex]::IsMatch($workflow, $discardedNativeResult)) {
    throw "cleanup discarded native result present: $discardedNativeResult"
  }
}
if ([regex]::IsMatch($workflow, 'catch\s*\{\s*\}')) { throw 'cleanup empty catch present' }
$startFailureText = Get-FunctionText 'Start-SuspendedOwnedProcess'
if ($startFailureText -cnotmatch 'if \(-not \[Task7OwnedJob\]::TerminateJobObject' -or
    $startFailureText -cnotmatch 'elseif \(-not \[Task7OwnedJob\]::TerminateProcess' -or
    $startFailureText -cnotmatch '\$failedWait\s*=\s*\[Task7OwnedJob\]::WaitForSingleObject' -or
    $startFailureText -cnotmatch '\$failedWait\s+-ne\s+\[Task7OwnedJob\]::WAIT_OBJECT_0') {
  throw 'cleanup suspended-launch native result checks drifted'
}
$failureSentinelCounts = @{
  'TASK7_BROWSER_ONLY_SENTINEL' = 3
  'TASK7_LAUNCH_COMBINED_SENTINEL' = 2
  'TASK7_BROWSER_COMBINED_SENTINEL' = 2
  'TASK7_JOB_CLEANUP_SENTINEL' = 2
}
foreach ($failureSentinel in $failureSentinelCounts.GetEnumerator()) {
  if ([regex]::Matches($workflow, [string]$failureSentinel.Key).Count -ne [int]$failureSentinel.Value) {
    throw "cleanup failure-preservation self-test drifted: $($failureSentinel.Key)"
  }
}
$primaryFailureText = Get-FunctionText 'Start-OwnedJobPrimary'
foreach ($requiredPrimaryFailureToken in @(
    '$launchFailure = $null', '$browserRestoreFailure = $null',
    "Add-CleanupFailure `$localCleanupFailures 'browser arguments restore' `$browserRestoreFailure",
    "Add-CleanupFailure `$localCleanupFailures 'primary Job cleanup' `$_")) {
  if ($primaryFailureText.IndexOf($requiredPrimaryFailureToken, [StringComparison]::Ordinal) -lt 0) {
    throw "cleanup primary failure preservation drifted: $requiredPrimaryFailureToken"
  }
}
if ([regex]::Matches($primaryFailureText, '\$null\s+-ne\s+\$browserRestoreFailure').Count -ne 2 -or
    $primaryFailureText -match '\$launchFailure[^\r\n]*-and[^\r\n]*\$browserRestoreFailure') {
  throw 'cleanup browser restore branch labeling drifted'
}
$environmentCleanupLines = @(
  'try { [Environment]::SetEnvironmentVariable($browserArgsName, $originalBrowserArgs, ''Process'') } catch { Add-CleanupFailure $cleanupFailures ''browser arguments'' $_ }',
  'try { [Environment]::SetEnvironmentVariable(''CARGO_TARGET_DIR'', $originalTarget, ''Process'') } catch { Add-CleanupFailure $cleanupFailures ''CARGO_TARGET_DIR'' $_ }',
  'try { [Environment]::SetEnvironmentVariable(''CARGO_INCREMENTAL'', $originalIncremental, ''Process'') } catch { Add-CleanupFailure $cleanupFailures ''CARGO_INCREMENTAL'' $_ }',
  'try { [Environment]::SetEnvironmentVariable(''CARGO_BUILD_JOBS'', $originalJobs, ''Process'') } catch { Add-CleanupFailure $cleanupFailures ''CARGO_BUILD_JOBS'' $_ }'
)
foreach ($environmentCleanupLine in $environmentCleanupLines) {
  if ([regex]::Matches($workflow, [regex]::Escape($environmentCleanupLine)).Count -ne 1) {
    throw "cleanup independent environment restoration drifted: $environmentCleanupLine"
  }
}
if ((Get-FunctionText 'New-OwnedJob').IndexOf('CreateJobObjectW([IntPtr]::Zero, $null)', [StringComparison]::Ordinal) -lt 0 -or
    (Get-FunctionText 'Stop-OwnedJob').IndexOf('[Task7OwnedJob]::TerminateJobObject', [StringComparison]::Ordinal) -lt 0) {
  throw 'cleanup unnamed Job or Job-level termination drifted'
}
$cleanupThrowAt = $workflow.IndexOf('throw "cleanup failed count=$($cleanupFailures.Count) |', [StringComparison]::Ordinal)
$cleanupPassAt = $workflow.IndexOf('$preflightEvidence | Add-Member -NotePropertyName Cleanup', [StringComparison]::Ordinal)
if ($cleanupThrowAt -lt 0 -or $cleanupPassAt -lt 0 -or $cleanupThrowAt -ge $cleanupPassAt -or
    [regex]::Matches($workflow, "-NotePropertyValue 'PASS'").Count -ne 1) {
  throw 'cleanup PASS ordering drifted'
}

$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$tempItem = Get-Item -LiteralPath $tempRoot -Force
if ($tempItem -isnot [IO.DirectoryInfo] -or ($tempItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
  throw 'cleanup launcher TEMP root authentication failed'
}
$runnerPath = Join-Path $tempRoot ("uipilot-task7-job-preflight-$([guid]::NewGuid().ToString('N')).ps1")
$runnerFull = [IO.Path]::GetFullPath($runnerPath)
if (-not $runnerFull.StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
    (Test-Path -LiteralPath $runnerFull)) {
  throw 'cleanup launcher runner path authentication failed'
}
$utf8NoBom = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText($runnerFull, $workflow + "`r`n", $utf8NoBom)
$runnerItem = Get-Item -LiteralPath $runnerFull -Force
if ($runnerItem -isnot [IO.FileInfo] -or ($runnerItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
  throw 'cleanup launcher runner type drifted'
}
$sha256 = [Security.Cryptography.SHA256]::Create()
try {
  $expectedHash = $sha256.ComputeHash($utf8NoBom.GetBytes($workflow + "`r`n"))
  $actualHash = $sha256.ComputeHash([IO.File]::ReadAllBytes($runnerFull))
} finally {
  $sha256.Dispose()
}
if ([BitConverter]::ToString($expectedHash) -cne [BitConverter]::ToString($actualHash)) {
  throw 'cleanup launcher runner hash drifted'
}
$runnerTokens = $null
$runnerErrors = $null
[void][Management.Automation.Language.Parser]::ParseFile($runnerFull, [ref]$runnerTokens, [ref]$runnerErrors)
if ($runnerErrors.Count -ne 0) { throw 'cleanup launcher runner AST drifted' }

try {
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runnerFull
  $runnerExit = $LASTEXITCODE
  if ($runnerExit -ne 0) { throw "cleanup Job preflight runner failed with exit $runnerExit" }
} catch {
  $launcherFailure = $_
} finally {
  try {
    if ($null -ne $runnerPath -and (Test-Path -LiteralPath $runnerPath)) {
      $deleteItem = Get-Item -LiteralPath $runnerPath -Force
      if ($deleteItem -isnot [IO.FileInfo] -or ($deleteItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
          -not [IO.Path]::GetFullPath($deleteItem.FullName).StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'cleanup launcher runner delete authentication failed'
      }
      Remove-Item -LiteralPath $deleteItem.FullName -Force
    }
    if ($null -ne $runnerPath -and (Test-Path -LiteralPath $runnerPath)) {
      throw 'cleanup launcher runner residue remains'
    }
  } catch { $cleanupFailure = $_ }
}
if ($null -ne $launcherFailure -and $null -ne $cleanupFailure) {
  throw "$($launcherFailure.Exception.Message) | launcher cleanup: $($cleanupFailure.Exception.Message)"
}
if ($null -ne $launcherFailure) { throw $launcherFailure }
if ($null -ne $cleanupFailure) { throw $cleanupFailure }
```

Expected: provenance, exact source oracles, runner hash/AST, the fenced preflight, and runner cleanup all pass. Any
provenance, fixture, build, ownership, cleanup, or residue failure remains No-Go and does not authorize a retry.

The following historical material remains for audit only and is permanently non-executable. No later docs revision may
reactivate it by replacing or bypassing its ownership functions:

Use one temporary, uncommitted `src/main.ts` measurement wrapper around the already approved adapter. For cold startup, wrap `client.listenShown`: after the real listener Promise resolves, record `performance.now()` before `load_settings` starts. View-ready already guarantees mount/native binding, and `performance.now()` is elapsed milliseconds since `performance.timeOrigin`, so backend settings latency is not mixed into this UI metric. Set `startupComplete = true` only after `core.start()` resolves. For warm show, increment `shownObserved` at the real shown callback, record `performance.now()`, then after core handling use two nested `requestAnimationFrame` callbacks so the first updated frame has painted. Increment `paintObserved` at the second callback before checking focus. If the input is not focused/enabled, increment `focusFailureObserved` and record no bucket; otherwise increment `bucketObserved` exactly when the event advances warmup or appends to the empty/preserved array. Discard the first five successful painted events globally, then record exactly 100 empty-query and 100 preserved-query samples.

The wrapper exposes only a temporary local CDP read seam, never query/UI text or individual warm samples:

```ts
const HARNESS_QUERY = 'calc'

Object.defineProperty(globalThis, '__UIPILOT_TASK7_PERF__', {
  configurable: true,
  value: Object.freeze({
    read: () => ({
      coldReadyMs,
      startupComplete,
      queryEmpty: core.getSnapshot().query === '',
      queryInputFocused: document.activeElement === queryInput && !queryInput.disabled,
      queryMatchesHarnessValue: core.getSnapshot().query === HARNESS_QUERY,
      warmupObserved,
      empty: aggregate(emptySamples, 100),
      preserved: aggregate(preservedSamples, 100),
      shownObserved,
      paintObserved,
      focusFailureObserved,
      bucketObserved,
    }),
  }),
})
```

`HARNESS_QUERY` is the fixed local literal `calc`; only the equality boolean above leaves the wrapper. `aggregate` always returns exact `{ complete, count, p95 }`, with `p95: null` until `count === 100`; at completion it sorts a copy and returns index `Math.ceil(0.95 * 100) - 1`. The warm callback increments `warmupObserved` through five without storing those timings, then appends to exactly one array according to the committed query's emptiness. The four stage counters are monotonic nonnegative 32-bit integers and never contain timing, text, values, query, IDs, or payload. The seam never returns individual timing values. Source inspection must show the wrapper contains no command/event/logging change and the only diff is `src/main.ts`.

Build the measurement artifact from that exact wrapper, not from the prior clean Vite/Tauri output:

```powershell
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$dirty = @(& git diff --name-only)
Assert-NativeExit 'measurement-wrapper diff inventory'
$staged = @(& git diff --cached --name-only)
Assert-NativeExit 'measurement-wrapper staged inventory'
if ($dirty.Count -ne 1 -or $dirty[0] -cne 'src/main.ts' -or $staged.Count -ne 0) { throw 'measurement wrapper scope drifted' }
$measurementBuildStarted = (Get-Date).ToUniversalTime()
npm.cmd run tauri build -- --no-bundle
Assert-NativeExit 'measurement-wrapper Tauri release build'
$metadataJson = @(cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps)
Assert-NativeExit 'measurement cargo metadata'
try { $metadata = ($metadataJson -join "`n") | ConvertFrom-Json } catch { throw 'measurement cargo metadata returned invalid JSON' }
$measurementExe = Get-Item -LiteralPath (Join-Path ([string]$metadata.target_directory) 'release\uipilot.exe') -Force
if ($measurementExe -isnot [IO.FileInfo] -or $measurementExe.LastWriteTimeUtc -lt $measurementBuildStarted) {
  throw 'measurement release executable was not rebuilt from the wrapper'
}
$measurementHash = (Get-FileHash -LiteralPath $measurementExe.FullName -Algorithm SHA256).Hash
```

Run this complete disposable workflow from PowerShell 5.1. It uses the approved Task 6 second-instance callback as the only event producer: starting the exact same executable with no arguments while the primary is alive must route to `ShowTarget::Launcher`, ignore argv/cwd, and exit. It never synthesizes `launcher://shown`, adds a command, or uses Vite/Tauri dev.

<!-- TASK7_PERF_WORKFLOW_BEGIN -->
```powershell
$ErrorActionPreference = 'Stop'
$expectedMeasurementHash = [Environment]::GetEnvironmentVariable('TASK7_MEASUREMENT_SHA256', 'Process')
if ($expectedMeasurementHash -cnotmatch '^[0-9A-F]{64}$') { throw 'authenticated measurement SHA-256 is missing' }
$metadataJson = @(cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps)
if ($LASTEXITCODE -ne 0) { throw 'performance cargo metadata failed' }
try { $metadata = ($metadataJson -join "`n") | ConvertFrom-Json } catch { throw 'performance cargo metadata returned invalid JSON' }
$measurementExe = Get-Item -LiteralPath (Join-Path ([string]$metadata.target_directory) 'release\uipilot.exe') -Force
if ($measurementExe -isnot [IO.FileInfo] -or
    (Get-FileHash -LiteralPath $measurementExe.FullName -Algorithm SHA256).Hash -cne $expectedMeasurementHash) {
  throw 'performance executable does not match the authenticated measurement artifact'
}
$port = 9227
$browserArgsName = 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'
$originalBrowserArgs = [Environment]::GetEnvironmentVariable($browserArgsName, 'Process')
if ($null -ne $originalBrowserArgs) { throw 'measurement requires no pre-existing WebView2 browser arguments' }
$collectorPath = Join-Path $env:TEMP ("uipilot-task7-cdp-$([guid]::NewGuid().ToString('N')).mjs")
$activePrimary = $null
$activeCollector = $null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class Task7ForegroundWindow {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}

public static class Task7UnicodeInput {
    public const uint INPUT_KEYBOARD = 1;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint KEYEVENTF_UNICODE = 0x0004;

    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT {
        public int dx;
        public int dy;
        public uint mouseData;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HARDWAREINPUT {
        public uint uMsg;
        public ushort wParamL;
        public ushort wParamH;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public HARDWAREINPUT hi;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT {
        public uint type;
        public INPUTUNION U;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint cInputs, INPUT[] pInputs, int cbSize);
}
'@

$harnessQuery = 'calc'

function Invoke-HarnessQueryInput([switch]$ValidateOnly) {
  if ($harnessQuery -cne 'calc' -or $harnessQuery.Length -ne 4) {
    throw 'fixed harness query drifted'
  }
  $inputSize = [Runtime.InteropServices.Marshal]::SizeOf([type][Task7UnicodeInput+INPUT])
  $unionSize = [Runtime.InteropServices.Marshal]::SizeOf([type][Task7UnicodeInput+INPUTUNION])
  $expectedInputSize = if ([IntPtr]::Size -eq 8) { 40 } else { 28 }
  $expectedUnionSize = if ([IntPtr]::Size -eq 8) { 32 } else { 24 }
  if ($inputSize -ne $expectedInputSize) { throw 'Win32 INPUT layout size drifted' }
  if ($unionSize -ne $expectedUnionSize) { throw 'Win32 INPUT union layout size drifted' }

  $inputs = [Task7UnicodeInput+INPUT[]]::new(8)
  for ($index = 0; $index -lt $harnessQuery.Length; $index++) {
    $scan = [uint16][char]$harnessQuery[$index]
    foreach ($keyUp in @($false, $true)) {
      $key = [Task7UnicodeInput+KEYBDINPUT]::new()
      $key.wVk = 0
      $key.wScan = $scan
      $key.dwFlags = [Task7UnicodeInput]::KEYEVENTF_UNICODE
      if ($keyUp) { $key.dwFlags = $key.dwFlags -bor [Task7UnicodeInput]::KEYEVENTF_KEYUP }
      $union = [Task7UnicodeInput+INPUTUNION]::new()
      $union.ki = $key
      $input = [Task7UnicodeInput+INPUT]::new()
      $input.type = [Task7UnicodeInput]::INPUT_KEYBOARD
      $input.U = $union
      $inputs[($index * 2) + [int]$keyUp] = $input
    }
  }

  if ($inputs.Length -ne 8) { throw 'fixed harness INPUT count drifted' }
  for ($index = 0; $index -lt 4; $index++) {
    $scan = [uint16][char]$harnessQuery[$index]
    $down = $inputs[$index * 2]
    $up = $inputs[($index * 2) + 1]
    if ($down.type -ne [Task7UnicodeInput]::INPUT_KEYBOARD -or $down.U.ki.wVk -ne 0 -or
        $down.U.ki.wScan -ne $scan -or $down.U.ki.dwFlags -ne [Task7UnicodeInput]::KEYEVENTF_UNICODE -or
        $up.type -ne [Task7UnicodeInput]::INPUT_KEYBOARD -or $up.U.ki.wVk -ne 0 -or
        $up.U.ki.wScan -ne $scan -or
        $up.U.ki.dwFlags -ne ([Task7UnicodeInput]::KEYEVENTF_UNICODE -bor [Task7UnicodeInput]::KEYEVENTF_KEYUP)) {
      throw 'fixed harness Unicode INPUT layout drifted'
    }
  }

  if ($ValidateOnly) {
    [pscustomobject]@{
      InputCount = $inputs.Length
      InputSize = $inputSize
      UnionSize = $unionSize
      QueryUnits = $harnessQuery.Length
    }
    return
  }
  $sent = [Task7UnicodeInput]::SendInput([uint32]$inputs.Length, $inputs, $inputSize)
  $win32Error = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
  if ($sent -ne 8) { throw "fixed harness SendInput failed expected=8 actual=$sent win32=$win32Error" }
}

$unicodeInputSelfTest = Invoke-HarnessQueryInput -ValidateOnly
if ($unicodeInputSelfTest.InputCount -ne 8 -or $unicodeInputSelfTest.QueryUnits -ne 4 -or
    $unicodeInputSelfTest.InputSize -ne $(if ([IntPtr]::Size -eq 8) { 40 } else { 28 }) -or
    $unicodeInputSelfTest.UnionSize -ne $(if ([IntPtr]::Size -eq 8) { 32 } else { 24 })) {
  throw 'fixed harness Unicode INPUT no-injection self-test failed'
}

function Get-ProcessRows {
  @(Get-CimInstance Win32_Process -ErrorAction Stop)
}

function Get-ExactExecutableRows([string]$path) {
  @(Get-ProcessRows | Where-Object {
    $null -ne $_.ExecutablePath -and [string]::Equals($_.ExecutablePath, $path, [StringComparison]::OrdinalIgnoreCase)
  })
}

[long]$ticksPerCimMicrosecond = [TimeSpan]::TicksPerMillisecond / 1000

function Convert-ToCimMicrosecondUtc([DateTime]$value) {
  $utc = $value.ToUniversalTime()
  [DateTime]::new(
    [long]($utc.Ticks - ($utc.Ticks % $ticksPerCimMicrosecond)),
    [DateTimeKind]::Utc
  )
}

function Get-RowStartUtc($row) {
  Convert-ToCimMicrosecondUtc ([DateTime]$row.CreationDate)
}

function Get-ProcessStartUtc([Diagnostics.Process]$process) {
  Convert-ToCimMicrosecondUtc $process.StartTime
}

$self = [Diagnostics.Process]::GetCurrentProcess()
try {
  $selfRows = @(Get-CimInstance Win32_Process -Filter "ProcessId=$($self.Id)" -ErrorAction Stop)
  if ($selfRows.Count -ne 1) { throw 'current-process CIM positive-control row count drifted' }
  $rawProcessUtc = $self.StartTime.ToUniversalTime()
  $rawCimUtc = ([DateTime]$selfRows[0].CreationDate).ToUniversalTime()
  if ([Math]::Abs($rawProcessUtc.Ticks - $rawCimUtc.Ticks) -ge $ticksPerCimMicrosecond) {
    throw 'current-process raw clocks differ by at least one CIM microsecond'
  }
  $normalizedProcessUtc = Convert-ToCimMicrosecondUtc $rawProcessUtc
  $normalizedCimUtc = Convert-ToCimMicrosecondUtc $rawCimUtc
  if ($normalizedProcessUtc -ne $normalizedCimUtc) { throw 'current-process normalized clocks differ' }
  $oneMicrosecondLater = Convert-ToCimMicrosecondUtc ($rawCimUtc.AddTicks($ticksPerCimMicrosecond))
  if (($oneMicrosecondLater.Ticks - $normalizedCimUtc.Ticks) -ne $ticksPerCimMicrosecond -or
      $normalizedProcessUtc -eq $oneMicrosecondLater) {
    throw 'exact one-microsecond identity mismatch was not rejected'
  }
} finally {
  $self.Dispose()
}

function Assert-ProcessIdentity([Diagnostics.Process]$process, $row, [string]$label) {
  if ($process.Id -ne [int]$row.ProcessId -or (Get-ProcessStartUtc $process) -ne (Get-RowStartUtc $row)) {
    throw "$label process identity mismatch"
  }
  [void]$process.SafeHandle
}

function Open-VerifiedProcess($row, [string]$label) {
  try { $process = [Diagnostics.Process]::GetProcessById([int]$row.ProcessId) } catch [ArgumentException] { return $null }
  try {
    Assert-ProcessIdentity $process $row $label
    $process
  } catch {
    $process.Dispose()
    throw
  }
}

function New-OwnedRecord([Diagnostics.Process]$process, [int]$ownedPid, [DateTime]$startUtc, [int]$parentPid) {
  [pscustomobject]@{
    Pid = $ownedPid
    StartUtc = $startUtc
    ParentPid = $parentPid
    Handle = $process
    ExitCutoffUtc = $null
  }
}

$recordCheckProcess = [Diagnostics.Process]::GetCurrentProcess()
try {
  $recordCheckStart = Get-ProcessStartUtc $recordCheckProcess
  $recordCheck = New-OwnedRecord $recordCheckProcess $recordCheckProcess.Id $recordCheckStart 17
  if ($recordCheck.Pid -ne $recordCheckProcess.Id -or $recordCheck.StartUtc -ne $recordCheckStart -or
      $recordCheck.ParentPid -ne 17 -or $recordCheck.Handle -ne $recordCheckProcess) {
    throw 'New-OwnedRecord PowerShell 5.1 self-check failed'
  }
} finally {
  $recordCheckProcess.Dispose()
}

function New-OwnedSet([Diagnostics.Process]$primary) {
  [void]$primary.SafeHandle
  $rows = @(Get-CimInstance Win32_Process -Filter "ProcessId=$($primary.Id)" -ErrorAction Stop)
  if ($rows.Count -gt 1) { throw 'duplicate primary process row' }
  if ($rows.Count -eq 1) { Assert-ProcessIdentity $primary $rows[0] 'primary' }
  $startUtc = Get-ProcessStartUtc $primary
  $parentPid = if ($rows.Count -eq 1) { [int]$rows[0].ParentProcessId } else { 0 }
  $owned = @{}
  $owned[$primary.Id] = New-OwnedRecord $primary $primary.Id $startUtc $parentPid
  $owned
}

function Update-ExitCutoff($record) {
  if ($record.Handle.HasExited -and $null -eq $record.ExitCutoffUtc) {
    $record.ExitCutoffUtc = Convert-ToCimMicrosecondUtc $record.Handle.ExitTime
  }
}

function Discover-OwnedDescendants([hashtable]$owned) {
  $rows = @(Get-ProcessRows)
  foreach ($record in $owned.Values) {
    $samePid = @($rows | Where-Object { [int]$_.ProcessId -eq [int]$record.Pid })
    if ($samePid.Count -gt 1) { throw 'duplicate PID rows during ownership scan' }
    if ($samePid.Count -eq 1 -and (Get-RowStartUtc $samePid[0]) -ne $record.StartUtc) {
      throw 'PID reuse ambiguity during ownership scan'
    }
    Update-ExitCutoff $record
  }
  $added = 0
  do {
    $changed = $false
    foreach ($row in $rows) {
      $parentPid = [int]$row.ParentProcessId
      if (-not $owned.ContainsKey($parentPid)) { continue }
      $parent = $owned[$parentPid]
      $childPid = [int]$row.ProcessId
      $childStart = Get-RowStartUtc $row
      if ($childStart -lt $parent.StartUtc) { throw 'child predates authenticated parent identity' }
      Update-ExitCutoff $parent
      if ($parent.Handle.HasExited -and $childStart -gt $parent.ExitCutoffUtc) {
        throw 'child postdates authenticated parent exit cutoff'
      }
      if ($owned.ContainsKey($childPid)) {
        if ($owned[$childPid].StartUtc -ne $childStart) { throw 'owned PID was reused' }
        continue
      }
      $handle = Open-VerifiedProcess $row 'descendant'
      if ($null -eq $handle) { continue }
      Update-ExitCutoff $parent
      if ($parent.Handle.HasExited -and $childStart -gt $parent.ExitCutoffUtc) {
        $handle.Dispose()
        throw 'verified child postdates authenticated parent exit cutoff'
      }
      $owned[$childPid] = New-OwnedRecord $handle $childPid $childStart $parentPid
      $added++
      $changed = $true
    }
  } while ($changed)
  $added
}

function Dispose-OwnedChildren([hashtable]$owned, [int]$rootPid) {
  foreach ($record in @($owned.Values | Where-Object { $_.Pid -ne $rootPid })) {
    $record.Handle.Dispose()
  }
}

function Get-PortListeners {
  @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object { $_.LocalPort -eq $port })
}

function Restore-BrowserArguments {
  [Environment]::SetEnvironmentVariable($browserArgsName, $originalBrowserArgs, 'Process')
}

function Get-SanitizedCollectorStderr($collector) {
  if ($collector.StderrConsumed) { return [string]$collector.SanitizedStderr }
  try {
    if (-not $collector.StderrTask.Wait(2000)) { throw 'collector stderr drain did not complete' }
    if ($collector.StderrTask.IsFaulted -or $collector.StderrTask.IsCanceled) {
      throw 'collector stderr drain faulted'
    }
    $safeLines = @([string]$collector.StderrTask.Result -split '\r?\n' | ForEach-Object { $_.Trim() } | Where-Object {
      $_.Length -ge 1 -and $_.Length -le 160 -and $_ -cmatch '^(?:Error: )?[A-Za-z0-9][A-Za-z0-9 _:-]*$'
    } | Select-Object -Unique -First 4)
    $sanitized = if ($safeLines.Count) { $safeLines -join ' | ' } else { 'collector-stderr-unclassified' }
    if ($sanitized.Length -gt 512) { $sanitized = $sanitized.Substring(0, 512) }
    $collector.SanitizedStderr = $sanitized
    $collector.StderrConsumed = $true
    $sanitized
  } catch {
    $collector.SanitizedStderr = 'collector-stderr-unavailable'
    $collector.StderrConsumed = $true
    throw
  }
}

function Stop-OwnedCollector($collector) {
  if ($null -eq $collector) { return }
  if ($collector.Stopped) { return }
  $collector.Stopped = $true
  $failures = [Collections.Generic.List[string]]::new()
  $process = $collector.Process
  try { [void]$process.SafeHandle } catch { [void]$failures.Add("collector handle: $($_.Exception.Message)") }
  try { $process.StandardInput.Close() } catch { [void]$failures.Add("collector stdin: $($_.Exception.Message)") }
  try {
    if (-not $process.WaitForExit(2000)) {
      $process.Kill()
      if (-not $process.WaitForExit(2000)) { throw 'collector handle did not exit after kill' }
    }
  } catch {
    [void]$failures.Add("collector process: $($_.Exception.Message)")
  }
  if ($process.HasExited) {
    try { [void](Get-SanitizedCollectorStderr $collector) } catch {
      [void]$failures.Add("collector stderr: $($_.Exception.Message)")
    }
  }
  if (-not $collector.StderrTask.IsCompleted) {
    [void]$failures.Add('collector stderr task remained incomplete')
  } else {
    try { $collector.StderrTask.Dispose() } catch { [void]$failures.Add("collector stderr task disposal: $($_.Exception.Message)") }
  }
  if (-not $collector.Disposed) {
    try { $process.Dispose() } catch { [void]$failures.Add("collector handle disposal: $($_.Exception.Message)") }
    $collector.Disposed = $true
  }
  if ($failures.Count) { throw "collector cleanup failed: $($failures -join ' | ')" }
}

function Add-CleanupFailure([Collections.Generic.List[string]]$failures, [string]$label, $errorRecord) {
  $message = "${label}: $($errorRecord.Exception.Message)"
  if (-not $failures.Contains($message)) { [void]$failures.Add($message) }
}

function Stop-OwnedTree([Diagnostics.Process]$primary, [string]$exactExe) {
  if ($null -eq $primary) { return }
  $failures = [Collections.Generic.List[string]]::new()
  $owned = @{}
  $rootPid = $primary.Id
  try {
    try {
      $owned = New-OwnedSet $primary
    } catch {
      Add-CleanupFailure $failures 'primary ownership' $_
    }

    if ($owned.ContainsKey($rootPid)) {
      try { [void](Discover-OwnedDescendants $owned) } catch { Add-CleanupFailure $failures 'initial descendant discovery' $_ }
    }

    try {
      if (-not $primary.HasExited) {
        [void]$primary.CloseMainWindow()
        [void]$primary.WaitForExit(2000)
      }
      if (-not $primary.HasExited) {
        $primary.Kill()
        if (-not $primary.WaitForExit(2000)) { throw 'primary handle did not exit after kill' }
      }
    } catch {
      Add-CleanupFailure $failures 'primary shutdown' $_
    }
    if ($owned.ContainsKey($rootPid)) {
      try { Update-ExitCutoff $owned[$rootPid] } catch { Add-CleanupFailure $failures 'primary exit cutoff' $_ }
    }

    $stable = 0
    $cleanupDeadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
      $added = 0
      if ($owned.ContainsKey($rootPid)) {
        try { $added = [int](Discover-OwnedDescendants $owned) } catch { Add-CleanupFailure $failures 'descendant rescan' $_ }
      }
      foreach ($record in @($owned.Values | Where-Object { $_.Pid -ne $rootPid })) {
        try {
          Update-ExitCutoff $record
          if (-not $record.Handle.HasExited) {
            if ((Get-ProcessStartUtc $record.Handle) -ne $record.StartUtc) { throw 'descendant identity changed before kill' }
            [void]$record.Handle.SafeHandle
            $record.Handle.Kill()
            $remainingMs = [int][Math]::Floor(($cleanupDeadline - [DateTime]::UtcNow).TotalMilliseconds)
            if ($remainingMs -le 0) { throw 'cleanup deadline expired before descendant wait' }
            if (-not $record.Handle.WaitForExit([Math]::Min(2000, $remainingMs))) {
              throw 'descendant handle did not exit before its bounded wait'
            }
            Update-ExitCutoff $record
          }
        } catch {
          Add-CleanupFailure $failures "descendant cleanup $($record.Pid)" $_
        }
      }
      $alive = @($owned.Values | Where-Object { -not $_.Handle.HasExited })
      $stable = if ($added -eq 0 -and $alive.Count -eq 0) { $stable + 1 } else { 0 }
      if ($stable -lt 2) { Start-Sleep -Milliseconds 25 }
    } while ($stable -lt 2 -and [DateTime]::UtcNow -lt $cleanupDeadline)
    if ($stable -lt 2) {
      [void]$failures.Add("cleanup deadline exhausted with $($alive.Count) live retained handles")
    }

    try {
      $exactRemaining = @(Get-ExactExecutableRows $exactExe).Count
      if ($exactRemaining -ne 0) { throw "$exactRemaining exact executable process(es) remained" }
    } catch {
      Add-CleanupFailure $failures 'exact executable verification' $_
    }
    try {
      $portRemaining = @(Get-PortListeners).Count
      if ($portRemaining -ne 0) { throw "$portRemaining debug port listener(s) remained" }
    } catch {
      Add-CleanupFailure $failures 'debug port verification' $_
    }
  } finally {
    foreach ($record in @($owned.Values)) {
      try { $record.Handle.Dispose() } catch { Add-CleanupFailure $failures "handle disposal $($record.Pid)" $_ }
    }
    if (-not $owned.ContainsKey($rootPid)) {
      try { $primary.Dispose() } catch { Add-CleanupFailure $failures 'primary handle disposal' $_ }
    }
  }
  if ($failures.Count) { throw "owned cleanup failed: $($failures -join ' | ')" }
}

function Start-OwnedPrimary([string]$exactExe) {
  if ((Get-ExactExecutableRows $exactExe).Count -ne 0) { throw 'exact measurement executable already running' }
  if ((Get-PortListeners).Count -ne 0) { throw 'measurement debug port already in use' }
  $primary = $null
  try {
    try {
      [Environment]::SetEnvironmentVariable($browserArgsName, "--remote-debugging-port=$port", 'Process')
      $primary = Start-Process -FilePath $exactExe -PassThru
      if ($null -eq $primary) { throw 'measurement primary did not start' }
      [void]$primary.SafeHandle
    } finally {
      Restore-BrowserArguments
    }
    if ($primary.HasExited) { throw 'measurement primary exited immediately' }
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
      if ($primary.HasExited) { throw 'measurement primary exited before CDP readiness' }
      $owned = New-OwnedSet $primary
      try {
        [void](Discover-OwnedDescendants $owned)
        $listeners = @(Get-PortListeners)
        if ($listeners.Count -gt 0) {
          if (@($listeners | Where-Object { -not $owned.ContainsKey([int]$_.OwningProcess) }).Count) {
            throw 'debug port listener is outside the owned primary process tree'
          }
          return $primary
        }
      } finally {
        Dispose-OwnedChildren $owned $primary.Id
      }
      Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'owned WebView2 debug port did not become ready within 10 seconds'
  } catch {
    $originalFailure = $_
    $cleanupFailures = [Collections.Generic.List[string]]::new()
    if ($null -ne $primary) {
      try { Stop-OwnedTree $primary $exactExe } catch { Add-CleanupFailure $cleanupFailures 'primary tree cleanup' $_ }
    }
    try { Restore-BrowserArguments } catch { Add-CleanupFailure $cleanupFailures 'browser argument restore' $_ }
    try {
      $exactRemaining = @(Get-ExactExecutableRows $exactExe).Count
      $portRemaining = @(Get-PortListeners).Count
      if ($exactRemaining -ne 0 -or $portRemaining -ne 0) {
        throw "post-launch residue: exact=$exactRemaining port=$portRemaining"
      }
    } catch {
      Add-CleanupFailure $cleanupFailures 'post-launch residue verification' $_
    }
    if ($cleanupFailures.Count) {
      throw "primary start failed: $($originalFailure.Exception.Message) | cleanup failed: $($cleanupFailures -join ' | ')"
    }
    throw $originalFailure
  }
}

$collectorSource = @'
import readline from 'node:readline'

const port = Number(process.argv[2])
if (port !== 9227) throw new Error('unexpected CDP port')
const endpoint = `http://127.0.0.1:${port}/json/list`
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
const exactKeys = (value, keys) => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('non-object schema')
  if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify([...keys].sort())) throw new Error('schema keys')
}
const finite = value => typeof value === 'number' && Number.isFinite(value) && value >= 0

async function findPage() {
  const deadline = Date.now() + 10000
  while (Date.now() < deadline) {
    let response
    try {
      response = await fetch(endpoint, { signal: AbortSignal.timeout(500) })
    } catch {
      await sleep(25)
      continue
    }
    if (!response.ok) throw new Error('CDP endpoint status')
    let targets
    try { targets = await response.json() } catch { throw new Error('CDP endpoint JSON') }
    if (!Array.isArray(targets)) throw new Error('CDP target inventory')
    const pages = targets.filter(target => target?.type === 'page')
    if (pages.length > 1) throw new Error('extra CDP page target')
    if (pages.length === 1) {
      const socketUrl = new URL(pages[0].webSocketDebuggerUrl)
      if (socketUrl.protocol !== 'ws:' || !['127.0.0.1', 'localhost'].includes(socketUrl.hostname) || Number(socketUrl.port) !== port) {
        throw new Error('non-loopback CDP target')
      }
      return socketUrl.href
    }
    await sleep(25)
  }
  throw new Error('CDP page target timeout')
}

function validateBucket(bucket) {
  exactKeys(bucket, ['complete', 'count', 'p95'])
  if (typeof bucket.complete !== 'boolean' || !Number.isInteger(bucket.count) || bucket.count < 0 || bucket.count > 100) {
    throw new Error('bucket schema')
  }
  if (bucket.complete !== (bucket.count === 100)) throw new Error('bucket completion')
  if (bucket.complete ? !finite(bucket.p95) : bucket.p95 !== null) throw new Error('bucket p95')
}

function validateReadout(value) {
  exactKeys(value, [
    'coldReadyMs', 'startupComplete', 'queryEmpty', 'queryInputFocused', 'queryMatchesHarnessValue',
    'warmupObserved', 'empty', 'preserved', 'shownObserved', 'paintObserved',
    'focusFailureObserved', 'bucketObserved',
  ])
  if (value.coldReadyMs !== null && !finite(value.coldReadyMs)) throw new Error('cold schema')
  if (typeof value.startupComplete !== 'boolean' || typeof value.queryEmpty !== 'boolean' ||
      typeof value.queryInputFocused !== 'boolean' || typeof value.queryMatchesHarnessValue !== 'boolean') throw new Error('boolean schema')
  if (!Number.isInteger(value.warmupObserved) || value.warmupObserved < 0 || value.warmupObserved > 5) throw new Error('warmup schema')
  for (const key of ['shownObserved', 'paintObserved', 'focusFailureObserved', 'bucketObserved']) {
    if (!Number.isInteger(value[key]) || value[key] < 0 || value[key] > 2147483647) throw new Error('stage counter schema')
  }
  validateBucket(value.empty)
  validateBucket(value.preserved)
  return value
}

function validateRequest(request) {
  exactKeys(request, request.op === 'query' ? ['op', 'expected'] : request.op === 'count' ? ['op', 'bucket', 'expected'] : ['op'])
  if (!['cold', 'startup', 'query', 'focus', 'harness', 'count', 'final'].includes(request.op)) throw new Error('request op')
  if (request.op === 'query' && typeof request.expected !== 'boolean') throw new Error('query request')
  if (request.op === 'count' && (!['warmup', 'empty', 'preserved'].includes(request.bucket) ||
      !Number.isInteger(request.expected) || request.expected < 0 || request.expected > 100)) throw new Error('count request')
}

const socket = new WebSocket(await findPage())
await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error('CDP socket timeout')), 5000)
  socket.addEventListener('open', () => { clearTimeout(timer); resolve() }, { once: true })
  socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('CDP socket error')) }, { once: true })
})
let nextId = 1
const pending = new Map()
socket.addEventListener('message', event => {
  let message
  try { message = JSON.parse(String(event.data)) } catch { throw new Error('CDP message JSON') }
  if (!pending.has(message.id)) return
  const { resolve, reject, timer } = pending.get(message.id)
  pending.delete(message.id)
  clearTimeout(timer)
  if (message.error || message.result?.exceptionDetails) reject(new Error('CDP evaluate error'))
  else resolve(message.result?.result?.value)
})

function readOnce() {
  const id = nextId++
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('CDP evaluate timeout')) }, 1000)
    pending.set(id, { resolve, reject, timer })
    socket.send(JSON.stringify({
      id,
      method: 'Runtime.evaluate',
      params: {
        expression: `(() => {
          const seam = globalThis.__UIPILOT_TASK7_PERF__
          return seam === undefined ? { seamReady: false, readout: null } : { seamReady: true, readout: seam.read() }
        })()`,
        returnByValue: true,
      },
    }))
  })
}

function matches(request, value) {
  if (request.op === 'cold') return finite(value.coldReadyMs)
  if (request.op === 'startup') return value.startupComplete
  if (request.op === 'query') return value.queryEmpty === request.expected
  if (request.op === 'focus') return value.queryInputFocused
  if (request.op === 'harness') return value.queryMatchesHarnessValue
  if (request.op === 'count') {
    const count = request.bucket === 'warmup' ? value.warmupObserved : value[request.bucket].count
    if (count > request.expected) throw new Error('counter exceeded expected value')
    return count === request.expected
  }
  return value.warmupObserved === 5 && value.empty.complete && value.preserved.complete
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })
for await (const line of input) {
  let request
  try { request = JSON.parse(line) } catch { throw new Error('request JSON') }
  validateRequest(request)
  const deadline = Date.now() + 10000
  let value
  let matched = false
  do {
    const envelope = await readOnce()
    exactKeys(envelope, ['seamReady', 'readout'])
    if (typeof envelope.seamReady !== 'boolean') throw new Error('seam readiness schema')
    if (!envelope.seamReady) {
      if (envelope.readout !== null) throw new Error('missing seam readout schema')
      await sleep(25)
      continue
    }
    value = validateReadout(envelope.readout)
    if (matches(request, value)) {
      matched = true
      break
    }
    await sleep(25)
  } while (Date.now() < deadline)
  if (value === undefined) throw new Error('measurement readout unavailable')
  process.stdout.write(`${JSON.stringify({ ok: matched, readout: value })}\n`)
}
socket.close()
'@

function Start-Collector {
  [IO.File]::WriteAllText($collectorPath, $collectorSource, [Text.UTF8Encoding]::new($false))
  $nodePath = (Get-Command node -ErrorAction Stop).Source
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $nodePath
  $start.Arguments = "`"$collectorPath`" $port"
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardInput = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $process = [Diagnostics.Process]::Start($start)
  if ($null -eq $process) { throw 'CDP collector did not start' }
  $collector = [pscustomobject]@{
    Process = $process
    StderrTask = $process.StandardError.ReadToEndAsync()
    StderrConsumed = $false
    SanitizedStderr = $null
    LastReadout = $null
    StageBaseline = $null
    Stopped = $false
    Disposed = $false
  }
  try {
    [void]$process.SafeHandle
    if ($process.HasExited) {
      [void]$process.WaitForExit(2000)
      $stderr = Get-SanitizedCollectorStderr $collector
      throw "CDP collector exited during startup PrimaryFailure=$stderr"
    }
    $collector
  } catch {
    $original = $_
    $cleanupFailure = $null
    try { Stop-OwnedCollector $collector } catch { $cleanupFailure = $_ }
    if ($null -ne $cleanupFailure) {
      throw "$($original.Exception.Message) | cleanup failed: $($cleanupFailure.Exception.Message)"
    }
    throw $original
  }
}

function Assert-CollectorReadout($value) {
  $expectedKeys = @(
    'bucketObserved', 'coldReadyMs', 'empty', 'focusFailureObserved', 'paintObserved', 'preserved',
    'queryEmpty', 'queryInputFocused', 'queryMatchesHarnessValue', 'shownObserved', 'startupComplete',
    'warmupObserved'
  )
  $actualKeys = @($value.PSObject.Properties.Name | Sort-Object)
  if (@(Compare-Object $actualKeys $expectedKeys -CaseSensitive).Count) {
    throw 'collector readout keys drifted'
  }
  foreach ($key in @('startupComplete', 'queryEmpty', 'queryInputFocused', 'queryMatchesHarnessValue')) {
    if ($value.$key -isnot [bool]) { throw 'collector readout boolean schema drifted' }
  }
  foreach ($key in @('shownObserved', 'paintObserved', 'focusFailureObserved', 'bucketObserved')) {
    if ($value.$key -isnot [int] -or $value.$key -lt 0) { throw 'collector stage counter schema drifted' }
  }
  $value
}

function Get-RelativeStageCounterText($collector) {
  $value = $collector.LastReadout
  $baseline = $collector.StageBaseline
  $shown = if ($null -eq $value) { 0 } else { [int]$value.shownObserved }
  $paint = if ($null -eq $value) { 0 } else { [int]$value.paintObserved }
  $focusFailure = if ($null -eq $value) { 0 } else { [int]$value.focusFailureObserved }
  $bucket = if ($null -eq $value) { 0 } else { [int]$value.bucketObserved }
  if ($null -ne $baseline) {
    $shown -= [int]$baseline.shownObserved
    $paint -= [int]$baseline.paintObserved
    $focusFailure -= [int]$baseline.focusFailureObserved
    $bucket -= [int]$baseline.bucketObserved
  }
  if ($shown -lt 0 -or $paint -lt 0 -or $focusFailure -lt 0 -or $bucket -lt 0) {
    throw 'relative stage counter moved backwards'
  }
  "shown=$shown paint=$paint focusFailure=$focusFailure bucket=$bucket"
}

$stageFixture = [pscustomobject]@{
  coldReadyMs = $null; startupComplete = $true; queryEmpty = $true; queryInputFocused = $false
  queryMatchesHarnessValue = $false; warmupObserved = 0
  empty = [pscustomobject]@{ complete = $false; count = 0; p95 = $null }
  preserved = [pscustomobject]@{ complete = $false; count = 0; p95 = $null }
  shownObserved = 0; paintObserved = 0; focusFailureObserved = 0; bucketObserved = 0
}
[void](Assert-CollectorReadout $stageFixture)
$stageFixtureRejected = 0
foreach ($invalidStageFixture in @(
  ([pscustomobject]@{ coldReadyMs = $null; startupComplete = $true; queryEmpty = $true; queryInputFocused = $false; queryMatchesHarnessValue = $false; warmupObserved = 0; empty = $stageFixture.empty; preserved = $stageFixture.preserved; shownObserved = [decimal]1.25; paintObserved = 0; focusFailureObserved = 0; bucketObserved = 0 }),
  ([pscustomobject]@{ coldReadyMs = $null; startupComplete = $true; queryEmpty = $true; queryInputFocused = $false; queryMatchesHarnessValue = $false; warmupObserved = 0; empty = $stageFixture.empty; preserved = $stageFixture.preserved; shownObserved = [double]1; paintObserved = 0; focusFailureObserved = 0; bucketObserved = 0 }),
  ($stageFixture | Select-Object *, @{ Name = 'extra'; Expression = { $true } }),
  ($stageFixture | Select-Object * -ExcludeProperty bucketObserved)
)) {
  try { [void](Assert-CollectorReadout $invalidStageFixture) } catch { $stageFixtureRejected++ }
}
if ($stageFixtureRejected -ne 4) { throw 'collector stage counter negative fixtures drifted' }

function Get-CollectorExitFailure($collector, [string]$context) {
  $process = $collector.Process
  if (-not $process.HasExited -and -not $process.WaitForExit(2000)) {
    return "collector-read-fault context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $exitCode = $process.ExitCode
  try { $stderr = Get-SanitizedCollectorStderr $collector } catch { $stderr = 'collector-stderr-unavailable' }
  "collector-exited-without-response context=$context exitCode=$exitCode $(Get-RelativeStageCounterText $collector) PrimaryFailure=$stderr"
}

$collectorContextPattern = '^(?:cold:(?:[1-9]|[12][0-9]|30)|startup|query-empty|warmup:[1-5]|empty:(?:[1-9]|[1-9][0-9]|100)|query-seed-startup|query-seed-focus|query-seed-harness|focus|harness|preserved:(?:[1-9]|[1-9][0-9]|100)|final)$'
$approvedCollectorContexts = [Collections.Generic.List[string]]::new()
foreach ($index in 1..30) { $approvedCollectorContexts.Add("cold:$index") }
$approvedCollectorContexts.Add('startup')
$approvedCollectorContexts.Add('query-empty')
foreach ($index in 1..5) { $approvedCollectorContexts.Add("warmup:$index") }
foreach ($index in 1..100) { $approvedCollectorContexts.Add("empty:$index") }
$approvedCollectorContexts.Add('query-seed-startup')
$approvedCollectorContexts.Add('query-seed-focus')
$approvedCollectorContexts.Add('query-seed-harness')
$approvedCollectorContexts.Add('focus')
$approvedCollectorContexts.Add('harness')
foreach ($index in 1..100) { $approvedCollectorContexts.Add("preserved:$index") }
$approvedCollectorContexts.Add('final')
if ($approvedCollectorContexts.Count -ne 243) { throw 'approved collector context fixture count drifted' }
foreach ($approvedContext in $approvedCollectorContexts) {
  if ($approvedContext -cnotmatch $collectorContextPattern) {
    throw "approved collector context was rejected: $approvedContext"
  }
}
$rejectedCollectorContexts = @(
  'Query-seed-startup', 'query-Seed-focus', 'query-seed-HARNESS',
  ' query-seed-startup', 'query-seed-focus ',
  'prefix-query-seed-harness', 'query-seed-startup-suffix', 'query-seed-unknown'
)
foreach ($rejectedContext in $rejectedCollectorContexts) {
  if ($rejectedContext -cmatch $collectorContextPattern) {
    throw "unapproved collector context was accepted: $rejectedContext"
  }
}

function Invoke-Collector($collector, [hashtable]$request, [string]$context) {
  if ($context -cnotmatch $collectorContextPattern) {
    throw 'collector context is outside the approved fixed inventory'
  }
  $process = $collector.Process
  if ($process.HasExited) { throw (Get-CollectorExitFailure $collector $context) }
  try {
    $process.StandardInput.WriteLine(($request | ConvertTo-Json -Compress))
    $process.StandardInput.Flush()
  } catch {
    if ($process.HasExited) { throw (Get-CollectorExitFailure $collector $context) }
    throw "collector-write-fault context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $read = $process.StandardOutput.ReadLineAsync()
  try { $completed = $read.Wait(12000) } catch {
    throw "collector-read-fault context=$context $(Get-RelativeStageCounterText $collector)"
  }
  if (-not $completed) {
    if ($process.HasExited) { throw (Get-CollectorExitFailure $collector $context) }
    [void]$process.SafeHandle
    if ($process.HasExited) { throw (Get-CollectorExitFailure $collector $context) }
    throw "collector-response-deadline context=$context elapsedBoundMs=12000 alive=true $(Get-RelativeStageCounterText $collector)"
  }
  if ($read.IsFaulted -or $read.IsCanceled) {
    throw "collector-read-fault context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $line = $read.Result
  if ($null -eq $line -or $process.HasExited) { throw (Get-CollectorExitFailure $collector $context) }
  try { $envelope = $line | ConvertFrom-Json } catch {
    throw "collector-invalid-json context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $envelopeKeys = @($envelope.PSObject.Properties.Name | Sort-Object)
  if (@(Compare-Object $envelopeKeys @('ok', 'readout') -CaseSensitive).Count -or $envelope.ok -isnot [bool]) {
    throw "collector-invalid-envelope context=$context $(Get-RelativeStageCounterText $collector)"
  }
  try { $readout = Assert-CollectorReadout $envelope.readout } catch {
    throw "collector-invalid-readout context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $collector.LastReadout = $readout
  if (-not $envelope.ok) {
    throw "collector-request-timeout context=$context $(Get-RelativeStageCounterText $collector)"
  }
  $readout
}

function Invoke-SecondInstance(
  [string]$exactExe,
  [Diagnostics.Process]$primary,
  $collector,
  [string]$bucket,
  [int]$expected
) {
  $before = @(Get-ExactExecutableRows $exactExe)
  if ($before.Count -ne 1 -or [int]$before[0].ProcessId -ne $primary.Id) { throw 'primary ownership changed before second instance' }
  $secondary = $null
  try {
    $secondary = Start-Process -FilePath $exactExe -PassThru
    if ($null -eq $secondary) { throw 'second instance did not start' }
    [void]$secondary.SafeHandle
    if (-not $secondary.WaitForExit(10000)) { throw 'second instance did not exit within 10 seconds' }
    if ($secondary.ExitCode -ne 0) { throw 'second instance exited nonzero' }
  } finally {
    if ($null -ne $secondary) {
      try {
        if (-not $secondary.HasExited) {
          $secondary.Kill()
          if (-not $secondary.WaitForExit(2000)) { throw 'second-instance handle did not exit after kill' }
        }
      } finally {
        $secondary.Dispose()
      }
    }
  }
  $after = @(Get-ExactExecutableRows $exactExe)
  if ($after.Count -ne 1 -or [int]$after[0].ProcessId -ne $primary.Id) { throw 'second instance left an extra exact-path process' }
  [void](Invoke-Collector $collector @{ op = 'count'; bucket = $bucket; expected = $expected } "${bucket}:$expected")
}

function Assert-PrimaryOwnsForeground([Diagnostics.Process]$primary) {
  $owned = New-OwnedSet $primary
  try {
    [void](Discover-OwnedDescendants $owned)
    $hwnd = [Task7ForegroundWindow]::GetForegroundWindow()
    if ($hwnd -eq [IntPtr]::Zero) { throw 'no foreground window before fixed harness input' }
    [uint32]$foregroundPid = 0
    [void][Task7ForegroundWindow]::GetWindowThreadProcessId($hwnd, [ref]$foregroundPid)
    if ($foregroundPid -eq 0 -or -not $owned.ContainsKey([int]$foregroundPid)) {
      throw 'foreground HWND is outside the authenticated primary process tree'
    }
    $record = $owned[[int]$foregroundPid]
    if ($record.Handle.HasExited -or (Get-ProcessStartUtc $record.Handle) -ne $record.StartUtc) {
      throw 'foreground process identity changed before fixed harness input'
    }
    [void]$record.Handle.SafeHandle
  } finally {
    Dispose-OwnedChildren $owned $primary.Id
  }
}

$collectorFixtureSource = @'
setTimeout(() => {
  process.stderr.write('TASK7_COLLECTOR_SENTINEL\n')
  process.exit(17)
}, 100)
'@
$collectorFixtureOriginalSource = $collectorSource
$collectorFixture = $null
$collectorFixtureFailure = $null
$collectorFixtureCleanupFailure = $null
try {
  $collectorSource = $collectorFixtureSource
  $collectorFixture = Start-Collector
  try { [void](Invoke-Collector $collectorFixture @{ op = 'cold' } 'cold:1') } catch {
    $collectorFixtureFailure = $_
  }
} finally {
  if ($null -ne $collectorFixture) {
    try { Stop-OwnedCollector $collectorFixture } catch { $collectorFixtureCleanupFailure = $_ }
  }
  $collectorSource = $collectorFixtureOriginalSource
  if (Test-Path -LiteralPath $collectorPath) { Remove-Item -LiteralPath $collectorPath -Force }
}
if ($null -ne $collectorFixtureCleanupFailure) {
  throw "collector sentinel fixture cleanup failed: $($collectorFixtureCleanupFailure.Exception.Message)"
}
if ($null -eq $collectorFixtureFailure -or
    -not $collectorFixtureFailure.Exception.Message.Contains('collector-exited-without-response') -or
    -not $collectorFixtureFailure.Exception.Message.Contains('context=cold:1') -or
    -not $collectorFixtureFailure.Exception.Message.Contains('exitCode=17') -or
    -not $collectorFixtureFailure.Exception.Message.Contains('PrimaryFailure=TASK7_COLLECTOR_SENTINEL') -or
    $collectorFixtureFailure.Exception.Message.Contains('collector-response-deadline') -or
    -not $collectorFixture.Stopped -or -not $collectorFixture.Disposed -or
    -not $collectorFixture.StderrConsumed -or (Test-Path -LiteralPath $collectorPath)) {
  throw 'collector stderr/EOF sentinel fixture failed or left resources'
}
[pscustomobject]@{
  CollectorExitFixture = 'PASS'
  CollectorExitContext = 'cold:1'
  CollectorExitCode = 17
  CollectorSentinelPreserved = $true
  CollectorFixtureResourcesRemoved = $true
} | Format-List

$cold = [Collections.Generic.List[double]]::new()
$workflowOriginal = $null
try {
  $preflightSentinel = 'Task7 cleanup preflight sentinel'
  $preflightPrimary = $null
  $preflightOriginal = $null
  $preflightCleanupFailures = [Collections.Generic.List[string]]::new()
  try {
    $preflightPrimary = Start-OwnedPrimary $measurementExe.FullName
    throw $preflightSentinel
  } catch {
    $preflightOriginal = $_
  }
  if ($null -ne $preflightPrimary) {
    try { Stop-OwnedTree $preflightPrimary $measurementExe.FullName } catch {
      Add-CleanupFailure $preflightCleanupFailures 'preflight primary tree cleanup' $_
    } finally {
      $preflightPrimary = $null
    }
  }
  try { Restore-BrowserArguments } catch { Add-CleanupFailure $preflightCleanupFailures 'preflight browser argument restore' $_ }
  try {
    $preflightExact = @(Get-ExactExecutableRows $measurementExe.FullName).Count
    $preflightPort = @(Get-PortListeners).Count
    if ($preflightExact -ne 0 -or $preflightPort -ne 0) {
      throw "preflight residue: exact=$preflightExact port=$preflightPort"
    }
  } catch {
    Add-CleanupFailure $preflightCleanupFailures 'preflight residue verification' $_
  }
  if ($preflightCleanupFailures.Count) {
    Write-Output "Cleanup=FAIL category=preflight count=$($preflightCleanupFailures.Count)"
    throw "preflight primary failure: $($preflightOriginal.Exception.Message) | cleanup failed: $($preflightCleanupFailures -join ' | ')"
  }
  if ($preflightOriginal.Exception.Message -cne $preflightSentinel) {
    throw "cleanup preflight did not preserve sentinel: $($preflightOriginal.Exception.Message)"
  }
  [pscustomobject]@{
    CleanupPreflight = 'PASS'
    PrimaryFailure = $preflightOriginal.Exception.Message
    OwnedProcessesRemaining = 0
    ExactExecutableRemaining = $preflightExact
    PortRemaining = $preflightPort
  } | Format-List

  $querySeedPrimary = $null
  $querySeedCollector = $null
  $querySeedSecondary = $null
  $querySeedOriginal = $null
  $querySeedCleanupFailures = [Collections.Generic.List[string]]::new()
  try {
    $querySeedPrimary = Start-OwnedPrimary $measurementExe.FullName
    $querySeedCollector = Start-Collector
    [void](Invoke-Collector $querySeedCollector @{ op = 'startup' } 'query-seed-startup')

    $beforeShow = @(Get-ExactExecutableRows $measurementExe.FullName)
    if ($beforeShow.Count -ne 1 -or [int]$beforeShow[0].ProcessId -ne $querySeedPrimary.Id) {
      throw 'query-seed primary ownership changed before show request'
    }
    try {
      $querySeedSecondary = Start-Process -FilePath $measurementExe.FullName -PassThru
      if ($null -eq $querySeedSecondary) { throw 'query-seed show request did not start' }
      [void]$querySeedSecondary.SafeHandle
      if (-not $querySeedSecondary.WaitForExit(10000)) { throw 'query-seed show request did not exit within 10 seconds' }
      if ($querySeedSecondary.ExitCode -ne 0) { throw 'query-seed show request exited nonzero' }
    } finally {
      if ($null -ne $querySeedSecondary) {
        try {
          if (-not $querySeedSecondary.HasExited) {
            $querySeedSecondary.Kill()
            if (-not $querySeedSecondary.WaitForExit(2000)) {
              throw 'query-seed show request handle did not exit after kill'
            }
          }
        } finally {
          $querySeedSecondary.Dispose()
          $querySeedSecondary = $null
        }
      }
    }
    $afterShow = @(Get-ExactExecutableRows $measurementExe.FullName)
    if ($afterShow.Count -ne 1 -or [int]$afterShow[0].ProcessId -ne $querySeedPrimary.Id) {
      throw 'query-seed show request left an extra exact-path process'
    }

    [void](Invoke-Collector $querySeedCollector @{ op = 'focus' } 'query-seed-focus')
    Assert-PrimaryOwnsForeground $querySeedPrimary
    Invoke-HarnessQueryInput
    Assert-PrimaryOwnsForeground $querySeedPrimary
    [void](Invoke-Collector $querySeedCollector @{ op = 'harness' } 'query-seed-harness')
  } catch {
    $querySeedOriginal = $_
  }
  try { Stop-OwnedCollector $querySeedCollector } catch {
    Add-CleanupFailure $querySeedCleanupFailures 'query-seed collector cleanup' $_
  } finally {
    $querySeedCollector = $null
  }
  try { Stop-OwnedTree $querySeedPrimary $measurementExe.FullName } catch {
    Add-CleanupFailure $querySeedCleanupFailures 'query-seed primary tree cleanup' $_
  } finally {
    $querySeedPrimary = $null
  }
  try {
    Restore-BrowserArguments
    if ([Environment]::GetEnvironmentVariable($browserArgsName, 'Process') -ne $originalBrowserArgs) {
      throw 'query-seed browser argument restore mismatch'
    }
  } catch {
    Add-CleanupFailure $querySeedCleanupFailures 'query-seed browser argument restore' $_
  }
  try {
    if (Test-Path -LiteralPath $collectorPath) { Remove-Item -LiteralPath $collectorPath -Force }
    if (Test-Path -LiteralPath $collectorPath) { throw 'query-seed collector file remained' }
  } catch {
    Add-CleanupFailure $querySeedCleanupFailures 'query-seed collector file cleanup' $_
  }
  try {
    $querySeedExact = @(Get-ExactExecutableRows $measurementExe.FullName).Count
    $querySeedPort = @(Get-PortListeners).Count
    if ($querySeedExact -ne 0 -or $querySeedPort -ne 0) {
      throw "query-seed residue: exact=$querySeedExact port=$querySeedPort"
    }
  } catch {
    Add-CleanupFailure $querySeedCleanupFailures 'query-seed residue verification' $_
  }
  if ($null -ne $querySeedOriginal) {
    if ($querySeedCleanupFailures.Count) {
      Write-Output "Cleanup=FAIL category=query-seed count=$($querySeedCleanupFailures.Count)"
      throw "query-seed preflight failed: $($querySeedOriginal.Exception.Message) | cleanup failed: $($querySeedCleanupFailures -join ' | ')"
    }
    throw $querySeedOriginal
  }
  if ($querySeedCleanupFailures.Count) {
    Write-Output "Cleanup=FAIL category=query-seed count=$($querySeedCleanupFailures.Count)"
    throw "query-seed preflight cleanup failed: $($querySeedCleanupFailures -join ' | ')"
  }
  [pscustomobject]@{
    QuerySeedPreflight = 'PASS'
    TimedSamplesRetained = 0
    ExactExecutableRemaining = $querySeedExact
    PortRemaining = $querySeedPort
    BrowserArgumentsRestored = $true
    CollectorFileRemaining = [int](Test-Path -LiteralPath $collectorPath)
  } | Format-List

  for ($sample = 1; $sample -le 30; $sample++) {
    $sampleOriginal = $null
    try {
      $activePrimary = Start-OwnedPrimary $measurementExe.FullName
      $activeCollector = Start-Collector
      $readout = Invoke-Collector $activeCollector @{ op = 'cold' } "cold:$sample"
      if ($null -eq $readout.coldReadyMs) { throw 'cold sample missing' }
      $cold.Add([double]$readout.coldReadyMs)
    } catch {
      $sampleOriginal = $_
    }
    $sampleCleanupFailures = [Collections.Generic.List[string]]::new()
    try { Stop-OwnedCollector $activeCollector } catch {
      Add-CleanupFailure $sampleCleanupFailures 'cold collector cleanup' $_
    } finally {
      $activeCollector = $null
    }
    try { Stop-OwnedTree $activePrimary $measurementExe.FullName } catch {
      Add-CleanupFailure $sampleCleanupFailures 'cold primary tree cleanup' $_
    } finally {
      $activePrimary = $null
    }
    try { Restore-BrowserArguments } catch { Add-CleanupFailure $sampleCleanupFailures 'cold browser argument restore' $_ }
    try {
      $sampleExact = @(Get-ExactExecutableRows $measurementExe.FullName).Count
      $samplePort = @(Get-PortListeners).Count
      if ($sampleExact -ne 0 -or $samplePort -ne 0) { throw "cold residue: exact=$sampleExact port=$samplePort" }
    } catch {
      Add-CleanupFailure $sampleCleanupFailures 'cold residue verification' $_
    }
    if ($null -ne $sampleOriginal) {
      if ($sampleCleanupFailures.Count) {
        Write-Output "Cleanup=FAIL category=cold count=$($sampleCleanupFailures.Count)"
        throw "cold sample failed: $($sampleOriginal.Exception.Message) | cleanup failed: $($sampleCleanupFailures -join ' | ')"
      }
      throw $sampleOriginal
    }
    if ($sampleCleanupFailures.Count) {
      Write-Output "Cleanup=FAIL category=cold count=$($sampleCleanupFailures.Count)"
      throw "cold sample cleanup failed: $($sampleCleanupFailures -join ' | ')"
    }
  }
  $sortedCold = @($cold | Sort-Object)
  if ($sortedCold.Count -ne 30) { throw 'cold sample count drifted' }
  $coldP95 = [double]$sortedCold[[Math]::Ceiling(0.95 * 30) - 1]
  $cold.Clear()

  $activePrimary = Start-OwnedPrimary $measurementExe.FullName
  $activeCollector = Start-Collector
  $warmStageBaseline = Invoke-Collector $activeCollector @{ op = 'startup' } 'startup'
  $activeCollector.StageBaseline = $warmStageBaseline
  [void](Invoke-Collector $activeCollector @{ op = 'query'; expected = $true } 'query-empty')
  for ($i = 1; $i -le 5; $i++) {
    Invoke-SecondInstance $measurementExe.FullName $activePrimary $activeCollector 'warmup' $i
  }
  for ($i = 1; $i -le 100; $i++) {
    Invoke-SecondInstance $measurementExe.FullName $activePrimary $activeCollector 'empty' $i
  }
  [void](Invoke-Collector $activeCollector @{ op = 'focus' } 'focus')
  Assert-PrimaryOwnsForeground $activePrimary
  Invoke-HarnessQueryInput
  Assert-PrimaryOwnsForeground $activePrimary
  [void](Invoke-Collector $activeCollector @{ op = 'harness' } 'harness')
  for ($i = 1; $i -le 100; $i++) {
    Invoke-SecondInstance $measurementExe.FullName $activePrimary $activeCollector 'preserved' $i
  }
  $warm = Invoke-Collector $activeCollector @{ op = 'final' } 'final'
  $stageShown = [int]$warm.shownObserved - [int]$warmStageBaseline.shownObserved
  $stagePaint = [int]$warm.paintObserved - [int]$warmStageBaseline.paintObserved
  $stageFocusFailure = [int]$warm.focusFailureObserved - [int]$warmStageBaseline.focusFailureObserved
  $stageBucket = [int]$warm.bucketObserved - [int]$warmStageBaseline.bucketObserved
  if ($stageShown -ne 205 -or $stagePaint -ne 205 -or $stageFocusFailure -ne 0 -or $stageBucket -ne 205) {
    throw "warm stage totals drifted shown=$stageShown paint=$stagePaint focusFailure=$stageFocusFailure bucket=$stageBucket"
  }
  if ($coldP95 -gt 750 -or [double]$warm.empty.p95 -gt 100 -or [double]$warm.preserved.p95 -gt 100) {
    throw 'Task 7 responsiveness threshold exceeded'
  }
  [pscustomobject]@{
    ColdCount = 30
    ColdP95 = $coldP95
    WarmupCount = [int]$warm.warmupObserved
    EmptyCount = [int]$warm.empty.count
    EmptyP95 = [double]$warm.empty.p95
    PreservedCount = [int]$warm.preserved.count
    PreservedP95 = [double]$warm.preserved.p95
    StageShown = $stageShown
    StagePaint = $stagePaint
    StageFocusFailure = $stageFocusFailure
    StageBucket = $stageBucket
  } | Format-List
} catch {
  $workflowOriginal = $_
}
$workflowCleanupFailures = [Collections.Generic.List[string]]::new()
try { Stop-OwnedCollector $activeCollector } catch {
  Add-CleanupFailure $workflowCleanupFailures 'workflow collector cleanup' $_
} finally {
  $activeCollector = $null
}
try { Stop-OwnedTree $activePrimary $measurementExe.FullName } catch {
  Add-CleanupFailure $workflowCleanupFailures 'workflow primary tree cleanup' $_
} finally {
  $activePrimary = $null
}
try { Restore-BrowserArguments } catch { Add-CleanupFailure $workflowCleanupFailures 'workflow browser argument restore' $_ }
try {
  if (Test-Path -LiteralPath $collectorPath) { Remove-Item -LiteralPath $collectorPath -Force }
  if (Test-Path -LiteralPath $collectorPath) { throw 'temporary collector remained after removal' }
} catch {
  Add-CleanupFailure $workflowCleanupFailures 'temporary collector cleanup' $_
}
try {
  $workflowExact = @(Get-ExactExecutableRows $measurementExe.FullName).Count
  $workflowPort = @(Get-PortListeners).Count
  if ($workflowExact -ne 0 -or $workflowPort -ne 0) {
    throw "workflow residue: exact=$workflowExact port=$workflowPort"
  }
} catch {
  Add-CleanupFailure $workflowCleanupFailures 'workflow residue verification' $_
}
if ($null -ne $workflowOriginal) {
  if ($workflowCleanupFailures.Count) {
    Write-Output "Cleanup=FAIL category=workflow count=$($workflowCleanupFailures.Count)"
    throw "workflow failed: $($workflowOriginal.Exception.Message) | cleanup failed: $($workflowCleanupFailures -join ' | ')"
  }
  throw $workflowOriginal
}
if ($workflowCleanupFailures.Count) {
  Write-Output "Cleanup=FAIL category=workflow count=$($workflowCleanupFailures.Count)"
  throw "workflow cleanup failed: $($workflowCleanupFailures -join ' | ')"
}
```
<!-- TASK7_PERF_WORKFLOW_END -->

The fenced workflow is longer than the Windows command-line limit. Execute that exact fence through one authenticated disposable runner, never `EncodedCommand`:

```powershell
$ErrorActionPreference = 'Stop'
$planWorktree = 'D:\code\UiPilot_tools\.worktrees\foundation-task-7-design'
$planBranch = 'codex/foundation-task-7-design'
$planRelativePath = 'docs/superpowers/plans/2026-07-19-foundation-task-7-keyboard-first-launcher-ui.md'
$planGoSha = [Environment]::GetEnvironmentVariable('TASK7_PLAN_GO_SHA', 'Process')
if ($planGoSha -cnotmatch '^[0-9a-f]{40}$') { throw 'written Plan Go SHA is missing' }

function Invoke-GitCapture([string]$root, [string]$label, [string[]]$arguments) {
  $output = @(& git -C $root @arguments)
  $exitCode = $LASTEXITCODE
  if ($exitCode -ne 0) { throw "$label failed with exit $exitCode" }
  $output
}

function Read-ApprovedPlan([string]$root, [string]$branch, [string]$approvedSha, [string]$relativePath) {
  $expectedRoot = [IO.Path]::GetFullPath($root)
  $rootItem = Get-Item -LiteralPath $expectedRoot -Force
  if ($rootItem -isnot [IO.DirectoryInfo] -or
      ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      $rootItem.FullName -cne $expectedRoot) {
    throw 'approved plan root is not the exact regular directory'
  }
  $gitRoot = @(Invoke-GitCapture $expectedRoot 'plan repository root' @('rev-parse', '--show-toplevel'))
  if ($gitRoot.Count -ne 1 -or [IO.Path]::GetFullPath($gitRoot[0]) -cne $expectedRoot) {
    throw 'plan repository root drifted'
  }
  $actualBranch = @(Invoke-GitCapture $expectedRoot 'plan branch' @('symbolic-ref', '--short', 'HEAD'))
  if ($actualBranch.Count -ne 1 -or $actualBranch[0] -cne $branch) { throw 'plan branch drifted' }
  $head = @(Invoke-GitCapture $expectedRoot 'plan HEAD' @('rev-parse', 'HEAD'))
  if ($head.Count -ne 1 -or $head[0] -cne $approvedSha) { throw 'plan HEAD does not match written Plan Go' }
  $status = @(Invoke-GitCapture $expectedRoot 'plan status' @('status', '--porcelain=v1', '--untracked-files=all'))
  if ($status.Count) { throw 'approved plan worktree is not clean' }
  $cached = @(Invoke-GitCapture $expectedRoot 'plan index' @('diff', '--cached', '--name-only'))
  if ($cached.Count) { throw 'approved plan index is not clean' }

  $cursor = $rootItem
  $segments = @($relativePath -split '/')
  for ($index = 0; $index -lt $segments.Count; $index++) {
    $matches = @(Get-ChildItem -LiteralPath $cursor.FullName -Force | Where-Object { $_.Name -ceq $segments[$index] })
    if ($matches.Count -ne 1) { throw 'approved plan path casing or component count drifted' }
    $cursor = $matches[0]
    if ($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'approved plan path contains a reparse point' }
    if ($index -lt $segments.Count - 1 -and $cursor -isnot [IO.DirectoryInfo]) {
      throw 'approved plan parent is not a regular directory'
    }
  }
  if ($cursor -isnot [IO.FileInfo]) { throw 'approved plan is not a regular file' }
  $planItem = $cursor
  $tracked = @(Invoke-GitCapture $expectedRoot 'tracked plan path' @('ls-files', '--error-unmatch', '--', $relativePath))
  if ($tracked.Count -ne 1 -or $tracked[0] -cne $relativePath) { throw 'tracked plan path drifted' }
  $approvedBlob = @(Invoke-GitCapture $expectedRoot 'approved plan blob' @('rev-parse', "$approvedSha`:$relativePath"))
  if ($approvedBlob.Count -ne 1 -or $approvedBlob[0] -cnotmatch '^[0-9a-f]{40}$') { throw 'approved plan blob id is invalid' }
  $workingBlob = @(Invoke-GitCapture $expectedRoot 'working plan clean-filter hash' @(
    'hash-object', "--path=$relativePath", '--', $planItem.FullName
  ))
  if ($workingBlob.Count -ne 1 -or $workingBlob[0] -cne $approvedBlob[0]) {
    throw 'working plan does not match the approved clean-filtered blob'
  }
  $planText = [IO.File]::ReadAllText($planItem.FullName, [Text.UTF8Encoding]::new($false))
  $workingBlobAfterRead = @(Invoke-GitCapture $expectedRoot 'post-read plan clean-filter hash' @(
    'hash-object', "--path=$relativePath", '--', $planItem.FullName
  ))
  if ($workingBlobAfterRead.Count -ne 1 -or $workingBlobAfterRead[0] -cne $approvedBlob[0]) {
    throw 'working plan changed while authenticating the approved blob'
  }
  $planText
}

$provenanceFixtureRoot = Join-Path $env:TEMP ("uipilot-task7-plan-provenance-$([guid]::NewGuid().ToString('N'))")
$provenanceFixtureOriginal = $null
$provenanceFixtureCleanupFailures = [Collections.Generic.List[string]]::new()
try {
  $null = New-Item -ItemType Directory -Path $provenanceFixtureRoot
  $null = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture git init' @('init', "--initial-branch=$planBranch"))
  $null = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture user name' @('config', 'user.name', 'Task7 Fixture'))
  $null = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture user email' @('config', 'user.email', 'task7-fixture@example.invalid'))
  $fixturePlanPath = Join-Path $provenanceFixtureRoot ($planRelativePath -replace '/', '\')
  $null = New-Item -ItemType Directory -Path (Split-Path -Parent $fixturePlanPath)
  [IO.File]::WriteAllText($fixturePlanPath, "approved plan`n", [Text.UTF8Encoding]::new($false))
  $null = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture git add' @('add', '--', $planRelativePath))
  $null = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture git commit' @('commit', '-m', 'fixture baseline'))
  $fixtureSha = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture HEAD' @('rev-parse', 'HEAD'))
  if ($fixtureSha.Count -ne 1) { throw 'fixture HEAD count drifted' }
  [IO.File]::AppendAllText($fixturePlanPath, "altered working content`n", [Text.UTF8Encoding]::new($false))
  $fixtureHeadAfterAlter = @(Invoke-GitCapture $provenanceFixtureRoot 'fixture unchanged HEAD' @('rev-parse', 'HEAD'))
  if ($fixtureHeadAfterAlter.Count -ne 1 -or $fixtureHeadAfterAlter[0] -cne $fixtureSha[0]) {
    throw 'fixture HEAD changed after working-content alteration'
  }
  $fixtureRejected = $false
  try {
    [void](Read-ApprovedPlan $provenanceFixtureRoot $planBranch $fixtureSha[0] $planRelativePath)
  } catch {
    if ($_.Exception.Message -cne 'approved plan worktree is not clean') { throw }
    $fixtureRejected = $true
  }
  if (-not $fixtureRejected) { throw 'altered working plan passed approved-plan provenance' }
} catch {
  $provenanceFixtureOriginal = $_
} finally {
  try {
    if (Test-Path -LiteralPath $provenanceFixtureRoot) {
      $fixtureRootItem = Get-Item -LiteralPath $provenanceFixtureRoot -Force
      if ($fixtureRootItem -isnot [IO.DirectoryInfo] -or
          ($fixtureRootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
          $fixtureRootItem.FullName -cne $provenanceFixtureRoot) {
        throw 'provenance fixture cleanup target is unsafe'
      }
      Remove-Item -LiteralPath $provenanceFixtureRoot -Recurse -Force
    }
    if (Test-Path -LiteralPath $provenanceFixtureRoot) { throw 'provenance fixture remained after cleanup' }
  } catch {
    [void]$provenanceFixtureCleanupFailures.Add("provenance fixture cleanup: $($_.Exception.Message)")
  }
}
if ($null -ne $provenanceFixtureOriginal) {
  if ($provenanceFixtureCleanupFailures.Count) {
    throw "provenance fixture failed: $($provenanceFixtureOriginal.Exception.Message) | cleanup failed: $($provenanceFixtureCleanupFailures -join ' | ')"
  }
  throw $provenanceFixtureOriginal
}
if ($provenanceFixtureCleanupFailures.Count) {
  throw "provenance fixture cleanup failed: $($provenanceFixtureCleanupFailures -join ' | ')"
}

$planText = Read-ApprovedPlan $planWorktree $planBranch $planGoSha $planRelativePath
$pattern = '(?s)<!-- TASK7_PERF_WORKFLOW_BEGIN -->\s*```powershell\r?\n(?<workflow>.*?)\r?\n```\s*<!-- TASK7_PERF_WORKFLOW_END -->'
$matches = [regex]::Matches($planText, $pattern)
if ($matches.Count -ne 1) { throw 'exact performance workflow fence was not found once' }
$workflow = $matches[0].Groups['workflow'].Value
if ($workflow -match 'EncodedCommand') { throw 'EncodedCommand is forbidden' }
$tokens = $null
$parseErrors = $null
$workflowAst = [Management.Automation.Language.Parser]::ParseInput($workflow, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw "performance workflow PowerShell 5.1 AST failed: $($parseErrors[0].Message)" }
$collectorStderrDrainCount = [regex]::Matches($workflow, '\.StandardError\.ReadToEndAsync\(\)').Count
if ($collectorStderrDrainCount -ne 1) { throw "collector stderr drain count drifted: $collectorStderrDrainCount" }
$collectorInvokeCount = [regex]::Matches($workflow, '(?m)\bInvoke-Collector\s+\$').Count
if ($collectorInvokeCount -ne 11) { throw "collector invocation count drifted: $collectorInvokeCount" }
$collectorContextAssignments = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.AssignmentStatementAst] -and
    $node.Left -is [Management.Automation.Language.VariableExpressionAst] -and
    $node.Left.VariablePath.UserPath -ceq 'collectorContextPattern'
}, $true))
$invokeCollectorFunctions = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -ceq 'Invoke-Collector'
}, $true))
if ($collectorContextAssignments.Count -ne 1 -or $invokeCollectorFunctions.Count -ne 1) {
  throw 'collector context pattern or Invoke-Collector definition count drifted'
}
$invokeCollectorSource = $invokeCollectorFunctions[0].Extent.Text
if ([regex]::Matches($invokeCollectorSource, '\$context -cnotmatch \$collectorContextPattern').Count -ne 1 -or
    $invokeCollectorSource -match '-cnotmatch\s+[''"]') {
  throw 'Invoke-Collector does not use the unique collector context pattern'
}
$collectorContextSelfTestStart = $workflow.IndexOf('$collectorContextPattern =', [StringComparison]::Ordinal)
$collectorContextSelfTestEnd = $workflow.IndexOf('function Invoke-Collector', [StringComparison]::Ordinal)
if ($collectorContextSelfTestStart -lt 0 -or $collectorContextSelfTestEnd -le $collectorContextSelfTestStart) {
  throw 'collector context self-test region was not found'
}
$collectorContextSelfTest = $workflow.Substring(
  $collectorContextSelfTestStart,
  $collectorContextSelfTestEnd - $collectorContextSelfTestStart
)
$contextTokens = $null
$contextErrors = $null
[void][Management.Automation.Language.Parser]::ParseInput(
  $collectorContextSelfTest, [ref]$contextTokens, [ref]$contextErrors
)
if ($contextErrors.Count) { throw "collector context self-test AST failed: $($contextErrors[0].Message)" }
$contextSelfTestOutput = @(Invoke-Expression $collectorContextSelfTest)
if ($contextSelfTestOutput.Count -ne 0 -or $approvedCollectorContexts.Count -ne 243 -or
    $rejectedCollectorContexts.Count -ne 8) {
  throw 'collector context positive/negative self-test drifted or leaked output'
}

$stageSchemaStart = $workflow.IndexOf('function Assert-CollectorReadout', [StringComparison]::Ordinal)
$stageSchemaEnd = $workflow.IndexOf('function Get-CollectorExitFailure', $stageSchemaStart, [StringComparison]::Ordinal)
if ($stageSchemaStart -lt 0 -or $stageSchemaEnd -le $stageSchemaStart) {
  throw 'collector stage schema self-test region was not found'
}
$stageSchemaSource = $workflow.Substring($stageSchemaStart, $stageSchemaEnd - $stageSchemaStart)
$stageTokens = $null
$stageErrors = $null
[void][Management.Automation.Language.Parser]::ParseInput($stageSchemaSource, [ref]$stageTokens, [ref]$stageErrors)
if ($stageErrors.Count) { throw "collector stage schema AST failed: $($stageErrors[0].Message)" }
$stageSchemaOutput = @(Invoke-Expression $stageSchemaSource)
if ($stageSchemaOutput.Count -ne 0 -or $stageFixtureRejected -ne 4) {
  throw 'collector stage schema positive/negative fixtures drifted or leaked output'
}
foreach ($stageCounter in @('shownObserved', 'paintObserved', 'focusFailureObserved', 'bucketObserved')) {
  if ([regex]::Matches($workflow, "'${stageCounter}'").Count -lt 2) {
    throw "collector stage counter schema is missing $stageCounter"
  }
}
if ([regex]::Matches($workflow, 'StageBaseline = \$null').Count -ne 1 -or
    [regex]::Matches($workflow, '\$activeCollector\.StageBaseline = \$warmStageBaseline').Count -ne 1 -or
    [regex]::Matches($workflow, 'shown=\$shown paint=\$paint focusFailure=\$focusFailure bucket=\$bucket').Count -ne 1 -or
    [regex]::Matches($workflow, 'stageShown -ne 205').Count -ne 1 -or
    [regex]::Matches($workflow, 'stagePaint -ne 205').Count -ne 1 -or
    [regex]::Matches($workflow, 'stageFocusFailure -ne 0').Count -ne 1 -or
    [regex]::Matches($workflow, 'stageBucket -ne 205').Count -ne 1) {
  throw 'formal warm stage baseline, failure output, or terminal totals drifted'
}
if ([regex]::Matches($workflow, 'JSON\.stringify\(\{ ok: matched, readout: value \}\)').Count -ne 1 -or
    [regex]::Matches($workflow, 'collector-request-timeout context=\$context').Count -ne 1) {
  throw 'collector structured timeout evidence drifted'
}
$cleanupPassLiteral = 'Write-' + 'Output ' + [char]39 + 'Cleanup=' + 'PASS' + [char]39
$runnerCleanupFailLiteral = 'Write-' + 'Output "Cleanup=' + 'FAIL category=runner'
if ($workflow.Contains($cleanupPassLiteral) -or
    [regex]::Matches($workflow, 'Write-Output "Cleanup=FAIL category=preflight').Count -ne 1 -or
    [regex]::Matches($workflow, 'Write-Output "Cleanup=FAIL category=query-seed').Count -ne 2 -or
    [regex]::Matches($workflow, 'Write-Output "Cleanup=FAIL category=cold').Count -ne 2 -or
    [regex]::Matches($workflow, 'Write-Output "Cleanup=FAIL category=workflow').Count -ne 2 -or
    [regex]::Matches($planText, [regex]::Escape($runnerCleanupFailLiteral)).Count -ne 3 -or
    [regex]::Matches($planText, [regex]::Escape($cleanupPassLiteral)).Count -ne 2) {
  throw 'cleanup PASS/FAIL category output contract drifted'
}

foreach ($forbiddenInputSource in @(
  'SendKeys', 'System.Windows.Forms', 'Clipboard', 'Input.dispatchKeyEvent', 'Input.insertText',
  'Input.imeSetComposition', 'document.execCommand', 'ActivateKeyboardLayout', 'LoadKeyboardLayout',
  'Set-WinUserLanguageList', 'ImmSetCompositionString', 'ImmNotifyIME'
)) {
  if ($workflow.IndexOf($forbiddenInputSource, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
    throw "forbidden measurement input producer remains: $forbiddenInputSource"
  }
}
if ($workflow -match '(?i)\b(?:retry|heuristic)\b' -or $workflow -match '(?i)\.value\s*=') {
  throw 'measurement input producer contains a forbidden retry, heuristic, or DOM mutation'
}
$tsHarnessLiteral = 'const HARNESS_' + 'QUERY = ' + [char]39 + 'ca' + 'lc' + [char]39
$tsHarnessPattern = [regex]::Escape($tsHarnessLiteral)
$runnerHarnessLiteral = '$harnessQuery = ' + [char]39 + 'ca' + 'lc' + [char]39
$runnerHarnessPattern = '(?m)^\$harnessQuery = ''calc''$'
$tsHarnessLiteralCount = [regex]::Matches(
  $planText, $tsHarnessPattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant
).Count
$runnerHarnessLiteralCount = [regex]::Matches(
  $workflow, $runnerHarnessPattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant
).Count
if ($tsHarnessLiteralCount -ne 1 -or $runnerHarnessLiteralCount -ne 1) {
  throw 'fixed calc harness literal drifted'
}
$tsNonCalcFixture = $planText.Replace($tsHarnessLiteral, $tsHarnessLiteral.Replace('calc', 'other'))
$runnerNonCalcFixture = $workflow.Replace($runnerHarnessLiteral, $runnerHarnessLiteral.Replace('calc', 'other'))
$tsNonCalcFixtureCount = [regex]::Matches(
  $tsNonCalcFixture, $tsHarnessPattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant
).Count
$runnerNonCalcFixtureCount = [regex]::Matches(
  $runnerNonCalcFixture, $runnerHarnessPattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant
).Count
if ($tsNonCalcFixtureCount -ne 0 -or $runnerNonCalcFixtureCount -ne 0) {
  throw 'non-calc harness literal fixture was not rejected'
}
$unicodeInputHelpers = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -ceq 'Invoke-HarnessQueryInput'
}, $true))
if ($unicodeInputHelpers.Count -ne 1) { throw 'fixed harness SendInput helper count drifted' }
$unicodeInputCalls = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'Invoke-HarnessQueryInput'
}, $true))
if ($unicodeInputCalls.Count -ne 3 -or
    [regex]::Matches($workflow, '(?m)^\s*Invoke-HarnessQueryInput\s*$').Count -ne 2 -or
    [regex]::Matches($workflow, 'Invoke-HarnessQueryInput -ValidateOnly').Count -ne 1) {
  throw 'fixed harness helper must have one no-injection self-test and two production calls'
}
if ([regex]::Matches($workflow, 'public static extern uint SendInput\(').Count -ne 1 -or
    [regex]::Matches($workflow, '\[Task7UnicodeInput\]::SendInput\(').Count -ne 1 -or
    [regex]::Matches($workflow, '\[Task7UnicodeInput\+INPUT\[\]\]::new\(8\)').Count -ne 1) {
  throw 'fixed harness P/Invoke or exact eight-INPUT construction drifted'
}
$querySeedRegionStart = $workflow.IndexOf('$querySeedPrimary = $null', [StringComparison]::Ordinal)
$querySeedRegionEnd = $workflow.IndexOf('for ($sample = 1; $sample -le 30; $sample++)', [StringComparison]::Ordinal)
$warmRegionStart = $workflow.LastIndexOf('$activePrimary = Start-OwnedPrimary', [StringComparison]::Ordinal)
$warmRegionEnd = $workflow.IndexOf('$warm = Invoke-Collector', $warmRegionStart, [StringComparison]::Ordinal)
if ($querySeedRegionStart -lt 0 -or $querySeedRegionEnd -le $querySeedRegionStart -or
    $warmRegionStart -lt 0 -or $warmRegionEnd -le $warmRegionStart) {
  throw 'fixed harness production regions were not found'
}
$querySeedRegion = $workflow.Substring($querySeedRegionStart, $querySeedRegionEnd - $querySeedRegionStart)
$warmRegion = $workflow.Substring($warmRegionStart, $warmRegionEnd - $warmRegionStart)
if ([regex]::Matches($querySeedRegion, '(?m)^\s*Invoke-HarnessQueryInput\s*$').Count -ne 1 -or
    [regex]::Matches($querySeedRegion, '(?m)^\s*Assert-PrimaryOwnsForeground \$querySeedPrimary\s*$').Count -ne 2 -or
    [regex]::Matches($warmRegion, '(?m)^\s*Invoke-HarnessQueryInput\s*$').Count -ne 1 -or
    [regex]::Matches($warmRegion, '(?m)^\s*Assert-PrimaryOwnsForeground \$activePrimary\s*$').Count -ne 2) {
  throw 'preflight and warm workflow must each call one helper between two foreground checks'
}

function Get-AutomaticPidParameterCount([string]$source) {
  $fixtureTokens = $null
  $fixtureErrors = $null
  $fixtureAst = [Management.Automation.Language.Parser]::ParseInput(
    $source, [ref]$fixtureTokens, [ref]$fixtureErrors
  )
  if ($fixtureErrors.Count) { throw "PID parameter oracle source failed to parse: $($fixtureErrors[0].Message)" }
  @($fixtureAst.FindAll({
    param($node)
    $node -is [Management.Automation.Language.ParameterAst] -and
      [string]::Equals($node.Name.VariablePath.UserPath, 'PID', [StringComparison]::OrdinalIgnoreCase)
  }, $true)).Count
}

if ((Get-AutomaticPidParameterCount $workflow) -ne 0) {
  throw 'forbidden read-only PID parameter collision remains'
}
foreach ($pidFixture in @('function f([int]$pid) {}', 'function f([int]$PID) {}')) {
  if ((Get-AutomaticPidParameterCount $pidFixture) -ne 1) {
    throw 'case-insensitive PID parameter oracle fixture failed'
  }
}

$addCleanupAst = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Add-CleanupFailure'
}, $true))
$stopTreeAst = @($workflowAst.FindAll({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Stop-OwnedTree'
}, $true))
if ($addCleanupAst.Count -ne 1 -or $stopTreeAst.Count -ne 1) { throw 'ownership fixture function count drifted' }
$stopTreeSource = $stopTreeAst[0].Extent.Text
if ($stopTreeSource -match 'New-OwnedRecord|primary fallback identity') {
  throw 'Stop-OwnedTree still contains an unauthenticated ancestry fallback'
}
Invoke-Expression $addCleanupAst[0].Extent.Text
Invoke-Expression $stopTreeSource
$script:fixtureNewOwnedSetCalls = 0
$script:fixtureDiscoverCalls = 0
function New-OwnedSet([Diagnostics.Process]$primary) {
  $script:fixtureNewOwnedSetCalls++
  throw 'fixture ownership failure'
}
function Discover-OwnedDescendants([hashtable]$owned) {
  $script:fixtureDiscoverCalls++
  throw 'fixture descendant discovery must not run'
}
function Get-ExactExecutableRows([string]$path) { @() }
function Get-PortListeners { @() }

$fixturePrimary = $null
$fixtureWitness = $null
$fixturePrimaryHandedToCleanup = $false
$ownershipFixtureFailure = $null
$ownershipFixturePrimaryExited = $false
try {
  $fixturePrimary = Start-Process -FilePath powershell.exe -ArgumentList @(
    '-NoProfile', '-Command', 'Start-Sleep -Seconds 30'
  ) -WindowStyle Hidden -PassThru
  if ($null -eq $fixturePrimary) { throw 'ownership fixture primary did not start' }
  [void]$fixturePrimary.SafeHandle
  $fixtureWitness = [Diagnostics.Process]::GetProcessById($fixturePrimary.Id)
  [void]$fixtureWitness.SafeHandle
  $fixturePrimaryHandedToCleanup = $true
  try { Stop-OwnedTree $fixturePrimary 'Z:\uipilot-ownership-fixture.exe' } catch { $ownershipFixtureFailure = $_ }
  $ownershipFixturePrimaryExited = $fixtureWitness.WaitForExit(5000)
} finally {
  try {
    if ($null -ne $fixtureWitness -and -not $fixtureWitness.HasExited) {
      $fixtureWitness.Kill()
      [void]$fixtureWitness.WaitForExit(2000)
    }
  } finally {
    if ($null -ne $fixtureWitness) { $fixtureWitness.Dispose() }
    if ($null -ne $fixturePrimary -and -not $fixturePrimaryHandedToCleanup) {
      try {
        if (-not $fixturePrimary.HasExited) {
          $fixturePrimary.Kill()
          [void]$fixturePrimary.WaitForExit(2000)
        }
      } finally {
        $fixturePrimary.Dispose()
      }
    }
  }
}
if ($null -eq $ownershipFixtureFailure -or
    -not $ownershipFixtureFailure.Exception.Message.Contains('fixture ownership failure') -or
    $script:fixtureNewOwnedSetCalls -ne 1 -or $script:fixtureDiscoverCalls -ne 0 -or
    -not $ownershipFixturePrimaryExited) {
  throw 'ownership failure fixture did not limit cleanup to the retained primary'
}

function Convert-ToSanitizedWorkflowOutput([string]$value) {
  if ($null -eq $value) { return '' }
  ([regex]::Replace($value, '[\x00-\x08\x0B\x0C\x0E-\x1F]', '')).Trim()
}

function Invoke-CapturedPowerShellFile([string]$path, [bool]$replayOutput) {
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = 'powershell.exe'
  $startInfo.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$path`""
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $child = [Diagnostics.Process]::new()
  $child.StartInfo = $startInfo
  $childStarted = $false
  $childOriginal = $null
  $childCleanupFailures = [Collections.Generic.List[string]]::new()
  try {
    if (-not $child.Start()) { throw 'PowerShell child did not start' }
    $childStarted = $true
    [void]$child.SafeHandle
    $stdoutTask = $child.StandardOutput.ReadToEndAsync()
    $stderrTask = $child.StandardError.ReadToEndAsync()
    $child.WaitForExit()
    $exitCode = $child.ExitCode
    $failed = $exitCode -ne 0
    $stdout = Convert-ToSanitizedWorkflowOutput $stdoutTask.Result
    $stderr = Convert-ToSanitizedWorkflowOutput $stderrTask.Result
    if ($replayOutput -and $stdout.Length) { [Console]::Out.WriteLine($stdout) }
    if ($replayOutput -and $stderr.Length) { [Console]::Error.WriteLine($stderr) }
    if ($failed) {
      $capturedPrimary = (@($stderr, $stdout) | Where-Object { $_.Length }) -join ' | '
      if (-not $capturedPrimary.Length) { $capturedPrimary = "child exited with code $exitCode without output" }
      throw "PrimaryFailure: $capturedPrimary (exit $exitCode)"
    }
  } catch {
    $childOriginal = $_
  } finally {
    if ($childStarted) {
      try {
        if (-not $child.HasExited) {
          $child.Kill()
          if (-not $child.WaitForExit(2000)) { throw 'PowerShell child did not exit after retained-handle kill' }
        }
      } catch {
        [void]$childCleanupFailures.Add("PowerShell child cleanup: $($_.Exception.Message)")
      }
    }
    try { $child.Dispose() } catch {
      [void]$childCleanupFailures.Add("PowerShell child handle disposal: $($_.Exception.Message)")
    }
  }
  if ($null -ne $childOriginal) {
    if ($childCleanupFailures.Count) {
      throw "$($childOriginal.Exception.Message) | child cleanup failed: $($childCleanupFailures -join ' | ')"
    }
    throw $childOriginal
  }
  if ($childCleanupFailures.Count) { throw "PowerShell child cleanup failed: $($childCleanupFailures -join ' | ')" }
}

$sentinelPath = Join-Path $env:TEMP ("uipilot-task7-child-sentinel-$([guid]::NewGuid().ToString('N')).ps1")
$sentinelFailure = $null
try {
  [IO.File]::WriteAllText($sentinelPath, "throw 'TASK7_CHILD_SENTINEL'", [Text.UTF8Encoding]::new($false))
  try { Invoke-CapturedPowerShellFile $sentinelPath $false } catch { $sentinelFailure = $_ }
} finally {
  if (Test-Path -LiteralPath $sentinelPath) { Remove-Item -LiteralPath $sentinelPath -Force }
}
if ((Test-Path -LiteralPath $sentinelPath) -or $null -eq $sentinelFailure -or
    -not $sentinelFailure.Exception.Message.Contains('PrimaryFailure:') -or
    -not $sentinelFailure.Exception.Message.Contains('TASK7_CHILD_SENTINEL')) {
  throw 'captured child sentinel fixture failed or left its temporary file'
}

[pscustomobject]@{
  PlanProvenanceNegativeFixture = 'PASS'
  AutomaticPidCaseFixtures = 2
  OwnershipFailureNewOwnedSetCalls = $script:fixtureNewOwnedSetCalls
  OwnershipFailureDescendantDiscoveryCalls = $script:fixtureDiscoverCalls
  OwnershipFailurePrimaryExited = $ownershipFixturePrimaryExited
  ChildPrimaryFailureSentinelPreserved = $true
  ChildSentinelFileRemoved = -not (Test-Path -LiteralPath $sentinelPath)
  CollectorStderrDrainCount = $collectorStderrDrainCount
  CollectorInvokeCount = $collectorInvokeCount
  CollectorContextPositiveFixtures = $approvedCollectorContexts.Count
  CollectorContextNegativeFixtures = $rejectedCollectorContexts.Count
  CollectorStageSchemaNegativeFixtures = $stageFixtureRejected
  TypeScriptCalcLiteralCount = $tsHarnessLiteralCount
  RunnerCalcLiteralCount = $runnerHarnessLiteralCount
  TypeScriptNonCalcFixtureCount = $tsNonCalcFixtureCount
  RunnerNonCalcFixtureCount = $runnerNonCalcFixtureCount
} | Format-List

if ($measurementHash -cnotmatch '^[0-9A-F]{64}$') { throw 'measurement artifact hash is invalid' }
$sha256 = [Security.Cryptography.SHA256]::Create()
try {
  $expectedRunnerHash = ([BitConverter]::ToString(
    $sha256.ComputeHash([Text.UTF8Encoding]::new($false).GetBytes($workflow))
  )).Replace('-', '')
} finally {
  $sha256.Dispose()
}

$runnerPath = Join-Path $env:TEMP ("uipilot-task7-performance-$([guid]::NewGuid().ToString('N')).ps1")
$runnerOriginal = $null
$runnerCleanupFailures = [Collections.Generic.List[string]]::new()
$priorMeasurementHash = [Environment]::GetEnvironmentVariable('TASK7_MEASUREMENT_SHA256', 'Process')
if ($null -ne $priorMeasurementHash) { throw 'measurement hash environment variable must be initially absent' }
try {
  if (Test-Path -LiteralPath $runnerPath) { throw 'unique runner path already exists' }
  [IO.File]::WriteAllText($runnerPath, $workflow, [Text.UTF8Encoding]::new($false))
  $runnerItem = Get-Item -LiteralPath $runnerPath -Force
  if ($runnerItem -isnot [IO.FileInfo] -or ($runnerItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'temporary runner is not a regular non-reparse file'
  }
  $runnerBytes = [IO.File]::ReadAllBytes($runnerPath)
  if ($runnerBytes.Length -ge 3 -and $runnerBytes[0] -eq 0xEF -and $runnerBytes[1] -eq 0xBB -and $runnerBytes[2] -eq 0xBF) {
    throw 'temporary runner contains a UTF-8 BOM'
  }
  $runnerHash = (Get-FileHash -LiteralPath $runnerPath -Algorithm SHA256).Hash
  if ($runnerHash -cne $expectedRunnerHash) { throw 'temporary runner SHA-256 does not match exact fenced content' }
  if ((Get-FileHash -LiteralPath $runnerPath -Algorithm SHA256).Hash -cne $runnerHash) { throw 'temporary runner SHA-256 changed before launch' }
  $runnerSource = [IO.File]::ReadAllText($runnerPath, [Text.UTF8Encoding]::new($false))
  if ($runnerSource -cne $workflow) { throw 'temporary runner differs from exact fenced workflow' }
  $runnerTokens = $null
  $runnerParseErrors = $null
  [void][Management.Automation.Language.Parser]::ParseFile($runnerPath, [ref]$runnerTokens, [ref]$runnerParseErrors)
  if ($runnerParseErrors.Count) { throw "temporary runner PowerShell 5.1 AST failed: $($runnerParseErrors[0].Message)" }
  [Environment]::SetEnvironmentVariable('TASK7_MEASUREMENT_SHA256', $measurementHash, 'Process')
  Invoke-CapturedPowerShellFile $runnerPath $true
} catch {
  $runnerOriginal = $_
} finally {
  try {
    [Environment]::SetEnvironmentVariable('TASK7_MEASUREMENT_SHA256', $priorMeasurementHash, 'Process')
  } catch {
    [void]$runnerCleanupFailures.Add("measurement hash environment restore: $($_.Exception.Message)")
  }
  try {
    if (Test-Path -LiteralPath $runnerPath) { Remove-Item -LiteralPath $runnerPath -Force }
    if (Test-Path -LiteralPath $runnerPath) { throw 'temporary runner remained after removal' }
  } catch {
    [void]$runnerCleanupFailures.Add("temporary runner cleanup: $($_.Exception.Message)")
  }
}
if ($null -ne $runnerOriginal) {
  if ($runnerCleanupFailures.Count) {
    Write-Output "Cleanup=FAIL category=runner count=$($runnerCleanupFailures.Count)"
    throw "performance runner failed: $($runnerOriginal.Exception.Message) | cleanup failed: $($runnerCleanupFailures -join ' | ')"
  }
  if ($runnerOriginal.Exception.Message.Contains('Cleanup=FAIL')) {
    throw $runnerOriginal
  }
  if ($runnerOriginal.Exception.Message.Contains('cleanup failed')) {
    Write-Output "Cleanup=FAIL category=runner count=1"
    throw $runnerOriginal
  }
  Write-Output 'Cleanup=PASS'
  throw $runnerOriginal
}
if ($runnerCleanupFailures.Count) {
  Write-Output "Cleanup=FAIL category=runner count=$($runnerCleanupFailures.Count)"
  throw "performance runner cleanup failed: $($runnerCleanupFailures -join ' | ')"
}
Write-Output 'Cleanup=PASS'
[pscustomobject]@{ RunnerSha256 = $runnerHash; RunnerRemoved = -not (Test-Path -LiteralPath $runnerPath) } | Format-List
```

The approved plan is read only after the exact docs root/branch/HEAD, clean index/worktree/untracked inventory, case-sensitive regular path, approved commit blob, and working file's clean-filtered blob all agree; a second blob hash closes the read race. The dirty-content fixture proves unchanged `HEAD` alone cannot authenticate a modified plan. The automatic-variable oracle compares AST parameter names to `PID` with `OrdinalIgnoreCase`, and its lowercase/uppercase fixtures must both fail closed. The child runner captures stdout/stderr in memory, strips control characters, replays only that sanitized output, and retains `PrimaryFailure` text alongside any parent cleanup failure.

Each collector record owns one retained Node `Process`, exactly one asynchronous stderr drain, one cached last readout, one optional formal-warm stage baseline, and one idempotent stop/dispose state. One case-sensitive `$collectorContextPattern` is the only runtime context inventory; its no-product self-test accepts all 243 approved values and rejects eight fixed case, whitespace, affix, and unknown-value fixtures before any product process starts. Node returns the exact `{ ok, readout }` envelope, including the last valid readout when its fixed request deadline expires; PowerShell authenticates the full readout and computes only relative-baseline `shown`, `paint`, `focusFailure`, and `bucket` integers. A successful request still emits no context, counters, or individual timing. A failure emits only one approved context, those four relative integers, and a fixed class: `collector-request-timeout`, `collector-response-deadline` with `elapsedBoundMs=12000 alive=true`, `collector-exited-without-response` with exit code and bounded sanitized `PrimaryFailure`, `collector-read-fault`, `collector-write-fault`, `collector-invalid-json`, `collector-invalid-envelope`, or `collector-invalid-readout`. The positive schema fixture and Decimal, Double, extra-field, and missing-field fixtures execute before any product process; all four invalid fixtures must be rejected. The timeout remains exactly 12 seconds; requests are never retried, the collector is never restarted inside a sample, and no event is synthesized. The stderr/EOF fixture must classify its nonzero child as exit rather than deadline, preserve only `TASK7_COLLECTOR_SENTINEL`, and prove the process, task, handle, and file are consumed or removed once.

The prior 709.7-second run is frozen only as failed diagnostic input: prefix fixtures and zero-sample executable cleanup preflight passed, then the unclassified `CDP collector response timeout` occurred. The later complete run under plan `496b643b951fc593ae29dbb535d267cf741973f8` classified its stop at `context=harness / Error: measurement request timeout`; both runs certify zero aggregate or sample count, and no cold/warm result may be retained from either one. Their exact process, port, collector, temporary-file, browser-argument, and source recovery remains valid cleanup evidence.

The bounded follow-up diagnostic authenticated the existing measurement artifact and then recorded these exact fixed booleans: `startup { seamReady=true, startupComplete=true, focused=false, control=false, committed=false }`, `shown-focus { seamReady=true, startupComplete=true, focused=true, control=false, committed=false }`, and `after-calc { seamReady=true, startupComplete=true, focused=true, control=false, committed=false }`. It used one real no-argument secondary show request, sent the former Forms-based fixed query once, stopped at `after-calc-timeout`, sent no Enter, and recovered with cleanup failures `0`, exact executable `0`, port `9227` listeners `0`, probe temporary files `0`, browser arguments restored, and both worktrees clean. This evidence establishes that startup/show/focus/foreground passed while the former measurement input producer had zero control or committed effect; it is a Task 9 plan input and not a Task 7 product finding.

The later full run under plan `ccb48bc57d81057a993354e16ac151dfdff45838` passed the cleanup and query-seed preflights but stopped at `preserved:48 / Error: measurement request timeout`; it certifies zero aggregate or sample count and cannot be resumed or combined with any later run. The bounded stage-counter diagnostic then used a fresh isolated build and completed 60 consecutive real no-argument secondaries with exact relative counters `shown=60`, `paint=60`, `focusFailure=0`, and `bucket=60`, classified only as `diagnostic-not-reproduced count=60`. That is accepted diagnostic evidence that all four stages can complete continuously in this environment, not a formal 100-preserved or P95 result. Its independent `Cleanup=FAIL count=1` remains a cleanup No-Go even though later read-only checks found zero process, listening-port, temporary-file, environment, or worktree residue; neither fact overwrites the other.

The fixed `calc` text is harness input only and is never printed or retained. The runner constructs exactly eight keyboard `INPUT` records: one `KEYEVENTF_UNICODE` keydown and one `KEYEVENTF_UNICODE | KEYEVENTF_KEYUP` keyup for each of the four UTF-16 code units. Its sole helper calls the single `SendInput` P/Invoke and requires an exact return count of eight while preserving the immediate Win32 error on failure. The no-injection self-test authenticates the complete union layout, platform-specific `INPUT` size, count, scan codes, types, and flags before any product process starts. Clipboard, input-method or keyboard-layout switching, UI Automation, CDP input dispatch, DOM mutation, product APIs, retry, and heuristic input fallbacks remain forbidden.

Before the query-seed preflight and the warm query seed, the collector must report the WebView query input focused and enabled, and the foreground HWND PID must resolve to a still-matching retained primary/descendant process identity. The same foreground identity check runs again immediately after the shared helper. The zero-sample query-seed preflight uses one retained primary, waits for startup, starts exactly one no-argument secondary for the real Task 6 show path, applies the shared helper once, and requires `queryMatchesHarnessValue: true` through the existing 12-second collector deadline. It cleans the collector, primary tree, browser arguments, collector file, exact executable, and port before any of the 30 cold or 205 warm measurements begin. The warm workflow calls that same helper once; it has no second injection implementation.

The Node collector accepts only the seven exact request shapes above, treats a missing seam as not-ready until the fixed deadline, then validates the strict full numeric/boolean schema on every read and prints only aggregate readouts. The warm primary's existing `startup` response is the one formal stage baseline; the separate query-seed preflight completes and cleans before this baseline exists. Every one of the 205 warm events comes from a no-argument second launch of the exact release executable and is acknowledged before the next launch. Final success requires relative `shown=205`, `paint=205`, `focusFailure=0`, and `bucket=205` together with `warmup=5`, `empty=100`, `preserved=100`, and all existing P95 thresholds. Port 9227 must be owned only by the authenticated primary process tree; the HTTP and WebSocket endpoints must both be loopback and expose exactly one page. Every stored process/CIM start or exit timestamp is first truncated to the shared CIM microsecond boundary; every identity equality and parent/child ordering check then remains exact, and the executable current-process control must reject an exact one-microsecond offset. A tolerance window is forbidden. Cleanup does one non-blocking discovery pass, closes or kills the retained primary first, records its normalized exit cutoff, then discovers and cleans eligible children until two consecutive passes add nothing and retain no live owned handle. If initial ownership authentication fails, cleanup records the failure, closes or kills only the retained primary handle, performs no descendant discovery or kill, restores the environment, and fails on any exact-executable or port residue. Otherwise, a child discovered after primary exit is expected cleanup work when its normalized creation time is at or before its authenticated parent's exit cutoff; PID reuse, child-before-parent, creation after the cutoff, unverifiable identity, deadline exhaustion, exact-executable residue, or port residue fails closed. Collector, tree, browser-argument, temporary collector/runner, and residue cleanup are attempted independently; all cleanup failures are accumulated without replacing an earlier workflow failure. Any cleanup error must emit exact `Cleanup=FAIL category=<preflight|query-seed|cold|workflow|runner> count=N`; only an execution with zero accumulated cleanup errors may emit `Cleanup=PASS`. Later external zero-residue evidence never overwrites a recorded cleanup failure. Every authenticated retained process handle, including the primary, remains alive through discovery and final residue checks and is disposed exactly once. Bare PID termination and `EncodedCommand` are forbidden.

After performance evidence, restore and rebuild the final clean release from the first line:

```powershell
git restore --worktree -- src/main.ts
if ($LASTEXITCODE -ne 0) { throw 'measurement wrapper restore failed' }
$status = @(& git status --porcelain=v1 --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'clean source status failed' }
if ($status.Count) { throw 'measurement wrapper was not fully removed' }
$finalBuildStarted = (Get-Date).ToUniversalTime()
npm.cmd run tauri build -- --no-bundle
if ($LASTEXITCODE -ne 0) { throw 'final clean Tauri release build failed' }
$metadataJson = @(cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps)
if ($LASTEXITCODE -ne 0) { throw 'final cargo metadata failed' }
try { $metadata = ($metadataJson -join "`n") | ConvertFrom-Json } catch { throw 'final cargo metadata returned invalid JSON' }
$finalExe = Get-Item -LiteralPath (Join-Path ([string]$metadata.target_directory) 'release\uipilot.exe') -Force
if ($finalExe -isnot [IO.FileInfo] -or $finalExe.LastWriteTimeUtc -lt $finalBuildStarted) { throw 'final clean release executable was not rebuilt' }
[pscustomobject]@{
  Path = $finalExe.FullName
  Sha256 = (Get-FileHash -LiteralPath $finalExe.FullName -Algorithm SHA256).Hash
  LastWriteTimeUtc = $finalExe.LastWriteTimeUtc.ToString('O')
  Length = $finalExe.Length
} | Format-List
```

Use only this final clean release exe for the 100/150/200% zoom, Narrator, forced-colors, long-text, focus/navigation, and zero-remote-network smoke above. Launch and clean it with the same exact-path/PID-tree flow; remote debugging may be enabled only to inspect network entries, never to change app state. Require the source worktree to remain clean before and after every final smoke. The measurement hash and final clean hash are reported separately; neither artifact is an installer, signed artifact, trial, or release.

Any threshold/accessibility failure returns to the owning RED/GREEN task. It cannot be waived by jsdom.

- [ ] **Step 4: Run the local package/source/trust checkpoint from the first line**

This step follows Step 2 directly. It has no dependency on historical Steps 3/3A and makes no release/QA claim.

```powershell
$ErrorActionPreference = 'Stop'
function Assert-NativeExit([string]$label) {
  if ($LASTEXITCODE -ne 0) { throw "$label failed with exit $LASTEXITCODE" }
}
$baseline = 'a8626e72e97a5caa924333e6d6545efe9cd2e6d0'
git merge-base --is-ancestor $baseline HEAD
Assert-NativeExit 'Task 6 baseline ancestry'
$expected = @(
  'package.json','package-lock.json','tsconfig.json','src/launcher-core.ts','src/native-input.ts',
  'src/launcher-view.tsx','src/launcher.test.tsx','src/main.ts','src/protocol.ts','src/styles.css'
) | Sort-Object -CaseSensitive -Unique
$rootText = (& git rev-parse --show-toplevel).Trim()
Assert-NativeExit 'authenticate worktree root'
$root = [IO.Path]::GetFullPath($rootText).TrimEnd('\')
$changed = @(& git diff --name-only "$baseline..HEAD")
Assert-NativeExit 'final changed-path inventory'
$changed = @($changed | Sort-Object -CaseSensitive -Unique)
$scopeDelta = @(Compare-Object -CaseSensitive $expected $changed)
if ($scopeDelta.Count) { throw "final exact path set drifted: $($scopeDelta -join '; ')" }
if (@($changed | Where-Object { $_ -ceq 'src/main.ts' }).Count -ne 1) { throw 'final src/main.ts count is not one' }
if (@($changed | Where-Object { $_ -ceq 'src/launcher-core.test.ts' }).Count -ne 0) { throw 'split core test file is forbidden' }
if (Test-Path -LiteralPath 'src/launcher-core.test.ts') { throw 'split core test file exists anywhere in final worktree' }
$status = @(& git status --porcelain=v1 --untracked-files=all)
Assert-NativeExit 'final worktree status'
if ($status.Count) { throw 'worktree/index/untracked is not clean' }
foreach ($frozen in @('index.html','vite.config.ts','src-tauri','security-probe.html','src/security-probe.ts','scripts')) {
  git diff --quiet $baseline HEAD -- $frozen
  Assert-NativeExit "frozen path comparison $frozen"
}
$inventoryRoots = @('src-tauri/capabilities','src-tauri/permissions','scripts')
foreach ($inventoryRoot in $inventoryRoots) {
  $baselineInventory = @(& git ls-tree -r --name-only $baseline -- $inventoryRoot)
  Assert-NativeExit "baseline inventory $inventoryRoot"
  $currentInventory = @(Get-ChildItem -LiteralPath $inventoryRoot -Recurse -Force -File | ForEach-Object {
    $full = [IO.Path]::GetFullPath($_.FullName)
    if (-not $full.StartsWith("$root\", [StringComparison]::OrdinalIgnoreCase)) { throw "inventory escaped root: $full" }
    $full.Substring($root.Length + 1).Replace('\','/')
  })
  if (@(Compare-Object -CaseSensitive $baselineInventory $currentInventory).Count) { throw "frozen inventory drifted: $inventoryRoot" }
}
$auditPaths = $expected + @('index.html','vite.config.ts','src-tauri','security-probe.html','src/security-probe.ts','scripts')
$flagged = @(& git ls-files -v -- $auditPaths | Where-Object { $_ -cmatch '^[a-zS] ' })
Assert-NativeExit 'audited index flags'
if ($flagged.Count) { throw "assume-unchanged/skip-worktree audited input: $($flagged -join '; ')" }
foreach ($relative in $auditPaths) {
  $item = Get-Item -LiteralPath $relative -Force
  $expectsDirectory = $relative -in @('src-tauri','scripts')
  if (($item -is [IO.DirectoryInfo]) -ne $expectsDirectory) { throw "audited path type changed: $relative" }
  $cursor = $item
  while ($null -ne $cursor) {
    if (($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "reparse audited input: $relative" }
    if ($cursor -ne $item -and $cursor -isnot [IO.DirectoryInfo]) { throw "non-directory audited parent: $relative" }
    if ([IO.Path]::GetFullPath($cursor.FullName).TrimEnd('\') -eq $root) { break }
    $cursor = if ($cursor -is [IO.DirectoryInfo]) { $cursor.Parent } else { $cursor.Directory }
  }
  if ($null -eq $cursor) { throw "audited path escaped root: $relative" }
}
foreach ($tree in @('src-tauri','scripts')) {
  $reparse = @(Get-ChildItem -LiteralPath $tree -Recurse -Force | Where-Object {
    ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
  })
  if ($reparse.Count) { throw "reparse inside frozen tree: $tree" }
}
$nodeVersion = node --version
Assert-NativeExit 'node --version'
$npmVersion = npm.cmd --version
Assert-NativeExit 'npm --version'
if ($nodeVersion.Trim() -ne 'v22.21.1' -or $npmVersion.Trim() -ne '10.9.4') { throw 'Node/npm drifted' }
if ((Get-FileHash package.json -Algorithm SHA256).Hash -ne 'B6DEDB7563EFEC6ACF8C4E50CB2DFAA567BCC6140A554A87AC0903C5C122B005') { throw 'package hash drifted' }
if ((Get-FileHash package-lock.json -Algorithm SHA256).Hash -ne '783B63D3F591E40F3F9D3BE6A85AEE2B18C5E0A83982DD8BC8763218CF05EE22') { throw 'lock hash drifted' }
@'
const lock = require('./package-lock.json')
const root = lock.packages['']
if (!root || lock.lockfileVersion !== 3 || Object.keys(lock.packages).length !== 220) throw new Error('lock inventory')
const exact = { antd: '6.5.1', react: '19.2.7', 'react-dom': '19.2.7' }
for (const [name, version] of Object.entries(exact)) if (root.dependencies[name] !== version) throw new Error(`direct ${name}`)
if (root.devDependencies['@types/react'] !== '19.2.17' || root.devDependencies['@types/react-dom'] !== '19.2.3') throw new Error('React types')
for (const [path, entry] of Object.entries(lock.packages)) {
  if (path === '') continue
  if (entry.link || !entry.resolved?.startsWith('https://registry.npmmirror.com/') || !entry.integrity?.startsWith('sha512-')) throw new Error(`entry ${path}`)
  if (!entry.license) throw new Error(`missing license ${path}`)
  if (entry.hasInstallScript && path !== 'node_modules/fsevents') throw new Error(`unapproved lifecycle script ${path}`)
}
'@ | node
Assert-NativeExit 'final package-lock Node oracle'
$view = Get-Content -Raw -Encoding utf8 src/launcher-view.tsx
$core = Get-Content -Raw -Encoding utf8 src/launcher-core.ts
$main = Get-Content -Raw -Encoding utf8 src/main.ts
$protocol = Get-Content -Raw -Encoding utf8 src/protocol.ts
$native = Get-Content -Raw -Encoding utf8 src/native-input.ts
foreach ($forbidden in @('@tauri-apps/api','@ant-design/icons','dangerouslySetInnerHTML')) {
  if ($view.Contains($forbidden)) { throw "view boundary: $forbidden" }
}
if ($view -cmatch '(?m)\b(?:AutoComplete|Select|Card|Modal|Popconfirm)\b') { throw 'forbidden AntD component' }
if ($core -match '(?m)(?:from\s+|import\()\s*["''](?:react|antd|@tauri-apps/api)') { throw 'core boundary' }
foreach ($required in @('compositionStart','compositionInput','ordinaryInput','compositionBoundary')) {
  if (-not $protocol.Contains($required)) { throw "R3 protocol missing $required" }
}
foreach ($required in @('isTrusted','isComposing','compositionstart','compositionend','addEventListener','removeEventListener')) {
  if (-not $native.Contains($required)) { throw "R3 native boundary missing $required" }
}
foreach ($forbidden in @('compositionupdate','compositionUpdate','compositionEnd','setTimeout','queueMicrotask','tombstone','suppression')) {
  if ($protocol.Contains($forbidden) -or $native.Contains($forbidden) -or $core.Contains($forbidden)) {
    throw "R3 source retained $forbidden"
  }
}
if ([regex]::Matches($main, "'launcher://shown'").Count -ne 1) { throw 'event inventory' }
foreach ($command in @('search_apps','execute_result','load_settings','save_settings','rescan_apps','export_validation_data','clear_validation_data','hide_launcher')) {
  if ([regex]::Matches($main, "'$command'").Count -ne 1) { throw "command inventory: $command" }
}
git diff --check "$baseline..HEAD"
$diffCheckExit = $LASTEXITCODE
if ($diffCheckExit -ne 0) { throw "final diff check failed with exit $diffCheckExit" }
$commits = @(& git rev-list --reverse "$baseline..HEAD")
Assert-NativeExit 'final commit inventory'
foreach ($commit in $commits) {
  git show --check --format= $commit
  Assert-NativeExit "commit check $commit"
}
```

Expected: the committed diff is exactly the ten final paths, `src/launcher.test.tsx` is the sole test path, `src/main.ts` count is one, `src/launcher-core.test.ts` is absent, all frozen paths are byte-identical, and every package/source/index/reparse/commit check passes.

- [ ] **Step 5: Request final local Task 7 code review**

Report:

- R3 Design/Plan/Security Go SHAs, Task 5 Phase-A baseline, Task 6 Code Go/local integration/trust baseline, and final HEAD.
- Ordered commits with exact file sets.
- Dependency hashes, exact 70-entry appendix, lifecycle/audit/npm-ls outcomes.
- Fresh tests/build/Rust/Clippy/security config results.
- R1 Pass-A and R2 post-GREEN No-Go evidence plus the passing R3 production-gate environment/sanitized result; no query
  text or value.
- Final local Vite bundle raw/gzip. Explicitly report that startup/show P95, Job cleanup certification, zoom/Narrator,
  forced-colors/long-text/focus, zero-network smoke, runtime-positive probe, installer/signing/trial/release remain
  unresolved release/QA blockers.
- Trust checkpoint output and clean staged/unstaged/untracked status.
- Explicit local `TaskCodeGo` request and explicit `ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` status. Do not claim
  final release Security Go from this local gate.

Do not merge main, push, sign, trial, release, run the runtime positive probe, or clean evidence worktrees while review is pending.

---

## Appendix A: Exact Added Dependency Entries

All 70 entries below are additions relative to `1dafdac` lockfile. All resolve from `registry.npmmirror.com`, have no lifecycle script, and are the complete non-root delta; no baseline non-root entry changes or disappears.

| Package path | Version | Role | License | Integrity |
|---|---:|---|---|---|
| `@ant-design/colors` | `8.0.1` | transitive prod | `MIT` | `sha512-foPVl0+SWIslGUtD/xBr1p9U4AKzPhNYEseXYRRo5QSzGACYZrQbe11AYJbYfAWnWSpGBx6JjBmSeugUsD9vqQ==` |
| `@ant-design/cssinjs` | `2.1.2` | transitive prod | `MIT` | `sha512-2Hy8BnCEH31xPeSLbhhB2ctCPXE2ZnASdi+KbSeS79BNbUhL9hAEe20SkUk+BR8aKTmqb6+FKFruk7w8z0VoRQ==` |
| `@ant-design/cssinjs-utils` | `2.1.2` | transitive prod | `MIT` | `sha512-5fTHQ158jJJ5dC/ECeyIdZUzKxE/mpEMRZxthyG1sw/AKRHKgJBg00Yi6ACVXgycdje7KahRNvNET/uBccwCnA==` |
| `@ant-design/fast-color` | `3.0.1` | transitive prod | `MIT` | `sha512-esKJegpW4nckh0o6kV3Tkb7NPIZYbPnnFxmQDUmL08ukXZAvV85TZBr70eGuke/CIArLaP6aw8lt9KILjnWuOw==` |
| `@ant-design/icons` | `6.3.2` | transitive prod | `MIT` | `sha512-B6O5a5XJ4wjtNOfZejXYwHW5zvKV5gYkjGf11dHGLEbKn0ABDGndo41+gfIiXyTFhvESj4XTotuud33mUFid0g==` |
| `@ant-design/icons-svg` | `4.5.0` | transitive prod | `MIT` | `sha512-1BTUFyKPTBZ53MuTP8s0k5SFEXL7o3VHEOwLgzaoWKwnBeqIcqUtVshc4SKzhI6uACfqhJqBwBUE9FsWR3uULA==` |
| `@ant-design/react-slick` | `2.0.0` | transitive prod | `MIT` | `sha512-HMS9sRoEmZey8LsE/Yo6+klhlzU12PisjrVcydW3So7RdklyEd2qehyU6a7Yp+OYN72mgsYs3NFCyP2lCPFVqg==` |
| `@babel/runtime` | `7.29.7` | transitive prod | `MIT` | `sha512-Nq8OhGWiZIZGV6hLHoyAKLLcJihP/xFeBMGJoUrxTX2psI8dCifzLhZISFb+VWS3wFMRDmCGw5R+dOySCqPLhw==` |
| `@emotion/hash` | `0.8.0` | transitive prod | `MIT` | `sha512-kBJtf7PH6aWwZ6fka3zQ0p6SBYzx4fl1LoZXE2RrnYST9Xljm7WfKJrU4g/Xr3Beg72MLrp1AWNUmuYJTL7Cow==` |
| `@emotion/unitless` | `0.7.5` | transitive prod | `MIT` | `sha512-OWORNpfjMsSSUBVrRBVGECkhWcULOAJz9ZW8uK9qgxD+87M7jHRcvh/A96XXNhXTLmKcoYSQtBEX7lHMO7YRwg==` |
| `@rc-component/async-validator` | `6.0.0` | transitive prod | `MIT` | `sha512-D3AGQwdyE58gmvx6waVSXJ80JGO+IY5L2O8HDnSOex7JNlzB3GuN/4hyHNTdhy2qtOhkpbIjmeAN3tL993wKbA==` |
| `@rc-component/cascader` | `1.17.0` | transitive prod | `MIT` | `sha512-3cVNG0zrQF1PoXq262L3wGCU+/YLEC1mGSVHDl577dQmA0ZKkXFbY6nwyXo+beCcM7buo49t24jkr+QZdL7O8w==` |
| `@rc-component/checkbox` | `2.0.0` | transitive prod | `MIT` | `sha512-3CXGPpAR9gsPKeO2N78HAPOzU30UdemD6HGJoWVJOpa6WleaGB5kzZj3v6bdTZab31YuWgY/RxV3VKPctn0DwQ==` |
| `@rc-component/collapse` | `1.2.0` | transitive prod | `MIT` | `sha512-ZRYSKSS39qsFx93p26bde7JUZJshsUBEQRlRXPuJYlAiNX0vyYlF5TsAm8JZN3LcF8XvKikdzPbgAtXSbkLUkw==` |
| `@rc-component/color-picker` | `3.1.1` | transitive prod | `MIT` | `sha512-OHaCHLHszCegdXmIq2ZRIZBN/EtpT6Wm8SG/gpzLATHbVKc/avvuKi+zlOuk05FTWvgaMmpxAko44uRJ3M+2pg==` |
| `@rc-component/context` | `2.0.2` | transitive prod | `MIT` | `sha512-uiGpAlblCNlziHPwj4S4Iy/oemeuz/hR03mbiEjTCXwsqOIN3BOzsRMyDwpyO5Fm0vIEEJRUf9ZtbRLbhksuTA==` |
| `@rc-component/dialog` | `1.10.0` | transitive prod | `MIT` | `sha512-eDukNlz9vNszAGv7i3zKXdxEd3wgVmNxuJijYt8zvTh17QwTu8KK/bdURRd/lU4qaMzhO1HKKmMrwOnkaw0BvQ==` |
| `@rc-component/drawer` | `1.4.2` | transitive prod | `MIT` | `sha512-1ib+fZEp6FBu+YvcIktm+nCQ+Q+qIpwpoaJH6opGr4ofh2QMq+qdr5DLC4oCf5qf3pcWX9lUWPYX652k4ini8Q==` |
| `@rc-component/dropdown` | `1.0.3` | transitive prod | `MIT` | `sha512-YTST/N6kpqpDz3IMuM/PSSZnrDpSOA6dgHv12gPA90ZTSLv2CoqkZ0+9NtwTY6BeO7dstPblSic2QJg7dSFy/g==` |
| `@rc-component/form` | `1.8.5` | transitive prod | `MIT` | `sha512-d24EYtvUOBhxEtSd/EqIu9DaMuqrWF2IRIvAFCTM6NQ/GJIYNr8DvEpUSUlv2uPxEJ0ZPwYQ+wwlGIAaiHvdrw==` |
| `@rc-component/image` | `1.9.0` | transitive prod | `MIT` | `sha512-khF7w7xkBH5B1bsBcI1FSUZdkyd1aqpl2eYyILCqCzzQH3XdfehGUaZTnptyaJJfs09/R5hv9jXWyazOMFIClQ==` |
| `@rc-component/input` | `1.3.1` | transitive prod | `MIT` | `sha512-iFvTUT9W+JC/MSin2aGAk8NqsVlTzcExNC9DZariON1IWirju9NoNeEk47an4Q8iHazkoVI/y1LnDi88+CPcig==` |
| `@rc-component/input-number` | `1.6.2` | transitive prod | `MIT` | `sha512-Gjcq7meZlCOiWN1t1xCC+7/s85humHVokTBI7PJgTfoyw5OWF74y3e6P8PHX104g9+b54jsodFIzyaj6p8LI9w==` |
| `@rc-component/mentions` | `1.10.0` | transitive prod | `MIT` | `sha512-CI1njYUVY0NjHtLhNoVmXlJyy568Sfep9Wsak6vmGjtT6uazx98djGYlCXz2xkHhEm73g91Y3MTvzUyE5avI7w==` |
| `@rc-component/menu` | `1.4.1` | transitive prod | `MIT` | `sha512-3GsVRoQ4cnF/AoIQ4P+Z1haBfgfBPQfLT1RJY3Nu4DzOnheTslfCiGSPj7bv/cLj5sW5pHqN25dDXGP3JELAlQ==` |
| `@rc-component/mini-decimal` | `1.1.4` | transitive prod | `MIT` | `sha512-xiuXcaCwyOWpD8a8scdExFl+bntNphAW8XeenL1ig2en0AAZY0Pcp4pC0dI22qJ+NvxKn9RoNIoRdqYU3BLH4w==` |
| `@rc-component/motion` | `1.3.3` | transitive prod | `MIT` | `sha512-Xh3IszxvlSv3/PLYFyC2UZi9LNB83yOnkB/LNmRzaypZLvkhqUIPS7MQpGZcCMWrNsXV2p6YTSWbSGvFpEle9A==` |
| `@rc-component/mutate-observer` | `2.0.1` | transitive prod | `MIT` | `sha512-AyarjoLU5YlxuValRi+w8JRH2Z84TBbFO2RoGWz9d8bSu0FqT8DtugH3xC3BV7mUwlmROFauyWuXFuq4IFbH+w==` |
| `@rc-component/notification` | `2.0.7` | transitive prod | `MIT` | `sha512-nqZzpf6BPdaj+3ILx7si79LLmqPKyUmQoXa+/9gg0SkH0v1DbD66oJgRMSBEVnd/zUT3D4gwxWIHUKebYf2ZXQ==` |
| `@rc-component/overflow` | `1.0.1` | transitive prod | `MIT` | `sha512-syfmgAABaHCnCDzPwHZ/2tuvIcpOO3jefYZMmfkN+pmo8HKTzsfhS57vxo4ksPdN0By+uWVJhJWNFozNBxi2eA==` |
| `@rc-component/pagination` | `1.4.0` | transitive prod | `MIT` | `sha512-CW1g7P9V8u+e8JQdUsl2RWg+GCsoee0mtJjZUCCxn/vb3jzOwDKm6hAdwddHCVBfWJ58eGUBZz3IvnU8rRktjw==` |
| `@rc-component/picker` | `1.11.0` | transitive prod | `MIT` | `sha512-6qXGKtoJvO8sUd17m5cyNEbEJub0zflCHnaZTBBmj63DPRZYc0WEHN8rp6hFSl+yMCJS/dJY5G+1fQ8bLCuD7A==` |
| `@rc-component/portal` | `2.2.1` | transitive prod | `MIT` | `sha512-ck+r1kW/JSv0wxPji3KN2ss9K6Z0qqwusw/mf/0JobXhZ8hC2ejZwCJObW/SvDi0uhA0VzmCnx0CaCci95tcmA==` |
| `@rc-component/progress` | `1.0.2` | transitive prod | `MIT` | `sha512-WZUnH9eGxH1+xodZKqdrHke59uyGZSWgj5HBM5Kwk5BrTMuAORO7VJ2IP5Qbm9aH3n9x3IcesqHHR0NWPBC7fQ==` |
| `@rc-component/qrcode` | `2.0.0` | transitive prod | `MIT` | `sha512-aAv3QhPP1xyafuTZOxub6a54pCeBnN3IwQkpETrBtthq4BL5IgxnCbuoBWPDpdLw1y1j6BgBUCAKV92+yX06Dw==` |
| `@rc-component/rate` | `1.0.1` | transitive prod | `MIT` | `sha512-bkXxeBqDpl5IOC7yL7GcSYjQx9G8H+6kLYQnNZWeBYq2OYIv1MONd6mqKTjnnJYpV0cQIU2z3atdW0j1kttpTw==` |
| `@rc-component/resize-observer` | `1.1.2` | transitive prod | `MIT` | `sha512-t/Bb0W8uvL4PYKAB3YcChC+DlHh0Wt5kM7q/J+0qpVEUMLe7Hk5zuvc9km0hMnTFPSx5Z7Wu/fzCLN6erVLE8Q==` |
| `@rc-component/segmented` | `1.3.0` | transitive prod | `MIT` | `sha512-5J/bJ01mbDnoA6P/FW8SxUvKn+OgUSTZJPzCNnTBntG50tzoP7DydGhqxp7ggZXZls7me3mc2EQDXakU3iTVFg==` |
| `@rc-component/select` | `1.8.2` | transitive prod | `MIT` | `sha512-HQ9zuYqjfZTlcEMWlU1GAPBajd2OHIMVHyjZSGVTCVARwkfCgvXZMTEn0cduy3L+ejAKkaZluOQvxovZoaJaQw==` |
| `@rc-component/slider` | `1.1.1` | transitive prod | `MIT` | `sha512-LSzgWGYDgeCDgR4r1XlU29gbYws6HpLnvJd/uMhLeW/vQgxldeR+Wb4uzHDCHiYEbr1bnEHWdjkPxjJRHxuiig==` |
| `@rc-component/steps` | `1.2.2` | transitive prod | `MIT` | `sha512-/yVIZ00gDYYPHSY0JP+M+s3ZvuXLu2f9rEjQqiUDs7EcYsUYrpJ/1bLj9aI9R7MBR3fu/NGh6RM9u2qGfqp+Nw==` |
| `@rc-component/switch` | `1.0.3` | transitive prod | `MIT` | `sha512-Jgi+EbOBquje/XNdofr7xbJQZPYJP+BlPfR0h+WN4zFkdtB2EWqEfvkXJWeipflwjWip0/17rNbxEAqs8hVHfw==` |
| `@rc-component/table` | `1.10.4` | transitive prod | `MIT` | `sha512-HwoTnrwc29zeoXkXGhWqzJh8FIibGUxi1jM4LtoSzmR9d5Vv5osUQpZxnXKBP8iOCvyD6BQzZm1nXJRcnrxpAg==` |
| `@rc-component/tabs` | `1.11.0` | transitive prod | `MIT` | `sha512-hA/drZYOVa/MMIb4M2fWf3yaTyTG4qVuIABmghvEhyfw2nBob5VTH69lMCDjSVKmgODjO6nWlCV+gVn3xBrj5Q==` |
| `@rc-component/tooltip` | `1.4.0` | transitive prod | `MIT` | `sha512-8Rx5DCctIlLI4raR0I0xHjVTf1aF48+gKCNeAAo5bmF5VoR5YED+A/XEqzXv9KKqrJDRcd3Wndpxh2hyzrTtSg==` |
| `@rc-component/tour` | `2.4.0` | transitive prod | `MIT` | `sha512-aui4r4TqmTzwaBgcQxHYep8kM8PTjZFufjokObpy35KfFeZ0k9ArquWFZqegQlH24P14t+F0qO0mGTgzlav1yg==` |
| `@rc-component/tree` | `1.3.2` | transitive prod | `MIT` | `sha512-bJFj46wEkpBPnWyTm18XmgAgNQ/4YvprxMOPPY2a6rmhGJYxLuNKEFiL5Qej4Qctu9wHJm8WW+v2SYskafE0kA==` |
| `@rc-component/tree-select` | `1.11.0` | transitive prod | `MIT` | `sha512-EhS0X0wtUhBfK4S5TlpSY3MR9ndPMGgujtt1PJW3Ej+ToAlnS/6ohYURtCoXBYGqazUwHmgQGVUDsfpVwhWPkg==` |
| `@rc-component/trigger` | `3.10.1` | transitive prod | `MIT` | `sha512-mXlDN0IXdtV8Yqqm8195ECCyrbmfvvfKvwVvSlH0+qvKD6BUF8gRhEjSy0FOcD1+CcDRHgTiX99LoxfQrmh3Cw==` |
| `@rc-component/upload` | `1.1.1` | transitive prod | `MIT` | `sha512-GvYWSKeaJTOxxC5p6+nOSadzfvXA1h8C/iHFPFZX+szH3JUXrvs+DLiW8YUTBgvMh8m63mJeHrlYlJzAlg+pDA==` |
| `@rc-component/util` | `1.12.0` | transitive prod | `MIT` | `sha512-AEjPL8JVdohIITaiXokyjL9WQ6tKWWjAYK9QU16tGNE9JaQABBQy+hA4H2Lup5MgXy9yY3iLrbZJheuU13hTdQ==` |
| `@rc-component/virtual-list` | `1.4.0` | transitive prod | `MIT` | `sha512-qoyNStkTJQDezPjBibGA5HNxS9NiKJvemD1bLp7qfyxDlwy7ofPLUP0ZqJ47hR8AKcFaizd0AP/7QWLTLpudKQ==` |
| `@rc-component/virtual-list/node_modules/@babel/runtime` | `8.0.0` | transitive prod | `MIT` | `sha512-sL6cvO2IfkSu/iU+zs2S/w01B7A8V7suXSIKEN4hPFFdZoiPGxrj5pAG0lCaqLWiEIrjKzdznIWuaLcxPR53qw==` |
| `@types/react` | `19.2.17` | direct dev | `MIT` | `sha512-MXfmqaVPEVgkBT/aY0aGCkRWWtByiYQXo3xdQ8r5RzuFrPiRn8Gar2tQdXSUQ2GKV3bkXckek89V8wQBY2Q/Aw==` |
| `@types/react-dom` | `19.2.3` | direct dev | `MIT` | `sha512-jp2L/eY6fn+KgVVQAOqYItbF0VY/YApe5Mz2F0aykSO8gx31bYCZyvSeYxCHKvzHG5eZjc+zyaS5BrBWya2+kQ==` |
| `antd` | `6.5.1` | direct prod | `MIT` | `sha512-VZVVF9zYI6S0NHqboVhCoY9Iiqj6dphW1NPB+sEaAf2HuIQ0haXWXj7ZvAXTRDzusktV6+cvvrSZEdRi4twATg==` |
| `clsx` | `2.1.1` | transitive prod | `MIT` | `sha512-eYm0QWBtUrBWZWG0d386OGAw16Z995PiOVo2B7bjWSbHedGl5e0ZWaq65kOGgUSNesEIDkB9ISbTg/JK9dhCZA==` |
| `compute-scroll-into-view` | `3.1.1` | transitive prod | `MIT` | `sha512-VRhuHOLoKYOy4UbilLbUzbYg93XLjv2PncJC50EuTWPA3gaja1UjBsUP/D/9/juV3vQFr6XBEzn9KCAHdUvOHw==` |
| `csstype` | `3.2.3` | transitive prod | `MIT` | `sha512-z1HGKcYy2xA8AGQfwrn0PAy+PB7X/GSj3UVJW9qKyn43xWa+gl5nXmU4qqLMRzWVLFC8KusUX8T/0kCiOYpAIQ==` |
| `dayjs` | `1.11.21` | transitive prod | `MIT` | `sha512-98IT+HOahAisibz/yjKbzuOBwYcjJ7BCLPzARyHiyEBmRz4fatF+KPJszEHXsGYjUG234aH/cOjW1wwTbKUZlA==` |
| `is-mobile` | `5.0.0` | transitive prod | `MIT` | `sha512-Tz/yndySvLAEXh+Uk8liFCxOwVH6YutuR74utvOcu7I9Di+DwM0mtdPVZNaVvvBUM2OXxne/NhOs1zAO7riusQ==` |
| `json2mq` | `0.2.0` | transitive prod | `MIT` | `sha512-SzoRg7ux5DWTII9J2qkrZrqV1gt+rTaoufMxEzXbS26Uid0NwaJd123HcoB80TgubEppxxIGdNxCx50fEoEWQA==` |
| `react` | `19.2.7` | direct prod | `MIT` | `sha512-HNe9WslTbXmFK8o8cmwgAeJFSBvt1bPdHCVKtaaV+WlAN36mpT4hcRpwbf3fY56ar2oIXzsBpOAiIRHAdY0OlQ==` |
| `react-dom` | `19.2.7` | direct prod | `MIT` | `sha512-t0BRVXvbiE/o20Hfw669rLbMCDWtYZLvmJigy2f0MxsXF+71pxhR3xOkspmsO8h3ZlNzyibAmtCa3l4lYKk6gQ==` |
| `react-is` | `19.2.7` | transitive prod | `MIT` | `sha512-kZFnouyVv7eP/Phmrlo9FK+zcAdriZJvzxXHF1Sl1P377WSGe2G/JxVolhTrB/jeV47lKImhNUsijjHAAbcl/A==` |
| `scheduler` | `0.27.0` | transitive prod | `MIT` | `sha512-eNv+WrVbKu1f3vbYJT/xtiF5syA5HPIMtf9IgY/nKg0sWqzAUEvqY/xm7OcZc/qafLx/iO9FgOmeSAp4v5ti/Q==` |
| `scroll-into-view-if-needed` | `3.1.0` | transitive prod | `MIT` | `sha512-49oNpRjWRvnU8NyGVmUaYG4jtTkNonFZI86MmGRDqBphEK2EXT9gdEUoQPZhuBM8yWHxCWbobltqYO5M4XrUvQ==` |
| `string-convert` | `0.2.1` | transitive prod | `MIT` | `sha512-u/1tdPl4yQnPBjnVrmdLo9gtuLvELKsAoRapekWggdiQNvvvum+jYF329d84NAa660KQw7pB2n36KrIKVoXa3A==` |
| `stylis` | `4.4.0` | transitive prod | `MIT` | `sha512-5Z9ZpRzfuH6l/UAvCPAPUo3665Nk2wLaZU3x+TLHKVzIz33+sbJqbtrYoC3KD4/uVOr2Zp+L0LySezP9OHV9yA==` |
| `throttle-debounce` | `5.0.2` | transitive prod | `MIT` | `sha512-B71/4oyj61iNH0KeCamLuE2rmKuTO5byTOSVwECM5FA7TiAiAW+UqTKZ9ERueC4qvgSttUhdmq1mXC3kJqGX7A==` |
