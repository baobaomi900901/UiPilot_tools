# Stage A Backend Integration Report (Tasks 2-4)

## Scope

Implemented the Rust-owned find-window controller, narrow command and preference boundary,
native window lifecycle integration, scoped capabilities, and deterministic command permissions.
The phase was intentionally committed as one backend integration unit after the task scope was
expanded from Task 2 to Tasks 2-4.

## RED

The first focused run was:

`cargo test --manifest-path src-tauri/Cargo.toml find_window::tests -- --nocapture`

It failed to compile with 74 expected missing-symbol errors for the wished-for controller API,
including `FindWindowController`, readiness states, queued completion outcomes, native focus
snapshots, execution-hide admission, and timeout constants. This established the initial RED
before production controller code existed.

During self-review, two high-risk regressions were added for stale transfer snapshots and visible
replacement-transfer timeout rollback. Both now pass and protect confirmed-focus state and
phase-correct origin restoration.

## GREEN

The controller now owns `NotReady`, `PreparedNotReady`, `Hidden`, `Transferring`,
`VisibleReady`, and `HidingForExecution`, checked identifiers, retained pin state, confirmed focus
edges, one active transfer, and one latest-only queued transaction. Listener-first preparation is
five-second, commit is idempotent, queue replacement completes the old waiter once, and transfer
admission requires a matching focus/foreground snapshot.

Execution resolves a Rust-only ticket, performs authenticated Shell work without controller or
registry locks, then reacquires controller before find registry admission. Current unpinned
execution retires the exact result set before native hide; stale and pinned tickets do not mutate
lifecycle state. Hide success clears the scope before queued forwarding, while failure retains the
active invocation and keeps ordinary admission closed behind the queued replacement.

Settings now maintain independent checked theme and file-preview revisions. Find readiness and
preview responses expose only the narrow preference fields and canonical decimal revisions. Main
and find commands validate their exact caller label before protected state access.

Native integration precreates `find`, snapshots main/find focus plus foreground HWND, lowers main,
shows and focuses find without holding controller locks, and commits `on_show` before emitting the
forward payload. Pre-ownership failure restores captured native state; post-ownership emit failure
clears the new scope and hides find. Run exit terminates queued waiters through controller shutdown.

## Permission Generation

`src-tauri/build.rs` now lists the frozen command identifiers. `cargo check` regenerated:

- `commit_find_ready.toml`
- `get_find_ready_status.toml`
- `hide_find_window.toml`
- `open_find_window.toml`
- `prepare_find_initialization.toml`
- `set_find_pinned.toml`
- `set_find_preview_preference.toml`
- refreshed `search_files.toml`
- refreshed `set_file_preview_preference.toml`

The two refreshed existing files have no semantic diff after ignoring end-of-line normalization.
The new files contain only the expected allow/deny identifier and command mapping.

## Verification

- Focused controller: 14 passed, 0 failed.
- Commands: 54 passed, 0 failed.
- Settings: 26 passed, 0 failed.
- Lifecycle: 54 passed, 0 failed.
- Full library: 492 passed, 0 failed, 2 ignored.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: passed.

The compile check reports three non-fatal dead-code warnings for test-facing controller views and
legacy test helpers. No lint suppression was added, and the repository lint oracle passes.

## Preservation

The pre-existing dirty `src-tauri/Cargo.toml`, plan document, CodeGraph index, patch artifacts,
Everything database/config files, and other unrelated user files were not staged or modified by
this phase.
