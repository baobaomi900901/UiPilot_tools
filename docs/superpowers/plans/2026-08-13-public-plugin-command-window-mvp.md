# Public Plugin Command and Singleton Window MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved public plugin MVP without changing `/math` or turning `/find` into a plugin: users can install a validated public package, route one configurable command to it, receive either one host-rendered result list or one host-managed singleton window, and execute at most one host-owned copy action per result.

**Architecture:** Keep `PluginManager` as the integration facade, but move new public contracts into focused `public_plugins` modules instead of further expanding the existing `src-tauri/src/plugins.rs`. Rust owns package staging, compatibility, durable state, request identity, scheduling, result authorization, native windows, permissions, and rollback. TypeScript owns launcher presentation and the host window shell; plugin JavaScript receives only the frozen per-request API and isolated window update bridge defined by the approved specification.

**Tech Stack:** Rust 1.96, Tauri 2.11, Windows APIs through `windows` 0.61, WebView2 through `webview2-com` 0.38.2, TypeScript 7, React 19, Ant Design 6, Vitest 4, jsdom 29. Add a Rust ZIP reader with only required archive features and Tauri dialog 2 for user-selected `.uipilot-plugin` files; use Windows DPAPI through the existing `windows` crate for MVP secret storage.

## Binding Specification

[UiPilot public plugin command and singleton window MVP design](../specs/2026-08-13-public-plugin-command-window-mvp-design.md) is the source of truth. In particular:

- Sections 5-9 define parsing, activation/output modes, manifest, settings schema, `onCommand`, per-request API identity, DTOs, and limits.
- Sections 10-11 define launcher ownership, main results, global native handoff, singleton window shell, pin, close, drag, position, and theme.
- Sections 12-14 define permissions, atomic install/update, scheduling, failure accounting, isolation, and resource limits.
- Sections 15-18 define `/demo`, tests, non-goals, and completion criteria.

## Global Constraints

- Preserve all existing user changes. Every task stages and commits only its own files; never clean or reset the current dirty worktree.
- Do not modify the `/math` package or behavior. `/find` remains system-reserved and retains its existing user-visible behavior.
- Public plugins have exactly one effective command and at most one visible window. `outputMode` is static package metadata.
- Package candidates exist only in staging until static validation, permission confirmation, and isolated Runtime `ready` all succeed. Failed updates leave the current generation untouched.
- Public resources are exactly root `plugin.json` plus lowercase `.html`, `.js`, and `.css` ordinary files with fixed MIME types. No MIME sniffing, extra JSON, images, fonts, media, source maps, double extensions, links, or reparse points.
- Each dispatched handler receives a fresh deeply frozen `PluginInvocation` and `UiPilotPluginApiV1`, bound to `(pluginId, pluginGeneration, requestId)`. Rust revalidates at command entry and before reads or state commits.
- Each plugin generation has at most one running request and one latest waiting candidate. Request timeout starts at actual dispatch; normal supersession is not a plugin fault.
- Each main result has zero or one `copyText` default action. Real action payloads never cross into the launcher frontend.
- `/find` and plugin windows share one host-wide `MainWindowTransferCoordinator`. No lock spans native window calls, event emission, shell/clipboard side effects, WebView acknowledgement, or async waiting.
- A plugin window is never always-on-top. Pin only disables automatic hide for the current process.
- Limits are fixed at 64 KiB per response, 5 MiB private JSON storage, 20 main results, 5 seconds for `live`, 30 seconds for `submit`, and 5 seconds for visible-window ready/update acknowledgement.
- Runtime and content WebViews cannot use Node, Electron, generic Tauri APIs, network, arbitrary files, native binaries, Shell, or input synthesis.
- Automated tests never control the user's mouse or keyboard. Ask for explicit permission immediately before any harness that changes foreground focus.

## Estimation Model

`AI coding` is active test/code/document editing in an already prepared workspace with dependencies cached. `Checkpoint` includes that work plus focused compilation and tests. It excludes user response time, network/package-registry outages, Windows focus-policy failures, and unrelated dirty-worktree conflicts.

| Task | AI coding | Through focused checkpoint |
|---|---:|---:|
| 1. Public package intake and staging | 2.5-4 h | 4-6.25 h |
| 2. Durable names, settings, storage, and secrets | 3.5-5.5 h | 5.75-9 h |
| 3. Runtime API, scheduler, and atomic activation | 4.5-7.25 h | 7.25-11.5 h |
| 4. Main results and plugin management UI | 3.5-5.5 h | 5.5-8.75 h |
| 5. Singleton plugin window and global handoff | 5.25-8.5 h | 8.5-13.75 h |
| 6. `/demo`, SDK artifacts, and acceptance | 2.25-3.75 h | 4.75-8.5 h including user acceptance |
| **Total** | **21.5-34.5 h** | **36-58 h including user acceptance** |

The largest uncertainty is Task 5 because it combines WebView2 isolation with real native focus behavior. The first reliable implementation checkpoint is Task 1, not the total estimate.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6`. The shared files and security contracts make serial execution safer than parallel edits.
- Every task uses TDD: add focused failing tests, confirm the intended failure, implement the smallest contract, rerun focused tests, then create one atomic commit. Review fixes may be separate commits.
- Use table-driven tests for shared validation matrices. Keep async ordering, caller authorization, rollback, stale ownership, and native focus sequences as separately named tests.
- Each task performs a specification-compliance self-check and its listed automated verification before the dependent task begins. Do not add independent review rounds unless the user asks.
- Generated Tauri permissions are committed with the command that requires them. Capabilities must use non-overlapping exact label families.
- Existing internal plugin packages continue through their compatibility loader and resource rules; public `schemaVersion: 1` rules do not apply retroactively.

---

### Task 1: Public Package Intake and Staging

**Files:** create `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/package.rs`; modify `src-tauri/src/plugins.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.

**Dependencies:** approved design sections `7.1 包结构`, `7.3 清单校验`, `7.4 API 兼容`, `12.1 第一阶段可用能力`, `12.2 已保留但未开放的权限`, and `13.3 安装、升级、禁用和卸载`.

**Produces:** `PublicManifestV1`, `PublicPackageSource::{Archive, DevelopmentDirectory}`, `PreparedPublicPlugin`, `PublicPackageError`, and `stage_public_package(source, staging_root, host) -> Result<PreparedPublicPlugin, PublicPackageError>`. A prepared candidate is not routable and owns cleanup of its staging directory until Task 3 atomically promotes it.

- [ ] **Step 1: Add the strict public manifest model and compatibility matrix.** Parse exact `schemaVersion: 1` keys, one command, `live | submit`, `mainResult | window`, settings definitions, platform/API versions, permissions, and all field limits. Keep legacy `manifest: 1` dispatch unchanged. **Estimate:** AI coding 35-55 min; checkpoint 55-85 min.
- [ ] **Step 2: Add bounded ZIP extraction and development-directory snapshots.** Copy either source into a fresh transaction staging directory, then apply the identical validator. Reject duplicate canonical paths, traversal, absolute paths, ADS, encrypted entries, reparse points, symlinks, double extensions, count/depth/size overflow, and every resource outside `plugin.json/.html/.js/.css`. Never execute directly from the chosen archive or development directory. **Estimate:** AI coding 45-75 min; checkpoint 75-120 min.
- [ ] **Step 3: Freeze the staged snapshot and fixed MIME resolver.** Hash and revalidate ordinary files, map only the approved MIME types, forbid sniffing and `data:`/remote CSS resources, and preserve the old internal asset resolver unchanged. **Estimate:** AI coding 35-55 min; checkpoint 55-90 min.
- [ ] **Step 4: Add cleanup and hostile-package regression matrices.** Prove every static failure removes staging, creates no plugin record/route/name reservation, and returns a stable `InvalidPackage`, `IncompatiblePlatform`, `IncompatibleApi`, or `UnsupportedPermission` category. **Estimate:** AI coding 30-45 min; checkpoint 50-75 min.

**Distinct test coverage:** valid public window/main-result packages; public-vs-legacy discriminator; incompatible platform/API; unsupported reserved permission; malformed setting definitions; ZIP path collision and case folding; double extension; fixed MIME; failed staging cleanup; unchanged legacy asset behavior.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::tests plugins::tests::package_state -- --nocapture`

### Task 2: Durable Names, Settings, Storage, and Secrets

**Files:** create `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/secrets.rs`; modify `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/atomic_file.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.

**Dependencies:** Task 1; design sections `8 插件设置 Schema`, `12.1 第一阶段可用能力`, `12.3 禁止能力`, `13.1 设置页`, `13.2 启动名称`, and `13.3 安装、升级、禁用和卸载`.

**Produces:** `PluginStateStore`, `EffectivePluginConfig`, `PluginStorageStore`, and `PluginSecretStore`. Reads return immutable snapshots; writes are plugin-scoped, atomic, quota-checked, and generation-independent where the specification requires data to survive upgrades.

- [ ] **Step 1: Persist effective names and lifecycle state.** Store one effective name, user-disabled/automatic-fault state, permission grants, and inventory revision; enforce global case-folded command uniqueness against installed plugins and system-reserved names. **Estimate:** AI coding 40-65 min; checkpoint 65-100 min.
- [ ] **Step 2: Persist and validate host-rendered settings.** Apply manifest defaults, retain values by stable key across compatible upgrades, reject type/range/option violations atomically, and never persist secret defaults. **Estimate:** AI coding 45-75 min; checkpoint 75-120 min.
- [ ] **Step 3: Implement the 5 MiB private JSON store.** Reject non-finite numbers, prototype-special keys, quota overflow, and cross-plugin access; `set/remove` must leave the old document intact on any failure. **Estimate:** AI coding 40-65 min; checkpoint 65-105 min.
- [ ] **Step 4: Implement Windows DPAPI secret writes and presence checks.** Bind encrypted records to plugin ID and setting key, never return plaintext to Runtime, preserve secrets across upgrades, and honor uninstall delete/retain choice. Non-Windows builds expose an unsupported backend without weakening the public contract. **Estimate:** AI coding 45-80 min; checkpoint 80-130 min.
- [ ] **Step 5: Add migration, corruption quarantine, and uninstall/retain tests.** A corrupt plugin-state document must not expose another plugin's data or block healthy siblings. **Estimate:** AI coding 30-45 min; checkpoint 50-80 min.

**Distinct test coverage:** effective-name collisions and atomic rename; disabled name remains reserved; settings schema evolution; independent plugin data; quota rollback; DPAPI round trip without logged plaintext; secret unreadability; uninstall delete/retain; corrupt-document quarantine.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::state::tests public_plugins::storage::tests public_plugins::secrets::tests -- --nocapture`

### Task 3: Runtime API, Latest-Only Scheduler, and Atomic Activation

**Files:** create `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/scheduler.rs`; modify `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/plugin-runtime.json`, `src-tauri/capabilities/main.json`; create generated permissions for `plugin_api_call`, `complete_plugin_command`, `prepare_public_plugin_install`, `commit_public_plugin_install`, `cancel_public_plugin_install`, `set_plugin_enabled`, `set_plugin_effective_name`, `save_plugin_settings`, and `uninstall_plugin`.

**Dependencies:** Tasks 1-2; design sections `9.1 Runtime 装载`, `9.2 调用 DTO`, `9.3 Runtime 宿主 API`, `10.1 路由和请求所有权`, `14.1 请求时效`, and `14.2 故障隔离`.

**Produces:** `PluginRequestScheduler`, `PluginRequestContext`, `PluginApiOperation`, and atomic manager operations for package install/update/reload/enable/disable/uninstall. Hidden Runtime labels become `plugin-runtime-*`; visible-window labels reserved by Task 5 use a non-overlapping family.

- [ ] **Step 1: Replace the public Runtime bootstrap with `onCommand(invocation, api)`.** Create a fresh deeply frozen invocation/API facade per dispatch, derive plugin/generation from the exact caller label, attach only request ID in the bridge, remove `api.resolve`, and keep the legacy bridge only for internal packages. **Estimate:** AI coding 60-95 min; checkpoint 95-150 min.
- [ ] **Step 2: Implement guarded API operations.** Route `storage.get/set/remove`, `settings.get`, and `settings.isSecretConfigured` through one narrow tagged command; reject malformed/forged context as `InvalidContext` and stale context as `ExpiredRequestError`, rechecking before state commit. **Estimate:** AI coding 45-75 min; checkpoint 75-120 min.
- [ ] **Step 3: Implement one-running/one-latest scheduling.** Allocate `requestId` only at dispatch, expire A when B arrives, replace waiting B with C, start timeout at dispatch, and isolate schedulers by plugin generation. **Estimate:** AI coding 55-90 min; checkpoint 90-145 min.
- [ ] **Step 4: Implement timeout, Runtime replacement, and fault accounting.** Destroy a timed-out Runtime, increment generation, dispatch only the still-valid latest candidate to the same installed package, discard candidates on upgrade/disable/uninstall, and exclude normal supersession from fault counts. Persist automatic disable only after three current-owner faults within five minutes; redact inputs, returned text, secrets, authorization data, and real paths from logs. **Estimate:** AI coding 45-75 min; checkpoint 75-120 min.
- [ ] **Step 5: Complete two-phase atomic activation and management commands.** `prepare_public_plugin_install(path)` stages and validates the user-selected archive and returns only a one-use token plus safe manifest, unsigned-source, permission, version, and conflict summary. After explicit UI confirmation, `commit_public_plugin_install(token)` starts the isolated Runtime, waits for `ready`, then atomically commits package, state, route, name, and generation; `cancel_public_plugin_install(token)` removes staging. Tokens expire and are main-caller-bound. Failure cleans staging; update/reload failure keeps the old generation. Existing development install/reload enters the same preparation path without a file picker. **Estimate:** AI coding 55-90 min; checkpoint 90-150 min.

**Distinct test coverage:** frozen per-request API; wrong label before state access; forged vs expired context; state invalidated between entry and commit; A-running/B-waiting/C-replaces-B ordering; dispatch-based 5/30-second timeout; normal cancellation not counted; Runtime restart; install cancellation; ready timeout cleanup; update rollback; no overlapping runtime/window capabilities.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::runtime::tests public_plugins::scheduler::tests plugins::tests::query commands::tests -- --nocapture`

### Task 4: Main Results and Plugin Management UI

**Files:** modify `src-tauri/src/plugins.rs`, `src-tauri/src/result_registry.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/styles.css`, `src-tauri/capabilities/main.json`, `package.json`, `package-lock.json`; add `tauri-plugin-dialog` 2, `@tauri-apps/plugin-dialog` 2, and only the main-window open-dialog permission.

**Dependencies:** Tasks 1-3; design sections `5.1 命令发现与解析`, `5.2 激活模式`, `9.4 主窗口结果 DTO`, `10.2 mainResult`, `13.1 设置页`, and `18 完成标准` items 3-4.

**Produces:** exact public inventory/settings DTO parsers, `live/submit` routing through effective names, host-rendered pure-text results, and one current `copyText` default action bound to plugin/generation/request/result.

- [ ] **Step 1: Route effective public commands without changing ordinary app search.** Implement `/`, prefix discovery, full-command hint, input-required behavior, ASCII-space body parsing, 150 ms `live` debounce, and first-Enter `submit`. **Estimate:** AI coding 35-60 min; checkpoint 60-95 min.
- [ ] **Step 2: Validate and register `MainResultResponse`.** Enforce exact keys, 0-20 items, text and 64 KiB limits, zero/one `copyText`, reject `actions[]`, and retain real action payload only in Rust under `pluginId + generation + requestId + resultId`. **Estimate:** AI coding 40-65 min; checkpoint 65-100 min.
- [ ] **Step 3: Add launcher ownership and result interaction.** Only the current view epoch/control/value/submission token may publish, clear, or show errors; first Enter submits, second Enter or row click executes the current default action, and no-action rows remain display-only. **Estimate:** AI coding 45-75 min; checkpoint 75-120 min.
- [ ] **Step 4: Complete the plugin settings UI.** Register `tauri-plugin-dialog` in Rust and grant only main `dialog:allow-open`. A user click opens a single-file `.uipilot-plugin` picker; the UI calls prepare, displays unsigned-source status, exact permissions, version/change summary, and conflicts, then explicitly commits or cancels. Add development install/reload, enable/disable, rename/reset, uninstall with delete/retain choice, and host-rendered bool/text/number/select/secret controls whose secret value is never read back. Do not expose `outputMode` as a setting. **Estimate:** AI coding 50-80 min; checkpoint 80-130 min.
- [ ] **Step 5: Preserve action safety and regressions.** Copy rechecks the full binding and permission immediately before clipboard write; success follows existing clear/hide behavior, failure preserves current results. `/math`, `/find`, settings, and application results remain unchanged. **Estimate:** AI coding 30-45 min; checkpoint 50-80 min.

**Distinct test coverage:** command discovery and preserved internal spaces; live debounce; submit first/second Enter; late A success/failure after edited B; response and action validation; click/Enter equivalence; stale result cannot copy; management operation errors stay row-local; dynamic settings validation; `/math` and `/find` regression.

**Verify:** `npm.cmd test -- src/launcher.test.tsx` then `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests commands::tests::execute_plugin plugins::tests::query -- --nocapture` and `npm.cmd run build`

### Task 5: Singleton Plugin Window and Global Native Handoff

**Files:** create `src-tauri/src/window_transfer.rs`, `src-tauri/src/plugin_window.rs`, `src/plugin-window-core.ts`, `src/plugin-window-core.test.ts`, `src/plugin-window-view.tsx`, `src/plugin-window-view.test.tsx`, `src-tauri/capabilities/plugin-window-shell.json`, `src-tauri/capabilities/plugin-window-content.json`; modify `src-tauri/src/find_window.rs`, `src-tauri/src/lifecycle.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src-tauri/tauri.conf.json`, `src/main.ts`, `src/protocol.ts`, `src/styles.css`, `src-tauri/build.rs`; add generated plugin-window command permissions.

**Dependencies:** Tasks 1-4; design sections `10.3 window`, `11 插件窗口合同`, `12 权限与安全边界`, `16.3 插件窗口`, and `18 完成标准` items 5-6.

**Produces:** one host-wide `MainWindowTransferCoordinator`, one `PluginWindowController` per plugin ID, and one host-owned singleton native window per plugin. Exact label families are `plugin-runtime-*` for hidden handlers, `plugin-shell-*` for host chrome, and `plugin-content-*` for isolated content; capabilities must not overlap.

- [ ] **Step 1: Extract the shared native transfer lease and adapt `/find`.** Preserve `/find` semantics while moving serial native side effects, focus snapshots, main topmost downgrade/restore, expected blur consumption, timeout, and CAS cleanup into `MainWindowTransferCoordinator`. **Estimate:** AI coding 60-100 min; checkpoint 100-165 min.
- [ ] **Step 2: Add plugin-window admission and update ownership.** Bind every transaction to UI intent epoch, submission token, plugin ID, generation, request ID, and target window; recheck after each await and before show/focus/clear/hide; support latest update, ready/ack timeout, pin, forced close, and stale rollback. **Estimate:** AI coding 60-100 min; checkpoint 100-165 min.
- [ ] **Step 3: Build the isolated native window.** Use a host-owned shell WebView and a separately labelled plugin content WebView/custom source so content cannot access shell DOM or generic Tauri APIs. Keep runtime, shell, and content capability patterns non-overlapping; deny navigation, downloads, new windows, and unapproved resources. **Estimate:** AI coding 80-130 min; checkpoint 130-210 min.
- [ ] **Step 4: Build the host shell UI.** Route shell startup by exact label, render fixed-size icon-only pin/close controls with tooltips and accessible names, provide drag region, apply host theme tokens before content update, persist position by plugin ID, and keep pin process-local/non-topmost. **Estimate:** AI coding 55-90 min; checkpoint 90-145 min.
- [ ] **Step 5: Add ordered failure and cross-window tests.** Cover edit-during-ready, repeated submission, `/find` vs plugin serialization, stale show/focus/clear, conditional native cleanup, focus refusal, close/pin blur, update ack failure, display topology change, upgrade/disable/uninstall destruction, and no lock across native/emit/ack work. **Estimate:** AI coding 50-80 min; checkpoint 80-130 min.

**Distinct test coverage:** `/find` retains existing transfer behavior; `/demo A -> user edit -> A ready` is inert; `/demo A -> /find B` serializes to B; stale rollback cannot overwrite B; current failure restores captured state; singleton reuse; fixed is not topmost; position correction; exact capability caller guards.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml window_transfer::tests plugin_window::tests find_window::tests lifecycle::tests commands::tests -- --nocapture`, then `npm.cmd test -- src/plugin-window-core.test.ts src/plugin-window-view.test.tsx src/find-core.test.ts src/find-view.test.tsx` and `npm.cmd run build`

### Task 6: `/demo`, SDK Artifacts, and Acceptance

**Files:** create `examples/public-plugins/com.uipilot.demo/package/plugin.json`, `examples/public-plugins/com.uipilot.demo/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.demo/package/dist/window.html`, `examples/public-plugins/com.uipilot.demo/package/dist/window.js`, `examples/public-plugins/com.uipilot.demo/package/dist/window.css`, `examples/public-plugins/com.uipilot.demo/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.demo/README.md`, `scripts/package-demo-plugin.ps1`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `src-tauri/tests/public_plugin_window_events.rs`; modify `package.json` and `package-lock.json` to add a pinned JSON Schema test dependency, plus only wiring/tests discovered necessary during integration.

**Dependencies:** Tasks 1-5; design sections `15 /demo 参考插件`, `16 测试`, `17 非目标`, and `18 完成标准`.

**Produces:** an independently removable `/demo` package, public schema/types/developer guide, complete automated gates, and a user-operated acceptance script.

- [ ] **Step 1: Implement `/demo` as a standalone public package.** Keep README/tests outside the strict package root. Default to `submit + window`; `onCommand` returns `str yyyy-mm-dd`; the content page displays input, platform, current theme, instance `1`, and return text using only HTML/JS/CSS. Add a packaging script that includes only the contents of `package/` with `plugin.json` at archive root. No `/demo` literal or fallback enters host source. **Estimate:** AI coding 35-55 min; checkpoint 55-90 min.
- [ ] **Step 2: Verify the static `mainResult` variant.** Test the same package configuration with `submit + mainResult`, one pure-text result, and one `copyText` default action; the mode changes only after reload. **Estimate:** AI coding 20-35 min; checkpoint 35-60 min.
- [ ] **Step 3: Publish SDK artifacts from the implemented contract.** Document package layout, manifest schema, `onCommand`, frozen API, errors, limits, permissions, examples, and unsupported background/multi-action capabilities. Validate the example manifest against the checked-in JSON Schema. **Estimate:** AI coding 30-50 min; checkpoint 50-80 min.
- [ ] **Step 4: Run all non-interactive quality gates and fix only in-scope failures.** Include a process-failure isolation probe proving one public Runtime/window can be reclaimed without blocking the launcher, other plugins, `/math`, or `/find`. If the WebView2 topology cannot prove that isolation, mark public plugin release No-Go and stop rather than weakening the boundary. Do not absorb unrelated dirty-worktree failures; report them separately. **Estimate:** AI coding 30-60 min; checkpoint 90-180 min.
- [ ] **Step 5: Run real-window and manual acceptance only after permission.** First notify the user that native APIs will briefly change foreground focus but will not control mouse or keyboard. Treat an unavailable interactive-session precondition as inconclusive, not pass. The user alone types `/demo str`, tests singleton reuse/pin/close/drag/position/theme, switches mode/reloads, and confirms second-Enter copy. **Estimate:** AI preparation 10-20 min; automated focus harness 15-30 min; user acceptance 20-40 min.

**Distinct test coverage:** no host `/demo` hardcoding; package removal removes command; five window fields; static mode switch; 64 KiB and permission failures; runtime crash isolation; normal-permission dev launch; `/math` and `/find` full regression; real native transaction behavior without input synthesis.

**Verify:**

```powershell
npm.cmd test
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
node --test examples/public-plugins/com.uipilot.demo/tests/runtime.test.js
```

After explicit user approval only:

```powershell
$env:UIPILOT_RUN_REAL_WINDOW_TESTS='1'
cargo test --manifest-path src-tauri/Cargo.toml --test public_plugin_window_events -- --nocapture
```

## Final Checklist

- [ ] Every approved specification requirement maps to one task and one owning test suite.
- [ ] Public and internal manifest/resource paths remain explicitly separated.
- [ ] Installation cannot produce an incompatible or not-ready installed record; update failure leaves the old generation usable.
- [ ] Plugin API identity is immutable and revalidated before every state commit; expired completions cannot affect newer UI or data.
- [ ] Scheduler cardinality is one running plus one latest waiting per plugin generation.
- [ ] Main results expose at most one host-owned copy action and no real action payload to TypeScript.
- [ ] `/find` and plugin windows use the same native transfer coordinator without changing `/find` behavior.
- [ ] Runtime, shell, and content WebViews have non-overlapping labels and least-privilege capabilities; failed process-isolation proof is an explicit public-release No-Go.
- [ ] No test or implementation synthesizes mouse or keyboard input.
- [ ] Existing unrelated worktree changes remain unstaged and unmodified.
