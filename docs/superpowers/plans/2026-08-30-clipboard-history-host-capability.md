# Clipboard History Host Capability Implementation Plan

**Goal:** Ship the UiPilot host-side clipboard history capability required by the approved clipboard history Panel plugin: Windows-only recording of recent text, image, and file-list clipboard changes, plus a narrow Panel bridge that can restore one selected record and paste it back to the previously captured external window.

**Architecture:** Keep clipboard history as a host-owned service. Public plugin JavaScript receives only summaries through a `window.uipilotPluginPanel.clipboardHistory` bridge; raw text, image bytes, file paths, native HWND/PID, and input synthesis remain inside Rust. The implementation flows through public contract changes first, then host-key routing, then storage/observer/bridge/paste, with user-facing copy and final acceptance last.

**Technology:** Rust/Tauri 2, Windows clipboard and foreground APIs through the existing `windows` crate, existing `png` support for PNG encoding, TypeScript/React, Vitest, schemars/AJV, and the public plugin CLI.

## Global Constraints

- Source of truth: [clipboard history host capability request](../specs/2026-08-30-clipboard-history-host-capability.md), especially sections 4 through 8.
- This plan covers only UiPilot host/main-program work. Do not implement the clipboard history Panel plugin in this plan.
- Freeze final permission names as `clipboard.history.read` and `clipboard.history.paste`; keep existing `clipboard.read` unavailable and not equivalent to clipboard history read.
- Freeze final Panel bridge as `window.uipilotPluginPanel.clipboardHistory`, following the DTOs and error names in spec sections 5.2 and 5.4.
- Preserve `schemaVersion = 1` and `apiVersion = 1`; current baseline is Host `0.3.2`, and this capability bumps the host/package version to `0.3.3`.
- Windows is the only supported platform for this capability. Non-Windows builds must reject the capability as unsupported.
- Public plugins must never receive raw clipboard text, original PNG bytes, complete file paths, HWND, PID, arbitrary key names, or paste counts.
- Do not control the user's mouse or keyboard in automated tests. Real paste behavior is verified only through explicit user-run manual acceptance.
- Preserve pre-existing user changes. Each task stages and commits only task-owned hunks.

## Core Contract Overview

- Manifest permissions: `clipboard.history.read` authorizes host-side collection and summary exposure; `clipboard.history.paste` authorizes one explicit Enter-driven restore-and-paste action.
- Manifest `panel.hostKeys` adds declarations `Tab`, `Shift+Tab`, and `Enter` after existing order `ArrowDown < ArrowUp < Primary+N`.
- Delivered Panel host-key events use DOM-style keys: `Tab` and `Shift+Tab` both deliver `key: 'Tab'`, distinguished by `shiftKey`; `Enter` delivers `key: 'Enter'`.
- Panel bridge methods are `list()`, `onChanged(handler)`, `paste({ id, routeSequence })`, `remove({ id })`, and `clear()`.
- Stable paste error names are `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, `PasteTargetUnavailable`, and `ClipboardWriteFailed`.

## Global Execution Rules

- Every task follows focused TDD: add focused failing tests, confirm the intended failure, implement the minimum frozen contract, rerun focused tests, and commit.
- Each task produces at least one atomic local commit. Review fixes may add separate commits. Do not push unless the user explicitly asks.
- Dependency order: `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6 -> Task 7 -> Task 8`.
- Each task receives specification-compliance and code-quality review before a dependent task begins.
- Command convention: use `npm.cmd` on Windows for npm scripts and `cargo test --manifest-path src-tauri/Cargo.toml ...` for Rust-focused tests.

### Task 1: Public Contract, Permissions, Schema, CLI, And SDK

**Files:** `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/tests.rs`, `src/protocol.ts`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `packages/plugin-cli/schema/uipilot-plugin-v1.schema.json`, `packages/plugin-cli/src/generated/manifest-validator.mjs`, `packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/tests/manifest.test.ts`.

**Dependencies:** Approved spec sections 3, 5.1, 5.2, 5.3, and 8.1.

- [ ] Bump host/package version from `0.3.2` to `0.3.3` in Rust, Tauri, npm, and any public host-version source used by manifest compatibility checks.
- [ ] Add `clipboard.history.read` and `clipboard.history.paste` to Rust manifest parsing, generated schema, CLI validation, frontend protocol types, install summaries, and SDK docs.
- [ ] Keep `clipboard.read` present only as a reserved unsupported permission; it must not grant clipboard history access and must not pass as an alias for `clipboard.history.read`.
- [ ] Add `Tab`, `Shift+Tab`, and `Enter` manifest host-key declarations with canonical order `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter` across Rust, schema, CLI, and docs.
- [ ] Add SDK types for `UiPilotPluginPanelClipboardHistoryApiV1`, `ClipboardHistorySnapshot`, `ClipboardHistoryEntrySummary`, and the final `readonly clipboardHistory` property on `UiPilotPluginPanelApiV1`.
- [ ] Document permission copy and privacy warning: authorized clipboard changes are recorded locally while UiPilot is running, the plugin is enabled, and permission is granted.

**Distinct test coverage:** New permissions are accepted only as their final names; `clipboard.read` remains unsupported and non-equivalent; duplicate/unknown host keys fail closed; valid host keys normalize in canonical order; SDK contract rejects extra fields and exposes `clipboardHistory`.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::manifest`; `cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check`; `npm.cmd test --workspace @uipilot/plugin-cli -- manifest.test.ts`; `npm.cmd run check --workspace @uipilot/plugin-cli`; `npm.cmd run build`.

### Task 2: Launcher And Rust Panel Host-Key Routing Extension

**Files:** `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/protocol.test.ts`, `src/launcher.test.tsx`, `src-tauri/src/plugin_panel.rs`, `src-tauri/src/commands.rs`, `src-tauri/capabilities/main.json`, `src-tauri/capabilities/plugin-panel-content.json`.

**Dependencies:** Task 1; spec sections 4.4, 5.3, and 8.1.

- [ ] Extend strict frontend parsers and launcher state so Panel snapshots can carry the new host-key declarations without stale open/submit results overwriting newer sessions.
- [ ] Match unmodified `Tab`, only-Shift `Shift+Tab`, and unmodified non-composing `Enter` only when declared, current epoch, and receiver armed.
- [ ] Synchronously `preventDefault()` for matched Tab/Shift+Tab/Enter; declared Enter must not call `submitPanel`, and undeclared Enter keeps existing submit behavior.
- [ ] Extend Rust `PluginPanelHostKey` delivery so `Tab` and `Shift+Tab` both deliver `key: 'Tab'` with correct `shiftKey`, while `Enter` delivers `key: 'Enter'`.
- [ ] Preserve existing route sequencing, queue capacity, ack timeout, stale-session no-op behavior, and capability separation between main-only enqueue and panel-content-only ack.

**Distinct test coverage:** `Tab` no modifiers routes only declared `Tab`; `Shift+Tab` routes only declared `Shift+Tab`; `Ctrl+Tab`, `Alt+Tab`, composing Enter, stale epoch, unarmed receiver, and undeclared Enter do not route; declared Enter bypasses submit; queue-full and protocol-violation behavior remains unchanged.

**Verify:** `npm.cmd test -- src/protocol.test.ts src/launcher.test.tsx`; `cargo test --manifest-path src-tauri/Cargo.toml plugin_panel::tests`; `cargo test --manifest-path src-tauri/Cargo.toml commands::tests`.

### Task 3: Clipboard History Core Model, Store, Fingerprint, And Preview

**Files:** create `src-tauri/src/clipboard_history/mod.rs`, create `src-tauri/src/clipboard_history/model.rs`, create `src-tauri/src/clipboard_history/store.rs`, create `src-tauri/src/clipboard_history/preview.rs`, modify `src-tauri/src/lib.rs`.

**Dependencies:** Task 1; spec sections 4.1, 4.2, 4.3, 4.5, 5.2, 6, 7, and 8.1.

- [ ] Define host-owned records for text, image, and file-list history with opaque stable `id`, immutable `capturedAt`, internal recency rank, persisted `revision`, persisted fingerprint, and plugin-scoped ownership.
- [ ] Implement `textPreview`: at most 120 Unicode scalar values, whitespace/newlines folded to one space, empty or whitespace-only text represented as an empty preview while remaining restorable.
- [ ] Implement fingerprint rules exactly from spec section 4.2 for text, decoded image pixels, and Windows-normalized ordered file paths.
- [ ] Persist a per-plugin index atomically in the plugin-isolated user data directory, with image PNG files stored beside the index and clipboard content excluded from logs and errors.
- [ ] Enforce capacity: 20 entries max, 10 MiB per image, 100 MiB total image storage, eviction from oldest recency rank, and image file cleanup on eviction/remove/clear.
- [ ] Generate Panel thumbnails as PNG summaries with long edge no larger than 256 px and encoded size no larger than 256 KiB, shrinking repeatedly until both limits are satisfied.
- [ ] Handle corrupted indexes by isolating the bad file and starting from empty history without crashing.

**Distinct test coverage:** `capturedAt` survives restore/move-to-front; recency order changes without requiring `capturedAt` order; `id` survives restart and is not reused after deletion; `revision` increments on capture/remove/clear/evict/move-to-front; fingerprints dedupe each kind; text previews never expose full long text; an image larger than 10 MiB is not stored or truncated into history; thumbnails satisfy both 256 px and 256 KiB limits; image eviction removes files.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml clipboard_history`.

### Task 4: Windows Clipboard Observer And Plugin Lifecycle Activation

**Files:** create `src-tauri/src/clipboard_history/observer.rs`, create `src-tauri/src/clipboard_history/service.rs`, modify `src-tauri/src/lib.rs`, modify `src-tauri/src/public_plugins.rs`, modify `src-tauri/src/commands.rs`.

**Dependencies:** Task 3; spec sections 4.1, 4.2, 4.3, 4.5, 6, 7, and 8.1.

- [ ] Start the observer only when at least one installed public plugin is enabled and granted `clipboard.history.read`; fan out each accepted capture only to plugin stores whose plugin is currently enabled and granted `clipboard.history.read`.
- [ ] Stop recording immediately for a plugin when its permission is revoked, the plugin is disabled, fault-stopped, or fully uninstalled; keep the global observer alive if other plugins remain authorized.
- [ ] Read each Windows clipboard update into at most one normalized capture using priority `files -> image -> Unicode text`; do not read Win+V history or backfill while UiPilot/plugin is stopped.
- [ ] Treat HTML/RTF plus Unicode text fallback as Unicode text fallback only; ignore unsupported formats without logging content.
- [ ] Retry temporarily busy clipboard reads up to 3 short attempts within 250 ms total, then skip that change without blocking UI or crashing.
- [ ] Suppress listener feedback when `paste()` restores an existing record to the clipboard; the record moves to front once instead of creating a duplicate.
- [ ] On complete uninstall, delete history data; on retain-data uninstall, preserve it for reinstall plus authorization.

**Distinct test coverage:** fake clipboard reader proves lifecycle gating; mixed-format priority; busy clipboard retry budget; unsupported format ignored; disable/revoke/uninstall stops capture; retain-data uninstall preserves index; restore feedback does not duplicate.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml clipboard_history`; `cargo test --manifest-path src-tauri/Cargo.toml public_plugins::tests`.

### Task 5: Panel Clipboard-History Bridge For Snapshot, Subscribe, Remove, And Clear

**Files:** `src-tauri/src/plugin_panel.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/clipboard_history/service.rs`, `src-tauri/capabilities/plugin-panel-content.json`, create `src-tauri/permissions/autogenerated/plugin_panel_clipboard_history_list.toml`, create `src-tauri/permissions/autogenerated/plugin_panel_clipboard_history_remove.toml`, create `src-tauri/permissions/autogenerated/plugin_panel_clipboard_history_clear.toml`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`.

**Dependencies:** Task 4; spec sections 5.2, 6, 7, and 8.1.

- [ ] Register new Tauri commands, generated permission files, invoke handlers, and capability allowlist entries for `clipboardHistory.list()`, `clipboardHistory.onChanged(handler)`, `clipboardHistory.remove({ id })`, and `clipboardHistory.clear()` before exposing them in the Panel bootstrap.
- [ ] Inject those methods into the existing Panel bootstrap under `window.uipilotPluginPanel`.
- [ ] Gate every bridge call by panel-content caller, current `pluginId`, current plugin generation, current Panel session epoch, enabled plugin state, and `clipboard.history.read` permission.
- [ ] Return snapshots in host recency order with only summary fields: `textPreview`, image thumbnail data URL plus dimensions, or file name/count/availability; thumbnails must satisfy 256 px and 256 KiB limits, and snapshots must never return raw text, original PNG bytes, or complete paths.
- [ ] Implement `onChanged()` so registration asynchronously emits the current snapshot, future events are monotonic by `revision`, high-frequency changes may coalesce to latest, handler errors do not break future delivery, and unsubscribe prevents later delivery.
- [ ] Implement `remove()` and `clear()` as host-store mutations that increment `revision`, remove matching image data, and notify live subscribers.

**Distinct test coverage:** main-window callers are rejected before protected state access; missing command registration or missing capability entries fail tests; stale sessions get `ExpiredPanelSession`; revoked permission gets `PermissionDenied`; snapshots are redacted and thumbnails stay within 256 px/256 KiB; `onChanged()` initial delivery and coalescing keep monotonic `revision`; handler throw and unsubscribe behave as specified; remove/clear delete images and notify.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml plugin_panel::tests`; `cargo test --manifest-path src-tauri/Cargo.toml commands::tests`; `cargo test --manifest-path src-tauri/Cargo.toml clipboard_history`; `npm.cmd run build`.

### Task 6: Paste Coordinator And Safe Restore-To-External-Window Flow

**Files:** create `src-tauri/src/clipboard_history/paste.rs`, modify `src-tauri/src/clipboard_history/service.rs`, modify `src-tauri/src/plugin_panel.rs`, modify `src-tauri/src/commands.rs`, modify `src-tauri/src/lifecycle.rs`, modify `src-tauri/capabilities/plugin-panel-content.json`, create `src-tauri/permissions/autogenerated/plugin_panel_clipboard_history_paste.toml`.

**Dependencies:** Tasks 2 and 5; spec sections 5.4, 6, 7, and 8.1.

- [ ] Register the paste Tauri command, generated permission file, invoke handler, and plugin-panel-content capability allowlist entry before exposing `clipboardHistory.paste({ id, routeSequence })` in the Panel bootstrap.
- [ ] Map host errors to stable JS `Error.name` values from spec section 5.4.
- [ ] Admit paste only after validating, in this order: current plugin/session/generation, granted `clipboard.history.read` and `clipboard.history.paste`, existing record id, image/index backing data availability, unconsumed current Enter Host Key ticket matching `routeSequence`, file-list availability, and captured external target HWND/PID eligibility.
- [ ] Reject before writing the system clipboard if any pre-hide validation fails; Panel remains visible and the user's current clipboard is not overwritten.
- [ ] Write the selected record back to the Windows clipboard as Unicode text, PNG image, or file list only after all pre-hide validation succeeds; if writing fails, reject with `ClipboardWriteFailed` and keep Panel visible.
- [ ] After successful clipboard write, atomically consume the Enter ticket, move the record to front, increment `revision`, resolve `{ outcome: 'admitted' }`, then hide UiPilot through the explicit-return lifecycle.
- [ ] Revalidate captured external HWND/PID again after hide; after hide send exactly one Windows `Ctrl+V` only when the target is foreground, not a Shell window, and not UiPilot-owned.
- [ ] If focus or input send fails after hiding, do not retry and do not restore the previous clipboard; leave selected content as the system clipboard.

**Distinct test coverage:** revoked read permission and revoked paste permission each return `PermissionDenied`; stale session, stale route sequence, duplicate Enter ticket, missing record, unavailable file, unavailable image backing data, unavailable index data, pre-hide target failure, UiPilot/Shell target rejection, and clipboard-write failure all reject before overwriting the clipboard or hiding; post-hide focus failure leaves selected content on the clipboard without retry; successful text/image/file paste admission produces the specified terminal state without leaking sensitive data.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml clipboard_history`; `cargo test --manifest-path src-tauri/Cargo.toml plugin_panel::tests`; `cargo test --manifest-path src-tauri/Cargo.toml lifecycle::tests`; `cargo test --manifest-path src-tauri/Cargo.toml commands::tests`.

### Task 7: Settings, Install Confirmation, Permission Copy, And Host Documentation

**Files:** `src/protocol.ts`, `src/public-plugin-panel.tsx`, `src/launcher-view.tsx`, `src/launcher.test.tsx`, `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`.

**Dependencies:** Task 6; spec sections 4.5, 5.1, 5.2, 5.4, 8.1, and 10.

- [ ] Show clear user-facing permission labels for `clipboard.history.read` and `clipboard.history.paste` in prepare/install summaries, settings plugin detail, and permission lists.
- [ ] Display the privacy warning that authorized clipboard changes are recorded locally while UiPilot runs, the plugin is enabled, and permission is granted; do not imply password/sensitive-source detection.
- [ ] Document the final bridge property, DTOs, error names, host-key declaration table, lifecycle limits, non-Windows unsupported behavior, and manual acceptance expectations.
- [ ] Keep plugin developer docs explicit that plugin tasks must not patch host sources; plugin agents should wait for this host capability before implementing clipboard history.

**Distinct test coverage:** install/settings views render both new permissions with supported/granted/unsupported state; warning copy appears for clipboard history read; docs contain final permission names and do not mention `clipboard.read` as an alias.

**Verify:** `npm.cmd test -- src/launcher.test.tsx`; `npm.cmd run build`; `Select-String -Path docs/plugin-sdk/public-plugin-v1.md,docs/plugin-sdk/public-plugin-developer-guide.md,docs/plugin-sdk/uipilot-plugin-api-v1.d.ts -Pattern 'clipboard.history.read|clipboard.history.paste|clipboardHistory|PermissionDenied'`.

### Task 8: Host-Side Integration Fixture, Final Verification, And Manual Acceptance Handoff

**Files:** create `tests/fixtures/public-plugins/clipboard-history-host-fixture/plugin.json`, create `tests/fixtures/public-plugins/clipboard-history-host-fixture/dist/panel.html`, create `tests/fixtures/public-plugins/clipboard-history-host-fixture/dist/panel.js`, create `docs/superpowers/checklists/2026-08-30-clipboard-history-host-manual-acceptance.md`, modify `src-tauri/src/public_plugins/tests.rs`, modify `src-tauri/src/plugin_panel.rs`, modify `src-tauri/src/commands.rs`.

**Dependencies:** Task 7; spec sections 8.1 and 8.3.

- [ ] Add a minimal host-contract fixture that declares `clipboard.history.read`, `clipboard.history.paste`, and host keys `Tab`, `Shift+Tab`, `Enter`; it exists only for host testing and does not become the product clipboard history plugin.
- [ ] Exercise fixture install/validation, Panel readiness, host-key delivery, bridge snapshot retrieval, remove/clear mutation, and paste admission through host-owned tests or test instrumentation.
- [ ] Run final automated acceptance covering spec section 8.1, including redaction, routeSequence protection, permission gating, corrupted store recovery, and stale-session cancellation.
- [ ] Prepare a manual acceptance note for the user covering real Notepad/WeChat/Explorer paste scenarios; do not automate user input or mouse/keyboard control.

**Distinct test coverage:** The fixture proves host capability availability without plugin business UI; automated tests cover host security and state machines; manual checklist remains the only place that verifies real foreground focus and `Ctrl+V` behavior.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml`; `npm.cmd test`; `npm.cmd run build`; `cargo check --manifest-path src-tauri/Cargo.toml`; `git diff --check`.

## Final Checklist

- [ ] Spec section 8.1 host automated acceptance passes.
- [ ] SDK docs and generated schema agree on permissions, host keys, DTOs, and error names.
- [ ] No public plugin receives raw clipboard contents, original images, complete file paths, HWND, PID, arbitrary keys, or paste counts.
- [ ] `clipboard.read` remains unsupported and non-equivalent to `clipboard.history.read`.
- [ ] The product Panel plugin implementation is still not part of this plan.
- [ ] User performs Windows manual acceptance for text, image, file-list, WeChat return focus, restart persistence, and invalid-file behavior.
