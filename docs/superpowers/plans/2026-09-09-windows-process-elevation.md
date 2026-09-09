# Stage 2D Windows Process Elevation Implementation Plan

**Date:** 2026-09-09
**Status:** APPROVED — accepted by the user on 2026-09-09. User also approved separately filling Issue 2 coverage first; Stage 2D implementation has not started.
**Approved design:** [Windows Process Elevation](../specs/2026-09-09-windows-process-elevation-design.md), accepted by user continuation after `bf38290`.

Deliver one same-account, one-shot privileged stop workflow. The ordinary host
coordinates launch; a minimal native helper authenticates its peer, makes a fresh
plan, owns confirmation and reuses existing process admission and stop logic.
This is one dependent workflow, not separate platform subsystems. Task boundaries
below are review checkpoints within Stage 2D, not permission to advance stages.

## Technology And Constraints

Use Rust 2021 with the repository's Rust 1.96 floor, windows 0.61.3 and existing
serde/serde_json/sha2 versions where needed. Preserve locks; add only required
Win32 features, recording their purpose. No Tauri/WebView dependency in the helper.
Design sections 2–8 are the canonical contracts, including scopes, token rules,
timeouts, 8 MiB framing, confirmation ownership and uncertain results.

No elevated Restart, SeDebugPrivilege, services, public plugin API, launcher
entry, settings, Stage 3, unrelated lint repairs or Issue 1/2 fixes. Issue 1 remains
open. Before implementation approval, user must explicitly choose whether to
repair Issue 2 separately or retain its gap with justification. Recommendation:
repair it as a separately authorized follow-up; this plan does not authorize it.
Disposition update: the user subsequently approved that separate coverage follow-up.

Installation/UAC and native interactive tests change the user's system or
foreground experience. Prepare exact binaries, destination, file list, ACLs and
cleanup instructions first; obtain explicit approval for that concrete experiment
before installation or UAC. No automated consent/input synthesis or CI bypass.
Only test-owned targets may be stopped; keep exact cleanup handles.

## File And Contract Map

Paths are relative to the worktree root; entries below are proposed new files
unless identified as existing. Do not create all scaffolding upfront.

- `spikes/process-elevation-feasibility/{Cargo.toml,Cargo.lock,src/main.rs}`:
  isolated, non-destructive launch/pipe/native-window probe with no stop API.
- `scripts/test-process-elevation-feasibility.ps1`: explicit experiment modes,
  deployment inventory and bounded cleanup of the approved owned destination.
- `src-tauri/src/process_elevation/{mod.rs,protocol.rs,session.rs,trust.rs,pipe.rs,launch.rs}`:
  private coordinator and shared protocol/trust/lifecycle mechanisms.
- `src-tauri/process-elevation-helper/{Cargo.toml,Cargo.lock,src/main.rs,src/confirmation.rs}`:
  separate minimal Cargo package and native entry/confirmation, not uipilot_lib.
- `src-tauri/src/process_control/elevation.rs` and
  `src-tauri/src/process_control/tests/elevation.rs`: exact-selector conversion,
  privileged plan ownership, stop composition and Fake assertions.
- Existing `src-tauri/src/process_control/{mod.rs,core.rs,windows.rs,tests.rs}`:
  narrow shared-source declarations, additional exact protected host identity,
  and cancellable planning observation seam; existing defaults stay unchanged.
- Existing `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`:
  private module declaration and required dependencies/features only.
- `scripts/test-process-elevation.ps1`: installed owned-fixture end-to-end runner.
- `docs/superpowers/verification/2026-09-09-process-elevation-feasibility.md`
  and `2026-09-09-windows-process-elevation.md`: gate and final evidence.

Helper `main.rs` uses explicit Rust path-module declarations to compile the same
`process_control/mod.rs` and `process_elevation/mod.rs` sources under those crate-root
names; no algorithm copy or public API extraction. Dependency-free coordinator code
compiles in both packages; do not add an `elevation-helper` feature or a cfg switch
that changes host behavior under `--all-features`. Helper entry calls only the
restricted stop workflow. Existing restart code may compile as shared source, but
no request variant or helper call exposes managed launch/restart. Document the
dependency/import graph during Task 2; if sharing would pull Tauri into the helper,
stop and adjust the private boundary before proceeding.

Central internal names: `ElevationRequest` contains the design's version/request ID,
scope, operation and exact root selector; `AuthenticatedSession` privately owns
verified peers and transport; `ElevationPlan` owns the helper's frozen Single or
Batch plan; `ElevationTerminal` distinguishes verified pre-effect failure,
operation report and host-side OutcomeUnknown. No public constructors mint peer
authentication or confirmation. Wire DTOs never serialize ProcessTarget/admissions.
Protocol v1 uses a four-byte little-endian byte length followed by strict UTF-8
JSON, typed exact integers and deny-unknown-fields decoding. No JS number layer.

## Common Execution Rules

Order: Task 1 → 2 → 3 → 4 → 5 → 6. Task 1's feasibility must pass and receive
independent review before production implementation. A blocked installation gate
is not an N/A pass. Broader installer changes require separate scope approval;
do not edit `tauri.conf.json` or add an installer redesign silently.

For behavior changes: focused tests → capture intended failure → minimal change
→ focused pass → atomic task commit. Feasibility and evidence tasks record actual
experiments; do not fabricate RED. Each task gets one independent review covering
specification and quality/scope before its dependent task; only actual blockers
receive targeted fixes/rechecks. User stage gates still apply.

Run commands from the worktree root using `--locked --offline` after each new
package lock has been generated. Record any required dependency download explicitly.
Focused Windows fixtures run serially; final default regression also runs with
normal concurrency. Record command, source SHA, counts, exit status and duration.
Capture initial host baseline before code changes. Preserve inherited schema E0425
and external Clippy diagnostics as red; zero new process_control/elevation findings.

### Task 1: Non-Destructive Deployment And Authentication Feasibility

**Files:** feasibility spike, feasibility PowerShell runner and feasibility evidence
record from the file map. Read existing `src-tauri/tauri.conf.json`; do not modify it.
**Dependencies:** Design §§3, 5, 7, 9; explicit experiment approval before system changes.

- [ ] Inventory current packaging and produce an exact protected test-deployment
  proposal. The probe has distinct host/helper modes with fixed expected image
  identity and no termination entry. Record production installer compatibility
  separately: a manually protected fixture is not production packaging evidence.
- [ ] Prove loader dependencies, protected ancestor/file permissions, lease or
  exclusion behavior, absolute launch and same-account UAC peer/token checks.
- [ ] Exercise restricted pipe DACL, first instance, actual peer PID/creationTime,
  retained launch HANDLE and identification-only SQOS. Prove the server cannot use
  the helper client token for privileged impersonation.
- [ ] Show a native confirmation specimen with a 256-row synthetic list, long
  text, Cancel default, keyboard, Light/Dark/high contrast and scaling. No target
  planning/stop behavior in this probe.
- [ ] Record PASS/BLOCKED for each prerequisite, residual manual evidence, exact
  cleanup and proposed production build/deployment path. If current packaging
  needs broader changes, stop here and request scope approval before Task 2.

**Distinct test coverage:** user-writable/reparse/renamed image rejection; spoofed
peer, wrong account/session/version; canceled UAC and late child; pipe instance
collision; SQOS restriction; no stop imports/calls; owned files and handles cleaned.
**Verify:** `cargo test --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline`
and, after experiment approval,
`powershell.exe -NoProfile -File scripts/test-process-elevation-feasibility.ps1 -Mode Verify`.
The runner must require the approved deployment configuration and refuse defaults
that install, elevate or change permissions without that configuration.

### Task 2: Private Shared Boundary And Strict Session Protocol

**Files:** process_elevation `mod.rs`, `protocol.rs`, `session.rs`; helper package
manifest/lock and `src/main.rs`; existing host manifest/lock and `lib.rs`.
Tests live in these modules under `#[cfg(test)]`.
**Dependencies:** Task 1 passed; design §§5–6, 8.

- [ ] Establish the private shared-source helper build without Tauri dependencies.
  Keep the production helper fail-closed before authentication/confirmation exists.
- [ ] Implement versioned request/cancel/terminal frames and typed phase transitions;
  reject unknown fields, unsupported operations, exact-integer overflow and replay.
- [ ] Enforce length before allocation, full-read bounds, one request/result and
  worst-case preview/report sizing before confirmation; never truncate identities.

**Distinct test coverage:** empty/oversized/partial frames; duplicate request;
Confirm/Execute/Restart payload rejection; max integer round trip; wrong request
ID/version; second task after terminal; report budget rejection before effects.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_elevation::`
and `cargo check --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline`.

### Task 3: Trusted Launch And Authenticated Local Transport

**Files:** process_elevation `trust.rs`, `pipe.rs`, `launch.rs`, `mod.rs`, `session.rs`;
helper `src/main.rs`; manifest/lock feature adjustments only as needed.
**Dependencies:** Task 2; Task 1 evidence; design §§3, 5, 8.

- [ ] Promote the verified installation/loader policy and exact-peer validation
  into production RAII values; verify both ends before request payload/planning.
- [ ] Implement fixed runas launch, protected package identity/version/leases,
  DACL/SQOS transport and non-inheritable handles; no workspace fallback.
- [ ] Own launch work off the UI thread; keep pending-launch slot until OS return.
  Enforce launch/authentication/report deadlines with cancellable pipe I/O.

**Distinct test coverage:** launch canceled → late helper → invalid session/no
planning; pending launch expired → second launch refused until first OS return;
wrong SID/session/image/creationTime → PeerRejected; missing returned HANDLE →
LaunchFailed; forged endpoint/nonce and permission-denied inspection → no payload;
disconnect mid-frame → bounded termination; every resource released once.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_elevation:: -- --test-threads=1`.

### Task 4: Fresh Privileged Planning And Existing Stop Composition

**Files:** process_control `elevation.rs`, `tests/elevation.rs`, existing `mod.rs`,
`core.rs`, `windows.rs`, `tests.rs`; process_elevation `mod.rs`, `protocol.rs`.
**Dependencies:** Task 3; design §§4, 6–8; Stage 2B §17 and unchanged Stage 2A semantics.

- [ ] Derive eligibility only from a wholly known pre-effect originating attempt;
  discard old plan/admission state and keep reports non-executable.
- [ ] Convert exact selector by helper observation, mint fresh authority-bound
  target, plan Single/Tree/SameName and retain both exact protected host identities.
- [ ] Add cancellation/deadline checks between planning observations without
  changing existing callers' defaults or Stage 2B execution semantics.
- [ ] Consume the frozen plan once; delegate each admitted stop to existing
  operation/batch logic. Provide no publicly mintable confirmation token.

**Distinct test coverage:** mixed/uncertain report → no elevation; old PID reused →
no plan; host/helper root → ProtectedTarget; protected non-root → explicit result;
cancel during planning → zero stop; new same-name child after preview → excluded;
changed identity/stop-admission failure after confirmation → no effect on that
target; original ordering/256 limit/deadlines and partial results retained.
These new elevation assertions do not claim to repair Stage 2C Issue 2.
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control::tests::elevation -- --test-threads=1`.

### Task 5: Helper-Owned Confirmation And One-Shot Lifecycle

**Files:** helper `src/main.rs`, `src/confirmation.rs`; process_elevation `session.rs`,
`mod.rs`, `protocol.rs`; process_control `elevation.rs` for the private handoff only.
**Dependencies:** Task 4; design §§7–8 and `docs/ui-guidelines.md` native exception.

- [ ] Render exact frozen preview/omissions as inert native text with complete
  scrollable/full-text access and no user-supplied UI resources.
- [ ] Bind local confirmation to the displayed plan and atomic execution transition.
  Native cancel/default focus/Escape and host cancel share one pre-effect state.
- [ ] Recheck peer liveness before confirmation/execute; implement phase timers and
  single terminal report. Once executing, finish existing bounded batch despite
  host loss; host missing verified terminal receives OutcomeUnknown.

**Distinct test coverage:** cancel wins before execution → terminal/no stops;
execution wins then cancel/disconnect → finish/report-or-unknown, never rollback;
double click → one consumption; lost final report → no success inferred from exit;
confirmation expires → no effects; fake host Confirm message → protocol rejection.
Human checks cover 256 targets, control characters/long paths, DPI, both themes,
high contrast, accessible names, Tab/Escape/Enter and default Cancel.
**Verify:** `cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline confirmation::`
and `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_elevation::session::`.

### Task 6: Installed Owned-Windows Acceptance And Aggregate Evidence

**Files:** `scripts/test-process-elevation.ps1`, helper-owned fixture/test support
in `src-tauri/process-elevation-helper/src/fixtures.rs` (test-only), separate
`src-tauri/process-elevation-helper/src/bin/owned-fixture.rs` and
`src-tauri/process-elevation-helper/src/bin/owned-controller.rs`, helper manifest
test-artifact declarations, final evidence record and this plan's factual status only.
**Dependencies:** Task 5; design §9; approved protected deployment and human test session.

- [ ] Run the installed production helper through an owned controller using the
  production coordinator sources and approved fixture-only deployment identity;
  no production switch bypasses trusted image/account checks. Keep fixture modes
  in a separate test artifact, never in the released privileged executable.
  Require a non-default `owned-fixtures` Cargo feature for the two test binaries;
  the main helper must never declare/import fixture code. Use a separately built
  fixture-only package identity for the owned controller; production trusted-host
  metadata must never accept that controller identity. The runner builds and records
  both release and test artifact inventories, verifying no fixture bits are shipped.
  `process-elevation-helper/Cargo.toml` owns both `[[bin]]` targets with
  `required-features = ["owned-fixtures"]` and `test = false`. Their entry points
  are the named `src/bin` files; only those targets import `fixtures.rs`.
  Set the main binary name to `uipilot-process-elevation-helper`. Build it without
  default/test features, then build the two test binaries explicitly:

  ```text
  cargo build --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --release --no-default-features --bin uipilot-process-elevation-helper
  cargo build --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --release --features owned-fixtures --bin owned-controller --bin owned-fixture
  ```

  The approved experiment deploys an isolated protected test package whose immutable
  metadata pins the controller digest as its host and pins the unchanged release
  helper digest. Normal installed metadata pins the real UiPilot host instead.
  The helper reads metadata only from its own verified protected installation;
  no command argument/environment variable can redirect the trust root or select
  a relaxed test policy. Record both metadata inventories and prove cross-package
  controller rejection. Never replace the real installed host's trust metadata.
- [ ] Verify both stop operations and all scopes on explicitly owned fixtures,
  fresh set confirmation, cancellation, pre-effect rejection and result loss.
  SameName uses a unique fixture name and rejects any unowned matching process.
- [ ] Record actual UAC consent/cancellation and wrong-account evidence where
  available; unperformed required evidence stays pending, not a mock-derived PASS.
- [ ] Aggregate independent review, cleanup, dependency/package contents, helper
  loader imports, red baseline comparison and final user acceptance record.

**Distinct test coverage:** real privilege boundary against owned elevated fixtures;
both protected peer processes survive; no lingering helper/fixture/pipe/file lease;
lost report retains uncertainty. No random process selection or UAC automation.
**Verify:** after explicit system-test approval,
`powershell.exe -NoProfile -File scripts/test-process-elevation.ps1 -Mode Verify`.
The runner requires the approved deployment inventory; all affected absolute paths
must be validated before cleanup, and no global process-name cleanup is allowed.

## Final Matrix And Acceptance

Run once on the final code revision (repeat only after material changes/failure):

```text
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo fmt --manifest-path src-tauri/process-elevation-helper/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control:: -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline
cargo clippy --manifest-path src-tauri/process-elevation-helper/Cargo.toml --locked --offline --all-targets -- -D warnings
```

Host-module sharing must not cause legacy process-control real fixtures to run
twice concurrently; helper full tests run separately after host tests. Record any
existing snapshot flake as unresolved unless a separately authorized fix proves
its root cause. Final artifact/dependency and log review confirms no Tauri in helper,
no executable Restart route, no sensitive logs and no broadened plugin/UI scope.

Plan acceptance alone does not waive feasibility, experiment permission or Issue 2
disposition. After implementation is authorized, start only Task 1. Stage 2D final
acceptance does not authorize host-entry or Stage 3 design/development automatically.

Independent review on 2026-09-09 (`elevation_plan_review`) identified two plan
clarifications: helper feature isolation and executable fixture deployment. Both
were corrected and passed targeted recheck; no blocking findings remain. This is
a documentation review, not proof that builds, experiments or product tests pass.
