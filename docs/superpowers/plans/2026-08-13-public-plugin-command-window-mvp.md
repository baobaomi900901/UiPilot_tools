# Public Plugin Command and Singleton Window MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved public plugin MVP without changing `/math` or turning `/find` into a plugin: users can install a validated public package, route one configurable command to it, receive either one host-rendered result list or one host-managed singleton window, and execute at most one host-owned copy action per result.

**Architecture:** Keep `PluginManager` as the integration facade, but move new public contracts into focused `public_plugins` modules instead of further expanding the existing `src-tauri/src/plugins.rs`. Rust owns package staging, compatibility, durable state, request identity, scheduling, result authorization, native windows, permissions, and rollback. TypeScript owns launcher presentation and the host window shell; plugin JavaScript receives only the frozen per-request API and isolated window update bridge defined by the approved specification.

**Tech Stack:** Rust 1.96, Tauri 2.11, Windows APIs through `windows` 0.61, WebView2 through `webview2-com` 0.38.2, TypeScript 7, React 19, Ant Design 6, Vitest 4, and jsdom 29. Add a Rust ZIP reader with only required archive features, Tauri dialog 2 for user-selected `.uipilot-plugin` files, and `schemars` for manifest/data DTO JSON Schema generation; use Windows DPAPI through the existing `windows` crate for MVP secret storage. The function-bearing TypeScript API declaration remains a compact checked-in contract because `schemars` does not generate TypeScript function interfaces.

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

`AI coding` is active code, test, and document editing in an already prepared workspace with dependencies cached. `Checkpoint` includes that work plus the task's focused compilation and core-risk tests. It excludes user response time, network/package-registry outages, Windows focus-policy failures, and unrelated dirty-worktree conflicts.

| Task | AI coding | Through focused checkpoint |
|---|---:|---:|
| 1. Public package intake and staging | 1.5-2.5 h | 2.5-4 h |
| 2. Durable names, settings, storage, and secrets | 2-3.5 h | 3.5-5.5 h |
| 3. Runtime API, scheduler, and atomic activation | 3.5-5.5 h | 5.5-8.5 h |
| 4. Main results and plugin management UI | 2.5-4 h | 4-6.5 h |
| 5. Singleton plugin window and global handoff | 4-6 h | 6-9.5 h |
| 6. `/demo`, SDK artifacts, and acceptance | 1.5-2.5 h | 2.5-4.5 h including user acceptance |
| **Total** | **15-24 h** | **24-39 h including user acceptance** |

Task 5 remains the largest uncertainty because WebView2 isolation and Windows focus policy are environment-sensitive. The first reliable estimate checkpoint is the completed Task 1, after package and build dependencies are known.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6`. Shared files and security contracts require serial task boundaries.
- Implement each task as one vertical slice, then add and run its listed core-risk tests before the checkpoint. The plan does not require a separate red-green-refactor ceremony for every subitem.
- Combine variants that share ordering and terminal behavior into table-driven tests. Keep atomic activation, latest-only scheduling, caller authorization, asynchronous ownership, and focus rollback explicit; do not create one test per equivalent error code or API method.
- Create one atomic commit after each task checkpoint. Run focused verification at task boundaries and the full regression suite once in Task 6. Do not add independent review rounds unless the user asks.
- Generated Tauri permissions are committed with the command that requires them. Capabilities must use non-overlapping exact label families.
- Existing internal plugin packages continue through their compatibility loader and resource rules; public `schemaVersion: 1` rules do not apply retroactively.
---

### Task 1: Public Package Intake and Staging

**Files:** create `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/package.rs`; modify `src-tauri/src/plugins.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.

**Dependencies:** approved design sections `7.1 包结构`, `7.3 清单校验`, `7.4 API 兼容`, `12.1 第一阶段可用能力`, `12.2 已保留但未开放的权限`, and `13.3 安装、升级、禁用和卸载`.

**Produces:** `PublicManifestV1`, `PublicPackageSource::{Archive, DevelopmentDirectory}`, `PreparedPublicPlugin`, `PublicPackageError`, and `stage_public_package(source, staging_root, host) -> Result<PreparedPublicPlugin, PublicPackageError>`. A prepared candidate is not routable and owns cleanup until Task 3 atomically promotes it.

- [ ] **Step 1: Implement strict intake and a frozen staging snapshot.** Parse exact `schemaVersion: 1` data, check platform/API/permission compatibility, and copy ZIP or development-directory input into a fresh transaction directory before validation. Reject traversal, absolute/ADS/link/reparse paths, canonical collisions, archive limits, double extensions, and resources outside root `plugin.json` plus lowercase `.html/.js/.css`. Resolve MIME only through the fixed extension table; do not add or call a MIME-sniffing library. **Estimate:** AI coding 55-90 min.
- [ ] **Step 2: Integrate preparation and atomic cleanup boundaries.** Hash and revalidate ordinary staged files, preserve the legacy internal-package loader, and ensure every rejection removes staging without creating a route, plugin record, or name reservation. **Estimate:** AI coding 20-35 min.
- [ ] **Step 3: Add three table-driven package test groups.** Cover accepted archive/directory packages, rejected resource/path variants, and incompatible or malformed candidates with cleanup and unchanged legacy behavior. **Estimate:** AI coding 15-25 min.

**Core test coverage:** (1) valid `window` and `mainResult` candidates; (2) illegal extension, double extension, traversal, case-folded collision, and fixed-MIME enforcement; (3) incompatible platform/API, unsupported permission, malformed settings, staging cleanup, and legacy-package isolation.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::tests plugins::tests::package_state -- --nocapture`

### Task 2: Durable Names, Settings, Storage, and Secrets

**Files:** create `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/secrets.rs`; modify `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/atomic_file.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.

**Dependencies:** Task 1; design sections `8 插件设置 Schema`, `12.1 第一阶段可用能力`, `12.3 禁止能力`, `13.1 设置页`, `13.2 启动名称`, and `13.3 安装、升级、禁用和卸载`.

**Produces:** `PluginStateStore`, `EffectivePluginConfig`, `PluginStorageStore`, and `PluginSecretStore`. State and private JSON writes use the existing atomic-file pattern; secret values use Windows DPAPI and are never returned to Runtime.

- [ ] **Step 1: Implement durable lifecycle, effective-name, and settings state.** Persist one globally unique case-folded command name, enabled/fault state, permission grants, inventory revision, and validated bool/text/number/select settings. Preserve compatible values across upgrades and never persist a secret default. **Estimate:** AI coding 35-60 min.
- [ ] **Step 2: Implement private storage and DPAPI secrets.** Enforce plugin scope and the 5 MiB JSON limit with atomic rollback; bind encrypted secret records to plugin ID and setting key, expose presence only, and support uninstall delete/retain behavior. Preserve position/settings/storage/secrets across compatible plugin upgrades. **Estimate:** AI coding 55-95 min.
- [ ] **Step 3: Add boundary and rollback tests.** Parameterize shared setting and lifecycle variants while retaining explicit assertions for atomic rename, quota rollback, cross-plugin denial, DPAPI round trip without logged/plaintext exposure, and uninstall retention. **Estimate:** AI coding 30-55 min.

**Core test coverage:** effective-name collision and atomic rename; min/max/enum setting validation; independent plugin stores and quota rollback; one shared caller-scope guard; DPAPI write/presence/unreadability; uninstall delete/retain; corrupt state quarantines only its owner.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::state::tests public_plugins::storage::tests public_plugins::secrets::tests -- --nocapture`

### Task 3: Runtime API, Latest-Only Scheduler, and Atomic Activation

**Files:** create `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/scheduler.rs`; modify `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/plugin-runtime.json`, `src-tauri/capabilities/main.json`; create generated permissions for `plugin_api_call`, `complete_plugin_command`, `prepare_public_plugin_install`, `commit_public_plugin_install`, `cancel_public_plugin_install`, `set_plugin_enabled`, `set_plugin_effective_name`, `save_plugin_settings`, and `uninstall_plugin`.

**Dependencies:** Tasks 1-2; design sections `9.1 Runtime 装载`, `9.2 调用 DTO`, `9.3 Runtime 宿主 API`, `10.1 路由和请求所有权`, `14.1 请求时效`, and `14.2 故障隔离`.

**Produces:** `PluginRequestScheduler`, `PluginRequestContext`, `PluginApiOperation`, and atomic install/update/reload/enable/disable/uninstall operations. Runtime labels use only `plugin-runtime-*`; Task 5 visible-window labels remain disjoint.

- [ ] **Step 1: Implement the public Runtime bootstrap and shared API guard.** Dispatch only `onCommand(invocation, api)`, create fresh deeply frozen per-request objects, bind the facade to `(pluginId, pluginGeneration, requestId)`, derive identity from the exact caller label, and revalidate through one Rust guard before reads or commits. Preserve the legacy internal bridge separately. **Estimate:** AI coding 70-105 min.
- [ ] **Step 2: Implement latest-only scheduling, timeout, and Runtime replacement.** Maintain one running plus one latest waiting candidate per plugin generation; allocate request IDs and start 5/30-second timeouts only at dispatch; expire A when B arrives, replace B with C, and rebuild/increment generation after timeout without counting normal supersession as a fault. **Estimate:** AI coding 65-100 min.
- [ ] **Step 3: Implement two-phase activation and management commands.** Prepare returns one caller-bound token and safe confirmation summary; commit waits for isolated Runtime `ready` before atomically changing package/state/route/name/generation; cancel and expired tokens remove staging. Update/reload failure leaves the current generation usable. **Estimate:** AI coding 50-80 min.
- [ ] **Step 4: Add the three core contract tests.** Use one guard table for valid/forged/expired context, one A/B/C scheduler test including timeout generation replacement, and one prepare/commit/cancel table including ready failure and update rollback. **Estimate:** AI coding 25-45 min.

**Core test coverage:** `InvalidContext` versus `ExpiredRequestError` at the shared guard; state invalidated between command entry and commit; A running/B waiting/C replaces B; dispatch-based timeout and generation rebuild; prepare/cancel/commit cleanup; failed update keeps the old Runtime and route; runtime/window capabilities do not overlap.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::runtime::tests public_plugins::scheduler::tests plugins::tests::query commands::tests -- --nocapture`

### Task 4: Main Results and Plugin Management UI

**Files:** modify `src-tauri/src/plugins.rs`, `src-tauri/src/result_registry.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/styles.css`, `src-tauri/capabilities/main.json`, `package.json`, `package-lock.json`; add `tauri-plugin-dialog` 2, `@tauri-apps/plugin-dialog` 2, and only the main-window open-dialog permission.

**Dependencies:** Tasks 1-3; design sections `5.1 命令发现与解析`, `5.2 激活模式`, `9.4 主窗口结果 DTO`, `10.2 mainResult`, `13.1 设置页`, and `18 完成标准` items 3-4.

**Produces:** exact public inventory/settings DTO parsers, `live/submit` routing through effective names, host-rendered pure-text results, and one current `copyText` action bound to plugin/generation/request/result.

- [ ] **Step 1: Implement command routing and bounded main results.** Preserve ordinary app search while adding prefix discovery, ASCII-space body parsing, 150 ms `live` debounce, first-Enter `submit`, exact response validation, and Rust-only storage of real action payloads. **Estimate:** AI coding 55-85 min.
- [ ] **Step 2: Implement launcher ownership and result interaction.** Bind publish/clear/error/copy behavior to the current view epoch, control value, submission token, plugin generation, request, and result; execute the single default action on second Enter or row click and preserve results on clipboard failure. **Estimate:** AI coding 40-65 min.
- [ ] **Step 3: Complete host-rendered plugin management and settings UI.** Add the main-only package picker and prepare/confirm/commit flow, development install/reload, enable/disable, rename/reset, uninstall delete/retain, and bool/text/number/select/secret controls. Never read secret values or expose `outputMode` as a setting. **Estimate:** AI coding 40-65 min.
- [ ] **Step 4: Add focused frontend/backend tests using existing Vitest/jsdom event patterns.** Do not add `@testing-library/user-event`; cover routing/result rendering, one settings submit interaction, stale result ownership, and `/math`/`/find` regression. **Estimate:** AI coding 15-25 min.

**Core test coverage:** preserved internal spaces and activation modes; result bounds and no `actions[]`; late A success/failure after user-edited B is inert; current copy action works by click/Enter; stale action is rejected; management errors stay local; all setting widget types submit valid values.

**Verify:** `npm.cmd test -- src/launcher.test.tsx`, then `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests commands::tests::execute_plugin plugins::tests::query -- --nocapture`, then `npm.cmd run build`

### Task 5: Singleton Plugin Window and Global Native Handoff

**Files:** create `src-tauri/src/window_transfer.rs`, `src-tauri/src/plugin_window.rs`, `src/plugin-window-core.ts`, `src/plugin-window-core.test.ts`, `src/plugin-window-view.tsx`, `src/plugin-window-view.test.tsx`, `src-tauri/capabilities/plugin-window-shell.json`, `src-tauri/capabilities/plugin-window-content.json`; modify `src-tauri/src/find_window.rs`, `src-tauri/src/lifecycle.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src-tauri/tauri.conf.json`, `src/main.ts`, `src/protocol.ts`, `src/styles.css`, `src-tauri/build.rs`; add generated plugin-window command permissions.

**Dependencies:** Tasks 1-4; design sections `10.3 window`, `11 插件窗口合同`, `12 权限与安全边界`, `16.3 插件窗口`, and `18 完成标准` items 5-6.

**Produces:** one host-wide `MainWindowTransferCoordinator`, one `PluginWindowController` per plugin ID, and one host-owned singleton native window per plugin. Label families are exactly `plugin-runtime-*`, `plugin-shell-*`, and `plugin-content-*`, with no capability overlap.

- [ ] **Step 1: Extract the shared native transfer lease and adapt `/find`.** Serialize native side effects while preserving `/find`; capture focus/topmost state, consume the expected main blur, revalidate ownership after async points, and restore only through a current-owner CAS. **Estimate:** AI coding 55-80 min.
- [ ] **Step 2: Implement plugin-window admission and lifecycle state.** Bind each transaction to UI intent epoch, submission token, plugin/generation/request, and target window; support singleton reuse, latest update/ack, pin, forced close, upgrade/disable/uninstall teardown, and stale completion suppression. **Estimate:** AI coding 50-75 min.
- [ ] **Step 3: Build and authorize the isolated native window.** Use a host shell WebView and separately labelled plugin-content WebView/custom source; deny generic Tauri access, navigation, downloads, new windows, and unapproved resources. Keep all native/WebView work outside controller locks. **Estimate:** AI coding 80-120 min.
- [ ] **Step 4: Build the host shell UI and persistence.** Add fixed-size icon pin/close controls, accessible tooltips, drag region, host theme tokens before content update, and per-plugin position correction/restoration. Pin remains process-local and never enables always-on-top. **Estimate:** AI coding 40-60 min.
- [ ] **Step 5: Add two deterministic handoff tests plus caller-guard checks.** Test with mocked native ports/handles only; automated Task 5 tests must not move real focus or synthesize input. **Estimate:** AI coding 15-25 min.

**Core test coverage:** (1) `A ready pending -> user edit or /find B -> A completes` leaves A unable to show, focus, clear, or hide and B remains owner; (2) `A begins focus -> B supersedes A -> A fails` prevents stale rollback, while a current unsuperseded failure restores the captured state. Also verify singleton reuse and exact shell/content caller guards.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml window_transfer::tests plugin_window::tests find_window::tests lifecycle::tests commands::tests -- --nocapture`, then `npm.cmd test -- src/plugin-window-core.test.ts src/plugin-window-view.test.tsx src/find-core.test.ts src/find-view.test.tsx`, then `npm.cmd run build`

### Task 6: `/demo`, SDK Artifacts, and Acceptance

**Files:** create `examples/public-plugins/com.uipilot.demo/package/plugin.json`, `examples/public-plugins/com.uipilot.demo/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.demo/package/dist/window.html`, `examples/public-plugins/com.uipilot.demo/package/dist/window.js`, `examples/public-plugins/com.uipilot.demo/package/dist/window.css`, `examples/public-plugins/com.uipilot.demo/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.demo/tests/sdk-contract.ts`, `examples/public-plugins/com.uipilot.demo/README.md`, `scripts/package-demo-plugin.ps1`, `src-tauri/src/bin/generate_public_plugin_schema.rs`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `src-tauri/tests/public_plugin_window_events.rs`; modify only integration wiring discovered necessary within the approved contract.

**Dependencies:** Tasks 1-5; design sections `15 /demo 参考插件`, `16 测试`, `17 非目标`, and `18 完成标准`.

**Produces:** an independently removable `/demo` package, generated data Schema, a compact TypeScript API declaration, developer guide, one process-isolation probe, and a user-operated acceptance flow.

- [ ] **Step 1: Implement `/demo` in both static package modes.** Default to `submit + window`; display input, platform, current theme, instance `1`, and `str yyyy-mm-dd`, with host-owned pin/close controls. Switch the package metadata to `mainResult` only on reload and return one `copyText` action. Keep README/tests outside the strict package root and add no host-side `/demo` fallback. **Estimate:** AI coding 35-55 min.
- [ ] **Step 2: Generate data Schema and publish the SDK contract.** Use `schemars` for deterministic manifest/data DTO JSON Schema. Keep `PluginHandler` and `Readonly<UiPilotPluginApiV1>` in a small checked-in `.d.ts`, compile the demo fixture against it, and document package layout, identity, errors, limits, permissions, and unsupported background/multi-action behavior. **Estimate:** AI coding 25-40 min.
- [ ] **Step 3: Run the non-interactive gates and WebView2 isolation probe.** Run the full test/build/lint suite once. Prove a failed public Runtime/window can be reclaimed without blocking the launcher, another plugin, `/math`, or `/find`; inability to prove process isolation remains a public-release No-Go. Report unrelated dirty-worktree failures without fixing them. **Estimate:** AI coding 20-35 min.
- [ ] **Step 4: Prepare one real-window diagnostic and manual acceptance pass.** Run the gated harness only after explicit permission; it may briefly change foreground focus but never controls mouse or keyboard. The user alone types `/demo str` and verifies singleton reuse, pin/close, drag/position, theme, reload mode change, and second-Enter copy. An unavailable interactive-session precondition is inconclusive, not a failure or pass. **Estimate:** AI preparation 10-20 min; checkpoint allowance includes 20-40 min of user acceptance.

**Core test coverage:** removing the package removes `/demo`; window/main-result modes expose the required fields/action; generated Schema matches Rust DTOs; `.d.ts` compiles against the demo; runtime/window failure isolation meets the No-Go gate; manual focus behavior is checked once without input synthesis.

**Verify:**

```powershell
npm.cmd test
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check
node --test examples/public-plugins/com.uipilot.demo/tests/runtime.test.js
npm exec tsc -- --noEmit --strict examples/public-plugins/com.uipilot.demo/tests/sdk-contract.ts
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
