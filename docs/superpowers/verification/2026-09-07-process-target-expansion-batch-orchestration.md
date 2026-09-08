# Stage 2B Process Target Expansion And Batch Orchestration Verification Record

**Date:** 2026-09-08  
**Status:** AWAITING USER ACCEPTANCE  
**Verification outcome:** `DONE_WITH_CONCERNS` - all Stage 2B focused, serialized
Windows, complete `process_control`, default full-test, format, dependency,
cleanup, and scope gates pass at the final source revision. The all-features test
and both required Clippy commands remain factually red only for the accepted
Stage 1/2A repository baseline. Final Clippy output has zero `process_control`
diagnostics.

This record adds no product behavior. Stage 2B is not accepted until the user
reviews this evidence and explicitly accepts it. No Restart, elevation, host
product-entry, or Stage 3 design or implementation may begin before that gate.

## Revision Under Verification

- Branch: `codex/plugin-platform-roadmap`
- Approved changed-scope comparison base: `6d44729`
- Task 6 clean execution base: `1766be84ebd0b2d53073222e64f4bd6dc7753793`
- Final source/test revision: `ec153562bafb4812ae3760c536d90dbaf700f468`
- The documentation commit containing this record follows the tested source
  revision and changes no Cargo, product, frontend, or test source.

## Environment

All commands ran from the repository root in the same standard-user Windows
session.

| Item | Exact result |
|---|---|
| OS | `Microsoft Windows 11 专业工作站版` (Windows 11 Pro for Workstations) |
| Version / build | `10.0.26100` / `26100` |
| Architecture | `64 位` (`x64`) |
| Administrator/elevated | `False`; standard-user session |
| rustc | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| cargo | `cargo 1.96.1 (356927216 2026-06-26)` |

## Required Command Results

The focused commands below were freshly run at `7995c34`; the later `ec15356`
change adds only the fixture-guard ownership regression. Serialized Windows and
default full tests were rerun at final revision `ec15356` by the scoped fix
worker, and the evidence owner reran complete `process_control`, all-features,
and both exact Clippy gates there. Command wall times are orchestration wall times
unless identified as harness times.

| Command | Exit | Exact result and duration |
|---|---:|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | Clean; wall 2.074s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_catalog -- --nocapture` | 0 | **12 passed, 0 failed, 0 ignored**; wall 1.611s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_tree -- --nocapture` | 0 | **10 passed, 0 failed, 0 ignored**; wall 1.412s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_same_name -- --nocapture` | 0 | **4 passed, 0 failed, 0 ignored**; wall 1.437s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::batch_execution -- --nocapture` | 0 | **17 passed, 0 failed, 0 ignored**; wall 1.383s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::batch::windows_batch -- --nocapture --test-threads=1` | 0 | **8 passed, 0 failed, 1 ignored**; harness 16.55s. The ignored test is the explicitly spawned fixture mode. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1` | 0 | **152 passed, 0 failed, 2 ignored**; wall 22.254s. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline` | 0 | Library **1126 passed, 0 failed, 4 ignored** in 13.79s; integration **1 passed, 0 failed**. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features` | 1 | **FAIL**, no tests ran; inherited schema-generator `E0425` at `src/bin/generate_public_plugin_schema.rs:11`; wall 29.160s. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` | 1 | **FAIL**, accepted baseline of **12 library / 13 library-test** diagnostics, all outside `process_control`; **0 `process_control` diagnostics**; wall 15.629s. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings` | 1 | **FAIL**, same **12 / 13** diagnostics plus inherited schema `E0425`; **0 `process_control` diagnostics**; wall 13.156s. |
| `git diff --exit-code 6d44729...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock` | 0 | Empty dependency diff; wall 0.801s. |
| `git diff --name-status 6d44729...HEAD` | 0 | Exact changed-file list recorded below; wall 0.745s. |
| `Get-CimInstance Win32_OperatingSystem \| Select-Object Caption, Version, BuildNumber, OSArchitecture` | 0 | Exact OS values above; wall 1.002s. |
| `[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)` | 0 | `False`; wall 0.678s. |
| `rustc --version` | 0 | Exact version above; wall 0.808s. |
| `cargo --version` | 0 | Exact version above; wall 0.714s. |

The all-features and Clippy gates are red, not pass. Their external diagnostics
exactly match the accepted Stage 2A record: the 12 library sites, one additional
library-test site, and schema-generator `E0425`. They neither improve nor exceed
that accepted baseline.

## Verification Discovery And Correction

At clean base `1766be8`, the first exact default full run failed with exit 1 in
17.854s: **1123 passed, 1 failed, 4 ignored**. The sole failure was
`process_control::tests::batch::windows_batch::windows_batch_graceful_timeout_keeps_later_target_running`
at `src/process_control/tests/batch.rs:3323`, `fixture readiness timed out`.
Immediately preceding serialized Windows and complete `process_control` runs had
passed **6/0/1** in 16.25s and **150/0/2** in 22.38s respectively.

The same initial revision also exposed nine new `process_control` Clippy sites:
one production `clone_on_copy`, seven Stage 2B batch-test diagnostics, and the
Stage 2B-expanded `FakeMutation` `large_enum_variant`. They were not relabeled as
inherited. The scoped correction at `7995c34` removed those diagnostics and made
fixture mode assignment precede snapshot correlation, so unrelated parallel
snapshot work cannot consume the fixture's five-second mode deadline. A source
ordering regression was added. The final default parallel suite and both final
Clippy captures above prove the correction; there are now zero
`process_control` diagnostics.

The independent review then found that `PendingChild` ownership could be
disarmed before cleanup-HANDLE registration succeeded. Revision `ec15356` keeps
the pending guard armed through correlation, exact HANDLE open, creation-time
validation, and controller registration, and adds a regression that injects
failure before registration and observes bounded child cleanup. The final
Windows **8/0/1**, default **1126/0/4**, and complete `process_control`
**152/0/2** results above include that correction.

## Deterministic Fake Evidence

- Catalog, tree, same-name, and execution focused suites pass **43 tests** in
  aggregate. The complete `process_control` suite covers the same contracts with
  all Stage 1 and Stage 2A admission/operation behavior.
- Tree and same-name suites accept exactly 256 reliable targets and reject 257
  with `TargetSetTooLarge`, no plan, and no operation calls.
- Fake monotonic-clock execution covers expiry before admission, expiry after a
  successful admission with exact close, an admission failure that crosses the
  deadline, unattempted ordered tails, and a target admitted before the batch
  deadline receiving the full Stage 2A five-second operation budget.
- Frozen tree and same-name plans exclude later candidates and retain exact
  identities despite later name/parent changes. Tree ordering is deepest first;
  same-name ordering is PID then creation time.
- Best-effort execution continues after target failure and returns every ordered
  outcome. Omissions or any failed/not-attempted result produce `Partial`;
  successful complete selection produces `Complete`.
- Normal, error, deadline, and unwind tests assert exact one-time HANDLE closure.
  Tests and source guards prove no rollback, retry, automatic force fallback, or
  elevation attempt.

## Real Windows Standard-User Evidence

The serialized Windows suite uses only explicitly spawned, test-owned fixtures
identified by handshake PID and creation time. Each controller retains an exact
cleanup HANDLE. Same-name tests copy the test executable to a unique temporary
name and abort before effects if any reliable same-name process is not owned.

- Graceful and forced three-node tree cases passed and report descendants before
  the root.
- Graceful and forced same-name cases passed across distinct paths. Each freezes
  two owned targets, starts a third newcomer after planning, proves that newcomer
  survives and is absent from the report, then cleans it through the retained
  HANDLE.
- The mixed graceful case reports one ignored-close timeout, continues to close
  the other fixture, preserves complete ordered counters, and returns `Partial`.
- Fixture setup/readiness/correlation/cleanup operations are individually
  bounded. In the production protocol's worst graceful path, the 30-second batch
  scheduling budget composes with the inherited five-second single-target
  operation budget, approximately 35 seconds; this is a bounded protocol budget,
  not an unbounded wait guarantee.
- Normal assertions and `Drop` cleanup cover success, failure, and unwind.
  Post-run `%TEMP%\\uipilot-batch-*` directory count was **0** (exit 0, wall
  0.602s), proving no copied fixture executable/control directory residue. All
  retained fixture process waits completed and cleanup HANDLE owners dropped.
- Standard-user access/error classification and stable redacted strings are
  covered. Real PPL is **N/A** because no safe owned PPL fixture exists.

## Logging And Data Handling

Source guards and stable-display tests confirm ordinary output is limited to
stable codes, scope/operation, fixture labels, and aggregate counts. Production
code adds no PID, creation time, name, path, parent PID, tree edge, HANDLE, HWND,
window-title, raw Win32-code, or command-line logging. Preview names/paths remain
in memory; unreliable hints do not enter previews/reports/logs. Stage 2B adds no
disk history or telemetry.

## Dependency And Changed-File Scope

`git diff --name-status 6d44729...ec15356` contains exactly:

```text
A docs/superpowers/plans/2026-09-07-process-target-expansion-batch-orchestration.md
A src-tauri/src/process_control/batch.rs
M src-tauri/src/process_control/core.rs
M src-tauri/src/process_control/mod.rs
M src-tauri/src/process_control/tests.rs
A src-tauri/src/process_control/tests/batch.rs
```

Cargo files are unchanged. A separate scoped diff returned exit 0 for
`process_control/operation.rs`, `process_control/windows.rs`, Tauri wiring
(`src-tauri/src/lib.rs` and `main.rs`), frontend `src/`, `package.json`, and
`docs/plugin-sdk`. Therefore public plugin contracts, persistence, Restart,
elevation, macOS platform implementation, and unrelated sources are unchanged.
The changed `process_control/mod.rs` adds the specified non-Windows facade stubs
that return `PlatformUnsupported`; it does not add a non-Windows adapter or
side-effect implementation. Stage 2B remains a crate-private process-control
module expansion.

## Known Limits And Residual Risks

- Windows parent PID is a same-snapshot relationship hint, not stable object
  identity. Creation-time ordering, frozen exact targets, and per-target final
  admission narrow but do not eliminate that limitation.
- `IncompleteSelection` means reliable targets may still be confirmed and run;
  it does not claim every actual descendant or same-name process was found.
- Names and parent relationships may change after confirmation. Execution still
  uses only the frozen exact identity set.
- Serial graceful work may exhaust the 30-second batch scheduling deadline and
  leave explicitly reported targets unattempted. There is no cancellation,
  background recovery, cross-call retry, auto-force, or elevation.
- Stage 2A's inherited HWND/PID residual race remains: HWND ownership can change
  after final facts refresh and before posting. Force completion is also
  asynchronous: a successful `TerminateProcess` call may remain pending or have
  unknown completion at the five-second observation boundary.
- Non-Windows facade behavior has source-contract coverage only; no non-Windows
  build or runtime test was run on this Windows host.
- Task 3's minor mirrored-validation risk remains: `batch.rs`
  `same_name_hint_is_comparable` mirrors private `core.rs` `valid_name`. They are
  equivalent now, but a future change to one could drift from the other.

## Exit Gate And Stop

The implementation, plan, deterministic/Windows evidence, cleanup, scope, and
zero-new-diagnostic requirements are satisfied. The concise independent final
targeted review approved the correction and record with no remaining Critical or
Important finding. User review and explicit acceptance remain the only open exit
item; the documented mirrored-validation Minor remains nonblocking.

**Stop:** Stage 2B remains **AWAITING USER ACCEPTANCE**. This record does not
accept Stage 2B and does not authorize Restart, elevation, host product-entry,
Stage 3, or any further design or implementation.
