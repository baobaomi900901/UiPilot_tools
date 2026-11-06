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
cargo metadata --manifest-path src-tauri/Cargo.toml --filter-platform aarch64-apple-darwin --format-version 1 > /dev/null
cargo check --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin
```

The copied lock is a seed: Cargo prunes unrelated product packages and replaces the root package. Record the resolved versions in the fixture lock and compare tauri, tauri-build, wry, tao and the global shortcut plugin against the product lock before recording a result. Use this fixture lock for both runs.

1. In two separate terminals inside this fixture start `node serve.mjs 14327` and `node serve.mjs 14328`. Both servers bind to loopback and serve the same two allowlisted files. An occupied port is a test setup failure.
2. Run `node ../../node_modules/@tauri-apps/cli/tauri.js dev --target aarch64-apple-darwin`. In the **local** page, click “Run protected IPC”. Require the local marker, exactly one Rust log entry, the main label and actual dev URL. Record the displayed location and the scheme, host and port printed by the Rust command.
3. Click “Open the external-origin fixture” in that **same** window. Record the new URL, then attempt the same IPC. A bridge refusal or unavailable bridge on the external page plus no additional Rust command entry verifies that route. If navigation itself is blocked, record only the navigation denial and keep the same-window IPC result pending. A timeout, blank page or console error without an explicit outcome is inconclusive. A protected command from the external fixture is a failure.
4. Start again on the local page. With permission to switch foreground, focus another app, press Control+Option+Shift+U, click the input if necessary for diagnosis and record whether the probe came forward and whether typing worked **without a click**. A shortcut registration conflict or missing focus is a failed row; do not grant Accessibility permissions to force a pass. Quit the test app and check that the shortcut is released.
5. With only the external server on port 14328 still running, build `node ../../node_modules/@tauri-apps/cli/tauri.js build --target aarch64-apple-darwin --bundles app`. For internal testing, seal the finished .app with a local ad-hoc signature using `codesign --force --sign - --timestamp=none "$app"` and check `codesign --verify --deep --strict "$app"`, where app is the path to that local bundle. Launch its executable directly in a terminal to preserve its log. Repeat steps 2–4 from its **packaged** local origin. Record the signed/unsigned state, app hash, OS/build, chip, origin, action, result, command-entry count and focus outcome for each run. This internal probe has its own app identifier; a production launcher bundle requires its own release evidence.

Stop both servers and close the probe. Keep generated Cargo.lock, targets and Tauri schema files outside tracked changes.

## Native window fallback

If Tauri compilation is blocked, the native focus probe can still exercise a real WKWebView window and ordinary chord. A local ad-hoc signed copy is placed in `handoff/UiPilot Native Focus Probe.app` in this worktree. To reproduce it in a fresh checkout, run `mkdir -p handoff && sh build-native-window.sh handoff` inside this fixture. The script refuses to replace an existing app. After agreeing to the focus change, run the following in a terminal to see stdout:

```sh
"./handoff/UiPilot Native Focus Probe.app/Contents/MacOS/NativeFocusProbe"
```

Require a successful shortcut registration and “local page loaded”. Switch to another app, press Control+Option+Shift+U, type without clicking and check for “input observed (text omitted)”. Close the window to quit. Do not run the Tauri and native probes simultaneously: they register the same chord. This standalone program does not exercise Tauri origins, IPC or release signing.

2026-11-06 headless observation: Apple M1 Pro, macOS 15.7.4 (24G517), arm64, Command Line Tools SDK 15.2 / Apple clang 16.0.0; full Xcode absent. The native linkage/fixture check passed, found 43 system bundles including Calculator, and rejected the missing directory. Both loopback servers served the expected files and rejected unknown paths. The native window binary was compiled and its local signature verified without launching it. These results do not establish focus, IPC or product acceptance.

## Prepared Tauri handoff (2026-11-06)

The probe worktree contains two local test bundles under `handoff/`. The production app was built with the pinned Tauri CLI and its production custom-protocol feature. The dev-origin app uses the same fixture lock and an optimized Cargo build without that feature, so Tauri selects the configured devUrl. It exercises the dev origin separately from the CLI dev watch/process lifecycle. Run one app at a time; both use the same bundle ID and shortcut.

From this fixture directory, keep the external server running in one terminal:

```sh
node serve.mjs 14328
```

For the production run, start the app in another terminal:

```sh
"./handoff/UiPilot macOS Probe.app/Contents/MacOS/uipilot-macos-probe"
```

For the dev-origin run, quit the production app. Start `node serve.mjs 14327` in a second terminal and start the dev app in a third:

```sh
"./handoff/UiPilot Dev Origin Probe.app/Contents/MacOS/uipilot-macos-probe"
```

In each app, follow the protected IPC, external navigation and focus checks above. Record the actual local URL and external URL first, so an unexpected launch mode is visible. Both prepared bundles are thin arm64, use LSMinimumSystemVersion/minos 15.7.4 and link WebKit. Their local ad-hoc signatures passed `codesign --verify --deep --strict`. Rust 1.96.1, cargo 1.96.1, CLI 2.11.4 and Node 22.22.1 were used. The resolved versions of tauri (2.11.5), tauri-build (2.6.3), wry (0.55.1), tao (0.35.3) and the global shortcut plugin (2.3.2) matched the product lock. No probe GUI was launched as part of the build. The checks below identify the prepared executable bytes; runtime observations must be recorded separately.

| Local executable | SHA-256 after local signing |
| --- | --- |
| Production | `3ff511a908c410c39c290a3201f11705beddd51e156097af0156a22d6654de98` |
| Dev origin | `664d77a4d5abd47500a375b41188546b2704b84a6227eba6d938e93f2f6fb18d` |

To reproduce the separate dev-origin executable after building the production app, run `cargo build --locked --offline --release --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --no-default-features`. Copy the production bundle to a new path, replace its Contents/MacOS/uipilot-macos-probe with that executable, then ad-hoc sign and verify that copy.
