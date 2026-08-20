# Public Plugin Window Timer API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `subagent-driven-development` or `executing-plans` to implement this plan task by task. Track work with the checkboxes below.

**Goal:** Add the approved host-owned single timer API to public plugin windows, including pause/reset, hidden-window continuation, message-center completion, one-shot host alarm, and an independent Pomodoro example plugin.

**Architecture:** Extend the pre-release public plugin v1 contract with `timer.control`, then implement one in-memory timer record per `pluginId + pluginGeneration`. The timer service owns its clock, scheduler, ClaimTicket, and AudioTicket; plugin window sessions only authorize control and project revisioned state. Completion persists through the existing message center before any bounded native alarm side effect.

**Tech Stack:** Rust 1.96, Tauri 2.11, Windows APIs, TypeScript 7, WebView2, Vitest, Schemars.

Approved source of truth: [2026-08-20-public-plugin-window-timer-api-design.md](../specs/2026-08-20-public-plugin-window-timer-api-design.md).

## Global Constraints

- Extend pre-release `apiVersion: 1` in place; do not add a compatibility layer or change `notifications.publish()` / `notifications.schedule()`.
- `timer.control` is Windows-only and valid only with `ui.window`, `notifications.publish`, and a `submit + window` manifest.
- `window.uipilotPluginWindow.timer` exists for every plugin content window; authorization is enforced in Rust on every call.
- Cross-boundary revisions are canonical decimal `u64` strings and reuse `parseU64Decimal` / `compareU64Decimal`; never convert them to JavaScript `number`.
- Lock order and external-I/O boundaries follow design sections 11.3 and 14 exactly. No lock crosses message I/O, native window work, frontend evaluation, or audio playback.
- The host timer is process-local, counts Windows sleep elapsed time, and is discarded without recovery or replay on process exit.
- Do not modify `package.json`, `package-lock.json`, `packages/plugin-cli/**`, or generated CLI validator artifacts. The validation CLI task consumes the final Timer schema/SDK baseline after this plan completes.
- Automated work must not control the user's mouse, keyboard, or real foreground focus. Real-window and audible-alarm acceptance requires explicit user confirmation.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`.
- Each task uses focused TDD, produces one atomic commit, and must not include pre-existing or unrelated workspace changes.
- The approved design remains authoritative for DTOs, state transitions, linearization, error names, failure behavior, and acceptance; this plan only assigns ownership and verification.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml`. Run frontend and example commands from the repository root.

### Task 1: Freeze Manifest, Schema, and TypeScript Contracts

**Files:** `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/state_tests.rs`, `src/protocol.ts`, create `src/protocol.test.ts`; modify `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/public-plugin-v1.md`

**Dependencies:** Design sections 6-8, 15, 19, and 20.1-20.2.

- [x] Add `PublicPermission::TimerControl` with the exact `timer.control` wire value and enforce the Windows, dependency-permission, output-mode, activation-mode, and window-entry combination rules.
- [x] Preserve the existing exact permission-grant equality rule for install and update; add focused coverage proving a missing grant cannot commit or replace the current generation.
- [x] Add the exact timer DTOs and window API types from design section 7, including non-optional `timer`, canonical `U64Decimal`, nullable first-idle fields, and fixed error names.
- [x] Reuse the existing decimal-u64 parser/comparator in frontend protocol code and cover `9 -> 10`, `99 -> 100`, values above `Number.MAX_SAFE_INTEGER`, `u64::MAX`, and malformed values.
- [x] Regenerate the Rust-owned JSON Schema and update the public v1 contract summary without touching validation CLI outputs.

**Distinct test coverage:** legal three-permission Windows manifest; missing dependency; wrong mode; macOS; unknown permission; exact grants; timer input boundary and unknown-field DTO parsing; decimal revision ordering and rejection.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::manifest && cargo test --manifest-path src-tauri/Cargo.toml public_plugins::state_tests && cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check && npm.cmd test -- src/protocol.test.ts`

### Task 2: Implement the Host Timer State Machine and Scheduler

**Files:** create `src-tauri/src/public_plugins/timers.rs`; modify `src-tauri/src/public_plugins.rs`

**Dependencies:** Task 1; design sections 7.2-12.1, 14-15, 18.2, and 20.2-20.3/20.6.

- [x] Implement the replaceable clock abstraction, production Windows elapsed-time clock using `GetTickCount64`, deterministic test clock, one shared due-time worker, and one timer record per `pluginId + pluginGeneration`.
- [x] Implement `idle | running | paused | claiming | fired`, frozen round data, canonical revision projection, checked internal identities, and failure-closed `TimerUnavailable`.
- [x] Implement Start/Stop/Reset/get-state semantics, including nullable first idle, Reset display duration, pause/resume, idempotency, claiming projection, one queue owner, and no public claiming phase.
- [x] Implement ClaimTicket creation and validation entirely inside the timer service, but leave lifecycle admission, message persistence, and audio effects behind explicit callbacks owned by Task 3.
- [x] Make shutdown terminal, clear queued work, revoke tickets, and expose pure in-memory lifecycle hooks for generation cancellation and session consumers.

**Distinct test coverage:** every transition in design table 11.1; Stop-before-claim and claim-before-Stop barriers; exactly-once claim per round; sleep-style clock advance; wall-clock independence; stale queue entries; revision/identity exhaustion; process shutdown.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::timers::tests`

### Task 3: Integrate Delivery Admission, Message Persistence, Alarm, and Plugin Lifecycle

**Files:** create `src-tauri/src/public_plugins/timer_alarm.rs` and `src-tauri/resources/sounds/timer-complete.wav`; modify `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`

**Dependencies:** Task 2; design sections 11.3, 12-14, 18.2-18.3, and 20.5-20.6; existing `MessagePublisher` / `MessageCenterService` contract.

- [x] Wire the timer worker to delivery admission using the fixed `plugin mutation guard -> timer record` order; failed eligibility must skip message persistence and return a still-current claim to idle.
- [x] Persist admitted completion through `MessagePublisher::commit_publish` outside all plugin, timer, session, and message locks; dispatch existing post-guard message effects independently.
- [x] Add an injectable alarm trait and Windows production adapter using `PlaySoundW` with the bundled finite WAV, `SND_FILENAME | SND_ASYNC | SND_NODEFAULT`, and `PlaySoundW(NULL, NULL, 0)` for best-effort stop. Add only the required `Win32_Media_Audio` Windows crate feature, validate AudioTicket immediately before start, and never roll back `fired` or a saved message for playback failure.
- [x] On committed disable, fault-disable, uninstall, generation replacement, successful upgrade, and shutdown, revoke sessions and cancel the matching timer/tickets/audio before releasing the mutation boundary. Failed upgrade must leave the current generation unchanged.
- [x] Keep rename/settings changes limited to session revocation; retain the timer, frozen name/message data, and any admitted claiming work as specified.

**Distinct test coverage:** lifecycle-before-admission skips the publisher; eligibility failure with a valid ticket returns idle; admission-before-lifecycle may save but cannot fire or sound; Reset-before-admission skips persistence; Reset during persistence preserves a saved message but blocks fired/audio; Reset before/after audio start; failure upgrade preservation; message failure returns idle and never sounds.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml public_plugins::timers::tests`

### Task 4: Add Window Timer Sessions, Commands, and Bootstrap Projection

**Files:** `src-tauri/src/plugin_window.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/plugin-window-content.json`, `src/protocol.ts`, `src/protocol.test.ts`

**Dependencies:** Tasks 1-3; design sections 7.6-9, 14-17, 18.1, and 20.4/20.7.

- [x] Add checked session generations and the `prepared | active | closing | revoked` lifecycle to each plugin content window owner without exposing generation or identity fields to JavaScript.
- [x] Permit only `getState()` and one `onStateChanged()` registration during prepared; activate control only after update ack plus successful native show/focus commit; push one full activation snapshot.
- [x] Add the four narrow Tauri commands and exact content-label, current plugin generation, health, permission, owner, and session guards. Shell, Runtime, main, find, and forged labels must fail before timer state access.
- [x] Inject and deep-freeze the non-optional timer facade in the private bootstrap, keep Tauri internals unavailable, and deliver full private state snapshots only to the current content session.
- [x] Implement frontend session-local revision convergence, equal-revision running-anchor refresh for the latest `getState()` token only, single-handler enforcement, idempotent unsubscribe, and handler exception isolation.
- [x] On hide/close/auto-hide/new invocation/reload/lifecycle change, make the session closing before native work. A failed hide must issue a new session or destroy the window, never revive the old session object.

**Distinct test coverage:** prepare/ack/focus failure never activates; prepared mutation rejection; activation snapshot closes the subscribe/read race; API-call versus hide barrier in both orders; stale Promise/event cannot affect a new session; same-revision restrictions; timer field present but unauthorized; exact capability and command caller matrix.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml plugin_window::tests && cargo test --manifest-path src-tauri/Cargo.toml commands::tests && npm.cmd test -- src/plugin-window-core.test.ts src/protocol.test.ts`

### Task 5: Add the Independent Pomodoro Example and Developer Documentation

**Files:** create `examples/public-plugins/com.uipilot.pomodoro/package/plugin.json`, `examples/public-plugins/com.uipilot.pomodoro/package/icon.png`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.html`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.css`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`, `examples/public-plugins/com.uipilot.pomodoro/README.md`; modify `docs/plugin-sdk/public-plugin-developer-guide.md` and `docs/plugin-sdk/public-plugin-v1.md`

**Dependencies:** Tasks 1-4; design sections 17 and 21-22.

- [x] Build a separate Windows `submit + window` example declaring all three permissions; leave both demo plugins and request-scoped delayed notification semantics unchanged.
- [x] Have Runtime return the initial `10_000` ms and completion message as window data. Render `00:10` locally before the host owns a round; Enter must not auto-start.
- [x] Register the timer subscription before the first state read, merge snapshots by canonical revision, interpolate running display with `performance.now()`, and expose Start/Pause/Resume/Reset controls against the frozen host API.
- [x] Document the permission bundle, session lifetime, Start/Stop/Reset contract, hidden-window continuation, completion ordering, error handling, revision merge rules, process-exit behavior, and install/test commands.
- [x] Add SDK contract and behavior tests covering initial display, explicit start, prepared/active interaction expectations, pause/resume/reset projection, fired display, and existing Demo compatibility.

**Distinct test coverage:** valid example manifest and package paths; Runtime output; TypeScript SDK consumption; no local background authority; initial host idle remains null while UI displays `00:10`; existing `demo-win` and `demo-return` tests remain unchanged and pass.

**Verify:** `node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js && npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts && cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check`

## Final Verification and User Gate

- [ ] Run `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml`.
- [ ] Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings`.
- [ ] Run `npm.cmd test` and `npm.cmd run build`.
- [ ] Confirm the generated Schema and SDK baseline are stable, then hand their exact changes to the validation CLI task; do not let the CLI regenerate from an intermediate Timer contract.
- [ ] Stop and ask the user before the design section 21 manual acceptance. The user operates all real windows, focus, mouse, keyboard, and audible-alarm checks.
