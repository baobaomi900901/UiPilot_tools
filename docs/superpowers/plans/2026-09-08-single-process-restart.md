# Stage 2C Single-Process Restart Implementation Plan

> **For agentic workers:** Use subagent-driven-development or executing-plans. Execute dependent tasks serially. Checkboxes record implementation progress.

**Date:** 2026-09-08
**Status:** APPROVED - accepted by the user on 2026-09-08. Tasks 1-2 complete. Task 3 implemented at 9a15810 with passing tests and independent product/scope review; user decision on missing pre-implementation RED evidence is pending. Tasks 4-6 not started.
**Approved design:** [Single-Process Restart](../specs/2026-09-08-single-process-restart-design.md), accepted 2026-09-08 at `d209c2a`.
**Goal:** Deliver managed launch receipts and explicit single-process restart without stopping any process whose original launch context cannot be established.
**Architecture:** Keep receipt ownership, planning and restart composition in a new private restart module. Its Windows adapter owns path leases and CreateProcessW resources; existing identity and single-target stop implementations retain their contracts.
**Tech Stack:** Rust 1.96.1, existing windows 0.61.3 features, Windows 11 x64, standard-user execution, current locked dependencies.

## Global Constraints

- This is one managed-launch/restart workflow. Tasks are sequential implementation gates, not independent platform workstreams.
- Follow design sections 1-8 as the source of truth for security, state, ordering, failure and resource contracts. This plan defines file ownership and task handoffs, not an alternative specification.
- A missing or mismatched launch receipt rejects before stopping. Existing arbitrary processes, ShellExecute launches and unrecorded descendants do not gain restart support.
- Receipts/plans are host-instance-bound, non-Clone/non-Copy/non-serializable and immutable to callers. No PID registry or persistence is added.
- Initial executable, raw UTF-16 command line, initial directory and explicit environment are captured by the managed creation path; no reconstruction of unknown startup data or restart-time inheritance.
- Paths must satisfy the approved local fixed-volume, volume-GUID and no-reparse policy. Lease every relevant component through CreateProcessW; final-file-only protection is insufficient.
- Windows feasibility is a hard gate before destructive restart implementation. Failure means a design blocker, not permission to introduce Shell, undocumented queries or a weaker path policy.
- Restart admission is distinct from the fresh Terminate/ForceTerminate admission consumed by Stage 2A. Stage 2A must continue rejecting an unchanged Restart admission.
- Only a successful observed-exit stop permits one launch. Preserve failed/uncertain stop results, irreversible launch failure and successful-creation/unknown-identity distinctions.
- Receipt attempt ownership prevents concurrent launches. Consume before possible creation; a consumed receipt never becomes available on error or unwind. A replacement receipt belongs only to the new exact target.
- No batch restart, elevation, services/UWP, UI/Tauri/public plugins, existing launcher migration, background recovery, automatic force/retry/rollback, inherited arbitrary handles or next-stage work.
- Cargo.toml/Cargo.lock, core.rs, operation.rs, windows.rs, batch.rs and existing tests remain behaviorally unchanged. A contradiction requiring changes outside the named files is reported before expansion.
- Real effects act only on current test-owned fixtures; guards and bounded cleanup exist before fallible ownership transfer. Parent/child handshake budgets must not include unrelated identity-correlation work.
- One concise independent review per task checks correctness and scope. Fix actual blockers and recheck only those corrections. Manual-only tests are identified explicitly; no speculative review loops.

## File Ownership And Handoffs

Create these focused files without moving accepted Stage 1/2A/2B code:

- `src-tauri/src/process_control/restart.rs`: validated launch inputs, receipt/attempt ownership, restart plans/reports, generic composition, and private Windows module declaration.
- `src-tauri/src/process_control/restart/windows.rs`: path-component leases, supported-volume checks, process creation and RAII ownership of returned native resources. Implement the new private restart adapter for the existing WindowsProcessAdapter; do not change its accepted stop implementation.
- `src-tauri/src/process_control/tests/restart.rs`: new Fake restart adapter composed with the existing FakeProcessAdapter; receipt, planning, state and outcome tests. Delegate existing platform/operation methods rather than duplicating their logic.
- `src-tauri/src/process_control/tests/restart/windows.rs`: non-destructive feasibility proof, owned launch/restart fixtures and resource tests.
- `docs/superpowers/verification/2026-09-08-restart-path-feasibility.md`: the Task 1 feasibility result and supported-path evidence.
- `docs/superpowers/verification/2026-09-08-single-process-restart.md`: final aggregate evidence and user acceptance gate.

Modify `process_control/mod.rs` only for module/type declarations and Windows/non-Windows facade forwarding; modify `process_control/tests.rs` only to declare the restart test module. Update this plan only for factual status/checks. Existing application launch callers are not rewired.

Shared names used by later tasks:

- `ManagedLaunchInput`: trusted internal launch request; absolute executable/directory, exact command-line code units and explicit environment entries. Validation belongs to Task 3, not to callers.
- `ProcessLaunchReceipt`: privately minted launch record plus shared attempt state; produced only after managed creation and exact new identity observation.
- `ProcessRestartMode`: `Graceful` or `Force`; no default force escalation.
- `ProcessRestartPlan` / `ProcessRestartPreview`: frozen receipt-derived request and read-only confirmation values.
- `ProcessRestartReport`: preserves stop and launch phases and the four terminal states named in design section 6; successful identified creation supplies the replacement receipt.
- `ManagedLaunchOutcome`: distinguishes confirmed creation with receipt from creation with unknown identity; pre-creation rejection/failure cannot conceal a successfully created process.
- Private `ProcessRestartAdapter`: extends the existing operation adapter with validated path-lease acquisition, creation and returned-process identity observation. Native resources stay inside associated RAII values.
- `CoreAuthority::launch_managed`, `plan_restart`, `execute_restart_with_clock`; Windows facade `ProcessAuthority::launch_managed`, `plan_restart`, `execute_restart`. Non-Windows counterparts reject before effects. `plan_restart` takes the exact selected target, optional borrowed receipt and explicit mode so no-receipt/mismatch rejection is testable without minting fake caller provenance.

All constructors/state transitions that mint or modify receipt authority are private. Stable error names follow design section 6 categories; no raw native values or sensitive launch inputs enter Display/logs. No string-valued status replaces typed phase outcomes.

## Common Execution And Verification

- Task order: 1 -> 2 -> 3 -> 4 -> 5 -> 6. Task 1 must have passing feasibility evidence and independent review before Task 2; each later task requires its predecessor's review to pass.
- Record implementation-start HEAD and a serialized complete process_control baseline. Accepted Stage 2B evidence at `ec15356` is 152 passed, 0 failed, 2 ignored; default library 1126 passed, 0 failed, 4 ignored plus one integration test. Do not silently absorb unrelated drift.
- For new behavior, use focused tests and intended-failure evidence before implementation. For feasibility, report the actual experiment result; do not manufacture a failing test to imitate TDD.
- Use `cargo ... --manifest-path src-tauri/Cargo.toml --locked --offline`. Run real fixtures serially during focused tests. Default full regression must also succeed under normal test concurrency; do not hide a default-suite failure by reporting only serialized results.
- Use one task implementation commit and separate blocker-fix commits. Keep scratch reports ignored; only named code/docs are committed. Preserve unrelated changes.
- Capture exact command, revision, exit status, counts and duration. Run the focused suite while iterating, affected regression after changes, and the final aggregate matrix once at Task 6. Report failures accurately before deciding scoped corrections.

### Task 1: Non-Destructive Windows Path Feasibility

**Files:** Create `tests/restart.rs`, `tests/restart/windows.rs` under process_control; modify `tests.rs` to declare the module; create the path-feasibility verification record above.
**Dependencies:** Design section 5 path-policy and feasibility gate, sections 7-8. No production restart module exists yet.

- [x] Build an owned local executable/directory fixture and a bounded child handshake for arguments, effective directory and environment markers. The original ignored libtest entry was replaced by the user-authorized minimal owned helper after documented handshake failures.
- [x] Demonstrate CreateProcessW using the proposed volume-qualified paths while all executable/directory components and the final file are leased. Use explicit command line/environment and no inherited handles; keep an independent guarded cleanup process handle until the fixture exits.
- [x] Attempt file and ancestor-directory replacement/rename and incompatible opens while leased; prove expected rejection without changing non-owned paths. Demonstrate fail-closed rejection of reparse/unsupported paths and sharing denial before any creation or target stop.
- [x] Record the effective path form, flags, share/access combinations, filesystem, standard-user result and cleanup. Unsupported setup is a blocked gate, not a passing N/A. Prove leases are released on failure/unwind and the temporary tree can be removed afterward.
- [x] Stop here if the property is not demonstrated. No production managed-launch, stop or restart implementation can proceed on an assumed path guarantee.

**Distinct test coverage:** Actual path-to-image/directory behavior; ancestor rename/replacement; reparse rejection; incompatible sharing; Unicode/space paths; lease/returned-handle cleanup. Do not change existing applications or user files.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::windows::restart_path_feasibility -- --nocapture --test-threads=1`

### Task 2: Production Path Leases And Launch Adapter

**Files:** Create `restart.rs`, `restart/windows.rs`; modify `mod.rs`, `tests/restart.rs`, `tests/restart/windows.rs` under process_control.
**Dependencies:** Task 1 PASS; design sections 5 and 7.

- [x] Introduce the private ProcessRestartAdapter boundary and RAII lease/created-process resource types, implementing only launch-side platform primitives at this step.
- [x] Implement the verified fixed-volume/no-reparse component-walk, final identity checks and denied write/delete sharing policy. Keep leases through the creation call and make every intermediate failure release the full acquired set.
- [x] Implement explicit CreateProcessW invocation, mutable command-line buffer, Unicode environment, noninheritance, returned process/thread guards and identity observation through the returned handle. Never reopen a returned process by bare PID to establish receipt identity.
- [x] Replace feasibility-only mechanics with calls to these production primitives, retaining the Task 1 experiments as regressions. Do not keep a second implementation of the path policy in tests.

**Distinct test coverage:** Partial component acquisition failure, mismatch after acquisition, unsupported volume/path, native creation failure, process/thread identity-observation failure, and exact cleanup. Re-run Task 1 real proof against the production implementation before Task 3.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart -- --nocapture --test-threads=1`

### Task 3: Managed Launch Receipts And Restart Plans

**Files:** Modify `restart.rs`, `mod.rs`, `tests/restart.rs`, `tests/restart/windows.rs` under process_control.
**Dependencies:** Task 2; design sections 2-4, 6-7.

- [x] Validate complete ManagedLaunchInput, distinguish known empty arguments from unavailable context, validate UTF-16 string/environment structure and documented platform limits, and freeze the effective values used for initial creation.
- [x] Implement launch_managed with explicit creation/identity outcomes. Mint ProcessLaunchReceipt only from the returned exact identity and successful launch record; after creation succeeds, identity failure must return a created/unknown outcome without a retry or automatic kill.
- [x] Implement plan_restart with exact target/receipt/HostInstanceId checks, receipt availability and Restart-policy admission. Reject absent/mismatched/expired/consumed context without stopping. Freeze mode and confirmation values in a handle-free plan.
- [x] Implement shared Ready/Executing/Consumed attempt ownership and a private RAII attempt guard. Planning/canceling alone must not claim or consume the receipt; two plans borrow the same ownership state.
- [x] Add narrow facade aliases/forwarding and non-Windows rejection. Lock receipt/plan constructor, clone, serialization and mutation boundaries with proportional contract tests.

**Distinct test coverage:** Incomplete request; empty vs missing arguments; embedded NUL/invalid environment; frozen explicit environment rather than ambient reuse; receipt cannot attach arbitrary PID context; missing external-process receipt; target/host mismatch; plan cancellation and replanning; creation succeeded but identity unknown; same receipt shared across plans without publicly cloning it.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::restart_receipt -- --nocapture`

**Execution record (2026-09-09):** At `9a15810`, focused tests passed 8/0 and
serialized process_control passed 169/0/2 ignored. Strict Clippy retained the
inherited 12/13 external diagnostics. The independent reviewer found no product
correctness or scope defect, but the required pre-implementation behavioral RED
evidence was not captured. The user must decide whether to accept this documented
process deviation before Task 4 begins; no retrospective RED is claimed. The
implementation also required the narrow native environment comparator and legacy
whole-facade expected-catalog synchronization, preserving existing behavior.

### Task 4: Single-Process Restart Composition

**Files:** Modify `restart.rs`, `mod.rs`, `tests/restart.rs` under process_control.
**Dependencies:** Task 3; design sections 5-7.

- [ ] Implement execute_restart_with_clock consuming the frozen plan and acquiring its attempt state before effects. Revalidate provenance, exact target and full path lease; all preflight failure paths release the attempt and temporary resources.
- [ ] Acquire Restart-policy admission, then fresh matching Terminate/ForceTerminate admission for the same target. Release the policy handle and call the accepted Stage 2A method with the supplied clock and its full five-second budget. Do not change Stage 2A IntentMismatch behavior.
- [ ] Return the exact stop failure report without launching for every non-Ok stop result, including TargetExited and uncertain completion. After observed exit, mark the receipt Consumed before a single possible creation call.
- [ ] Return distinct created, launch-failed and identity-unknown phase outcomes; identified creation returns a new receipt bound to the new exact target. Never retry/rollback or revive the old receipt after possible launch.
- [ ] Wire Windows execute_restart using SystemMonotonicClock and non-Windows preflight rejection. Preserve stable redacted error/result presentation.

**Distinct test coverage:** Graceful/Force routing; old target changes after planning; every stop error prevents launch; stop success followed by native launch failure/identity failure; replacement receipt identity; simultaneous attempts return RestartInProgress with zero secondary calls; unexecuted plan cancellation; failure before consumption allows explicit replanning; failure/unwind after consumption forbids another launch; exact handle/lease/attempt cleanup on each phase boundary.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::restart_execution -- --nocapture`

### Task 5: Owned Windows Restart Integration

**Files:** Modify only `tests/restart/windows.rs` under process_control.
**Dependencies:** Tasks 1-4; design sections 5, 7-8.

- [ ] Extend the owned fixture entry for graceful close, ignore-close and force-stop scenarios; retain independent cleanup guards for both old and created instances before fallible handoff/registration.
- [ ] Launch through launch_managed, capture the receipt, plan/execute both modes, and prove old exact-process exit plus new exact identity and preserved command line, effective working directory and environment markers.
- [ ] Prove ignored graceful close launches nothing; changed executable/path/context rejects before stop; deliberate launch failure after a successful stop reports the irreversible partial outcome without retry.
- [ ] Validate unknown-identity handling with a controlled failure seam and prove returned native handles close while the test's separate cleanup authority remains effective. Do not manufacture receipts for arbitrary existing processes.
- [ ] Test subsequent restart with the replacement receipt and rejection of consumed old plans; verify no fixture process, cleanup HANDLE, temporary copied executable or control directory remains after normal and failure/unwind paths.

**Distinct test coverage:** Real argument quoting/Unicode/empty arguments; effective CWD/environment roundtrip; new PID/creationTime; both stop modes; timeout/no relaunch; changed-path preflight; post-stop failure; receipt replacement; parallel default-suite fixture setup without budget races. PPL/admin-only behavior remains unsupported, not a reason to act on system processes.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart::windows::restart_windows -- --nocapture --test-threads=1`

### Task 6: Aggregate Evidence And Acceptance Gate

**Files:** Create `docs/superpowers/verification/2026-09-08-single-process-restart.md`; update this plan's factual status/checks only.
**Dependencies:** Tasks 1-5; design section 8 and accepted Stage 2B verification baseline.

- [ ] Collect focused receipt/execution/path/Windows results, complete process_control and default full regression, standard-user environment, owned-fixture cleanup and exact tested revision.
- [ ] Run rustfmt, all-features tests and both Clippy variants; keep inherited schema/12-library/13-library-test deviations explicitly red and prove zero new process_control diagnostics. Newly discovered restart regressions must be corrected within their owning task and reverified before this gate closes.
- [ ] Verify unchanged Cargo, accepted stop/batch modules, frontend/Tauri/plugin contracts and existing application launch callers. Record only the authorized restart module/tests and evidence changes.
- [ ] Disclose managed-only coverage, original-initial-context versus dynamic runtime state, path-policy limits, spawn-not-readiness, irreversible post-stop failures, no automatic recovery, PPL/non-Windows test limitations and any residual review Minor.
- [ ] Obtain one concise independent final correctness/scope review and set the record to AWAITING USER ACCEPTANCE. Do not mark the stage accepted or start elevation/host-entry work before explicit user acceptance.

**Verify:** Run the following final commands with exact results/revisions recorded; reuse a same-revision run where it already supplies the required evidence rather than repeating costly real tests without a reason.

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
git diff --exit-code d209c2a...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/process_control/core.rs src-tauri/src/process_control/operation.rs src-tauri/src/process_control/windows.rs src-tauri/src/process_control/batch.rs src/ docs/plugin-sdk/
git diff --name-status d209c2a...HEAD
```

Expected: scoped/default tests and format pass; no new module diagnostics; inherited aggregate failures match the accepted baseline and remain labeled FAIL; cleanup and changed-scope evidence are explicit. Windows feasibility remains an earlier mandatory gate, not a deferred final assumption.

## Approval And Next Step

- [x] Independent plan review confirms design coverage and bounded task scope (2026-09-08; no material findings).
- [x] User approves this implementation plan before Task 1 experiments or code begin (2026-09-08).
- [ ] User accepts final Stage 2C evidence before any later substage begins.

The next authorized action after plan approval is Task 1 only. Failure of the
path-policy experiment returns to the design decision; it never authorizes
destructive restart with a weaker guarantee.
