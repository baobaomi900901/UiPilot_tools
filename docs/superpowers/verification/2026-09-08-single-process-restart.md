# Stage 2C Single-Process Restart Verification Record

**Date:** 2026-09-09

**Status:** ACCEPTED WITH CONCERNS (conditional technical acceptance, 2026-09-09)

**Verification outcome:** **ACCEPTED WITH CONCERNS**.
Focused restart, serialized complete `process_control`, final default full,
format, scope, and cleanup evidence pass. An initial default run timed out in an
unchanged accepted Stage 2B batch fixture; the diagnostic-only run and final
original-source run passed, but the intermittent startup/snapshot risk remains
unresolved. Independent specification review passed and quality review passed
with concerns, with no Critical or Important finding. The inherited all-features
schema error and strict Clippy baselines remain explicitly red.

This record adds no product or test behavior. It aggregates Tasks 1-5 at the
final source/test revision and preserves the approval stop gate.

The user offered conditional technical acceptance on 2026-09-09 and explicitly
required the work to stay at this gate. That acceptance is recorded here; it does
not waive any retained risk or authorize further work. Elevation, host-entry and
all later roadmap stages remain on hold until separate explicit direction.

## Open Stage 2C Follow-Ups

- [GitHub Issue #1: investigate the intermittent batch-fixture snapshot timeout](https://github.com/baobaomi900901/UiPilot_tools/issues/1) — OPEN; root cause unconfirmed, no fix claimed. Closure requires evidence beyond a passing rerun.
- [GitHub Issue #2: add the fresh stop-admission failure executor assertion](https://github.com/baobaomi900901/UiPilot_tools/issues/2) — OPEN; nonblocking test-evidence gap, not a claimed production defect.

These issues are tracking records only. Their creation does not start a repair
task or release the pause. The inherited all-features/Clippy debt remains red and
outside these two narrowly scoped follow-ups.

## Revision And Environment

- Branch: `codex/plugin-platform-roadmap`
- Approved design/scope comparison base: `d209c2a`
- Final source/test revision: `8961143595424172f88f934c6afd2855c2f107f0`
- The evidence and plan-status edits follow that tested revision and change no
  Cargo, product, frontend, plugin SDK, or test source.
- Standard-user Windows session: `Microsoft Windows 11 专业工作站版`, version
  `10.0.26100`, build `26100`, x64; administrator/elevated: `False`.
- Toolchain: `rustc 1.96.1 (31fca3adb 2026-06-26)`; `cargo 1.96.1
  (356927216 2026-06-26)`.

## Exact Final Commands

Commands ran sequentially from the repository root. Real restart fixtures were
serialized; the default full suite used normal parallelism. A first complete
`process_control` capture exceeded the tool's initial output window, so the same
command was rerun once solely to capture its terminal result; no other real test
was repeated without cause.

| Command | Exit | Exact result and duration |
|---|---:|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | Clean; wall 1.386s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::restart -- --nocapture --test-threads=1` | 0 | **30 passed, 0 failed, 0 ignored**, 1130 filtered out; harness 12.67s; wall 14.020s. The printed panic messages are caught unwind-path assertions whose tests passed. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1` | 0 | **182 passed, 0 failed, 2 ignored**, 976 filtered out; harness 37.96s; wall 39.026s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline` | 0 | Final original-source run: library **1156 passed, 0 failed, 4 ignored** in 19.05s after 41.39s compilation; integration **1 passed, 0 failed**. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features` | 101 | **FAIL**, no tests ran: inherited schema-generator `E0425` at `src/bin/generate_public_plugin_schema.rs:11`; wall 44.197s. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` | 101 | **FAIL**: inherited **12 library / 13 library-test** diagnostics, all outside `process_control`; **0 `process_control` diagnostics**; wall 20.633s. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings` | 101 | **FAIL**: the same inherited **12 / 13** diagnostics plus schema-generator `E0425`; **0 `process_control` diagnostics**; wall 19.073s. |
| `git diff --exit-code d209c2a...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/process_control/core.rs src-tauri/src/process_control/operation.rs src-tauri/src/process_control/windows.rs src-tauri/src/process_control/batch.rs src/ docs/plugin-sdk/` | 0 | Empty diff; wall 0.096s. |
| `git diff --name-status d209c2a...HEAD` | 0 | Exact changed-file list recorded below; wall 0.090s. |

## Initial Default Failure And Factual Recovery

The first normal-parallel default run exited 101: library **1155 passed, 1
failed, 4 ignored** in 19.51s (wall 20.506s). The unchanged accepted Stage 2B
test
`process_control::tests::batch::windows_batch::windows_batch_graceful_timeout_keeps_later_target_running`
failed at `tests/batch.rs:3509` with `fixture did not enter snapshot`. It was
reported to the controller immediately and remains an observed unresolved
intermittent startup/snapshot risk. It is not relabeled as inherited, fixed, or
erased, and no root cause is claimed.

A diagnostic-only default run then passed library **1156/0/4** plus integration
**1/1** in 19.08s. The controller removed all temporary diagnostics via
`apply_patch`; `git diff --exit-code -- src-tauri/src/process_control/tests/batch.rs`
confirmed the exact original source.
The final exact default command at original source revision
`8961143595424172f88f934c6afd2855c2f107f0` passed with the result in the table.
No behavior fix or diagnostic source was committed. The independent final review
must assess readiness with this retained concern; this record does not waive it.

## Focused Contract Evidence

- The 30-test restart suite aggregates deterministic receipt/planning and
  execution coverage, production path-policy tests, and owned Windows restart
  integration. It covers missing/malformed context before effects, exact
  host/target binding, immutable raw-command-line/initial-directory/environment
  inputs, Graceful and Force routing, every modeled stop failure preventing
  launch, one launch only after observed exit, receipt replacement, concurrent
  attempt exclusion, cancellation/replanning, and exact attempt/lease/native
  resource cleanup on normal, error, and unwind paths.
- The Windows cases passed native managed creation and restart with spaces,
  quotes, Unicode and a known empty argument list; explicit effective CWD and
  environment; new exact PID/creation time; graceful/force behavior; ignored-close
  timeout with no launch; changed executable-parent rejection before stop; and
  replacement-receipt reuse. Initial and restart identity-error and selected
  post-stop creation failure cases use controlled fault seams. They are not
  represented as naturally observed OS faults. The existing invalid-image case
  remains the native `CreateProcessW` failure observation.
- Task 1's original libtest handshake failures were replaced by the
  user-authorized minimal helper and the production feasibility proof now passes;
  this does not assert a retrospective root cause. Task 3 did not capture a
  genuine behavioral RED; that deviation was accepted by the user through
  continuation on 2026-09-09. Task 4 retains the nonblocking Minor that the
  fresh stop-admission failure branch is statically correct but lacks its own
  focused executor assertion.

## Native Path, Resource, And Cleanup Evidence

The passing native tests exercise fixed local NTFS volume admission, volume-GUID
effective paths, no-reparse component walking, distinct executable/CWD volumes,
and retained component/final-file leases through `CreateProcessW`. They prove
the ordinary standard-user contract demonstrated on the available D: executable
and C: CWD volumes; no FAT/exFAT/ReFS volume, kernel/admin tampering, or volume
removal was tested.

Every created test process obtains independent bounded cleanup authority before
fallible identity observation or handoff. RAII tests observe one-time closure of
lease, process, thread, operation, and cleanup handles on the covered outcomes.
Fixture trees call asserted recursive removal and check that the owned path no
longer exists. After the final matrix, the observed count of matching
`%TEMP%\UiPilot * 雪 *` directories was **0**. This proves only the observed
test-owned cleanup results; it is not a claim about arbitrary system resources.

## Scope And Dependency Evidence

The exact changed-file list against `d209c2a` is:

```text
A docs/superpowers/plans/2026-09-08-single-process-restart.md
A docs/superpowers/verification/2026-09-08-restart-path-feasibility.md
M src-tauri/src/process_control/mod.rs
A src-tauri/src/process_control/restart.rs
A src-tauri/src/process_control/restart/windows.rs
M src-tauri/src/process_control/tests.rs
M src-tauri/src/process_control/tests/batch.rs
A src-tauri/src/process_control/tests/restart.rs
A src-tauri/src/process_control/tests/restart/windows.rs
```

Cargo files and accepted `core.rs`, `operation.rs`, `windows.rs`, and `batch.rs`
are unchanged. Frontend `src/` and `docs/plugin-sdk/` are unchanged. Because the
complete list contains no other Tauri/backend application files, existing
application launch callers and UI/Tauri/plugin contracts are unchanged. The
`tests/batch.rs` diff contains only the closed facade-golden aliases and method
signatures required by the new private facade. Task 3's other narrow dependency
was the fallible native environment-name comparator in `restart/windows.rs`;
existing native stop behavior was not changed.

## Limits And Failure Semantics

- Coverage is managed-only. Receipts are minted for current-host managed
  launches; arbitrary/pre-existing processes, ShellExecute launches, and
  unrecorded descendants have no attachable launch context and reject before
  stop.
- The receipt freezes original initial command line, initial working directory,
  and explicit environment. It does not preserve a process's later dynamic CWD,
  environment, windows, documents, session state, or health.
- Raw UTF-16 command-line semantics are preserved: the input includes the
  executable token and is passed with a separate application name; it is not
  tokenized and reconstructed. Known-empty user arguments are represented by an
  executable-only command line; unavailable arguments reject.
- `CreateProcessW` success is process creation, not application readiness.
  After observed old-process exit, launch failure is irreversible. Successful
  creation followed by unknown identity reports the possible new process without
  a replacement receipt. Neither case retries, rolls back, auto-forces, kills the
  possible replacement, or elevates.
- No arbitrary HANDLE inheritance, PPL/admin fixture, elevation, service/UWP,
  non-Windows runtime, or unsupported-filesystem native fixture was exercised.
  Non-Windows remains a fail-before-effects facade contract verified on this
  Windows host only.

## Exit Gate

The final required default normal-parallel regression passes at the exact tested
source, while the first run's startup/snapshot timeout remains an unresolved
intermittent test risk. The independent final review found specification
compliance **PASS** and quality **PASS WITH CONCERNS**, with no Critical or
Important finding. It retained two nonblocking concerns: Task 4's missing focused
fresh-stop-admission executor assertion and the initial unchanged Stage 2B batch
fixture's parallel snapshot timeout. The final original-source pass supports the
gate but does not fix or explain that intermittent failure.

The implementation and evidence are **ACCEPTED WITH CONCERNS** under the user's
conditional technical acceptance. Both linked follow-ups remain open. The user
explicitly requested a hold at this gate: do not start their fixes, elevation,
host-entry work, or any later stage without separate explicit direction.

## Separately Authorized Issue 2 Coverage Follow-up (2026-09-09)

After accepting the Stage 2D plan, the user approved filling the fresh-stop-admission
assertion gap separately. Added
`restart_execution_stop_admission_failure_never_stops_or_launches_and_releases_resources`
in `src-tauri/src/process_control/tests/restart.rs`. Production implementation unchanged.
For Graceful and Force, Restart policy admission succeeds and the next observation
open is denied. The executor must return Admission(InspectionDenied), open no
operation handle, send no close/termination, create no replacement, release the
policy handle and path leases once, and permit explicit receipt replanning.

Validation on the follow-up test diff over `a2dff9e`:

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline restart_execution_stop_admission_failure -- --test-threads=1`: 1 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline restart_execution_ -- --test-threads=1`: 10 passed, 0 failed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: passed.

This adds coverage for existing correct behavior; no failing-product RED or product
fix is claimed. No real process effects, UAC, full-suite or lint reruns were needed
for this test-only change. Issue 1 and inherited baseline failures remain unchanged.
The historical Stage 2C acceptance evidence above is not retroactively rewritten.
Independent review `stop_admission_assertion_review`: no blocking findings; confirmed
the fresh-admission branch, both modes, no effects and resource/receipt assertions.
GitHub Issue 2 has not been closed by this local change.
