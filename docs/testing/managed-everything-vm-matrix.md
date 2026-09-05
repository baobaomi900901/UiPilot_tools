# Managed Everything Windows VM Matrix

This is the release-blocking operator checklist for the 13 rows in
`子系统 3：验证与发布门禁`. Local automation does not satisfy this matrix. Every row starts
`NOT RUN`; changing a row to `PASS` requires the named evidence files from a disposable
Windows VM and an operator review of the stable outcome. Any `FAIL` or `NOT RUN` blocks
installation readiness.

## Common admission and variables

Do not run these commands without explicit approval for VM mutation. Start from a disposable
snapshot, open an explicitly elevated PowerShell, and build the independently named instrumented
Setup outside the VM run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/build-managed-everything-installer.ps1 -Configuration Release -Mode VmInstrumentedSetup
cargo.exe build --manifest-path spikes/everything-ipc/Cargo.toml
$VmSetup = (Resolve-Path 'src-tauri/target/vm-instrumented/release/bundle/nsis/UiPilot_tools VM Instrumented Setup_*_x64-setup.exe').Path
$Marker = 'C:\UiPilot-disposable-vm.marker'
[IO.File]::WriteAllText($Marker, 'UIPILOT_DISPOSABLE_VM=I_UNDERSTAND_THIS_MACHINE_WILL_BE_MUTATED')
```

The marker is an acknowledgement, not an authorization substitute. Every `Vm*` mode independently
requires elevation, validates the exact marker content, rejects a reparse marker, and accepts only the
independently named Setup under `target/vm-instrumented`. The scripts never invoke the Helper directly.
Fill these operator-supplied values before using a row:

```powershell
$PrivateClientPid = 0
$OtherSessionClientPid = 0
$DefaultEverythingPid = 0
$UiPilotPid = 0
$SystemTokenPid = 0
$NetworkServiceTokenPid = 0
$AuthorizedUser = Get-Credential
$OtherUser = Get-Credential
```

Evidence is emitted under ignored `artifacts/managed-everything/`. It contains only stable labels,
booleans, counts, exit codes, and hashes; machine, user, SID, absolute-path, search-text, and result-path
data are forbidden. Cleanup means revert the disposable VM snapshot after copying out only the named
redacted evidence. Never carry an instrumented Setup or marker into a release machine.

## Row 1 — clean administrator first install

- Prerequisites: clean supported Windows x64 VM; administrator is also the desktop user; no UiPilot
  Service, manifest, managed ProgramData root, or UiPilot process exists.
- Commands:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row01-install
  # Launch UiPilot_tools normally, set $PrivateClientPid, and perform one operator-owned /find smoke check.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmInstalledRuntime -DisposableVmMarker $Marker -EvidenceName row01-runtime -ClientProcessId $PrivateClientPid
  ```
- Expected stable outcome: one private Service is running; a non-elevated UiPilot owns one Ready private
  client in the desktop session; the intended desktop user receives results without starting Everything.
- Evidence: `installer-row01-install.json`, `runtime-row01-runtime.json`, plus the operator smoke result
  recorded without search text or result paths.
- Cleanup: close UiPilot and revert the VM snapshot.
- Status: `NOT RUN`

## Row 2 — standard-user launch with different UAC administrator

- Prerequisites: clean VM with distinct standard desktop user and administrator credential; Process
  Explorer or an ACL viewer is available for the staging observation.
- Command, entered from the standard-user session and run in the credentialed elevated shell:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row02-credential-install
  # Back in the original standard-user session, launch UiPilot and set $PrivateClientPid.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmInstalledRuntime -DisposableVmMarker $Marker -EvidenceName row02-standard-runtime -ClientProcessId $PrivateClientPid
  ```
- Expected stable outcome: during Setup, the standard user cannot read, replace, or delete the protected
  Helper/request/journal; a separately extracted Helper run unelevated is rejected before request access;
  no Helper remains in the install directory. Normal staging deletion succeeds. Repeat with an injected
  AV/handle hold: any residue stays protected and non-reusable while the verified Service remains valid.
  The original standard user, not the credential provider, can use `/find`.
- Evidence: `installer-row02-credential-install.json`, `runtime-row02-standard-runtime.json`, and redacted
  operator booleans for the four staging access attempts and both cleanup variants.
- Cleanup: release the injected handle if used and revert the VM snapshot.
- Status: `NOT RUN`

## Row 3 — coexistence with default Everything

- Prerequisites: default Everything installed and running; record its PID in `$DefaultEverythingPid` and
  capture its configuration hash before UiPilot installation.
- Commands:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row03-install
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmCoexistence -DisposableVmMarker $Marker -EvidenceName row03-coexistence -ClientProcessId $PrivateClientPid -DefaultEverythingProcessId $DefaultEverythingPid
  ```
- Expected stable outcome: install, search, normal/tray exit, and later uninstall leave the same default
  Everything process, Service, configuration hash, and data untouched.
- Evidence: `installer-row03-install.json`, `runtime-row03-coexistence.json`, and redacted before/after
  equality booleans for the default instance.
- Cleanup: use Row 6 true uninstall, confirm default Everything still runs, then revert the snapshot.
- Status: `NOT RUN`

## Row 4 — both upgrade paths, cancellation, and writable source replacement

- Prerequisites: old production UiPilot installed for the two upgrade paths, with old uninstaller, private
  Service, manifest, ProgramData, and application hashes recorded. The source-race variant additionally needs
  a standard-user-writable `$VmTestRoot` and a synchronized low-privilege actor supplied by a future
  Instrumented Setup interface.
- Commands for manual replacement and Tauri `/UPDATE`, respectively:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row04-manual-replacement
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmUpdate -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row04-tauri-update
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmCancelBeforeInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row04-cancel
  $VmInstallDirectory = [IO.Directory]::CreateDirectory((Join-Path $VmTestRoot 'UiPilot-race')).FullName
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmSourceReplacement -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row04-source-race -VmTestRoot $VmTestRoot -InstallDirectory $VmInstallDirectory
  ```
- Interface blocker: `VmSourceReplacement` fails closed before launching Setup. The current Instrumented Setup
  has no synchronization point after Helper opens the source and before protected copy, and the five-file
  harness has no safely scoped low-privilege actor token. A post-copy administrator replacement would not prove
  the required held-source identity boundary and must not be accepted as evidence.
- Expected stable outcome: neither upgrade path calls the old uninstaller. Cancellation or failure before
  Install Section changes nothing. During the source race, a low-privilege replacement attempt gets a sharing
  violation or Helper copies from one continuously held identity; the protected target never contains replacement
  bytes. An already running UiPilot/private client causes no executable write error after Install Section, and
  old Service state remains rollback-capable.
- Evidence: `installer-row04-manual-replacement.json`, `installer-row04-tauri-update.json`,
  `installer-row04-cancel.json`, `installer-row04-source-race.json`, and redacted before/after equality booleans.
- Cleanup: close any source-race handles and revert the snapshot.
- Status: `NOT RUN` — **BLOCKED: synchronized low-privilege source-handle probe missing**

## Row 5 — bounded client and Service recovery

- Prerequisites: Row 1 installed/Ready state; set `$PrivateClientPid` immediately before each independent
  snapshot run.
- Commands:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmRetryBudget -DisposableVmMarker $Marker -EvidenceName row05-client-retry-budget -ClientProcessId $PrivateClientPid
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmRecovery -DisposableVmMarker $Marker -EvidenceName row05-service-unavailable -ClientProcessId $PrivateClientPid -RecoveryTarget Service
  ```
- Expected stable outcome: three successive exact private-client deaths each produce one PID/creation-bound,
  DB-loaded, fixed-Query2 Ready replacement. Terminating that third replacement produces no fourth Ready
  private process throughout the complete 65-second startup budget while the exact host and Service remain
  alive. Service death independently makes the exact client readiness probe unavailable. No unowned Everything
  process is terminated.
- Interface blocker: the current host exposes no external diagnostic/read-only surface for the Supervisor's
  exact `Unavailable(reason)` enum. These commands prove the externally observable readiness and restart-budget
  behavior only; they must not label the internal terminal state as observed or satisfy that final assertion.
- Evidence: `runtime-row05-client-retry-budget.json`, `runtime-row05-service-unavailable.json`; only the
  observed restart/termination counts and stable booleans are recorded. The retry file must contain
  `supervisor_terminal_state_observed=false` until a host observability interface exists.
- Cleanup: revert the snapshot after each termination variant.
- Status: `NOT RUN` — **BLOCKED: exact Supervisor terminal-state observability missing**

## Row 6 — true uninstall and default-instance preservation

- Prerequisites: installed private runtime; optional default Everything from Row 3; UiPilot closed.
- Command; on the maintenance page explicitly select **Uninstall application**:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmExplicitUninstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row06-uninstall
  ```
- Expected stable outcome: private Service, managed versions, transaction/runtime manifests, and managed
  ProgramData root are absent; the user-owned default Everything installation and process are unchanged.
- Evidence: `installer-row06-uninstall.json` and redacted absence/preservation booleans.
- Cleanup: revert the snapshot.
- Status: `NOT RUN`

## Row 7 — console/RDP isolation and authorization boundary

- Prerequisites: first-install desktop SID has concurrent console and RDP sessions; second non-admin user,
  second administrator, and suitable LocalSystem/NetworkService token-host PIDs are available. Run each
  credential command in its own disposable snapshot if account lockout policy requires it.
- Commands:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmMultiSession -DisposableVmMarker $Marker -EvidenceName row07-sessions -ClientProcessId $PrivateClientPid -OtherSessionClientProcessId $OtherSessionClientPid
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-authorized-interactive -TokenCase AuthorizedInteractive -TokenSource CredentialInteractive -Credential $AuthorizedUser
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-authorized-batch -TokenCase AuthorizedBatch -TokenSource CredentialBatch -Credential $AuthorizedUser
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-authorized-network -TokenCase AuthorizedNetwork -TokenSource CredentialNetwork -Credential $AuthorizedUser
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-system -TokenCase LocalSystem -TokenSource Process -TokenProcessId $SystemTokenPid
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-elevated-admin -TokenCase ElevatedAdmin -TokenSource Current
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-other-interactive -TokenCase OtherInteractive -TokenSource CredentialInteractive -Credential $OtherUser
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-other-batch -TokenCase OtherBatch -TokenSource CredentialBatch -Credential $OtherUser
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-network-service -TokenCase NetworkService -TokenSource Process -TokenProcessId $NetworkServiceTokenPid
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmPipeSecurity -DisposableVmMarker $Marker -EvidenceName row07-filtered-admin -TokenCase FilteredAdmin -TokenSource Linked
  # In a separately elevated shell signed in as the second administrator, run the same trusted Setup as repair.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmSidPreservation -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row07-second-admin-repair
  ```
- Expected stable outcome: console/RDP have distinct clients, INIs, DBs, and session IDs but one Service.
  Authorized-SID Interactive/Batch/Network, LocalSystem, and elevated administrator tokens pass read/write
  `AccessCheck` and connect. Other-user Interactive/Batch, NetworkService, and filtered-admin tokens fail
  read/write/create-instance `AccessCheck` and actual local connect/create with AccessDenied. Running Setup
  under a second administrator does not replace the first-install SID. Remote end-to-end refusal is logged
  separately and is not treated as DACL proof.
- Evidence: `runtime-row07-sessions.json`, the nine `runtime-row07-*.json` token files named above, and
  `installer-row07-second-admin-repair.json` with `desktop_sid_preserved=true` and no SID value.
- Cleanup: sign out the RDP session without changing account policy and revert the snapshot.
- Status: `NOT RUN`

## Row 8 — phase faults, termination recovery, bootstrap boundaries, and rollback failure

- Prerequisites: a verified old install; a separate snapshot per checkpoint/termination target; another
  elevated administrator credential for the recovery handoff variant.
- The currently available fault-return commands are listed below for diagnosis only; they do **not** satisfy
  this row's termination and recovery variants:

  ```powershell
  $InstallCheckpoints = @('bootstrap-root-created','transaction-temp-created','transaction-temp-flushed','transaction-temp-renamed','new-binary-verified','old-service-stopped','old-service-removed','new-service-installed','new-service-started','runtime-manifest-replaced','cleanup-complete','rollback-started','rollback-complete')
  foreach ($Checkpoint in $InstallCheckpoints) {
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmFaultCheckpoint -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName ("row08-$Checkpoint") -Checkpoint $Checkpoint -ExpectedExitCode 1
  }
  ```
- Interface blocker: the independently named Instrumented Setup exposes only
  `UIPILOT_VM_FAULT_CHECKPOINT`, which makes Helper return a failure. It has no synchronization/termination
  control for Helper, parent Setup, or simulated restart at a named phase; no rollback-failure trigger; and
  no protected operation/recovered-transaction-ID observation surface. Therefore no truthful executable
  commands exist yet for those required variants within this five-file test-harness scope. Do not substitute
  manual timing or edit this row to `PASS`.
- Expected stable outcome: each Service-phase failure restores the verified old Service. A rollback fault
  retains the old manifest and every verified binary with a diagnostic failure. Helper/Setup termination or
  restart at each phase and bootstrap create/flush/rename boundary converges on rerun to exactly one verified
  old/new state before application copy. Only exact bootstrap residue is recoverable. Recovery under another
  elevated administrator records distinct operation and recovered-transaction IDs without exposing them.
- Evidence: the checkpoint failure files are diagnostic only. Required Helper/Setup/system-termination,
  rollback-failure, bootstrap-recovery, and cross-admin handoff evidence cannot be produced until the
  Instrumented Setup gains the interfaces above.
- Cleanup: revert after every independent checkpoint variant.
- Status: `NOT RUN` — **BLOCKED: Instrumented Setup termination/recovery interface missing**

## Row 9 — process creation, readiness, logoff, and shutdown cleanup

- Prerequisites: installed app plus Task 2 unit-test evidence for Job creation/attribute/CreateProcess/resume
  faults; a separate snapshot for normal exit, tray exit, logoff, rapid shutdown, crash, and forced host exit.
- Commands: for normal exit, tray exit, crash, and target-session logoff, start `Observe` while the exact client
  is still alive, then perform the named action while the command is waiting. Run the logoff observer from a
  different administrator session that remains signed in. Use `ForceHost` only for its named forced variant:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmShutdown -DisposableVmMarker $Marker -EvidenceName row09-observe -ClientProcessId $PrivateClientPid -ShutdownAction Observe
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-runtime.ps1 -Mode VmShutdown -DisposableVmMarker $Marker -EvidenceName row09-force-host -ClientProcessId $PrivateClientPid -UiPilotProcessId $UiPilotPid -ShutdownAction ForceHost
  ```
- Interface blocker: an in-guest process cannot persist trustworthy postcondition evidence after rapid system
  shutdown. That variant needs a VM-external observer or an Instrumented host shutdown record before it can run;
  absence after reboot alone does not prove Job cleanup at the shutdown boundary.
- Expected stable outcome: failures before CreateProcess leave no child; a returned child is already in the
  Job; every exit phase leaves no session client while the private Service stays running. Job/attribute list,
  CreateProcess, resume, readiness, logoff, and rapid shutdown results match Task 2's ordering contracts.
- Evidence: one uniquely named `runtime-row09-<exit>.json` from a pre-started surviving observer for each
  observable exit, `runtime-row09-force-host.json`, the Task 2 unit-test output reference, and future external
  rapid-shutdown evidence.
- Cleanup: revert after each exit variant.
- Status: `NOT RUN` — **BLOCKED: VM-external rapid-shutdown observer missing**

## Row 10 — uninstall cancellation and manifest-last interrupted continuation

- Prerequisites: installed and running UiPilot for cancellation; closed UiPilot and a fresh snapshot for each
  deletion checkpoint.
- Commands:

  ```powershell
  # Select uninstall, cancel at the app-running gate, and verify no managed state changed.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmExplicitUninstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row10-cancel-running
  foreach ($Checkpoint in @('uninstall-referenced-directory-deleted','uninstall-manifest-deleted')) {
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmUninstallCheckpoint -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName ("row10-$Checkpoint") -Checkpoint $Checkpoint -ExpectedExitCode 1
  }
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmExplicitUninstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row10-resume-uninstall
  ```
- Expected stable outcome: cancellation before `PREUNINSTALL` changes nothing. After confirmation, interruption
  after Service deletion, each referenced-directory boundary, and manifest-last deletion resumes idempotently.
  A foreign same-name Service is never accepted as deleted.
- Evidence: `installer-row10-cancel-running.json`, one `installer-row10-<checkpoint>.json` per referenced
  directory occurrence, and `installer-row10-resume-uninstall.json`.
- Cleanup: revert after each interruption variant.
- Status: `NOT RUN`

## Row 11 — live pipe DACL and complete token matrix

- Prerequisites: Row 1 installed/Ready state and the identities/token-host PIDs from Row 7.
- Commands: run all nine `VmPipeSecurity` commands listed in Row 7 again with evidence names changed from
  `row07-` to `row11-`.
- Expected stable outcome: `GetSecurityInfo` returns a DACL whose normalized SDDL exactly equals the manifest;
  every positive and negative Windows `AccessCheck` plus actual local connect/create-instance result matches
  Row 7. A token carrying the authorized desktop SID is never classified as a negative case.
- Evidence: nine `runtime-row11-*.json` files, one per token case, plus a separately redacted remote-connect
  observation that is not used as authorization proof.
- Cleanup: close sacrificial token-host processes and revert the snapshot.
- Status: `NOT RUN`

## Row 12 — foreign Service/root/manifest conflicts fail closed

- Prerequisites: four independent clean snapshots preloaded respectively with a foreign same-name Service,
  foreign managed root, missing manifest beside managed state, and invalid signature/field manifest. Record
  original Service ImagePath/state and object hashes without writing raw paths to evidence.
- Command in each snapshot:

  ```powershell
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row12-foreign-state -ExpectedExitCode 1
  ```
- Expected stable outcome: Setup fails before modifying any Service, directory, or file; original hashes,
  ImagePath, state, and contents compare equal. Unknown state is not adopted, overwritten, or cleaned.
- Evidence: `installer-row12-foreign-state.json` from each labeled snapshot and redacted before/after equality
  booleans identifying only the conflict class.
- Cleanup: revert each snapshot without asking UiPilot cleanup code to remove foreign state.
- Status: `NOT RUN`

## Row 13 — upgrade/downgrade/repair maintenance and concurrency

- Prerequisites: separate old/new/same-version installs; valid old uninstaller; standard-user object-squatting
  harness; two elevated administrator sessions; snapshot for abandoned-owner and parent-death/activity-lease runs.
- Commands:

  ```powershell
  # Run once for manual upgrade, allowed downgrade, and same-version repair; never select uninstall here.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmInstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row13-reinstall
  # Same-version explicit maintenance uninstall; select Uninstall application.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmExplicitUninstall -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row13-explicit-uninstall
  # This covers Setup/Setup only; it must not be reused as evidence for the blocked variants below.
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-managed-everything-installer.ps1 -Mode VmConcurrency -VmSetupPath $VmSetup -DisposableVmMarker $Marker -EvidenceName row13-concurrency
  ```
- Interface blocker: the Instrumented Setup provides no programmatic true-uninstall participant, no
  standard-user mutex/activity-event precreate/hold/abandon/type/DACL harness, and no phase synchronization
  that can kill the parent Setup while the Helper activity lease is known live. The existing
  `VmConcurrency` mode starts two Setups and proves only that narrow variant. Cross-admin recovered IDs are
  likewise not observable. These variants cannot be truthfully automated in the assigned five files; do not
  infer them from two Setup exit codes or manual timing.
- Expected stable outcome: upgrade/downgrade/repair never starts the old uninstaller; all cancellation/failure
  points before main copy preserve the old uninstall entry and managed state. Explicit uninstall exits after the
  old uninstaller and never enters install. Exactly one trusted transaction owns the mutex/activity event.
  Standard-user precreate/hold/abandon/type or DACL changes fail before managed reads. Abandoned trusted ownership
  recovers durably. If parent Setup dies while Helper holds the activity lease, a new Setup remains busy until the
  old Helper rolls back/exits, then recovers with distinct protected operation/recovered IDs.
- Evidence: `installer-row13-reinstall.json`, `installer-row13-explicit-uninstall.json`, and
  `installer-row13-concurrency.json` cover only their named partial paths. Required true-uninstall,
  object-squatting, abandoned-owner, and parent-death/activity-lease evidence remains unavailable.
- Cleanup: release only operator-owned squatting handles, then revert the snapshot; never delete unknown objects
  through the product harness.
- Status: `NOT RUN` — **BLOCKED: Instrumented Setup concurrency-control interface missing**

## Release decision

Current matrix result: **BLOCKED — 13 required rows are `NOT RUN`**. A future explicitly approved disposable-VM
run may change a row only after all of its commands, manual observations, evidence review, and cleanup complete.
Local gate success must continue to emit `VM_MATRIX_NOT_RUN RELEASE_READY_FALSE` until then.
