# Launcher Default Capabilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show Find, web search, and enabled public plugins for empty/plain launcher input, support `/web-search`, and preserve current result ownership and execution safety.

**Architecture:** The approved [Launcher Default Capabilities Design](../specs/2026-08-23-launcher-default-capabilities-design.md) is the source of truth. The Rust `search_apps` pipeline becomes the sole producer of main-launcher rows; each row carries an explicit activation union, while the frontend owns only current-response validation, selection, completion, and dispatch to the existing Find or result-execution path.

**Tech Stack:** Rust/Tauri 2, TypeScript/React 19, Vitest, Rust unit tests.

## Global Constraints

- Preserve the design's fixed classification order: calculator, empty input, host-reserved commands, plugin slash commands, then plain text.
- Preserve the fixed row order for empty and plain input and the effective activation-name rules.
- `completionText` follows the approved 65,536-byte single-line grammar; invalid items are dropped individually.
- Local Settings/Launcher navigation shares an invocation and strictly increases `querySequence`; only a native re-show establishes a fresh invocation.
- `openFind` remains on the dedicated current-owner `open_find_window` CAS path and never becomes a `ResultAction`.
- Do not modify public-plugin invocation/window/result permissions, calculator semantics, the app identifier, or user-data locations.
- Do not synthesize input, control the mouse/keyboard, or run real-window tests. Manual acceptance requires notifying the user first.
- Preserve and do not stage pre-existing worktree changes, including current branding/icon and native-notification edits.

## Shared Contract

Implement the exact `LauncherResultActivation` union from design section `Launcher Result Activation Protocol` in Rust and TypeScript. Main-launcher response items must use exactly one of `completion`, `openFind`, or `executeResult`; code must not infer behavior from `iconKind`, `completionText`, or an empty result ID.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Every task follows focused TDD, then produces one atomic commit containing only that task's files.
- Run the focused commands listed per task. Run the full final checklist once after Task 3.
- Review fixes, if any, use separate commits and must not absorb pre-existing changes.

### Task 1: Activation DTO And Completion Contract

**Files:** `src-tauri/src/model.rs`, `src-tauri/src/apps/discovery.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/result_registry.rs`, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher.test.tsx`

**Dependencies:** Design sections `Completion Text Contract` and `Launcher Result Activation Protocol`.

- [ ] Add the tagged Rust activation enum and matching TypeScript discriminated union; make activation mandatory for every main-launcher `ResultItem` constructor.
- [ ] Add one Rust `valid_launcher_completion` validator and one TypeScript `safeLauncherActivation` parser with identical grammar and byte/control limits; backend generation must validate before publication and frontend parsing must validate before projection.
- [ ] Mark application, calculator, web-search, and public-plugin main-result rows as `executeResult`; mark existing plugin command suggestions as `completion`.
- [ ] Replace `PLUGIN_COMPLETION` inference with the new activation parser for the approved command/argument grammar, UTF-8 byte limit, outer-whitespace, line-separator, NUL, and Unicode-control rules.
- [ ] Parse activation item-by-item and retain other valid rows when one item has an invalid union or completion value.
- [ ] Enforce closed combinations before activation: only `executeResult` may call `execute_result`; completion/openFind rows cannot borrow an executable result ID.

**Distinct test coverage:** all three valid union variants; missing/unknown/mixed fields; `/demo-win ` and `/demo-win da`; preserved internal spaces; 65,536-byte boundary; NUL, CR/LF, `U+2028/U+2029`, Unicode controls, and oversized completion; a malformed completion beside valid rows drops only that item.

**Verify:** `npm test -- launcher.test.tsx`; `cargo test result_registry::tests`

### Task 2: Backend Capability Catalog And Query Classification

**Files:** `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`

**Dependencies:** Task 1; design sections `User Contract`, `Architecture`, `Data Flow`, and `Compatibility`.

- [ ] Add `web-search` to the host reserved-name set passed into `PublicPluginManager`, so installation and activation-name updates reject it through the existing state-store guard.
- [ ] Keep slash discovery behavior intact and add a separate enabled-command catalog query for empty/plain input: effective-name containment only, case-insensitive, enabled/current/fault-free only, sorted by effective activation name.
- [ ] Classify the outer-trimmed query in the approved order and never call application discovery for empty input or a recognized command/calculator branch.
- [ ] Generate empty rows as completion activations for `/find `, `/web-search `, and all enabled plugins.
- [ ] Generate plain rows in fixed order: openFind, executable configured-engine web search, matching plugin completions, then authorized application results. Build `/command ` for exact bare/slash names and `/command <trimmed-input>` otherwise.
- [ ] Add `/web-search` hint handling and `/web-search <query>` executable-result publication before plugin slash routing. A plugin inventory read failure omits plugin rows without removing Find/Web.
- [ ] Publish non-executable completion/openFind rows under the current query owner without installing a `ResultAction`; keep executable rows bound to the returned request ID.

**Distinct test coverage:** empty ordering and no application snapshot call; `d`/`win`/mixed-case containment; enabled versus disabled/faulted/stale plugins; effective override versus Manifest default; exact/bare/other-text completions; plugin inventory failure fallback; Google/Bing/Baidu direct command; empty web hint; calculator-only response; `web-search` collision during install and rename; slash discovery regression.

**Verify:** `cargo test commands::tests`; `cargo test command_suggestions_filter_match_sort_and_follow_effective_state`; `cargo test public_plugins::state::tests`

### Task 3: Frontend Empty-Query Lifecycle And Activation

**Files:** `src/launcher-core.ts`, `src/launcher.test.tsx`

**Dependencies:** Tasks 1 and 2; design sections `Data Flow`, `Failure Behavior`, `Testing`, and `Acceptance Criteria`.

- [ ] Remove the synthetic `localFindResult`; accept only backend-ordered rows and default selection to index zero when rows exist.
- [ ] Start sequence one with an empty query on a native Launcher show. Every edit, including clear/whitespace-only, starts a new query after immediately retiring visible old rows.
- [ ] Distinguish native show from local navigation: a native event resets sequence for its newly registered invocation; Settings-to-Launcher keeps the invocation, clears the launcher input, increments the existing sequence, and requests an empty snapshot.
- [ ] Route `completion` to `applyEdit`, `openFind` to `submitFind` with the exact response owner, and `executeResult` to the current request ID. Reject stale rows before any path performs an effect.
- [ ] When Enter submits `/web-search <query>` with no current row, accept the returned single executable web-search row and execute it once under the captured submit owner; do not generalize auto-execution to plugin main results.
- [ ] Preserve slash debounce, `/find <query>`, second-Enter plugin invocation, arrow/mouse ordering, calculator replacement, and activation failure notices.

**Distinct test coverage:** first native empty query uses sequence one; non-empty then clear; whitespace-only edit; hide/re-show gets a new invocation; multiple queries then Settings/back uses a higher sequence on the same invocation; completion does not hide/execute; second Enter invokes plugin; direct web command executes once; current versus stale openFind; delayed old response cannot replace new empty rows.

**Verify:** `npm test -- launcher.test.tsx`

## Final Checklist

- [ ] `npm test -- --run`
- [ ] `npm run build`
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] Confirm the approved acceptance matrix without real-window automation; notify the user before requesting manual UI acceptance.
- [ ] Confirm each task commit contains only planned files and no pre-existing worktree changes.
