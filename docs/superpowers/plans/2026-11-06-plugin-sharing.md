# Plugin sharing implementation

Implement OCR edited text → Ctrl+K → Share → selected receiving Panel, with translation receiving into its editor and starting translation once. The host discovers declared receivers; plugins own business processing. The governing contract is [the approved sharing design](../specs/2026-11-06-plugin-sharing-design.md), sections 3–8 and its acceptance matrix in section 10.

Architecture: an independent ShareCoordinator owns bounded snapshots, tickets, deadlines and delivery intent. Panel lifecycle gains one pending session alongside its active session. Native caller checks, restricted preparation, a commit gate and the generated bridge enforce the handoff. Existing ordinary Panel behavior remains compatible.

## Technology and global constraints

- Rust/Tauri 2, React 19/TypeScript/Vitest, Node plugin tests; Host 0.22.0, API/schema v1, share v1.
- Exact public signatures, byte limits, states, timeouts, identity, event ordering and failure behavior follow design sections 3–7; do not substitute ordinary teardown-first mounting or put shared text into launcher input.
- One active plus at most one pending Panel. Hidden target remains inaccessible to input until acceptance or confirmed cancellation. Native IPC checks the actual Webview and preparation phase.
- Only explicit user selection starts a transfer. No plugin enumeration/send API, body broadcast, automatic retry, persistent body or provider calls during tests.
- Keep existing unrelated dirty changes intact. Work on the already selected `codex/ocr-panel` branch. Completion notifications disabled.
- No Computer Use/native automation, installation, real clipboard or provider access. Native focus/theme/provider acceptance remains manual; mock Runtime/Webview and JSDOM tests do not prove native acceptance.

## Execution

Each task uses focused failing tests, minimum implementation, focused passing verification and an atomic commit excluding preexisting changes. Each task receives specification and quality review before dependent work. Commands run from the repository root unless a working directory is stated. Run one Cargo process at a time. Persist brief, report, review and progress evidence under `.superpowers/sdd/sharing/`.

Dependency order: Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6. Tasks 2–4 form an internal integration boundary and are not independently advertised as usable sharing.

### Task 1: Manifest, SDK and Host version contracts

**Files:** Rust `public_plugins/manifest.rs` and focused manifest tests; `packages/plugin-cli/src/manifest.ts` and tests/generated validator; SDK schema and TypeScript declaration; package/Cargo/Tauri version and lock files.

**Dependencies:** Design sections 3–5 and 9.

- [ ] Add strict optional `share` declarations and receiver validation consistently to Rust, schema and CLI, preserving legacy commandId omission.
- [ ] Declare optional Panel share APIs, readonly DTOs and optional `sharePrepare` entry for Runtime context and Panel update. Runtime implementation follows in Task 3.
- [ ] Raise Host/default CLI target to 0.22.0 while preserving old capability floors. Keep prose docs for Task 6.

**Distinct test coverage:** valid source/receiver/both; unknown/null/duplicates, unsafe IDs, Unicode title/control limits, receiver count/type/byte limits, platform/output/multi-command/minimumHost rejection; old packages unchanged; SDK positive/negative contracts.

**Verify:** `npm.cmd test --workspace @uipilot/plugin-cli -- manifest`; `npm.cmd run check --workspace @uipilot/plugin-cli`; `cargo test public_plugins::manifest` in `src-tauri`; focused SDK compile via `node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict <contract-test>`.

### Task 2: ShareCoordinator and discovery model

**Files:** new `src-tauri/src/plugin_share.rs` and submodules/tests; module registration; narrow manifest catalog access in `public_plugins/activation.rs`.

**Dependencies:** Task 1; design sections 3–7.

- [ ] Implement bounded payload/snapshot validation, identity-bound menu/selector tickets and stable receiver discovery without executing candidates.
- [ ] Implement state transitions, commit prerequisites, one-time body claim/ACK, local-cancellation qualification, deadline/recovery cleanup and body-free spent metadata using injected time/identities.
- [ ] Provide explicit coordinator operations usable by Task 3 without embedding native calls under its commit lock.

**Distinct test coverage:** immutable snapshots and source confirmation; invalidation-before-commit versus commit-before-invalidation; disabled/updated/self/size-limited receivers; claim lost/repeated/late and ACK late versus revoked; active cancellation versus unexpected document loss; recovery expiry never extended; metadata/body release at every terminal path. Real orchestration belongs to Task 3.

**Verify:** `cargo test plugin_share` in `src-tauri`.

### Task 3: Native handoff, restricted preparation and production bridges

**Files:** `plugin_panel.rs` and focused lifecycle submodules; `public_plugins.rs`, `public_plugins/{activation,runtime,scheduler}.rs`; `commands.rs`/focused share commands; `lib.rs`, lifecycle/ACL command registrations; new generated share bridge; production bootstrap and mock IPC tests.

**Dependencies:** Task 2; design sections 2, 4–7 and acceptance 3, 5–11, 13, 15.

- [ ] Add separate hidden pending session and empty `sharePrepare` initialization, without replacing source DOM/image or sending ordinary Ready before commit.
- [ ] Enforce preparation read-only restrictions at native Runtime and Panel IPC boundaries; cancellation revokes logical permission while retaining physical execution occupancy.
- [ ] Serialize commit and lifecycle invalidation; implement reversible native preparation, active target promotion, source revocation, hidden delivery, acceptance exposure and safe recovery/cancel handshake.
- [ ] Wire actual source snapshot/invalidation/confirmation, synchronous Ready barrier, one-shot private claim/receive/ACK and local pending/consumed/revoked bridge state. Public registration cannot mint gesture tickets.
- [ ] Expose strictly validated host-only discovery/selection/state/cancel/copy commands and events for Task 4. Record exact frontend wire signatures in the task report.

**Distinct test coverage:** actual generated IPC distinguishes main/child/other/stale caller; prepare effects rejected even through private IPC; source survives prepare failure; target Busy no queue; Ready/offer/claim/handler/ACK ordering through production bootstrap; Promise Ready rejected only for sharing; cancellation ACK lost destroys document before input; delayed claim after continued editing never calls handler; native preparation failure before commit versus display failure after Accepted; source/target lifecycle invalidation serialized against commit and ACK. Use mock native adapters, not live desktop control.

**Verify:** `cargo test plugin_share` and `cargo test plugin_panel` in `src-tauri`; `npm.cmd test -- src/plugin-panel-share-bootstrap.test.ts src/plugin-panel-action-bootstrap.test.ts src/plugin-panel-image-bootstrap.test.ts`; additional exact IPC/service filter recorded in report.

### Task 4: Launcher share selector and recovery UI

**Files:** `src/launcher-{core,view}.tsx` or existing `.ts` equivalent, `protocol.ts`, `public-plugin-panel.tsx`, focused share components/styles/tests, `panel-context-actions.js` if needed.

**Dependencies:** Task 3 frontend wire interfaces; design sections 6, 8; `docs/ui-guidelines.md`.

- [ ] Add host-owned Share action, searchable receiver selection with limits/reasons and deterministic IDs, preparing state and cancellation.
- [ ] Keep source focus/selection on cancelled selection; disable host input and hide native target through delivery; show target tag/empty input after accepted.
- [ ] Add bounded recovery text, explicit copy/continue/close, and route all exit/new navigation through revocation or document destruction.

**Distinct test coverage:** Ctrl+K from host/content focus; two receivers and no automatic choice; IME/held Enter; Esc/Ctrl+K/Tab trapped; no body in launcher query/history; accepted vs unknown/failed keyboard ownership; continue awaits cancellation ACK and late response cannot reopen; Light/Dark tokens and accessible labels.

**Verify:** `npm.cmd test -- src/plugin-share.test.tsx src/launcher.test.tsx src/panel-context-actions.test.ts`; `npm.cmd run build`.

### Task 5: OCR and translation sharing integration

**Files:** `examples/public-plugins/com.uipilot.{ocr,translate}/package/{plugin.json,dist/*}`, plugin tests and README.

**Dependencies:** Tasks 1–4; design sections 4–5, 6.2–6.3, 9–10; public-plugin skill/guide/UI guidelines.

- [ ] OCR source shares current edited valid full text; content/availability changes increment revision, theme/focus do not.
- [ ] Translation declares receiver and translated-text source; prepare without business, synchronous receipt writes editor and defers one automatic translation; newer input guards stale work.
- [ ] Bump OCR to 1.1.0 and translation to 1.5.0, minimum Host 0.22.0. Keep IDs/configuration and normal entry behavior.

**Distinct test coverage:** OCR edits/clear/loading/failure source invalidation, exact whitespace/byte bounds; translation sharing vs ordinary/selection entries; no provider call before receive, exactly one after receive; preferences ready before Prepared; user edits beat old async translation; missing service config retains received text; translated output can be shared by new user action.

**Verify:** `node --test examples/public-plugins/com.uipilot.ocr/tests/*.test.js examples/public-plugins/com.uipilot.translate/tests/*.test.js`; compile both SDK contract tests; CLI validate both packages.

### Task 6: Integrated acceptance, developer documentation and packages

**Files:** focused integration fixtures/tests, SDK prose/developer guide, plugin examples/skill relevant paragraphs, manual acceptance record, delivery archives under `artifacts/sharing-delivery/`.

**Dependencies:** Tasks 1–5; design section 10 complete matrix.

- [ ] Exercise production host scheduling/bridges with fake OCR, translation and an independent draft-only receiver; demonstrate user-selected delivery and plugin-owned business decision.
- [ ] Document source/receiver authoring, preparation restrictions, cancellation/Ready contracts, versions and honest native acceptance boundary.
- [ ] Build CLI, pack and validate new archives without overwriting old deliveries; verify archive/source parity.
- [ ] Run full relevant suites/build/type checks, review entire feature delta and resolve findings. Preserve a final evidence report including manual native checklist.

**Verify:** `npm.cmd test`; `cargo test --lib` in `src-tauri`; `npm.cmd run verify --workspace @uipilot/plugin-cli`; `npm.cmd run build`; plugin tests/SDK checks and production CLI pack/archive validation. Do not repeat unchanged full suites after purely documentary edits.
