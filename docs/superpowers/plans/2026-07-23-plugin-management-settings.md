# Plugin Management Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the obsolete research/validation controls and add a main-window-only plugin inventory whose reload and delete operations preserve query, action, runtime, and filesystem linearizability.

**Architecture:** Replace the immutable plugin catalog with one `PluginManager` that owns active and staged runtime identities behind a fixed admission gate. Management commands serialize on a mutation lock, then use generation-bound query/action data and Windows handle-based quarantine moves as their commit protocols. The settings frontend keeps ordinary settings, plugin list ownership, and row mutations as independent state domains and reconciles stale mutations with a fresh authoritative list.

**Tech Stack:** Rust 2021, Tauri 2, Windows filesystem APIs, React 19, TypeScript 7, Ant Design 6, `react-markdown`, Vitest, Rust unit tests.

## Global Constraints

- Work only in `D:\code\UiPilot_tools\.worktrees\plugin-management-settings` on `codex/plugin-management-settings`.
- Follow red-green-refactor for every behavior change; run the named focused test before implementation and after the smallest production edit.
- Never start `npm run tauri dev` during automated work. The user owns GUI/manual testing.
- Preserve startup application discovery, file-index shutdown, Windows session-end handling, existing hotkey/autostart behavior, and `/math` host-rendered result behavior.
- Do not expose plugin paths, permissions, runtime labels, runtime data directories, generation, README contents in errors, or manifest source in frontend DTOs or command errors.
- Fixed lock order: mutation lock -> plugin admission -> manager catalog -> at most one readiness/disabled/timeout/pending state -> `ResultRegistry`. Resolve registry actions before taking admission; do not hold catalog/state/registry locks during readiness waits, filesystem I/O, WebView close, or clipboard writes.
- A reload commit is active/staged ownership promotion plus `QueryDomain::Plugin` epoch invalidation under one write admission. A delete commit is a no-follow, handle-based atomic rename out of the plugin root followed by in-memory removal and plugin-domain invalidation under the same write admission.
- Keep the promoted generation data directory. Clean only rolled-back staged data, replaced old-active data, or deleted-active data, and treat cleanup failures as best effort.

---

## Task 1: Remove Research ID And Validation-Only Surfaces

**Files:**

- Modify: `src/protocol.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher-view.tsx`
- Modify: `src/main.ts`
- Modify: `src/launcher.test.tsx`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/model.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lifecycle.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/main.json`
- Delete: `src-tauri/src/validation_data.rs`
- Delete: `src-tauri/src/validation_export.rs`
- Delete: `src-tauri/src/session_marker.rs`

- [x] Add/adjust frontend contract tests proving `SettingsView`, `UserSettingsUpdate`, `LauncherClient`, `SettingsSnapshot`, error codes, notices, and settings UI contain no Research ID, rescan, validation export, validation clear, or confirmation state.
- [x] Run `npm test -- --run launcher.test.tsx`; expect the new contract assertions to fail against the existing fields and controls.
- [x] Add Rust source/serialization tests proving old `researchId` input is ignored, the next settings write omits it, the three obsolete commands are absent from handler/build/capabilities, and startup/execute/exit paths contain no validation state or notice.
- [x] Run `cargo test --manifest-path .\src-tauri\Cargo.toml settings::tests commands::tests lib::tests lifecycle::tests`; expect failures while the old subsystem remains.
- [x] Remove the frontend fields, actions, methods, notices, buttons, and operation variants. Keep only ordinary settings `load | save | hotkey` operation ownership.
- [x] Remove `research_id` from persisted and wire settings structures. Let serde ignore the legacy JSON key, so a subsequent atomic settings write naturally drops it.
- [x] Remove validation recording from application execution and launcher lifecycle without changing action, persistence, hide, or file-index cleanup ordering.
- [x] Remove the validation modules and only their now-unused export/dialog/marker helpers. Keep `atomic_file.rs` wherever settings or file-index persistence still consumes it.
- [x] Run `npm test -- --run launcher.test.tsx` and the focused Rust test command; expect green.
- [x] Run `rg -n "researchId|research_id|rescan_apps|export_validation_data|clear_validation_data|validation_data|validation_export|session_marker|validationFailed" src src-tauri --glob '!src-tauri/target/**'`; expect no production references and only intentional legacy fixture strings, if any.
- [x] Commit with `git commit -am "refactor: remove validation settings subsystem"` after deleting files with `git add -A`.

## Task 2: Add Plugin Description And Inventory Contracts

**Files:**

- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/main.json`
- Add: `examples/plugins/internal.math/README.md`

- [ ] Add plugin tests for valid, missing, oversized, invalid UTF-8, directory, symlink/junction, and other reparse `README.md` inputs. Assert description failures return `None` while the plugin itself remains loadable.
- [ ] Add command contract tests for a camelCase `PluginView { id, version, trigger, description }`, fixed `pluginListFailed`, main-window caller guard before manager access, deterministic ID ordering, and the exact approved DTO fields.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml plugins::tests commands::tests::plugin`; expect compilation/assertion failures before the DTO and reader exist.
- [ ] Add `Version::display() -> String` and `PluginCatalogEntry::view() -> PluginView`. Read only root `README.md`, require a non-reparse ordinary file, cap bytes at `16 * 1024`, validate UTF-8, and never make description validity part of plugin activation.
- [ ] Add `PluginManager::list_views(&self) -> Result<Vec<PluginView>, PluginManagementError>` returning a short-lived snapshot sorted by ID.
- [ ] Add `#[tauri::command] list_plugins` with `require_main_window` as its first side-effecting step and fixed error mapping. Register only the main-window capability and build manifest permission.
- [ ] Write `examples/plugins/internal.math/README.md` documenting `/math <expression>`, live host-rendered results, and Enter copying the result.
- [ ] Run the focused Rust tests; expect green.
- [ ] Commit with `git add src-tauri examples && git commit -m "feat: expose plugin inventory descriptions"`.

## Task 3: Bind Plugin Queries And Actions To Generations

**Files:**

- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/result_registry.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] Add tests proving `PluginRoute`, `PendingPluginQuery`, and `ResultAction::CopyText` carry one `u64` generation, and callbacks with mismatched label/generation cannot publish.
- [ ] Add registry/manager concurrency tests for an old query publishing after a reload/delete commit and an already-resolved old CopyText action racing a generation switch. Assert no stale result publication or clipboard call.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml result_registry::tests commands::tests::plugin plugins::tests`; expect new tests to fail.
- [ ] Introduce `RuntimeIdentity { plugin_id, window_label, generation }` and `PluginAdmission { gate: RwLock<()> }`. Put `generation` in active entries, routes, pending requests, and CopyText actions.
- [ ] Under admission read, atomically snapshot the active route, issue the plugin-domain query token, and register pending work; release admission before waiting for runtime.
- [ ] Under admission read, recheck callback label, pending generation, and current active generation, then call `publish_if_latest` before releasing admission.
- [ ] Refactor plugin CopyText execution so registry resolve completes first; then admission read reauthorizes ID + generation + permission and remains held through the clipboard write. Do not acquire registry from inside admission on this path.
- [ ] Add a single commit helper that, while write admission remains held and manager locks are released, calls `ResultRegistry::invalidate_domain(QueryDomain::Plugin)`. Treat domain epoch exhaustion as fail closed.
- [ ] Run the focused tests; expect green, including the two race cases.
- [ ] Commit with `git add src-tauri/src && git commit -m "refactor: generation-bind plugin results"`.

## Task 4: Replace The Immutable Catalog With Runtime Ownership Slots

**Files:**

- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] Add manager tests for monotonic generations, unique active identities/data directories, staged asset serving without staged query routing, generation overflow fail-closed behavior, and active snapshot reads that do not wait on runtime response.
- [ ] Add callback tests proving callbacks capture only `(plugin_id, label, generation)` and dynamically resolve staged, active, or absent ownership under admission. Cover delayed old-active callbacks, delayed rolled-back staged callbacks, and normal-close callbacks after ownership removal.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml plugins::tests`; expect failures against `OnceLock<PluginCatalog>` and plugin-ID-only callbacks.
- [ ] Replace `OnceLock<PluginCatalog>` and the independent label maps with `PluginManagerState { active, staged_assets, ownership }` behind one manager lock, plus one mutation lock and one admission gate.
- [ ] Represent staged readiness as `ready`, `failed`, and a condvar keyed by the immutable runtime identity. Keep active and staged asset/ownership mappings in the same manager critical section so one identity cannot own two slots.
- [ ] Give every runtime a generation-specific WebView label and data directory. Custom protocol lookup may resolve active or staged assets; user query routing may resolve active entries only.
- [ ] Change ready/process-failed/unexpected-close handlers to capture only runtime identity. Ready uses admission read and mutates staged readiness only; failure/close uses admission write and dynamically performs staged failure or active disable + pending cancellation + plugin-domain invalidation.
- [ ] Ensure host-initiated close removes ownership first so its delayed close callback resolves absent.
- [ ] Run focused tests; expect green.
- [ ] Commit with `git add src-tauri/src && git commit -m "refactor: model plugin runtime ownership"`.

## Task 5: Implement Transactional Reload With A Fixed Deadline

**Files:**

- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/main.json`

- [ ] Build a fake runtime/filesystem harness and tests for valid reload, changed ID, conflicting trigger, invalid manifest, creation failure, never-ready timeout, ready-then-failed-before-commit, and final WebView absence. Every failure must preserve the old active route and permission.
- [ ] Add tests that promotion atomically removes staged asset and ownership, retains promoted generation data, closes/cleans only old active data, and routes the promoted runtime's later process-failed/close callback through active failure handling.
- [ ] Add timeout tests asserting `PLUGIN_RUNTIME_READY_TIMEOUT == 500ms`, unified staged rollback ordering, both staged maps removed, staged runtime closed before staged data cleanup, and a second mutation can start immediately after timeout.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml plugins::tests::reload commands::tests::plugin`; expect failures before reload exists.
- [ ] Add `PluginManager::reload_plugin(...) -> Result<PluginView, PluginManagementError>` and `PLUGIN_RUNTIME_READY_TIMEOUT: Duration = Duration::from_millis(500)`.
- [ ] Hold the mutation lock from candidate preparation through commit or rollback, but hold no admission/catalog/readiness/pending/registry lock during the bounded readiness wait.
- [ ] Before promotion, take write admission and recheck that the identity uniquely owns both staged maps, `ready == true`, `failed == false`, and the WebView still exists.
- [ ] In one manager critical section, move staged asset and ownership into active, remove old ownership, and cancel old-generation pending state. With write admission still held and manager state released, invalidate the whole plugin domain; only then release admission.
- [ ] On any pre-commit failure or timeout, under write admission remove both staged mappings, then release admission, close staged runtime, and best-effort clean staged data. Do not touch the old active entry/runtime.
- [ ] After commit, close old runtime and best-effort clean old-generation data; never clean promoted data.
- [ ] Add main-only `reload_plugin(plugin_id) -> PluginView`, fixed `pluginReloadFailed`, handler/build/capability wiring, and guard-before-manager tests.
- [ ] Run focused tests; expect green.
- [ ] Commit with `git add src-tauri && git commit -m "feat: transactionally reload plugins"`.

## Task 6: Implement Reliable Delete By Handle-Based Quarantine Move

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/plugins.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/main.json`

- [ ] Add Windows backend tests or an injectable move backend for: ordinary direct child success, directory replaced after load, reparse target, volume/file identity mismatch, destination collision, move failure, and cleanup failure.
- [ ] Add state tests proving move failure has zero manager/runtime/filesystem side effects; move success removes the original plugin-root path and active routing immediately; quarantine/data cleanup failures still return success; restart scanning does not load quarantine leftovers.
- [ ] Add race tests proving old pending publications and already-resolved old actions cannot cross delete commit.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml plugins::tests::delete commands::tests::plugin`; expect failures before delete exists.
- [ ] At initial load/successful reload, capture the plugin directory volume/file identity from a no-follow directory handle. Precreate a host quarantine directory under app data, outside `plugins`, on the same volume, and best-effort clean leftovers at startup.
- [ ] Implement a Windows no-follow directory handle using `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`. Revalidate ordinary directory, direct-child relation, and stored identity, then rename by handle to a host-generated non-overwriting quarantine name on the same volume.
- [ ] Add `PluginManager::delete_plugin(...)`: hold mutation + write admission, perform the atomic move as the filesystem commit, then remove active ownership/routing/authorization and cancel pending state. Release manager state, invalidate the whole plugin domain, then release admission.
- [ ] If rename fails, return without any memory/runtime/original-path change. After a successful commit, close old runtime, then best-effort clean its generation data and quarantine subtree. Cleanup errors cannot change success.
- [ ] Add main-only `delete_plugin(plugin_id) -> ()`, fixed `pluginDeleteFailed`, handler/build/capability wiring, and caller-guard tests.
- [ ] Run focused tests; expect green.
- [ ] Commit with `git add src-tauri && git commit -m "feat: reliably delete plugins"`.

## Task 7: Add Strict Frontend Plugin Protocol And Independent State

**Files:**

- Modify: `src/protocol.ts`
- Modify: `src/main.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher.test.tsx`

- [ ] Add parser tests for exact dense `PluginView[]`: approved keys only, plain objects only, valid strings, nullable description, duplicate ID rejection, and untrusted Tauri payload rejection.
- [ ] Add exact invoke tests for `list_plugins`, `reload_plugin { pluginId }`, and `delete_plugin { pluginId }`; assert removed commands are never invoked.
- [ ] Add core tests for list `idle | loading | ready | error`, entry-on-every-settings-view, explicit retry, error vs empty, independent ordinary settings draft, and per-row reload/delete state.
- [ ] Add epoch/token race tests for stale list responses and stale row responses. For both reload and delete, cover leave/reenter -> new list returns old snapshot -> old mutation succeeds later; assert a new current-epoch list reconciles to backend. Also assert a stale mutation failure does not render its old error but still reconciles when settings is current.
- [ ] Run `npm test -- --run launcher.test.tsx`; expect type/test failures before new protocol/state exists.
- [ ] Add `PluginView`, strict `parsePluginViews`, `PluginListSnapshot`, `PluginRowSnapshot`, and error codes `pluginListFailed | pluginReloadFailed | pluginDeleteFailed`.
- [ ] Add client methods that parse list/reload responses before returning them to core.
- [ ] Implement plugin-list ownership as `{ viewEpoch, token }`, separate from ordinary `SettingsOperation`; every list attempt allocates a fresh token. A newer reconciliation token replaces any earlier list owner.
- [ ] Implement row ownership as `{ pluginId, viewEpoch, token, kind }`; only current owner may directly apply success/error. On stale completion, if the current view is settings, immediately issue a new current-epoch list request; otherwise wait for the next settings entry.
- [ ] Ensure reconciliation touches only plugin list owner/state and never settings drafts or ordinary settings operation.
- [ ] Run the focused frontend tests; expect green.
- [ ] Commit with `git add src && git commit -m "feat: model plugin settings state"`.

## Task 8: Render The Plugin Inventory And Safe Markdown

**Files:**

- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `src/launcher-view.tsx`
- Modify: `src/styles.css`
- Modify: `src/launcher.test.tsx`

- [ ] Add component tests for loading, explicit list error + retry, ready empty state, plugin metadata, fallback description, row-only loading, row error, delete confirmation, and successful row update/removal.
- [ ] Add security tests with Markdown containing raw HTML, a link, an image, and script-like content. Assert no `a`, `img`, raw HTML node, navigation, or external resource is rendered, while headings, paragraphs, lists, emphasis, and code render.
- [ ] Run `npm test -- --run launcher.test.tsx`; expect failures before UI/dependency changes.
- [ ] Install `react-markdown` with the existing npm lock workflow.
- [ ] Add an unframed full-width plugin section below basic settings. Render ID, version, trigger, description, reload button, delete button, and an Ant Design confirmation for delete. Avoid nested cards.
- [ ] Configure `ReactMarkdown` with only `h1..h6`, `p`, `ul`, `ol`, `li`, `em`, `strong`, `code`, and `pre`; do not enable raw HTML and do not allow `a` or `img`.
- [ ] Keep row controls stable in width/height, text wrapping within the settings viewport, and plugin state independent from Save/Reload Settings controls.
- [ ] Run focused tests and `npm run build`; expect green.
- [ ] Commit with `git add package.json package-lock.json src && git commit -m "feat: render plugin management settings"`.

## Task 9: Cross-Layer Verification And Manual Handoff

**Files:**

- Modify if needed: `docs/superpowers/specs/2026-07-23-plugin-management-settings-design.md` only for implementation-discovered factual corrections
- Add: `docs/superpowers/verification/2026-07-23-plugin-management-settings.md`

- [ ] Run `npm test`; expect all frontend tests green.
- [ ] Run `npm run build`; expect TypeScript and Vite production build green.
- [ ] Run `cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check`; if it fails, run formatter, review only intended files, then rerun check.
- [ ] Run `cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings`; expect green.
- [ ] Run `cargo test --manifest-path .\src-tauri\Cargo.toml`; expect all non-ignored Rust tests green.
- [ ] Run repository-specific boundary/performance scripts only if their prerequisites are present; record skipped scripts and reason rather than silently omitting them.
- [ ] Audit `git diff main...HEAD`, `git status --short`, capability/build command parity, lock-order-sensitive paths, cleanup targets, stale response reconciliation, and fixed error messages.
- [ ] Write the verification record with exact commands/results and a manual worktree checklist covering: list rendering, README fallback, reload success/failure preserving old plugin, delete confirmation/success, original package path disappearance, cleanup persistence semantics, `/math` result/copy behavior, stale trigger disappearance after delete/restart, and settings draft preservation.
- [ ] Commit final verification/test corrections with `git add -A && git commit -m "test: verify plugin management settings"`.
- [ ] Stop before merging to `main`; provide the worktree path, branch, setup command, and manual test cases to the user for acceptance.

## Plan Self-Review

- [ ] Every design requirement maps to a task: validation removal (1), README/list contract (2), generation/admission (3), callback ownership (4), bounded transactional reload (5), atomic quarantine delete (6), stale-response reconciliation (7), safe Markdown UI (8), and full verification/manual handoff (9).
- [ ] Reload and delete commit points, rollback boundaries, callback ownership, data-directory cleanup, and lock order are consistent with the reviewed design.
- [ ] All paths and commands are concrete; no placeholder filenames, test names, or APIs remain.
- [ ] `PluginView` remains frontend-safe and generation remains internal; row/list/settings ownership types remain independent.
