# Pomodoro Duration Selector and Window Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan sequentially. Track work with the checkboxes below.

**Goal:** Add a session-bound private storage facade to public plugin content windows, make complete plugin uninstall recoverable across concurrent writes and cleanup failures, and persist the Pomodoro plugin's selected 10/15/25/30/45-minute duration for the next round.

**Architecture:** A manager-owned `PluginDataCallGate` gives Runtime and Window storage calls one shared uninstall boundary while the existing window call lease continues to own session-phase admission. Complete uninstall becomes a durable, one-way owner-cleanup transaction. The Pomodoro content window treats storage as a next-round preference and keeps the host Timer as the sole authority for the current round.

**Tech Stack:** Rust 1.96, Tauri 2.11, TypeScript 7, WebView2, React 19, Vitest, Node test runner.

Approved source of truth: [2026-08-22-pomodoro-duration-selector-design.md](../specs/2026-08-22-pomodoro-duration-selector-design.md).

## Global Constraints

- Extend pre-release `apiVersion: 1` in place. Add no compatibility layer, Manifest field, or permission.
- Runtime and Window storage share one plugin-private namespace, 5 MiB quota, atomic persistence, and the exact key rule `^[a-z][a-z0-9.-]{0,63}$` plus rejection of `__proto__`, `prototype`, and `constructor`.
- `storage.get` is allowed in Prepared and Active window sessions; `storage.set/remove` are Active-only. Main, find, Runtime, shell, other plugins, stale sessions, and forged labels receive zero Window storage access.
- Window call lease linearizes session/phase admission. `ActivationBundle` validation plus `PluginDataCallLease` signing linearizes storage access.
- Lock order is `plugin mutation -> scheduler -> data gate`; Runtime uses only `scheduler -> data gate`, and Window uses only `plugin mutation -> data gate`. Release all mutexes before waiting for Window/data leases or doing filesystem I/O.
- `admissionEpoch` is a checked process-local `u64`, never wraps or crosses JavaScript, and is captured by the ActivationBundle, Window owner, and Runtime request.
- Complete uninstall is irreversible after the durable `PluginOwnerCleanupReceipt` commit. Until all owner targets are deleted and the receipt is cleared, the same plugin ID cannot install, update, or activate.
- Timer state, `timerRevision`, message delivery, Toast, tray attention, and alarm behavior remain unchanged.
- Automated work must not control the user's mouse, keyboard, foreground focus, Toast UI, or real audio. Manual validation starts only after explicit notice.
- Execute with one implementation Agent in the current workspace. Every commit must exclude all pre-existing native-attention, icon, CodeGraph, Everything, patch, and other unrelated changes.

## Core Contract Overview

Use the exact identities, error mapping, transaction order, and UI state from design sections 4-9:

- `ActivationBundle` adds `admissionEpoch`; `ScheduledPluginRequest` and `PluginWindowOwner` capture it without exposing it to plugin JavaScript.
- `PluginDataCallGate::try_acquire(pluginId, generation, activationId, admissionEpoch)` atomically validates the open epoch and returns a lease covering the complete storage I/O.
- `UiPilotPluginWindowStorageApiV1` exposes frozen `get`, `set`, and `remove` methods using the current decimal session generation captured by bootstrap.
- Window storage errors are exactly `InvalidCaller`, `ExpiredWindowSessionError`, `InvalidOperation`, `StorageError`, and invariant-only `InvalidContext` as assigned in design section 4.
- `PluginOwnerCleanupReceipt` covers storage, secrets, uninstalled state owner, package tree, and window position. `dataCleanupPending` is the stable management command code after an already-committed uninstall cannot finish cleanup.
- `RuntimeRecoveryToken` is the sole process-local owner for rebuilding an absent Runtime after pre-commit abort; a published recovery-needed Bundle is not normally dispatchable until the token commits readiness.
- Pomodoro keeps `effectiveDurationMinutes`, nullable `persistedDurationMinutes`, nullable `pendingDurationMinutes`, and host Timer state as separate values.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4`.
- Each task uses focused TDD, ends with one atomic commit, and receives specification/compliance review before the next dependent task begins. Repeated red/green/commit mechanics are not restated.
- The approved design is authoritative for authorization, linearization, lock order, rollback, failure behavior, and acceptance. This plan assigns implementation ownership without weakening those contracts.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml`. Run frontend and example commands from the repository root.

### Task 1: Establish Recoverable Uninstall and Startup Recovery

**Files:** create `src-tauri/src/public_plugins/owner_cleanup.rs`; modify `src-tauri/src/public_plugins/activation_bundle.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/secrets.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugin_window.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src/protocol.ts`, `src/main.ts`, `src/public-plugin-panel.tsx`, `src/launcher.test.tsx`

**Dependencies:** Design sections 6.1, 8, 9.1, 9.4, and acceptance item 11.6. This task must leave the existing Runtime storage path safe before any Window storage command is introduced.

- [ ] Add the checked process-local `admissionEpoch` allocator. Capture the epoch in ActivationBundle and ScheduledPluginRequest, preserve it for config-only replacement, and allocate a new epoch for activation replacement or failed-uninstall recovery.
- [ ] Split the current uninstall into explicit `begin`, `commit`, `finish_cleanup`, and `abort_before_commit` transaction phases. `begin` closes manager/window-transfer admission and invalidates the Runtime scheduler while holding only the approved lock order; because current Runtime storage executes inside `scheduler.with_current`, scheduler invalidation must wait for an already-running storage closure before proceeding.
- [ ] Make the command-side online order exact: begin and drain the current Window/Timer session; durably commit `PluginOwnerCleanupReceipt` and publish no Bundle; destroy the Runtime and plugin window; only then delete storage, secrets, uninstalled state owner, package tree, and window position; clear the receipt last. A late `Moved` callback from the destroyed window must not recreate a position after receipt clear.
- [ ] Persist receipts outside all target roots using only validated backend identities and fixed-root target keys. Missing targets are success; any target or receipt-clear failure leaves the receipt and same-ID install/update/activation block intact.
- [ ] On a failure before durable commit, keep uninstall admission closed while the command destroys the old Runtime/window. Then `abort_before_commit` publishes the still-installed Bundle identity under a new epoch with `Runtime absent / recovery needed`; normal scheduler dispatch remains closed, and abort does not create a Runtime. After durable commit, never restore the plugin; return `PublicPluginManagementError::DataCleanupPending` / `CommandError.code = dataCleanupPending`.
- [ ] Before enqueue/dispatch, the next command for a recovery-needed Bundle must acquire the unique `RuntimeRecoveryToken` for its exact plugin/generation/activation/epoch. The token owner stages one candidate and runs the existing Runtime readiness path; concurrent commands share that attempt or are superseded and never create another Runtime. Ready completion must CAS the same token before marking Runtime present, opening scheduler admission, and allowing only the current command owner to dispatch. Readiness failure uses the same token to run the existing `mark_runtime_unavailable`/fault-disable transition and dispatches no command. Stale success/failure cannot affect a later token or epoch.
- [ ] Freeze recovery submission completion as latest-wins. Every command keeps its own registered receiver, but only the current submission token may dispatch after readiness. When B supersedes A, atomically remove A from `by_token` and `token_by_request` and send `None` exactly once to A before B proceeds. Runtime readiness failure/fault-disable sends `None` exactly once to every waiter and removes every associated index entry. Dropped senders, token cancellation, and stale readiness completion must also retire their owned entries; no receiver may remain pending after the recovery attempt reaches ready, failed, cancelled, or superseded.
- [ ] Freeze startup recovery in `PublicPluginService::initialize`: after `SettingsStore` is loaded and available, but before `PublicPluginManager::load` reads state or constructs any ActivationBundle, load and retry receipts. Any uncleared receipt ID enters the blocked set; even a crash snapshot whose state owner still says `installed=true` must not create a Bundle.
- [ ] Preserve retain-data behavior: retain storage, secrets, and retained state config while removing package, Runtime/window, and position. Make normal restart and successful upgrade preserve data, and make complete uninstall followed by same-ID reinstall start empty.
- [ ] Add a root `tests::public_plugin_cleanup_recovery_precedes_activation` wiring test for `SettingsStore -> receipt recovery -> manager load/bundle construction`, plus plugin-window ordering coverage proving teardown completes before position cleanup.
- [ ] In the settings UI, treat `dataCleanupPending` as committed uninstall: end loading, refresh inventory, remove the row, and show “插件已卸载，数据清理将在下次启动时重试” at page level instead of “操作不可用”.

**Distinct test coverage:** admissionEpoch allocator exhaustion never wraps; config-only replacement preserves its epoch while new activation and abort replacement allocate a non-reused epoch; pause existing Runtime storage inside `scheduler.with_current`, begin uninstall, prove durable commit waits for the closure, then prove no deleted root is recreated; exact online order proves Runtime/window destruction precedes position cleanup and a late move cannot rewrite it; pre-commit abort rejects the old request/facade and leaves Runtime recovery-needed; two concurrent subsequent commands create one Runtime/one Bundle owner, A is superseded and receives `None`, B alone dispatches after ready and later receives its normal terminal result, and both submission indexes end empty; stale readiness completion cannot open a later token; readiness failure fault-disables once, both receivers receive `None`, and both indexes end empty; receipt with state still marked installed is processed before bundle construction; storage succeeds while secret/state/package/position cleanup individually fail, keeping the block; receipt-clear failure remains blocked until a later native startup clears it; ordinary restart and successful upgrade preserve a stored value; retain-data reinstall restores it; complete uninstall cleanup plus same-ID reinstall reads no value; frontend pending-cleanup behavior ends busy state and reloads inventory.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation_bundle::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::owner_cleanup::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_window::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib tests::public_plugin_cleanup_recovery_precedes_activation && cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests && npm.cmd test -- src/launcher.test.tsx`

### Task 2: Add Shared Data Admission and Runtime Storage Contract

**Files:** create `src-tauri/src/public_plugins/data_call_gate.rs`; modify `src-tauri/src/public_plugins/activation_bundle.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/storage_tests.rs`, `src-tauri/src/public_plugins.rs`

**Dependencies:** Task 1; design sections 4, 6, 6.1, and 9.1. Gate integration and uninstall close/drain are one atomic task so no commit can expose a lease that uninstall ignores.

- [ ] Add the per-plugin `PluginDataCallGate` and RAII lease keyed by `pluginId + generation + activationId + admissionEpoch`. Config-only replacement preserves the gate; replacement/disable/recovery changes or closes it.
- [ ] In `scheduler.with_current`, read activationId/epoch from the guarded request and call `data_gate.try_acquire` directly without mutation. Hold the lease through complete Runtime storage I/O; never leave the current guard and then acquire a lease.
- [ ] Extend Task 1's uninstall `begin` phase to acquire `mutation -> scheduler -> data gate`, close new data admission, release all mutexes, then drain existing leases before durable commit. Never hold scheduler/data-gate mutex while waiting for a lease.
- [ ] Replace the prototype-only key check with the shared regex-plus-prototype validator. Quarantine loaded invalid documents; map key/value failures to `InvalidOperation` and quota/serialization/filesystem failures to `StorageError`.

**Distinct test coverage:** the gate preserves admission only for the exact Task 1 epoch and rejects every old/replaced epoch; Runtime current guard racing gate close has no deadlock; data lease covers full storage I/O; uninstall waits an already-signed lease but rejects a request that has not signed one; table-driven key/value/quota errors have exact classes and preserve the old document.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::data_call_gate::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::scheduler::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::runtime::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::storage_tests`

### Task 3: Expose Session-Bound Window Storage

**Files:** modify `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugin_window.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/plugin-window-content.json`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`; generate `src-tauri/permissions/autogenerated/plugin_window_storage_get.toml`, `src-tauri/permissions/autogenerated/plugin_window_storage_set.toml`, `src-tauri/permissions/autogenerated/plugin_window_storage_remove.toml`

**Dependencies:** Tasks 1-2; design sections 4-6, 8, 9.1-9.2. The safe uninstall/data-gate boundary is complete before this task exposes mutable Window storage.

- [ ] Generalize `TimerCallLease` and its in-flight counter into the shared Window session/call lease without creating a second state machine or changing Timer projection.
- [ ] Capture admissionEpoch in `PluginWindowOwner`. Allow read leases in Prepared/Active and mutable leases only in Active; Closing/Revoked and stale decimal session generations fail before manager or storage access.
- [ ] Add manager-owned `window_storage_get/set/remove` methods in `activation.rs`. Each method validates the exact Bundle/owner/epoch and signs the data lease atomically under the manager mutation boundary, releases manager mutexes, then performs storage I/O using the backend-derived scope while the lease remains alive. Commands must not inspect manager-private Bundle, mutation, gate, scope, or storage fields.
- [ ] Add the three narrow commands. After Window admission, call the corresponding manager method while retaining the Window lease; the manager method owns data admission and storage I/O, then the command releases the Window lease and returns the fixed DTO/error.
- [ ] Add each command to `src-tauri/build.rs`, invoke allowlists, content capability, generated permissions, exact caller/error mapping, and the deeply frozen bootstrap storage facade whose methods capture the current session generation without caller-supplied scope. Extend the root capability test to prove all three commands appear in the build manifest and only the content capability.
- [ ] Publish the exact TypeScript API and documentation. Extend the Pomodoro SDK contract to type-check `storage.get`, `storage.set`, `storage.remove`, `JsonValue`, and the readonly Window storage facade; keep both Demo SDK contracts passing.

**Distinct test coverage:** Prepared get succeeds while writes fail; Active permits all three; a Window call holding only its first lease is rejected by concurrent gate close; an admitted write finishes while hide/close and uninstall wait; stale facade/session/epoch and every invalid caller produce the exact error with zero access; Runtime set -> Window get and Window set -> Runtime get share one namespace, while another plugin reading the same key gets `null`; bootstrap objects are frozen; all three build/capability permissions are exact; the SDK contract actually calls all three storage methods and checks the nullable read type.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_window::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib tests::public_plugin_commands_have_non_overlapping_exact_capabilities && npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`

### Task 4: Persist and Apply the Pomodoro Duration Selector

**Files:** modify `examples/public-plugins/com.uipilot.pomodoro/package/plugin.json`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.html`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.css`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`, `examples/public-plugins/com.uipilot.pomodoro/README.md`

**Dependencies:** Tasks 1-3; design sections 3-5, 7-10.

- [ ] Bump the reference plugin version and render the fixed 10/15/25/30/45-minute selector in the content area's top-right without changing Manifest settings or permissions.
- [ ] Stop treating Runtime's development-only 10-second value as the next-round duration. Keep only the invocation-derived completion message in window data; initialize effective duration to 10 minutes in content.
- [ ] On each `onUpdate`, create a new view epoch, invalidate the prior save token and prior Timer ownership, dispose the prior Timer subscription, set `pendingDurationMinutes=null`, clear the prior save error, and set `durationReadPending=true`; show 10 minutes and disable the selector while the initial storage read owns the view. Subscribe to Timer first, then read Timer baseline and `pomodoro.duration-minutes` in parallel. Only the current epoch may settle a storage read/save, Timer baseline/control Promise, or Timer subscription callback and update DOM, error, pending, or projected Timer state; an already-queued callback from a disposed subscription must still fail the epoch check.
- [ ] Keep effective, persisted, pending, and host Timer state separate. Missing/invalid/read-failed storage projects 10 without writing it; successful selection atomically persists the exact allowed integer; failed save restores persisted or 10.
- [ ] Show effective duration while idle and the host's current-round duration/remaining while running, paused, or fired. A new idle/fired round uses effective minutes; paused resume remains argument-free.
- [ ] Disable the selector plus idle Start/fired Restart while a save is pending; allow paused Resume. A duration change during running/paused never changes the current round or timer revision.
- [ ] Update example tests, SDK contract, and README for persistence, next-round semantics, failure recovery, and the required reinstall after the example version changes.

**Distinct test coverage:** exact options/order/labels and default 10 minutes; during initial read the selector displays 10 and is disabled; legal values restore while missing/invalid values remain unpersisted 10; a read from an old view epoch cannot overwrite a newer update or selection; a new update during an old save clears pending ownership, and the old save's later resolve or reject changes neither DOM, error text, nor the new pending state; after a second `onUpdate`, the old epoch's `timer.getState()` completion, Timer control resolve/reject, and already-queued subscription callback cannot change the new view's DOM, error, pending, or projected Timer state; save pending blocks idle/fired start but not paused resume; save success starts 25 minutes and failure starts the restored value; running/paused selection leaves current state/revision untouched; a reconstructed content model reads mocked persisted state after close/reopen. Rust coverage in Task 1 owns process restart, upgrade, retain-data reinstall, and full-uninstall reinstall.

**Verify:** `node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js && npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`

## Final Verification and User Gate

- [ ] Run `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml`.
- [ ] Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings`.
- [ ] Run `npm.cmd test` and `npm.cmd run build`.
- [ ] Run `node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js`.
- [ ] Run `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts examples/public-plugins/com.uipilot.demo-return/tests/sdk-contract.ts`.
- [ ] Confirm the diff contains only the four task commits and no pre-existing user/Agent changes.
- [ ] Notify the user before manual validation. The user reinstalls the updated Pomodoro package, operates all windows and controls, checks persistence across reopen/restart/upgrade/retain-data/full-uninstall, and confirms current-round versus next-round behavior. The Agent never controls mouse or keyboard.
