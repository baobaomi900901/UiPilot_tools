# Plugin sharing: deliver the single-panel text flow

Implement the user-approved [single-panel sharing contract](../specs/2026-11-06-plugin-sharing-design.md): OCR edited text → host receiver chooser → translation, with errors remaining on the target. This replaces the previous nine-task plan and its lifecycle subplan. No dual-Panel rollback or global authority redesign.

## Common constraints

Rust/Tauri2, React/TypeScript, Host0.22.0/API1. Preserve all preexisting dirty work; same codex/ocr-panel branch. No Computer Use, native automation, real provider/clipboard, user installation/grant/process changes, push or merge. Use focused behavioral RED/GREEN; one Cargo process, final unchanged filters may reuse the exact freshly built test executable with recorded path/time. Independent review of the working user flow, then delivery verification. No separate infrastructure-only task expansion.

### Task 1: Complete the single-panel OCR-to-translation flow

**Files:** SDK share declarations/tests; `src-tauri/src/plugin_share.rs` (small scoped record/DTO/helpers), existing `plugin_panel.rs` and focused childmodules, commands/ACL/lib wiring; actual generated Panel bridge; `src/launcher-core.ts`, `launcher-view.tsx`, `main.ts`, `protocol.ts`, focused share UI; OCR/translation package and tests. Existing Task1 manifest validators/version remain. Remove obsolete sharing-only coordinator/prepare/authority machinery from subsequent experimental commits.

**Dependencies:** new design above; original before-complexity reference3740a96, before-feature71f03ef; currentHEAD and dirty backups must be inspected before removal.

- [ ] Preserve the former implementation via local backup branch and binary working-diff backup; remove only our superseded sharing machinery without reset/clean or losing user changes. Restore unrelated ordinary manager/scheduler/network behavior, keeping manifest/source/receiver declarations. No old sharePrepare/authority registry dead code.
- [ ] Implement source snapshots, current-user receiver selection and ordinary one-Panel opening/delivery. Bind actual callers and exact current documents; enforce limits and once-only claim/handler. Use existing lifecycle instead of new cross-manager locks.
- [ ] Wire Ctrl+K, searchable selector, source hide/bounds suppression, target loading/error/adoption and keyboard ownership. Show target errors without source rollback. No payload in launcher query/events/history.
- [ ] Wire OCR current edited source; translation receiver and output source, auto-translate once after receipt, retain text on service error, preserve ordinary entry/theme behavior. OCR1.1.0/translate1.5.0, minHost0.22.0.
- [ ] Execute meaningful production bridge/native mock/UI/plugin tests for the design acceptance cases; demonstrate the complete fake-service path before calling the task ready for review.

**Distinct test coverage:** all six acceptance bullets in design; especially target failure stays target, no duplicated translation, multiple receiver choice, late old result cannot overwrite new session, actual-Webview caller rejection, targeted event privacy and ordinary OCR/translate regressions.

**Verify:** targeted `cargo test --lib plugin_share` / `plugin_panel` / affected IPC filters, Vitest generated bridge/share UI/launcher cases, Node OCR/translate tests and SDK type/manifest validation. Report actual commands/counts and scope. Independent review before Task2; fix concrete correctness issues without reintroducing discarded architecture.

### Task 2: Verify and deliver usable plugin packages

**Files:** final SDK/developer docs, focused acceptance record, archives under `artifacts/sharing-delivery/`; preserve all prior delivery artifacts.

**Dependencies:** reviewed Task1 working vertical flow.

- [ ] Run `npm.cmd test`, `npm.cmd run build`, `cargo test --lib`, CLI workspace verify and plugin SDK checks against the actual working tree. Distinguish user fixture changes from clean-commit evidence.
- [ ] Build/pack/validate OCR and translation archives with production CLI, check archive/source parity, document simple source/receiver API and user manual acceptance steps.
- [ ] Final whole-feature review includes removal of obsolete machinery and all user flow changes; close meaningful findings and state native/provider acceptance honestly.

**Distinct test coverage:** packaged manifests/assets match sources and Host0.22 contract; independent draft receiver proves plugin-owned processing; no source rollback on any target failure.

**Verify:** full commands above, production CLI pack/validate and checksums; no repeated unchanged full suite after documentary edits.
