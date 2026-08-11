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

`open_find_window` uses one serialized native transfer transaction. Before any
operation that can move focus, it captures prior visibility, focus, and
always-on-top observations and enters a transfer-ID-bound `TransferringToFind`
state. That state reserves exactly the matching native `main Focused(false)`
event; it does not suppress unrelated later focus loss.

The transaction:

1. lowers `main` from always-on-top;
2. shows `find` if necessary;
3. requests focus for `find`;
4. waits for the matching `main Focused(false)` and `find Focused(true)`
   observations within a two-second production deadline; tests inject a shorter
   deterministic deadline;
5. establishes find ownership and emits the forward only after both
   observations prove the handoff.

The suppression reservation spans from before lowering `main` until the
matching focus events are observed or the transaction rolls back. The transfer
ID and expected window labels bind both events, so a delayed event from an old
transfer cannot suppress a later genuine blur.

After success, `main` remains visible and non-topmost while `find` has focus. A
`main Focused(true)` event restores `main` to always-on-top. Any subsequent
unsuppressed `main Focused(false)` follows the existing clear-and-hide path.
Hiding `find` while `main` is not focused leaves `main` visible and non-topmost;
focusing `main` restores its normal topmost state.

If lowering, showing, focusing, focus observation, ownership establishment, or
emission fails, the transaction invalidates its transfer ID and
performs a best-effort rollback to the captured native state: `find` returns to
its prior visibility, `main` regains its prior topmost state and focus when it
previously owned focus, and no main authorization or launcher input is retired.
If rollback itself fails, both result scopes fail closed and the command returns
the fixed window failure.

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

`find` does not receive the full settings DTO or general settings commands. A
narrow initialization command returns only:

```ts
interface FindPreferencesView {
  preferencesRevision: string
  theme: 'system' | 'dark' | 'light'
  filePreviewEnabled: boolean
  pinned: boolean
}
```

The command validates the `find` caller before reading `SettingsStore` or the
controller. A separate find-only command accepts exactly
`{ enabled: boolean }` and persists only `filePreviewEnabled`. It uses the
existing critical-operation reservation and fixed settings error mapping.

The preview toggle retains its last durable value, disables while a write is
pending, commits only on owned success, and rolls back on owned failure. A late
completion cannot overwrite a newer preferences revision.

When `main` durably changes theme, Rust emits a narrow theme-change event to
`find`:

```ts
interface FindThemeChanged {
  preferencesRevision: string
  theme: 'system' | 'dark' | 'light'
}
```

A preferences revision is a checked Rust `u64` serialized with the same
canonical decimal rules as a forward sequence. `find` accepts only a newer
revision and one of the three theme values. Thus explicit themes and `system`
behavior remain correct without granting `find` full settings access.

## Frozen Terminology And Wire Contract

- **Window scope**: the Rust-selected `main` or `find` authorization context,
  derived only from a validated caller label.
- **Scope generation**: a checked Rust counter that invalidates every invocation
  and result mapping in exactly one window scope when that scope is hidden or
  failed closed.
- **Transfer ID**: a checked Rust identifier for one native focus transaction;
  only native focus events carrying the current transfer ID may complete or
  suppress events for that transaction.
- **Preferences revision**: a checked Rust `u64` ordering the narrow find
  preference snapshot and theme events, serialized as canonical decimal text.
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

### Launcher Submission

1. `main` recognizes `/find` or `/find ` followed by text.
2. It captures a submission owner containing a fresh token, current view epoch,
   query control key, and exact control value. It also captures the current main
   invocation and application query sequence for conditional retirement.
3. It calls `open_find_window` with the query and captured main ownership.
4. Rust validates that the caller label is exactly `main` before state access.
5. The controller allocates the next checked forward sequence and a fresh opaque
   find invocation ID. Exhaustion fails before native or registry mutation.
6. The controller completes the native focus transfer.
7. Before emitting, Rust executes `find_scope.on_show(invocationId)` or an
   equivalent atomic ownership establishment. It then emits
   `{ invocationId, forwardSequence, query }`.
8. If emission fails after ownership establishment, Rust hides `find`, clears
   the new find ownership, rolls back the native transfer, and returns failure.
9. After successful emission, Rust conditionally retires the captured main
   application query. This comparison-and-retire occurs before command success
   is returned. A mismatch is a successful no-op because a newer main query
   already owns the scope.
10. Only the still-current submission owner may clear the launcher input on
    success or publish a failure message. A stale completion has no frontend
    effect.

The ordering is fixed: successful find ownership and emission, conditional main
retirement, command success, then owner-checked launcher clearing. A failed
transfer never retires main authorization.

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

1. `find` sends its opaque request ID and result ID to `execute_result`.
2. Rust validates the caller and atomically resolves the action plus its
   `ExecutionTicket` from the `find` scope.
3. Existing path identity revalidation and Shell execution run without holding
   the registry or controller lock. A stale lease may safely complete its
   already-authorized Shell operation.
4. After successful Shell completion, Rust enters one controller/registry
   completion linearization point. It checks that the ticket still matches the
   current find scope generation, invocation, result-set generation, request
   ID, and result ID, then reads the current pin state.
5. A stale ticket returns the successful execution outcome but cannot clear,
   hide, or otherwise mutate the newer find UI.
6. A current ticket with pin set clears and hides nothing.
7. A current ticket with pin unset conditionally retires exactly that result set
   and then requests native hide.
8. If native hide fails after conditional retirement, the window remains
   visible with inert old result IDs, the command returns the fixed window
   failure, and the frontend requires a new query. No newer result set can be
   cleared by this path.

Pin state is sampled only at step 4. Therefore a pin change while Shell work is
pending deterministically controls the completion behavior at that point.

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

`FindView` obtains `FindPreferencesView` during its readiness handshake before
rendering interactive controls. It listens for narrow theme updates and uses
the dedicated preview command for persistence.

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

- Forward sequence exhaustion fails closed before native or registry mutation.
- Malformed, noncanonical, stale, or duplicate forward payloads are ignored by
  `FindView`; they never reset invocation or query state.
- Invalid or exhausted local query sequences start no search and expose no
  result IDs.
- Native lookup, topmost change, show, focus, event observation, or emission
  failure follows the focus-transfer rollback contract and returns one fixed
  path-free window error.
- A failed transfer never retires main authorization or clears launcher input.
- Conditional main retirement cannot clear a newer main query and never touches
  `find` or plugin ownership.
- A stale launcher submission completion cannot clear or report into newer
  frontend state.
- Invalid caller labels fail before controller, registry, settings, filesystem,
  or Shell access.
- Invalid, stale, or cross-scope result IDs retain fixed path-free errors.
- A stale execution ticket may finish its authorized side effect but cannot
  mutate current window lifecycle or authorization.
- Hide failure after current execution fails closed by retaining a visible
  window with retired, inert result IDs and a fixed error.
- A preview preference failure restores the last durable value; late preference
  completions cannot overwrite newer state.
- Hiding `find` invalidates only file results. Hiding `main` invalidates only
  main results.
- Process shutdown owns final destruction; ordinary close never destroys
  `find`.

## Testing

### Frontend

- `/find str` forwards `str`, clears only on owned success, and never enters
  embedded file mode.
- Failed forwarding preserves the owned launcher command and shows the mapped
  error.
- Submission A success after edit/submission B cannot clear B.
- Submission A failure after edit/submission B cannot report into B.
- `/find` forwards empty text and performs no file search.
- Valid newer forwards replace the query and reset only local query sequence.
- Malformed decimal, overflow, duplicate, and stale forwards are ignored.
- Forwarded queries preserve category, sort, preview preference, and pin state.
- Query sequence never exceeds `Number.MAX_SAFE_INTEGER`.
- Unpinned Escape requests hide; pinned Escape is inert; close always forces
  hide.
- Pinned execution retains current query and results.
- Find preference initialization exposes only theme, preview, and pin.
- Preview persistence success commits; failure rolls back; stale completions are
  inert.
- Theme events accept only valid values and a newer preferences revision.
- Existing category, list, preview, keyboard, stale-response, and error tests
  move to the dedicated file-search core without losing coverage.

### Rust

- The controller resolves exactly one precreated `find` window and never creates
  another instance.
- Before readiness, only the latest forward is delivered.
- Focus transfer lowers `main`, suppresses exactly the matching blur, observes
  both native events, and leaves both windows visible.
- A later real `main` blur hides normally; `main` refocus restores topmost.
- Every native failure point rolls back prior visibility, focus, and topmost
  observations; rollback failure fails both scopes closed.
- Find ownership is established before emit; emit failure hides and clears that
  ownership.
- Forward and result-set counter exhaustion fail before partial mutation.
- Main and find scopes publish and resolve concurrently without invalidation.
- Pending app search A, successful `/find`, then late A publication and
  execution are rejected.
- Conditional main retirement mismatch preserves a newer main query and never
  changes plugin or find ownership.
- `execute A pending -> forward B -> A success` leaves B visible and authorized.
- Pin off-to-on before A completion keeps current A visible; pin on-to-off hides
  only when A's ticket remains current.
- Hide failure retires only the ticket's current result set and cannot clear a
  newer result set.
- Find preference commands guard the caller first and expose or mutate only the
  narrow fields.
- Capability and Tauri configuration tests enforce exact labels and minimum
  permissions.

### Real Window Event Tests

A Windows-only native harness uses real Tauri window events, not closure-only
simulation, to verify:

- `main Focused(false)` and `find Focused(true)` complete one transfer;
- the handoff blur does not hide `main`;
- `main` is non-topmost during `find` focus and restores topmost on refocus;
- a later genuine blur hides `main`;
- timeout and focus failure release suppression and perform rollback;
- a delayed old focus event cannot consume a newer transfer reservation.

These tests do not synthesize user mouse or keyboard input.

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
2. Failed or out-of-order forwards preserve newer launcher input and errors.
3. Repeated submissions update the same window with the frozen payload and
   preserve filters, preview preference, and pin state.
4. Focus transfer suppresses exactly its own main blur, downgrades and restores
   main topmost state correctly, and rolls back every failure point.
5. Both window scopes remain concurrently usable; successful forwarding
   conditionally retires only the captured main application query.
6. Unpinned focus loss and current execution hide `find`; pinned focus loss and
   current execution preserve it.
7. A stale execution ticket can complete its authorized Shell operation but
   cannot hide, clear, or mutate a newer query.
8. Pinned Escape is inert, while close always hides.
9. `/find` opens an empty search box without issuing a search.
10. `find` receives only narrow initial preferences and preview persistence
    authority, while theme updates remain correct.
11. Automated verification passes and the user completes manual acceptance
    without Codex controlling input devices.
