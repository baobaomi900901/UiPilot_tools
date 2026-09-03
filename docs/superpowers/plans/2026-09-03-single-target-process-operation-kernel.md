# Stage 2A Single-Target Process Operation Kernel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Checkboxes are the execution record.

**Date:** 2026-09-03
**Status:** Implementation and final review complete — user acceptance pending

**Goal:** Deliver the host-internal single-target Windows process operation kernel for graceful GUI close and force termination, with final identity/protection revalidation, exact minimum rights, bounded completion semantics, and no product or plugin entry point.

**Architecture:** Extend the existing deep `process_control` Module with a focused private `operation.rs`. The Operation Core owns final admission, ordering, deadlines, stable errors, and retry policy; `windows.rs` and the deterministic Fake Adapter expose only platform primitives. `ProcessAuthority` remains the only crate-private facade and consumes the opaque Stage 1 `ProcessAdmission` exactly once.

**Tech Stack:** Rust `1.96.1`, edition 2021, `windows 0.61.3`, Windows 11 x64, the existing Cargo dependency graph.

**Approved design:** [`Stage 2A Single-Target Process Operation Kernel Design`](../specs/2026-09-03-single-target-process-operation-kernel-design.md)

## Global Constraints

- Implementation scope is measured from design-approval commit `e5c320a`; preserve unrelated user changes.
- Implement only Stage 2A. Do not add process-tree/same-name expansion, Restart, elevation, Tauri state/commands, Launcher/Panel/UI, public plugin contracts, configuration, persistence, macOS, console control, or service control.
- `ProcessAuthority::terminate` and `force_terminate` consume a non-cloneable `ProcessAdmission`; an intent mismatch performs no operation-platform call or side effect.
- Final operation admission follows approved design section `5. Shared Final Operation Admission` exactly and closes the admission HANDLE immediately before the operation-specific side-effect closure.
- Graceful access is exactly `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`; force access adds exactly `PROCESS_TERMINATE`.
- Never request `PROCESS_ALL_ACCESS`, `PROCESS_CREATE_THREAD`, VM/token rights, `SeDebugPrivilege`, or inheritable process handles.
- Graceful close targets only visible, no-owner, exact-PID top-level desktop windows, revalidates complete window facts before each `PostMessageW`, and never falls back to force termination.
- `Ok(())` always means the operation HANDLE was observed signaled. A queued message or successful `TerminateProcess` call alone is not completion.
- Use one monotonic five-second deadline per side-effect flow and a final zero-time poll at its boundary; never sleep in Fake Adapter tests.
- No automatic retry or elevation. Only pre-side-effect `InspectionDenied` and `OperationAccessDenied` are eligible for a future broker to restart from confirmation and Stage 1 admission.
- Ordinary logs and error display contain stable codes only, never PID, path, command line, HWND, HANDLE, window title, user data, or raw Win32 code.
- Real process side effects are restricted to exact helper children created and retained by the test fixture. Never operate on an existing user or system process.
- Stage 2A does not synthesize input, focus, activate, or foreground any target window. Automated test invocation is the explicit authorization to operate on its own fixture child; no product confirmation UI is introduced here.
- `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` remain byte-identical; all required Windows features are already enabled.

## File Structure And Core Contract

- `src-tauri/src/process_control/operation.rs` — stable operation errors, private operation values/Adapter/clock Seams, shared final admission, graceful and force algorithms.
- `src-tauri/src/process_control/core.rs` — retain Stage 1 semantics; add only narrow internal accessors and consuming admission decomposition required by `operation.rs`.
- `src-tauri/src/process_control/windows.rs` — Win32 operation primitives, failure classification, HANDLE/HWND wrappers, and native helper seams.
- `src-tauri/src/process_control/mod.rs` — declare `operation`, re-export the crate-private error type, and add the two facade methods without Tauri wiring.
- `src-tauri/src/process_control/tests.rs` — extend the existing scripted Fake Adapter, Fake Clock, contract tests, and ignored Windows helper fixture.
- `docs/superpowers/verification/2026-09-03-single-target-process-operation-kernel.md` — exact automated/manual evidence, environment, scope, and known limitations.

Use these exact shared names unless the approved design requires a mechanical signature adjustment:

- Public-to-crate result: `ProcessOperationError` with exactly the 12 variants in design section `11. Stable Error Contract`.
- Private values: `OperationAccess`, `OperationFailure`, `WindowFailure`, `WindowCandidate<W>`, `WindowFacts`, and `ExitWait`.
- Private Seams: `ProcessOperationAdapter: ProcessPlatformAdapter` and `MonotonicClock`.
- Production clock: `SystemMonotonicClock`; test clock: `FakeMonotonicClock`.
- `OperationFailure` distinguishes `ExitedOrChanged`, `AccessDenied`, and `Unavailable`.
- `WindowFailure` distinguishes `Gone`, `AccessDenied`, and `Unavailable`.
- `ExitWait` distinguishes `Exited` and `TimedOut`; wait inspection failure remains a `Result` error.
- `OperationAccess::GRACEFUL` and `OperationAccess::FORCE` are the only constructible production access profiles.
- Generic core entry points are `terminate_with_clock` and `force_terminate_with_clock`; production facade methods supply `SystemMonotonicClock`, while tests supply the Fake Clock.

The design specification remains canonical for interface semantics, event order, security boundaries, error priority, platform limitations, and completion behavior.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5`. Tasks 2 and 3 both edit the shared Operation Core and must not run in parallel.
- Every task follows focused TDD: add the task-specific failing tests, confirm the intended failure, implement the minimum contract, rerun the focused tests, and commit one atomic change.
- A task commit must not contain another task, a later subsystem, or pre-existing user changes. Review fixes use separate commits.
- Each task receives specification-compliance and code-quality review before its dependent task begins when the active execution workflow provides those gates.
- Run Rust commands from the repository root with `--manifest-path src-tauri/Cargo.toml --locked --offline`.
- Product code must never launch or clean test helpers. Helper cleanup remains test-fixture-only, bounded, and anchored to the retained `Child` HANDLE.

---

### Task 1: Operation Contracts And Final Admission Kernel

**Files:** Create `src-tauri/src/process_control/operation.rs`; modify `src-tauri/src/process_control/core.rs`, `src-tauri/src/process_control/mod.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Approved design sections `3.1 Separate Operation Core`, `4. Module Interface`, `5. Shared Final Operation Admission`, `6. Least-Privilege Operation Access`, `9. Private Adapter And Clock Seams`, `10. Resource Ownership And Concurrency`, `11. Stable Error Contract`, and `12. Logging And Data Handling`.

- [x] Define `ProcessOperationError` with stable redacted Display values and the private shared values/Seams from `File Structure And Core Contract`; expose only the error type through `mod.rs`.
- [x] Add narrow `CoreAuthority` accessors for its Adapter and exact self identity, plus `CoreAdmission::into_parts`; do not expose a raw HANDLE, PID-reopen facility, public fields, Clone, or serialization.
- [x] Implement one shared final-operation helper that consumes the admission, validates intent before operation calls, opens exactly one operation HANDLE while the admission HANDLE remains alive, then checks creation time, liveness, protection readability, self, critical, and PPL in the approved order.
- [x] Make the helper drop the admission HANDLE immediately before invoking a passed operation closure and keep the operation HANDLE alive through that closure; every rejection must skip the closure and close all owned handles exactly once.
- [x] Extend the existing Fake Adapter with separate observation/open-operation call events, operation-access capture, scripted window/termination/wait outcomes, side-effect counters, and ordered handle events. Add a Fake Clock that advances from a fixed `Instant` without real waiting.
- [x] Preserve the existing Stage 1 Adapter contract and source guards; `core.rs` remains free of operation primitives and platform types.

**Distinct test coverage:** Intent mismatch drops the lease with zero operation calls; each supported operation intent selects only its exact access profile; operation-open exited/access/unavailable classifications; replacement creation time returns `TargetChanged` before replacement protection; exit and inspection failure precedence; self/critical/PPL fail closed; event log proves admission HANDLE open during final checks, dropped immediately before the closure, and operation HANDLE retained through closure; all early failures call the closure zero times and close handles once; error Display output is exactly the stable code and contains no scripted sensitive values.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_admission -- --nocapture`

### Task 2: Graceful GUI Termination Core

**Files:** Modify `src-tauri/src/process_control/operation.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Task 1; approved design sections `3.2 Ordinary Termination Means GUI Graceful Close`, `3.3 Success Means Exit Was Observed`, `3.4 Fixed Five-Second Monotonic Deadline`, `7. Ordinary Termination Flow`, `7.1 HWND Identity Limitation`, `10. Resource Ownership And Concurrency`, and `11. Stable Error Contract`.

- [x] Implement `terminate_with_clock` through the shared final-operation helper using `OperationAccess::GRACEFUL`; never invoke the force primitive.
- [x] Start one five-second deadline after final admission, enumerate candidates once, filter enumeration facts by exact PID/visible/no-owner, and perform zero-time process plus current PID/visibility/owner revalidation immediately before each post.
- [x] Post `WM_CLOSE` at most once to every still-eligible token in enumeration order, stop on target exit or deadline, and apply the exact zero-window, changed/gone window, all-denied, all-unavailable, partial-success, pre-first-post deadline, timeout, and completion-unknown aggregation rules.
- [x] Wait only for the deadline's remaining duration and perform the mandatory final zero-time poll; return `Ok(())` only after observing the operation HANDLE signaled.
- [x] Keep the narrow HWND/PID reuse limitation explicit in source comments at the final window-facts/post seam without claiming HANDLE-equivalent targeting.

**Distinct test coverage:** Hidden, owned, and other-PID candidates are excluded; the Adapter contract supplies no child or message-only candidate; multiple valid windows each receive one post; facts changing between enumeration and post are skipped; a gone candidate is not retargeted; no initial/remaining candidate maps to `GracefulCloseUnavailable` unless the process has exited; exit before first or between posts stops side effects and succeeds; all access failures versus mixed/other failures; partial post success waits and ignores later post failures for classification; Fake Clock consumption before first post and between posts; successful exit, `GracefulCloseTimedOut`, and `CompletionUnknown`; exact event order and proof that the terminate primitive is never called.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_terminate -- --nocapture`

### Task 3: Force-Termination Core

**Files:** Modify `src-tauri/src/process_control/operation.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Tasks 1-2; approved design sections `3.3 Success Means Exit Was Observed`, `3.4 Fixed Five-Second Monotonic Deadline`, `6. Least-Privilege Operation Access`, `8. Force-Termination Flow`, `10. Resource Ownership And Concurrency`, and `11. Stable Error Contract`.

- [x] Implement `force_terminate_with_clock` through the shared final-operation helper using `OperationAccess::FORCE` and fixed exit code `1`.
- [x] Poll immediately before the side effect, call the terminate primitive at most once, wait only for the deadline's remaining duration, and perform the final zero-time poll.
- [x] On terminate-call failure, re-poll the same operation HANDLE before mapping access versus other failures; a signaled handle returns success, an unobservable terminal state returns `CompletionUnknown`.
- [x] Preserve the post-side-effect uncertainty boundary: successful terminate plus timeout is `TerminationPending`, wait failure is `CompletionUnknown`, and neither can retry, elevate, or invoke terminate again.
- [x] Verify two independent leases against the same fake object follow the approved final-admission versus post-admission concurrent-exit outcomes without global locks or deduplication.

**Distinct test coverage:** Pre-call concurrent exit produces success without terminate; terminate is called once with exit code `1`; immediate/delayed exit succeeds; successful terminate plus deadline exhaustion is `TerminationPending`; successful terminate plus wait-inspection failure is `CompletionUnknown`; failed terminate plus signaled re-poll succeeds; failed terminate plus alive maps access to `OperationAccessDenied` and other failure to `OperationUnavailable`; failed terminate plus failed re-poll is `CompletionUnknown`; no automatic retry/elevation; independent lease ordering and exact HANDLE drops.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_force_terminate -- --nocapture`

### Task 4: Windows Adapter, Facade, And Owned-Helper Tests

**Files:** Modify `src-tauri/src/process_control/windows.rs`, `src-tauri/src/process_control/mod.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Tasks 1-3; approved design sections `4. Module Interface`, `6. Least-Privilege Operation Access`, `7. Ordinary Termination Flow`, `7.1 HWND Identity Limitation`, `8. Force-Termination Flow`, `9. Private Adapter And Clock Seams`, `12. Logging And Data Handling`, `13. Windows Adapter`, and `14.2 Real Windows Tests`.

- [x] Implement `ProcessOperationAdapter` for `WindowsProcessAdapter` with public Win32 APIs only: exact non-inheritable `OpenProcess`, `EnumWindows`, `GetWindowThreadProcessId`, `IsWindowVisible`, `GetWindow(GW_OWNER)`, `PostMessageW(WM_CLOSE)`, `TerminateProcess`, and `WaitForSingleObject`.
- [x] Add narrow native helper seams for exact access-mask conversion, window collection/fact refresh, remaining-duration conversion, wait-result mapping, and context-specific error classification. Round a positive remaining duration up to a finite millisecond value, never pass `INFINITE`, and map UIPI `ERROR_ACCESS_DENIED` without exposing raw codes.
- [x] Add the two production facade methods in `mod.rs`, consume the concrete admission inner value, and supply `SystemMonotonicClock`; keep equivalent non-Windows methods returning `PlatformUnsupported` without manufacturing an admission.
- [x] Replace the Stage 1 “no side-effect source” guard with Stage 2A scope guards that require the approved primitives while rejecting `PROCESS_ALL_ACCESS`, `PROCESS_CREATE_THREAD`, `SeDebugPrivilege`, `EndTask`, taskkill, PowerShell, WMI, Shell execution, and undocumented NT APIs.
- [x] Extend the ignored current-test-executable fixture with bounded `sleep`, `graceful-window`, and `ignore-close-window` modes. The two window modes create one visible, no-owner, non-activating top-level window and run a message loop without stealing focus; the first destroys the window and exits on `WM_CLOSE`, while the second keeps pumping and ignores it.
- [x] Add readiness polling through the Adapter's read-only window facts before invoking product operations. Every real test retains the exact `Child` and uses only its bounded fixture cleanup path.
- [x] Add real Windows tests for graceful exit, no-window unavailable, five-second ignored-close timeout, and force termination. Reuse the existing real changed-creation-time and self-protection admission tests as proof that those targets cannot produce a Stage 2A lease or side effect; do not manufacture an invalid operation lease. No test discovers an arbitrary process and then performs an operation on it.

**Distinct test coverage:** Exact native access masks and non-inheritance for both profiles; HWND wrapper/fact conversion and `Gone`/access/unavailable classification; wait mapping for zero and bounded waits; current helper window eligibility; graceful helper exits through `WM_CLOSE`; windowless helper receives no side effect; ignore-close helper times out and is cleaned by its retained child; force helper exits and signals its exact operation HANDLE; changed target and self make zero side-effect calls; fixture readiness and cleanup are bounded; source contains no forbidden fallback or wider right.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::windows_tests::windows_operation -- --nocapture --test-threads=1`

### Task 5: Stage 2A Verification Record And Stop Gate

**Files:** Create `docs/superpowers/verification/2026-09-03-single-target-process-operation-kernel.md`.

**Dependencies:** Tasks 1-4; approved design sections `7.1 HWND Identity Limitation`, `12. Logging And Data Handling`, `14. Verification`, and `15. Exit Gate`.

- [x] Run the focused admission, graceful, force, Windows-operation, and complete `process_control` suites; record exact commands, exit codes, counts, durations, and helper cleanup results.
- [x] Run format, default/all-feature test, and both Clippy gates. Compare any failure with the Stage 1 accepted baseline, prove there is no new `process_control` diagnostic, and record facts rather than silently treating an inherited failure as success.
- [x] In a Windows 11 x64 standard-user session, record graceful GUI close, windowless rejection, ignored-close five-second timeout, force termination, no residual helper/HANDLE, and ordinary-log redaction. Record exact OS build, architecture, elevation state, Rust, and Cargo versions.
- [x] Record the HWND/PID residual race, asynchronous `TerminateProcess` completion boundary, no auto-force/retry/elevation, and no action against existing user/system processes.
- [x] Record the final changed-file list and prove Cargo files, Tauri wiring, frontend, public plugin contracts, persistence, Restart, target expansion, and elevation remain unchanged.
- [x] Stop at the Stage 2A acceptance gate. Do not mark the subsystem accepted and do not design or implement the next Stage 2 subsystem without explicit user acceptance of the evidence.

**Distinct test coverage:** This task owns aggregate regression, standard-user manual evidence, baseline comparison, changed-scope proof, and the acceptance record only; it adds no product behavior.

**Verify:** Run from repository root:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_admission -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_terminate -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_force_terminate -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::windows_tests::windows_operation -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
git diff --exit-code e5c320a...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock
Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture
[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
rustc --version
cargo --version
```

Expected: format, all focused suites, complete `process_control`, and default full tests exit 0; Cargo dependency diff is empty; `process_control` has zero Clippy diagnostics; inherited all-features/Clippy deviations, if still present, match rather than exceed the accepted Stage 1 baseline; the verification record contains exact environment and results; no helper process or HANDLE remains.

## Final Acceptance Checklist

- [x] Design sections `4-6` are owned by Task 1.
- [x] Design sections `7` and `7.1` are owned by Task 2 and completed by the Windows portion of Task 4.
- [x] Design section `8` is owned by Task 3 and completed by the Windows portion of Task 4.
- [x] Design sections `9-13` are covered by Tasks 1-4 without leaking platform resources or adding wider rights.
- [x] Design section `14` and every item in `15. Exit Gate` are evidenced by Task 5.
- [ ] The user reviewed the verification record and explicitly accepted Stage 2A before any later Stage 2 design, plan, or code began.
