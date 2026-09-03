# Stage 2A Single-Target Process Operation Kernel Verification Record

**Date:** 2026-09-03
**Status:** AWAITING USER ACCEPTANCE
**Verification outcome:** `DONE_WITH_CONCERNS` — all Stage 2A focused, Windows,
format, complete `process_control`, default full-test, dependency, cleanup, and
scope checks pass. Both required Clippy commands and the all-features test command
remain red only for the accepted Stage 1 baseline deviations described below;
there are zero `process_control` Clippy diagnostics at the final source revision.
**Scope:** Stage 2A aggregate regression, Windows standard-user operation evidence,
baseline comparison, changed-file audit, and exit-gate evidence only. This record
adds no product behavior.

Stage 2A is not accepted by this document. Explicit user review and acceptance of
this record is the final exit-gate item, and no later Stage 2 subsystem work may
begin before that acceptance.

## Revision Under Verification

- Branch: `codex/plugin-platform-roadmap`
- Stage 2A design-approval / dependency-comparison base: `e5c320a`
- Task 5 execution base before final Task 4 lint correction: `46148a2`
- Prior verification-record completion commit: `b1eb6f3`
- Final implementation/source/test HEAD under test:
  `0e9a9d122a49112d07049aa4039b008387bc53eb`
  (`test(process-control): cover admission unwind cleanup`)
- The documentation commit containing this record follows the tested source HEAD
  and does not alter product, test, Cargo, or source files.

The approved design is marked accepted by the user on 2026-09-03. The separate
implementation plan is also marked approved on 2026-09-03 and records Tasks 1-4
complete. Task 5 does not change its checkboxes or ledger.

## Environment

All required commands and Windows helper tests ran from the repository root in
the same standard-user session.

| Item | Exact result |
|---|---|
| OS | `Microsoft Windows 11 专业工作站版` (Windows 11 Pro for Workstations) |
| Version / build | `10.0.26100` / `26100` |
| Architecture | `64 位` (`x64`) |
| Administrator/elevated | `False`; standard-user session |
| rustc | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| cargo | `cargo 1.96.1 (356927216 2026-06-26)` |

The accepted Stage 1 record noted a plan value of Rust/Cargo 1.96.0 while the
verification host provided 1.96.1. The current implementation plan now names
1.96.1; the same installed toolchain is recorded rather than treated as a gate
failure.

## Required Command Results

Every required command below was freshly run at final source/test HEAD `0e9a9d1`.
Failing Cargo commands were additionally rerun through a PowerShell wrapper only
to print their raw `$LASTEXITCODE`; all three raw Cargo exits were `101` and the
same diagnostics reproduced.

| Command | Exit | Exact result and duration |
|---|---:|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | Clean; command wall time 1.8s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_admission -- --nocapture` | 0 | Main target: **11 passed, 0 failed, 0 ignored, 1066 filtered**, harness 0.00s; Cargo profile 0.43s; command wall 1.5s. Other unit targets ran 0 tests; integration had 1 filtered test. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_terminate -- --nocapture` | 0 | Main target: **15 passed, 0 failed, 0 ignored, 1062 filtered**, harness 0.00s; Cargo profile 0.40s; command wall 1.4s. Other unit targets ran 0 tests; integration had 1 filtered test. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::operation_force_terminate -- --nocapture` | 0 | Main target: **13 passed, 0 failed, 0 ignored, 1064 filtered**, harness 0.00s; Cargo profile 0.42s; command wall 1.5s. Other unit targets ran 0 tests; integration had 1 filtered test. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::windows_tests::windows_operation -- --nocapture --test-threads=1` | 0 | Serialized main target: **11 passed, 0 failed, 0 ignored, 1066 filtered** in **5.90s**; Cargo profile 0.39s; command wall 7.3s. Other unit targets ran 0 tests; integration had 1 filtered test. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1` | 0 | Main target: **100 passed, 0 failed, 1 ignored, 976 filtered** in **7.97s**; Cargo profile 0.41s; command wall 9.4s. Other unit targets ran 0 tests; integration had 1 filtered test. The ignored item is the helper fixture explicitly spawned by owned-child tests. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline` | 0 | Main library: **1074 passed, 0 failed, 3 ignored, 0 filtered** in **6.14s**; integration: **1 passed, 0 failed** in 0.00s; remaining unit/doc targets ran 0 tests. Cargo profile 0.40s; command wall 9.1s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features` | 101 | No tests ran. The schema-generator binary and binary-test target cannot resolve cfg-excluded `uipilot_lib::public_plugin_manifest_schema`: `E0425` at `src/bin/generate_public_plugin_schema.rs:11`. Initial command wall 28.1s; raw exit 101 and the same failure reproduced on a 2.0s incremental capture. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` | 101 | Reproduced the accepted baseline: **12 library** and **13 library-test** deny-warning diagnostics, all outside `process_control`; **0 `process_control` diagnostics**. Initial command wall 14.7s; raw exit 101 and the same counts reproduced on a 17.8s capture. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings` | 101 | The same **12 library / 13 library-test** Clippy baseline, all outside `process_control`, plus schema `E0425` for the binary and binary-test targets; **0 `process_control` diagnostics**. Initial command wall 14.1s; raw exit 101 and the same result reproduced on a 22.5s capture. |
| `git diff --exit-code e5c320a...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock` | 0 | Empty dependency diff; command wall 0.8s. |
| `Get-CimInstance Win32_OperatingSystem \| Select-Object Caption, Version, BuildNumber, OSArchitecture` | 0 | Returned the exact OS values above; command wall 1.3s. |
| `[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)` | 0 | `False`; command wall 0.6s. |
| `rustc --version` | 0 | Version above; command wall 0.9s. |
| `cargo --version` | 0 | Version above; command wall 0.8s. |

The all-features test and both Clippy commands are factually **FAIL**, not green.
They match but do not improve the user-accepted Stage 1 repository baseline. The
Stage 2A-owned requirement is satisfied because the final Clippy output contains
zero diagnostic paths under `process_control` and the 12/13 counts do not exceed
the baseline.

## Baseline Comparison And Known Aggregate Deviations

The exact inherited Clippy sites are unchanged from the accepted Stage 1 record:

- library and library-test: `src/commands.rs:1290` (`too_many_arguments`),
  `src/clipboard_history/observer.rs:381` (`manual_div_ceil`),
  `src/clipboard_history/paste.rs:139` (`forget_non_drop`),
  `src/clipboard_history/store.rs:149` (`redundant_closure`),
  `src/clipboard_history/store.rs:337` (`match_single_binding`),
  `src/public_plugins/network/broker.rs:422`, `:433`, `:464`, `:465`, and `:605`
  (`needless_as_bytes`), `src/plugin_panel.rs:1130` (`nonminimal_bool`), and
  `src/lib.rs:229` (`collapsible_match`): **12 sites**;
- library-test only: `src/clipboard_history/mod.rs:783`
  (`needless_borrows_for_generic_args`): **1 additional site**, for **13 total**;
- all-features schema compile failure:
  `src/bin/generate_public_plugin_schema.rs:11` (`E0425`) for the binary and
  binary-test targets.

At task base `46148a2`, Task 5 initially found two additional Stage 2A diagnostics
in `process_control/windows.rs`. They were corrected and cumulatively re-reviewed
in source commit `7eeaad6`. The complete matrix above is a fresh rerun at the
current test revision `0e9a9d1` and proves both diagnostics remain absent; neither
Clippy gate contains any new `process_control` diagnostic.

## Fake Adapter And Operation Contract Evidence

The focused contract suites passed all **11 final-admission**, **15 graceful**, and
**13 force** tests, and the complete `process_control` suite passed all applicable
Stage 1 and Stage 2A tests.

- Final admission selects exactly graceful or force access, rejects intent
  mismatch before operation-platform calls, revalidates creation time, liveness,
  protection, exact self, critical, and PPL policy, retains both handles through
  the checks, then closes the Stage 1 admission handle immediately before the
  side-effect closure.
- `operation_admission_unwind_drops_handles_exactly_once_in_order` wraps final
  admission in `catch_unwind(AssertUnwindSafe(...))` and uses
  `std::panic::resume_unwind` inside the operation closure. Its event timeline
  proves the admission handle drops before closure entry, the closure sees only
  the operation handle active, the operation handle drops during unwind, both
  handles drop exactly once, and the unwind is caught without invoking the panic
  hook.
- Graceful close selects only visible, ownerless, exact-PID top-level windows,
  posts each eligible window once, rechecks operation-HANDLE liveness and current
  PID/visibility/owner before every post, never retargets a gone/changed HWND, and
  never calls `TerminateProcess`.
- The single five-second monotonic deadline includes enumeration, revalidation,
  posting, and completion. Tests cover exhaustion before the first post, between
  posts, during fact refresh, and after partial success, plus the required final
  zero-time completion poll.
- Force termination calls `TerminateProcess` at most once with exit code `1` and
  covers pre-call exit, immediate/delayed signaled completion, pending completion,
  terminate failure followed by signaled/alive/unknown repoll, access denial, and
  completion inspection failure.
- Failure, success, timeout, unwind, and independent-lease cases assert exact
  handle accounting. Behavioral call-count coverage proves termination is
  invoked at most once; no operation retry, elevation attempt, or
  graceful-to-force fallback occurs.
- Stable error-display tests cover the complete operation error set and expose
  only stable codes without process or platform data.

## Windows Standard-User Evidence

The serialized real-Windows operation suite passed **11 tests** in **5.90s** and
uses only helper subprocesses created from the current Rust test executable with
the exact ignored-fixture mode.

- **Graceful GUI close:** an owned helper created a visible, ownerless top-level
  window and message loop. `WM_CLOSE` produced `Ok(())`; the exact child exited
  within the test's two-second bound, followed by bounded cleanup.
- **Windowless rejection:** an owned 30-second helper exposed no eligible window.
  The result was exactly `GracefulCloseUnavailable`; the child was proven still
  alive before cleanup, demonstrating no side effect, then only that retained
  child was cleaned.
- **Ignored-close timeout:** an owned GUI helper ignored `WM_CLOSE`. The operation
  used the five-second graceful deadline and returned exactly
  `GracefulCloseTimedOut`; the passing test measured at least 4.5s and at most 8s,
  proved the child still alive before cleanup, then observed its exit. The test
  does not print the individual elapsed value; the complete serialized suite was
  5.90s.
- **Force termination:** an owned ordinary helper was force-terminated, the
  operation returned `Ok(())`, and the retained exact child signaled within the
  two-second assertion before bounded cleanup.
- **Pre-side-effect identity/protection:** changing only the current process
  creation time returned exactly `TargetChanged`; the current host's exact
  identity returned `ProtectedTarget`.
- **Least access:** graceful operation opening requested exactly
  `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`; force added only
  `PROCESS_TERMINATE`; both were non-inheritable. Source guards rejected
  `PROCESS_ALL_ACCESS`, thread/VM/token rights, `SeDebugPrivilege`, shell/WMI,
  `taskkill`, fallback native termination APIs, and raw-HANDLE exposure.
- **HANDLE cleanup:** the real `OwnedHandle` Drop regression observed closer calls
  `0` before Drop and exactly `1` after Drop. Fake paths assert exact opens/drops,
  including deterministic admission-before-closure and operation-during-unwind
  ordering with one drop per handle.
  After all final Cargo commands, a CIM query for an `uipilot_lib-*.exe` command
  line containing the exact fixture test name returned
  `FIXTURE_HELPER_COUNT=0` in 1.7s. No fixture helper remained; no separate
  system-wide HANDLE census was performed.
- **Protected process:** no inspectable real protected/PPL target was present; the
  complete suite recorded permitted `environment N/A`. Deterministic Fake tests
  for critical, PPL, combined protection, and unreadable protection passed.

Real operation tests terminate only helpers they created and retained as exact
`Child` values. Existing user and system processes were not acted upon. Current
self/protected checks are admission-only; tests never attempt real critical/PPL
termination and never enable debug privilege.

## Logging And Data Handling

`operation_error_display_is_exact_and_redacted` and the Windows context-specific
error-classification/redaction test passed. A supplemental scan of production
`process_control` files (`core.rs`, `mod.rs`, `operation.rs`, and `windows.rs`)
found `PRODUCTION_LOG_EMISSION_MATCH_COUNT=0` for `tracing`, `log`, print/debug,
and log-level emission macros; command wall time was 1.3s.

Ordinary Display/results therefore contain only stable error codes and no PID,
path, command line, window title/content, HWND, HANDLE, raw Win32 code, or user
data. Window candidate inspection reads no title or content. Stage 2A writes no
operation history and retains no process identity on disk.

## HWND Identity Limitation And Force Completion Boundary

The operation HANDLE remains open through graceful-close completion checking.
Immediately before each `PostMessageW`, the implementation polls that HANDLE for
liveness and re-reads the HWND's current PID, visibility, and owner. An invalid or
changed HWND is skipped and never replaced with a newly found window.

A narrow residual race remains between the last HANDLE/PID/window-fact validation
and `PostMessageW`: the target can exit and both its HWND and PID can be reused.
Windows provides no public API that sends `WM_CLOSE` to a process HANDLE, so
graceful close cannot claim force termination's HANDLE-backed identity guarantee.
This limitation remains user-visible and is not represented as eliminated.

`TerminateProcess` completion is asynchronous. A successful call alone is not
reported as `Ok(())`; the retained operation HANDLE must be observed signaled by
the bounded wait or final zero-time poll. If it remains alive at the five-second
boundary the result is `TerminationPending`; an unreadable completion state is
`CompletionUnknown`. There is no automatic retry, automatic force after graceful
failure, Restart execution, or automatic elevation.

## Dependency And Changed-File Scope

`src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` are byte-identical to Stage 2A
base `e5c320a`. Their Git blob IDs at both base and final source HEAD are,
respectively, `36aba5680ba2f2eb5aceda973a465293ccb4fb3e` and
`e19e3b0c1c6bfcf2ca7f1fc2a8d6826ed5e00346`. The existing `windows 0.61.3`
dependency already includes `Win32_System_Threading` and
`Win32_UI_WindowsAndMessaging`.

The final intended tracked file list relative to `e5c320a` at tested source/test
commit `0e9a9d1` is:

- `docs/superpowers/plans/2026-09-03-single-target-process-operation-kernel.md`
  — approved implementation plan;
- `docs/superpowers/verification/2026-09-03-single-target-process-operation-kernel.md`
  — this Task 5 evidence record;
- `src-tauri/src/process_control/core.rs` — scoped adapter contract extension;
- `src-tauri/src/process_control/mod.rs` — private facade operation methods;
- `src-tauri/src/process_control/operation.rs` — private operation core;
- `src-tauri/src/process_control/tests.rs` — Fake and owned-helper tests;
- `src-tauri/src/process_control/windows.rs` — Windows operation adapter.

No Cargo dependency/lockfile, Tauri command/state/capability/wiring, frontend,
public plugin contract/API/schema, persistence, Restart implementation, target
expansion, batch/same-name/process-tree/search behavior, elevation broker, macOS
adapter, console/service control, launcher, settings, panel, or host product entry
point changed. The final-review correction relative to `b1eb6f3` adds only the
test-only unwind regression; the following documentation commit updates only this
verification record.

## Design 14 / 15 Exit Checklist

| Requirement | Evidence | Result |
|---|---|---|
| Approved design and separate plan | Both tracked documents state user approval on 2026-09-03. | PASS |
| Tasks 1-4 ownership | Task 1 owns design 4-6; Task 2 plus Task 4 Windows work owns 7/7.1; Task 3 plus Task 4 Windows work owns 8; Tasks 1-4 collectively cover 9-13. The final cumulative Task 4 re-review reported compliant with no findings. | PASS |
| Fake operation contracts | Focused admission 11, graceful 15, and force 13 tests pass with identity, access, ordering, deadlines, error, concurrency, unwind, and exact-cleanup evidence. | PASS |
| Real Windows owned-helper scenarios | Serialized 11-test suite passes graceful GUI, windowless, ignored-close timeout, force, stale identity, self, access, redaction, and guard cases. | PASS |
| Minimal rights / final revalidation / cleanup | Exact access tests, normal and unwind operation event timelines, Fake open/drop counts, real RAII exact-close test, and residual helper count 0. | PASS |
| Completion and uncertain-side-effect contract | Graceful and force suites cover signaled success, timeout/pending, and completion unknown with final zero polls. | PASS |
| No retry / auto-force / elevation | Behavioral exact one-call tests prove no retry; focused tests and the Windows source guard prove no auto-force or elevation. | PASS |
| No out-of-scope product/platform work | Dependency and final changed-file audit above. | PASS |
| Regression/environment/limitations/scope recorded | Exact command table, environment, baseline, protected-target N/A, HWND race, async force boundary, cleanup, logging, and changed files above. | PASS with recorded deviations/N/A |
| Aggregate Cargo gates | Fmt, focused, `process_control`, and default full tests pass. All-features test and both Clippy commands remain red only for the accepted Stage 1 baseline; zero `process_control` diagnostics. | RECORDED DEVIATIONS — aggregate commands not all green |
| User review and explicit Stage 2A acceptance | This record has not yet been reviewed and accepted by the user. | **PENDING — REQUIRED FINAL GATE** |

## Known Deviations And Stop Gate

1. The all-features test command exits 101 during schema-generator compilation
   with the inherited Stage 1 `E0425`; it does not run tests.
2. Both Clippy commands exit 101 with the inherited 12 library / 13 library-test
   diagnostics outside `process_control`. All-features Clippy additionally emits
   the inherited schema `E0425`. There are zero `process_control` diagnostics.
3. A real inspectable protected/PPL target was unavailable, recorded as permitted
   environment N/A with deterministic Fake coverage.
4. The graceful HWND/PID residual reuse race and asynchronous force completion
   boundary remain explicit platform limitations as described above.

The evidence is complete enough to request user review, but Stage 2A remains
**AWAITING USER ACCEPTANCE**. This document does not accept the subsystem. Do not
create detailed design, plan, or code for process tree, same-name batch, Restart,
elevation broker, or any later Stage 2 subsystem until the user explicitly accepts
this verification record.
