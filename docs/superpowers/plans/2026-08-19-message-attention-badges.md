# Message Attention And Dual Badges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the original tray icon flashing until the main window receives focus, while one persisted unread count drives badges on both the launcher settings button and the settings `消息` tab.

**Architecture:** The Rust tray-attention worker owns a single serial event queue for `MessageArrived`, `MainFocusChanged(bool)`, ticks, and shutdown. Persisted unread state remains independent and continues through the existing message-center core; both React badge locations project that same snapshot through Ant Design `Badge`.

**Tech Stack:** Rust, Tauri 2.11, React 19, TypeScript, Ant Design 6, Vitest.

**Approved specification:** [2026-08-18-public-plugin-message-center-design.md](../specs/2026-08-18-public-plugin-message-center-design.md), especially sections 4.2-4.4, 11-13, 15-17.

## Global Constraints

- Main-window focus acknowledges only process-local tray attention; it never writes `readAt` or changes `unreadCount`.
- Entering settings `消息` remains the only ordinary UI action that marks the open-cutoff messages read.
- Tray attention alternates the original icon and a same-size transparent frame every 500 ms, with no duration deadline.
- `MessageArrived` and every `MainFocusChanged(bool)` share one serial order; no native focus query precedes a separate start command.
- A focused main window suppresses a new flash, and `Focused(true)` always stops an earlier flash.
- Controller construction failure degrades to a terminal no-op without failing application setup; runtime adapter replacement is unsupported.
- Both badge locations show 1-99 exactly, 100 as `99+`, hide zero, and show `!` for the existing unavailable terminal state.
- No new frontend command, plugin API, persisted field, input synthesis, real focus automation, or mouse/keyboard control.
- Preserve all pre-existing working-tree changes and stage only task-owned hunks.

## Global Execution Rules

- Dependency order: `Task 1 -> Task 2 -> Task 3`.
- Each task follows focused TDD once: add failing tests, confirm the intended failure, implement the minimum approved contract, rerun focused tests, and create one atomic commit.
- Rust commands use `--manifest-path src-tauri/Cargo.toml`; frontend commands run from the repository root.
- Run full Rust/frontend regression, lint, and build once after Task 3.
- Real tray animation and native focus acceptance are user-operated only after automated verification.

### Task 1: Serial Native Tray Attention

**Files:** `src-tauri/src/message_center/tray_flash.rs`, `src-tauri/src/message_center.rs`, `src-tauri/src/lib.rs`

**Dependencies:** Approved design sections 4.4, 11, 13, 15, and backend items in 16.1.

- [ ] Replace the deadline-based `Restart` worker with one state machine that serializes `MessageArrived`, `MainFocusChanged(bool)`, 500 ms ticks, and `Shutdown`; generate the transparent frame from the normal icon dimensions instead of loading `tray-reminder.png`.
- [ ] Track `mainFocused`, visual frame, active state, and `running | degraded | terminal` exactly as specified; repeated messages share one loop, focus loss alone does not start one, and focus gain restores the normal frame without touching message storage.
- [ ] Route every main `Focused(bool)` event to the message-center tray port before the existing expected-blur early return. Route committed messages to `MessageArrived` without a separate focus query.
- [ ] Keep normal tray creation mandatory but degrade attention-worker/channel construction failure to a terminal no-op port; isolate adapter/send failures and make shutdown terminal.

**Distinct test coverage:** `MessageArrived -> MainFocusChanged(true)` ends normal; `MainFocusChanged(true) -> MessageArrived` never starts; false focus followed by a new message starts; repeated messages do not add loops; no time deadline stops an active loop; transparent/normal update failures enter degraded and ignore later messages; construction failure installs no-op; shutdown rejects queued/late messages; source-contract test proves focus reporting precedes expected-blur return.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml message_center::tray_flash::tests` and `cargo test --manifest-path src-tauri/Cargo.toml tests::delayed_plugin_messages_start_with_app_and_stop_before_native_effects`

### Task 2: Launcher Settings Button Badge

**Files:** `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/styles.css`

**Dependencies:** Task 1; approved design sections 4.3, 12, and frontend items in 16.1.

- [ ] Replace the custom absolutely positioned count element with Ant Design `Badge` while retaining the fixed 28 px settings-button footprint, tooltip, focus ring, and click target.
- [ ] Project only the existing `messageCenter` snapshot: zero hidden, 1-99 exact, 100 `99+`, and unavailable `!`; do not add another message subscription or local count.
- [ ] Keep the badge visually inside the stable suffix container so WebView layout cannot clip it and its appearance cannot resize the input.

**Distinct test coverage:** Real Ant Badge DOM is present for 1, 99, 100, and unavailable; absent for zero; the settings button remains inside the input suffix, clickable, and fixed-size; returning to launcher preserves an unread count that has not been marked read.

**Verify:** `npm.cmd test -- src/launcher.test.tsx -t "settings badge"`

### Task 3: Settings Message Tab Badge And Read Clearing

**Files:** `src/launcher-view.tsx`, `src/launcher.test.tsx`, `src/styles.css`

**Dependencies:** Task 2; approved design sections 4.2-4.3, 9, 12, 15, and frontend items in 16.1.

- [ ] Wrap only the existing `消息` tab label with the same Ant Badge projection used by the launcher button; keep tab order, text, keyboard navigation, and stable dimensions.
- [ ] Preserve the existing `messageCenter.enter()` open-cutoff operation as the sole read transition. Its ready snapshot/event clears both projections; merely opening or focusing main leaves both intact.
- [ ] Ensure a later message while the `消息` tab remains selected recreates the tab badge and the launcher badge after returning, without changing view or focus.

**Distinct test coverage:** Both locations display the same 1/99/99+ and unavailable values across navigation; main activation does not clear either; selecting `消息` accepts the mark-read snapshot and clears both; an event after the open cutoff recreates the tab badge while selected and is still present on return to launcher.

**Verify:** `npm.cmd test -- src/launcher.test.tsx -t "message tab badge"`

## Final Verification

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] `npm.cmd test`
- [ ] `npm.cmd run build`
- [ ] Confirm task commits contain no pre-existing user changes.
- [ ] Ask the user before native acceptance. The user publishes a delayed `demo-win` message with main unfocused, observes continuous original/transparent flashing, focuses main to stop it while both badges remain, then enters `设置 -> 消息` to clear both badges. A second message while main is already focused must not start flashing.
