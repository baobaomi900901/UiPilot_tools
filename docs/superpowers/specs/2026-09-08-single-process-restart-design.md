# Stage 2C Single-Process Restart Design

**Date:** 2026-09-08
**Status:** APPROVED - accepted by the user on 2026-09-08
**Prerequisite:** Stage 2B accepted by the user on 2026-09-08.

## 1. Goal And User Decision

Deliver a host-internal single-process restart with explicit stop and launch
outcomes. The user approved rejecting restart when original arguments or working
directory cannot be established reliably. Rejection must occur before stopping
the old process. Reopening an executable alone is not a successful substitute.

The user accepted this design on 2026-09-08. A separate implementation plan must
be approved before implementation starts.

## 2. Current Boundary And Alternatives

ProcessRecord currently contains an exact ProcessTarget, observed name, optional
executable path, and parent hint. It contains no command line or working directory.
Existing application launching uses ShellExecute/activation and has no complete
process-bound launch receipt. Those paths do not become restartable implicitly.

Alternatives considered:

- Executable-only relaunch: broad availability but loses startup context; rejected
  by the user's decision.
- Recover arbitrary external process context: do not promise this. Win32_Process
  exposes a command line but not a general original-working-directory property;
  registered application restart settings describe opt-in restart arguments, not
  a complete original launch record. No remote PEB parsing, undocumented NT query,
  Shell, WMI fallback, or guessed directory is introduced.
- Host-recorded launch receipt: recommended. Capture the exact supported launch
  inputs when the host creates the process, bind them to its returned identity,
  and use only that receipt for subsequent restart.

Consequently, this first version supports processes launched through the new
internal managed path. Pre-existing processes, existing ShellExecute-launched
applications, and their descendants without their own receipts return
LaunchContextUnavailable before any stop. The user accepted this coverage
restriction together with the design.

## 3. Scope

One inseparable workflow: managed launch creates a trustworthy receipt; restart
consumes a plan derived from that receipt. Without the creation path, the strict
restart policy would have no usable source of startup information. This is not a
general plugin process-launch capability or a launcher UI migration.

Included: ordinary Win32 executable launch under the current host security
context, in-memory receipt, one-target restart, explicit Graceful/Force modes,
identity revalidation, failure reporting, Fake and owned-Windows verification.

Excluded: process-tree/same-name restart, elevation, services/UWP activation,
Shell associations/scripts, custom inherited handles, arbitrary external context
recovery, UI/Tauri/plugin exposure, disk history, automatic retry/rollback,
application session/document recovery, and later roadmap stages.

## 4. Receipt And Planning Contract

The process-control Module supplies an internal managed launch operation. Its
trusted host-only input requires an absolute executable path, exact UTF-16 command
line, explicit absolute initial working directory, and explicit environment block.
Empty arguments are valid; unknown arguments are not. No PATH lookup, implicit
working directory, command-string reparsing, or environment inheritance at restart.

Capture the effective launch environment once. Windows drive-directory entries
and environment-block ordering/termination follow the documented CreateProcessW
contract. Reject invalid strings, embedded NULs, unsupported launch options, or
incomplete inputs before launch. The supported command line includes the original
executable token; pass a separate non-null lpApplicationName to avoid ambiguous
executable selection. Do not tokenize and reconstruct an arbitrary raw command line.

A successful managed creation records the HostInstanceId, exact returned process
PID/creationTime, frozen launch inputs, and observed executable file identity.
Only the Module can mint ProcessLaunchReceipt from its successful creation path.
There is no API for attaching caller-supplied context to an arbitrary PID.

Receipt and ProcessRestartPlan have private fields, no public constructors,
Clone/Copy, Serialize/Deserialize or mutable accessors. They remain in memory and expire with
the host instance. No global on-disk or in-memory PID registry is required: the
host owns a receipt value and the plan privately shares its single-attempt state.

Planning borrows the receipt and binds Graceful or Force. It verifies target
identity/protection, receipt provenance, and launch-input availability before
returning a handle-free plan. Canceling/dropping an unexecuted plan leaves the
receipt usable. Preview contains the old display identity, executable, launch
arguments/directory for host confirmation, and stop mode; secrets are never logged.
Environment values remain private. Argument presentation belongs to the later
host confirmation surface and must account for sensitive content.

Two plans from the same receipt cannot execute concurrently. An atomic receipt
state transition admits one attempt; other attempts return RestartInProgress
without stop/launch calls. Receipts for distinct targets remain independent.

## 5. Execution And Irreversibility

The caller confirms a frozen plan, then passes that same plan by value to execute.
Execution validates host binding and acquires the receipt attempt before effects.
Revalidate the exact old target and the frozen executable/directory availability.
Acquire a short-lived executable read lease before stopping, verify it against
the receipt's file identity, and retain it through the launch call. Reject changed
or unavailable executable identity before stopping. This lease is not held while
the user considers confirmation.

The supported executable and working-directory paths are local fixed-volume
paths with no reparse points, junctions, symlinks, network shares, device aliases,
or alternate data streams. Resolve the volume to its GUID rather than depending
on a mutable drive-letter mapping. The initial managed launch validates and uses
these same effective paths; the receipt records exactly what it passed to Windows.

At both initial launch and restart, walk and validate every component from the
volume root to the executable and working directory. Open directory components
with the documented directory/reparse inspection flags, reject any reparse point,
and retain handles denying incompatible write/delete access to every component.
Retain the final executable read handle as well. Check volume/file identities,
component names and final resolved paths after all leases are held; any mismatch,
unsupported filesystem behavior, or sharing denial rejects before stopping.

Use only the verified volume-qualified application/directory paths in
CreateProcessW, and retain all leases until that call returns. The intended
property is stable resolution to the verified executable and working directory
during this interval, including parent rename/replacement protection. A final-file
handle alone is explicitly insufficient. Do not fall back to an unleased path.

An owned-Windows feasibility test must demonstrate this path policy with
CreateProcessW before implementing destructive restart. If a supported path form
cannot satisfy it, report a design blocker rather than replacing the contract with
a final-file-only check. Kernel/admin tampering and volume removal are outside
this ordinary host-token model; launch failure remains possible and is reported.
File identity is not a package signature or a promise to freeze DLLs.

Use the existing Stage 1 Restart intent for restart-policy admission. The accepted
Stage 2A operation methods reject Restart directly; do not weaken that contract.
Compose the selected stop by obtaining a fresh Stage 1 Terminate/ForceTerminate
admission for the same exact target, then invoking the matching Stage 2A method.
Drop the restart-policy admission before the single-target operation. Stage 2A
still performs its own final identity/protection checks and owns its five-second
budget. There is no automatically chosen stronger mode.

Only Stage 2A Ok, meaning the exact old process was observed exited, permits a
single launch attempt. TargetExited during admission, graceful timeout,
TerminationPending, CompletionUnknown, and all other stop failures do not launch.
They do not count as a successful restart, and later UI must not claim otherwise.

Immediately before a possible launch, permanently mark the old receipt consumed.
Then call CreateProcessW once with the frozen inputs and no inherited handles.
If creation fails after the old process has exited, report that irreversible
partial outcome. Do not restart an old PID, undo the stop, or retry launch.

If stopping fails before launch, release the attempt state. A new explicit
user-confirmed plan may revalidate the same receipt; there is no automatic retry.
Unwind before the consumed transition releases the attempt; unwind after that
transition cannot make another launch permissible. Close all temporary handles.

On creation success, close the returned thread handle, capture the new exact
identity using the returned process handle, mint a replacement receipt, then close
the process handle after required observation. Future restart uses the replacement
receipt, not the consumed old receipt.

## 6. Result Semantics

Use a typed result with separate stop and launch information, not a single bool.
Before any stop call, stable errors distinguish unavailable context, expired
receipt/plan, concurrent attempt, target/protection failure, unsupported platform,
and changed/unavailable launch executable. These errors have zero restart effects.

After entering the stop operation, return a structured report preserving its
original stable error category and whether launch was attempted. Required terminal
states: StopFailedLaunchNotAttempted, OldProcessExitedLaunchFailed,
OldProcessExitedNewProcessCreated, and OldProcessExitedLaunchIdentityUnknown.

Creation success is not application readiness: CreateProcessW can return before
initialization completes. If identity observation fails after successful creation,
report launch may have succeeded, no receipt, and no retry; do not hide the new
process behind a preflight-style error or automatically kill it. Do not promise
restoration of windows, documents, session state, or later process health.

Stable errors/logs contain codes and phases only. No raw Windows error, PID,
command line, working directory, environment, or path is written to normal logs.

## 7. Windows And Resource Policy

Use documented CreateProcessW with explicit application, mutable command-line
buffer, explicit directory/environment, CREATE_UNICODE_ENVIRONMENT, and
bInheritHandles = FALSE. Close both returned PROCESS_INFORMATION handles on all
relevant success/error paths. Launch uses the host's current token; no impersonation,
UAC, debug privilege, PROCESS_ALL_ACCESS or remote thread creation.

Restart does not require PROCESS_CREATE_THREAD on the old process: new-process
creation is distinct from injecting a thread into that target. Observation and
stop handles retain the accepted minimum-rights policies.

The launch-context promise concerns recorded initial launch inputs, not mutable
current directory/environment inside the old running application. Dynamic runtime
state is outside this contract. Non-Windows rejects before any effect.

## 8. Verification And Exit Gate

Fake coverage must prove missing context stops before effects, host/target binding,
immutable mode/inputs, both stop paths, no launch on every stop failure, one launch
after observed exit, consumed receipt on launch failure/unknown state, replacement
receipt binding, overlapping-plan exclusion, cancellation/replanning, and exact
handle/attempt-state cleanup on error and unwind.

Owned Windows fixtures verify arguments containing spaces/quotes/Unicode, explicit
working directory and environment, a new PID/creationTime, old exact-process exit,
Graceful/Force behavior, and no launch after ignored-close timeout. Invalid path,
changed executable, pre-existing process without receipt, failure after stop, and
initial-creation identity-observation failure must have honest outcomes. Every
fixture retains a separate bounded cleanup capability before effects.

Run proportional focused/full regression and Clippy; compare inherited Stage 2B
deviations without suppressing new diagnostics. The later implementation plan
owns task decomposition, file boundaries, commands, and evidence collection.

Exit requires the approved design and separate plan, working managed receipt and
restart chain, owned-Windows evidence, no new module diagnostics, one independent
correctness/scope review, and explicit user acceptance. Design approval authorizes
the separate implementation plan only. Product code requires plan approval;
elevation and host-entry work retain their later stage gates.

## 9. Official References

- [CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw): independent application/command-line/directory/environment inputs, handle ownership, token behavior, and asynchronous initialization.
- [GetApplicationRestartSettings](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getapplicationrestartsettings): registered restart settings are not a general complete launch snapshot.
- [Win32_Process](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process): process properties do not establish a general original-working-directory capture path.
- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew): sharing restrictions and directory/reparse-point flags used to establish transient path leases.
- [GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew): resolved handle paths and volume-GUID naming; the complete lease policy above is this project's proposed contract, not a pathname-launch guarantee supplied by this API alone.

## 10. Approval

The user approved strict missing-context rejection, managed-launch coverage,
receipt lifetime/concurrency, and the stop/launch result contract on 2026-09-08.

Independent review on 2026-09-08 found a pathname-to-file identity gap. The targeted
revision added full path-component protection and an explicit Windows feasibility
gate. Recheck approved this draft for user review with no remaining blocker; it
does not approve implementation or imply that the feasibility test has run.
