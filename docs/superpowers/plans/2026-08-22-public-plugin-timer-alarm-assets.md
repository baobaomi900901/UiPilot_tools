# Public Plugin Timer Alarm Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the ordinary message sound host-owned while moving each `timer.control` alarm into the plugin package as a strictly validated, host-private, immutable WAV asset that remains correctly bound across install, update, timer completion, focus, and teardown races.

**Architecture:** Package preparation separates public Web resources from a private `ValidatedAlarmAsset`. An atomic `ActivationBundle` publishes config, Runtime, activation identity, and alarm bytes together. Timer tickets freeze that exact activation and asset. The native attention coordinator grants one epoch owner, while a single Windows audio adapter serializes all `PlaySoundW` calls and owns any memory still reachable by WinMM.

**Tech Stack:** Rust 1.96, Tauri 2.11, `windows` 0.61, `webview2-com` 0.38, Win32 `PlaySoundW`, existing Rust and Vitest suites.

Approved source of truth: [2026-08-22-public-plugin-timer-alarm-assets-design.md](../specs/2026-08-22-public-plugin-timer-alarm-assets-design.md).

## Global Constraints

- Public JavaScript API, Manifest schema, permissions, Timer DTOs, and message DTOs remain unchanged.
- `resources/sounds/message-notification.wav` stays in the host and plays once for ordinary messages. A `timer.control` plugin must provide exactly `assets/sounds/timer-alarm.wav`; there is no host alarm fallback.
- The alarm participates in package staging and digest validation but never enters Runtime/window resource maps. Its protocol path always returns `403`, and plugin CSP includes `media-src 'none'`.
- Every public-plugin Runtime and content WebView must complete the inert-URL native mute handshake before any plugin-controlled navigation or script can run. The main WebView is unaffected.
- `activationId` and `alarmEpoch` are process-local checked `u64` identities that never wrap or reuse within a process.
- Lock order is `plugin mutation -> window session -> timer -> ActivationBundle`. Native audio, WebView operations, attention admission, filesystem I/O, and message persistence do not run while these locks are held, except where the approved design explicitly defines a narrower linearization closure.
- Windows audio uses only validated in-memory WAV bytes for Timer alarms. The adapter is the sole serialization point for ordinary play, Timer start/stop, and cleanup.
- Automated tests must not show Toasts, move foreground focus, synthesize input, or play real audio. The final Windows/audio/focus check is user-operated after explicit notice.
- Execute sequentially in the current workspace with one implementation Agent. Preserve all pre-existing modified and untracked files unless a task explicitly owns them.

## Core Contract Overview

Use the exact identities and state machines from design sections 3 and 8-12:

- `AlarmAssetIdentity` binds `pluginId + pluginGeneration + activationId + packageDigest + resourceSha256 + fixedRelativePath`.
- `ValidatedAlarmAsset` is that identity plus immutable `Arc<[u8]>` bytes.
- `ActivationBundle` atomically exposes config, public Runtime snapshot, optional private alarm, generation, and activationId.
- Activation reservation follows `Reserved -> Committing -> Durable -> Published`; the durable helper returns only `NotCommitted | Committed | Unknown`.
- `TimerKey` includes `pluginId + activationId`; completion carries its full `AudioTicket`, frozen alarm identity, and frozen bytes.
- `timerAudioOwner` follows `reserved -> playing` within a monotonic `alarmEpoch`. The first valid ticket owns the epoch; later tickets do not mix, switch, retry, or become fallback owners.
- `WindowsAudioAdapter` owns `PlayingMemory`. A failed stop moves the buffer to process-static `ProcessAudioQuarantine` and makes the whole native audio adapter terminal.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6`.
- Each task uses focused TDD, ends with one atomic commit, and excludes unrelated workspace changes. Repeated red/green/commit mechanics are not restated below.
- The approved design is authoritative for interfaces, ordering, rollback, lock boundaries, terminal states, and acceptance. The plan assigns implementation ownership without weakening those contracts.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml`.
- Do not start the manual Windows/audio/focus gate until all automated checks in Task 6 pass.

### Task 1: Validate and Separate the Host-Private Alarm Asset

**Files:** create `src-tauri/src/public_plugins/alarm_asset.rs`, create `examples/public-plugins/com.uipilot.pomodoro/package/assets/sounds/timer-alarm.wav`; modify `src-tauri/src/public_plugins/package.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/tests.rs`

**Dependencies:** Design sections 5-7, 13.1, 16.1-16.2.

- [ ] Add fixed alarm path and identity/asset preparation types, and implement the exact checked RIFF/WAVE PCM parser from sections 6.1-6.2.
- [ ] Copy the approved host alarm into the Pomodoro package at the fixed private path before enforcing the new package rule, so the reference package and intermediate development state remain installable.
- [ ] Make directory and archive staging enforce the `timer.control` presence/absence rule, reject every other WAV, and include the private alarm in file/count/size/digest/revalidation checks.
- [ ] Copy development hardlinks into an ordinary staging file with link count one; continue to reject symlinks, reparse points, non-files, and staged multi-link files.
- [ ] Split the prepared snapshot into public resources and the validated private alarm so `asset()` and `window_asset()` can never return the alarm; return `403` for its fixed path without revealing load state.

**Distinct test coverage:** table-driven valid directory/archive packages; missing/wrong-case/extra/non-timer WAV rejection; exact RIFF chunk order, padding, length arithmetic, PCM equations, duration and size boundaries; source hardlink becomes a single-link staging file; private alarm affects digest/revalidation but is absent from public resource enumeration and always returns `403`.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::alarm_asset::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests`

### Task 2: Publish Atomic Activation Bundles and Non-Reusable Identities

**Files:** create `src-tauri/src/public_plugins/activation_bundle.rs`; modify `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`, `src-tauri/src/public_plugins.rs`

**Dependencies:** Task 1; design sections 8-9, 13.3, 16.3-16.4.

- [ ] Replace independently observed config/Runtime/alarm state with complete `ActivationBundle` snapshots and a process-global checked activationId allocator; add the bundle-owned alarm registry keyed by activationId.
- [ ] Implement the per-plugin activation reservation and the exact `Reserved -> Committing -> Durable -> Published` transitions, including a durable state helper whose only outcomes are `NotCommitted`, `Committed`, and `Unknown`.
- [ ] Preserve the old Bundle only when `NotCommitted` is followed by a successful old-digest revalidation. Treat `Unknown`, ambiguous durable state, Committing/Durable unwind, poison, or publication failure as terminal for the public-plugin service.
- [ ] Apply the mutation matrix: install/update/re-enable/fault recovery/replacement get a new activationId and revoke the old runtime authority; disable/uninstall/fault-disable remove it; rename/settings atomically replace config only and preserve generation, activationId, Runtime, alarm, Timer, and owner.
- [ ] Reorder uninstall and successful replacement so Bundle/timer/owner authority is revoked at the in-memory linearization point before lock-free window destruction, native attention effects, and package deletion.

**Distinct test coverage:** uninstall then reinstall can reuse generation `1` but never activationId; old cancel/publish/fault results cannot affect the new Bundle; reservation blocks concurrent mutations while readers retain the complete old Bundle; failure injection before durable replace, inside each helper outcome, between `Committed` and `Durable`, and after `Durable`; config-only rename/settings preserve an active Timer and owner; cleanup failure after publication does not roll back the new Bundle.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation_bundle::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::state_tests`

### Task 3: Enforce Pre-Navigation Native WebView Muting

**Files:** create `src-tauri/src/public_plugins/webview_audio_guard.rs`; modify `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugin_window.rs`, `src-tauri/Cargo.toml`

**Dependencies:** Task 2; design sections 7.2, 9.1 item 6, 13.2, 16.2.

- [ ] Create public-plugin Runtime and content WebViews on an inert host-owned URL with no plugin initialization script or controllable resource loaded.
- [ ] Behind a fakeable Windows boundary, obtain `ICoreWebView2_8`, call `SetIsMuted(true)`, read back true, register `IsMutedChanged`, and complete a bounded readiness handshake before native navigation to the plugin URL.
- [ ] On cast/set/read/listener/timeout failure, destroy the candidate WebView and fail closed. On a later false mute observation, destroy the affected WebView and revoke the matching Runtime or content session.
- [ ] Keep mute enforcement across reload and session reuse, deny plugin navigation/mute commands, and add `media-src 'none'` to both Runtime host and protocol response CSP.

**Distinct test coverage:** ordered inert-create -> cast -> set -> readback -> listener -> navigate sequence; every preparation step and timeout fails before plugin navigation; the first plugin script observes a muted controller; runtime/content reloads remain muted; a later unmute event destroys only the exact current activation/session; CSP and fixed-path denial block media as defense in depth.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::webview_audio_guard::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_window::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests`

### Task 4: Freeze Activation and Alarm Authority into Timer Tickets

**Files:** modify `src-tauri/src/public_plugins/timers.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/plugin_window.rs`, `src-tauri/src/message_center.rs`

**Dependencies:** Tasks 2-3; design sections 8.3-8.4, 10, 13.2, 16.3-16.5.

- [ ] Extend `TimerKey`, ClaimTicket, AudioTicket, lifecycle cancellation, and caller authorization to use activationId in addition to the existing diagnostic generation and round identities.
- [ ] On Timer start, clone the current Bundle's `ValidatedAlarmAsset` into the frozen round together with the completion text and plugin-name snapshot; never re-resolve an alarm by current pluginId/generation after start.
- [ ] Carry `AudioTicket + AlarmAssetIdentity + Arc<[u8]>` through successful fired completion and native-attention admission while keeping public Timer/message DTOs unchanged.
- [ ] Convert reset, new round, disable, uninstall, update, fault-disable, and shutdown effects to compare the complete activation/ticket identity before cancellation; dispatch all effects after Timer/plugin locks are released.

**Distinct test coverage:** an update before a new round gives only the new round the new asset; an already running round retains old frozen bytes through package deletion; old generation-1 tickets fail after uninstall/reinstall generation-1 because activationId differs; Reset/update/focus/cancel races reject stale completion and audio effects; identity/round/audio/revision exhaustion never wraps.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::timers::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib message_center::tests`

### Task 5: Add Epoch Ownership and a Memory-Safe Windows Audio Adapter

**Files:** modify `src-tauri/src/native_attention/mod.rs`, `src-tauri/src/native_attention/windows_audio.rs`, `src-tauri/src/lib.rs`

**Dependencies:** Task 4; design sections 4.3, 11-12, 13.2, 16.5.

- [ ] Replace the shared active-ticket set with one logical `timerAudioOwner { reserved | playing }` and checked alarmEpoch stamping at mailbox admission. The first valid ticket owns the epoch; same-epoch tickets never start, switch, mix, or become fallback.
- [ ] Revalidate epoch, full ticket, and asset identity around asynchronous start. A late success may stop only its matching adapter state and cannot stop a newer owner.
- [ ] Make `WindowsAudioAdapter` the sole serial point for all WinMM calls: ordinary sound remains filename-backed one-shot host audio, while Timer uses `PlaySoundW(SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT)` with adapter-owned `PlayingMemory`.
- [ ] Keep the alarm `Arc<[u8]>` alive until a successful matching stop. On stop failure, move it to process-static non-dropping `ProcessAudioQuarantine`, make the whole native audio adapter terminal, and keep Toast/tray/badge/message paths operational.
- [ ] Ensure focus, cancellation, shutdown, worker panic/disconnect, root/adapter drop-order variants, and lock-poison fallback cannot release bytes still used by WinMM or let an old stop affect a new owner.

**Distinct test coverage:** first valid owner and invalid-first admission; concurrent start barriers with later tickets; cancel/focus/reset/update before call, during call, and after return; ordinary audio suppressed only while Timer audio actually plays and never replayed; all WinMM calls serialized; start failure ends only that round; stop failure quarantines one bounded buffer and makes subsequent ordinary/Timer audio fail closed; both coordinator/adapter drop orders and poison fallback retain quarantined memory.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib native_attention::tests && cargo test --manifest-path src-tauri/Cargo.toml --lib native_attention::windows_audio::tests`

### Task 6: Migrate Pomodoro Assets, Update SDK Guidance, and Integrate

**Files:** modify `examples/public-plugins/com.uipilot.pomodoro/package/plugin.json`, `examples/public-plugins/com.uipilot.pomodoro/README.md`, `examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`, `src-tauri/src/public_plugins/tests.rs`; remove `src-tauri/resources/sounds/attention-alarm.wav`

**Dependencies:** Tasks 1-5; design sections 14-17.

- [ ] Increase the migrated Pomodoro example version and keep only `message-notification.wav` in host packaging and startup resolution; the package-owned WAV was introduced in Task 1 to keep intermediate builds valid.
- [ ] Update the Pomodoro README/tests and the third-party guide with the fixed resource contract, strict WAV limits, host/private ownership boundary, reinstall requirement, and the unchanged JavaScript/Manifest interfaces.
- [ ] Assert the host package contains only ordinary message audio, the Pomodoro directory/archive package contains exactly one valid private alarm, and the schema still has no configurable audio path.
- [ ] Run the complete automated gate and inspect staged changes so no pre-existing user files or unrelated native-attention edits are absorbed accidentally.

**Distinct test coverage:** clean install of the migrated Pomodoro package; old package without the fixed alarm fails instead of using a host fallback; development directory and archive produce the same private identity/hash; public Runtime/window requests still return `403`; host resource resolution no longer references `attention-alarm.wav`.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml && cargo fmt --manifest-path src-tauri/Cargo.toml -- --check && cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings && npm.cmd test && npm.cmd run build`

## Final User Gate

- [ ] All automated checks pass and the diff maps only to the six task commits plus the approved plugin alarm asset.
- [ ] Notify the user before any real Windows, Toast, foreground-focus, tray, or audible validation. The Agent never controls mouse or keyboard.
- [ ] The user completely uninstalls the previously installed Pomodoro plugin, reinstalls the migrated package, starts a short Timer, closes the plugin window, confirms the package-owned alarm loops at completion, and confirms opening the main window stops it without clearing the unread message.
