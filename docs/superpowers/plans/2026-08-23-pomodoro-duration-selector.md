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
- Pomodoro keeps `effectiveDurationMinutes`, nullable `persistedDurationMinutes`, nullable `pendingDurationMinutes`, and host Timer state as separate values.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4`.
- Each task uses focused TDD, ends with one atomic commit, and receives specification/compliance review before the next dependent task begins. Repeated red/green/commit mechanics are not restated.
- The approved design is authoritative for authorization, linearization, lock order, rollback, failure behavior, and acceptance. This plan assigns implementation ownership without weakening those contracts.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml`. Run frontend and example commands from the repository root.

### Task 1: Add Shared Storage Admission and Runtime Integration

**Files:** create `src-tauri/src/public_plugins/data_call_gate.rs`; modify `src-tauri/src/public_plugins/activation_bundle.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/storage_tests.rs`, `src-tauri/src/public_plugins.rs`

**Dependencies:** Design sections 4, 6, 6.1, and 9.1.

- [ ] Add a checked admission-epoch allocator and per-plugin `PluginDataCallGate`/RAII lease. Bind each published ActivationBundle, scheduled Runtime request, and later Window owner to the exact `pluginId + generation + activationId + admissionEpoch` tuple.
- [ ] Make config-only bundle replacement preserve the epoch; activation replacement, failure recovery, disable/uninstall, and a reopened failed transaction must close or replace it exactly as specified.
- [ ] Extend `scheduler.with_current()` access so Runtime storage reads the captured activationId/epoch while holding the current-request guard and directly calls `data_gate.try_acquire` without acquiring the mutation gate.
- [ ] Hold the data lease through the complete Runtime storage operation, map invalid key/value to `InvalidOperation`, map quota/serialization/storage failures to `StorageError`, and leave notification/settings behavior unchanged.
- [ ] Replace the prototype-only key check with the one shared Runtime/Window validator. Quarantine loaded documents containing any newly invalid key instead of adding legacy compatibility.

**Distinct test coverage:** checked epoch exhaustion; config-only preservation versus new-activation replacement; Runtime current guard concurrently racing gate close with no deadlock; an old request cannot acquire a new epoch; data lease remains counted through storage I/O; `9 -> 10`-style identity progression never wraps; table-driven Runtime/storage rejection of regex failures and all three prototype keys with exact error classes.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::data_call_gate::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::scheduler::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::runtime::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::storage_tests`

### Task 2: Expose Session-Bound Window Storage

**Files:** modify `src-tauri/src/plugin_window.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/plugin-window-content.json`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`; generate `src-tauri/permissions/autogenerated/plugin_window_storage_get.toml`, `src-tauri/permissions/autogenerated/plugin_window_storage_set.toml`, `src-tauri/permissions/autogenerated/plugin_window_storage_remove.toml`

**Dependencies:** Task 1; design sections 4-6, 8, 9.1-9.2.

- [ ] Generalize `TimerCallLease` and the Timer session in-flight counter into the shared Window session/call lease without creating a second session state machine. Preserve all existing Timer behavior and event projection.
- [ ] Capture admissionEpoch in `PluginWindowOwner`. Permit read leases in Prepared/Active and mutable leases only in Active; Closing/Revoked and stale decimal session generations fail before manager or storage access.
- [ ] Add the three narrow Tauri commands. After the Window lease, atomically validate the current ActivationBundle and sign a data lease under the mutation/data-gate order, release all manager/controller locks, then call `PluginStorageStore`.
- [ ] Add the command allowlist, content capability, generated permissions, exact caller/error mapping, and focused guard tests for content versus shell/main/find/Runtime/other-plugin/forged labels.
- [ ] Extend `PUBLIC_CONTENT_BOOTSTRAP` with a deeply frozen non-optional storage facade whose methods capture the current session generation. Do not expose Tauri internals, plugin identity, epoch, activation, or paths.
- [ ] Publish the exact TypeScript API and update the public v1/developer guide with Prepared-read/Active-write, lifecycle expiry, shared namespace, key/value/quota, and uninstall semantics.

**Distinct test coverage:** Prepared `get` succeeds while Prepared `set/remove` fail; Active permits all three; a Window lease paused before data admission is rejected by a concurrent gate close; an admitted write finishes while hide/close waits; a stale facade cannot cross session or epoch; malformed session and each caller class produce the exact stable error with zero storage access; bootstrap objects/methods are frozen and capture no caller-supplied scope.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_window::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests && npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`

### Task 3: Make Complete Uninstall Recoverable

**Files:** create `src-tauri/src/public_plugins/owner_cleanup.rs`; modify `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/storage.rs`, `src-tauri/src/public_plugins/secrets.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src/protocol.ts`, `src/main.ts`, `src/public-plugin-panel.tsx`, `src/launcher.test.tsx`

**Dependencies:** Tasks 1-2; design sections 6.1, 8, 9.1, 9.4, and acceptance item 11.6.

- [ ] Split the current uninstall path into close-admission, Window/data drain, durable publish, owner cleanup, and receipt-clear phases. Implement the unique `mutation -> scheduler -> data gate` acquisition order and never wait for leases while holding those locks.
- [ ] Persist `PluginOwnerCleanupReceipt` outside every target owner root before publishing the no-Bundle state. Store only validated backend identities and fixed-root target keys, never caller paths.
- [ ] Make complete cleanup idempotently remove ordinary storage state/directory, secret owner, uninstalled state owner, installed package tree, and saved window position. Clear the receipt and report success only after every target succeeds; missing targets count as success.
- [ ] On pre-commit failure, destroy the old Runtime/window and reopen only a newly validated admission epoch for the still-installed plugin. On post-commit cleanup failure, never restore the plugin; retain the receipt, block the same plugin ID, and retry during native process startup.
- [ ] Preserve the existing retain-data contract: retain storage, secrets, and retained state configuration while removing package, Runtime/window, and saved position through the existing retained-uninstall lifecycle.
- [ ] Add `PublicPluginManagementError::DataCleanupPending`, serialize `CommandError.code` as `dataCleanupPending`, and make the settings page finish loading, refresh inventory, remove the plugin row, and show the fixed page-level cleanup message rather than “操作不可用”.

**Distinct test coverage:** paused Window and Runtime writes both complete before durable uninstall and cannot recreate deleted roots; a Window call with only the first lease is rejected; pre-commit drain/durable failures reopen a new epoch without reviving old requests; storage succeeds while each of secret/state/package/position cleanup fails, leaving the receipt and same-ID block; restart retries all targets idempotently and clears the block only at full success; retain-data reinstall recovers the duration value; frontend `dataCleanupPending` ends busy state, reloads inventory, removes the row, and renders the fixed page-level notice.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::owner_cleanup::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests && npm.cmd test -- src/launcher.test.tsx`

### Task 4: Persist and Apply the Pomodoro Duration Selector

**Files:** modify `examples/public-plugins/com.uipilot.pomodoro/package/plugin.json`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.html`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.css`, `examples/public-plugins/com.uipilot.pomodoro/package/dist/window.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts`, `examples/public-plugins/com.uipilot.pomodoro/README.md`

**Dependencies:** Tasks 1-3; design sections 3-5, 7-10.

- [ ] Bump the reference plugin version and render the fixed 10/15/25/30/45-minute selector in the content area's top-right without changing Manifest settings or permissions.
- [ ] Stop treating Runtime's development-only 10-second value as the next-round duration. Keep only the invocation-derived completion message in window data; initialize effective duration to 10 minutes in content.
- [ ] On each `onUpdate`, create a new view epoch, subscribe to Timer state first, then read Timer baseline and `pomodoro.duration-minutes` in parallel. Only the current epoch may update DOM/error state.
- [ ] Keep effective, persisted, pending, and host Timer state separate. Missing/invalid/read-failed storage projects 10 without writing it; successful selection atomically persists the exact allowed integer; failed save restores persisted or 10.
- [ ] Show effective duration while idle and the host's current-round duration/remaining while running, paused, or fired. A new idle/fired round uses effective minutes; paused resume remains argument-free.
- [ ] Disable the selector plus idle Start/fired Restart while a save is pending; allow paused Resume. A duration change during running/paused never changes the current round or timer revision.
- [ ] Update example tests, SDK contract, and README for persistence, next-round semantics, failure recovery, and the required reinstall after the example version changes.

**Distinct test coverage:** exact options/order/labels and default 10 minutes; legal saved values restore while missing/invalid values remain unpersisted 10; save pending blocks idle/fired start but not paused resume; save success starts 25 minutes and failure starts the restored value; running/paused selection leaves current state/revision untouched; stale read/save/Timer completions cannot mutate a new view epoch; a reconstructed content model reads the mocked persisted value after close/reopen. Rust coverage in Task 3 owns process restart, upgrade, retain-data reinstall, and full-uninstall reinstall.

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
