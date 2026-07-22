# Internal Plugin MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Load a removable internal JavaScript plugin package so `/math 1+1` renders `2` in the existing host result list and `Enter` copies `2`, while removing the package and restarting removes the feature completely.

**Architecture:** Keep the existing main WebView and `SearchResponse` contract. Rust scans `<appDataDir>/plugins` once at startup, creates one hidden WebView2 runtime per valid plugin, routes matching queries through the existing `search_apps` command, and publishes validated plugin results through the existing `ResultRegistry`. Plugin code receives a narrow injected bridge and can only submit results; Rust owns clipboard writes, result expiry, permissions, and window hiding.

**Tech Stack:** Tauri 2.11, Rust 1.96, WebView2, `webview2-com` 0.38.2, official `tauri-plugin-clipboard-manager` 2, TypeScript 7, React 19, Vitest 4.

## Global Constraints

- Baseline and merge base are exactly `main@16dd26ea9346809c1aad8462aa811df37036b68b`.
- Work only on branch `codex/internal-plugin-mvp` in `D:\code\UiPilot_tools\.worktrees\internal-plugin-mvp`; do not merge to `main`.
- Follow strict RED -> verify expected failure -> minimal GREEN -> full relevant tests for every production behavior.
- UiPilot host and `internal.math` are separate deliverables. No `/math` literal, expression parser, or math fallback may exist under `src/` or `src-tauri/src/`.
- The host version and sample `minHostVersion` are `0.2.0`; update all three existing version sources consistently.
- Plugin packages live only below `<appDataDir>/plugins/<plugin-id>` and are discovered only at process startup. Do not persist plugin IDs, triggers, paths, results, or actions.
- Each MVP plugin has exactly one static feature and one trigger. Unknown manifest fields and permissions disable the complete plugin.
- The visible UI remains the existing main WebView. Plugin HTML is never shown and cannot inject DOM, styles, ARIA, focus, or icons into the main view.
- Plugin runtimes have no Node, Electron, network, file API, WebAssembly, `eval`, worker, iframe, form, download, navigation, or new-window capability.
- A plugin runtime gets only the generated `publish_plugin_results` custom-command permission plus `core:event:allow-listen` and `core:event:allow-unlisten`. Do not grant clipboard plugin commands to any WebView.
- Rust validates caller label, plugin identity, current request, protocol version, exact keys, result count, text lengths, serialized size, and declared `clipboard.writeText` permission.
- A plugin query times out after 500ms. Three consecutive timeouts disable and destroy that runtime until restart; one valid on-time response resets the counter.
- New query, hide, runtime disable, process failure, or host exit invalidates old plugin results. Old or forged IDs never touch clipboard.
- `copyText.text` is at most 4 KiB UTF-8; a complete response is at most 128 KiB and has at most 20 results.
- Successful copy uses the existing registry-first `clear_and_hide` path and does not update application use counts or validation data. Clipboard failure leaves the current result and window usable.
- Continue compiling `test-instrumentation` as probe-only. Production modules must not add lint suppressions beyond the existing exact `search_files` exception.
- Do not add a plugin manager UI, hot reload, network proxy, storage API, package installer, signer, market, telemetry, or public SDK.

---

### Task 1: Removable Manifest Catalog

**Files:**
- Create: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/tauri.conf.json`
- Test: inline `#[cfg(test)]` module in `src-tauri/src/plugins.rs`
- Test: source-oracle tests in `src-tauri/src/lib.rs`

**Interfaces:**
- Produces `PluginManager::load(&self, app_data_dir: &Path, host_version: Version) -> Result<(), PluginSetupError>`.
- Produces `PluginManager::route(&self, query: &str) -> Option<PluginRoute>`.
- Produces `PluginManager::authorizes_clipboard(&self, plugin_id: &str) -> bool`.
- Produces immutable `PluginCatalog` entries with `id`, `version`, `runtime`, `feature`, `permissions`, `root`, and derived collision-free window label.

- [ ] **Step 1: Write manifest and removability tests first**

Add tests that construct temporary `plugins` roots with the standard library only and assert:

Define a test-only `TestRoot` beside these tests. It must create a process-unique directory under `std::env::temp_dir()` using the PID plus an `AtomicU64`, expose `write_plugin` and `remove_plugin` helpers that operate on direct children only, and remove its directory in `Drop`.

```rust
#[test]
fn package_presence_registers_trigger_and_removal_on_reload_removes_it() {
    let root = TestRoot::new();
    root.write_plugin("internal.math", valid_manifest("/math"));
    let loaded = PluginCatalog::load(&root.path, Version::new(0, 2, 0)).unwrap();
    assert_eq!(loaded.route("/math 1+1").unwrap().input, "1+1");

    root.remove_plugin("internal.math");
    let reloaded = PluginCatalog::load(&root.path, Version::new(0, 2, 0)).unwrap();
    assert!(reloaded.route("/math 1+1").is_none());
}
```

Cover exact-key rejection, manifest/version grammar, host minimum, ID/feature/trigger bounds, unknown/duplicate permissions, duplicate IDs/triggers disabling every participant, direct-child-only scanning, ordinary-file requirements, reparse/symlink rejection, and route semantics for exact trigger, ASCII-space body, similar prefix, and ordinary application query.

- [ ] **Step 2: Verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests -- --nocapture
```

Expected: compile failure because `plugins` and the catalog interfaces do not exist.

- [ ] **Step 3: Implement the smallest immutable catalog**

Use `serde(deny_unknown_fields)` DTOs and manual three-part `Version` parsing; do not add a semver dependency. The production shape must stay narrow:

```rust
pub(crate) struct PluginManager {
    catalog: std::sync::OnceLock<PluginCatalog>,
}

#[derive(Clone)]
pub(crate) struct PluginRoute {
    pub(crate) plugin_id: String,
    pub(crate) window_label: String,
    pub(crate) input: String,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Version([u32; 3]);
```

Derive labels as `plugin-` plus lowercase hexadecimal UTF-8 bytes of the validated plugin ID so `a.b` and `a-b` cannot collide. Scan only direct subdirectories, collect candidates first, then disable all participants in duplicate ID or trigger sets. An absent `plugins` directory is an empty catalog, not startup failure. Individual invalid packages are excluded without blocking valid siblings; only app-data/root I/O failures return `PluginSetupError`.

Update `package.json`, root `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` from `0.1.0` to `0.2.0`; let Cargo update only the root package version in `Cargo.lock` at this task.

Add the product module under the same production cfg as other modules and manage exactly one `Arc<PluginManager>` from `run()`. During production setup call `load` with `app.path().app_data_dir()?` before launcher readiness.

- [ ] **Step 4: Verify GREEN and no host math**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests
cargo test --manifest-path src-tauri/Cargo.toml tests::production_modules_have_no_task6_lint_exceptions
rg -n '/math|internal\.math|Expression|calculate\(' src src-tauri/src
```

Expected: Rust tests pass; source search returns no matches.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugins.rs src-tauri/src/lib.rs package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "feat: load removable internal plugin manifests"
```

### Task 2: Hidden WebView Runtime and Asset Boundary

**Files:**
- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Test: inline tests in `src-tauri/src/plugins.rs`
- Test: source-oracle tests in `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes immutable catalog and derived labels from Task 1.
- Produces `PluginManager::asset_response(label: &str, request_path: &str) -> tauri::http::Response<Vec<u8>>`.
- Produces `PluginManager::create_runtimes(&self, app: &tauri::App, app_data_dir: &Path) -> Result<(), PluginSetupError>`.
- Produces `PluginManager::mark_ready(label: &str)` and `PluginManager::disable_runtime(label: &str)`.

- [ ] **Step 1: Write boundary and wiring tests first**

Tests must prove the protocol handler derives identity from `UriSchemeContext::webview_label`, never from a URL parameter; serves only ordinary `.html`/`.js` files below that plugin root; returns fixed 403/404/415 responses; includes the exact plugin CSP; rejects absolute paths, `%`, empty/dot/parent components, alternate data streams, reparse points, another plugin root, and unsupported MIME types.

Add source oracles requiring all of these builder calls and forbidding `visible(true)`, generic external navigation, and direct plugin paths in the main WebView:

```rust
WebviewWindowBuilder::new(app, label, WebviewUrl::CustomProtocol(url))
    .visible(false)
    .focusable(false)
    .skip_taskbar(true)
    .incognito(true)
    .data_directory(data_directory)
    .on_navigation(...)
    .on_new_window(|_, _| NewWindowResponse::Deny)
```

- [ ] **Step 2: Verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests::asset -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml tests::plugin_runtime_wiring_is_narrow -- --nocapture
```

Expected: tests fail because the protocol and runtime builder are absent.

- [ ] **Step 3: Implement the custom source and runtime**

Register one `uipilot-plugin` protocol on the application builder. The handler must delegate to the label-bound catalog and set this header on every successful HTML/JS response:

```text
default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'
```

Inject a fixed, host-owned bridge at document start. It exposes only frozen `window.uipilot.onQuery(handler)` and `window.uipilot.publishResults(response)`. The bridge uses Tauri's internal invoke/event primitives, buffers a query until the handler is installed, and sets the exact title `uipilot-plugin-ready` only after event listening and handler installation complete. It must not expose raw `invoke`, label, paths, or event emit.

Use `on_document_title_changed` to mark readiness. Allow navigation only to the runtime's custom-protocol origin and deny all new windows and downloads. Give every runtime a distinct data directory and incognito mode.

On Windows, add direct `webview2-com = "=0.38.2"` under the desktop target dependencies and attach `ProcessFailedEventHandler` through `WebviewWindow::with_webview`. The callback disables the label and invalidates pending work. Do not log WebView2 raw error details. Task 3 creates the runtime capability after its generated command permission exists, so Task 2 must not leave a capability that references a missing permission.

- [ ] **Step 4: Verify GREEN**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests
cargo test --manifest-path src-tauri/Cargo.toml tests::plugin_runtime_wiring_is_narrow
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: all commands pass with no warnings.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugins.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat: isolate hidden plugin runtimes"
```

### Task 3: Real-Time Query and Result Publication

**Files:**
- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/result_registry.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Create or regenerate: `src-tauri/permissions/autogenerated/publish_plugin_results.toml`
- Create: `src-tauri/capabilities/plugin-runtime.json`
- Test: inline tests in the modified Rust modules

**Interfaces:**
- Adds `QueryDomain::Plugin`.
- Adds `ResultAction::CopyText { plugin_id: String, text: String }`.
- Produces async plugin query dispatch returning `Vec<PluginResult>` or fixed `PluginQueryError`.
- Produces Tauri command `publish_plugin_results(window, plugins, response: serde_json::Value)`.
- Keeps frontend `LauncherClient.searchApps` and `SearchResponse` unchanged.

- [ ] **Step 1: Write RED tests for routing, races, validation, and timeout**

Test the `search_apps` orchestration with injected seams so:

```rust
// plugin route wins without reading AppCache or SettingsStore
// absent package falls through to normal application search
// latest Plugin token is the only publication owner
// stale/unknown/duplicate response cannot publish or consume IDs
// 0 and 20 items pass; 21 items, bad exact keys, bad protocol, and size overflow fail as one response
// valid response resets timeout count; three consecutive 500ms timeouts disable runtime
```

Test `publish_plugin_results` rejects non-`plugin-*` labels before state access, derives plugin identity only from the label, and sends an invalid marker to the waiting search when response validation fails.

- [ ] **Step 2: Verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::plugin -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests::query -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests::query_domains -- --nocapture
```

Expected: compile/test failures for the missing plugin domain, command, and dispatch behavior.

- [ ] **Step 3: Implement minimal async routing**

Make `search_apps` async without changing its wire arguments. Guard `main` first, then ask `PluginManager::route`. A plugin route begins `QueryDomain::Plugin`, dispatches `PluginQueryRequest { protocolVersion: 1, requestId, input }` to the exact plugin window, waits on a standard-library channel in `tauri::async_runtime::spawn_blocking` with `recv_timeout(Duration::from_millis(500))`, and publishes through the existing registry. An absent route calls the existing pure `search_apps_with` unchanged.

Use exact response DTOs:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginQueryResponse {
    protocol_version: u32,
    request_id: String,
    items: Vec<PluginResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginResult {
    title: String,
    subtitle: Option<String>,
    action: PluginAction,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum PluginAction { CopyText { text: String } }
```

Measure Unicode limits with `.chars().count()` and byte/message limits with UTF-8/`serde_json::to_vec`. Validate `clipboard.writeText` both when accepting the response and later when executing the stored action.

Add `publish_plugin_results` to `build.rs` AppManifest and the production `generate_handler!`, update exact command/source-oracle counts, and generate its permission. Create `plugin-runtime.json` with only this generated command permission plus `core:event:allow-listen` and `core:event:allow-unlisten`, scoped to `windows: ["plugin-*"]`. Add source oracles forbidding wildcard capabilities and clipboard permissions. Keep all existing main capability entries unchanged.

- [ ] **Step 4: Verify GREEN**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::plugin
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests::query
cargo test --manifest-path src-tauri/Cargo.toml result_registry::tests
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
```

Expected: tests pass and capability/command inventories agree exactly.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugins.rs src-tauri/src/commands.rs src-tauri/src/result_registry.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/permissions/autogenerated/publish_plugin_results.toml src-tauri/capabilities/plugin-runtime.json
git commit -m "feat: route live queries through internal plugins"
```

### Task 4: Host-Owned Clipboard Execution

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/result_registry.rs`
- Modify: `src/protocol.ts`
- Modify: `src/launcher.test.tsx`
- Test: inline Rust tests in `commands.rs` and `result_registry.rs`

**Interfaces:**
- Adds `ExecuteOutcome::TextCopied` / TypeScript `{ status: 'textCopied' }`.
- Uses only Rust `ClipboardExt::write_text`; no JavaScript clipboard dependency or permission.
- Preserves all application execution behavior and error priority unchanged.

- [ ] **Step 1: Write RED execution tests**

Add pure seam tests proving:

```rust
// CopyText rechecks current plugin permission before clipboard.
// Clipboard success calls clear_and_hide once and returns TextCopied.
// Clipboard failure returns clipboardWriteFailed without clear/hide.
// CopyText never calls app launch, validation record, or use-count increment.
// Stale/unknown IDs stop before permission and clipboard.
// Application actions retain the existing action -> hide -> validation -> settings order.
```

Add a frontend test resolving `{ status: 'textCopied' }` and verifying the frontend does not call `hide_launcher`; Rust remains the hide owner.

- [ ] **Step 2: Verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::execute_plugin -- --nocapture
npm test -- src/launcher.test.tsx
```

Expected: failures for the missing outcome and copy branch.

- [ ] **Step 3: Add the official clipboard plugin in Rust only**

Add `tauri-plugin-clipboard-manager = "2"`, initialize it in the production builder, and call:

```rust
use tauri_plugin_clipboard_manager::ClipboardExt;
app.clipboard()
    .write_text(text.to_owned())
    .map_err(|_| CommandError::clipboard_write_failed())?;
```

Do not add `@tauri-apps/plugin-clipboard-manager` to `package.json`. Do not add any `clipboard-manager:*` permission to `main.json` or `plugin-runtime.json`.

Extend `execute_result` with `AppHandle` and `State<Arc<PluginManager>>`. Resolve once, then branch by action. Plugin success calls the same `clear_and_hide`; plugin failure keeps the registry/current window. Add fixed `clipboardWriteFailed` and `pluginPermissionDenied` codes/messages to Rust and TypeScript.

- [ ] **Step 4: Verify GREEN and permission absence**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::execute
npm test -- src/launcher.test.tsx
rg -n 'clipboard-manager:' src-tauri/capabilities
```

Expected: tests pass; final `rg` returns no matches.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/commands.rs src-tauri/src/result_registry.rs src/protocol.ts src/launcher.test.tsx
git commit -m "feat: execute plugin clipboard actions in Rust"
```

### Task 5: Separate `/math` Package and Removal Proof

**Files:**
- Create: `examples/plugins/internal.math/plugin.json`
- Create: `examples/plugins/internal.math/runtime.html`
- Create: `examples/plugins/internal.math/runtime.js`
- Create: `examples/plugins/internal.math/runtime.test.js`
- Modify: `src-tauri/src/plugins.rs` tests only if the catalog fixture needs final package coverage
- Test: Vitest test beside the sample runtime

**Interfaces:**
- Consumes frozen `window.uipilot.onQuery` and `publishResults` bridge from Task 2.
- Produces no host API and no host source import.

- [ ] **Step 1: Write parser and package RED tests**

The test imports only `runtime.js` and covers precedence, left associativity, unary signs, parentheses, decimals, whitespace, negative zero, incomplete expressions, invalid characters, division by zero, `NaN`/infinity, and the exact flow:

```javascript
expect(calculate('1+1')).toBe('2')
expect(calculate('2+3*4')).toBe('14')
expect(calculate('(2+3)*4')).toBe('20')
expect(calculate('1/0')).toBeNull()
```

Test `plugin.json` exact values: manifest 1, `internal.math`, version/minHost `0.2.0`, runtime `runtime.html`, feature `calculate`, trigger `/math`, and only `clipboard.writeText`.

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- examples/plugins/internal.math/runtime.test.js
```

Expected: failure because the package does not exist.

- [ ] **Step 3: Implement a small recursive-descent parser**

Use this grammar and no other features:

```text
expression = term, { ("+" | "-"), term };
term       = factor, { ("*" | "/"), factor };
factor     = { "+" | "-" }, (number | "(", expression, ")");
number     = digits, [".", digits] | ".", digits;
```

Do not use `eval`, `Function`, regular-expression replacement evaluation, third-party parser, exponent, percent, implicit multiplication, constants, functions, or scientific notation. Use JavaScript `Number`, reject non-finite values, and return `Object.is(value, -0) ? '0' : value.toString()`.

At module load, register only when `globalThis.uipilot` exists. For each query, calculate the input and publish either an empty items array or exactly:

```javascript
{
  title: result,
  subtitle: 'Copy result',
  action: { type: 'copyText', text: result }
}
```

`runtime.html` contains only UTF-8 metadata and one external module script. The host response supplies CSP; do not add inline script or style.

- [ ] **Step 4: Verify package behavior and removability contract**

Run:

```powershell
npm test -- examples/plugins/internal.math/runtime.test.js
rg -n '/math|internal\.math|calculate\(' src src-tauri/src
cargo test --manifest-path src-tauri/Cargo.toml plugins::tests::package_presence_registers_trigger_and_removal_on_reload_removes_it
```

Expected: package tests and Rust removability test pass; host source search returns no matches.

- [ ] **Step 5: Commit**

```powershell
git add examples/plugins/internal.math src-tauri/src/plugins.rs
git commit -m "feat: add removable math plugin example"
```

## Final Verification

After all task reviews are clean, run exactly:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo check --manifest-path src-tauri/Cargo.toml --all-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts/test-security-config.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-security-probe.ps1
git diff --check 16dd26ea9346809c1aad8462aa811df37036b68b..HEAD
git status --short
```

Expected: every command exits 0; Vitest reports all tests passing; both Rust feature sets pass; Clippy has zero warnings; security scripts pass; diff check prints nothing; worktree is clean.

Do not merge. The controller performs whole-branch review, then provides the user the worktree path and manual install/remove/copy/hang test cases.
