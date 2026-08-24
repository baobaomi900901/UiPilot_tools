# Public Plugin Panel Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a third public-plugin `outputMode: "panel"` that opens a single command tag in the main launcher input and mounts an isolated plugin HTML surface in the result region, with Enter-submitted arguments and session discard on hide.

**Architecture:** Extend the v1 package contract with `panel.entry` and `ui.panel`, then add a host `PanelSession` that owns a child WebView label, `sessionEpoch`, and per-Enter `submissionToken`s. Launcher rows for panel plugins use `panelActivation` so one Enter opens the session; subsequent Enter uses R1 (`onCommand` again with argument-only input). Frontend owns tag chrome + separate suffix input; Rust owns isolation, ready/ack, latest-wins buffering, and teardown.

**Tech Stack:** Rust, Tauri 2 multi-webview, TypeScript, React 19, Vitest, plugin-cli schema sync.

**Approved specification:** [`docs/superpowers/specs/2026-08-24-public-plugin-panel-mode-design.md`](../specs/2026-08-24-public-plugin-panel-mode-design.md)

## Global Constraints

- Implement the exact contracts in design sections `3` (HCI), `4` (SDK/manifest), `5` (host architecture), and `6` (failure behavior). Do not reopen frozen P1/P2 decisions.
- Keep `schemaVersion: 1` and `apiVersion: 1`. Bump the shipping host package/version to `0.3.0` and require panel packages to set `minimumHostVersion: "0.3.0"`.
- `panel` requires `activationMode: "submit"`, `panel.entry`, and permission `ui.panel`. It forbids `window`, `ui.window`, and `timer.control`.
- Isolation is a host-managed independently labeled **child WebView**. Forbidden: iframe, `srcdoc`, injecting plugin HTML into the main webview, or navigating the main webview to plugin content.
- Maintain separate sequences: `sessionEpoch` for the mounted session, and per-Enter `requestId`/`submissionToken` bound to that epoch. Late results must fail closed when either identity no longer matches.
- Runtime channel is **R1**: every Enter (including the first) calls `onCommand` with argument-only input.
- While a tag is present, the user cannot activate another command; they must clear the tag first. Host may still force-end on disable/uninstall/upgrade/hide.
- No real mouse, keyboard, or foreground-focus automation. Manual HCI acceptance requires explicit user participation.
- Do not absorb unrelated workspace changes into panel-feature commits.

## Shared Contracts

Cite design §4–§5; do not redefine DTOs ad hoc in tasks.

- Manifest: `PublicOutputMode::Panel`, `PublicPanelV1 { entry }`, `PublicPermission::UiPanel` (`"ui.panel"`).
- Response: `PublicPluginResponse::Panel(PublicPanelResponse { request_id, data })`.
- Launcher activation: `LauncherResultActivation::PanelActivation { plugin_id, initial_argument, favorite }` (wire `kind: "panelActivation"`).
- Session: `PanelSessionIdentity { session_epoch, plugin_id, generation, command_label, content_label }`.
- Content bridge: `window.uipilotPluginPanel` with `onUpdate` + `storage` only (no close/timer/notifications).
- Frontend model: `panelSession: null | { pluginId, commandLabel, argument, sessionEpoch }` plus separate suffix input ownership.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6`.
- Every task follows focused TDD once, produces one atomic commit for that task only, and receives specification-compliance review before a dependent task starts when the active workflow provides those gates.
- Run Rust from repo root with `--manifest-path src-tauri/Cargo.toml`. Run frontend/CLI from repo root.
- Full-suite gates run once after Task 6, not after every task.

---

### Task 1: Manifest, Schema, CLI, And Host Version Floor

**Files:** `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/tests.rs`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/src/validate.ts`, `packages/plugin-cli/src/package-policy.ts`, `packages/plugin-cli/tests/manifest.test.ts`, regenerate `packages/plugin-cli/src/generated/manifest-validator.mjs` via existing sync/build scripts, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs` (host `Version::new(0, 3, 0)` load site), root/`package.json` version fields if they track host release.

**Dependencies:** Design sections `4.1`–`4.5`, `4.3`, and `8` rows for Manifest/permission.

- [ ] Add `panel` output mode, `PublicPanelV1 { entry }`, and `ui.panel` permission with the exact legal/illegal combination matrix from design §4.2.
- [ ] Keep `timer.control` window-only; reject `panel + timer.control`, `panel + live`, `panel + window`/`ui.window`, and `mainResult + ui.panel`/`panel`.
- [ ] Sync schema, TypeScript API docs, developer guide, and plugin-cli validator/allowlist in lockstep; regenerate the standalone Ajv validator.
- [ ] Bump shipping host version to `0.3.0` everywhere the host reports/compares `minimumHostVersion`, and document that panel packages must declare `"0.3.0"`.

**Distinct test coverage:** legal panel package accepts; each illegal combination fails closed; old packages without `panel` still validate; CLI rejects panel packages missing `ui.panel` or `panel.entry`; host version floor rejects panel packages on a simulated older host.

**Verify:** `cargo test public_plugins::tests --manifest-path src-tauri/Cargo.toml`; `npm test --workspace @uipilot/plugin-cli -- manifest.test.ts`

### Task 2: Panel Response Discriminant And Catalog `panelActivation`

**Files:** `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src/protocol.ts`, related backend/frontend protocol tests.

**Dependencies:** Task 1; design sections `4.4`, `5.3`, `5.6`, and `3.1` initial-argument rules.

- [ ] Extend `PublicPluginResponse` with `Panel({ requestId, data })` under the existing size budget; reject panel Runtime replies that look like main-result lists.
- [ ] Route `outputMode: panel` Runtime completions through the Panel discriminant; unexpected `MainResults`/`Window` from a panel route fail the submission without changing launcher mode.
- [ ] Emit catalog/search rows for panel plugins as `panelActivation` (not `pluginCompletion`), including `pluginId`, `favorite`, and `initialArgument`.
- [ ] Compute `initialArgument` per design §3.1: empty for default/favorite/slash-prefix discovery; current free-text query for plain-text match; slash submit `/cmd args` uses args-only text.
- [ ] Parse the new activation strictly on the frontend wire; malformed panel rows drop that row only.

**Distinct test coverage:** panel Runtime success maps to Panel; wrong discriminant fails closed; favorite plain-text match preserves query as `initialArgument`; `/bb` discovery and empty home use `""`; window plugins remain `pluginCompletion` with two-Enter behavior unchanged.

**Verify:** `cargo test panel_activation --manifest-path src-tauri/Cargo.toml`; `cargo test public_plugins --manifest-path src-tauri/Cargo.toml`; `npm.cmd test -- src/protocol.test.ts src/launcher.test.tsx`

### Task 3: PanelSession Controller, Child WebView Isolation, And Bridge

**Files:** create `src-tauri/src/plugin_panel.rs` (or equivalent module owned by public-plugins), wire through `src-tauri/src/lib.rs` / `commands.rs` / `build.rs`; add capability `src-tauri/capabilities/plugin-panel-content.json`; generate narrow permissions for panel ready/ack/storage as needed; mirror isolation helpers from `src-tauri/src/plugin_window.rs` and `webview_audio_guard` where appropriate.

**Dependencies:** Task 2; design sections `5.1`, `5.2`, `5.4`, `5.5`, and `5.7`.

- [ ] Implement `PanelSession` with independent `sessionEpoch` and per-Enter token binding; at most one live session host-wide.
- [ ] Mount an independently labeled child WebView into the main window result slot using the public-plugin custom protocol, navigation allowlist, CSP, no new-window, no download; never touch main webview navigation/DOM for plugin HTML.
- [ ] Implement ready/ack handshake analogous to plugin-window content; while not ready, retain **only the latest** pending argument.
- [ ] Inject frozen `window.uipilotPluginPanel` with `onUpdate` + `storage`; delete/deny Tauri internals exposure beyond allowlisted panel commands; caller guards reject non-allowlisted commands from panel labels.
- [ ] Teardown destroys the child WebView and bumps/drops `sessionEpoch` so in-flight work cannot revive UI.

**Distinct test coverage:** child label isolation invariants (source/contract tests like plugin_window); forbid iframe/main injection markers; ready-before-submit; latest-pending overwrite while not ready; late submission after teardown ignored; storage available under panel session rules; unauthorized invoke from panel label denied.

**Verify:** `cargo test plugin_panel --manifest-path src-tauri/Cargo.toml`; `cargo test public_plugin_commands_have_non_overlapping_exact_capabilities --manifest-path src-tauri/Cargo.toml`

### Task 4: Open/Submit Commands, R1 Dispatch, And Epoch-Gated Settlement

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/capabilities/main.json`, permission toml generation, `src-tauri/src/lib.rs` / `build.rs` registrations.

**Dependencies:** Task 3; design sections `5.2`–`5.4`, `5.3` (R1), and `6`.

- [ ] Add main-only commands to open a panel session and to submit an argument while a session is live (names may be `open_plugin_panel` / `submit_plugin_panel`; keep exact main label guard).
- [ ] Each open/submit allocates a new `requestId`/`submissionToken` bound to the current `sessionEpoch` and dispatches `onCommand` with argument-only input (R1).
- [ ] Apply Runtime Panel responses only when token and epoch still match; submitting B while A is in flight discards A; hide/clear/teardown discards all in-flight results for the old epoch.
- [ ] Hard failures (invalid response, load/ready failure) roll back to empty launcher state and surface the existing host error channel.

**Distinct test coverage:** ordered scenario A in-flight then B commit → only B applies; hide during A → A late success inert; epoch bump after teardown rejects stale token; empty argument submit allowed; unexpected MainResults/Window from panel route fails without leaving a zombie tag.

**Verify:** `cargo test plugin_panel --manifest-path src-tauri/Cargo.toml`; `cargo test public_plugins::scheduler --manifest-path src-tauri/Cargo.toml`

### Task 5: Launcher Tag Chrome, Suffix Input, And One-Enter HCI

**Files:** `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`, `src/protocol.ts` (client wrappers if needed), `src/main.ts` as required for invoke wiring.

**Dependencies:** Task 4; design sections `3.1`–`3.4`, `3.6`–`3.7`, `5.6`, and caret/`initialArgument` rules.

- [ ] Render host-owned command tag + separate suffix `<input>`; do not use a single synthetic string field.
- [ ] One Enter on `panelActivation` opens the panel (bypass `pluginCompletion` arming). Populate suffix from `initialArgument`; caret at `0` when empty, else at end.
- [ ] While tagged: Enter submits suffix via panel submit command; result list stays hidden; no other result/command activation paths remain reachable.
- [ ] × click and Backspace only when `selectionStart === selectionEnd === 0` end the session and restore empty launcher.
- [ ] Preserve IME/composition inside suffix; Ctrl+A/Home/paste operate on suffix only.

**Distinct test coverage:** one-Enter from panelActivation; `/cmd hello` caret-at-end behavior in model; plain-text match preserves argument; Backspace-at-0 clears tag while Backspace mid-text does not; duplicate Enter while submitting keeps latest-owner semantics on the frontend; window pluginCompletion two-Enter path unchanged.

**Verify:** `npm.cmd test -- src/launcher.test.tsx`; `npm.cmd run build`

### Task 6: Hide/Reset Wiring, Example Panel Plugin, And Docs Lockstep

**Files:** `src-tauri/src/lib.rs` (main hide/focus paths), `src-tauri/src/lifecycle.rs` if hide coordination lives there, `src/launcher-core.ts` shown/hide handlers, create `examples/public-plugins/com.uipilot.demo-panel/**`, update `.gitignore` dist exceptions if needed like note/demo-win, refresh SDK guide examples table, mark design status Approved if still draft.

**Dependencies:** Tasks 3–5; design sections `3.5`, `3.6`, `5.7`, `6`, and `7`.

- [ ] On every main-window hide path (Escape, blur-hide, tray hide, and existing clear-and-hide), tear down the panel session before or as part of hide so the next shown event is a fresh initialized launcher with no tag/webview residue.
- [ ] Force-end panel sessions on plugin disable/uninstall/upgrade.
- [ ] Add a minimal `com.uipilot.demo-panel` example: `submit + panel`, `ui.panel`, `panel.entry`, Runtime returning `{ requestId, data }`, content using `uipilotPluginPanel.onUpdate`, `minimumHostVersion: "0.3.0"`.
- [ ] Update developer guide selection table and acceptance notes; keep note/window demos unchanged.

**Distinct test coverage:** hide then shown clears panel model/session epoch; disable/uninstall while open tears down; demo-panel package validates on CLI for Windows; contract test that demo content never calls `invoke(` directly.

**Verify:** `cargo test plugin_panel --manifest-path src-tauri/Cargo.toml`; `node --test examples/public-plugins/com.uipilot.demo-panel/tests/*.js` (or the repo’s established example test command); `npm exec --workspace @uipilot/plugin-cli -- uipilot-plugin validate examples/public-plugins/com.uipilot.demo-panel/package --platform windows`

## Final Verification And Acceptance

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features` and confirm zero failures.
- [ ] Run `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features -- -D warnings`.
- [ ] Run `npm.cmd test`, `npm.cmd run build`, and plugin-cli tests; confirm zero failures.
- [ ] Compare the implementation against design §3.7 HCI checklist and §7 technical checklist.
- [ ] Ask the user before manual UI acceptance. The user performs: slash open, list one-Enter open, plain-text preserve, tag × / Backspace-at-0, Enter submit, Escape hide + reopen fresh, and attempts to run another command while tagged. The agent observes only; no mouse/keyboard control.

## Spec Coverage Self-Check

| Design requirement | Task |
|--------------------|------|
| `outputMode: panel` + `panel.entry` + `ui.panel` matrix | Task 1 |
| Host `0.3.0` / `minimumHostVersion` | Task 1 |
| `PublicPluginResponse::Panel` | Task 2 |
| `panelActivation` + `initialArgument` sources | Task 2, Task 5 |
| Child WebView isolation / CSP / caller guards | Task 3 |
| `sessionEpoch` vs submission token; latest pending | Task 3, Task 4 |
| R1 every Enter → `onCommand` | Task 4 |
| Late A discarded after B/hide/teardown | Task 4 |
| Tag + suffix input; caret rules; Backspace-at-0 | Task 5 |
| One-Enter list open; exclusivity while tagged | Task 5 |
| Hide/show fresh launcher; force-end on uninstall | Task 6 |
| Example panel plugin + docs | Task 6 |
