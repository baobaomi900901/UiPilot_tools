# Stage 2D Task 1: Deployment And Authentication Feasibility

**Date:** 2026-09-09
**Status:** IN PROGRESS — packaging expansion approved and optional profile implemented; actual installation/UAC and full Task 1 feasibility remain unverified.
**Inspected revision:** `2a57db3a546113a7d36a9011ad833a9f0a96958a`.
**Plan:** [Stage 2D Task 1](../plans/2026-09-09-windows-process-elevation.md).

## Initial Inventory Evidence (Before Packaging Approval)

- `src-tauri/tauri.conf.json` enables bundle targets `all`, but has no
  `bundle.windows.nsis.installMode`, custom installer template/hooks or external
  helper binary entry. Resources list Everything assets and notification audio;
  no elevation helper or immutable host/helper digest manifest is present.
- `scripts/tauri.ps1` forwards build arguments to the local Tauri CLI. Its automatic
  override applies only to development server ports. The tracked
  `tauri.security-probe.conf.json` changes only the frontend build command.
  No inspected tracked release configuration establishes a protected elevation package.
- Cargo.lock pins tauri-utils 2.9.3. Its locally cached `src/config.rs` lines 818–840
  define NSISInstallerMode's default as CurrentUser (non-administrator installation)
  and PerMachine as the Program Files/admin installation alternative. NsisConfig
  uses that enum's default at lines 873–875. This establishes an NSIS default gap,
  not an assertion that every possible MSI or external installer is unsafe.
- The worktree lacks `node_modules/@tauri-apps/cli/config.schema.json` and the local
  CLI executable. No dependency installation was attempted. Rust dependencies are
  available; the preceding independent baseline at this same revision ran
  `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline process_control:: -- --test-threads=1`:
  182 passed, 0 failed, 2 fixture entrypoints ignored. That is a process-control
  baseline only, not deployment or elevation evidence.

These observations do not prove effective ACLs of any installed application.
No installer was built/run, no installed files were probed or changed, and no
UAC/token/pipe/native-confirmation experiment was run. At that initial checkpoint
no Task 1 spike or runner had been created; packaging scope approval was pending.

## Required Scope Decision

Design §3 and Task 1 explicitly require stopping if production packaging needs
broader changes. Current tracked configuration does not provide the required
trusted package. A manually protected test directory would demonstrate only the
fixture, so it cannot close this gate or authorize Task 2.

Recommended minimum expansion: add an opt-in, NSIS-only protected installation
profile for elevation-capable builds, retaining ordinary installation behavior.
This is a proposal, not an applied configuration or a claim that PerMachine alone
proves trust. The existing default/per-user builds must fail closed for elevation.

Proposed configuration file: `src-tauri/tauri.elevation.conf.json` with this initial
installation policy (helper/resources are added only when the approved artifacts exist):

```json
{
  "bundle": {
    "targets": ["nsis"],
    "windows": {
      "nsis": { "installMode": "perMachine" }
    }
  }
}
```

The requested expansion includes a build/package inventory for the host, fixed
helper, immutable digest/version manifest and every shipped dependency/resource;
an installer guard against user-writable or reparse destinations; and validation
that install/upgrade/uninstall preserve the protected package boundary. Use a
separate protected package identity/destination for experiments, never replace the
user's current installed host or change its permissions to create evidence.

Candidate files for that separately approved work are the opt-in config above,
`scripts/build-process-elevation-package.ps1`, and an NSIS hook file
`src-tauri/windows/elevation-installer-hooks.nsh` if needed for destination checks.
No auto-update framework, service, plugin signing, Stage 3 or default-installer
migration is included. If implementation needs those, stop and report separately.

## Next Checkpoint After Scope Approval

Prepare the approved Task 1 non-destructive probe and runner, build them locally,
verify their no-stop boundary and have an independent agent review them. Produce
the exact binary hashes, proposed absolute test destination, complete copy/ACL
inventory and bounded cleanup commands before requesting installation/UAC approval.
At that point authorization concerns concrete artifacts, not a generic permission
to run arbitrary elevated commands. No installation approval is requested here.

Until the native experiment actually runs, loader trust, same-account pipe/token
compatibility, peer binding, SQOS, UAC cancellation and confirmation UI remain
NOT RUN. Task 1 is not complete and Tasks 2–6 remain unstarted.

## Independent Review

`elevation_packaging_gate_review` on 2026-09-09: no blocking findings in this
inventory/gate report. Confirmed tracked configuration and pinned default;
older Everything design proposals are not implemented installer evidence.
Confirmed the approved design/plan requires separate packaging-scope approval.
Read-only review; no native tests, installation or UAC performed.

## Approved Optional Packaging Implementation

The user approved the minimum packaging expansion after `11abb19`. Added:

- `src-tauri/tauri.elevation.conf.json`: opt-in x64 NSIS per-machine profile,
  separate `UiPilot_tools Protected` product and `com.uipilot.launcher.protected`
  identity. The base configuration is unchanged.
- `spikes/process-elevation-feasibility`: currently a standalone read-only native
  installation-directory checker, not yet the host/helper IPC/UI probe. Fixed
  known-folder destination, NTFS, non-reparse ancestor/object handles, trusted
  owner and conservative DACL write-grant checks. Reject unknown ACE types,
  null DACL, unsafe ownership/grants and unreadable paths. It never repairs ACLs,
  starts another process or provides a process-control operation.
- `src-tauri/windows/elevation-installer-hooks.nsh`: embeds that checker into
  install/upgrade and uninstall checks. Saves/restores OUTDIR; no shell command
  fallback. Checked handles are released when the checker finishes, so this is
  a package-directory check, not the future helper's runtime trust/lease protocol.
- `scripts/build-process-elevation-package.ps1`: default Check mode inventories
  configuration/resources without building or installing; explicit Build mode
  builds the checker and protected installer and records exact installer/host/checker
  SHA-256 values. No mode launches the installer. Inventory explicitly reports
  `processElevationAvailable: false`; no runtime helper manifest is claimed.
- `scripts/test-process-elevation-package.ps1` and the compile-only NSIS harness:
  verify inventory, base-config preservation, alternate-directory rejection and
  install/uninstall hook syntax.

Checks completed:

- Initial policy RED: two intended unsafe-write assertions failed against the
  empty policy stub; after implementation all 4 Rust tests passed.
- `cargo test --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline`: 4 passed, 0 failed, including an owned user-writable-directory rejection.
- `cargo fmt --manifest-path spikes/process-elevation-feasibility/Cargo.toml --check`: passed.
- `cargo clippy --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline --all-targets -- -D warnings`: passed after correcting one new comparison lint.
- `powershell.exe -NoProfile -File scripts/test-process-elevation-package.ps1`: PROTECTED_PACKAGE_CHECK_PASS.
- Cached NSIS `makensis.exe /V2 hook-compile-test.nsi`: compiled; test installer not run.
- Read-only release checker `preinstall "C:\Program Files\UiPilot_tools Protected"`:
  passed against this machine's current directory chain; no installation performed.
- `npm.cmd ci --ignore-scripts` installed locked local build dependencies; existing
  package locks unchanged. New isolated Rust package has its own lock.
- Final `powershell.exe -NoProfile -File scripts/build-process-elevation-package.ps1 -Mode Build`:
  succeeded with locked/offline Cargo arguments; produced
  `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/UiPilot_tools Protected_0.3.4_x64-setup.exe`.
  Installer SHA-256: `fd46ff865f931ed6f8a6ae35f3d34c100e427c7db42b5b6e5a7077df29474d81`.
  Adjacent `protected-package-inventory.json` records all input resource hashes
  and the exact host/checker hashes. Existing Vite chunk-size warning remains;
  installer was not executed. Full default/all-features tests were not rerun for
  this isolated packaging change; inherited baseline failures are not cleared.

Independent `protected_packaging_review` found one uninstall working-directory
defect; fixed and targeted recheck passed. The agent independently ran the 4 Rust
tests and package script successfully; no remaining blocking finding in the
reviewed preparatory changes.

Limits: actual install/upgrade/uninstall, temporary checker extraction trust,
and post-install directory ACLs have NOT been verified. Tauri CLI 2.11.4's generated
template performs `SetOutPath $INSTDIR` before PREINSTALL; it can create a selected
directory before rejection. No zero-filesystem-effects claim applies to an actual
installer run. The helper IPC/token/UAC/native-confirmation probe remains pending;
Task 1 is not complete and Task 2 has not started. The generated installer must not
be installed merely because compilation passed.

## Ordinary-Token Pipe Experiment (2026-09-10)

Added `spikes/process-elevation-feasibility/src/pipe_probe.rs`, included only under
`cfg(all(test, windows))`. Extra Windows APIs are dev-dependencies; the installer
checker has no pipe/child-launch mode. This increment validates actual Win32 calls
before constructing an elevated experiment, not production protocol code or Task 2.

The experiment creates one local byte-mode named pipe using a random 256-bit name,
an explicit current-logon-SID DACL with specific data/attribute/synchronize rights,
first-instance exclusivity and remote-client rejection. It launches only its own
test executable's ignored child fixture, with retained Child ownership. Both ends
compare OS-reported peer PID and observed creation time; the host binds the peer
to its owned child's process handle. A random challenge/echo/ack proves a complete
exchange before the fixture exits.

The client opens with identification-only SQOS. After reading its response, the
server inspects the actual thread token under impersonation and requires
SecurityIdentification, then reverts on every exit/unwind. A failure to revert
aborts the test process rather than continuing under another identity. NOWAIT I/O,
15-second polling deadline and owned-child cleanup bound the experiment's waits;
cleanup never searches or terminates a process by name. Ordinary logs suppress
fixture output and emit only stable failure classifications.

Validation on the diff over `c0a9ec2`:

- Focused `cargo test --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline pipe_probe::owned_cross_process -- --test-threads=1`:
  1 passed, 0 failed, 0.04 seconds test time.
- Independent `ordinary_pipe_probe_review` ran the full isolated package suite:
  5 passed, 0 failed, 1 ignored (the explicitly spawned child entrypoint).
- Strict all-target Clippy passed; rustfmt applied. Independent review found no
  actionable issue in the scoped ordinary-token experiment.

This is native experimental evidence, not a fabricated test-first RED for an
existing Windows API. No production implementation was added. It does not prove
wrong-account or elevated-token rejection, image/package trust, hostile peer
resistance, remote-session rejection, UAC behavior, or privileged impersonation
failure against a protected resource. Only the identification token level was
observed; the remote-rejection flag was set, not tested with a remote client.
No actual installation, UAC, elevated fixture or native confirmation was run.
Remaining Task 1 work includes the protected host/helper probe, negative identity
cases, cancellation/late-helper behavior and the approved manual experiment.

### Ordinary-Token Rejection And Cancellation Follow-up (2026-09-10)

On the diff over `9fb5691`, added four real owned-child scenarios:

- Correct pipe, wrong expected parent PID: fixture requires ServerMismatch before
  any challenge is supplied.
- Correct PID, wrong expected creationTime: fixture requires ServerChanged before
  any challenge is supplied.
- Parent closes its endpoint, then releases the child's stdin start gate: the
  late child cannot connect and requires ConnectDeadline. No sleep determines
  this event order; the 100 ms connection budget starts inside the released child.
- Client confirms host identity and sends a readiness marker; parent observes it
  and closes the endpoint: the client requires PipeDisconnected, not ReadDeadline.

The child asserts the exact expected error; an unrelated failure or success makes
its test process fail. Happy-path launches explicitly clear the scenario/expected
error environment variables. The parent observes child completion with a deadline;
the existing owned-child guard remains responsible for failed-path cleanup.

Initial focused run: 4 passed, 1 failed, 1 ignored in 15.30 seconds. The new
disconnect case exposed read_frame retrying every native error until timeout.
Changed only the test probe: BROKEN_PIPE/PIPE_NOT_CONNECTED return PipeDisconnected;
NO_DATA/PIPE_LISTENING remain retryable; other errors return PipeReadFailed.
Focused rerun: 5 passed, 0 failed, 1 ignored in 0.28 seconds. Strict all-target
Clippy passed after removing a redundant unit expression; rustfmt applied.

These tests validate ordinary-token expected-parent identity rejection and
endpoint lifecycle, not UAC cancellation, privileged helper launch or production
caller authentication. Protected image verification, wrong-account/session/token
cases, elevated native confirmation and actual installation remain untested.

Independent `pipe_negative_cases_review`: no blocking findings; full isolated
package tests passed (9 passed, 0 failed, 1 ignored child entrypoint; 0.13 seconds).
Nonblocking coverage caveat: disconnect asserts the stable classification, not
an independent elapsed-time upper bound. No installation or UAC performed.

## Protected UAC Probe Prepared (2026-09-10)

Prepared an opt-in `uipilot-uac-probe` executable under the isolated spike's
`uac-probe` feature; it is not a shipped helper or a Stage 2 implementation.
Its commands are `inspect`, `self-token-check`, `host`, and internal `helper`.
Only `host` can request UAC, and only after its own executable is verified at the
fixed protected installation location. The installed command is a nonce
challenge/echo/ack experiment, with no process stop/restart, plugin or user command.
It does not yet implement the native 256-row confirmation specimen.

Changes:

- `src/policy.rs` shares the existing ACL policy without algorithm changes.
  `native.rs` shares directory verification and provides the separate fixed probe
  location check; existing installer-check command behavior remains intact.
- `src/bin/uac_probe.rs` holds directory leases and a no-write/no-delete executable
  lease through the handshake; authenticates actual pipe peer PID/creationTime
  and fixed image path. Host binds helper to the returned ShellExecuteEx HANDLE.
- `src/uac_identity.rs` independently reads tokens: initiating host must be medium
  integrity in the active local console session; helper must be high-integrity,
  elevated, enabled Administrators member, same TokenUser and same session. SYSTEM,
  alternate-account, remote/session mismatch and unavailable observations reject.
- The existing pipe transport is shared with the opt-in binary; child fixtures
  and fixture termination remain `cfg(test)` only. Installer checker has no new
  runtime launch mode. Tests explicitly cover account/session/role rejection;
  synthetic role tests are not actual alternate-account UAC evidence.
- Launch runs on a COM-initialized worker with a 120-second coordinator wait.
  Ownership transfers through a rendezvous channel; failed delivery closes the
  worker's HANDLE. This timeout is not a cancellation of the OS UAC prompt or
  ShellExecute operation. A late helper must fail without a valid host/pipe.
- `scripts/test-process-elevation-feasibility.ps1` defaults to Check (read-only
  inventory), supports Prepare (local build), and requires an explicit matching
  ApprovedSha256 for Verify against the fixed installed file. Verify may request
  UAC; it has not been run. No mode installs files or repairs existing ACLs.

### Exact Proposed Experiment Inventory — Not Yet Authorized Or Executed

Source binary:
`spikes/process-elevation-feasibility/target/release/uipilot-uac-probe.exe`.

SHA-256: `5931eb64cc72d6da8a29deb71129938e2068c27206ecc29cbe46096aff753291`.

Fixed destination: `C:\Program Files\UiPilot Elevation Probe` (absent when checked).
Single installed file: `uipilot-uac-probe.exe`. No services, registry entries,
shortcuts, user data migration or changes to the installed UiPilot application.

Directory security descriptor:
`O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)`.
The new file must likewise be owned by Administrators, with SYSTEM/Administrators
full access and Users read/execute only; no user-writable explicit ACE.

After explicit experiment approval, the installation action must require the
destination to remain absent, validate the existing non-reparse ancestor chain,
create the protected directory with this descriptor, and copy the reviewed binary
from a handle held against writes/deletion after hashing it against the approved
digest. Set/verify file ownership and DACL; rehash the installed bytes. Abort if
the destination already exists; never reuse or repair it. A deployment failure
must report any created path and clean only this action's proven owned artifacts.

Then run installed `inspect` and the ordinary-token runner:

```text
powershell.exe -NoProfile -File scripts/test-process-elevation-feasibility.ps1 -Mode Verify -ApprovedSha256 5931eb64cc72d6da8a29deb71129938e2068c27206ecc29cbe46096aff753291
```

The user must handle Windows UAC themselves. Successful consent and canceled
consent are separate recorded observations; never automatically retry cancellation.
No alternate account or extra administrator credentials are needed for the first
same-account experiment. After helper/host exit, cleanup verifies the exact owned
non-reparse directory and file/hash, deletes only the single installed file, then
removes the empty directory. Do not recurse into unexpected contents or terminate
processes by name. Preserve/report unexpected state rather than deleting it.

### Preparation Evidence And Limits

- Feature-enabled isolated tests: 13 passed, 0 failed, 1 ignored owned child entry.
- Feature-enabled strict all-target Clippy passed; rustfmt applied.
- Prepare built the release executable using `cargo rustc ... -- -C target-feature=+crt-static`.
  Initial dynamic VCRUNTIME dependency was removed before proposing deployment.
- `dumpbin /DEPENDENTS` on the final binary lists only shell32, ole32, kernel32,
  advapi32, bcrypt, api-ms-win-core-synch-l1-2-0, oleaut32 and ntdll (kernel32 also
  appears with uppercase spelling). No app-local VC runtime dependency remains.
  This import-table inspection does not prove every possible dynamic OS load.
- Final release `self-token-check`: exit 0, UAC_PROBE_COMMAND_PASS.
- Final release `inspect` from the worktree: exit 1, UntrustedProbeLocation.
- No actual `host`, `helper`, installed `Verify`, installation, elevation or native
  confirmation was executed during preparation.

Task 1 remains incomplete. Installed medium→high token inspection, protected image
compatibility, ShellExecute returned-handle behavior, actual UAC cancellation/late
helper exit and native confirmation still need evidence. A source review cannot
substitute for these observations. Tasks 2–6 remain unstarted.

Independent `uac_probe_preparation_review`: source/runner review found no confirmed
blocker; independently ran feature tests (13 passed, 1 ignored), verified both
safe release commands and confirmed the exact proposed binary SHA-256. No elevated
command or installation was run by the reviewer. Awaiting explicit authorization
for this concrete temporary deployment and UAC experiment.

## Authorized Installed UAC Experiment (2026-09-10)

The user explicitly authorized the concrete temporary deployment, handshake and
cleanup after reviewing the inventory. Executed the already reviewed binary
SHA-256 `5931eb64cc72d6da8a29deb71129938e2068c27206ecc29cbe46096aff753291`
at source revision `326f3ef`; no product/probe code changes in this run.

### Deployment And Independent Inspection

The fixed source digest and absent destination were rechecked. A bounded
PowerShell deployment script was independently reviewed before execution. It was
passed as an in-memory encoded command to hidden elevated PowerShell, not loaded
from a mutable script path by the elevated interpreter. Its copy used the same
read-locked source stream that had matched the approved hash, CreateNew output,
explicit Administrators ownership and the reviewed protected ACLs.

- Deployment process returned `DEPLOY_EXIT=0`.
- Installed binary hash matched the approved digest.
- Directory SDDL: `O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)`.
- File SDDL: `O:BAG:BAD:PAI(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)`.
- Installed `inspect` returned UAC_PROBE_COMMAND_PASS, exit 0.
- Independent `uac_deployment_review` reread the installed hash/ACLs and ran only
  `inspect`; all matched. The reviewer did not perform a UAC operation.

The deployment uses an absence precheck plus directory creation, not an atomic
create-only directory primitive. Trusted parent permissions and post-create
owner/ACL validation constrain ordinary-user interference; this is not a proof
against concurrent malicious administrators.

### Actual Medium-to-High Handshake

Executed the documented Verify command with the approved SHA-256. It returned
two UAC_PROBE_COMMAND_PASS lines and exit 0 (installed inspection, then handshake).
Under the reviewed executable's success conditions this demonstrates, on this
machine and account:

- ShellExecuteEx runas supplied a usable retained helper process handle.
- Actual high-integrity enabled-admin helper and medium-integrity host had matching
  TokenUser and local-console session; both could perform the required observations.
- Both ends matched exact peer PID/creationTime and the fixed protected image.
- The logon-SID pipe ACL allowed the elevated client without generic-write grants.
- Nonce challenge/echo/ack completed; server observed SecurityIdentification SQOS.
- Host observed helper exit and exit code 0 before returning success.

This does not prove process-operation behavior, arbitrary high-process access,
actual cross-account denial, or a protected-resource impersonation attempt. The
probe has no such operations. No existing user/application process was terminated.

### Cancellation Attempt: NOT VERIFIED

The user-facing instruction requested cancellation for the next UAC invocation.
That invocation instead returned UAC_PROBE_COMMAND_PASS and exit 0. Record it as a
second successful handshake, not UacCancelled. No inference is made about which
button was pressed or whether Windows policy displayed a consent prompt.
No automatic retry or further cancellation attempt was made.

Actual UAC cancellation, the 120-second launch-wait expiry/late-helper path and
prompt dismissal remain unverified. Ordinary-token endpoint-cancellation tests
above are separate evidence and cannot substitute for these Windows observations.

### Cleanup

The reviewed cleanup verified the fixed non-reparse directory, exact single-file
contents and matching binary hash. It removed only that file and the empty owned
directory; no recursive deletion or process-name termination was used.

- Elevated cleanup returned `CLEANUP_EXIT=0`.
- Parent `Test-Path -LiteralPath 'C:\Program Files\UiPilot Elevation Probe'` returned False.
- Read-only process-name lookup returned no currently running probe; nothing was
  killed through that lookup.
- Workspace-built artifacts remain in ignored target directories for evidence;
  the temporary Program Files installation is gone. No services, registry entries
  or shortcuts were created by this experiment.

Task 1 remains IN PROGRESS: native confirmation specimen and its human checks,
actual UAC cancellation/late-helper evidence, and remaining rejection scenarios
are not complete. This successful echo experiment does not authorize Task 2 or
constitute Stage 2D acceptance.

Independent final check by `uac_deployment_review`: temporary directory absent,
zero currently named probe processes, and cancellation-NOT-VERIFIED interpretation
confirmed. No further UAC attempt was performed during that review.

## Native Confirmation Specimen Prepared (2026-09-10)

Added opt-in `confirmation-specimen` feature and a separate
`uipilot-confirmation-specimen` binary. It contains only a synthetic preview and
Win32 controls; no process catalog, transport, elevation or execution entry is
connected. This prepares the Task 1 UI experiment, not Stage 2D Task 5 production UI.

The frozen sample has 256 numbered synthetic targets and an explicit simulated
incomplete-selection warning. Native LISTBOX provides vertical/horizontal access;
a read-only multiline EDIT exposes the full selected text, including a long-path
sample. Control/bidi characters in the dedicated sample are displayed as escapes.
The window title/header and confirmation button explicitly label simulation.

Model outcomes are Pending, Cancelled, ConfirmedSimulation and TimedOut. The first
terminal decision is final. The window requests default/focused Cancel, handles
Escape/close and a 120-second timer, and uses native dialog-message keyboard routing.
The actual keyboard delivery has not yet been observed. The shared callback state
uses a stable allocation; model borrows end before reentrant Windows calls.

Light/Dark palettes use the semantic colors from `src/ui-theme.css`. Native stock
font and OS GDI scaling follow the approved native-control exception. Windows
high-contrast colors, sampled on launch, override the content palette; a theme or
high-contrast change should be tested by reopening the sample. Standard control
names and focus behavior are retained; owner-drawn buttons show danger outline,
focus, pressed/hot and disabled branches. Native hover delivery, narrator naming,
contrast and multi-monitor/scaling behavior still need actual UI verification.

Preparation evidence over `dfdebd8`:

- Initial model RED: 1 passed, 2 failed (control escaping and absorbing terminal
  state). After implementation: 3 passed, 0 failed.
- Independent `confirmation_specimen_review` ran the 3 model tests, then reviewed
  state lifetime/reentrancy and identified missing button-state paint branches.
  Those branches were added and passed targeted source recheck. Verdict: ready
  for human validation, not runtime UI PASS.
- Complete isolated `cargo test --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline --all-features`:
  3 specimen tests plus 13 other tests passed; 1 owned child entrypoint ignored.
- All-target/all-feature strict Clippy passed; rustfmt applied.
- Release static-CRT build succeeded. Binary SHA-256:
  `a69585d2f7b8a507d928bf6cbd758bc757a5d9d0ba07216f6efd36ad5f804478`.

Binary: `spikes/process-elevation-feasibility/target/release/uipilot-confirmation-specimen.exe`.
It requires explicit `--show light` or `--show dark`; no arguments do not open UI.
No window was displayed during preparation, and no screenshot/render/keyboard
check is claimed. Neither UAC probe binary nor temporary installation was changed.

Human checks still needed: initial Enter returns Cancelled; Escape/close cancels;
Tab/Shift+Tab traverse visible controls; selecting row 256 exposes the complete
long path; row 255 shows escaped controls; intentional confirmation returns only
ConfirmedSimulation; Light/Dark, high contrast, scaling and accessible names are
usable. A normal-token window does not prove a high-integrity UI integration.
Task 1 remains incomplete; no Task 2 or Stage 3 work started.

### Human Keyboard Finding And Enter Routing Fix

The user verified Light initial Enter closes with CONFIRMATION_SPECIMEN_Cancelled.
The user then confirmed one Tab moves the visible focus rectangle to the simulation
Confirm button, but Enter still returned Cancelled. Therefore initial Cancel and
Tab focus movement were observed; focused-confirm activation failed.

Source investigation found DM_GETDEFID always returned Cancel, while DM_SETDEFID
was ignored. With the owner-drawn buttons this retained the Cancel action despite
the focused Confirm. On the user's explicit repair request, changed only the
specimen routing: DM_GETDEFID queries current focus and button enabled state,
selecting Confirm only for the exact valid enabled Confirm HWND. All other focus
states select Cancel. No sticky Confirm default is stored when focus leaves it.

Added three regression cases for focused Confirm, initial/other focus and
disabled/absent Confirm. The first run failed the focused-Confirm assertion
(returned Cancel ID 2, expected Confirm ID 1); the other two passed. After the
fix, all six specimen tests passed. Strict all-target Clippy for the specimen
feature passed, and the static-CRT release binary was rebuilt at the original path.
Rebuilt specimen SHA-256:
`d76e9454ff78d5d4cfa9643eb7da7e11b804aad8c206c59791f179775ef7e613`.

Independent `confirmation_enter_fix_review` found no actionable issue and reran
all six tests successfully. No GUI or input synthesis was performed by the agent.
Actual native Tab→Enter and retained initial-Enter behavior after this change
still require human retest; unit policy checks do not establish runtime UI PASS.
