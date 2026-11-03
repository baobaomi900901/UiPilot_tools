# Clipboard history actions implementation plan

Goal: modernize the clipboard Panel and support Tab/Delete through the shared action system.
Architecture: extend existing host-key routing; the plugin owns actions and mutations, the Host owns menus/status. TypeScript/Rust/JavaScript. Specification: ../specs/2026-11-02-clipboard-history-delete-host-capability.md (approved by user, including Host work).
Constraints: preserve existing edits, rust_uia read-only, no real clipboard/history changes in tests. Use synthetic fixtures. Keep Enter paste authorization intact. Delete requires Host 0.16.0; this is the current unreleased development version.
Workflow: add focused regression tests before implementation; verify focused suites and build. No commits of unrelated changes.

### Task 1: Host Delete contract and routing
- [x] Extend declarations, version gates, generated schema/validator, protocol and native chord validation.
- [x] Route only declared, unmodified, non-repeated Delete with empty input, outside IME and menus.
Files: launcher-core/view, protocol, plugin_panel.rs, manifest.rs, plugin-cli manifest/schema, SDK docs.
Dependencies: specification Current API gap and Proposed Host/API behavior.
Verify: npm test -- src/launcher.test.tsx -t 'Delete'; CLI manifest tests; cargo test --lib delete_host_key.
Coverage: text protects editing; modifiers/IME/repeat do not enqueue; undeclared keys stay native; Delete never yields an Enter paste ticket.

### Task 2: Plugin actions and status
- [x] Register category and selected-item actions, share remove logic, guard stale selection/session and duplicate mutations.
- [x] Move feedback to Host; update preview, package version, minimum Host, README and SDK tests.
Files: examples/public-plugins/com.uipilot.clipboard-history/.
Dependencies: Task 1; specification Plugin-side changes.
Verify: node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/*.test.js; SDK typecheck; CLI validate.
Coverage: open action then change selection/session -> old callback inert; pending delete then new session -> old completion cannot publish status/focus; failed delete retains record; empty list disables delete.

### Task 3: Integration verification
- [x] Run focused Host/plugin tests and production frontend build; review patch against acceptance.
- [x] Provide manual acceptance steps without operating on real clipboard data.
Dependencies: Tasks 1 and 2; specification Acceptance.
Verify: npm run build; focused native tests and package validation.

Verification: 34 plugin tests, 60 CLI manifest tests, frontend Delete/routed-key regression tests, 83 native Panel tests and native manifest Delete gate passed. SDK typecheck and production frontend/CLI builds passed. Independent review completed and focus-stealing issue fixed. Final reviewed plugin archive is supplied for manual installation; live clipboard acceptance is left to the user.

Final verification: full protocol/Panel bridge suites 33 passed; Rust-generated schema equality passed (resource copying disabled only for generator process to avoid a running resource file lock). Final archive validation succeeded and every packaged file was compared byte-for-byte with the source.
