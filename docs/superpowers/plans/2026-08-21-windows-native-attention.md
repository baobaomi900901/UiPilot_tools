# Windows Native Attention Coordinator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the separate message Toast, tray flash, and Timer alarm paths with the approved bounded Windows native-attention coordinator, including ordinary one-shot sound, shared looping Timer alarm, stable dev/release Toast identity, and deterministic focus/shutdown behavior.

**Architecture:** Message persistence and Timer authority remain in their existing owners. A new process-level coordinator receives exactly one classified post-guard event per published message, serializes focus, Toast, tray, and audio effects through the approved bounded mailbox, and asks the Timer service to linearize audio admission/cancellation. Windows-specific identity, Toast, tray, and audio adapters stay behind narrow ports so automated tests never display real Toasts, alter foreground focus, or play sound.

**Tech Stack:** Rust 1.96, Tauri 2.11, `windows` 0.61, WinRT Toast APIs, Win32 Shell/COM, `PlaySoundW`, existing Rust unit/integration tests.

Approved source of truth: [2026-08-21-windows-native-attention-design.md](../specs/2026-08-21-windows-native-attention-design.md).

## Global Constraints

- Windows-only behavior; public plugin API, permissions, manifest, message DTOs, and Timer DTOs remain unchanged.
- Debug uses AUMID `com.uipilot.launcher.dev` and `%APPDATA%\Microsoft\Windows\Start Menu\Programs\UiPilot Dev.lnk`; Release uses `com.uipilot.launcher` and `UiPilot.lnk`.
- `SetCurrentProcessExplicitAppUserModelID` runs before `tauri::Builder::default()` and before any Tauri window, tray, Jump List, or notifier is created.
- The only nested focus lock order is `Timer -> attention admission`; every other Timer/plugin path releases its locks before attention admission, and workers release mailbox locks before Timer/native calls.
- Mailbox bounds are ordinary published 64, pending Timer keys 64, active Timer tickets 64, focus FIFO 128, active Toasts/callbacks 64, plus an independent terminal Shutdown flag. No unbounded fallback channel is allowed.
- Audio substates `issued | admitted | confirmed` do not advance public `timerRevision`; Timer phase/round/claiming transitions retain their existing revision contract.
- Toast text uses a fixed DOM template and text nodes. Plugins cannot control launch data, actions, title, sound, tray icon, AUMID, focus confirmation, or attention priority.
- The bundled WAV is `src-tauri/resources/sounds/attention-alarm.wav`, copied from `C:\Users\moby\Downloads\196838__idepe__alarm_clock.wav`, with SHA-256 `9F66E473EEEE7AAF75AB2761423DAD1D04FA3F019744899DD154350F4117A8F3`.
- Automated work must not show real Toasts, control the foreground window, synthesize input, or play real sound. Manual Windows/Toast/focus/audio acceptance requires explicit user confirmation.

## Core Contract Overview

The exact event DTOs, mailbox slots, failure behavior, and sequence rules are defined by design sections 4-9. Task boundaries rely on these internal interfaces:

- `TimerOperation<T>` returns `result` plus `TimerPostLockEffect::AudioCancelled` even when the result is `TimerUnavailable`.
- `PluginTimerService` owns `admit_audio_start`, no-start/play-failure confirmation, `terminate_all_audio`, and the narrow focus-linearization closure that permits the sole `Timer -> attention admission` nesting.
- `NativeAttentionCoordinator` admits `PublishedAttention`, Timer cancellation, exact focus transitions, Toast callbacks, and Shutdown; all production effects are invoked only by its worker.
- `MessageCenterService::dispatch_post_guard` still emits the frontend state first, then classifies a committed publication as `Ordinary` or `TimerCompletion { audio_ticket }` exactly once for native attention.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4`.
- Each task uses focused TDD, produces one atomic commit, and excludes pre-existing/unrelated workspace files.
- The approved design is authoritative for state, ordering, lock boundaries, rollback, degradation, and acceptance. This plan assigns ownership rather than redefining those contracts.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml`.
- Specification-compliance and code-quality review occur at each task boundary; review fixes are separate commits when needed.

### Task 1: Add Timer Audio Authority and Lock-After Effects

**Files:** `src-tauri/src/public_plugins/timers.rs`, `src-tauri/src/public_plugins/activation.rs`

**Dependencies:** Design sections 4.3, 5.3, 8, 9, and 10.1 items 3-8, 11-13, 20-22.

- [ ] Replace the TimerRecord audio `Option<AudioTicket>` with the bounded `none | issued | admitted | confirmed` authority while preserving the existing public state projection and fired revision.
- [ ] Replace `TimerMutation.audio_to_stop` and lifecycle-returned optional tickets with `TimerOperation<T>` and `TimerPostLockEffect`, ensuring every removal/replacement of `admitted` emits exactly one lock-after cancellation on success or error.
- [ ] Add narrow, idempotent methods for audio-start admission, focus/no-start confirmation, play-failure confirmation, and process-terminal `terminate_all_audio`; no method performs native I/O or attention admission while holding the Timer lock.
- [ ] Add the focus-linearization closure/guard used by Task 2: it acquires the Timer lock first and permits the coordinator to acquire attention admission and commit `Focused(true)` before confirming the current issued/admitted slots.
- [ ] Update Reset, fired-to-new-Start, disable, fault-disable, uninstall, successful generation replacement, revision/round/audio identity exhaustion, and shutdown to return and dispatch all post-lock effects before returning the public result.

**Distinct test coverage:** `issued -> admitted -> confirmed` keeps the fired revision; Reset/new Start increments only for the public Timer transition; admitted cancellation survives `TimerUnavailable`; fired-to-new-round and each lifecycle removal emit exactly one cancellation; issued needs no cancellation but cannot later admit; terminal cleanup covers mixed issued/admitted records.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::timers::tests && cargo test --manifest-path src-tauri/Cargo.toml public_plugins::activation::tests`

### Task 2: Implement the Bounded Coordinator and Classified Dispatch

**Files:** create `src-tauri/src/native_attention/mod.rs`; modify `src-tauri/src/lib.rs`, `src-tauri/src/message_center.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/activation.rs`

**Dependencies:** Task 1; design sections 3-5, 8-9, 10.1 items 1-13 and 18-22.

- [ ] Implement the fixed-capacity mailbox, monotonic `attentionSequence`, ordinary quota, Timer-key slots, 128-entry exact focus FIFO, Toast callback slots, active-Timer set, independent Shutdown wakeup, and terminal fail-closed transition. Worker selection always uses the smallest admitted sequence.
- [ ] Implement fakeable Toast, tray, audio, route, and Timer-authority ports plus `CleanupGuard`, `catch_unwind`, receiver-disconnect handling, and cross-thread emergency-stop that never touches apartment-bound Toast objects.
- [ ] Centralize message dispatch so the frontend ready/unavailable event is emitted first and each committed publication creates exactly one `PublishedAttention`; ordinary Runtime/scheduled messages and Timer completion remain distinct even when the Timer audio ticket is absent.
- [ ] Replace direct `TimerAlarm` playback with coordinator admission. Timer completion persists first, completes the claim, and then submits `TimerCompletion { audio_ticket }`; cancellation effects are submitted only after Timer/plugin locks are released.
- [ ] Linearize `Focused(true)` with the sole `Timer -> attention admission` nesting after both locks are held; `Focused(false)` uses attention admission only. The worker clears the entire active-Timer set at the true-event sequence, stops tray/audio, and does not mark messages read or retract Toasts.
- [ ] Preserve exact event outcomes under capacity pressure: ordinary overflow drops only that message's native effects; Timer overflow terminates issued authority; focus/control inconsistency enters terminal cleanup; Shutdown cannot be blocked by any slot.

**Distinct test coverage:** message-before-focus shows Toast then stops tray/audio; focus-before-message suppresses effects; `true -> false -> later Timer` is not confirmed by the old focus; both Timer-lock/focus barrier orders; Reset with delayed cancellation followed by focus/blur/ordinary message leaves no stale active ticket; multiple Timer tickets share one loop and partial cancellation continues; focus FIFO/ordinary/Timer/Toast hard limits; sequence exhaustion; worker panic/disconnect terminates issued and admitted states; Timer loop suppresses ordinary sound without suppressing Toast/tray/badge.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml native_attention::tests && cargo test --manifest-path src-tauri/Cargo.toml message_center::tests && cargo test --manifest-path src-tauri/Cargo.toml public_plugins::activation::tests`

### Task 3: Add Windows Identity, Shortcut, and MTA Toast Adapter

**Files:** create `src-tauri/src/native_attention/windows_identity.rs`, create `src-tauri/src/native_attention/windows_toast.rs`; modify `src-tauri/src/native_attention/mod.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`; remove `src-tauri/src/message_center/windows_notification.rs`

**Dependencies:** Task 2; design sections 6, 8-9, 10.1 items 14-17, and acceptance items 1, 8-9.

- [ ] Set the build-specific process AUMID before `uipilot_lib::run()` can construct Tauri; identity failure degrades only Toast and records a stable diagnostic.
- [ ] Implement the one-shot STA shortcut worker with balanced COM initialization, current-executable canonical target checks, fixed UiPilot AUMID allowlist, safe adoption of UiPilot-owned shortcuts, atomic replacement, and refusal to overwrite unknown same-name files.
- [ ] Implement an MTA Toast port factory owned by the attention worker: balanced WinRT initialization, `CreateToastNotifierWithId`, fixed DOM/text-node content, notification setting check, active-handle cap 64, and cleanup before `RoUninitialize`.
- [ ] Route `Activated | Failed | Dismissed` callbacks back through the coordinator's bounded `ToastCallback`; callbacks carry only host notification ID and fixed kind. First terminal callback wins, Activated routes to `ShowTarget::Messages`, and late/unknown/Shutdown callbacks are absorbed.
- [ ] Add only the Windows crate feature(s) required for RoInitialize/shortcut implementation; do not add a notification dependency or expose arbitrary launch/action data.

**Distinct test coverage:** Debug/Release AUMID selection and pre-Builder call order; missing shortcut creation; current-target/known-AUMID repair; unknown file refusal; STA/MTA initialize-uninitialize balance; XML metacharacters remain text; system-disabled and synchronous/asynchronous failure isolation; callback first-terminal semantics; active Toast cap and shutdown cleanup.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml native_attention::windows_identity::tests && cargo test --manifest-path src-tauri/Cargo.toml native_attention::windows_toast::tests && cargo test --manifest-path src-tauri/Cargo.toml lib::tests`

### Task 4: Wire Tray, One-Shot/Loop Audio, Resources, and Shutdown

**Files:** create `src-tauri/src/native_attention/tray.rs`, create `src-tauri/src/native_attention/windows_audio.rs`, create `src-tauri/resources/sounds/attention-alarm.wav`; modify `src-tauri/src/native_attention/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/message_center.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/tauri.conf.json`; remove `src-tauri/src/message_center/tray_flash.rs`, remove `src-tauri/src/public_plugins/timer_alarm.rs`

**Dependencies:** Tasks 1-3; design sections 3, 7-9, 10.1 items 1, 7-13, 17-24, section 10.2, and section 11.

- [ ] Move tray visual ownership under the coordinator while preserving the current 500 ms normal/transparent cadence. Focus restores the normal icon; unread messages remain persisted and badged until the Messages tab marks them read.
- [ ] Implement one Windows `PlaySoundW` port: ordinary uses one-shot async flags, Timer uses the same WAV with `SND_LOOP`, Timer has priority, additional Timer tickets do not restart sound, and only focus/Shutdown/final active-ticket cancellation stops the loop.
- [ ] Copy and verify the approved WAV, replace the old `timer-complete.wav` resource entry with `attention-alarm.wav`, and resolve the packaged path without depending on the user's Downloads folder at runtime.
- [ ] Construct/manage the coordinator before message and Timer producers start; install Windows ports after shortcut readiness; route the main window's real `WindowEvent::Focused(bool)` to the coordinator before launcher hide logic can return.
- [ ] Replace all legacy direct native-effect wiring, enforce shutdown order (stop producers, close admission, terminal wake, worker cleanup/join, absorb late callbacks), and retain no separate tray or Timer audio worker.
- [ ] Run full automated verification and inspect the diff for unrelated workspace files. Stop before any real Toast, foreground-focus, tray, or audible acceptance step and request user confirmation.

**Distinct test coverage:** ordinary message produces Toast/tray/one-shot only while unfocused; Timer starts/maintains one loop and suppresses ordinary audio; first/partial/final Timer cancellation behavior; focus stops sound/tray without clearing unread; audio/tray/Toast failure independence; startup construction failure installs terminal no-op; exit cleanup is idempotent; packaged WAV is RIFF/WAVE with the approved size/hash and resolvable Tauri path.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml && cargo fmt --manifest-path src-tauri/Cargo.toml -- --check && cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings && npm.cmd test && npm.cmd run build`

## Final User Gate

- [ ] Automated tests, formatting, lint, frontend regression, schema check, and production build are green.
- [ ] The tracked diff contains only the four task commits and the approved WAV resource; existing untracked/user files remain untouched.
- [ ] Notify the user before design section 10.2 manual Windows acceptance. The user alone operates real windows, focus, mouse, keyboard, Toast clicks, notification settings, tray exit, and audible checks.
