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

Dependency order: Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7 → Task 8 → Task 9. Tasks 2–7 form an internal integration boundary and are not independently advertised as usable sharing.

### Task 1: Manifest, SDK and Host version contracts

**Files:** Rust `public_plugins/manifest.rs` and focused manifest tests; `packages/plugin-cli/src/manifest.ts` and tests/generated validator; SDK schema and TypeScript declaration; package/Cargo/Tauri version and lock files.

**Dependencies:** Design sections 3–5 and 9.

- [ ] Add strict optional `share` declarations and receiver validation consistently to Rust, schema and CLI, preserving legacy commandId omission.
- [ ] Declare optional Panel share APIs, readonly DTOs and optional `sharePrepare` entry for Runtime context and Panel update. Runtime implementation follows in Task 3.
- [ ] Raise Host/default CLI target to 0.22.0 while preserving old capability floors. Keep prose docs for Task 9.

**Distinct test coverage:** valid source/receiver/both; unknown/null/duplicates, unsafe IDs, Unicode title/control limits, receiver count/type/byte limits, platform/output/multi-command/minimumHost rejection; old packages unchanged; SDK positive/negative contracts.

**Verify:** `npm.cmd test --workspace @uipilot/plugin-cli -- manifest`; `npm.cmd run check --workspace @uipilot/plugin-cli`; `cargo test --lib public_plugins::manifest` in `src-tauri`; focused SDK compile via `node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict <contract-test>`.

### Task 2: ShareCoordinator and discovery model

**Files:** new `src-tauri/src/plugin_share.rs` and submodules/tests; module registration; narrow manifest catalog access in `public_plugins/activation.rs`.

**Dependencies:** Task 1; design sections 3–7.

- [ ] Implement bounded payload/snapshot validation, identity-bound menu/selector tickets and stable receiver discovery without executing candidates.
- [ ] Implement state transitions, commit prerequisites, one-time body claim/ACK, local-cancellation qualification, deadline/recovery cleanup and body-free spent metadata using injected time/identities.
- [ ] Provide explicit coordinator operations usable by Task 3 without embedding native calls under its commit lock.

**Distinct test coverage:** immutable snapshots and source confirmation; invalidation-before-commit versus commit-before-invalidation; disabled/updated/self/size-limited receivers; claim lost/repeated/late and ACK late versus revoked; active cancellation versus unexpected document loss; recovery expiry never extended; metadata/body release at every terminal path. Real orchestration belongs to Tasks 4–6.

**Verify:** `cargo test --lib plugin_share` in `src-tauri`.

### Task 3: Restricted preparation Runtime authority

**Files:** `public_plugins/{scheduler,runtime,activation}.rs`, `public_plugins.rs`, focused Runtime/service tests.

**Dependencies:** Task 2; design sections 6.2 and 7; `.superpowers/sdd/sharing/host-integration-map.md` scheduling/authorization map.

- [ ] Add host-owned prepare request authority, empty onCommand input/no image and optional context.entry without changing ordinary legacy invocation.
- [ ] Fail Busy without queue/retry; logical cancellation revokes API authority while preserving the physical slot/original deadline until completion.
- [ ] Allow only own storage.get/settings.get/isSecretConfigured, rejecting effects at native scheduler/API/network/service admission, not only facade.
- [ ] Return initialization result to the pending handoff owner, never launcher result publication. Provide explicit preparation APIs for Task 4.

**Distinct test coverage:** real scheduler/service prepare admission with Busy and cancellation/late completion; malicious direct native API calls for each effect family denied with already-granted install permissions; read calls allowed only while current; normal commands/actions remain unchanged.

**Verify:** `cargo test --lib share_prepare` and `cargo test --lib public_plugins::scheduler` in `src-tauri`; production Runtime bootstrap compatibility via `npm.cmd test -- src/plugin-panel-action-bootstrap.test.ts`.

### Task 4: Pending Panel lifecycle and commit serialization

Execute this task through the sequential [Panel lifecycle subplan](2026-11-06-plugin-sharing-lifecycle.md), with independent review after 4A, 4B and 4C, and a combined Task 4 review after 4D. The following original requirements remain the aggregate acceptance gate.

**Files:** `plugin_panel.rs` and focused submodules, `plugin_share.rs`, manager/controller lifecycle authorization seams, mock native adapter tests.

**Dependencies:** Task 3; design sections 6.2–6.5, 7; host integration map lock-order/lifecycle sections.

- [ ] Add one hidden pending session retaining source DOM/image/model; use separate mount/bounds/ready path, not ordinary teardown-first mount.
- [ ] Establish shared commit/lifecycle serialization or equivalent atomic identity lease with a documented lock order; move teardown/effects outside short state critical sections.
- [ ] Implement reversible native preparation, target promotion/source authority revocation, hidden committed target, accepted exposure and safe cancellation/recovery intent termination.
- [ ] Classify unexpected document loss versus intentional navigation/hide/revocation; cover pending and active identities in all lifecycle hooks.
- [ ] Enforce pending private IPC read-only classes and unaccepted input barrier, including helpers bypassing begin_content_call. Provide bridge/host orchestration interfaces in report.

**Distinct test coverage:** source object/DOM adapter identity retained on preparation failure; invalidation before versus after commit yields one terminal outcome; manager permission/settings/update reservations cannot race promotion; native precommit failure restores only valid source, postaccept display failure never rolls back; source cleanup after commit does not cancel target; revoke ACK loss destroys before input, unexpected preaccept target loss keeps bounded recovery, intentional revoke releases immediately.

**Verify:** `cargo test --lib plugin_share` and `cargo test --lib plugin_panel` in `src-tauri`; exact focused authorization/lifecycle filters recorded in report.

### Task 5: Production source and receiver handshake bridges

**Files:** new `panel_share_bridge.js`, generated Panel bootstrap integration, share bridge private IPC handlers, `src/plugin-panel-share-bootstrap.test.ts` and existing bridge tests.

**Dependencies:** Task 4; design sections 4–7 and acceptance 6–7,9–10,15.

- [ ] Implement optional source/receive registration, local invalidation, bounded immutable snapshot and confirmation, receiver registration and Prepared ACK.
- [ ] Add synchronous sharing Ready barrier, one-shot offer/claim/handler/ACK and bridge-private pending/consumed/revoked intent; keep ordinary async Ready.
- [ ] Wire exact document/epoch/init/transfer checks at native bridge boundaries; no plugin-callable gesture mint or target selection.
- [ ] Implement cancellation ACK with late response discard and preserve theme-only UI updates without resetting initialization or received text.

**Distinct test coverage:** execute actual generated bootstrap for Ready/offer ordering, Promise/throw contract rejection, source provider timeout/changed snapshot, lost/repeated claims/ACK, deadline admission vs in-flight response, cancellation before delayed body prevents handler/new editor overwrite; native actual Webview mismatch rejected.

**Verify:** `npm.cmd test -- src/plugin-panel-share-bootstrap.test.ts src/plugin-panel-action-bootstrap.test.ts src/plugin-panel-image-bootstrap.test.ts`; `cargo test --lib plugin_share` in `src-tauri`.

### Task 6: Trusted menu authority and host sharing orchestration

**Files:** native Panel accelerator hook, share host commands/service, `commands.rs`, `lib.rs`, `build.rs`, capabilities/ACL, `panel-context-actions.js`, actual generated IPC tests.

**Dependencies:** Task 5; design sections 4,6–8 and acceptance 1,3–11,13,15; host integration map trusted-input section.

- [ ] Mint content source-menu tickets only from native active/focused Ctrl+K hook; actual main caller handles host input/footer gestures. No Panel IPC can mint user intent. Exposed JS ordinary menu cannot start sharing without ticket; separate selector token remains host-only.
- [ ] Wire complete snapshot → discovery → selected receiver → prepare → confirmation → native preparation → commit → Ready/claim/ACK lifecycle, with deadline-driven cancellation/recovery.
- [ ] Expose validated host-only selection/state/cancel/continue/copy interfaces/events. Native source/target input ownership and privacy cleanup apply to every exit path.
- [ ] Register state, commands and ACL together. Record exact frontend wire signatures for Task 7.

**Distinct test coverage:** actual generated Tauri IPC main/child/other/stale callers, synthetic/no-ticket menu denial, native repeat/IME conservative gating, independent selector token consumed once; production scheduling plus bridge ordering through fake adapters, not just isolated models; phase-specific update/hide/revocation races and failed copy/cancel feedback.

**Verify:** `cargo test --lib plugin_share` and relevant generated IPC/ACL tests in `src-tauri`; `npm.cmd test -- src/plugin-panel-share-bootstrap.test.ts src/panel-context-actions.test.ts`.

### Task 7: Launcher share selector and recovery UI

**Files:** `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/main.ts`, `src/protocol.ts`, `src/native-input.ts`, focused share components/styles/tests, `src/panel-context-actions.js` if needed.

**Dependencies:** Task 6 frontend wire interfaces; design sections 6, 8; `docs/ui-guidelines.md`.

- [ ] Add host-owned Share action, searchable receiver selection with limits/reasons and deterministic IDs, preparing state and cancellation.
- [ ] Keep source focus/selection on cancelled selection; disable host input and hide native target through delivery; show target tag/empty input after accepted.
- [ ] Add bounded recovery text, explicit copy/continue/close, and route all exit/new navigation through revocation or document destruction.

**Distinct test coverage:** Ctrl+K from host/content focus; two receivers and no automatic choice; IME/held Enter; Esc/Ctrl+K/Tab trapped; no body in launcher query/history; accepted vs unknown/failed keyboard ownership; continue awaits cancellation ACK and late response cannot reopen; Light/Dark tokens and accessible labels.

**Verify:** `npm.cmd test -- src/plugin-share.test.tsx src/launcher.test.tsx src/panel-context-actions.test.ts`; `npm.cmd run build`.

### Task 8: OCR and translation sharing integration

**Files:** `examples/public-plugins/com.uipilot.{ocr,translate}/package/{plugin.json,dist/*}`, plugin tests and README.

**Dependencies:** Tasks 1–7; design sections 4–5, 6.2–6.3, 9–10; public-plugin skill/guide/UI guidelines.

- [ ] OCR source shares current edited valid full text; content/availability changes increment revision, theme/focus do not.
- [ ] Translation declares receiver and translated-text source; prepare without business, synchronous receipt writes editor and defers one automatic translation; newer input guards stale work.
- [ ] Bump OCR to 1.1.0 and translation to 1.5.0, minimum Host 0.22.0. Keep IDs/configuration and normal entry behavior.

**Distinct test coverage:** OCR edits/clear/loading/failure source invalidation, exact whitespace/byte bounds; translation sharing vs ordinary/selection entries; no provider call before receive, exactly one after receive; preferences ready before Prepared; user edits beat old async translation; missing service config retains received text; translated output can be shared by new user action.

**Verify:** `node --test examples/public-plugins/com.uipilot.ocr/tests/*.test.js examples/public-plugins/com.uipilot.translate/tests/*.test.js`; compile both SDK contract tests; CLI validate both packages.

### Task 9: Integrated acceptance, developer documentation and packages

**Files:** focused integration fixtures/tests, SDK prose/developer guide, plugin examples/skill relevant paragraphs, manual acceptance record, delivery archives under `artifacts/sharing-delivery/`.

**Dependencies:** Tasks 1–8; design section 10 complete matrix.

- [ ] Exercise production host scheduling/bridges with fake OCR, translation and an independent draft-only receiver; demonstrate user-selected delivery and plugin-owned business decision.
- [ ] Document source/receiver authoring, preparation restrictions, cancellation/Ready contracts, versions and honest native acceptance boundary.
- [ ] Build CLI, pack and validate new archives without overwriting old deliveries; verify archive/source parity.
- [ ] Run full relevant suites/build/type checks, review entire feature delta and resolve findings. Preserve a final evidence report including manual native checklist.

**Verify:** `npm.cmd test`; `cargo test --lib` in `src-tauri`; `npm.cmd run verify --workspace @uipilot/plugin-cli`; `npm.cmd run build`; plugin tests/SDK checks and production CLI pack/archive validation. Do not repeat unchanged full suites after purely documentary edits.
