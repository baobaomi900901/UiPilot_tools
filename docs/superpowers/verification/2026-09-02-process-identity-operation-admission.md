# Stage 1 Process Identity And Operation Admission Verification Record

**Date:** 2026-09-03
**Status:** ACCEPTED WITH RECORDED DEVIATIONS（已验收，保留已记录偏差）
**Verification outcome:** `DONE_WITH_CONCERNS` — accepted by the user on 2026-09-03; focused and default tests pass, while the all-feature test gate and both Clippy gates remain red as accepted deviations.
**Scope:** Stage 1 aggregate regression, Windows read-only/admission evidence, changed-file audit, and exit-gate evidence only. This record adds no product behavior.

## Revision Under Verification

- Branch: `codex/plugin-platform-roadmap`
- Approved-design base: `890fdbdd5b38747852109a184d66e3c4e0238a74`
- Task 4 refresh execution-start HEAD: `33c32fce9107df157641ecce2d7ce71236905708`
- Final implementation/source HEAD under test: `33c32fce9107df157641ecce2d7ce71236905708`
- The earlier documentation commits remain in history. This refresh is a new documentation-only commit after the tested source revision and does not alter that source.

## Environment

All commands and the aggregate snapshot probe ran from the worktree root in the same standard-user session.

| Item | Fresh result |
|---|---|
| OS | Microsoft Windows 11 Pro for Workstations (`Microsoft Windows 11 专业工作站版`) |
| Version / build | `10.0.26100` / `26100` |
| Architecture | 64-bit (`x64`) |
| Administrator/elevated | `False`; standard-user session |
| rustc | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| cargo | `cargo 1.96.1 (356927216 2026-06-26)` |

The approved plan names Rust 1.96.0, while this verification host provides 1.96.1. This toolchain drift is recorded as a limitation; no source was changed to accommodate it.

## Required Command Results

Every command below was freshly run from the worktree root at source HEAD `33c32fc`. A failed command was rerun once with the same arguments to distinguish a transient failure; all three failing gates reproduced. The default full-test command passed on its first attempt, so there was no transient default-test failure.

| Command | Exit | Result |
|---|---:|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 0 | Clean. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1` | 0 | Main test target: **53 passed, 0 failed, 1 ignored, 976 filtered**. This includes **19 passed Windows tests plus 1 ignored fixture**. Other unit targets ran 0 tests; the integration target had 1 filtered test. The ignored item is the child fixture, which the live-child tests spawn explicitly. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline` | 0 | Passed on the first attempt with no transient failure. Main library: **1027 passed, 0 failed, 3 ignored, 0 filtered**; integration: **1 passed, 0 failed**; remaining unit/doc targets: 0 tests. |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features` | 101 | Reproduced on the required retry before tests ran: the schema-generator binary cannot resolve its schema function when `test-instrumentation` is enabled because that function is cfg-excluded. Source correction is outside Task 4. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` | 101 | Reproduced on retry. Clippy reported **12 library** and **13 library-test** deny-warning diagnostics at 13 distinct sites, all outside `process_control`; the test-only thirteenth site is in `clipboard_history`. There were **0 diagnostics in `process_control` paths** after the Task 3 lint cleanup. No unrelated fix was made. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings` | 101 | Reproduced on retry. The shared library subset is the same **12** unrelated diagnostics and the library-test target reports the same **13** sites; there are **0 diagnostics in `process_control` paths**. The schema `E0425` is additionally emitted for both the schema-generator binary and its binary-test target. No clean Clippy result exists for this revision/toolchain. |
| `git diff --exit-code 890fdbd...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock` | 0 | Empty dependency diff. |
| `Get-CimInstance Win32_OperatingSystem \| Select-Object Caption, Version, BuildNumber, OSArchitecture` | 0 | Environment values recorded above. |
| `[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)` | 0 | `False`. |
| `rustc --version` | 0 | Version recorded above. |
| `cargo --version` | 0 | Version recorded above. |

The required expectation that every Cargo command exit 0 is therefore **not met**. The user explicitly accepted this Stage 1 deviation on 2026-09-03; the repository-wide gates remain unresolved and must not be represented as green.

The exact remaining Clippy sites are:

- library and library-test: `src/commands.rs:1290` (`too_many_arguments`), `src/clipboard_history/observer.rs:381` (`manual_div_ceil`), `src/clipboard_history/paste.rs:139` (`forget_non_drop`), `src/clipboard_history/store.rs:149` (`redundant_closure`), `src/clipboard_history/store.rs:337` (`match_single_binding`), `src/public_plugins/network/broker.rs:422`, `:433`, `:464`, `:465`, and `:605` (`needless_as_bytes`), `src/plugin_panel.rs:1130` (`nonminimal_bool`), and `src/lib.rs:229` (`collapsible_match`): **12 sites**;
- library-test only: `src/clipboard_history/mod.rs:783` (`needless_borrows_for_generic_args`): **1 additional site**, for **13** total;
- all-features schema compile failure: `src/bin/generate_public_plugin_schema.rs:11` (`E0425`) for the binary and binary-test targets.

Thus the default gate's exact counts are 12 library / 13 library-test diagnostics. The all-features gate emits the same 12-site shared library subset and 13-site library-test set plus the two schema-target `E0425` diagnostics. Both commands exit 101, and neither emits a `process_control` diagnostic.

## Fake Adapter Contract Evidence

The focused process-control command passed all **18 snapshot** and **15 admission** Fake/core contract tests.

Snapshot coverage includes deterministic ordering, zero-omission success, duplicate/PID-change behavior, invalid identity, path fallback with same-handle revalidation, single-entry failure with partial catalog success, global and mid-enumeration failure without partial publication, refresh stability, metadata-independent identity, stable redacted error display, no Tauri/frontend wiring, mutation hooks, and exact temporary-handle closure. The deterministic omission-priority case asserts exactly:

- `exitedOrChanged = 1`
- `inspectionDenied = 1`
- `identityUnavailable = 1`
- `total = 3`
- opened handles `3`, dropped handles `3`

Additional exact omission examples passed: reliable success `0/0/0`; changed fallback `1/0/0`; denied entry `0/1/0`; two PID-zero entries `0/0/2`; valid/invalid duplicate ordering preserves the required `exitedOrChanged > inspectionDenied > identityUnavailable` priority.

Admission coverage includes invalid and expired targets before Adapter access, PID reuse as `TargetChanged`, exit before/after open, observation and creation-time inspection failures, zero creation time, exact liveness mapping, self/critical/PPL/unreadable protection, error priority, all three intents, retained lease ownership, and single consumption. Every intent requested only snapshot observation access; failure and Drop assertions showed exact open/drop balance and no side-effect Adapter.

The deterministic Design 12.1 retained-object regression adds the missing post-admission PID-reuse proof. Successful `Restart` admission opens one Fake handle pinned to the original process object's stable object ID and retains the original `ProcessTarget` and intent. Replacing the PID mapping creates a distinct object ID without retargeting that handle. A separately opened replacement handle resolves creation time `10001`, closes first against only the replacement object, and leaves the original open. Dropping the admission then closes the original handle exactly once against its original object ID. The exact trace contains only original open/creation/liveness/protection plus replacement open/creation; the source guard confirms there is no side-effect Adapter or side-effect call.

## Windows Standard-User Evidence

The focused command passed **19 Windows tests**, with the dedicated fixture ignored in the parent harness and invoked only as an exact child by the live tests.

- **Live snapshot and omissions:** because the earlier numeric snapshot was stale, a temporary read-only aggregate probe compiled the tracked `process_control` module directly. Its five payload lines emitted counts only, and its source and matching build outputs were deleted immediately. The fresh snapshot returned **310 process records** with omissions `exitedOrChanged = 0`, `inspectionDenied = 125`, `identityUnavailable = 1`, `total = 126`. These are a point-in-time observation and may vary with the host process set.
- **Current self:** the snapshot found the exact current test-process identity; `Terminate` admission returned `ProtectedTarget`. Its critical/PPL queries were readable and reported unprotected, so the denial is the exact-self policy rather than a name rule.
- **Modified real creation time:** the new real-Windows facade test retained the current process's real host-instance ID and PID, changed only its nonzero creation time, and admission returned exactly `TargetChanged`. It exercised `WindowsProcessAdapter` `OpenProcess` / `GetProcessTimes`; no Fake Adapter was used.
- **Live exact child admission:** a child created from the current Rust test executable entered the catalog and received a read-only `Restart` admission while the test retained that exact `Child`; dropping the admission and bounded child cleanup succeeded.
- **Numeric 259 and stale identity:** another retained exact child first received a read-only `ForceTerminate` admission, exited naturally with numeric code **259**, and the old target then returned `TargetExited`. This proves zero-time HANDLE signaling, not `STILL_ACTIVE`, decides liveness.
- **Unexpected-wait telemetry:** the defensive classifier maps both `WAIT_FAILED` and an unexpected `WAIT_EVENT(17)` to the inspection-failure representation `AccessDenied`. The snapshot regression produced no record, `inspectionDenied = 1`, `identityUnavailable = 0`, `total = 1`, and one exact temporary-handle close. The named mappings remain unchanged: `WAIT_TIMEOUT` is alive and `WAIT_OBJECT_0` is exited; admission continues to map liveness inspection failures to `InspectionDenied`.
- **Context-specific path-query fallback:** the production path-query classifier mapped `QueryFullProcessImageNameW` access denial to path-unavailable fallback. The core re-enumerated the name, compared it with Windows ordinal case-insensitive semantics, then rechecked liveness and creation time on the same retained HANDLE before publishing one pathless record with zero omissions, one open, and one close. The companion regression retained invalid-parameter/not-found as exited-or-changed and retained generic `OpenProcess`/admission access denial as `AccessDenied`, so those paths remain fail closed.
- **Protected target:** no inspectable real Protected Process/PPL was present. This item is explicitly **environment N/A**. Deterministic Fake Adapter coverage for critical, PPL, combined protection, and unreadable protection passed and remains mandatory evidence.
- **Helper cleanup:** after all commands, the count of matching fixture helper processes was **0**. Temporary aggregate-probe source residuals were **0**, and matching build-output residuals were **0**.
- **HANDLE/RAII:** the real private `OwnedHandle` Drop test observed closer calls `0` before Drop and exactly `1` after Drop. Production process and snapshot handles use that single-owner RAII path. Fake success/failure tests additionally assert exact opened/dropped counts.
- **Least access:** Windows open-seam evidence passed for exactly `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, non-inheritable. No operation handle or operation access was opened.
- **Ordinary error/log redaction:** the stable error-display test passed all eight approved Stage 1 codes; CNG initialization failure mapped only to `Unavailable`. A source scan found no ordinary log-emission macro in production `process_control`. No executable path, command line, user data, raw HANDLE, or raw platform error is emitted to ordinary callers/logs.

No test invokes product termination or restart. Fixture cleanup is limited to the exact retained child created by that test.

## Changed Files And Boundary Audit

Relative to approved-design base `890fdbd`, the final intended tracked file list is:

- `docs/superpowers/plans/2026-09-02-process-identity-operation-admission.md` — added approved plan
- `docs/superpowers/verification/2026-09-02-process-identity-operation-admission.md` — this record
- `src-tauri/src/lib.rs` — scoped module declaration and source guards only
- `src-tauri/src/process_control/core.rs` — added
- `src-tauri/src/process_control/mod.rs` — added
- `src-tauri/src/process_control/tests.rs` — added
- `src-tauri/src/process_control/windows.rs` — added

`src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` are byte-identical to the approved-design base. Their Git blob IDs are respectively `36aba5680ba2f2eb5aceda973a465293ccb4fb3e` and `e19e3b0c1c6bfcf2ca7f1fc2a8d6826ed5e00346` at both the base and verified worktree.

The changed-file audit plus focused source guards confirm there is no new Tauri command, managed state, capability, frontend, Runtime entry, public plugin contract, Public Plugin API v1, host version, SDK, schema, CLI, UI, process-tree/search feature, or macOS Adapter. Stage 1 adds no product process side effect, termination, restart, elevation, confirmation flow, operation HANDLE/access, raw-HANDLE exposure, Stage 2 operation error, or dependency/lockfile change.

## Design 12 / 13 Exit Checklist

| Requirement | Evidence | Result |
|---|---|---|
| 12.1 Fake Adapter contract | 18 snapshot + 15 admission tests passed; identity, omissions, fixed precedence, protection, access, liveness, redaction, no-side-effect, and handle ownership covered with exact counts above. The retained-object regression proves PID remapping cannot retarget an admitted Fake handle and that original/replacement objects close separately and exactly once. | PASS |
| 12.2 Windows tests | 19 Windows tests passed; the real modified-creation-time case returned `TargetChanged` through the Windows facade, and current self, exact child admission, code-259 exit / `TargetExited`, unexpected-wait inspection telemetry, context-specific path-query fallback with same-HANDLE revalidation, critical/PPL readability, access mask, Tool Help traversal, ordinal names, CNG, and RAII are covered. Real protected target is environment N/A as permitted. | PASS with allowed environment N/A |
| 12.3 Manual acceptance evidence | Standard-user OS/toolchain/elevation, live aggregate snapshot/omissions, self denial, live helper, stale target, redaction, and zero residual helper/probe counts are recorded above. | EVIDENCE RECORDED; USER ACCEPTED WITH RECORDED DEVIATIONS |
| 13: design and independent plan approved | Both tracked documents state user-approved status. | PASS |
| 13: catalog/admit Fake contracts | Focused Fake/core results above, including stable-object identity retention across PID remapping and separate exact-once original/replacement handle cleanup. | PASS |
| 13: Windows 11 read-only/admission validation | Focused Windows results and standard-user evidence above. | PASS with protected-target environment N/A |
| 13: deterministic PID reuse, exit, self, protection | Fake PID-reuse and real-Windows modified-creation-time cases returned `TargetChanged`; the live code-259 child returned `TargetExited`; exact-self `ProtectedTarget` and Fake protected/unreadable cases passed. | PASS |
| 13: failure paths have no process side effect or HANDLE leak | Fake exact counters, real RAII exact-close test, source guards, exact-child cleanup, and residual helper count 0. | PASS |
| 13: no prohibited product/API surface | Changed-file audit and source guards above. | PASS |
| 13: changes and limitations recorded | File list, toolchain drift, protected-target N/A, and failed aggregate gates are recorded here. | PASS |
| Task 4 aggregate Cargo gate | Default/focused tests and fmt pass; all-feature test and both Clippy commands reproducibly fail. | **FAIL — exit gate not all green** |
| User acceptance before Stage 2 | The user reviewed this record and selected acceptance option 1 on 2026-09-03, explicitly accepting the disclosed gate failures, toolchain drift, and permitted environment N/A. | **ACCEPTED WITH RECORDED DEVIATIONS** |

## Accepted Deviations And Stage Boundary

1. The required all-feature test command fails during schema-generator compilation under the all-feature cfg combination.
2. Both required Clippy commands fail with `-D warnings`. Task 3 removed the two Stage-1-owned test lints, leaving zero diagnostics in `process_control`; the remaining 12 library / 13 library-test diagnostics are in unrelated host areas. Fixing them requires source edits outside this evidence-only task.
3. The host uses Rust/Cargo 1.96.1 rather than the plan's stated 1.96.0.
4. A real inspectable protected target was unavailable; this is environment N/A, with deterministic Fake coverage retained.

The user explicitly accepted Stage 1 and the disclosed deviations on 2026-09-03. Stage 1 is therefore **ACCEPTED WITH RECORDED DEVIATIONS**; the aggregate gate remains factually red and the unresolved repository-wide failures remain follow-up work.

This acceptance closes Stage 1 only. Stage 2 design, planning, and implementation have **not** started and require a separate explicit user instruction under the roadmap's step-by-step acceptance process.
