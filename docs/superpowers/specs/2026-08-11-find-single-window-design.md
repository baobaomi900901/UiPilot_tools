# Single-Instance `/find` Window Design

## Status

Approved for implementation planning on 2026-08-11 after five independent
review rounds and final user confirmation.

## Goal

Move file search out of the launcher into one dedicated Tauri window while
preserving the existing Everything search, category filters, sort order,
preview, opaque result authorization, and authenticated file execution.

The launcher remains visible after submitting `/find`. Repeated submissions
reuse the same file-search window and replace only its query text.

## User Contract

### Opening And Forwarding

- Enter on `/find str` in `main` opens or focuses the single `find` window and
  submits `str`.
- Enter on `/find` opens or focuses `find` with an empty search box and does not
  start a search.
- A successful owned submission clears the launcher input but leaves `main`
  visible.
- A failed owned submission leaves the launcher input intact and displays a
  fixed failure message.
- Completion is ownership-checked. A late success for submission A cannot clear
  a newer edit or submission B, and a late failure for A cannot attach an error
  to B.
- There is exactly one `find` window per UiPilot process. Repeated submissions
  never create another window.
- A forward replaces only the search text. It preserves category, sort, preview
  preference, window geometry, and pin state.
- A forward shows and focuses the existing window even when it is already
  visible.
- Both windows may remain visible and usable. Windows permits only one
  keyboard-focused window at a time.
- During the `main` to `find` focus handoff, the exact resulting `main` blur is
  suppressed and `main` is lowered from always-on-top. It remains visible
  without covering the non-topmost `find` window.
- A later genuine `main` blur retains the launcher's existing automatic-hide
  behavior. When `main` regains focus, it restores its always-on-top state.
- `find` never becomes always-on-top as part of the handoff or pin behavior.
- An old execution completion never hides a newer admitted query. If execution
  has already entered its native-hide phase, the latest new main forward waits
  and is displayed only after that phase terminates.

### Pin And Hide Behavior

- Pin state starts unpinned on every process start and is retained only for that
  process lifetime.
- Pinning prevents automatic hiding. It does not make the window always-on-top.
- When unpinned, losing focus automatically hides `find`.
- When unpinned, successfully executing a selected current result automatically
  hides `find`.
- When pinned, losing focus or successfully executing a selected current result
  leaves `find` visible and preserves its query and results.
- When pinned, Escape is inert.
- The close button always hides `find`, whether pinned or unpinned.
- Closing or hiding never destroys `find`.

## Architecture

### Precreated Windows

Tauri configuration declares two windows:

- `main`: the existing launcher window.
- `find`: a hidden, resizable, undecorated file-search window created once
  during application startup.

`find` defaults to approximately 900 by 600 logical pixels with a minimum size
of approximately 720 by 420. The first version does not persist its size or
position across process restarts. Hiding and showing the precreated window
naturally preserves its geometry during one run.

### FindWindowController

A Rust-managed `FindWindowController` is the single owner of native `find`
lifecycle state. Its explicit admission states are `NotReady`,
`PreparedNotReady(token)`, `Hidden`, `Transferring`, `VisibleReady`, and
`HidingForExecution(ticket)`.

It owns the in-memory pin state, readiness token, confirmed focus edges, current
find invocation, and at most one complete queued open transaction containing
payload, prepared main-retirement lease, deadline, and async waiter.

Replacing a queued transaction completes the old waiter as `superseded` and
drops its lease. Timeout and shutdown complete waiters with a fixed unavailable
outcome. No waiter is left without exactly one terminal result.

Only `VisibleReady` with a matching active invocation admits file search, pin,
explicit hide, or result execution. The readiness path is exactly
`NotReady -> PreparedNotReady(token) -> Hidden`; a new prepare replaces the
token while remaining `PreparedNotReady`. An uncommitted token expires after
five seconds and returns admission to `NotReady`. Only fresh successful
`on_show` moves `Hidden -> Transferring -> VisibleReady`; successful hide always
returns to `Hidden` after closing the find scope.

Native focus-loss handling consults Rust-owned pin and admission state. The
WebView cannot bypass native decisions by changing JavaScript state.

### Native Main-To-Find Focus Transfer

`open_find_window` uses one serialized native transfer transaction. A transfer
ID identifies controller state and waiters; native `Focused(bool)` events do not
and cannot carry that ID.

Before native work, the controller records the last confirmed focus state for
both labels and takes fresh `is_focused` plus Windows foreground-window
snapshots. A transfer may start only from a snapshot consistent with its actual
precondition. It enters `Transferring { transfer_id, phase }`, lowers
`main`, shows `find`, and requests focus without holding the controller lock.

Each window listener reports only `{ label, focused }`. Under the controller
lock, the handler:

1. rejects duplicate values that are not a focus edge;
2. records the new confirmed edge for that label;
3. checks the active transaction phase;
4. takes or schedules a fresh native focus/foreground snapshot;
5. advances the transaction only when the observed edges and snapshot together
   prove `main` is unfocused, `find` is focused, and the foreground window is
   `find`.

A delayed event from an older operation cannot complete the current transfer
when the native snapshot contradicts it. If the snapshot already proves the
required final state, accepting the edge is safe regardless of which queued
notification caused the handler to recheck it. Repeated identical events are
idempotent.

Focus handlers notify an async waiter through a one-shot signal. The command
waits up to two seconds in production; tests inject a shorter deterministic
deadline. No native call or async wait occurs while holding the controller lock,
and the event handler never waits for a lock held by the command.

After success, `main` remains visible and non-topmost while `find` has focus. A
confirmed `main Focused(true)` edge restores `main` to always-on-top. Any later
confirmed and unsuppressed `main Focused(false)` follows the existing
clear-and-hide path. Hiding `find` while `main` is not focused leaves `main`
visible and non-topmost; focusing `main` restores its normal topmost state.

Rollback is phase-specific. Before find ownership commits, any lowering, show,
focus, observation, or timeout failure invalidates the transfer waiter and
best-effort restores the captured native visibility, focus, and topmost state.
After find ownership commits, old find authorization no longer exists; a later
emit failure must clear the new find scope and keep `find` hidden. It may restore
`main` focus/topmost state, but it never restores a previously visible stale
find UI. Rollback failure fails both scopes closed and returns the fixed window
failure.

### Window-Scoped Result Authorization

The current `ResultRegistry` is a single active result slot. Sharing it between
`main` and `find` would let either window invalidate the other window's result
IDs. The new design introduces explicit window-scoped authorization contexts:

- the `main` scope owns application and plugin query results;
- the `find` scope owns file query results.

Each scope preserves the existing scope generation, invocation, query sequence,
domain epoch, opaque ID, and stale-result rules. Beginning or hiding a query in
one scope never changes the other scope. Global ID allocation remains unique
and checked.

Commands select the scope from an already validated caller window label. A
caller cannot supply or spoof a scope parameter.

Any operation needing both lifecycle admission and find authorization acquires
the controller before the find registry and releases them before native or
async work. No path acquires them in the reverse order. Main retirement
preparation releases the main registry before entering the controller, and
readiness releases its settings snapshot before entering the controller.

Every published result mapping receives a checked result-set generation inside
its scope. Resolving a file action returns the Rust-owned action together with
an `ExecutionTicket` that binds the `find` scope, scope generation, invocation,
result-set generation, request ID, and result ID. The ticket is never serialized
to the WebView.

The main scope exposes `retire_application_query_if_current`. Under the registry
lock it compares the captured main invocation, application query sequence, and
application domain. On a match it advances the application domain epoch and
clears the current application mapping while leaving the main invocation active
for later searches. A mismatch changes nothing. It never changes plugin
ownership or the `find` scope.

### Narrow Find Preferences

`find` does not receive the full settings DTO or general settings commands. The
prepare phase returns only:

```ts
interface FindInitializationPrepared {
  initializationToken: string
  themeRevision: string
  theme: 'system' | 'dark' | 'light'
  filePreviewRevision: string
  filePreviewEnabled: boolean
  pinned: boolean
}
```

Theme revision and file-preview revision are independent checked Rust `u64`
counters serialized as canonical decimal text. Each field is reconciled only
against its own revision, so a theme update cannot suppress or overwrite a
preview completion and vice versa. Initialization tokens are opaque checked
Rust identifiers used only by the two-phase readiness handshake.

A separate find-only preview command accepts exactly `{ enabled: boolean }`,
persists only `filePreviewEnabled`, and returns:

```ts
interface FindPreviewPreferenceResult {
  filePreviewRevision: string
  filePreviewEnabled: boolean
}
```

It uses the existing critical-operation reservation and fixed settings error
mapping. The toggle retains its last durable value, disables while its write is
pending, commits only an owned result with a newer file-preview revision, and
rolls back an owned failure. A late completion cannot overwrite a newer field
revision.

When `main` durably changes theme, Rust emits:

```ts
interface FindThemeChanged {
  themeRevision: string
  theme: 'system' | 'dark' | 'light'
}
```

`find` accepts only one of the three theme values and a newer theme revision.
This keeps explicit and system themes correct without granting full settings
access.

## Frozen Terminology And Wire Contract

- **Window scope**: the Rust-selected `main` or `find` authorization context,
  derived only from a validated caller label.
- **Scope generation**: a checked Rust counter that invalidates every invocation
  and result mapping in exactly one window scope when that scope is hidden or
  failed closed.
- **Transfer ID**: a checked Rust identifier for controller transaction state
  and async waiters. Native focus events do not carry it; confirmed focus edges
  plus a fresh native focus/foreground snapshot prove transaction progress.
- **Theme revision**: a checked Rust `u64` ordering only durable theme state.
- **File-preview revision**: a separate checked Rust `u64` ordering only durable
  file-preview state. Both revisions use canonical decimal serialization.
- **Invocation**: one Rust-created ownership lifetime inside one window scope. A
  committed forward creates a fresh `find` invocation.
- **Forward sequence**: a process-local checked Rust `u64` ordering main-to-find
  submissions. It is serialized as a canonical unsigned decimal string with no
  leading zeroes except `"0"`.
- **Query sequence**: a `FindView`-local integer ordering file searches inside
  one invocation. It starts at zero and command input accepts only integers from
  1 through `Number.MAX_SAFE_INTEGER`.
- **Result-set generation**: a checked Rust counter identifying one published
  result mapping inside one window scope.
- **Execution ticket**: the Rust-only lease bound to the exact window scope,
  invocation, scope generation, result-set generation, request ID, and result
  ID that authorized one file action.
- **Submission owner**: the frontend tuple of submission token, view epoch,
  query control key, and exact control value captured when `/find` is submitted.
- **Initialization token**: an opaque Rust identifier for one uncommitted or
  committed frontend readiness attempt. It is unrelated to invocation and
  forward sequence.
- **Queued open transaction**: one complete controller-owned unit containing a
  forward payload, prepared main-retirement lease, deadline, and waiter. It ends
  exactly once as forwarded, superseded, failed, timed out, or shut down.

The only Rust-to-JavaScript forward event DTO is:

```ts
interface FindForwardPayload {
  invocationId: string
  forwardSequence: string
  query: string
}

type OpenFindOutcome =
  | { status: 'forwarded' }
  | { status: 'superseded' }

type FindReadyOutcome =
  | { status: 'prepared'; initialization: FindInitializationPrepared }
  | { status: 'ready'; initializationToken: string }
  | { status: 'superseded' }
```

`invocationId` is a non-empty opaque Rust identifier. `forwardSequence` must
parse as a canonical decimal `u64` and be strictly newer than the last accepted
forward. `query` is the exact text after `/find ` extraction, or empty for
`/find`. A forward sequence never becomes a query sequence, and a query sequence
is never reused across invocations.

## Data Flow

### Find Readiness And Preference Reconciliation

Readiness is a listener-first, two-phase, retryable handshake:

1. `FindView` creates its core with controls non-interactive and registers
   forward and theme listeners. Partial listener failure unregisters everything
   and never calls readiness commands.
2. It calls `prepare_find_initialization()`. Rust snapshots narrow preferences,
   releases settings state, creates a five-second initialization token, and
   changes `NotReady -> PreparedNotReady(token)`. A newer prepare replaces the
   old token while staying `PreparedNotReady`; the old token becomes
   `superseded`. No queued forward is removed, returned, emitted, or woken.
3. The frontend parses and reconciles the prepared snapshot. Lost/malformed
   response keeps listeners installed and retries prepare; expiry returns the
   controller to `NotReady` unless a newer preparation owns the state.
4. After successful parse, it calls
   `commit_find_ready({ initializationToken })`. A valid current token changes
   `PreparedNotReady(token) -> Hidden` and wakes the latest queued open
   transaction. That transaction performs the only handoff/on_show/emit/
   retirement flow.
5. Commit is idempotent for the retained committed token. If its response is
   lost, the frontend keeps listeners and retries commit or calls
   `get_find_ready_status({ initializationToken })`. Both return `ready` for the
   committed token and `superseded` for replaced, expired, or unknown tokens.
6. Only confirmed `ready` enables controls. Events may arrive before a retry
   response, so forward sequence and field revisions still reconcile
   independently.

Frontend destruction performs only best-effort local listener/core cleanup; it
does not claim to cancel server state. The next prepare supersedes an old token,
token expiry returns to `NotReady`, and process shutdown drops every token,
lease, and queued waiter.

### Launcher Submission

1. `main` captures the `/find` submission owner and main query ownership.
2. Rust validates `main`, prepares conditional retirement, and allocates checked
   forward/invocation IDs before side effects.
3. In `NotReady`, `PreparedNotReady`, `Transferring`, or
   `HidingForExecution`, the complete open transaction enters the single latest
   queue. In `Hidden`, it may start transfer immediately. `VisibleReady` starts
   a serialized replacement transfer for the same window.
4. Queueing C replaces B: B completes `superseded`, its lease is dropped, and it
   is never emitted or retired. Its stale frontend completion is inert.
5. A queued transaction has a five-second deadline. Timeout or shutdown removes
   it, drops its lease, and completes its waiter with fixed unavailable.
6. From `Hidden` admission, the selected transaction enters `Transferring` and
   performs the native focus handoff.
7. Rust commits find `on_show(invocationId)` before emit. Emit failure clears the
   new scope, leaves admission `Hidden`, keeps find hidden, drops retirement,
   and returns failure.
8. Successful emit is followed by non-failing prepared main-retirement commit.
9. The waiter completes `forwarded` only after retirement. Only the current
   submission owner clears launcher input; superseded or stale completions are
   inert.

Every forward, including one queued before readiness, follows this flow.
Initialization commands never carry or commit a forward.

### Find Query

1. `FindView` accepts only a payload with a valid invocation ID, canonical
   forward sequence, and sequence newer than the last accepted forward.
2. Accepting a forward installs its invocation, resets local query sequence to
   zero, replaces search text, and retains category, sort, preview preference,
   and pin presentation.
3. Empty text clears current file results without calling `search_files`.
4. Non-empty text increments the checked local query sequence and starts the
   existing Everything search with the payload invocation and that query
   sequence.
5. Every later text or category edit increments the same local query sequence.
6. Reaching `Number.MAX_SAFE_INTEGER` fails the current invocation closed and
   requires a new forward; it never sends an unsafe JavaScript integer.
7. Starting a query immediately invalidates the prior file result mapping in
   the `find` scope.
8. Existing owner-token checks reject late responses.

### Result Execution

1. `find` sends opaque request and result IDs to `execute_result`.
2. Rust validates the caller and atomically resolves the action plus its
   `ExecutionTicket` from the active `find` scope.
3. Existing path identity revalidation and Shell execution run without holding
   registry or controller locks. A stale lease may safely finish its already
   authorized Shell operation.
4. After Shell success, Rust takes controller then find registry locks. It
   verifies the ticket is current and samples pin state.
5. A stale or pinned ticket returns success without lifecycle mutation.
6. A current unpinned ticket atomically enters `HidingForExecution(ticket)` and
   retires exactly that result set before either lock is released.
7. This phase rejects local search, pin, explicit hide, and execution admission.
   A main forward may only enter the latest-only queue with the supersede and
   waiter rules defined above.
8. Native hide runs without locks. Its expected `find Focused(false)` edge is
   recorded once and performs no second clear or hide.
9. On hide success, Rust reacquires controller then find registry, confirms the
   phase, executes find-scope `hide_and_clear` (advancing scope generation and
   invalidating the invocation and every mapping), and changes admission to
   `Hidden` before releasing either lock. Hidden WebView commands cannot search.
10. If a queued forward exists, only its fresh `on_show` reactivates the scope
    and changes admission from `Hidden` to `VisibleReady`. Otherwise the window
    remains hidden and inactive.
11. On hide failure, the invocation remains active but the ticket result set is
    already retired. Rust processes the latest queued forward before returning
    to `VisibleReady` admission; without one, the visible frontend reports the
    fixed error and must issue a new query under the still-active invocation.

No query can be admitted between ticket validation and hide completion, and no
hidden invocation remains active after successful hide.

## Frontend Structure

The current file-mode responsibilities move out of the launcher state machine
into a dedicated file-search core and `FindView`. `main` retains only `/find`
recognition, forwarding, and owner-checked completion. It no longer renders the
embedded file result interface.

`FindView` contains:

- a search input in the top bar;
- an icon-only pin toggle with a tooltip and `aria-pressed` state;
- an icon-only close button with a tooltip;
- the existing category sidebar;
- the existing result list and keyboard navigation;
- the existing optional metadata preview;
- the existing status and error presentation.

The pin uses the repository's enabled icon library and has an explicit selected
state. It does not change native always-on-top state. Escape requests explicit
hide when unpinned and is inert when pinned. Close always requests forced hide.

`FindView` follows the listener-first initialization handshake before rendering
interactive controls. It reconciles initialization, forward events, theme
events, and preview command results by their independent sequence or field
revision, and uses the dedicated preview command for persistence.

## Commands And Capabilities

The command boundary is split by caller:

- `open_find_window(input) -> OpenFindOutcome`: main-only; input contains query,
  captured main invocation, and application query sequence.
- `prepare_find_initialization() -> FindReadyOutcome`: find-only; returns
  `prepared` with `FindInitializationPrepared`.
- `commit_find_ready({ initializationToken }) -> FindReadyOutcome`: find-only,
  idempotent for the current committed token.
- `get_find_ready_status({ initializationToken }) -> FindReadyOutcome`:
  find-only and read-only; returns `ready` only for the retained committed token,
  otherwise `prepared` for the current uncommitted token or `superseded`.
- application, general settings, and plugin commands: main-only.
- file search/execution, pin, preview update, and explicit hide: find-only and
  admitted only by the applicable controller state.

Every command guards the exact label before state access. The `find` capability
includes only file search/execution, these three readiness commands, pin,
preview update, explicit hide, and minimum event-listen permissions. It does not
receive application search, full settings, hotkey, autostart, plugin management,
or launcher-hide permissions.

## Failure Behavior

- Counter or retirement preparation exhaustion fails before queueing, native
  side effects, emit, or launcher clearing.
- Malformed, noncanonical, stale, or duplicate forward payloads are ignored.
- Partial listener registration fails before readiness preparation and cleans up
  all listeners.
- Lost or malformed prepare responses leave Rust in `PreparedNotReady` and listeners intact;
  the frontend retries prepare. Lost ready-commit responses retry idempotently
  or query ready status without unregistering listeners.
- Superseding or expiring an uncommitted preparation has no queue effect. Shutdown
  drops tokens and leases and completes every queued waiter.
- Replacing queued B with C immediately completes B as `superseded`, drops B's
  lease, and never emits or retires for B.
- Native pre-ownership failure restores captured state. Emit failure after
  ownership clears the new find scope and keeps `find` hidden.
- Main retirement commit is non-failing after emit and cannot clear newer main,
  plugin, or find ownership.
- Stale launcher completion cannot clear or report into newer frontend state.
- Invalid caller labels fail before controller, registry, settings, filesystem,
  or Shell access. Invalid, hidden-scope, stale, or cross-scope result IDs retain
  fixed path-free errors.
- A stale execution ticket cannot enter `HidingForExecution`.
- During `HidingForExecution`, ordinary admission stays closed and only the
  latest complete main open transaction may queue.
- Programmatic focus loss is consumed once by the expected-hide phase.
- Hide success executes find `hide_and_clear`, advances scope generation,
  invalidates the invocation, and leaves admission `Hidden` before processing a
  queued forward.
- Hide failure retains an active invocation but retired current IDs, processes a
  queued forward before reopening admission, and otherwise requires a new query.
- Preview failure restores the last durable field; late preview or theme results
  cannot overwrite a newer revision of that field.
- Process shutdown owns final destruction; ordinary close never destroys
  `find`.

## Testing

### Frontend

- `/find str` clears only on owned `forwarded`; `superseded` and every stale
  completion are inert.
- Submission A completion after edit/submission B cannot clear or report into B.
- Forward and theme listeners register before prepare; partial registration
  failure cleans up before readiness contact.
- Prepare response loss/parse failure retries while retaining listeners and
  controls remain disabled.
- Readiness commit response loss retries the same token or checks status; no listener
  teardown or duplicate ready transition occurs.
- Initialization responses contain no pending forward. First forward arrives
  only through the registered event after ready commit.
- Event/response orders converge independently by forward sequence, theme
  revision, and file-preview revision.
- Valid newer forwards reset local query sequence; malformed, duplicate, stale,
  and overflow values are ignored.
- Query sequence never exceeds `Number.MAX_SAFE_INTEGER`.
- Controls are non-interactive before confirmed ready and while execution is
  pending.
- Unpinned Escape requests hide; pinned Escape is inert; close follows admission.
- Preview success returns a field revision; failure rolls back; stale field
  completions are inert.
- Existing category, list, preview, keyboard, stale-response, and error tests
  move to the dedicated file-search core without losing coverage.

### Rust

- The controller has the frozen admission states and resolves exactly one
  precreated find window.
- Prepare enters `PreparedNotReady` and never drains or emits a queued forward; commit
  is idempotent and only commit wakes the latest original open transaction.
- Lost prepare/commit responses, token supersession/expiry, readiness
  timeout, and shutdown give every token and waiter a terminal state.
- Queue B replaced by C completes B `superseded`, drops B's retirement lease,
  emits only C, and commits retirement only for C when current.
- Focus edges plus fresh native snapshots complete transfers; duplicates and
  contradictory stale events are inert; async waiters never block UI or hold
  handler locks.
- Pre-commit rollback restores native state; post-ownership emit failure clears
  scope and keeps find hidden.
- Retirement preparation precedes native work; post-emit CAS cannot fail.
- Main and find scopes publish concurrently without invalidation; pending main A
  cannot publish or execute after a current successful forward.
- Local query B before execution-A completion makes A stale.
- Main forward B after A enters `HidingForExecution` queues until hide terminates.
- Search, pin, explicit hide, and second execution admission are closed during
  execution hide; expected programmatic blur is consumed once.
- Hide success calls find `hide_and_clear`, advances scope generation, enters
  `Hidden`, and rejects commands from the hidden old invocation.
- Only queued forward fresh `on_show` reactivates after hide success.
- Hide failure keeps the invocation active with inert old IDs and processes a
  queued forward before `VisibleReady` admission.
- Preference snapshots/events reconcile by independent field revision; narrow
  commands guard caller first.
- Capability and configuration tests enforce exact labels and permissions.

### Real Window Event Tests

A Windows-only native harness uses real Tauri window events, not closure-only
simulation, to verify:

- confirmed main blur plus find focus and a matching foreground snapshot complete
  one transfer even though events carry no transfer ID;
- duplicate and delayed events cannot complete a contradictory transaction;
- the handoff blur does not hide `main`;
- `main` is non-topmost during find focus and restores topmost on refocus;
- a later genuine blur hides `main`;
- timeout and focus failure release the async waiter and perform phase-correct
  rollback;
- an expected programmatic find blur during execution hide is recorded without a
  second clear or hide.

These tests do not synthesize user mouse or keyboard input. Any harness capable
of changing foreground focus is announced to the user before it runs.

### Verification Gates

- Focused frontend and Rust red-green tests.
- Full frontend test suite and production build.
- `cargo fmt --check`, full Rust tests, Clippy with warnings denied, and
  `cargo check`.
- Existing Everything IPC and security configuration checks affected by the
  capability change.
- Windows real-window event harness.
- Manual acceptance performed only by the user: open and update the singleton,
  switch focus between both windows, verify pin behavior, execute file and
  folder results, close and reopen the hidden window, and confirm no duplicate
  appears.

## Non-Goals

- Multiple file-search windows.
- Always-on-top behavior for `find`.
- Persisting pin state, geometry, or last query across process restarts.
- Granting `find` the full settings contract.
- Changing Everything query semantics, result limits, path authentication, or
  Shell execution.
- Adding pagination or background file refresh.
- Controlling the user's mouse or keyboard during verification.

## Acceptance

The feature is complete only when:

1. `/find str` leaves `main` visible and non-occluding, and clears only the
   current owned `forwarded` submission.
2. Listener-first prepare/commit readiness survives response loss, parse failure,
   retry, and shutdown without losing or double-delivering a forward.
3. Pending forwards are never returned by initialization; ready commit wakes the
   original latest transaction, which follows the one frozen handoff/on_show/
   emit/retirement path.
4. Replacing queued B with C completes B `superseded`, drops its lease, emits and
   retires only for C, and leaves no waiter hanging.
5. Focus transfer is proven by edges plus native snapshots without blocking the
   UI thread or relying on event-carried IDs.
6. Pre-ownership failure restores native state; post-ownership emit failure
   keeps find hidden and never restores stale UI.
7. Successful forwarding performs non-failing conditional main retirement
   without affecting newer main, plugin, or find ownership.
8. Unpinned current execution enters a closed admission phase; pinned or stale
   execution does not hide.
9. Hide success advances find scope generation, invalidates the invocation, and
   leaves `Hidden`; only queued forward fresh `on_show` reactivates it.
10. Hide failure cannot admit an intervening query and preserves only an active
    invocation with retired old IDs.
11. Pinned Escape is inert; close hides when admission permits; `/find` opens
    empty without searching.
12. `find` receives only narrow independently revisioned theme/preview state.
13. Automated verification passes and manual acceptance uses only user-operated
    mouse and keyboard.