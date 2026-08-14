# Public Plugin MVP Task 5 Report

## Outcome

Implemented host-wide native transfer ownership, per-plugin singleton window state, two-phase frontend handoff, isolated Shell/Content WebViews, host-owned controls, and per-plugin position persistence.

Commit: this Task 5 commit (`feat: add isolated public plugin windows`)

## Behavior

- `/find` and plugin windows share one `MainWindowTransferCoordinator`; newer owners make stale rollback inert, and expected main blur is consumed exactly once even when its event arrives after commit.
- Window submissions bind UI epoch, submission token, plugin ID, generation, request ID, control value, and derived shell/content labels.
- Runtime `WindowResponse` is request-owned, denies unknown fields, validates JSON recursively, and obeys the 64 KiB response budget.
- Content ready and update ack accept only the exact current content label and request. Reused same-generation windows skip duplicate ready while retaining instance number `1`.
- Runtime completion prepares hidden content and returns a one-time transfer token. The frontend submits it only while the originating query still owns the current input; Rust then revalidates owner and lease after topmost, show, focus evidence, and hide operations.
- Shell and Content use disjoint `plugin-shell-*` and `plugin-content-*` labels and capabilities. Content receives only ready/ack commands, deletes the broad Tauri injection, denies navigation outside the local protocol, downloads, new windows, remote resources, forms, frames, and objects.
- Host Shell owns drag, pin, close, theme, and layout. Pin is process-local and never enables always-on-top; close hides and clears pin.
- Positions are stored atomically per plugin ID, corrected to an available work area on restore, retained across disable, and removed when uninstall deletes plugin data.
- Upgrade, disable, and uninstall tear down the current native shell and invalidate its owner state.

## Verification

Rust commands used `CARGO_INCREMENTAL=0` and the isolated target `C:\Users\moby\AppData\Local\Temp\uipilot-public-plugin-task3-target`.

- `window_transfer::tests`: 2 passed.
- `plugin_window::tests`: 6 passed.
- Strict WindowResponse parser: 1 passed.
- Plugin-window position persistence: 1 passed.
- `commands::tests`: 57 passed.
- `find_window::tests`: 15 passed.
- `lifecycle::tests`: 56 passed.
- Exact production command, lifecycle wiring, and four-capability separation tests: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --tests`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed after formatting.
- Plugin-window plus launcher tests: 117 passed, 18 skipped.
- Plugin-window plus `/find` frontend regressions: 24 passed.
- `npm.cmd run build`: TypeScript and production Vite build passed.
- `git diff --check`: passed; line-ending warnings only.

## Remaining Gate

- No real window, foreground focus, mouse, or keyboard operation was run in Task 5.
- WebView2 process isolation and real focus behavior remain Task 6 gates. Failure or inability to prove process isolation is still a public-release No-Go.
- Existing production `dead_code` warnings reserved for later scheduler/runtime paths remain unsuppressed.
