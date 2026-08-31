# Quicklinks Host Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 UiPilot 宿主侧新增内置 `/quicklinks` 快速链接能力：可在主窗口内维护快速链接，并在 launcher 中通过 `/jd 查询词` 打开默认浏览器。

**Architecture:** Quicklinks 是宿主内置功能，不接入 public plugin runtime panel/session。Rust 侧新增独立 `quicklinks` domain/store，搜索链路只暴露 registry action 与序列化 DTO；前端新增 `launcherMode='quicklinks'` 和内置 Quicklinks Panel 组件，复用主窗口输入/焦点体系但不调用 public plugin panel lifecycle。

**Tech Stack:** Rust 1.96、Tauri 2.11、Windows ShellExecuteW、`url` 2.5、`png` 0.18、React 19、Ant Design 6、Vitest 4。

## Global Constraints

- Approved design spec: `docs/superpowers/specs/2026-08-30-quicklinks-design.md`.
- Worktree: `D:\code\UiPilot_tools\.worktrees\quicklinks-host` on branch `codex/quicklinks-host`.
- Do not edit public plugin package code for this host feature.
- Do not control mouse or keyboard during verification; hand off UI checks to the user.
- Quicklinks only respond to slash commands. Ordinary text search ordering and favorite plugin prioritization stay unchanged.
- Command syntax is exactly `^[a-z][a-z0-9-]{0,31}$`; reserved built-ins are `find`, `quicklinks`, and `web-search`.
- URL templates require `http` or `https`, at least one literal `{Query}`, no NUL/control characters, and UTF-8 percent-encoding with spaces as `%20`.
- PNG icons must be decoded and exactly `128x128`; frontend receives data URLs, never raw local icon paths.
- Every implementation task follows TDD: add focused failing tests, confirm the intended failure, implement the minimum contract, rerun focused tests, then commit only that task’s files.
- Dependency order: Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6.

## Core Contract Overview

- `src-tauri/src/quicklinks.rs` owns persistence, validation, URL expansion, icon validation, and public DTOs for Tauri commands.
- `src-tauri/src/model.rs` adds `LauncherResultActivation::OpenQuicklinks` and `SearchResponse::auto_execute_result_id`.
- `src-tauri/src/result_registry.rs` adds `ResultAction::OpenQuicklink { id: String, url: String }`.
- `src-tauri/src/browser_open.rs` exposes `open_url(url::Url) -> Result<(), ()>`; `web_search::open_search` and quicklinks execution share it.
- `src/protocol.ts` mirrors Rust DTOs and adds quicklinks client methods.
- `src/launcher-core.ts` owns quicklinks state transitions, keyboard behavior, auto-execute gating, and command completion back to `/jd `.
- `src/quicklinks-panel.tsx` owns the visual management panel; `src/launcher-view.tsx` only wires it into the existing surface.

## Task List

### Task 1: Quicklinks Rust Domain And Persistence

**Files:** `src-tauri/src/quicklinks.rs`, `src-tauri/src/model.rs`, `src-tauri/src/lib.rs`

**Dependencies:** Design sections `数据模型`, `链接模板规则`, `图标规则`, `错误处理`.

- [ ] Create the `quicklinks` module with `QuicklinksStore`, `QuicklinkRecord`, `QuicklinkDraftInput`, `QuicklinkListResponse`, `QuicklinkError`, and fixed error-code serialization.
- [ ] Make launcher command validation reusable from `model.rs` without changing the accepted grammar.
- [ ] Implement config load/save under `app_data_dir/quicklinks/quicklinks.json` using temp-file + rename and an in-memory cache updated immediately after successful save.
- [ ] Quarantine corrupt config to `quicklinks.corrupt.<timestamp>.json`, return an empty list, and surface `quicklinkLoadFailed`.
- [ ] Implement URL template validation and `{Query}` expansion with URL-component percent encoding.
- [ ] Implement PNG validation/data-url helpers for exactly `128x128` decoded images.

**Distinct test coverage:** valid and invalid command grammar; reserved command rejection; template scheme/query placeholder/control-character rejection; `手机 A&B?` encoding; corrupt config backup and empty response; create/update/delete cache behavior; fake/corrupt/wrong-size PNG rejection and 128x128 PNG acceptance.

**Verify:** `cargo test --manifest-path src-tauri\Cargo.toml quicklinks::tests -- --quiet`

### Task 2: Quicklinks Tauri Commands And Namespace Conflict Checks

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/main.json`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`

**Dependencies:** Task 1; design sections `全局命令命名空间`, `后端接口设计`.

- [ ] Register `list_quicklinks`, `save_quicklink`, `delete_quicklink`, and `choose_quicklink_icon` for the `main` webview only.
- [ ] Add generated permission entries through the existing Tauri permission generation path and include them in `src-tauri/capabilities/main.json`.
- [ ] Validate save conflicts against reserved built-ins, existing quicklinks, installed public plugin effective names, and legacy plugin route names.
- [ ] Update public plugin install/effective-name rename checks so a plugin cannot claim an existing quicklink command.
- [ ] Keep command responses fixed-code and path-redacted; the frontend should not receive raw icon file paths.

**Distinct test coverage:** main caller succeeds while non-main callers fail; save rejects reserved names and public plugin conflicts; public plugin install/rename rejects quicklink conflicts; delete removes store record and icon reference; choose icon rejects invalid files and returns `{ token, dataUrl }`.

**Verify:** `cargo test --manifest-path src-tauri\Cargo.toml commands::tests -- --quiet`

### Task 3: Launcher Search, Auto-Execute, And Browser Open Execution

**Files:** `src-tauri/src/browser_open.rs`, `src-tauri/src/web_search.rs`, `src-tauri/src/result_registry.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`

**Dependencies:** Tasks 1-2; design sections `默认浏览器打开 URL`, `主界面搜索与执行`, `自动执行事件流`, `执行动作设计`.

- [ ] Add `browser_open::open_url(url::Url)` and refactor `web_search::open_search` to use it without changing existing provider URLs.
- [ ] Add `ResultAction::OpenQuicklink { id, url }` and execute it via the shared browser opener after reparsing and revalidating `http/https`.
- [ ] Add `LauncherResultActivation::OpenQuicklinks` and `SearchResponse::auto_execute_result_id`.
- [ ] Add `/quicklinks` built-in search result and `/` catalog ordering: `/find`, `/quicklinks`, `/web-search`, then public plugin suggestions.
- [ ] Add `/jd` and `/jd 参数` matching against the quicklinks cache before public plugin routes.
- [ ] Move `/web-search 参数` submit auto-execution onto `auto_execute_result_id` so quicklinks and web search share one frontend contract.

**Distinct test coverage:** `/quicklinks` returns `OpenQuicklinks`; `/` catalog order includes quicklinks between find and web-search; `/jd` returns `hasDefaultAction=false` and prompt; `/jd 手机` returns registry `OpenQuicklink` with encoded URL and sets `autoExecuteResultId` only on submit; `/web-search 手机` still works through `autoExecuteResultId`; quicklink execution hides launcher on success and returns `quicklinkOpenFailed` on invalid/open failure.

**Verify:** `cargo test --manifest-path src-tauri\Cargo.toml commands::tests -- --quiet`

### Task 4: Frontend Protocol And Launcher Core State

**Files:** `src/protocol.ts`, `src/protocol.test.ts`, `src/launcher-core.ts`, `src/launcher.test.tsx`

**Dependencies:** Task 3; design sections `前端状态设计`, `主界面搜索与执行`, `自动执行事件流`, `从 Quicklinks Panel 补全命令`.

- [ ] Mirror quicklinks DTOs, client methods, `openQuicklinks` activation, and `autoExecuteResultId` parsing in `src/protocol.ts`.
- [ ] Extend launcher model with `launcherMode='quicklinks'` and quicklinks state: `items`, `selectedId`, `draft`, `loadStatus`, `saveStatus`, `deleteStatus`, `fieldErrors`.
- [ ] Open Quicklinks mode from `/quicklinks` result selection or submit without starting a public plugin panel session.
- [ ] Implement load/list/new/select/edit/blur-save/delete/icon-select actions in core with debounced or owner-checked async state updates.
- [ ] Implement `completeQuicklinkCommand(command)` so Enter on a quicklink list item returns to launcher, fills `/{command} `, focuses the main input, and triggers parameter prompt search.
- [ ] Replace the frontend `/web-search\s+\S` hard-coded auto-execute condition with `response.autoExecuteResultId`, including stale-response checks.

**Distinct test coverage:** invalid activation/response payloads are rejected; `/quicklinks` selection changes mode to quicklinks; quicklinks mode does not call `openPluginPanel`/`closePluginPanel`; Esc exits to launcher; Enter on list item completes `/jd ` and focuses main input; blur-save does not write incomplete drafts; auto-execute ignores stale or non-default `autoExecuteResultId`.

**Verify:** `npm.cmd test -- src\protocol.test.ts src\launcher.test.tsx`

### Task 5: Quicklinks Panel UI And Styles

**Files:** `src/quicklinks-panel.tsx`, `src/quicklinks-panel.test.tsx`, `src/launcher-view.tsx`, `src/styles.css`, `dev/main-preview.html`

**Dependencies:** Task 4; design sections `Quicklinks Panel UI`, `从 Quicklinks Panel 补全命令`, `人工验收`.

- [ ] Add `QuicklinksPanel` with Notes-like left list and right form: 目录名称、启动键、图标、链接.
- [ ] Render the panel inside the main surface when `launcherMode='quicklinks'`, visually reusing the panel host area but not the public plugin iframe/WebView machinery.
- [ ] Use a command tag `/quicklinks`, a new item button, icon preview/choose button, delete confirmation, inline field errors, and save/load/delete status text.
- [ ] Implement keyboard handling for left list ArrowUp/ArrowDown/Enter/Escape without taking Tab focus away from the intended controls.
- [ ] Add dev preview fixture data for `/quicklinks` so styles can be tuned in browser without installing or manipulating real links.

**Distinct test coverage:** panel renders empty/list/error states; form fields display validation errors; choosing an icon calls the client method; delete confirmation calls delete; list Enter calls complete command; preview fixture shows at least two quicklinks.

**Verify:** `npm.cmd test -- src\quicklinks-panel.test.tsx src\launcher.test.tsx`

### Task 6: End-To-End Verification, Cleanup, And Handoff

**Files:** `docs/superpowers/specs/2026-08-30-quicklinks-design.md`, `docs/superpowers/plans/2026-08-31-quicklinks-host.md`, plus files changed by Tasks 1-5.

**Dependencies:** Tasks 1-5.

- [ ] Run formatting and focused test suites after all task commits.
- [ ] Run full Rust tests, frontend tests, and production build.
- [ ] Confirm no unrelated public plugin files are dirty or staged.
- [ ] Update the design document status only if implementation results changed the documented contract.
- [ ] Hand off manual verification steps to the user instead of driving the UI.

**Distinct test coverage:** final suites catch cross-task integration regressions; manual checks cover `/quicklinks` open, add `jd`, `/jd` prompt, `/jd 手机` browser open, panel Enter completion, edit immediate effect, and delete removal.

**Verify:**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo test --manifest-path src-tauri\Cargo.toml -- --quiet
npm.cmd test
npm.cmd run build
git status --short
```

## Final Checklist

- [ ] Quicklinks persistence and icon handling match the design spec.
- [ ] Command namespace conflicts are enforced in both quicklinks and public plugin flows.
- [ ] `/quicklinks`, `/jd`, and `/jd 参数` behave correctly in launcher search.
- [ ] `/web-search 参数` still works after migration to `autoExecuteResultId`.
- [ ] Quicklinks Panel is internal frontend state, not a public plugin panel session.
- [ ] No plugin-owned package changes are included in the host branch.
