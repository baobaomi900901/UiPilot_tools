> SUPERSEDED 2026-11-06: user confirmed target-side errors with no return to OCR. Do not execute this lifecycle subplan; use 2026-11-06-plugin-sharing.md.

# Plugin sharing: Panel lifecycle subplan

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

This subplan expands Task 4 of the main sharing plan. Internal order: 4A → review → 4B → review → 4C → review → 4D → combined Task 4 review. Main Tasks 5–9 retain their existing numbers. Main Tasks 1–3 have passed their review gates. These increments are internal and do not independently deliver usable sharing.


### Task 1: Guarded manager authority (4A)

**Files:** existing `src-tauri/src/public_plugins/activation_bundle.rs`, `public_plugins/activation.rs`; new `public_plugins/activation/share_authority.rs` and `share_authority_tests.rs`. If the registry type must be accessible from the sibling book, declare/re-export it through `activation.rs`; do not duplicate registry ownership. `public_plugins.rs` only where existing shutdown/runtime invalidation delivery requires a body-free after-lock notification bridge.

**Dependency:** reviewed Task 3 through `7d3090e`; Task 2 exact catalog identity. No Panel gate dependency.

- [ ] **Production wiring:** real reserve/publish/install/terminal/Drop; secret writes; runtime/network timeout and generation invalidation; close/shutdown/poison paths. Manager-confirmed issuance binds installation/receiver/command plus an independent authority revision. Non-authority theme/layout remains unchanged. Manager methods issue body-free revocation notifications only after their guards are released.

- [ ] **Internal handoff:** exact immutable lease identity, registry read guard validating source and target together, explicit irrevocable revocation history/classification and drained notification identity. These interfaces are initially internal; Panel/coordinator consumption arrives in 4D. Registry write exclusion is already exercised by production mutation paths and integration tests in 4A.

**Distinct test coverage / Verify:** actual manager permission/settings reservation paused before durability rejects old lease; rollback permits only fresh issuance; read guard holds reservation/secret/global-terminal writers until released; reverse ordering rejects old authority. Cover every non-reservation revoker, including terminal Drop affecting another plugin. Suggested filters: new `share_authority`, existing activation reservation/management and `share_prepare` regressions.

- [ ] **Required safety boundary:** A→B order on every book path; no A/B→G/C callback; no independent terminal publication that bypasses A; no old-lease revival; no A held through persistence/provider/effects. Notification subscription can be internal until 4D, but revocation publication cannot be deferred.

**Verify:** from `src-tauri`, `cargo test --lib share_authority`, `cargo test --lib activation_bundle`, and `cargo test --lib share_prepare`.

### Task 2: Ordinary Panel admission and source revocation (4B)

**Files:** existing `src-tauri/src/plugin_panel.rs`, `public_plugins.rs`, `public_plugins/scheduler.rs`, `public_plugins/activation.rs`, `public_plugins/network/authority_gate.rs`, `commands.rs`; new `plugin_panel/admission.rs` and `admission_tests.rs`. Touch `public_plugins/runtime.rs` or service child modules only if current-request checks demonstrably bypass the new scheduler-visible token; the review must audit their production consumers. Keep the token type in a dependency-neutral module accessible to controller and scheduler.

**Dependency:** 4A reviewed registry contract; existing service registration and Task 3 physical occupancy semantics.

- [ ] **Production wiring:** replace controller-held manager/service callbacks in action admission/completion and ordinary submission dispatch with manager-first authorization plus exact final session reservation publication. Detach retired images/registrations and execute teardown after core unlock. Wire session revocation into PanelAction and ordinary Panel submission owner/dispatch/current/completion, network admission/response and early service/credential checks. Existing ordinary behavior remains exercised; no pending session yet.

- [ ] **Internal handoff:** `RetiredSessionEffects` ownership and explicit after-unlock execution; session authority token and revoke operation; exact admission reservation/final publication helper; manager-first completion helper and wait/reacquire protocol. Report final Rust signatures, including which guard each requires. 4D uses synchronous token revocation at the swap; cleanup stays deferred.

**Distinct test coverage / Verify:** deterministic manager-first text/clipboard versus action/command/completion ordering; registration destructor reenters manager only after core unlock; admission paused before publication then session revoked cannot dispatch; already-running source rejects new storage/network/service/completion admissions before deferred cleanup. Existing Panel/action/submission tests plus new `panel_admission` tests and Task 3 regressions.

- [ ] **Required safety boundary:** remove all affected C→M and C→registration→M edges before 4C adds G; no condvar wait retaining M/A/G/registration; no registration destructor under C; no token-only precheck followed by unchecked final dispatch/publication. Source token revocation must be live in actual authority paths now, not a 4D TODO.

**Verify:** from `src-tauri`, `cargo test --lib panel_admission`, `cargo test --lib plugin_panel`, `cargo test --lib panel_action`, and `cargo test --lib share_prepare`.

### Task 3: Pending lifecycle and native presentation (4C)

**Files:** existing `src-tauri/src/plugin_panel.rs`, `plugin_panel/image_session.rs`, `commands.rs`; new `plugin_panel/pending.rs`, `presentation.rs`, `pending_tests.rs`, `presentation_tests.rs`. Reuse the existing Tauri mount/focus/teardown functions through the new lane. No new bridge JS, public API/ACL, trusted Ctrl+K or launcher selector UI.

**Dependency:** 4A/4B reviewed. Consume Task 3 registration and pure cancellation qualification; confirmed manager checks stay outside G/C/A.

- [ ] **Production wiring:** introduce the shared transition-gate owner and controller input/presentation revision; route existing ordinary bounds/show/hide/focus/destruction and modal-admission-capable internal API through one non-reentrant native executor. Existing controller/command input routes enforce pending/admitted ownership. Add a distinct hidden pending session, native mount, initialization/update ACK/receiver-registration state and read-only private IPC purpose classes, including helper bypasses. Preserve existing initial-navigation guard and ordinary mount compatibility. Native operations and compensation run outside state locks.

- [ ] **Internal handoff:** pending allocation/mount/cancel; exact document + initial update identity; preparation registration ownership/application; receiver/update qualification; native preparation receipt; requested versus admitted modal/input owner; qualified presentation effect/result and quarantine APIs. These host-only APIs are callable by integration tests and 4D; Task 5/6 later supplies the generated bridge/orchestration callers. No unreachable duplicate lifecycle model: pending uses the same controller and native adapter as ordinary production presentation.

**Distinct test coverage / Verify:** retained source object/image/model/selection adapter identity on preparation failure; pending own reads allowed and every effect helper denied; bounds cannot re-show hidden source/pending; modal admission strictly follows acknowledged hide; reentrant callback queues but cannot recursively drain; stale bounds/focus/show/close compensation targets only captured old child; uncertain destruction retains quarantine. Focused `plugin_panel` plus new native-adapter tests; no native acceptance claim.

- [ ] **Required safety boundary:** G→C→A ordering from its introduction; no ordinary show/focus bypass left for 4D; no native operation or registration Drop under G/C/A; no modal authoritative before hide; no async waiter retaining locks. Committed-hidden state may be represented internally, but no incomplete commit path is exposed.

**Verify:** from `src-tauri`, `cargo test --lib plugin_panel` and `cargo test --lib share_prepare`.

### Task 4: Atomic handoff and reasoned termination (4D)

**Files:** existing `src-tauri/src/plugin_share.rs`, `plugin_share/coordinator.rs`, `preparation.rs`, `delivery.rs`, `lifecycle.rs`; new `plugin_panel/handoff.rs`, `handoff_tests.rs`; 4C child modules and necessary `plugin_panel.rs`, `public_plugins.rs`, `commands.rs` lifecycle callers. `lib.rs` only if shared host state installation cannot be owned by the existing controller. No Task 5 bridge implementation or Task 6 command/ACL orchestration.

**Dependency:** all three earlier internal gates passed; existing Task 2 API/effect qualification and Task 3 corrected cancellation interfaces.

- [ ] **Production wiring:** shared G serializes controller/coordinator lifecycle; bind manager revocation notifications; validate leases under A with G/C held; apply one infallible Panel/coordinator swap and immediate source token revocation. Drain exact old-source teardown afterward. Wire hide/navigation/plugin/generation/session/current teardown, process/native loss and timers for active and pending identities. Sticky intentional revocation outranks queued crash loss. Implement Accepted/revoke-ACK exposure and deadline destruction/quarantine through the real 4C lane.

- [ ] **Internal handoff to Tasks 5/6:** concrete initialization/receiver ACK, source confirmation, native receipt, commit, current target qualification, claim/ACK/revoke ACK, reasoned loss/end, effects and timer deadline interfaces. Report exact final signatures and native caller expectations. Task 5 supplies generated JS; Task 6 wires public native command registrations/ACL/orchestration. Neither later task may be assigned lock safety, current pending/active lifecycle correctness or presentation barriers left incomplete here.

**Distinct test coverage / Verify:** all 12 deterministic cases in commit-gate-implementation-design.md, using real manager/controller/coordinator boundaries and mocked native adapter. Include before/after-swap invalidation, source cleanup not cancelling target, every final validation failure leaving both models unchanged, privacy release winning loss races, missing revoke ACK destroying before input, uncertain destroy quarantine, Accepted display failure never rolling back. Final exact `plugin_share`, `plugin_panel`, affected authorization/lifecycle and preparation regressions. Parent then reviews combined 4A–4D against every original Task 4 checkbox.

- [ ] **Required safety boundary:** no fallible coordinator commit after moving Panel source; no manager callback/registration drop/native call/new lock inside swap; no resurrection after accepted commit; no logical input release before exact Accepted/revoke ACK/destruction confirmation; no ignored manager revocation because UI notification is delayed. All source/target revision checks and authority exclusions must be real, not AtomicBool-only admission.


**Verify:** from `src-tauri`, `cargo test --lib plugin_share`, `cargo test --lib plugin_panel`, `cargo test --lib share_authority`, and `cargo test --lib share_prepare`; record any additional affected lifecycle filters.
