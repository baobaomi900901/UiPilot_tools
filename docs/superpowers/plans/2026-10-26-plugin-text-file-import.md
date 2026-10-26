# User-selected text file import implementation plan

Goal: Host0.13.0 and JSON workbench1.1.0 import a user-selected bounded UTF-8 text file through a window capability, keeping explicit format/save behavior.

Architecture: [file import design](../specs/2026-10-26-plugin-text-file-import-design.md) is the source of truth. Reuse WindowActionIdentity, manager authority, native dialog dependency and existing JSON workbench. Add a dedicated narrow file-read module and nonblocking picker permit, not a Runtime scheduling path.

## Global constraints

Windows local-file scope; no dependency installation, arbitrary paths, writes, Runtime API or persistent handles. Existing workspace C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools; D checkout source read-only, process-only CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target allowed. Host development explicitly authorized. User executes native manual tests; agents use fake dialogs/temp fixtures. One source writer, focused RED/GREEN, atomic task commit and combined independent Spec/Quality review; no broad historical reruns. Task1 -> review/source freeze -> Task2.

### Task 1: Window text-file capability and JSON import workflow

**Files:** new src-tauri/src/public_plugins/file_open.rs for error/result/reader and narrow testable adapter orchestration as appropriate; public_plugins module exports, activation.rs authorization, plugin_window.rs controller/bootstrap/native ownership, commands.rs and lib.rs/build.rs plus window-content ACL; manifest/schema/SDK docs/types and CLI validator/contracts/parity tests; frontend permission label; JSON workbench package/tests/README; all actual Host/release/CLI version sources. Read repository UI and public plugin guides first. Keep changes scoped; no unrelated refactors.

**Dependencies:** Design Public contract and versions, Authority and native lifetime, Bounded local read, Existing JSON workbench, Verification and delivery. Exact values/errors/API in design govern.

- [ ] Implement permission/version parity and session-captured optional files.openText facade plus private caller-limited IPC.
- [ ] Implement exact-identity authorization, global physical permit and scoped auto-blur guard without long leases; rechecks and guarded focus restoration; bounded strict local readonly reader.
- [ ] Add JSON workbench import/confirmation/edit ownership and error feedback while retaining original storage/revision/format/save contracts.
- [ ] Add focused behavior tests and update SDK/tutorial/example docs; record complete verification evidence and commit clean final source.

**Distinct test coverage:** All ordered permission, closure/refresh/late-result, permit/guard cleanup, bounded-reader and draft-protection cases in Verification and delivery. No native dialog launch. Test manager authority and controller lifecycle concretely, not only source string matching. Adapt only directly broken version/schema expectations.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib file_open` plus affected `plugin_window::tests` and manifest filters; targeted bootstrap tests; `npm test --workspace @uipilot/plugin-cli --` with affected manifest/validation filters and `npm run check --workspace @uipilot/plugin-cli`; `node --test examples/public-plugins/com.uipilot.json-workbench/tests/*.test.js` (PowerShell enumerate explicit paths if required), existing SDK contract typecheck. Generate/sync schemas and validator then freshness checks. `npm run build` once final source; further checks only for new changes/failures. Do not run root verify.

### Task 2: Package and user-operated acceptance handoff

**Files:** ignored .superpowers/deliveries/uipilot-0.13.0-acceptance, this plan, roadmap/progress/RESUME records.

**Dependencies:** Task1 source committed and independent Spec/Quality PASS.

- [ ] Build CLI, pack JSON workbench1.1.0 fresh archive and validate; copy resources and generate tiny nonprivate manual fixtures.
- [ ] Freeze source/dist, offline native build with com.uipilot.launcher.acceptance / UiPilot Acceptance, verify actual0.13.0 PE/hash/input stability and14 old binaries unchanged.
- [ ] Record build-vs-doc provenance and pending manual cases; hand off first short user group.

**Distinct test coverage:** Artifact/static validation and build identity. No agent runtime/native/system effects. User checks upgrade consent and import/format/save/reopen, then cancel/error/edited draft and theme behavior. Never infer native pass from mock tests.

**Verify:** CLI pack/validate JSON reports, acceptance verifier, `cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked` with process-only Cargo target and acceptance TAURI_CONFIG. No rebuild for docs-only changes.
