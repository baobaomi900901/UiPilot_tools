# Task 2 Report: Introduce Backend-Neutral File Actions

## Status

DONE

## Commit

- Subject: `refactor: generalize file result actions`
- Hash: recorded after commit in the task handoff.

## Implementation

- Added backend-neutral `EverythingPathAction` and `FileExecutionAction` in `file_search`.
- Moved `FileIndexStatus`, `FileResultKind`, `FileResultItem`, and `FileSearchResponse` into `file_search`; `file_index` retains the legacy draft and batch types and re-exports legacy-needed status and kind DTOs.
- Replaced `ResultAction::OpenIndexedPath` with `ResultAction::OpenFile(FileExecutionAction)` and preserved opaque registry resolution.
- Wrapped legacy `FileIndex` search results as `OpenFile(Indexed(...))`; no Query2 adapter or production search switch was added.
- Added the private `execute_file_action_with` dispatcher. Production keeps `spawn_blocking`, routes indexed actions to `FileIndex::execute_indexed_path`, and routes Everything actions once to Task 1 `path_auth::execute_authenticated_path`.
- Updated application action rejection to exhaustively handle `OpenFile`.

## TDD Evidence

### RED

- `cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests::registry_resolves_indexed_and_everything_actions_as_opaque_file_actions -- --nocapture`
  - Exit `1`: expected unresolved `EverythingPathAction`, `FileExecutionAction`, `ResultAction::OpenFile`, shared DTOs, and dispatcher errors.
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::execute_file_action_dispatches_each_backend_once -- --nocapture`
  - Exit `1`: expected missing neutral action and dispatcher errors.

### GREEN

- Both focused commands above: exit `0`; each selected test passed.
- `cargo test --manifest-path src-tauri/Cargo.toml result_registry -- --nocapture`: exit `0`; 19 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml execute_result -- --nocapture`: exit `0`; 1 passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: exit `0`.
- `git diff --check`: exit `0`.
- `cargo test --manifest-path src-tauri/Cargo.toml`: exit `0`; 390 passed, 2 ignored.

## Files

- `src-tauri/src/file_search/mod.rs`
- `src-tauri/src/file_index/mod.rs`
- `src-tauri/src/result_registry.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/apps/action.rs` (required exhaustive match update for the replaced `ResultAction` variant)
- `.superpowers/sdd/task-2-report.md`

## Warning Status

- The final full suite emitted six existing Task 1 `dead_code` warnings in `file_search/windows/path_auth.rs`; Task 2 adds none.

## Self-Review

- Registry and serialized `FileSearchResponse` tests verify indexed and Everything actions stay behind opaque IDs and canonical Everything identity data is not serialized.
- Dispatcher tests verify exactly one backend closure executes for each action kind.
- No Query2 adapter or production search routing change was introduced.

## Concerns

- None. The six path-auth warnings predate this task and are retained for the later production search integration.
