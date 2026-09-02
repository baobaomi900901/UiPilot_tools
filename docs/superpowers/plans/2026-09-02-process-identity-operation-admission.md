# Stage 1 Process Identity And Operation Admission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Checkboxes are the execution record.

**Date:** 2026-09-02
**Status:** Approved — accepted by the user on 2026-09-02

**Goal:** Deliver the host-internal, read-only process catalog and identity/protection admission lease defined by Stage 1, with deterministic fake-adapter contracts and read-only Windows 11 verification.

**Architecture:** Add one deep `process_control` module with a small crate-private facade and a private generic core. `core.rs` owns ordering, identity, omission, error, and admission semantics; `windows.rs` owns Win32 calls and HANDLE RAII; `tests.rs` owns the deterministic fake adapter. Stage 1 does not register Tauri state or commands and does not perform process operations.

**Tech Stack:** Rust `1.96.0`, edition 2021, `windows 0.61.3`, Windows 11 x64, the existing Cargo dependency graph.

**Approved design:** [`Stage 1 Process Identity And Operation Admission Design`](../specs/2026-09-02-process-identity-operation-admission-design.md)

## Global Constraints

- Scope begins at approved-design commit `890fdbd`; preserve unrelated user changes.
- `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` remain byte-identical: all required Windows features already exist.
- No Tauri command, managed state, capability file, frontend, public plugin contract, SDK, schema, CLI, termination, restart, elevation, confirmation UI, CPU, memory, search, or macOS Adapter.
- `ProcessTarget` identity is exactly `hostInstanceId + pid + creationTime`; name, path, and observed parent PID are metadata only.
- Stage 1 observation access is exactly `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`; never request `PROCESS_ALL_ACCESS`, `SeDebugPrivilege`, or an operation right.
- Liveness uses `WaitForSingleObject(handle, 0)`, never `GetExitCodeProcess == STILL_ACTIVE` as the deciding signal.
- UiPilot self, critical, Protected Process/PPL, and unreadable protection state fail closed.
- Ordinary logs and public error display never contain executable paths, raw Win32 codes, command lines, user data, or HANDLE values.
- `ProcessAdmission` is non-serializable, non-cloneable, single-consumption, and never leaves `process_control` with a raw HANDLE.
- Stage 2 owns operation algorithms, operation HANDLE access, elevation, operation errors, side effects, and the final operation handoff described by design section `7.5 Stage 2 Handoff Contract`.
- Windows test helpers may terminate only the exact child process created and retained by that test; product code has no process side effect.

## Core Contract Overview

The implementation uses the approved design terminology and these exact Rust names:

- Facade: `ProcessAuthority::new() -> Result<Self, ProcessAuthorityInitError>`, `ProcessAuthority::snapshot()`, and `ProcessAuthority::admit(&ProcessTarget, ProcessIntent)`.
- Values: `HostInstanceId([u8; 16])`, `ProcessTarget`, `ProcessRecord`, `ProcessOmissions`, `ProcessSnapshot`, `ProcessIntent`, `ProcessError`, and construction-only `ProcessAuthorityInitError::Unavailable`.
- Opaque lease: crate-private `ProcessAdmission`; its platform HANDLE field stays private to `process_control`.
- Private core: `CoreAuthority<A: ProcessPlatformAdapter>` and `CoreAdmission<H>`.
- Private adapter values: `ProcessCandidate`, `ProcessObservation`, `ProcessProtection`, `Liveness`, `ObservationFailure`, and `EnumerationFailure`.
- `ProcessIntent` has exactly `Terminate`, `ForceTerminate`, and `Restart` and only binds the lease; it does not select Stage 1 Windows rights.
- `ProcessError` has exactly `CatalogUnavailable`, `InvalidTarget`, `ExpiredTarget`, `TargetExited`, `TargetChanged`, `InspectionDenied`, `ProtectedTarget`, and `PlatformUnsupported`.

`ProcessPlatformAdapter` exposes only the primitives the core needs to enforce ordering: enumerate candidates, open one observation HANDLE, inspect creation time and metadata, perform zero-time liveness, and inspect critical/protection state. Its associated HANDLE type owns RAII cleanup. The core, not the Adapter, selects error precedence and omission categories.

The production facade wraps `CoreAuthority<WindowsProcessAdapter>`. Tests instantiate `CoreAuthority<FakeProcessAdapter>` directly. A non-Windows facade constructs without an Adapter and returns `ProcessError::PlatformUnsupported` from `snapshot()`/`admit()`.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4`; do not parallelize tasks that edit the shared core.
- Every task follows focused TDD: establish the intended failing test, implement the minimum contract, rerun the focused command, then commit one atomic change.
- Every task receives specification-compliance and code-quality review before its dependent task begins when the execution workflow provides those gates.
- Review fixes use separate commits and do not absorb another task or pre-existing user changes.
- Focused Rust commands use `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline <filter> -- --nocapture`.
- The full test, Clippy, format, manual, and scope gates run only once in Task 4.

---

### Task 1: Core Identity And Partial Process Snapshot

**Files:** Create `src-tauri/src/process_control/mod.rs`, `src-tauri/src/process_control/core.rs`, `src-tauri/src/process_control/tests.rs`; modify `src-tauri/src/lib.rs`.

**Dependencies:** Approved design sections `3 Decisions`, `4 Module And Seam`, `5 Domain Contract`, `6 Snapshot Flow`, `9 Stable Error Contract`, and `10 Concurrency And Security Invariants`.

- [ ] Declare `process_control` beside the other host modules under the existing production/test cfg. Limit the intentional Stage 1 dead-code exception to this one module declaration with a reason stating that Stage 2 supplies the first production caller; do not construct or manage the module merely to silence lints.
- [ ] Define the value types, private fields, strict constructors, omission counters, stable error enum/display strings, deterministic record ordering, and the private generic Adapter/core contract from `Core Contract Overview`.
- [ ] Implement `CoreAuthority::snapshot()` so global snapshot creation/traversal failure returns `CatalogUnavailable`, while each candidate contributes either one reliable record or exactly one omission in the design's priority order.
- [ ] Keep a candidate record only after the HANDLE-derived creation time, liveness, and display metadata agree. Path-unavailable fallback must re-enumerate the name and then recheck the original HANDLE's liveness and creation time before publishing.
- [ ] Build `FakeProcessAdapter` with scripted call outcomes, requested-access capture, open/drop counters, and mutation hooks between calls; it must not embed core error precedence inside the fake.
- [ ] Add a `lib.rs` source guard proving the module has the single scoped dead-code exception and has no `.manage(...)`, invoke handler, command, build capability, or frontend wiring.

**Distinct test coverage:** Successful deterministic PID ordering; duplicate/changed candidates; optional path fallback; each omission category is mutually exclusive; per-entry failure preserves other records; mid-enumeration/global failure publishes no partial snapshot; refresh preserves an unchanged target; metadata never participates in target equality; every temporary fake HANDLE closes exactly once; no Tauri wiring exists.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::snapshot -- --nocapture`

### Task 2: Identity And Protection Admission Lease

**Files:** Modify `src-tauri/src/process_control/mod.rs`, `src-tauri/src/process_control/core.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Task 1; approved design sections `7 Stage 1 Admission Lease`, `8 Protection Policy`, `9 Stable Error Contract`, and `10 Concurrency And Security Invariants`.

- [ ] Add `ProcessIntent` and private generic `CoreAdmission<H>` without exposing raw HANDLE, PID reopening, serialization, cloning, or operation access. Task 3 alone wraps the Windows specialization in the concrete crate-private `ProcessAdmission`.
- [ ] Implement `CoreAuthority::admit()` in the exact design order: target structure, host instance, observation open, creation time, zero-time liveness, protection readability, self identity, protection policy.
- [ ] Bind the accepted intent and original target to the lease; all three intents request the same observation access and no operation right.
- [ ] Map missing/exited targets, changed creation time, observation denial, liveness inspection failure, self, critical, PPL, and unreadable protection to the approved stable error. Close every temporary HANDLE on failure and close the retained HANDLE exactly once on lease Drop.
- [ ] Keep error priority in the core so a changed PID returns `TargetChanged` before inspecting the replacement process's protection state, and an expired host instance performs no platform open.

**Distinct test coverage:** Invalid target; host restart/instance mismatch with zero Adapter calls; PID reuse; exit before/after open; `WAIT_TIMEOUT`, `WAIT_OBJECT_0`, `WAIT_FAILED`, and unexpected wait values; self, critical, PPL, and protection-query failure; simultaneous failure priority; all intents use only observation access; lease is consumed once; drop and all failure paths close exact handles; no side-effect Adapter exists or is called.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::admission -- --nocapture`

### Task 3: Windows 11 Adapter And Read-Only Smoke Tests

**Files:** Create `src-tauri/src/process_control/windows.rs`; modify `src-tauri/src/process_control/mod.rs`, `src-tauri/src/process_control/tests.rs`.

**Dependencies:** Tasks 1-2; approved design sections `5.1 Host Instance`, `5.3 Process Record`, `7.2 Admission Order`, `7.3 Least Access`, `8 Protection Policy`, `11 Windows Adapter`, and `12 Verification`.

- [ ] Implement `WindowsProcessAdapter` and `OwnedProcessHandle` using existing `windows 0.61.3` features: Tool Help enumeration, `OpenProcess`, `QueryFullProcessImageNameW`, `GetProcessTimes`, `WaitForSingleObject`, `IsProcessCritical`, and `GetProcessInformation(ProcessProtectionLevelInfo)`.
- [ ] Open every catalog/admission HANDLE with exactly `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, non-inheritable. Map `WAIT_TIMEOUT` to alive, `WAIT_OBJECT_0` to exited, and `WAIT_FAILED`/unexpected results to inspection failure.
- [ ] Generate `HostInstanceId` with `BCryptGenRandom(None, ..., BCRYPT_USE_SYSTEM_PREFERRED_RNG)`; any non-success `NTSTATUS` returns redacted `ProcessAuthorityInitError::Unavailable`. Isolate the fill call behind private `generate_host_instance_with` so tests can inject success/failure without replacing the Windows Adapter. Do not add `rand`, `getrandom`, UUID, or another dependency.
- [ ] Canonicalize HANDLE-derived `FILETIME` to `u64`; preserve paths as `PathBuf`/UTF-16-derived Windows values until the crate-private record conversion; compare basenames with ordinal case-insensitive Windows semantics.
- [ ] Implement the Windows facade and concrete opaque `ProcessAdmission` around `CoreAdmission<OwnedProcessHandle>`. Add a non-Windows facade whose constructors/operations return `PlatformUnsupported` without defining a usable admission HANDLE. The Windows facade stores current host `pid + creationTime` as the exact self identity and delegates ordering to the core.
- [ ] Add real Windows tests that find the current test process, reject it as `ProtectedTarget`, admit a retained child while alive, then observe `TargetExited` after that same child exits. Spawn the child from the current Rust test executable through an ignored fixture test, not PowerShell or a product helper binary.
- [ ] Make the fixture exit with numeric code 259 after a bounded delay and retain the parent `Child` HANDLE through the stale-target check, proving `WaitForSingleObject` does not confuse it with `STILL_ACTIVE`. Cleanup is bounded to that exact child.
- [ ] Verify current-process critical/PPL queries return readable, unprotected results; classify unavailable protected-target inspection without enabling `SeDebugPrivilege`.

**Distinct test coverage:** CNG generation success/failure seam; Tool Help end versus traversal failure; path/name reconciliation; FILETIME conversion; exact access mask and non-inheritance; current-process self denial; ordinary child lease; code-259 child exit; Windows HANDLE drop counts; no PowerShell/WMI/Shell/undocumented NT fallback.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::windows -- --nocapture --test-threads=1`

### Task 4: Stage 1 Verification Record And Exit Evidence

**Files:** Create `docs/superpowers/verification/2026-09-02-process-identity-operation-admission.md`.

**Dependencies:** Tasks 1-3; approved design sections `12 Verification` and `13 Exit Gate`.

- [ ] Run the focused process-control suite and record test counts plus the exact Windows OS build, architecture, elevation state, and Rust toolchain.
- [ ] Run the full default/all-feature test, format, and Clippy gates below and record commands, exit codes, and any environment-only limitations.
- [ ] In a standard-user Windows 11 session, record current-process self denial, live helper admission, code-259 helper exit, stale-target rejection, omission counts, helper cleanup, and ordinary-log redaction. If no inspectable protected target exists, record that item as environment N/A while retaining the mandatory Fake Adapter contract result.
- [ ] Record the final changed-file list and confirm no Tauri command/state/capability/frontend/public plugin contract, process side effect, dependency, or lockfile change.
- [ ] Stop at the Stage 1 manual-acceptance gate. Do not mark Stage 1 accepted or begin Stage 2 without the user's explicit acceptance of the evidence.

**Distinct test coverage:** This task owns aggregate regression and manual evidence only; it does not add product behavior.

**Verify:** Run all commands from the repository root:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control -- --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
git diff --exit-code 890fdbd...HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock
Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture
[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
rustc --version
cargo --version
```

Expected: every Cargo command exits 0; the dependency diff is empty; the verification document contains the exact environment and results; no helper process or HANDLE remains.

## Final Acceptance Checklist

- [ ] Design section `12.1 Fake Adapter Contract Tests` is covered by Tasks 1-2.
- [ ] Design section `12.2 Windows Tests` is covered by Task 3.
- [ ] Design section `12.3 Manual Acceptance` and every item in `13 Exit Gate` are recorded by Task 4.
- [ ] Stage 1 contains no Stage 2 operation HANDLE, side effect, error name, elevation path, UI, Runtime, or public plugin contract.
- [ ] The user has reviewed the verification record and explicitly accepted Stage 1 before any Stage 2 design or implementation begins.
