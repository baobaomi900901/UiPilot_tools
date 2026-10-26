# Command Hotkeys Implementation Plan

Build one local host increment: Settings command rows assign one global chord to a stable public-plugin command. The architecture uses host Settings persistence, a command native registry serialized with launcher hotkey edits, and the existing lifecycle/activation paths.

**Specification:** [Command hotkeys design](../specs/2026-10-26-command-hotkeys-design.md), all sections. This document follows [repository plan guidelines](../../agents/implementation-plan-guidelines.md).

**Technology and constraints:** Existing Rust/Tauri 2 and React 19/TypeScript stack; no new dependency. Host 0.11.0, public plugin API v1 unchanged, existing example minimum-host versions unchanged. Native tests use fake registrations only. No real focus/input/global shortcut operations, user-data changes, network capability, new demo plugin, or changes to ordinary Enter semantics. Apply [UI guidelines](../../ui-guidelines.md).

## Execution rules

Task 1 -> Task 2. Focused behavior tests precede implementation; confirm the intended failures, implement the minimum, rerun the affected tests, and commit the complete vertical increment. Task 2 records delivery evidence in a separate atomic commit. Keep unrelated work out of both commits.

One parent design review, one implementer, and one combined implementation/code review suffice; fix concrete findings without adding serial layer gates or repeat full review loops. Use root commands below; Rust commands use `--manifest-path src-tauri/Cargo.toml --lib`. Do not run native registration or GUI interaction during automated tests.

### Task 1: Ship command bindings from Settings through native entry

**Files:** `src-tauri/src/command_hotkeys.rs`, `src-tauri/src/settings.rs`, `hotkey.rs`, `lifecycle.rs`, `commands.rs`, `public_plugins.rs`, `public_plugins/activation.rs`, `lib.rs`; `src-tauri/build.rs`, `src-tauri/capabilities/main.json` and generated command permission files; `src/protocol.ts`, `launcher-core.ts`, `launcher-view.tsx`, `public-plugin-panel.tsx`, `main.ts`, `hotkey-recorder.ts`, extracted `hotkey-recorder-input.tsx`, `styles.css`; existing focused test files. Update root `package.json`/`package-lock.json`, `src-tauri/Cargo.toml`/`Cargo.lock`/`tauri.conf.json`, and `packages/plugin-cli/src/contracts.ts` plus its version expectations.

**Dependencies:** Design sections “Settings and visible behavior”, “Registration, persistence, and locks”, “Native delivery and recording”, and “Stable command entry and execution”.

- [x] Implement host map, separate native registration table, shared serialization, main-only IPC, inventory enrichment, rollback/status, and both-way launcher conflict checks.
- [x] Connect startup and actual lifecycle publications, including runtime faults; implement prune/reinstall ordering and retainData-independent uninstall cleanup.
- [x] Carry stable identity through readiness/shown and foreground recording events, resolve current entry, and retain identity through asynchronous command routing. Preserve ordinary Enter and existing completion semantics.
- [x] Reuse recorder and command-row mutation/reload ownership for edit/clear, accessible status/focus, Light/Dark layout, and double-tap restriction.
- [x] Synchronize host 0.11.0 and CLI current host target without changing SDK/API contracts or old example floors.

**Distinct test coverage:** Own all automated cases in the design “Focused evidence and manual acceptance” section, especially gate linearization, non-main rejection before state access, persistence/native rollback, save/uninstall ordering, stale readiness/resolve replies, rename-and-label-reuse routing, and recording DOM/native deduplication. Use existing activation fixtures and fake registration operations.

**Verify:** `npm test -- src/hotkey-recorder.test.ts src/protocol.test.ts`; `npm test -- src/launcher.test.tsx src/public-plugin-settings.test.tsx -t command.hotkey`; `cargo test --manifest-path src-tauri/Cargo.toml --lib command_hotkey` (name new focused cases with this prefix); `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugin_prompt_and_dispatch_decisions_preserve_activation_modes`. Run additional existing focused test modules only where changed contracts require them.

### Task 2: Build and hand off the existing-plugin manual checks

**Files:** Release/version notes at the repository's existing release-note location, or `docs/superpowers/command-hotkeys-delivery.md` if no such location exists; this plan's checkboxes.

**Dependencies:** Task 1; design “Scope and outcome” and “Focused evidence and manual acceptance”.

- [x] Run one integrated verification and production build after focused tests/review fixes pass; record exact command results and executable path.
- [x] Provide a short manual sequence using existing Text Snippet and Notes: record/edit/clear; hidden/visible trigger; required-input prompt; panel entry; conflicts in both directions and OS rejection; rename; disable/re-enable; restart; update/uninstall including retainData.
- [x] Record two-theme/keyboard checks and distinguish automated evidence from unperformed human global-trigger/restart/system interaction. Do not claim manual success without results.
- [x] Commit delivery notes, then report build location, commit IDs, remaining manual checks, and any concrete limitation.

**Distinct test coverage:** End-to-end interaction belongs to the human checklist; existing packages supply the acceptance fixture. No new plugin, browser demo, native smoke automation, or additional review cycle.

**Verify:** Reuse completed focused test evidence; run `npm run build` and the existing acceptance build: `cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked`, with process-only `CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target` and `TAURI_CONFIG` identifying `com.uipilot.launcher.acceptance` / `UiPilot Acceptance`. Freeze clean source/dist input hashes before the native build and compare afterward. Do not repeat the broad verifier or unchanged suites. Preserve all previous delivery binaries; do not run the resulting app.



## 2026-10-26 Delivery status

Implementation and delivery tasks complete; native human acceptance is pending. Frozen executable source is `2b99e9e3c00341860f87062fc2fa8a693ac4fd75`; independent full review plus the accepted-Panel and failed-open cleanup deltas pass. Final covering frontend checks: 53 passed; the prior Rust/protocol/CLI evidence remains valid. Final frontend build and offline native build passed. Non-blocking Vite size and Rust test-helper dead-code warnings are recorded in the delivery notes.

Kit: `.superpowers/deliveries/uipilot-0.11.0-acceptance/`. Actual Windows version0.11.0; 39,950,848bytes; SHA256 `c00d796e41b2f4df4d513cab4dfa16a15dbabd00b35517bb8e6273507d86ac07`. 738 source/dist inputs unchanged, five resources/two existing archives matched, nine prior executables unchanged. No native app/global hotkey/clipboard/user-data operation was performed by the agent.

`README.zh-CN.md`, `manual-cases.zh-CN.md`, and `manual-acceptance.json` provide the initial three checks and later conflict/Panel/lifecycle/theme cases. All user outcomes are still pending; these checkboxes record implementation and handoff, not a blanket manual PASS. Continue with the user's feedback, not another feature or repeated whole-suite run. Ordinary Enter count and unrelated baseline credential issues remain deferred.

## 2026-10-26 Native Notes first-press correction

User accepted initial MainResult/required-input/restart cases, registered-key conflict, and disable/re-enable. Notes needed two presses on every return from another app. Controlled combined shown+entry tests reproduced a default-search timer starting during command resolution, whose response overwrote the synthetic Panel result. Sourcefd83558f63bf08189dc64f0f05a23afd9af5c2dd adds only generation capture/check to deferred search. Three RED ordering cases,22 focused GREEN tests and frontend build passed; independent delta Spec/QualityPASS. Existing Panel ownership/exit evidence retained.

Separate patch kit `.superpowers/deliveries/uipilot-0.11.0-panel-hotkey/`, same0.11.0/acceptance identity. Actual exe39,950,848bytes, SHA522ca3a54f2a2bbfc665998c09ecb63c4939543a85cd756008fae49a621df70f. Offline native build passed,738inputs stable,5resources/2archives matched,10prior executables unchanged. User retest pending: one press from another app enters Notes repeatedly; closing Notes restores ordinary search. No native effects executed by agent; no repeat of already-confirmed unrelated cases requested.

## 2026-10-26 Notes 过渡状态和共享标签修正

用户确认上一修正版 Notes 一次快捷键进入、关闭标签后恢复默认结果和普通搜索；仍发现打开期间出现临时 /notes 列表，以及 Notes 与 MainResult 命令标签样式不同。已记录功能通过，未把显示问题标为通过。

源码 454463e81650e72722d9e566a3c5dada8eb708a8 保留私有 Panel 打开归属，将临时结果排除出视图并显示现有命令输入区和加载反馈。取消、编辑或新命令会使旧打开失效；标签共用 UI 规范中的样式。59 项相关检查、最后提示位置调整后的 5 项 UI 检查、前端构建与独立复核通过，普通 Enter 流程和 SDK 不变。

新验收程序为 .superpowers/deliveries/uipilot-0.11.0-command-presentation/uipilot.exe，仍为 0.11.0 和原 acceptance 数据标识。离线原生构建通过，实际 39,950,848 字节，SHA256 d5b38c55a32fbe6037a7289dd92fcf58799427c28c0bd7fc9033e49375f8e04f。738 项构建输入前后一致，5 个资源、2 个既有插件包匹配，11 个旧程序 hash 未变。现有 Vite/Rust 非阻塞构建提示保留。

下一人工节点仅复测 Notes 一次快捷键打开时无临时列表、Notes/MainResult 标签在两种主题中一致、退出后搜索正常。原生显示与交互尚待用户确认；agent 未运行应用或真实快捷键。已有功能验收保留，总体阶段估计仍约 72%，不展开新功能或重复此前通过的完整矩阵。
