# Local developer refresh and recovery implementation plan

**Goal:** Ship Host 0.7.0 with an explicit local developer refresh, useful fault recovery and privacy-bounded diagnostic preview/copy, plus local manual acceptance fixtures.

**Architecture:** Reuse the existing staged prepare/commit and disabled-plugin enable transactions. [Design](../specs/2026-10-25-developer-recovery-design.md) is authoritative for wire formats, ordering, failure semantics and exclusions. Do not implement a second activation pipeline.

**Technology:** Existing Rust/Tauri 2, React 19, TypeScript 7 and offline plugin CLI; no new dependencies.

## Global constraints

- Only the main Host window may invoke management/diagnostic/copy commands.
- Development-directory refresh is an explicit same-version snapshot operation; no watcher, remembered path, healthy Runtime restart, automatic migration or package-history rollback.
- Preserve caller/token binding, exact consent, target-state revalidation, readiness and durable outcome handling. Already-admitted effects may complete; no new migration barrier.
- Diagnostics are explicit allowlist DTOs, maximum 16 KiB, with at most 64 in-memory latest-operation slots. Never include settings, content, paths, secrets, arbitrary errors or request identifiers.
- Manual functional acceptance is primary per user instruction. Keep focused authorization, persistence, stale-owner and privacy regressions plus builds. No mechanical per-file TDD or unrelated full-suite reruns.
- Work only in C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools. D:/code/UiPilot_tools source is read-only; its existing node_modules and src-tauri/target cache may be used. Do not launch Host, install plugins, touch credentials/services or use the network.
- Task 1 -> Task 2. One production writer at a time; each task commits only its changes and provides an ignored report. Review Task 1 before Task 2 changes source; final review covers this bounded vertical only.
- Rust commands use process-local CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target and --manifest-path src-tauri/Cargo.toml --offline. Build artifacts and reports remain ignored; only required plugin package dist assets may be explicitly force-added.
- Keep branch/worktree; no merge, push or cleanup. Read AGENTS.md, UI guidelines and relevant SDK contracts before editing.

### Task 1: Host developer refresh, recovery and diagnostics

**Files:** src-tauri/src/public_plugins/activation.rs, a focused public_plugins/diagnostics.rs module, public_plugins.rs, commands.rs, lib.rs; src/protocol.ts, main.ts, public-plugin-panel.tsx and focused helper/CSS/tests as appropriate; package.json/package-lock.json, src-tauri/Cargo.toml/Cargo.lock/tauri.conf.json, packages/plugin-cli/src/contracts.ts and directly affected tests; docs/plugin-sdk/public-plugin-developer-guide.md and public-plugin-v1.md.

**Dependencies:** Design sections Exact scope and interface changes, Ordering identity and failure behavior, Sanitized diagnostic contract; existing transaction, PublicPluginDetail, PublicPluginSettings, first-party clipboard backend.

- [x] Add targeted refresh input and token-bound intent without duplicating activation. Expose operation/previous version in existing confirmation.
- [x] Record bounded observational outcomes at real lifecycle branches, without changing transaction decisions/lock order.
- [x] Add main-window diagnostics lookup and exact-preview copy, strict Rust/TS shape/value/size validation.
- [x] Add detail refresh/re-enable and diagnosis preview/copy; reuse confirmation UI. Consume prepared UI token after every commit attempt; separate mutation success from refresh failure; guard stale async completions and duplicate mutations.
- [x] Add fixed safe Chinese errors and truthful persistence wording; no raw error interpolation. Preserve existing permission/network credential confirmation.
- [x] Synchronize release metadata/current CLI target to 0.7.0 without changing historical API capability minima. Document snapshot semantics, package versus data version and no automatic rollback/migration.
- [x] Self-review, commit and report code changes, focused commands/output and remaining manual checks.

**Distinct coverage:** Wrong main-window caller rejected before protected state access; wrong plugin/version/stale inventory rejected; caller/target token cannot be repurposed; target changes after prepare reject while unrelated later changes do not silently replace target expectations; bad readiness preserves previous package/config; verified NotCommitted versus Unknown/publication failure diagnostics; an earlier slow operation cannot overwrite a newer observation; strict diagnostic fields reject sentinels/unknown keys/noncanonical scalars and copy emits exactly preview; old UI request cannot update another detail session; commit failure clears consumed preparation; mutation success plus inventory failure is not reported as mutation failure.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --offline public_plugins::diagnostics; cargo test --manifest-path src-tauri/Cargo.toml --offline public_plugins::activation::tests; cargo check --manifest-path src-tauri/Cargo.toml --offline; npm test -- src/public-plugin-recovery.test.tsx src/public-plugin-diagnostics.test.ts; npm run build; npm run build --workspace @uipilot/plugin-cli. Use actual discovered focused names in report if harness layout requires adjustment. Re-run only affected existing protocol/management tests selected by name, not entire launcher suite.

### Task 2: Offline recovery fixtures and runnable acceptance kit

**Files:** examples/public-plugins/com.uipilot.demo-recovery/{README.md,package.json,package/plugin.json,package/dist/runtime.js,tests/runtime.test.js,scripts/create-fixtures.mjs}; ignored .superpowers/deliveries/uipilot-0.7.0-acceptance/; directly affected verification discovery only if needed. Host sources and SDK contracts remain read-only for this task.

**Dependencies:** Task 1 reviewed, Host0.7 contracts fixed; design sections Plugin data/version contract and independent fixture and Acceptance and minimal automated checks. Closest example demo-multi; no new capability or panel business UI.

- [x] Implement a small submit/mainResult recovery demo using stable plugin/command identity, persisted bounded schema-1 counter snapshot, read-only status/default-setting display, explicit increment and controlled fault input.
- [x] Generate separate A/B same-version, readiness-broken, throwing, compatible upgraded and synthetic schema-2 writer/reader directories. Unknown/malformed schema blocks all writes; schema2 fixture is disposable and explicitly invoked.
- [x] Keep fixtures strictly offline and harmless, with distinct source-path/content/error/setting sentinels for diagnostic privacy checks.
- [x] Validate source/package artifacts and add minimal data-preservation/single-write/uncertain-save tests.
- [x] Build a new acceptance executable using existing acceptance identifier and embedded frontend; copy the five required resources. Preserve all previous kits; never launch it.
- [x] Record exact source revision/input hashes, executable/resource/archive hashes, build exit evidence and Chinese manual instructions. Use visible full paths and distinguish folder selection from archive selection.
- [x] Commit tracked example changes only, report artifact provenance separately; all new manual cases remain pending until user reports.

**Distinct coverage:** Missing storage read does not write; valid increment writes one complete snapshot; unknown/newer/malformed schema remains byte-for-byte unchanged; synthetic schema2 survives older reader; write failure returns uncertainty rather than claiming rollback; Runtime throw never echoed as a report field.

**Verify:** node --test examples/public-plugins/com.uipilot.demo-recovery/tests/runtime.test.js; node --check examples/public-plugins/com.uipilot.demo-recovery/package/dist/runtime.js; CLI validate and pack each emitted fixture under Windows target, then validate archives. Native build: cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked with TAURI_CONFIG={"identifier":"com.uipilot.launcher.acceptance","productName":"UiPilot Acceptance"} after npm run build. Restore process environment afterwards.

## Delivery checkpoint

Independent final review; verify artifact hashes/source provenance. Hand user short initial manual checks and full Chinese guide, with remaining cases clearly pending. This completes only the developer recovery slice, not all M6/M7 or the entire Raycast-like foundation.

Status2026-10-25: Task1 implemented23bdf65, fixed728db33; Task2 source d845230. Host0.7.0 kit built and independently verified: 688 unchanged inputs, six plugin archives, five resources. Final independent SpecPASS/QualityApproved; C0/I0, existing non-blocking Vite warning retained. Ready for user acceptance; all nine native/manual cases remain pending. Later commits are documentation only, not new binary source.
