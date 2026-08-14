# Public Plugin MVP Task 6 Report

## Outcome

Implemented the removable `/demo` reference package, generated SDK artifacts, packaging workflow, runtime failure recovery, and the gated WebView2 process-isolation/focus acceptance harness.

Commit: this Task 6 commit (`feat: complete public plugin sdk acceptance`)

## Deliverables

- Added the independently removable `com.uipilot.demo` package with `submit + window` and reloadable `mainResult` modes, instance/theme/platform/date output, host-owned window controls, and one `copyText` action.
- Added package-local runtime tests, a checked TypeScript SDK fixture, developer README, and an atomic Windows packaging script compatible with strict public-package staging.
- Generated and checked in the manifest/data JSON Schema from the Rust DTOs with `schemars`, plus the compact readonly TypeScript API declaration and public-plugin developer guide.
- Added dispatch-time watchdog recovery, latest-waiting redispatch, generation replacement, and the three-fault disable window required by the approved runtime contract.
- Added a gated Windows integration harness that uses test-only Runtime/Shell/Content WebViews, production-format labels, actual trusted origin mapping, unique Runtime query markers, complete renderer diagnostics, and a returning Tauri event loop.

## Real WebView2 Gate

The gated harness was run only after explicit user permission. It did not synthesize or control mouse or keyboard input.

- Gate disabled: 1 passed, 0 failed, completed in 0.00s without creating real test windows.
- Gate enabled: 1 passed, 0 failed, completed in 6.04s.
- The passing probe required exactly one renderer for each mapped WebView, allowed main/find/Shell to share their observed trusted renderer, and required Runtime A, Runtime B, and Content to remain pairwise disjoint and disjoint from the trusted host renderer.
- It terminated the test-only Runtime A renderer and verified main/find/Shell, Runtime B, and Content remained alive; then terminated Content and verified main/find/Shell and Runtime B remained alive.
- It briefly showed and focused the main window through native APIs for about 250 ms, verified focus, and hid it. It never generated input.

## Verification

Rust commands used `CARGO_INCREMENTAL=0` and the isolated target `C:\Users\moby\AppData\Local\Temp\uipilot-public-plugin-task3-target` where applicable.

- Full Vitest suite: 152 passed, 18 skipped.
- TypeScript and production Vite build: passed; Vite reported only its existing chunk-size warning.
- Full Rust suite: library 537 passed and 2 ignored; gated integration path passed with the gate disabled.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: passed.
- Generated Schema `--check`: passed.
- Demo Node runtime tests: 4 passed.
- Demo TypeScript SDK fixture: passed with TypeScript 7's required `--ignoreConfig` flag.
- Demo packaging script and strict staged-package validation: passed.
- Real WebView2 isolation/failure-reclaim/focus gate: 1 passed in 6.04s.

## Manual Acceptance

The user-operated `/demo str` typing, drag/position, pin/close, reload-mode, and second-Enter copy walkthrough was not synthesized or performed by the agent. The automated real-window focus and isolation gate passed; the remaining visual workflow stays explicitly user-operated.
