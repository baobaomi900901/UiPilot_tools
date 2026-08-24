# Public Plugin Message Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans`. Execute sequentially in the current workspace with one implementation agent unless the user explicitly changes the workflow.

**Goal:** Add request-bound public-plugin message publishing, a persistent message center with unread state, Windows notifications, tray flashing, and the `demo-win` reference flow.

**Architecture:** A host-owned `MessageCenterService` persists records through the existing atomic current/backup mechanism. Public-plugin calls commit while the scheduler current-request guard is held, then return a `MessagePostGuardEffect` for event, notification, and tray dispatch after every protected lock is released. The main frontend consumes strict DTOs through a monotonic message-session state machine; Windows toast and tray adapters remain behind narrow host traits.

**Tech Stack:** Rust 1.96, Tauri 2.11, `windows` 0.61 WinRT APIs, TypeScript 7, React 19, Ant Design 6, OverlayScrollbars 2, Vitest 4.

**Approved specification:** [Public Plugin Message Center and Windows Notification Design](../specs/2026-08-18-public-plugin-message-center-design.md)

## Global Constraints

- Windows 11 x64 is the only native-notification target; `notifications.publish` remains unavailable on macOS.
- One valid plugin request may publish one 1-500 Unicode-scalar, single-line plain-text message.
- The atomic message-file replacement is the irreversible publish success point.
- Lock order is scheduler current guard -> message store -> atomic commit; no lock crosses Tauri emit, toast, tray, show/focus, or frontend acknowledgement.
- Message IDs and revisions are canonical decimal `u64` strings; TypeScript comparisons use one length-first `compareU64Decimal` helper and never `number`.
- `unavailable` is an absorbing frontend state for the current native process; only process restart resets it.
- Message arrival never shows, hides, focuses, or moves a window. Only a verified notification click may request `ShowTarget::Messages`.
- Do not add background plugin execution, remote push, notification actions, per-message deletion/search/export, or notification-driven cold start after UiPilot exits.
- Real Windows notifications, foreground changes, package installation, and user interaction require advance notice and explicit user confirmation. Agents never control the mouse or keyboard.
- Preserve all pre-existing workspace changes. Every task commits only its listed feature files.

## File Map

**New backend files**

- `src-tauri/src/message_center.rs`: service facade, DTOs, state events, post-guard effects, and adapter traits.
- `src-tauri/src/message_center/store.rs`: persistent document, mutations, recovery, limits, and terminal state.
- `src-tauri/src/message_center/store_tests.rs`: store and linearization tests.
- `src-tauri/src/message_center/windows_notification.rs`: WinRT toast adapter and fixed DOM builder.
- `src-tauri/src/message_center/tray_flash.rs`: single-timer tray reminder state machine and Tauri adapter.
- `src-tauri/icons/tray-reminder.png`: static host-owned reminder icon.

**New frontend files**

- `src/message-center-core.ts`: decimal comparison and absorbing frontend message state.
- `src/message-center-core.test.ts`: protocol, revision, and stale-completion tests.
- `src/message-center-panel.tsx`: settings message-list presentation.
- `src/message-center-panel.test.tsx`: list, empty, unavailable, and clear behavior.

**Existing integration files**

- Backend: `src-tauri/src/public_plugins/{runtime,scheduler,manifest,state,activation}.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/{commands,lifecycle,lib}.rs`, `src-tauri/build.rs`, `src-tauri/Cargo.toml`, `src-tauri/capabilities/main.json`, and generated command permissions.
- Frontend: `src/{protocol,main,launcher-core,launcher-view,launcher.test,styles}.ts*`.
- SDK and reference plugin: `docs/plugin-sdk/{uipilot-plugin-api-v1.d.ts,public-plugin-v1.md,public-plugin-developer-guide.md}`, `.gitignore`, and `examples/public-plugins/com.uipilot.demo-win/**`.

## Core Contract Overview

- Backend service: `MessageCenterService` with `summary`, `commit_publish`, `open_and_mark_read`, `read_snapshot`, `clear`, `dispatch_post_guard`, and `shutdown` operations.
- Internal deferred effect: `MessagePostGuardEffect::Published` or `MessagePostGuardEffect::BecameUnavailable`. It is dispatched only after `PluginRequestScheduler::with_current` returns.
- Internal plugin API result: `PluginApiExecution { result, post_guard_effect }`, allowing both success and error paths to carry a deferred host effect without emitting under the scheduler lock.
- Main commands: `get_message_summary`, `open_message_center`, `read_message_center`, and `clear_messages`.
- Main-only event: `message-center://state-changed` carrying the specification's `MessageHostStateChangedEvent` union.
- Lifecycle route: Rust `ShowTarget::Messages`, serialized as `messages`, opens the existing main window directly on the Messages settings tab.
- Store location: `<app-data>/message-center/messages.json` with `messages.json.backup` managed by `atomic_file::commit_with_backup`.
- Wire DTOs, error DTOs, revision cursors, XML construction rules, and failure semantics are canonical in design sections 7-13 and must not be redefined differently in task code.

## Global Execution Rules

- Each task follows focused TDD once: establish the intended failure, implement the minimum contract, run the focused verification, then create one atomic commit.
- Do not create per-task review agents. Automated focused tests gate dependent tasks; the user performs only the final real Windows acceptance.
- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`; `Task 3 -> Task 6`; `Task 2 -> Task 7`. Final verification requires Tasks 5, 6, and 7.
- Rust verification commands run from the repository root with `--manifest-path src-tauri/Cargo.toml`.
- Generated permission files may be regenerated, but a task must stage only the exact commands it adds.

---

### Task 1: Persistent Message Domain

**Files:** Create `src-tauri/src/message_center.rs`, `src-tauri/src/message_center/store.rs`, `src-tauri/src/message_center/store_tests.rs`; modify `src-tauri/src/lib.rs` only to declare the module.

**Dependencies:** Approved design sections 5, 8, 9, 10, and 15.

- [ ] Implement canonical `MessageStoreV1`/`MessageRecord` serialization, UTC RFC 3339 timestamps, 100-record eviction, host-assigned IDs, and revision advancement.
- [ ] Load valid current first, valid backup second, create an empty store only when neither file exists, and enter terminal unavailable without overwriting when existing candidates are invalid.
- [ ] Implement publish, open-cutoff read marking, read-only snapshot, clear, transient write failure, and counter-exhaustion transitions through `AtomicPaths` and `commit_with_backup`.
- [ ] Expose store outcomes that distinguish ready success, ready operation failure, and the first `BecameUnavailable` transition without performing Tauri or native side effects.

**Distinct test coverage:** first publish and restart recovery; 101st message evicts only the lowest ID; event-order-independent open cutoff; clear racing a later publish preserves the later unread record; current corruption recovers backup; current and backup corruption stays unavailable and preserves both files; write failure preserves the old ready snapshot; ID or revision exhaustion emits one transition and never wraps.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml message_center::store`

### Task 2: Request-Bound Plugin Publish API

**Files:** Modify `src-tauri/src/message_center.rs`, `src-tauri/src/public_plugins/{runtime,scheduler,manifest,state,activation}.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, and their existing Rust test modules.

**Dependencies:** Task 1; design sections 6, 7, 11, 14, and 15.

- [ ] Make `notifications.publish` installable only for a Windows host and require both manifest declaration and the current generation's persisted grant.
- [ ] Extend the deep-frozen runtime bootstrap with `api.notifications.publish({ content })`; extend `PluginApiOperation` and Rust validation without allowing unknown fields, controls, multiline content, or a second successful publish per request.
- [ ] Track the per-request successful-publish bit inside the scheduler's current request state and set it only after the atomic message commit succeeds.
- [ ] Return `PluginApiExecution` plus `MessagePostGuardEffect` from the internal runtime path. Update `plugin_api_call` so it dispatches the effect only after `with_current` has released the scheduler guard.
- [ ] Preserve existing expiry ownership: a commit that wins remains successful, while the old request's later window/main response can still be discarded by the scheduler.

**Distinct test coverage:** undeclared and ungranted calls fail before store access; forged caller/context and expired generation retain existing errors; invalid text matrix includes boundary whitespace, 500/501 scalars, controls, and unknown fields; first publish succeeds and second returns `AlreadyPublished`; failed persistence does not consume the one-publish allowance; `A commit -> guard release -> B supersedes A -> A effect dispatch` saves once and returns publish success; exhaustion returns `MessageStoreUnavailable` with only a deferred unavailable effect.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::`

### Task 3: Main Commands, State Events, and Message Route

**Files:** Modify `src-tauri/src/message_center.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lifecycle.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/main.json`; create generated permissions for `get_message_summary`, `open_message_center`, `read_message_center`, and `clear_messages`.

**Dependencies:** Tasks 1 and 2; design sections 9, 10, 11, 12, and 15.

- [ ] Add main-caller-only commands with exact ready/unavailable error DTOs; map records to view DTOs using the installed plugin's current icon URL or `null` without persisting icon generations.
- [ ] Emit `message-center://state-changed` only to `main`: ready after committed publish/read/clear, unavailable exactly once after a terminal transition, and no ready event for a failed commit.
- [ ] Add `ShowTarget::Messages` to readiness, serialization, and lifecycle tests without changing launcher/settings behavior or synthesizing input.
- [ ] Wire `MessageCenterService` into production setup before public-plugin manager initialization, register commands/state/capabilities, and shut it down from the existing clean-exit path.

**Distinct test coverage:** every command rejects non-main labels before protected state access; equal-revision full snapshots remain valid after an earlier ready event; lower snapshots request reread; transient `MessageOperationFailed` remains ready; `A ready queued -> B unavailable emitted -> A ready delivered` is allowed by backend ordering; notification route requests only `ShowTarget::Messages`; startup unavailable does not block launcher, find, or plugins without notification permission.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml message_center_`

### Task 4: Frontend Protocol and Absorbing State

**Files:** Create `src/message-center-core.ts`, `src/message-center-core.test.ts`; modify `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, and `src/launcher.test.tsx` test fixtures.

**Dependencies:** Task 3; design sections 5, 9, 10, 11, and 15.

- [ ] Add strict parsers for summary, snapshot, message view, state event, and store-status errors; implement the single canonical `compareU64Decimal` helper including `u64::MAX` validation.
- [ ] Install the state-event listener before the initial summary request, maintain separate summary/snapshot cursors, and reread only when a higher ready event invalidates the loaded list.
- [ ] Implement `unknown -> ready/unavailable` and `ready -> unavailable`; once unavailable is observed, discard every later ready event, successful summary/snapshot, and `storeStatus: ready` error until native-process restart.
- [ ] Route `messages` shown targets to the settings Messages tab, invoke open-and-mark-read once per entry, read without marking on later ready events, and expose clear state to the view.

**Distinct test coverage:** `9 -> 10`, `99 -> 100`, values over `Number.MAX_SAFE_INTEGER`, `u64::MAX`, malformed and out-of-range strings; event-before-equal-snapshot acceptance; higher-event/lower-snapshot reread; `A response delayed -> unavailable observed -> A ready event/summary/snapshot/ready-error` all discarded; WebView reload starts unknown and must query the still-unavailable native host before rendering.

**Verify:** `npm test -- message-center-core.test.ts launcher.test.tsx`

### Task 5: Messages Tab, Badge, and List UI

**Files:** Create `src/message-center-panel.tsx`, `src/message-center-panel.test.tsx`; modify `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/protocol.ts`, and `src/styles.css`.

**Dependencies:** Task 4; design sections 4.2, 4.3, 12, and 15.

- [ ] Move settings-tab ownership into the launcher snapshot, add the order General -> Messages -> Plugins, and retain the existing OverlayScrollbars behavior.
- [ ] Render newest-first rows with stable icon dimensions, current plugin icon/default fallback, local time, and wrapping plain text; do not interpret markup or nest cards.
- [ ] Add a fixed-size settings-button badge: hidden at zero, exact 1-99, `99+` at 100, and `!` for terminal unavailable without changing the input layout or focus outline.
- [ ] Mark existing records read on tab entry, keep later arrivals unread while the tab is open, keep the settings page open during clear, and show “消息不可用，请重启 UiPilot” with no retry control.

**Distinct test coverage:** tab order and keyboard selection; notification target selects Messages; empty versus unavailable copy; clear success and `MessageOperationFailed` preserve the settings window; arrivals while open restore the badge; 0/1/99/100/terminal badge rendering; unavailable has no retry button and delayed ready completions cannot restore list or badge.

**Verify:** `npm test -- message-center-panel.test.tsx launcher.test.tsx`

### Task 6: Windows Toast and Tray Effects

**Files:** Create `src-tauri/src/message_center/windows_notification.rs`, `src-tauri/src/message_center/tray_flash.rs`, `src-tauri/icons/tray-reminder.png`; modify `src-tauri/src/message_center.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/lifecycle.rs`, and `src-tauri/Cargo.toml`.

**Dependencies:** Task 3; design sections 4.4, 11, 13, 15, and 16.2.

- [ ] Enable only the required `windows` crate features for `Windows.Data.Xml.Dom`, `Windows.Foundation`, and `Windows.UI.Notifications`; do not add the Tauri notification plugin.
- [ ] Build Toast XML from one fixed host template and write plugin name/body only through `CreateTextNode()` plus `AppendChild()`, or equivalent DOM `InnerText`. Set the fixed route and validated host message ID through DOM attributes; never interpolate plugin text into `LoadXml()` or attributes.
- [ ] Implement the narrow toast trait using UiPilot's configured installed identity (`com.uipilot.launcher`), `Setting`, `Show`, `Activated`, `Failed`, `Dismissed`, active-handler cleanup, and best-effort `Hide` during clean exit.
- [ ] Retain the built Tauri tray handle, alternate normal/reminder icons every 500 ms for 6 seconds, restart the single deadline on a new message, and restore normal on timeout, adapter error, tray rebuild, or exit.
- [ ] Dispatch publish effects independently in ready event -> toast -> tray order; one adapter failure cannot skip the next effect or change the committed plugin result.

**Distinct test coverage:** DOM `InnerText` preserves `<>&"'`, `</text><actions>`, and fake launch payloads while the DOM has no action node; fake toast adapter covers disabled setting, synchronous/async failure, activation fixed-route, dismissal, and hide cleanup; fake-clock tray tests cover exact cadence, deadline replacement, one timer, failure cleanup, and shutdown; message content never enters ordinary diagnostics.

**Verify:**

- `cargo test --manifest-path src-tauri/Cargo.toml message_center::windows_notification`
- `cargo test --manifest-path src-tauri/Cargo.toml message_center::tray_flash`

### Task 7: SDK Contract and `demo-win`

**Files:** Modify `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `.gitignore`, `examples/public-plugins/com.uipilot.demo-win/README.md`, `examples/public-plugins/com.uipilot.demo-win/package/plugin.json`, `examples/public-plugins/com.uipilot.demo-win/package/dist/runtime.js`, and `examples/public-plugins/com.uipilot.demo-win/tests/{runtime.test.js,sdk-contract.ts}`.

**Dependencies:** Task 2; design sections 6, 7, and 14.

- [ ] Publish the frozen `notifications.publish` TypeScript API and document request-bound lifetime, Windows-only availability, permission grant, one-message limit, pure-text validation, and failure semantics.
- [ ] Unignore only reference-plugin package `dist` assets needed for source distribution; do not expose unrelated build output.
- [ ] Update `demo-win` to version `1.0.3`, Windows-only support, permissions `ui.window` plus `notifications.publish`, and publish the exact `returnText` before returning the existing window response.
- [ ] Keep `demo-return` unchanged and regenerate/verify the manifest schema remains canonical.

**Distinct test coverage:** demo mock receives exactly one publish with text identical to window `returnText`; publish rejection prevents a window response; SDK contract sees the new readonly API; install/update tests accept the permission on Windows, require its exact grant, and preserve the old version when authorization is refused.

**Verify:**

- `node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js`
- `npm run build`

## Final Verification and Acceptance

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] `npm test`
- [ ] `npm run build`
- [ ] Confirm every design acceptance item in section 17 maps to a passing automated test or the manual gate below.
- [ ] Before any real-window step, notify the user and wait for confirmation; do not control mouse or keyboard.
- [ ] With user confirmation, run the development acceptance from design section 16.2 under ordinary permissions.
- [ ] Build and install the ordinary-permission packaged artifact, then have the user verify production notification identity/icon, click routing, OS-disabled behavior, tray flashing, and clean-exit cancellation. `tauri dev` is not release evidence for these items.
