# Stage 2D Windows Process Elevation Design

**Date:** 2026-09-09
**Status:** READY FOR USER DESIGN ACCEPTANCE — architecture and scope agreed; independent review passed; detailed specification not yet accepted.

## 1. Authorization And Goal

Stage 2C remains ACCEPTED WITH CONCERNS. The user has lifted its hold only for
Windows elevation design, and approved same-account UAC plus a one-shot helper.
This document does not authorize implementation, an implementation plan, repairs
to Issues [1](https://github.com/baobaomi900901/UiPilot_tools/issues/1) or
[2](https://github.com/baobaomi900901/UiPilot_tools/issues/2), or Stage 3.

Stage 2D is a substage of host process control, not the roadmap's Stage 3
Capability model. Its goal is an explicitly confirmed privileged execution of
existing stop operations, preserving exact identity, admission, and uncertainty.

## 2. Agreed Scope And Alternatives

Included: Windows 11 x64; same Windows user account and interactive session;
one-shot elevated helper; Single, Tree, and SameName scopes; Terminate and
ForceTerminate; authenticated local transport; helper-owned confirmation;
bounded lifecycle and owned-fixture verification.

Excluded: elevated Restart or managed launch; alternate administrator credentials;
whole-host elevation; persistent service; SYSTEM; SeDebugPrivilege; public plugin,
Tauri, launcher/settings integration; arbitrary commands; persistent plans,
history, tokens or receipts; automatic retries; Stage 3 or later work.

Alternatives: elevating the whole host unnecessarily includes plugins and other
host functions. A persistent service introduces installation and lifetime costs.
The approved one-shot helper limits each privileged lifetime to one frozen task.
Existing Stage 2C receipts/plans cannot be serialized into it to obtain elevated
Restart. That capability requires a separately approved future design.

## 3. Trust And Deployment Boundary

The helper is a separate minimal native executable, not a privileged instance of
the Tauri application. It loads no plugin, WebView, user script, user configuration,
remote asset or user-supplied executable. The medium-integrity host remains the
request coordinator; the helper alone owns privileged planning and confirmation.

Production execution requires an administrator-installed, local protected package:
host, helper, immutable version/digest metadata, resources and dependencies must
not be writable or replaceable by the unelevated user, including ancestor paths.
Reject reparse/network paths and installations whose effective permissions cannot
be established. A Program Files string alone is not proof. Launch only the exact
installed helper, with an absolute trusted working directory and safe dependency
resolution; never search PATH or execute from the workspace or a temporary folder.
The helper's early startup must not load user-controlled dependencies before its
own validation. Host/helper package versions must match; updates must not substitute
files across validation and launch. Retained file/directory leases or equivalent
installer exclusion must enforce that invariant, not only a pre-launch hash.

This trusted package is a prerequisite, not a claim about today's installer.
The implementation plan must begin with a non-destructive feasibility gate proving
protected deployment, loader behavior and identity checks. If existing packaging
cannot meet it without broader installer changes, report the required scope and
seek approval; do not silently enlarge this stage or fall back to user-writable
execution. Authenticode may add assurance but plugin signing/Stage 9 is not required.

Threat boundary: reject spoofed peers, stale identities, replayed requests and
arbitrary privileged execution. Do not claim immunity to an already compromised
administrator/kernel or arbitrary code injection into the legitimate host. A
same-account SID and a nonce alone do not authenticate the correct application.

## 4. Elevation Eligibility

Follow [Stage 2B section 17](./2026-09-04-process-target-expansion-batch-orchestration-design.md):
only pre-effect InspectionDenied or OperationAccessDenied can offer explicit
elevation. End the old call and discard its plan/admissions; retain its report only
as a result, never as authorization. Do not elevate an old partial report's failed
subset. This first version requires the whole originating attempt to be known
pre-effect; mixed/uncertain batches do not offer this continuation.

ProtectedTarget, graceful timeout, TerminationPending and CompletionUnknown do
not qualify. No automatic UAC, force escalation or retry. If a caller no longer
has a reliable PID plus creationTime root selector, reject instead of guessing.
InspectionDenied therefore does not promise support for never-identifiable targets.

Eligibility is a host workflow rule, not proof trusted from an IPC boolean.
The helper independently limits the operation, validates the target, applies
protection and obtains explicit user confirmation for every request.

## 5. Launch And Peer Authentication

The host creates a single local named-pipe server with a random 256-bit request
identifier, explicit restrictive DACL, first-instance exclusivity, one instance,
and remote-client rejection. Do not use the default DACL or grant generic write
rights that also allow another pipe instance. Grant only the required connection
rights to the initiating logon context. Compatibility of its restricted/elevated
tokens must be demonstrated, not assumed; inability to connect fails closed.

Launch the fixed helper with ShellExecuteExW, the runas verb and
SEE_MASK_NOCLOSEPROCESS. This is the sole explicit Shell exception; no cmd,
PowerShell, scripts, association or arbitrary executable parameters. Bootstrap
arguments contain only validated protocol metadata, pipe identifier and expected
host PID/creationTime; treat all of them as untrusted input, not secrets or authority.
Retain the returned helper process HANDLE; missing HANDLE is launch failure.

Before any request payload or process planning, both sides validate the actual
pipe peer PID using GetNamedPipeClientProcessId/GetNamedPipeServerProcessId, bind
it to creationTime using a retained process HANDLE, and verify its installed
image identity/version and token. The host binds the client to its launched
helper HANDLE, not a caller-supplied PID. The helper binds the server to the
bootstrap host identity and its trusted installed host image. Recheck peer liveness
before confirmation/execute; any inability to establish identity is rejection.

Token checks require equal TokenUser SID, equal interactive TokenSessionId,
non-elevated initiating host and elevated administrator helper (not SYSTEM).
Different-account UAC must fail before planning. Account equality is not display
name equality. Do not require an undocumented equality of token logon LUIDs.
Reject remote/service/noninteractive sessions. The helper opens the pipe with
SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION to prevent the lower-integrity
server from impersonating its elevated client for privileged execution.

Only one authenticated connection is accepted. Failed identity, version or nonce
checks close the session without reconnect; nonce is correlation/anti-replay, not
a substitute for peer authentication. All HANDLEs are non-inheritable and RAII-owned.

## 6. Protocol And Ownership

Use a versioned, length-prefixed, bounded message protocol. Version 1 has at most
one request and one terminal result; maximum encoded frame size is 8 MiB. Check
length before allocation; reject unknown fields/variants, invalid lengths,
duplicate requests and out-of-order messages. Exact integer identities must not
round-trip through floating point. Before confirmation, verify the complete preview
and worst-case terminal report fit the frame bound; otherwise reject with
PayloadTooLarge before effects, never truncate or discover the limit after stopping.
No paths, command lines, env blocks, callbacks,
HWNDs, HANDLEs, ProcessTargets, admissions, plans or launch receipts are executable
request inputs. Preview paths are output-only plain text.

The single request contains request ID, protocol version, scope enum, operation
enum and root selector (nonzero PID, creationTime). That selector is not a
ProcessTarget: the helper re-observes the exact object, refuses PID reuse, then
constructs a target bound to its fresh ProcessAuthority/HostInstanceId. It takes a
new planning snapshot for Tree/SameName and uses the existing batch module. Single
scope creates a corresponding one-target in-memory plan. No old set is transferred.

Protect both the helper's exact self identity and the authenticated initiating
host's exact identity, in addition to current critical/PPL rules. Apply this at
planning and fresh execution admission, including members of batches. A protected
batch member is reported as ProtectedTarget, not silently removed; a protected
root rejects planning. Never implement these exclusions using a PID alone.

Module ownership: a host coordinator owns eligibility/launch/session; a narrow
transport owns framing and peer checks; the helper owns confirmation/lifecycle;
process_control continues to own identity, minimal-access operation and batching.
Reuse the process-control implementation through a private shared module boundary,
not copied algorithms or a new public SDK. Exact file arrangement belongs to the
separate implementation plan.

## 7. Confirmation And Execution

Flow: eligible failure → explicit elevation request → UAC → authenticated request
→ fresh helper plan → helper-owned confirmation → consume once → fresh Stage 1
and Stage 2A admission → existing operation/batch report → helper exits.

The helper displays its own trusted native confirmation window. The IPC protocol
has no Confirm/Execute message; the ordinary host cannot synthesize consent.
Show operation, scope, exact ordered target list (up to 256), names/PIDs/available
paths, total count, omissions and incomplete-selection warning. Allow scrolling
and full-text access; never silently truncate the target set. Names/paths are
untrusted display text, never markup; expose control characters visibly.

Native controls use OS theme/high-contrast/accessibility behavior; this is an
explicit proposed exception to CSS-token implementation, not to the semantic,
compact layout, dangerous-button and keyboard rules in
[UI guidelines](../../ui-guidelines.md). Support Light/Dark, visible focus,
accessible names, scaling and long text. Initial focus is Cancel; Escape/window
close cancels before execution. Enter cannot accidentally confirm on initial focus.
UAC is OS-owned and separate from this operation confirmation. No remember/skip
confirmation preference. No plugin or user-writable UI resource is loaded elevated.

The confirmed plan is the exact plan consumed; no re-expansion afterwards. Reuse
256-target limit, order, omissions, 30-second scheduling and entered-operation
5-second budget. Each target gets fresh admission and minimum intent-specific
rights; do not use PROCESS_ALL_ACCESS or PROCESS_CREATE_THREAD for stopping.
Protection and identity checks do not weaken merely because the caller is admin.

## 8. Lifecycle, Cancellation And Results

State machine: Launching → Authenticating → Planning → AwaitingConfirmation →
Executing → Terminal. Pre-execution states can terminate without process effects.
The helper accepts no second task; confirmation is consumed atomically before
entering execution. A failed operation cannot re-arm it.

Coordinator waiting for launch/UAC expires after 120 seconds; it invalidates the
request and closes its pipe. It cannot promise to dismiss an OS UAC prompt or
interrupt ShellExecuteEx. A late helper gets no valid session and must exit. Run
launch off the UI thread. Allow at most one pending launch per host, retaining that
slot until the OS launch call returns, including after logical cancellation.
Helper authentication/request waiting expires after 15 seconds; confirmation
after 120 seconds; planning after 30 seconds; terminal-report delivery after 5
seconds. Each deadline starts on entering its phase. Use monotonic clocks.
Planning checks cancellation/deadline between observations; synchronous Win32
calls are not claimed to be hard-real-time interruptible.

Before execution, authenticated disconnect, host exit or cancel invalidates the
plan and closes confirmation. Cancel is an optional host message only before
execution. Once execution has entered, it runs the existing bounded operation or
batch to completion even if transport/host disappears; do not invent rollback,
abort the helper mid-effect or change Stage 2B's batch semantics. A cancellation
racing with execution is not proof of zero effects. No automatic helper restart.

Before execution, stable session errors distinguish UacCancelled, LaunchFailed,
LaunchExpired, UntrustedInstallation, PeerRejected, AccountMismatch,
ProtocolRejected, SessionExpired and UserCancelled. Planning keeps existing
process errors. After execution, reuse complete per-target results and their
partial/unknown semantics; helper exit code alone never means operation success.

If the host loses the final report after sending the request, it cannot know
whether native confirmation/execution occurred: report session-level
OutcomeUnknown, preserving any already verified information without inventing
target successes. No auto-retry or elevation continuation from that result.
Only a verified terminal pre-effect cancellation/error establishes no effects.
Session-level uncertainty is distinct from a Stage 2A target CompletionUnknown.

Preview/result travel only to the authenticated host, remain in memory and are
not a replayable plan. Normal logs contain stable codes, scope/operation and
aggregate counts only; omit identities, names, paths, pipe IDs, tokens and raw
Win32 errors. No disk history or cross-launch recovery.

## 9. Verification And Exit Gate

The future implementation plan must first prove trusted installed launch,
same-account token/pipe compatibility, mutual exact peer binding, SQOS protection
and native confirmation feasibility using non-destructive owned fixtures. Failure
stops destructive implementation rather than relaxing the contract.

Deterministic checks must cover wrong SID/session/image/version, PID reuse,
missing launch HANDLE, spoofed endpoints, oversized/malformed/replayed frames,
untrusted install and dependency paths, timeout/late helper, double confirmation,
cancel/disconnect races, both protected host identities, pre-effect eligibility,
old partial-report rejection, fresh set confirmation, additions after confirmation,
batch limits/deadlines, and loss of report after effects. Assert no stop call on
every pre-execution rejection and exact HANDLE cleanup on all exits.

Real Windows tests operate only on explicitly created and owned fixtures. Retain
exact cleanup handles; never target arbitrary system processes. Automated tests
cover transport/planning/protection and result mappings. Human tests are required
for actual UAC acceptance/cancellation, other-account rejection where a test
account is available, and native confirmation keyboard/theme/accessibility.
Unperformed tests are recorded as such, never inferred from mocks. No CI UAC bypass.

Run relevant focused and process_control regression suites; compare default tests,
all-features and Clippy against inherited red baselines without claiming them fixed.
Issue 1 remains an unresolved stability risk; Issue 2 remains a coverage gap. Before
implementation approval, explicitly choose whether to repair Issue 2 separately
or retain it with recorded justification; this design does not make that choice.

Exit requires approved detailed design and separate plan, feasibility evidence,
owned Windows evidence and cleanup, no new process_control lint defects, one
independent correctness/scope review with no unresolved blocking findings, and
explicit user acceptance. This document contains verification requirements, not
completed tests or implementation authorization.

## 10. Sources And Review Gate

Existing contracts: [Stage 2B](./2026-09-04-process-target-expansion-batch-orchestration-design.md),
[Stage 2C](./2026-09-08-single-process-restart-design.md),
[roadmap](./2026-09-02-raycast-inspired-plugin-platform-roadmap-design.md).

Win32 references (design policy above is UiPilot's choice, not a Microsoft guarantee):

- [SHELLEXECUTEINFOW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow): launch flags and returned process HANDLE lifetime.
- [Named pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights): explicit ACL and specific access rights.
- [CreateNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea): first-instance and remote-client rejection flags.
- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew): client SQOS/identification controls.
- [Client PID](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid) and [server PID](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeserverprocessid): OS-observed peer identity components.

After independent review, the user must accept this written specification before
an implementation plan is prepared. No host launcher entry or Stage 3 design is
authorized by accepting this substage's design.

Independent read-only review on 2026-09-09 (`elevation_design_review`): no blocking
findings; ready for user design acceptance only. Reviewed Stage 2B handoff,
Stage 2C exclusions, exact-target/batch semantics, native UI exception and scope.
Deployment/token/UI feasibility remains unproven until implementation evidence.
No product tests or UAC were run for this documentation-only change.
