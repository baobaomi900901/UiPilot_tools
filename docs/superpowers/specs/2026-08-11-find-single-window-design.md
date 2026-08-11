# Single-Instance `/find` Window Design

## Status

Draft revised on 2026-08-11 after independent review. It requires another
independent approval before implementation planning.

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
window lifecycle state. It owns:

- the in-memory pin state;
- frontend readiness;
- the latest monotonically sequenced pending forward payload;
- the current find invocation;
- serialized show, focus-transfer, explicit-hide, execution-completion, and
  focus-loss decisions.

Before frontend readiness, the controller keeps only the newest forward. Once
ready, it performs the native focus transfer and emits that payload. Old,
duplicate, malformed, or exhausted forward sequences cannot replace newer
state.

Native focus-loss handling consults the Rust-owned pin state. The WebView
cannot bypass the native decision by changing JavaScript state.

### Native Main-To-Find Focus Transfer

`open_find_window` uses one serialized native transfer transaction. A transfer
ID identifies controller state and waiters; native `Focused(bool)` events do not
and cannot carry that ID.

Before native work, the controller records the last confirmed focus state for
both labels and takes fresh `is_focused` plus Windows foreground-window
snapshots. A transfer may start only from a snapshot consistent with its actual
precondition. It enters `TransferringToFind { transfer_id, phase }`, lowers
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
initialization handshake returns only:

```ts
interface FindInitialization {
  themeRevision: string
  theme: 'system' | 'dark' | 'light'
  filePreviewRevision: string
  filePreviewEnabled: boolean
  pinned: boolean
  pendingForward?: FindForwardPayload
}
```

Theme revision and file-preview revision are independent checked Rust `u64`
counters serialized as canonical decimal text. Each field is reconciled only
against its own revision, so a theme update cannot suppress or overwrite a
preview completion and vice versa.

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

The only Rust-to-JavaScript forward event DTO is:

```ts
interface FindForwardPayload {
  invocationId: string
  forwardSequence: string
  query: string
}
```

`invocationId` is a non-empty opaque Rust identifier. `forwardSequence` must
parse as a canonical decimal `u64` and be strictly newer than the last accepted
forward. `query` is the exact text after `/find ` extraction, or empty for
`/find`. A forward sequence never becomes a query sequence, and a query sequence
is never reused across invocations.

## Data Flow

### Find Readiness And Preference Reconciliation

Readiness is a listener-first handshake:

1. `FindView` creates its core while controls remain non-interactive.
2. It registers both forward and theme listeners and retains their unlisten
   handles. Listener registration failure destroys the core and never marks the
   controller ready.
3. It calls the find-only initialization command.
4. Rust snapshots both preference fields and revisions, then releases settings
   state before acquiring the controller. Under the controller lock it removes
   at most one latest pending forward into the response and marks the frontend
   ready at one linearization point. The returned pending forward is not also
   emitted. Because listeners were registered first, a theme update committed
   between the snapshot and ready point is delivered as a newer event.
5. Any forward or theme change committed after that point uses the already
   registered listener. Such an event may arrive before the initialization
   promise resolves; frontend reconciliation therefore compares forward
   sequence, theme revision, and file-preview revision independently and keeps
   the newest value for each dimension.
6. The initialization response applies only fields or a pending forward newer
+   than values already accepted from events.

Initialization failure unregisters both listeners, destroys the provisional
core, leaves the controller not ready, and preserves only the latest pending
forward for a later clean initialization attempt.
### Launcher Submission

1. `main` recognizes `/find` or `/find ` followed by text.
2. It captures a submission owner containing a fresh token, current view epoch,
   query control key, exact value, main invocation, and application query
   sequence.
3. Rust validates the exact `main` caller before state access.
4. Before any native side effect, the main scope prepares a conditional
   retirement lease. On a current match it records the expected ownership and a
   checked next application domain epoch without applying it. A mismatch yields
   an explicit no-op lease. Epoch exhaustion aborts before native work or emit,
   returns the fixed search-unavailable error, and leaves the launcher input and
   current mapping unchanged.
5. The controller allocates a checked forward sequence and fresh opaque find
   invocation, then completes the native focus transfer.
6. Rust commits `find_scope.on_show(invocationId)` before emit, then emits
   `{ invocationId, forwardSequence, query }`.
7. If emit fails after `on_show`, Rust clears the new find scope and keeps
   `find` hidden. It performs only the post-commit rollback defined above and
   never restores the old find UI.
8. After successful emit, Rust commits the prepared main retirement lease under
   the main registry lock. The commit cannot exhaust: it either applies the
   precomputed epoch when ownership still matches or does nothing after a newer
   main query. It never changes plugin or find ownership.
9. The command returns success only after retirement commit.
10. Only the still-current submission owner may clear launcher input on success
    or publish a failure. A stale completion has no frontend effect.

The ordering is fixed: prepare retirement, native handoff, find ownership,
emit, non-failing retirement commit, command success, then owner-checked
frontend completion.
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
   `ExecutionTicket` from the `find` scope.
3. Existing path identity revalidation and Shell execution run without holding
   the registry or controller lock. A stale lease may safely finish its already
   authorized Shell operation.
4. After Shell success, Rust takes controller then registry locks in one fixed
   order. It verifies the ticket still matches the current scope generation,
   invocation, result-set generation, request ID, and result ID, then samples
   current pin state.
5. A stale ticket returns the successful execution outcome without lifecycle or
   authorization mutation. A current pinned ticket also returns without hide.
6. For a current unpinned ticket, the controller atomically enters
   `HidingForExecution(ticket)` before either lock is released and conditionally
   retires exactly that result set.
7. While this phase is active, `search_files`, local search submission, pin
   mutation, explicit hide, and result execution cannot enter. `FindView`
   disables its editable controls while execution is pending. A new main
   forward is accepted only into the controller's single latest-forward queue.
8. Native hide runs without locks while the admission phase remains visible to
   every command. Its resulting `find Focused(false)` edge is marked as the
   expected programmatic hide; the listener records the edge but performs no
   second clear or hide.
9. On hide success, the controller leaves the phase with `find` hidden and its
   ticket result retired, then processes the latest queued forward, if any.
10. On hide failure, `find` remains visible with inert old IDs. The controller
    leaves the phase and processes a queued forward before ordinary admission
    resumes; without a queued forward, the frontend reports the fixed window
    error and requires a new query.

No new query can begin between ticket validation and native hide completion.
Therefore execution A can neither hide nor clear a query B; B is queued and
shown only after A's hide transaction terminates.
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

- `open_find_window`: callable only by `main`; accepts query plus captured main
  invocation and application query sequence.
- application, general settings, and plugin commands: remain callable only by
  `main`.
- file search and file result execution: callable only by `find` and resolve
  only against the `find` scope.
- find readiness/preferences, pin update, preview preference update, and
  explicit find hide: callable only by `find`.

Every command performs its exact label guard before state access or side
effects. `find` capability includes only file search, result execution,
readiness/preferences, pin, preview update, explicit hide, and minimum event
listen permissions. It does not receive application search, full load/save
settings, hotkey, autostart, plugin management, or launcher-hide permissions.

## Failure Behavior

- Forward sequence or retirement preparation exhaustion fails before native
  side effects, emit, or launcher clearing.
- Malformed, noncanonical, stale, or duplicate forward payloads are ignored and
  never reset invocation or query state.
- Listener registration or initialization failure unregisters partial listeners,
  destroys provisional frontend state, and leaves Rust not ready with only the
  latest pending forward retained.
- Snapshot/event reordering is reconciled independently by forward sequence,
  theme revision, and file-preview revision.
- Invalid or exhausted local query sequences start no search and expose no IDs.
- Native lookup, topmost, show, focus, observation, or pre-commit failure restores
  captured native state and returns one fixed path-free window error.
- Emit failure after find ownership commit clears that scope and keeps `find`
  hidden; it never restores a visible stale UI.
- A failed transfer never commits the prepared main retirement or clears owned
  launcher input.
- Main retirement commit is non-failing after emit: it applies its precomputed
  epoch only on the captured CAS match or is a no-op for newer ownership.
- A stale launcher completion cannot clear or report into newer frontend state.
- Invalid caller labels fail before controller, registry, settings, filesystem,
  or Shell access.
- Invalid, stale, or cross-scope result IDs retain fixed path-free errors.
- A stale execution ticket may finish its authorized side effect but cannot
  enter `HidingForExecution` or mutate current lifecycle or authorization.
- While `HidingForExecution` is active, search and lifecycle admission remains
  closed until native hide succeeds or fails; only the latest main forward may
  queue.
- Programmatic hide focus loss is consumed once by the expected-hide phase and
  cannot trigger a second clear.
- Hide failure retains a visible window with retired inert IDs, processes any
  queued forward before reopening admission, and otherwise returns a fixed
  error requiring a new query.
- Preview failure restores the last durable preview field; late preview or theme
  completions cannot overwrite a newer revision of the same field.
- Hiding `find` invalidates only file results. Hiding `main` invalidates only
  main results.
- Process shutdown owns final destruction; ordinary close never destroys
  `find`.
## Testing

### Frontend

- `/find str` forwards `str`, clears only on owned success, and never enters
  embedded file mode.
- Submission A success or failure after edit/submission B cannot clear or report
  into B.
- `/find` forwards empty text and performs no file search.
- Forward and theme listeners are registered before initialization; partial
  registration or initialization failure cleans up and does not mark ready.
- Event-before-response and response-before-event orders converge independently
  for forward sequence, theme revision, and file-preview revision.
- A pending forward returned by initialization is not applied twice.
- Valid newer forwards reset only local query sequence; malformed, overflow,
  duplicate, and stale forwards are ignored.
- Forwarded queries preserve category, sort, preview, and pin state.
- Query sequence never exceeds `Number.MAX_SAFE_INTEGER`.
- Controls remain non-interactive during initialization and execution pending,
  including the `HidingForExecution` phase.
- Unpinned Escape requests hide; pinned Escape is inert; close always forces
  hide outside a conflicting admission phase.
- Preview persistence returns a field revision; owned success commits, failure
  rolls back, and stale field completions are inert.
- Theme events accept only valid values and a newer theme revision.
- Existing category, list, preview, keyboard, stale-response, and error tests
  move to the dedicated file-search core without losing coverage.
### Rust

- The controller resolves exactly one precreated `find` window and never creates
  another instance.
- Before readiness, only the latest forward is retained and initialization
  returns it exactly once.
- Focus listeners update confirmed edges; duplicate events are inert, stale
  events with contradictory native snapshots cannot advance a transaction, and
  a snapshot already proving the target state completes safely.
- The async focus waiter never blocks the UI thread or holds a lock required by
  event handlers.
- Main lowering, refocus topmost restoration, genuine later blur hiding, timeout,
  and every pre-commit rollback point are deterministic.
- Find ownership is established before emit; emit failure clears new ownership,
  keeps find hidden, and never restores an old visible UI.
- Retirement preparation occurs before native work; exhaustion emits nothing;
  post-emit CAS commit cannot fail and preserves newer main ownership.
- Main and find scopes publish and resolve concurrently without invalidation.
- Pending app search A, successful `/find`, then late A publication and execution
  are rejected.
- `execute A pending -> local query B -> A success` makes A stale before hide.
- `execute A completion check -> main forward B -> native hide` queues B during
  `HidingForExecution` and shows it only after A hide terminates.
- Search, pin, explicit hide, and second execution admission are rejected during
  `HidingForExecution`; programmatic focus loss is consumed once.
- Pin off-to-on before completion keeps current A visible; pin on-to-off hides
  only if A remains current when the completion phase begins.
- Hide success/failure retires only A, cannot hide an admitted B, and processes
  the latest queued forward before reopening admission.
- Initialization snapshots racing theme events reconcile by field revision;
  preview updates return a durable file-preview revision and cannot overwrite
  theme state.
- Find preference commands guard the caller first and expose or mutate only the
  narrow fields.
- Capability and Tauri configuration tests enforce exact labels and minimum
  permissions.
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

1. `/find str` leaves `main` visible and non-occluding, clears only its owned
   input after success, and shows the one `find` window with `str`.
2. Listener-first initialization cannot lose the first forward or a concurrent
   theme update; field revisions reconcile every event/response order.
3. Repeated submissions update the same window and preserve filters, preview,
   and pin state.
4. Focus transfer is proven by confirmed edges plus native snapshots, never by
   an event-carried transfer ID; it does not block the UI thread or deadlock the
   event handler.
5. Pre-commit native failures restore captured state; post-ownership emit failure
   keeps find hidden and never restores a stale old UI.
6. Successful forwarding commits a prevalidated, non-failing conditional main
   retirement without affecting newer main, plugin, or find ownership.
7. Both window scopes remain concurrently usable.
8. Unpinned current execution hides `find`; pinned or stale execution preserves
   it.
9. `HidingForExecution` prevents any query from being admitted between ticket
   validation and hide completion, queues only the latest main forward, and
   consumes programmatic focus loss once.
10. Pinned Escape is inert, while close always hides when admission permits.
11. `/find` opens empty without issuing a search.
12. `find` receives only narrow independently revisioned theme/preview state and
    preview persistence authority.
13. Automated verification passes and the user completes manual acceptance
    without Codex controlling input devices.