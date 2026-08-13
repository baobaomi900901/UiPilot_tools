# Public Plugin MVP Task 3 Report

## Outcome

Implemented public-plugin activation transactions, hidden runtime ownership, latest-only scheduling, guarded Runtime API calls, management commands, Tauri registration, and non-overlapping capabilities.

Commit: this Task 3 commit (`feat: add public plugin runtime activation`)

## Behavior

- Prepare tokens are caller-bound, expire after five minutes, and clean their staged package on cancel or failure.
- Commit and enable stage an exact runtime snapshot, release the manager mutation lock while waiting for readiness, then revalidate state and staged ownership before promotion.
- Failed readiness or a stale concurrent mutation preserves the old active generation and removes only the caller's staged runtime.
- Runtime labels encode plugin identity and generation; the hidden Runtime WebView is served only from the private `uipilot-public-plugin` protocol and rejects navigation.
- The request scheduler keeps one running and one latest waiting request, starts timeouts at dispatch, and rebuilds context when dispatching a waiter.
- Runtime API calls require the exact runtime label, plugin id, generation, request id, and unexpired request context.
- Main-window commands cover prepare, commit, cancel, enable, rename, settings, and uninstall. Runtime-window commands cover API calls and completion.
- `main.json` contains only the seven Task 3 management permissions. `plugin-runtime.json` targets only `plugin-runtime-*` and contains only API, completion, listen, and unlisten permissions.

## Verification

All Rust verification below used `CARGO_INCREMENTAL=0` and the fresh temporary target `C:\Users\moby\AppData\Local\Temp\uipilot-public-plugin-task3-target` after the pre-existing `src-tauri/target` was shown to contain a damaged PDB and multiple damaged object files.

- `cargo check --manifest-path src-tauri/Cargo.toml --tests`: passed.
- `public_plugins::runtime::tests`: 2 passed.
- `public_plugins::scheduler::tests`: 2 passed.
- `public_plugins::activation::tests`: 3 passed.
- `commands::tests`: 56 passed, including exact public-plugin caller guards.
- `tests::public_plugin_commands_have_non_overlapping_exact_capabilities`: 1 passed.
- Task 1 package regression: 3 passed.
- Task 2 state/storage/secrets regressions: 3/3/2 passed.
- Exact production command registration, runtime capability, and lifecycle wiring tests: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed; line-ending warnings only.

## Follow-up

- The compiler reports 16 production `dead_code` warnings for invocation DTOs, scheduler enqueue/replacement paths, and supporting accessors reserved for Task 4. No suppression was added.
- A broad, non-required `tests::` run completed 514 tests with 3 failures and 2 ignored. Two Task 3 source-contract failures were corrected and their exact tests now pass. The remaining failure is the pre-existing `plugins::tests::delete::no_follow_handle_move_removes_original_path_and_preserves_identity` Windows file-handle test (`The system cannot find the file specified`) and was not modified.
- Real WebView readiness and focus behavior remains for the Task 6 manual gate; no GUI, input synthesis, or focus change was used in Task 3.