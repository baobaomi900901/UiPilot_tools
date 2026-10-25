# Command Hotkeys Implementation Plan

Build one local host increment: Settings command rows assign one global chord to a stable public-plugin command. The architecture uses host Settings persistence, a command native registry serialized with launcher hotkey edits, and the existing lifecycle/activation paths.

**Specification:** [Command hotkeys design](../specs/2026-10-26-command-hotkeys-design.md), all sections. This document follows [repository plan guidelines](../../agents/implementation-plan-guidelines.md).

**Technology and constraints:** Existing Rust/Tauri 2 and React 19/TypeScript stack; no new dependency. Host 0.11.0, public plugin API v1 unchanged, existing example minimum-host versions unchanged. Native tests use fake registrations only. No real focus/input/global shortcut operations, user-data changes, network capability, new demo plugin, or changes to ordinary Enter semantics. Apply [UI guidelines](../../ui-guidelines.md).

## Execution rules

Task 1 -> Task 2. Focused behavior tests precede implementation; confirm the intended failures, implement the minimum, rerun the affected tests, and commit the complete vertical increment. Task 2 records delivery evidence in a separate atomic commit. Keep unrelated work out of both commits.

One parent design review, one implementer, and one combined implementation/code review suffice; fix concrete findings without adding serial layer gates or repeat full review loops. Use root commands below; Rust commands use `--manifest-path src-tauri/Cargo.toml --lib`. Do not run native registration or GUI interaction during automated tests.

### Task 1: Ship command bindings from Settings through native entry

**Files:** `src-tauri/src/settings.rs`, `hotkey.rs`, `lifecycle.rs`, `commands.rs`, `public_plugins.rs`, `public_plugins/activation.rs`, `lib.rs`; `src-tauri/build.rs`, `src-tauri/capabilities/main.json` and generated command permission files; `src/protocol.ts`, `launcher-core.ts`, `launcher-view.tsx`, `public-plugin-panel.tsx`, `main.ts`, `hotkey-recorder.ts`, extracted `hotkey-recorder-input.tsx`, `styles.css`; existing focused test files. Update root `package.json`/`package-lock.json`, `src-tauri/Cargo.toml`/`Cargo.lock`/`tauri.conf.json`, and `packages/plugin-cli/src/contracts.ts` plus its version expectations.

**Dependencies:** Design sections “Settings and visible behavior”, “Registration, persistence, and locks”, “Native delivery and recording”, and “Stable command entry and execution”.

- [ ] Implement host map, separate native registration table, shared serialization, main-only IPC, inventory enrichment, rollback/status, and both-way launcher conflict checks.
- [ ] Connect startup and actual lifecycle publications, including runtime faults; implement prune/reinstall ordering and retainData-independent uninstall cleanup.
- [ ] Carry stable identity through readiness/shown and foreground recording events, resolve current entry, and retain identity through asynchronous command routing. Preserve ordinary Enter and existing completion semantics.
- [ ] Reuse recorder and command-row mutation/reload ownership for edit/clear, accessible status/focus, Light/Dark layout, and double-tap restriction.
- [ ] Synchronize host 0.11.0 and CLI current host target without changing SDK/API contracts or old example floors.

**Distinct test coverage:** Own all automated cases in the design “Focused evidence and manual acceptance” section, especially gate linearization, non-main rejection before state access, persistence/native rollback, save/uninstall ordering, stale readiness/resolve replies, rename-and-label-reuse routing, and recording DOM/native deduplication. Use existing activation fixtures and fake registration operations.

**Verify:** `npm test -- src/hotkey-recorder.test.ts src/protocol.test.ts`; `npm test -- src/launcher.test.tsx src/public-plugin-settings.test.tsx -t command.hotkey`; `cargo test --manifest-path src-tauri/Cargo.toml --lib command_hotkey` (name new focused cases with this prefix); `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugin_prompt_and_dispatch_decisions_preserve_activation_modes`. Run additional existing focused test modules only where changed contracts require them.

### Task 2: Build and hand off the existing-plugin manual checks

**Files:** Release/version notes at the repository's existing release-note location, or `docs/superpowers/command-hotkeys-delivery.md` if no such location exists; this plan's checkboxes.

**Dependencies:** Task 1; design “Scope and outcome” and “Focused evidence and manual acceptance”.

- [ ] Run one integrated verification and production build after focused tests/review fixes pass; record exact command results and executable path.
- [ ] Provide a short manual sequence using existing Text Snippet and Notes: record/edit/clear; hidden/visible trigger; required-input prompt; panel entry; conflicts in both directions and OS rejection; rename; disable/re-enable; restart; update/uninstall including retainData.
- [ ] Record two-theme/keyboard checks and distinguish automated evidence from unperformed human global-trigger/restart/system interaction. Do not claim manual success without results.
- [ ] Commit delivery notes, then report build location, commit IDs, remaining manual checks, and any concrete limitation.

**Distinct test coverage:** End-to-end interaction belongs to the human checklist; existing packages supply the acceptance fixture. No new plugin, browser demo, native smoke automation, or additional review cycle.

**Verify:** Reuse completed focused test evidence; run `npm run build` and the existing acceptance build: `cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked`, with process-only `CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target` and `TAURI_CONFIG` identifying `com.uipilot.launcher.acceptance` / `UiPilot Acceptance`. Freeze clean source/dist input hashes before the native build and compare afterward. Do not repeat the broad verifier or unchanged suites. Preserve all previous delivery binaries; do not run the resulting app.


