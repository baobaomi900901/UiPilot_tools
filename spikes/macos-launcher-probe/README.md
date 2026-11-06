# Apple Silicon launcher probe

This Task 1 fixture has its own bundle ID, capability file and Tauri entry point. Its only protected command prints the calling window and URL origin and returns a fixed marker. It contains no launcher settings, plugin runtime or Windows service.

## Headless checks

From the repository root:

```sh
sh spikes/macos-launcher-probe/check-native.sh
```

Requires Command Line Tools, Python 3, and an arm64 macOS host. The script compiles an Objective-C executable against Foundation and WebKit, verifies its Mach-O slice, and tests bundle discovery against a Unicode fixture, a link to an external bundle, a missing root and Calculator in /System/Applications. It never creates a window or launches another application. Its WebKit check covers framework linkage and class availability only.

## Tauri and WKWebView exercise

Use an isolated test account and obtain approval for the following GUI and focus changes. Use Rust 1.96.1, native arm64 Node (22.22.1 is available on the probe host), the CLI from the repository's npm lock, and the same macOS deployment target as the probe configuration. Prepare npm dependencies in the repository first. Do not install credentials, disable Gatekeeper or grant system-wide input permissions.

From the repository root set up the isolated fixture:

```sh
cd spikes/macos-launcher-probe
export MACOSX_DEPLOYMENT_TARGET=15.7.4
cp ../../src-tauri/Cargo.lock src-tauri/Cargo.lock
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 > /dev/null
cargo check --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin
```

The copied lock is a seed: Cargo prunes unrelated product packages and replaces the root package. Record the resolved versions in the fixture lock and compare tauri, tauri-build, wry, tao and the global shortcut plugin against the product lock before recording a result. Use this fixture lock for both runs.

1. In two separate terminals inside this fixture start `node serve.mjs 14327` and `node serve.mjs 14328`. Both servers bind to loopback and serve the same two allowlisted files. An occupied port is a test setup failure.
2. Run `node ../../node_modules/@tauri-apps/cli/tauri.js dev --target aarch64-apple-darwin`. In the **local** page, click “Run protected IPC”. Require the local marker, exactly one Rust log entry, the main label and actual dev URL. Record the displayed location separately from the host log (custom-scheme origins may serialize as null).
3. Click “Open the external-origin fixture” in that **same** window. Record the new URL, then attempt the same IPC. A bridge refusal or unavailable bridge on the external page plus no additional Rust command entry verifies that route. If navigation itself is blocked, record only the navigation denial and keep the same-window IPC result pending. A timeout, blank page or console error without an explicit outcome is inconclusive. A protected command from the external fixture is a failure.
4. Start again on the local page. With permission to switch foreground, focus another app, press Control+Option+Shift+U, click the input if necessary for diagnosis and record whether the probe came forward and whether typing worked **without a click**. A shortcut registration conflict or missing focus is a failed row; do not grant Accessibility permissions to force a pass. Quit the test app and check that the shortcut is released.
5. With only the external server on port 14328 still running, build `node ../../node_modules/@tauri-apps/cli/tauri.js build --target aarch64-apple-darwin --bundles app`. Launch the executable inside the built .app directly in a terminal to preserve its log. Repeat steps 2–4 from its **packaged** local origin. Record the signed/unsigned state, app hash, OS/build, chip, origin, action, result, command-entry count and focus outcome for each run. This internal probe has its own app identifier; a production launcher bundle requires its own release evidence.

Stop both servers and close the probe. Keep generated Cargo.lock, targets and Tauri schema files outside tracked changes.

## Native window fallback

If Tauri compilation is blocked, the native focus probe can still exercise a real WKWebView window and ordinary chord. A local ad-hoc signed copy is placed in `handoff/UiPilot Native Focus Probe.app` in this worktree. To reproduce it in a fresh checkout, run `mkdir -p handoff && sh build-native-window.sh handoff` inside this fixture. The script refuses to replace an existing app. After agreeing to the focus change, run the following in a terminal to see stdout:

```sh
"./handoff/UiPilot Native Focus Probe.app/Contents/MacOS/NativeFocusProbe"
```

Require a successful shortcut registration and “local page loaded”. Switch to another app, press Control+Option+Shift+U, type without clicking and check for “input observed (text omitted)”. Close the window to quit. Do not run the Tauri and native probes simultaneously: they register the same chord. This standalone program does not exercise Tauri origins, IPC or release signing.

2026-11-06 headless observation: Apple M1 Pro, macOS 15.7.4 (24G517), arm64, Command Line Tools SDK 15.2 / Apple clang 16.0.0; full Xcode absent. The native linkage/fixture check passed, found 43 system bundles including Calculator, and rejected the missing directory. Both loopback servers served the expected files and rejected unknown paths. The native window binary was compiled and its local signature verified without launching it. These results do not establish focus, IPC or product acceptance.
