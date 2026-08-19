# Web Search Engine Setting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent Bing/Baidu/Google choice to General settings and bind each browser-search result to the engine shown in its title.

**Architecture:** Extend the existing atomic `SettingsStore` with a strict `WebSearchEngine` enum. The backend snapshots that durable enum into each private `OpenWebSearch` action; the frontend only edits the enum and renders backend-owned result text.

**Tech Stack:** Rust, Serde, Tauri 2, React, TypeScript, Ant Design, Vitest.

**Approved specification:** [2026-08-17-web-search-engine-setting-design.md](../specs/2026-08-17-web-search-engine-setting-design.md)

## Global Constraints

- Persist and expose only `bing | baidu | google`; missing legacy values default to Bing and unknown values use existing corrupt-settings recovery.
- Search endpoints and query keys remain fixed in Rust; frontend and plugins never provide URLs.
- `OpenWebSearch` snapshots both engine and query so later setting changes cannot alter published results.
- Settings save success, failure, and reset must keep the settings page open; failure restores the durable value.
- Windows URI acceptance precedes launcher clear-and-hide; URI failure performs no hide.
- Math and slash-command routing remain unchanged and exclusive.
- No automated step starts the GUI, opens a browser, changes foreground focus, or synthesizes input. Manual acceptance requires the user.

## Global Execution Rules

- Each task follows focused TDD: add the distinct failing tests, confirm the intended failure, implement the minimum contract, then rerun the focused tests.
- Preserve all pre-existing worktree changes. Do not commit overlapping prior work; report the final diff and let the user choose consolidation.
- Dependency order is `Task 1 -> Task 2 -> Task 3`.

---

### Task 1: Durable Search Engine Setting

**Files:** `src-tauri/src/settings.rs`, `src-tauri/src/commands.rs`

**Dependencies:** Design sections `Data Contract`, `Settings Flow`, and `Failure Behavior`.

- [ ] Add `WebSearchEngine::{Bing, Baidu, Google}` with Serde camel-case values and `Default::Bing`.
- [ ] Add `web_search_engine` to `Settings`, `SettingsUpdate`, `SettingsView`, and `UserSettingsUpdate` without changing the existing atomic write, backup, or recovery path.
- [ ] Include Bing in defaults and reset candidates; preserve the field during unrelated settings operations.
- [ ] Keep unknown enum values strict so existing invalid-current/backup recovery owns the outcome.

**Distinct test coverage:** legacy JSON without the field loads Bing; all three values round-trip; invalid values follow current recovery; reset and unrelated updates preserve the required value.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml settings::tests`

### Task 2: General Settings Select And Save Ownership

**Files:** `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/launcher.test.tsx`

**Dependencies:** Task 1; design sections `User Contract` and `Settings Flow`.

- [ ] Add `WebSearchEngine` and `webSearchEngine` to the frontend protocol and settings snapshot.
- [ ] Add `setWebSearchEngine(engine: WebSearchEngine)` to `LauncherCore`; include the value in every save/reset candidate and durable rollback baseline.
- [ ] Add the `搜索引擎` Ant Design select to General settings with `Bing`, `百度`, and `Google` options.
- [ ] Reuse the existing settings save owner: disable while pending, remain on the settings page, commit on success, and restore the prior value on failure.
- [ ] Update the reset confirmation copy to state that Bing will be restored.

**Distinct test coverage:** load displays the durable value; change saves immediately; pending change is disabled; successful save remains in settings; failed save remains in settings and rolls back; reset submits Bing.

**Verify:** `npm.cmd test -- --run src/launcher.test.tsx`

### Task 3: Engine-Bound Search Result And Execution

**Files:** `src-tauri/src/web_search.rs`, `src-tauri/src/result_registry.rs`, `src-tauri/src/apps/action.rs`, `src-tauri/src/commands.rs`

**Dependencies:** Tasks 1 and 2; design sections `Provider Registry`, `Result Ownership And Execution`, and `Failure Behavior`.

- [ ] Replace the Bing-only helper with fixed provider metadata for Bing, Baidu, and Google, including exact titles and query keys.
- [ ] Change `OpenWebSearch` to carry `{ engine, query }`; publish the exact title and subtitle `搜索：{query}` for ordinary text.
- [ ] Read the durable engine once while generating a result set and preserve it in the private registry action.
- [ ] Execute only the resolved captured engine/query through the structured URL builder and Windows HTTPS handler.
- [ ] Preserve current ordering (`/find`, engine search, applications), math exclusivity, slash-command exclusion, and failure-without-hide behavior.

**Distinct test coverage:** provider table yields exact titles/endpoints/keys; Unicode, spaces, and reserved characters remain one query value; published DTO omits engine/query internals; changing settings cannot mutate an existing action; stale IDs and URI failure have zero hide side effects; success opens before clear-and-hide.

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml web_search` and `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::search`

## Final Verification

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin uipilot`
- [ ] `npm.cmd test -- --run`
- [ ] `npm.cmd run build`
- [ ] Ask the user to select and execute all three engines, restart for persistence, and reset to Bing. Do not perform these interactions automatically.
