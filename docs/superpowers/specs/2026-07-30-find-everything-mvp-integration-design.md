# /find Everything MVP Integration Design

## Status

Approved design for the first in-application Everything milestone.

This specification deliberately narrows the previously approved managed-runtime migration. It delivers the shortest safe path from the validated Query2 client to UiPilot /find. Installer, Service, ACL, owner, multi-user, automatic runtime management, pagination, and background refresh remain outside this milestone.

## Goal

Replace the production /find search call with the validated Everything Query2 client while preserving:

- ordinary filename-contains search;
- at most 200 results;
- modified-time descending order;
- result selection and metadata preview;
- Enter on a file reveals and selects it in Explorer;
- Enter on a folder opens it;
- Rust-owned opaque result actions and stale-result rejection.

The existing Rust FileIndex implementation remains in the repository and continues to compile, but production file search does not call it and never falls back to it.

## User Contract

### Search

- /find searches after each non-empty text edit using the existing invocation ID and query sequence.
- Input is plain text. Everything operators, wildcards, functions, macros, and advanced syntax are never exposed.
- A search returns at most 200 authenticated entries.
- Results are ordered by date modified descending.
- There is one Query2 request per search. There is no offset pagination or cutoff tie-group algorithm in this milestone.
- Category is fixed to all.
- Sort is fixed to modifiedDesc.
- The category strip and sort control are not rendered.
- The existing metadata preview remains available because it only consumes returned result metadata.

### Availability

- UiPilot does not start, install, configure, elevate, or repair Everything.
- UiPilot connects only to the default Everything 1.4 instance.
- If Everything is not running, cannot be reached, times out, overloads, or violates the protocol, the current search fails with the existing user-facing unavailable state.
- UiPilot does not fall back to FileIndex.
- There is no automatic retry timer. The next text edit or a new /find invocation may retry.

### Refresh

- UiPilot does not subscribe to file-index://changed for this backend.
- UiPilot does not poll Everything.
- A visible result set remains unchanged until the user edits the query, leaves /find, or starts a new invocation.
- Filesystem changes become visible after a new search.

## Architecture

### Reuse the Query2 Library In Process

src-tauri/Cargo.toml adds a path dependency on ../spikes/everything-ipc. The existing library remains the single implementation of:

- Query2 wire encoding;
- hidden reply-window lifecycle;
- WM_COPYDATA admission;
- one-active-request scheduling;
- bounded query admission;
- reply-contract validation;
- deadlines and overload behavior.

Tauri never invokes the CLI executable as a subprocess.

### EverythingSearchState

A new production state owns the reusable default-instance client:

    EverythingSearchState {
        client: Mutex<Option<Arc<EverythingClient>>>,
        revision: AtomicU64,
    }

Behavior:

- Connection is lazy so UiPilot startup succeeds when Everything is absent.
- A missing client connects with a 250 ms connection timeout.
- A cached client performs Query2 with a 1 second absolute deadline.
- Search does not hold the client mutex while waiting for Query2.
- IpcUnavailable, IpcSendFailed, ClientClosed, and Protocol errors evict the cached client.
- QueryTimedOut fails the current search without mandatory eviction.
- Overloaded fails only the current search and preserves the healthy client.
- A later search may reconnect after eviction.
- revision uses checked increment. Exhaustion permanently fails file search closed for the current process.

### Literal Query Encoding

Everything 1.4 treats spaces and characters such as |, !, angle brackets, quotes, asterisks, and question marks as syntax.

The adapter encodes every Unicode scalar value as an Everything 1.4 hexadecimal character entity:

    #x<HEX>:

Concatenating these entities creates one literal filename term. Empty input is not sent. Tests cover ASCII, spaces, Chinese text, operator characters, wildcard characters, and mixed input against an isolated Everything 1.4 instance.

Official reference:

- https://www.voidtools.com/support/everything/searching/

### Fixed Query Contract

Each search sends:

    offset = 0
    max_results = 200
    request_flags = 0x155
    sort = date modified descending
    deadline = now + 1 second

The validated IPC layer requires the reply offset, item count, request flags, and sort type to match this originating request before allocating item storage.

### File Search Module Boundaries

The target structure is:

    src-tauri/src/file_search/mod.rs
    src-tauri/src/file_search/everything.rs
    src-tauri/src/file_search/windows/mod.rs
    src-tauri/src/file_search/windows/path_auth.rs

Responsibilities:

- file_search/mod.rs owns backend-neutral search DTOs, result kinds, execution outcomes, and file action enums.
- file_search/everything.rs owns literal encoding, client caching, Query2 execution, metadata conversion, and Everything result authentication.
- file_search/windows/path_auth.rs owns path identity capture, component pinning, reparse rejection, final identity validation, and Shell dispatch.
- file_index remains responsible for the legacy index implementation and delegates shared Windows execution helpers to file_search/windows/path_auth.rs.

No shared path-authentication block is copied.

## Result Model

### Backend-Neutral Action

ResultRegistry stores:

    FileExecutionAction::Indexed(OpenIndexedPath)
    FileExecutionAction::Everything(EverythingPathAction)

ResultAction contains one file-action variant instead of requiring all file results to be OpenIndexedPath.

EverythingPathAction is Rust-owned and is never serialized to the WebView. It captures:

- display full path;
- normalized volume GUID path;
- normalized relative path;
- volume serial;
- 128-bit file ID;
- file or directory kind.

Canonical path is part of entry identity. Distinct hard-link paths remain distinct actions even when they share volume serial and file ID.

### Published View

The WebView receives only:

- opaque requestId;
- opaque resultId;
- name;
- file or folder kind;
- optional sizeBytes;
- modifiedUtc;
- fullPath for display;
- indexRevision compatibility value;
- status ready;
- total equal to the number of authenticated entries actually published.

Everything database total is not exposed because this milestone does not paginate and may discard unauthenticated entries.

indexRevision is the decimal serialization of EverythingSearchState revision. A fully authenticated successful batch consumes the next checked revision immediately before ResultRegistry publication. A superseded batch may consume a revision without publishing it; published revisions therefore remain strictly increasing but need not be contiguous. Failed searches do not allocate a revision.

## Search Flow

1. require_main_window remains the first command admission check.
2. The command validates invocation, query sequence, category all, and sort modifiedDesc.
3. ResultRegistry begin_query invalidates the previous user-query result set.
4. spawn_blocking calls EverythingSearchState outside the WebView thread.
5. The adapter encodes the query literally and sends one Query2 request.
6. Each returned entry is normalized and authenticated under the UiPilot process token.
7. Entries that disappeared, changed type, contain invalid paths, resolve through a reparse point, or cannot be authenticated are omitted.
8. Preserving Everything order, at most 200 authenticated drafts are produced.
9. EverythingSearchState allocates the next checked revision.
10. ResultRegistry publish_if_latest publishes only if the invocation, query sequence, and file domain are still current.
11. A stale or superseded query returns null and never replaces newer results.

There is no background refresh transaction in this milestone because every replacement is caused by an explicit user query edit.

## Windows Path Authentication

### Publication Authentication

Before an Everything item becomes visible, UiPilot opens and inspects it under the current process token to capture:

- final normalized volume GUID path;
- volume serial;
- file ID;
- final file/directory type;
- normalized relative path.

Publication authentication walks every component with the same strict no-reparse policy used by execution. Publication handles are released after identity capture; execution performs a fresh walk and holds its handles through Shell dispatch.

Authentication failure removes only that item. The rest of the result set may still publish.

### Execution Authentication

Enter execution performs these steps:

1. Resolve requestId/resultId from ResultRegistry.
2. Reopen every path component from the authenticated volume root with FILE_FLAG_OPEN_REPARSE_POINT.
3. Inspect each opened component and reject it if FILE_ATTRIBUTE_REPARSE_POINT is present at inspection.
4. Share only FILE_SHARE_READ | FILE_SHARE_WRITE, intentionally omitting FILE_SHARE_DELETE.
5. Validate each resolved component remains on the expected volume and canonical relative path.
6. Validate the final component volume serial, 128-bit file ID, and kind match EverythingPathAction.
7. Hold all component handles while the Shell API runs.
8. Reveal a file with SHOpenFolderAndSelectItems.
9. Open a directory with ShellExecuteExW.
10. Release handles only after the Shell call returns.

Every component is therefore checked with OPEN_REPARSE_POINT, and any reparse present at inspection is rejected. Because the handles do not share DELETE, parent or leaf rename, deletion, and replacement operations that require delete sharing fail while those handles remain held, through the return of the Shell call.

### Accepted Post-MVP Reparse Hardening

Windows share modes do not constrain FILE_WRITE_ATTRIBUTES. A malicious same-SID process that has or obtains suitable access may change reparse metadata in place between the final component inspection and path-based Shell resolution. This MVP does not claim to defend against that race.

This residual risk is inherited from the legacy Indexed path-based Shell execution boundary. The user accepted it on 2026-07-31 for the shortest MVP path, and it is tracked as post-MVP hardening. This MVP does not add an ineffective FILE_SHARE_WRITE change or expand into a Shell/oplock refactor.

Successful Shell dispatch clears and hides UiPilot. Stale, missing, denied, or failed dispatch leaves UiPilot visible and reports the existing mapped command error.

## Frontend Changes

- main.ts no longer requires a file-index event subscription for /find readiness.
- launcher-core enters file mode and starts the first search immediately.
- No file streaming poll or revision refresh timer is scheduled.
- Late responses remain rejected by the existing search owner token, view epoch, invocation ID, category, sort, and sequence checks.
- category remains all and sort remains modifiedDesc in state.
- setFileCategory and setFileSort do not create searches in this milestone.
- launcher-view does not render the category strip or sort button.
- The result list, keyboard navigation, Enter execution, status line, and optional metadata preview remain.

## Error Mapping

Backend failures map as follows:

- Everything absent or connection timeout: searchUnavailable.
- Invalid default-instance configuration: searchUnavailable and evict any cached client.
- Query timeout: searchUnavailable.
- Overloaded: searchUnavailable for the current query only.
- IPC send, client closed, or protocol mismatch: searchUnavailable and evict cached client.
- Request ID exhaustion: searchUnavailable and evict the exhausted cached client.
- revision exhaustion: searchUnavailable permanently for the current process.
- path disappeared before publication: omit item.
- stale identity at execution: staleRequest.
- missing path at execution: fileNotFound.
- access or Shell failure: fileOpenFailed.

No error response contains a local path.

## Testing

### Query Adapter

- Literal entity encoding neutralizes all Everything operators and wildcards.
- Isolated Everything 1.4 returns filename-contains matches for ASCII, spaces, Chinese text, and syntax characters.
- Query uses offset 0, max 200, flags 0x155, and date-modified descending sort.
- More than 200 matches returns only the first 200.
- Metadata conversion handles missing size or modified time without panic.
- Unavailable, timeout, overload, protocol failure, eviction, and reconnect behavior are deterministic.

### Command And Registry

- require_main_window remains the first statement.
- category other than all and sort other than modifiedDesc are rejected.
- stale query results never publish.
- opaque actions are not serialized.
- index revision is monotonic and checked.
- FileIndex search is not invoked and there is no fallback.
- Indexed and Everything actions both resolve through the backend-neutral file action.

### Windows Execution

- Identity capture records volume serial, file ID, canonical path, and kind.
- Hard links remain separate path actions.
- Leaf replacement with the same display metadata is stale.
- Parent rename is stale.
- Junction and reparse substitution are stale.
- File-to-folder and folder-to-file changes are stale.
- Component handles remain alive through the Shell callback.
- Files reveal and folders open on success.

### Frontend

- /find searches even when file-index event listening is unavailable.
- Category and sort controls are absent.
- Preview still renders selected result metadata.
- No streaming or revision refresh timer starts.
- A late result cannot replace a newer query.
- A rejected current search sets file.indexStatus to unavailable and preserves the mapped error status; without published result/request IDs, Enter remains inert.
- Enter sends the current opaque requestId/resultId.

### Verification Gates

- Focused Rust unit and integration tests.
- src-tauri cargo fmt, cargo test, cargo clippy with warnings denied, and cargo check.
- Frontend npm tests and production build.
- Existing Everything IPC full test suite and all-target Clippy.
- Isolated real Everything semantic test.
- Manual default-instance /find smoke:
  - at most 200 results;
  - descending modified times;
  - file reveal;
  - folder open;
  - unavailable state after Everything exits.

## Explicit Non-Goals

- Bundled startup or shutdown of Everything.
- Everything Service installation.
- UAC, owner SID, ACL, SDDL, or multi-user isolation.
- Installer or repair UI.
- Category filters.
- Sort switching.
- Pagination.
- File-change events.
- Polling or atomic refresh transactions.
- Removing FileIndex source code.
- Production release readiness.

## Acceptance

This milestone is complete only when:

1. With a manually running default Everything 1.4 instance, /find performs literal filename-contains search and displays at most 200 modified-descending results.
2. Enter reveals files and opens folders after identity revalidation.
3. Rapid text edits cannot publish stale results.
4. Everything absence fails quickly without fallback or automatic startup.
5. Category and sort controls are not visible.
6. No old FileIndex search or background worker is started by /find.
7. All defined Rust, frontend, IPC, path-race, build, and manual smoke gates pass.
8. No installer, Service, permission-management, pagination, or refresh code is added.
