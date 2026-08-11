# Single-Instance `/find` Window Design

## Status

Approved on 2026-08-11.

## Goal

Move file search out of the launcher into one dedicated Tauri window while preserving the existing Everything search, category filters, sort order, preview, opaque result authorization, and authenticated file execution.

The launcher remains visible after submitting `/find`. Repeated submissions reuse the same file-search window and replace only its query text.

## User Contract

### Opening And Forwarding

- Enter on `/find str` in the launcher opens or focuses the single `find` window and submits `str`.
- Enter on `/find` opens or focuses the `find` window with an empty search box and does not start a search.
- A successful submission clears the launcher input but leaves the launcher visible.
- A failed submission leaves the launcher input intact and displays a fixed failure message.
- There is exactly one `find` window per UiPilot process. Repeated submissions never create another window.
- A forwarded query replaces only the search text. It preserves category, sort, preview preference, window geometry, and pin state.
- Forwarding a query shows and focuses the existing window even when it is already visible.
- Both windows may remain visible and usable. Windows permits only one keyboard-focused window at a time.

### Pin And Hide Behavior

- The pin state starts unpinned on every process start and is retained only for that process lifetime.
- Pinning prevents automatic hiding. It does not make the window always-on-top.
- When unpinned, losing focus automatically hides the `find` window.
- When unpinned, successfully executing a selected result automatically hides the `find` window.
- When pinned, losing focus or successfully executing a selected result leaves the window visible and preserves its query and results.
- When pinned, Escape is inert.
- The close button always hides the window, whether pinned or unpinned.
- Closing or hiding never destroys the `find` window.

## Architecture

### Precreated Windows

Tauri configuration declares two windows:

- `main`: the existing launcher window.
- `find`: a hidden, resizable, undecorated file-search window created once during application startup.

The `find` window defaults to approximately 900 by 600 logical pixels with a minimum size of approximately 720 by 420. The first version does not persist its size or position across process restarts. Hiding and showing the precreated window naturally preserves its geometry during one run.

### FindWindowController

A Rust-managed `FindWindowController` is the single owner of native `find` window lifecycle state. It owns:

- the in-memory pin state;
- frontend readiness;
- the latest monotonically sequenced pending query;
- serialized show, focus, explicit-hide, and focus-loss decisions.

The controller keeps only the newest query received before frontend readiness. Once ready, it shows and focuses the window and emits the latest query with its sequence. Old or duplicate sequences cannot replace newer frontend state.

Native focus-loss handling consults the Rust-owned pin state. The WebView cannot bypass the native decision by changing local JavaScript state.

### Window-Scoped Result Authorization

The current `ResultRegistry` is a single active result slot. Sharing it between `main` and `find` would let either window invalidate the other window's result IDs. The new design introduces explicit window-scoped authorization contexts:

- the `main` scope owns application and plugin query results;
- the `find` scope owns file query results.

Each scope preserves the existing generation, invocation, query-sequence, domain-epoch, opaque-ID, and stale-result rules. Beginning or hiding a query in one scope never changes the other scope. Global ID allocation remains unique and checked.

Commands select the scope from an already validated caller window label. A caller cannot supply or spoof a scope parameter.

## Data Flow

### Launcher Submission

1. The launcher recognizes `/find` or `/find ` followed by text.
2. It calls `open_find_window` with the extracted query.
3. Rust verifies that the caller label is exactly `main` before reading state or causing side effects.
4. The controller assigns the next checked sequence and records the query as latest.
5. If the frontend is ready, the controller shows and focuses `find`, establishes a fresh file-search invocation, and emits the query payload.
6. If the frontend is not ready, the controller retains only that latest payload and delivers it after the readiness handshake.
7. The launcher clears its input only after the command succeeds.

### Find Query

1. `FindView` accepts only a query sequence newer than the last accepted sequence.
2. It replaces the search text while retaining category, sort, preview preference, and pin presentation.
3. Empty text clears current file results without calling `search_files`.
4. Non-empty text starts the existing Everything search with the `find` invocation and next query sequence.
5. Starting a new query immediately invalidates the previous file result set in the `find` authorization scope.
6. Late responses retain the existing owner-token checks and cannot overwrite a newer query.

### Result Execution

1. `find` sends its opaque request ID and result ID to `execute_result`.
2. Rust validates the caller label and resolves only against the `find` authorization scope.
3. Existing path identity revalidation and Shell execution remain unchanged.
4. On success, the controller hides and clears the `find` scope only when unpinned.
5. When pinned, successful execution leaves the current query, visible results, and authorization active.
6. Execution failure leaves the window visible and displays the existing mapped error.

## Frontend Structure

The current file-mode responsibilities move out of the launcher state machine into a dedicated file-search core and `FindView`.

`main` retains only `/find` command recognition and forwarding. It no longer renders the embedded file result interface.

`FindView` contains:

- a search input in the top bar;
- an icon-only pin toggle with a tooltip and `aria-pressed` state;
- an icon-only close button with a tooltip;
- the existing category sidebar;
- the existing result list and keyboard navigation;
- the existing optional metadata preview;
- the existing status and error presentation.

The pin button uses the repository's enabled icon library. Its selected state is visually explicit. Pinning does not change native always-on-top state.

Escape behavior is routed through the file-search core: it requests an explicit hide when unpinned and does nothing when pinned. The close button always requests a forced explicit hide.

## Commands And Capabilities

The command boundary is split by caller:

- `open_find_window`: callable only by `main`.
- application, settings, and plugin commands: remain callable only by `main`.
- file search and file result execution: callable only by `find`.
- pin update, find readiness, and explicit find hide: callable only by `find`.

Every command performs its exact window-label guard before state access or side effects. The `find` capability includes only file search, result execution, readiness, pin, explicit hide, and the minimum event permissions needed to receive query payloads. It does not receive application search, settings, plugin-management, or launcher-hide permissions.

## Failure Behavior

- Sequence exhaustion fails closed and does not emit or clear launcher input.
- Window lookup, show, focus, or event emission failure returns one fixed `windowFailed`-style error without paths, labels supplied by a caller, or native details.
- Frontend readiness failure retains at most the latest query and does not create another window.
- Invalid caller labels fail before controller, registry, filesystem, or Shell access.
- Invalid, stale, or cross-scope result IDs retain fixed path-free errors.
- Hiding `find` invalidates only file results. Hiding `main` invalidates only main results.
- Process shutdown owns final destruction; ordinary close requests never destroy `find`.

## Testing

### Frontend

- `/find str` forwards `str`, clears launcher input only on success, and never enters embedded file mode.
- Failed forwarding preserves the launcher command and shows the mapped error.
- `/find` forwards an empty query and performs no file search.
- A newer forwarded sequence replaces an older query; stale events are ignored.
- Forwarded queries preserve category, sort, preview preference, and pin state.
- Unpinned Escape requests hide; pinned Escape is inert.
- Close always requests forced hide.
- Unpinned successful execution requests automatic hide.
- Pinned successful execution retains query and results.
- Existing category, list, preview, keyboard, stale-response, and error tests move to the dedicated file-search core without losing coverage.

### Rust

- The controller precreates or resolves exactly one `find` window and never creates a second instance.
- Before readiness, only the latest query is delivered.
- Show, focus, and emission are ordered and failures fail closed.
- Unpinned focus loss hides; pinned focus loss does nothing.
- Pin state persists through hide/show and defaults to false in a new controller.
- Explicit close hides regardless of pin.
- Successful execution applies the pin-dependent hide rule.
- Main and find registry scopes can publish and resolve concurrently without invalidation.
- Cross-scope IDs and wrong-window callers are rejected before side effects.
- Capability and Tauri window configuration tests enforce exact labels and minimum permissions.

### Verification Gates

- Focused frontend red-green tests.
- Focused Rust red-green tests for controller, caller guards, and registry isolation.
- Full frontend test suite and production build.
- `cargo fmt --check`, full Rust tests, Clippy with warnings denied, and `cargo check`.
- Existing Everything IPC and security configuration checks affected by the capability change.
- Manual acceptance performed only by the user: open and update the singleton window, switch focus between both windows, verify pin behavior, execute file and folder results, close and reopen the hidden window, and confirm no duplicate window appears.

## Non-Goals

- Multiple file-search windows.
- Always-on-top behavior.
- Persisting pin state, geometry, or the last query across process restarts.
- Changing Everything query semantics, result limits, path authentication, or Shell execution.
- Adding pagination or background file refresh.
- Controlling the user's mouse or keyboard during verification.

## Acceptance

The feature is complete only when:

1. `/find str` leaves `main` visible, clears its input after success, and shows the one `find` window with `str`.
2. Repeated submissions update that same window and preserve its filters and pin state.
3. Both windows remain visible and their result authorizations do not interfere.
4. Unpinned focus loss and successful execution hide `find`.
5. Pinned focus loss and successful execution preserve the visible query and results.
6. Pinned Escape is inert, while close always hides.
7. `/find` opens an empty search box without issuing a search.
8. Automated verification passes and the user completes the manual acceptance steps without Codex controlling input devices.
