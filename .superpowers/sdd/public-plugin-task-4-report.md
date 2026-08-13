# Public Plugin MVP Task 4 Report

## Outcome

Implemented public-plugin command routing, latest-only live and submit dispatch, private result actions, and the main-window public-plugin management UI.

Commit: this Task 4 commit (`feat: add public plugin commands and management`)

## Behavior

- Exact effective-name routing supports live and submit activation modes while preserving internal command whitespace.
- The scheduler keeps one running and one latest waiting submission and settles replaced or expired work without publishing stale results.
- Runtime `mainResult` payloads are strictly bounded and parsed; action payloads remain Rust-owned and only opaque result ids cross to the frontend.
- Public copy actions recheck permission and generation at execution time before touching the clipboard.
- The launcher debounces live public commands by 150 ms, submits on Enter with a new query sequence, and keeps timers and result actions bound to their owning invocation.
- The management panel lists installed public plugins, uses prepare/confirm/cancel for archive and development installs, and supports enable, rename, uninstall, permissions, and all five setting kinds.
- Inventory responses omit runtime paths, output payloads, and secret values; secret settings expose only configured state.
- Main and runtime capabilities remain non-overlapping. The main window owns management and search; hidden runtimes own only API, completion, and event access.

## Verification

Rust verification used `CARGO_INCREMENTAL=0` and the isolated target `C:\Users\moby\AppData\Local\Temp\uipilot-public-plugin-task3-target` to avoid the corrupted legacy `src-tauri/target` cache.

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `commands::tests`: 56 passed.
- `public_plugins::activation::tests`: 6 passed.
- `result_registry::tests`: 28 passed.
- Exact production command registration, capability separation, and lifecycle wiring tests: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --tests`: passed.
- `npm.cmd test -- --exclude ".worktrees/**" src/launcher.test.tsx`: 113 passed, 18 skipped.
- `npm.cmd run build`: TypeScript and production Vite build passed.

## Notes

- Existing compiler `dead_code` warnings belong to later planned public-plugin paths; no warning suppression was added.
- Vite reports the existing large-chunk advisory; the production build succeeds.
- No GUI, foreground focus, mouse, or keyboard automation was used. Real WebView and focus validation remains gated for Task 6.
