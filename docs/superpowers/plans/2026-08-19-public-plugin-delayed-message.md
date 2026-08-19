# Public Plugin Delayed Message Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a host-owned, process-local `notifications.schedule()` API so `/demo-win str` opens its child window immediately and publishes the matching message 10 seconds later even when both windows are hidden.

**Architecture:** A single Rust scheduler owns an ordered in-memory queue and one worker. The request-bound Runtime API validates and atomically hands a plain-text message to that scheduler; delivery later revalidates plugin generation and reuses the existing message-center commit/effects path. No plugin code survives the command request, and no scheduled task survives process exit.

**Tech Stack:** Rust, Tauri 2, TypeScript API declarations, JavaScript reference plugin, existing message-center adapters.

**Approved specification:** [2026-08-19-public-plugin-delayed-message-design.md](../specs/2026-08-19-public-plugin-delayed-message-design.md), especially sections 5-9 for the API, queue, linearization, cancellation, and failure contracts.

## Global Constraints

- Windows-only capability; reuse the existing `notifications.publish` permission.
- `delayMs` is a JavaScript safe integer in `1_000..=86_400_000`.
- Each plugin may have at most 32 pending delayed messages.
- One request may submit only one immediate or delayed notification action.
- Successful queue insertion is irreversible for request supersession, but plugin disable, uninstall, or generation replacement cancels pending work.
- Window hide/close never cancels a task; native process exit clears all tasks without recovery.
- Queue/request/plugin locks never cross message persistence, frontend events, Windows notifications, or tray effects.
- Automated verification must not synthesize input or change real foreground focus. Manual Windows acceptance requires explicit user action.
- Preserve all pre-existing working-tree changes and stage only task-owned hunks.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3`.
- Each task follows focused TDD: add the owning tests, confirm the intended failure, implement the minimum approved contract, rerun focused tests, and create one atomic commit without unrelated changes.
- Rust commands use `--manifest-path src-tauri/Cargo.toml`; JavaScript commands run from the repository root.
- Run the full Rust and frontend regression suites once after Task 3, not after every task.
- This plan is executed inline by the primary Agent, matching the user's single-Agent preference.

### Task 1: Process-Local Delayed Message Scheduler

**Files:**
- Create: `src-tauri/src/public_plugins/delayed_messages.rs`
- Modify: `src-tauri/src/public_plugins.rs`

**Dependencies:** Design sections 6, 8, and 9.

- [ ] Add immutable scheduled-message and registration types carrying internal schedule ID, plugin ID, generation, name snapshot, request ID, content, and monotonic due time.
- [ ] Implement one shared ordered queue with `schedule`, plugin-wide cancellation, due-task claiming, worker startup, and terminal shutdown. Use one worker and wake it when an earlier deadline is inserted.
- [ ] Enforce the inclusive 1-second/24-hour delay bounds and 32-pending-per-plugin quota before insertion; failed insertion must not consume quota or an ID.
- [ ] Ensure claiming removes a task exactly once, cancellation wakes the worker, shutdown clears pending tasks, and process-local state has no persistence path.

**Distinct test coverage:** Table-driven delay boundaries; the 33rd pending task is rejected without leaking quota; two plugins have independent quotas; earlier insertion wakes/reorders the queue; multiple due tasks are each claimed once; cancelling one plugin does not affect another; shutdown prevents later delivery. All timing assertions use injected `Instant` values or direct due claiming rather than waiting 10 seconds.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::delayed_messages::tests`

### Task 2: Runtime API And Plugin Lifecycle Integration

**Files:**
- Modify: `src-tauri/src/public_plugins/runtime.rs`
- Modify: `src-tauri/src/public_plugins/activation.rs`
- Modify: `src-tauri/src/public_plugins.rs`
- Modify: `src-tauri/src/message_center.rs`
- Modify: `src-tauri/src/lib.rs`

**Dependencies:** Task 1; design sections 5, 7, 8, and 9.

- [ ] Extend the bootstrap and Rust wire DTO with `notificationSchedule`, `{ content, delayMs }`, `InvalidDelay`, and `ScheduleLimitExceeded`; deep-freeze the input snapshot exactly like `publish`.
- [ ] Inject the delayed scheduler into `PluginRuntimeApi`. Under the current-request guard, validate caller/context/permission/content/store availability/delay/quota, insert the task, then mark the existing shared notification-submission flag. Keep `publish` and `schedule` mutually exclusive through `AlreadyPublished`.
- [ ] Start the scheduler only after `PublicPluginManager` is held by `Arc`. At delivery, release the queue lock, revalidate installed/enabled/fault-free/current generation plus granted permission, then call the existing message-center commit and post-guard effect path.
- [ ] Cancel pending tasks at every plugin disable, uninstall, runtime fault, and generation replacement boundary. Eligibility revalidation is the delivery linearization point; a task claimed while still eligible may finish even if a later mutation begins.
- [ ] Add service shutdown and call it before message-center native-effect shutdown on `RunEvent::Exit`; hidden windows and prevented exit requests must not call it.

**Distinct test coverage:** Forged caller/context and missing permission fail before queue access; invalid delay/content and unavailable store leave no task; `publish -> schedule`, `schedule -> publish`, and `schedule -> schedule` allow only the first action; request A schedules, request B supersedes A, and A still delivers; request invalidation before insertion leaves nothing; disable/uninstall/update before eligibility claim cancels; generation changes after successful eligibility claim do not roll back an in-progress commit; ordinary store failure drops without retry while `BecameUnavailable` dispatches the existing terminal event; window hide has no scheduler call; clean process exit shuts the scheduler down before message-center effects.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::` and `cargo test --manifest-path src-tauri/Cargo.toml message_center::tests`

### Task 3: SDK Contract And Delayed `demo-win`

**Files:**
- Modify: `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`
- Modify: `docs/plugin-sdk/public-plugin-v1.md`
- Modify: `docs/plugin-sdk/public-plugin-developer-guide.md`
- Modify: `examples/public-plugins/com.uipilot.demo-win/package/plugin.json`
- Modify: `examples/public-plugins/com.uipilot.demo-win/package/dist/runtime.js`
- Modify: `examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js`
- Modify: `examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts`
- Modify: `examples/public-plugins/com.uipilot.demo-win/README.md`

**Dependencies:** Task 2; design sections 4, 5, 10, and 11.

- [ ] Publish the exact readonly `PluginNotificationScheduleInput` and `notifications.schedule()` type. Document acceptance semantics, 1-second/24-hour bounds, quota, request ownership, cancellation, process-exit loss, and the distinction from arbitrary background code.
- [ ] Bump `com.uipilot.demo-win` from `1.0.3` to `1.0.4`; replace immediate `publish` with one awaited `schedule({ content: returnText, delayMs: 10_000 })`, then return the existing window response immediately.
- [ ] Update Demo mocks and contract tests to assert one exact schedule call, immediate response, matching `Return text`, fixed 10-second delay, and rejection propagation. Leave `demo-return` unchanged.

**Distinct test coverage:** Manifest version/permissions remain exact; Runtime schedules once with the expected DTO and returns without waiting for delivery; scheduling rejection prevents a window response; TypeScript accepts the new readonly API while retaining immediate publish.

**Verify:** `node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js` and `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts`

## Final Verification

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] `npm.cmd test`
- [ ] `npm.cmd run build`
- [ ] Confirm the working tree still contains all unrelated user changes and the feature commits contain only delayed-message work.
- [ ] Ask the user before manual acceptance. The user runs `/demo-win str`, closes/hides both windows, waits about 10 seconds, and checks the Windows notification, tray reminder, unread badge, independent double submission, and cancellation after disabling/updating the plugin.
