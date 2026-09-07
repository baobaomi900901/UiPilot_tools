# Stage 2B Process Target Expansion And Batch Orchestration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Checkboxes are the execution record.

**Date:** 2026-09-07
**Status:** Implementation in progress — Task 1 implemented and independently reviewed through 02ec01a (no findings); Tasks 2-6 not started.

**Goal:** Deliver the host-internal Windows process-tree and same-name planning/execution Module that freezes at most 256 exact targets before confirmation, then serially reuses Stage 1 admission and Stage 2A operations with a 30-second scheduling deadline and complete per-target reporting.

**Architecture:** Add a focused batch.rs behind the existing crate-private ProcessAuthority facade. core.rs supplies one internal expansion snapshot without weakening the accepted ProcessSnapshot contract; batch.rs owns root preflight, frozen planning, tree/name selection, ordering, limits, execution, and reports while operation.rs and windows.rs retain their accepted single-target and Win32 responsibilities.

**Tech Stack:** Rust 1.96.1, edition 2021, windows 0.61.3, Windows 11 x64, the existing Cargo dependency graph.

**Approved design:** [Stage 2B Process Target Expansion And Batch Orchestration Design](../specs/2026-09-04-process-target-expansion-batch-orchestration-design.md)

## Global Constraints

- Implementation scope is measured from design-approval commit 6d44729; preserve unrelated user changes.
- Implement only Stage 2B. Do not add Restart, Force Restart, elevation, Tauri state/commands, Launcher/Panel/UI, public plugin contracts, persistence, background tasks, cancellation, macOS, Shell, taskkill, PowerShell, WMI, Job Object, or undocumented NT APIs.
- plan_batch is read-only and must freeze the only authorized target set before host confirmation; execute_batch consumes that exact plan and never re-enumerates by name or parent PID.
- ProcessBatchPlan is Authority-bound, handle-free, non-cloneable, non-serializable, non-persistent, and single-use. It has no wall-clock TTL.
- Preview exposes only a plan-local ordinal, display PID, frozen name, optional path, tree depth, operation/scope, counts, and completeness. Exact ProcessTarget values remain private inside the plan.
- Every target independently reuses Stage 1 admission and the matching Stage 2A method. Do not copy final admission, graceful-close, force-termination, access-mask, or completion logic into batch.rs.
- Tree planning includes the root, accepts only reliable full ancestry chains, orders deepest descendants first, and orders equal depth by PID then creationTime.
- Same-name planning includes the root, uses the existing Adapter name-equivalence seam, intentionally ignores executable path, and orders by PID then creationTime.
- The reliable target limit is exactly 256. A 257th reliable target returns TargetSetTooLarge with no plan, truncation, or side effect.
- Execution is serial best-effort. A 30-second monotonic scheduling deadline is checked before Stage 1 admission and after successful admission; an entered Stage 2A operation retains its accepted five-second deadline.
- After execution preflight, all expected target/platform outcomes return in ProcessBatchReport. Top-level ProcessBatchError must never hide possible partial side effects.
- No retry, rollback, automatic force fallback, automatic elevation, or cross-batch deduplication.
- Ordinary logs and stable Display values never include PID, creationTime, name, path, parent PID, tree edges, HANDLE, HWND, command line, or raw Win32 code.
- Real side effects are restricted to unique, exact fixture processes created by the current test. Every fixture node must have a retained bounded cleanup capability; abort before side effects if a name match is not owned by the test.
- Cargo.toml and Cargo.lock remain byte-identical. Existing operation.rs and windows.rs behavior is read-only for this stage unless implementation exposes a direct contradiction with the approved design; report such a contradiction instead of widening scope.

## File Structure And Core Contract

- Create src-tauri/src/process_control/batch.rs — batch values, planning preparation, tree/name selection, ordering, deadlines, execution, normalization, and report aggregation.
- Create src-tauri/src/process_control/tests/batch.rs — Stage 2B Fake and Windows tests as a child of the existing tests module, reusing ancestor Fake support without moving accepted Stage 1/2A tests.
- Modify src-tauri/src/process_control/core.rs — internal ExpansionSnapshot/entry/omission representation and the narrow accessors batch.rs needs; preserve ProcessSnapshot output and admission behavior.
- Modify src-tauri/src/process_control/tests.rs — declare the batch child test module; the child accesses existing private ancestor test support without changing its visibility or moving accepted tests.
- Modify src-tauri/src/process_control/mod.rs — declare batch, expose crate-private type aliases, and add Windows/non-Windows plan_batch and execute_batch facade methods.
- Create docs/superpowers/verification/2026-09-07-process-target-expansion-batch-orchestration.md — exact automated/manual evidence, inherited deviations, scope, and acceptance stop gate.
- Update this plan only for execution checkboxes and factual status.

Use these shared names throughout:

- Public-to-crate values: ProcessBatchScope, ProcessBatchOperation, ProcessBatchPlan, ProcessBatchPreview, ProcessBatchOmissions, ProcessBatchSelectionCompleteness, ProcessBatchTargetResult, ProcessBatchReport, ProcessBatchCompletion, ProcessBatchTargetOutcome, ProcessBatchTargetError, and ProcessBatchError.
- ProcessBatchScope variants: Tree and SameName.
- ProcessBatchOperation variants: Terminate and ForceTerminate.
- ProcessBatchTargetOutcome variants: ObservedExited, AlreadyExited, Failed(ProcessBatchTargetError), and NotAttemptedBatchDeadline.
- Selection completeness: CompleteSelection and IncompleteSelection.
- Report completion: Complete and Partial.
- Constants: BATCH_TARGET_LIMIT = 256 and BATCH_SCHEDULING_DEADLINE = Duration::from_secs(30).
- Internal core values: ExpansionSnapshot, ExpansionEntry, and ExpansionOmissionReason.
- Internal batch preparation value: PreparedBatchPlanning.
- Generic CoreAuthority methods: prepare_batch_planning, plan_batch, and execute_batch_with_clock.
- Production facade methods: ProcessAuthority::plan_batch and ProcessAuthority::execute_batch. execute_batch supplies SystemMonotonicClock.

Freeze these read-only accessors:

~~~rust
impl ProcessBatchPlan {
    pub(crate) fn scope(&self) -> ProcessBatchScope;
    pub(crate) fn operation(&self) -> ProcessBatchOperation;
    pub(crate) fn previews(&self) -> &[ProcessBatchPreview];
    pub(crate) fn omissions(&self) -> &ProcessBatchOmissions;
    pub(crate) fn selection_completeness(&self) -> ProcessBatchSelectionCompleteness;
}

impl ProcessBatchPreview {
    pub(crate) fn ordinal(&self) -> usize;
    pub(crate) fn pid(&self) -> u32;
    pub(crate) fn name(&self) -> &str;
    pub(crate) fn executable_path(&self) -> Option<&Path>;
    pub(crate) fn depth(&self) -> Option<usize>;
}

impl ProcessBatchTargetResult {
    pub(crate) fn preview(&self) -> &ProcessBatchPreview;
    pub(crate) fn outcome(&self) -> ProcessBatchTargetOutcome;
}

impl ProcessBatchReport {
    pub(crate) fn scope(&self) -> ProcessBatchScope;
    pub(crate) fn operation(&self) -> ProcessBatchOperation;
    pub(crate) fn results(&self) -> &[ProcessBatchTargetResult];
    pub(crate) fn omissions(&self) -> &ProcessBatchOmissions;
    pub(crate) fn observed_exited(&self) -> u64;
    pub(crate) fn already_exited(&self) -> u64;
    pub(crate) fn failed(&self) -> u64;
    pub(crate) fn not_attempted(&self) -> u64;
    pub(crate) fn completion(&self) -> ProcessBatchCompletion;
}
~~~

ProcessBatchOmissions exposes checked u64 getters for exited_or_changed, inspection_denied, identity_unavailable, ancestry_unverified, and total. No accessor returns a ProcessTarget, raw candidate, mutable collection, or execution primitive.

The approved design is canonical for field visibility, root error priority, ancestry validity, omission attribution, confirmation handoff, target result mapping, HANDLE lifetime, security restrictions, and exit criteria.

## Global Execution Rules

- Dependency order is Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6. All tasks edit a shared private Module or its evidence and must run serially.
- Before Task 1, record HEAD and run the serialized complete process_control baseline. At 6d44729 the accepted baseline is 100 passed, 0 failed, 1 ignored; stop and report unrelated drift rather than changing it.
- Every task follows focused TDD: add task-specific failing tests, confirm the intended failure, implement the minimum approved contract, rerun the focused tests, and commit an atomic change.
- Each task receives specification-compliance and code-quality review before its dependent task begins when the chosen execution workflow provides those gates.
- A task commit must not contain a later task or pre-existing user change. Review fixes use separate commits.
- Run Rust commands from the repository root with --manifest-path src-tauri/Cargo.toml --locked --offline.
- Run real Windows fixture suites with --test-threads=1. Product code must never create, discover for cleanup, or kill test helpers.

---

### Task 1: Expansion Catalog And Planning Contracts

**Files:** Create src-tauri/src/process_control/batch.rs and src-tauri/src/process_control/tests/batch.rs; modify src-tauri/src/process_control/core.rs, src-tauri/src/process_control/mod.rs, and src-tauri/src/process_control/tests.rs.

**Dependencies:** Approved design sections 4. Core Invariants, 5.3 Internal Expansion Snapshot, 6. Crate-Private Interface, 7.1 Root Preflight, 7.2 Root Correlation And Error Priority, 7.3 Candidate Normalization, 14. Top-Level Error Contract, 15.1 Planning, and 18. Logging And Data Handling.

- [x] Declare batch and its child test module without changing visibility or moving/reworking existing Stage 1/2A tests.
- [x] Define ProcessBatchScope, ProcessBatchOperation, ProcessBatchOmissions, ProcessBatchSelectionCompleteness, ProcessBatchPreview, ProcessBatchPlan, and ProcessBatchError with the exact variants/accessors above and stable Display codes from design section 14; all constructors remain private to batch.rs.
- [x] Add ExpansionSnapshot, ExpansionEntry, and ExpansionOmissionReason behind the core-to-batch internal seam. Capture raw PID/name/parent hints only for private scope attribution, attach a reliable ProcessRecord only after the existing full observation flow, and preserve the ordinary ProcessSnapshot records, ordering, omissions, errors, and HANDLE behavior exactly.
- [x] Implement candidate normalization for zero/duplicate PID, invalid names, per-candidate observation failure, full-enumeration failure, checked omission counts, and exact temporary-HANDLE cleanup.
- [x] Implement prepare_batch_planning: validate root and operation, retain one temporary Stage 1 admission while taking one expansion snapshot, correlate the exact root with the approved priority, then close the admission and return a handle-free PreparedBatchPlanning.
- [x] Add source/trait checks that the plan and preview cannot clone/serialize exact targets, raw hints never leave the Module, and no logging or platform side-effect primitive is introduced.

**Distinct test coverage:** Root invalid/expired/exited/changed/inspection/protected priority; catalog creation and traversal failure; same-PID replacement versus absent root re-poll; root admission remains open through snapshot and closes before prepared context return; duplicate PID makes all duplicates unreliable without changing ordinary ProcessSnapshot behavior; invalid/failed candidates retain private omission reasons; normal, error, and catch_unwind paths close root/snapshot/candidate handles once; all error Display text is the stable code and excludes scripted sensitive values.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_catalog -- --nocapture

### Task 2: Frozen Process-Tree Planning

**Files:** Modify src-tauri/src/process_control/batch.rs and src-tauri/src/process_control/tests/batch.rs.

**Dependencies:** Task 1; approved design sections 6.1 ProcessBatchPlan, 6.2 Read-Only Preview, 8. Tree Expansion, 10. Cardinality And Completeness, 11. Confirmation Handoff, and 24. Known Limits.

- [ ] Implement the private tree-plan builder over PreparedBatchPlanning; include the root and only descendants whose entire root path has unique reliable endpoints, matching observedParentPid, nondecreasing creationTime, and no cycle.
- [ ] Attribute a direct unreliable child to its observed Stage 1 omission and every blocked descendant/invalid edge/cycle to ancestryUnverified exactly once; never reconnect a reliable descendant across an unreliable ancestor.
- [ ] Compute immutable depth, then sort by depth descending, PID ascending, and creationTime ascending. Store exact targets privately while preview exposes only ordinal/display fields.
- [ ] Apply the shared checked 256-target limit after reliable selection; 257 returns TargetSetTooLarge without a partial plan.
- [ ] Derive CompleteSelection versus IncompleteSelection from the four omission counters and prove planning performs no operation call.

**Distinct test coverage:** Root-only tree; root/child/grandchild order; stable sibling order; parent PID reuse with creation-time inversion; self-loop, reachable cycle, duplicate/missing parent, direct unreliable child, reliable descendant behind an unreliable parent, one omission per candidate, 256 accepted/257 rejected, exact frozen preview, and candidate/relationship changes after planning leaving the plan unchanged.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_tree -- --nocapture

### Task 3: Same-Name Planning And Planning Facade

**Files:** Modify src-tauri/src/process_control/batch.rs, src-tauri/src/process_control/mod.rs, and src-tauri/src/process_control/tests/batch.rs.

**Dependencies:** Tasks 1-2; approved design sections 6. Crate-Private Interface, 9. Same-Name Expansion, 10. Cardinality And Completeness, 11. Confirmation Handoff, and 19. Windows Adapter.

- [ ] Implement the same-name builder using only the verified fresh root record and ProcessPlatformAdapter::process_names_equivalent; include every reliable platform-equivalent name regardless of path and always include the root.
- [ ] Count equivalent-name observation failures by their actual reason, count invalid/uncomparable names as identityUnavailable, and exclude known nonmatching names from scoped omissions.
- [ ] Sort reliable matches by PID then creationTime and reuse the same checked limit, immutable plan representation, preview rules, and completeness derivation as tree planning.
- [ ] Implement CoreAuthority::plan_batch as the Tree/SameName dispatcher and add the Windows facade plus non-Windows PlatformUnsupported behavior. The facade returns the opaque plan, not a target vector.
- [ ] Lock the confirmation handoff contract in source tests: no re-expansion/execution method accepts a name, parent PID, mutable target list, or caller-defined order.

**Distinct test coverage:** Case/non-ASCII Adapter-equivalent names, unequal names, same name at different paths, root inclusion, equivalent omitted candidates, invalid name, deterministic order, same-name 256/257 boundary, platform comparison failure, post-plan new same-name process exclusion, non-Windows contract, and facade/source proof that preview exposes no ProcessTarget or mutation seam.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_same_name -- --nocapture

### Task 4: Serial Batch Execution And Complete Reporting

**Files:** Modify src-tauri/src/process_control/batch.rs, src-tauri/src/process_control/mod.rs, and src-tauri/src/process_control/tests/batch.rs.

**Dependencies:** Tasks 1-3; approved design sections 12. Execution Flow, 13. Target Outcomes And Report, 14. Top-Level Error Contract, 15.2 Execution, 16. Concurrency And Retry Policy, and 17. Elevation Handoff.

- [ ] Define ProcessBatchTargetError, ProcessBatchTargetOutcome, ProcessBatchReport, and ProcessBatchCompletion once, with stable redacted Display and checked summary counts. Complete requires no failures, no unattempted targets, no omissions, and only ObservedExited/AlreadyExited outcomes.
- [ ] Implement execute_batch_with_clock: consume and Authority-check the plan before the loop, start one 30-second deadline, process the frozen order serially, and return a complete ordered report after preflight.
- [ ] Before every admission and after every successful admission, check the scheduling deadline. After the second check expires, close the current admission and mark it plus all remaining entries NotAttemptedBatchDeadline.
- [ ] For each started target, call CoreAuthority::admit with the bound intent and immediately call terminate_with_clock or force_terminate_with_clock with the same clock. Never duplicate Stage 2A logic or shorten its five-second deadline.
- [ ] Normalize Stage 1/Stage 2A TargetExited to AlreadyExited, Ok to ObservedExited, and every specified failure to ProcessBatchTargetError; map structurally unreachable post-preflight contract variants to OperationUnavailable without retry or top-level escape.
- [ ] Continue after every target outcome while the batch deadline permits. Preserve timeout/pending/unknown after-side-effect classifications and prohibit rollback, retry, force fallback, elevation, or cross-target admission reuse.
- [ ] Add ProcessAuthority::execute_batch for Windows using SystemMonotonicClock and the non-Windows preflight PlatformUnsupported path.

**Distinct test coverage:** ExpiredPlan causes zero admissions/effects; exact tree/name order; independent admission per target; success/failure/AlreadyExited sequences continue; root still runs after child failure; pre-admission deadline, successful-admission deadline, failed admission crossing deadline, and one in-flight Stage 2A retaining five seconds; all remaining entries marked once; every target-error mapping; Complete/Partial matrix including omissions; no top-level Err after loop entry; no retries/fallback/elevation; overlapping plans; normal/error/deadline/catch_unwind exact HANDLE drops.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_execution -- --nocapture

### Task 5: Windows Owned-Fixture Integration

**Files:** Modify src-tauri/src/process_control/tests/batch.rs.

**Dependencies:** Tasks 1-4; approved design sections 19. Windows Adapter, 21. Real Windows Verification, and 24. Known Limits. Production operation.rs and windows.rs remain unchanged.

- [ ] Add an ignored batch fixture entry with bounded sleep, graceful-window, ignore-close-window, tree-parent, tree-child, and handshake modes. Coordinate readiness and child PID publication through a unique test-owned temporary control directory with bounded polling; product code may not create or inspect these files.
- [ ] Build a controller that receives every node PID, correlates it to an exact snapshot creationTime, opens/retains a bounded cleanup HANDLE for every node, and tears down all nodes on normal, failure, panic, and Drop paths.
- [ ] For same-name cases, copy the current test executable to a unique temporary executable name, spawn only retained children from it, and abort before batch execution if any reliable matching process is not one of the test-owned exact targets.
- [ ] Add controlled root/child/grandchild tests for graceful and force tree execution; assert report order and exact final states without targeting any existing user/system process.
- [ ] Add graceful and force same-name tests, then create a new owned matching fixture after planning and prove that it remains alive and absent from the report until explicit fixture cleanup.
- [ ] Add a partial graceful-close case where one fixture ignores WM_CLOSE while later targets still run and the report retains timeout plus completed outcomes.
- [ ] Extend source guards to prove batch.rs uses existing Stage 1/2A methods and contains no process side-effect primitive, Shell fallback, wider access right, persistence, Tauri, public plugin, Restart, or elevation path.

**Distinct test coverage:** Unique-name ownership preflight; exact handshake identities; deepest-first tree report; same-name path independence; post-plan newcomer survival; partial failure continuation; standard-user redacted errors; bounded timing; every fixture process/HANDLE and temporary copied executable cleaned with zero residue. Real PPL remains N/A when no safe fixture exists.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::windows_batch -- --nocapture --test-threads=1

### Task 6: Stage 2B Verification Record And Stop Gate

**Files:** Create docs/superpowers/verification/2026-09-07-process-target-expansion-batch-orchestration.md; modify docs/superpowers/plans/2026-09-07-process-target-expansion-batch-orchestration.md only for factual execution status and checkboxes.

**Dependencies:** Tasks 1-5; approved design sections 18. Logging And Data Handling, 20. Deterministic Verification, 21. Real Windows Verification, 22. Regression And Evidence, 23. Exit Gate, and 24. Known Limits.

- [ ] Run every focused Stage 2B suite, serialized Windows batch suite, complete process_control suite, and default full tests; record exact commands, exit codes, counts, durations, and fixture cleanup.
- [ ] Run format, all-features tests, and both Clippy gates. Compare all failures with the accepted Stage 2A verification record, retain factual red gates, and prove Stage 2B adds zero process_control diagnostics.
- [ ] Record Windows 11 x64 standard-user environment, Rust/Cargo versions, 256/257 Fake evidence, 30-second Fake evidence, tree/name real scenarios, newcomer survival, partial result behavior, and zero fixture/HANDLE residue.
- [ ] Record the parent-PID limitation, IncompleteSelection semantics, approximately 35 seconds of protocol-controlled wait budget, inherited HWND/PID race, asynchronous force-completion boundary, and no auto-force/retry/elevation.
- [ ] Prove Cargo files, operation.rs, windows.rs, Tauri wiring, frontend, public plugin contracts, persistence, Restart, elevation, macOS, and unrelated sources remain unchanged from the approved scope.
- [ ] Keep the verification status AWAITING USER ACCEPTANCE. Do not mark Stage 2B accepted or start the Restart design without explicit user acceptance of the evidence.

**Distinct test coverage:** This task owns aggregate regression, baseline comparison, standard-user evidence, changed-scope proof, exact environment/toolchain record, known-limit disclosure, and the user-acceptance stop gate; it adds no product behavior.

**Verify:** Run from repository root:

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_catalog -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_tree -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_same_name -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_execution -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::windows_batch -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
git diff --exit-code 6d44729...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock
git diff --name-status 6d44729...HEAD
Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture
[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
rustc --version
cargo --version
~~~

Expected: format, every focused suite, serialized Windows batch tests, complete process_control, and default full tests exit 0; Cargo dependency diff is empty; Stage 2B has zero process_control Clippy diagnostics; inherited aggregate failures, if still present, do not exceed the accepted Stage 2A baseline and remain explicitly red; the verification record contains exact counts/environment/scope; no fixture process, cleanup HANDLE, or copied fixture executable remains.

## Final Acceptance Checklist

- [x] Task 1 covers design sections 4, 5.3, 7.1-7.3, 14, 15.1, and the planning portion of 18.
- [ ] Task 2 covers design sections 6.1-6.2, 8, 10, 11, and tree-related known limits.
- [ ] Task 3 covers design sections 6, 9-11, and the planning facade/platform contract.
- [ ] Task 4 covers design sections 12-17 and the execution portion of 18.
- [ ] Task 5 covers the real Windows requirements in sections 19, 21, and 24 without production fallback changes.
- [ ] Task 6 records every requirement in sections 20-24 and confirms the final changed scope.
- [ ] The user reviews the verification record and explicitly accepts Stage 2B before any Restart, elevation, host product-entry, or Stage 3 design/plan/code begins.
