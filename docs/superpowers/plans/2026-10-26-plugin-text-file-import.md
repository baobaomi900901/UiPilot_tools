# User-selected text file import implementation plan

Goal: Host0.13.0 and JSON workbench1.1.0 import a user-selected bounded UTF-8 text file through a window capability, keeping explicit format/save behavior.

Architecture: [file import design](../specs/2026-10-26-plugin-text-file-import-design.md) is the source of truth. Reuse WindowActionIdentity, manager authority, native dialog dependency and existing JSON workbench. Add a dedicated narrow file-read module and nonblocking picker permit, not a Runtime scheduling path.

## Global constraints

Windows local-file scope; no dependency installation, arbitrary paths, writes, Runtime API or persistent handles. Existing workspace C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools; D checkout source read-only, process-only CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target allowed. Host development explicitly authorized. User executes native manual tests; agents use fake dialogs/temp fixtures. One source writer, focused RED/GREEN, atomic task commit and combined independent Spec/Quality review; no broad historical reruns. Task1 -> review/source freeze -> Task2.

### Task 1: Window text-file capability and JSON import workflow

**Files:** new src-tauri/src/public_plugins/file_open.rs for error/result/reader and narrow testable adapter orchestration as appropriate; public_plugins module exports, activation.rs authorization, plugin_window.rs controller/bootstrap/native ownership, commands.rs and lib.rs/build.rs plus window-content ACL; manifest/schema/SDK docs/types and CLI validator/contracts/parity tests; frontend permission label; JSON workbench package/tests/README; all actual Host/release/CLI version sources. Read repository UI and public plugin guides first. Keep changes scoped; no unrelated refactors.

**Dependencies:** Design Public contract and versions, Authority and native lifetime, Bounded local read, Existing JSON workbench, Verification and delivery. Exact values/errors/API in design govern.

- [x] Implement permission/version parity and session-captured optional files.openText facade plus private caller-limited IPC.
- [x] Implement exact-identity authorization, global physical permit and scoped auto-blur guard without long leases; rechecks and guarded focus restoration; bounded strict local readonly reader.
- [x] Add JSON workbench import/confirmation/edit ownership and error feedback while retaining original storage/revision/format/save contracts.
- [x] Add focused behavior tests and update SDK/tutorial/example docs; record complete verification evidence and commit clean final source.

**Distinct test coverage:** All ordered permission, closure/refresh/late-result, permit/guard cleanup, bounded-reader and draft-protection cases in Verification and delivery. No native dialog launch. Test manager authority and controller lifecycle concretely, not only source string matching. Adapt only directly broken version/schema expectations.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib file_open` plus affected `plugin_window::tests` and manifest filters; targeted bootstrap tests; `npm test --workspace @uipilot/plugin-cli --` with affected manifest/validation filters and `npm run check --workspace @uipilot/plugin-cli`; `node --test examples/public-plugins/com.uipilot.json-workbench/tests/*.test.js` (PowerShell enumerate explicit paths if required), existing SDK contract typecheck. Generate/sync schemas and validator then freshness checks. `npm run build` once final source; further checks only for new changes/failures. Do not run root verify.

### Task 2: Package and user-operated acceptance handoff

**Files:** ignored .superpowers/deliveries/uipilot-0.13.0-acceptance, this plan, roadmap/progress/RESUME records.

**Dependencies:** Task1 source committed and independent Spec/Quality PASS.

- [x] Build CLI, pack JSON workbench1.1.0 fresh archive and validate; copy resources and generate tiny nonprivate manual fixtures.
- [x] Freeze source/dist, offline native build with com.uipilot.launcher.acceptance / UiPilot Acceptance, verify actual0.13.0 PE/hash/input stability and14 old binaries unchanged.
- [x] Record build-vs-doc provenance and pending manual cases; hand off first short user group.

**Distinct test coverage:** Artifact/static validation and build identity. No agent runtime/native/system effects. User checks upgrade consent and import/format/save/reopen, then cancel/error/edited draft and theme behavior. Never infer native pass from mock tests.

**Verify:** CLI pack/validate JSON reports, acceptance verifier, `cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked` with process-only Cargo target and acceptance TAURI_CONFIG. No rebuild for docs-only changes.

## Delivery status — 2026-10-26

Task1 implementation a33bcaba349e01d632c1b9263316609904b92fcb; independent review found one P2 queued-native-focus race, fixed in a93acb20421bc2a738db1d785153604a3d7faac9. Delta Spec/Quality PASS, no remaining actionable findings. Final relevant native10+controller27+window-action7, earlier manifest14, CLI84, bootstrap/consent9 and example25 checks passed with typechecks/schema freshness. Frontend build passed once; Rust-only focus fix preserved dist hashes. Existing Vite chunk and lifecycle dead-code warnings remain non-blocking.

Task2 delivered .superpowers/deliveries/uipilot-0.13.0-acceptance with executable, five resources, JSONworkbench1.1 archive and six generated manual fixtures. CLI build/pack/directory+archive validation and offline native build passed. Actual PE0.13.0, acceptance identity,40,129,536 bytes; SHA25619560eb7c5367f5fd50cc5a5c6a152145b9bf24ec43bf33f5b8ddcea86f8bb42. Archive SHA25687e0472b2fc2eaf972e9d4141008f2cbd74abab46916c0a93d7475f23e658762.746 tracked/dist inputs unchanged,14 earlier executables preserved; generated fixture hashes matched.

All new native/UI manual acceptance remains pending. First group: version0.13,permission update cancel/confirm,validJSON import into unsaved draft,explicitformat/save/reopen. No real dialogs/app/browser/clipboard/network/userdata effects byagents. This is one M5 file capability, not fullM5 or foundation completion. Source-vs-doc provenance is recorded in kit after this documentation commit.