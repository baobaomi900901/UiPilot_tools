# Plugin browser URL actions implementation plan

> Execute with subagent-driven-development, one source writer and one bounded combined review for the implementation slice.

**Goal:** Ship Host0.12 and text-snippet1.1 with explicit authorized default-browser opening through existing result actions.

**Architecture:** Follow [URL action design](../specs/2026-10-26-plugin-url-actions-design.md), especially Manifest/version, Validation, Admission and Frontend outcome contracts. Keep private URL authority inside existing result snapshots and reuse the native browser adapter. No new Runtime API, IPC or controller.

**Technology:** Existing Rust/Tauri, url crate, TypeScript/React, plain JS plugin and CLI. No dependency installation or new framework.

## Global constraints and execution

- Use existing managed W at C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools. D checkout tracked sources stay read-only. Process-only CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target is allowed.
- One writer; preserve prior accepted evidence. Focused RED for changed behavior, minimal implementation, focused GREEN, one build after final source, atomic commit and independent task review. No root verify/historic whole-branch review.
- Agents use fake browser adapters and temporary stores. Real browser/system/clipboard/data effects are exclusively user manual acceptance; no native App launch by agents.
- Task 1 -> reviewed source freeze -> Task 2. Spec is authoritative; implementation surprises must be resolved concretely before silently expanding scope.

### Task 1: Authorized URL result actions and existing-plugin workflow

**Files:** src-tauri/src/public_plugins/{manifest.rs,result_action.rs,activation.rs}, public_plugins.rs, result_registry.rs, commands.rs and directly affected native tests; src/{protocol.ts,launcher-core.ts,public-plugin-panel.tsx} and affected tests; SDK types/docs/schema and result-action manifest parity fixture; CLI manifest/contracts/schema/generated validator and affected tests; text-snippet package/runtime/tests/README; root package/lock, Cargo/lock, tauri version metadata. Keep browser_open behavior for existing builtins unchanged.

**Dependencies:** Design Manifest and version / Result and URL validation / Admission / Frontend / Existing example sections; old result action contract remains in force.

- [ ] Add url.open Windows permission with minimumHost0.12/resultActions/mainResult constraints and consent label; maintain Host/CLI/schema parity and update actual Host current + release version sources.
- [ ] Add exact openUrl action with strict shared host validator; private canonical URL, complete snapshot {copy,url} requirements across initial publication, resolve, Runtime admission/replacement and static execution.
- [ ] Reuse checked static admission and native adapter; implement urlOpened/urlOpenFailed, single source claim, grant/generation/lease enforcement, owned frontend feedback and explicit rerun behavior.
- [ ] Update text-snippet1.1.0 without changing storage/IDs/default copy; secondary action only on valid original/latest URL text, never uppercase row. Update concise SDK/tutorial/example docs.

**Distinct test coverage:** Invalid/oversized URL and empty-userinfo/repair forms reject before any opener; undeclared/revoked source permissions or stale/disabled/replaced identity yield zero effect; first valid admission opens once and consumes siblings, replay/open failure cannot open again; Runtime replacement retains required grants. A query/source change during an admitted effect suppresses old UI completion. Confirm both legacy copy and Runtime actions remain valid. Manifest cases cover missing opt-in, wrong platform, low/current minimum and valid old package. Example proves plain text/data/copy/save unchanged and original/latest action eligibility.

**Verify:** Filtered `cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib result_action` plus new URL-validator/manifest test filters; `npm test -- src/protocol.test.ts src/plugin-result-action-bootstrap.test.ts` and targeted launcher/consent tests; CLI manifest/parity affected tests and `npm run check --workspace @uipilot/plugin-cli`; `node --test examples/public-plugins/com.uipilot.text-snippet/tests/runtime.test.js` plus existing SDK typecheck mechanism. Regenerate Rust schema, sync CLI schema and validator; verify freshness. Final `npm run build`. Run additional checks only when a concrete changed contract warrants them. Record exact results and selected skips; retain known Vite size and unrelated baseline issues separately.

### Task 2: Separate build, package and manual handoff

**Files:** ignored .superpowers/deliveries/uipilot-0.12.0-acceptance; this plan and roadmap completion notes.

**Dependencies:** Task1 independent Spec/Quality pass. No implementation pending.

- [ ] Build CLI once and pack text-snippet1.1.0 to fresh archive (no execution or overwrite), validate artifact; copy existing resources into new acceptance kit.
- [ ] Freeze clean source/dist. Build native offline with existing acceptance identifier/product name. Confirm actual Windows version, hashes/resources/package and preserved prior executables; no App launch.
- [ ] Record source-vs-doc commit provenance and short manual cases; core manual results pending until user reports them.

**Distinct test coverage:** Directory and archive static validation, version/build input identity, old binaries unchanged. Manual consent denial/acceptance, Ctrl+K opening, no automatic open, consumed state/rerun and existing data/shortcut preservation. No repetitive broad matrix.

**Verify:** existing CLI pack/validate commands and acceptance verifier adapted to0.12; `cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked` with process-only CARGO_TARGET_DIR and TAURI_CONFIG for com.uipilot.launcher.acceptance / UiPilot Acceptance. Build once after review; preserve source and output hashes.