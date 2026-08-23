# Favorite Public Plugins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users persistently mark enabled public plugins as favorites from launcher result rows, make favorites available for arbitrary plain-text queries, and preserve the approved two-Enter completion contract for both submit and live plugins.

**Architecture:** Store `favorite` in host-owned public-plugin state and publish each successful mutation through the existing activation reservation transaction, so the running catalog and durable state change together. The backend owns catalog eligibility, grouping, completion generation, strict plugin identity, and preview/commit admission; the frontend owns completion provenance, query-sequence ordering, async UI ownership, and the in-WebView context menu.

**Tech Stack:** Rust, Tauri 2 capability permissions, TypeScript, React 19, Ant Design 6, Vitest.

**Approved specification:** [`docs/superpowers/specs/2026-08-23-favorite-public-plugins-design.md`](../specs/2026-08-23-favorite-public-plugins-design.md)

## Global Constraints

- Implement the exact contracts in the specification sections `User Contract`, `Persistent State`, `Backend Catalog`, `Launcher Wire Contract`, `Frontend Ownership And Data Flow`, and `Failure Behavior`.
- `favorite` is host-owned and keyed by stable `pluginId`; do not expose it through manifest, Runtime/window APIs, plugin permissions, settings, storage, or secrets.
- The mutation command is `set_plugin_favorite`, its permission is `allow-set-plugin-favorite`, and only the `main` capability and runtime label guard may authorize it.
- Favorite mutation must not invalidate the scheduler, reload Runtime, change generation/activation/admission identity, operate plugin windows, synthesize input, or change native focus.
- `completionOrigin` is a strict internal main-launcher DTO; preview never dispatches Runtime, commit validates `pluginId` against the final eligible route, and calls without origin retain the existing activation matrix.
- The completion state machine and the opaque favorite-interaction owner use current invocation, control, query sequence/value, result key, and plugin identity exactly as specified.
- Sequence exhaustion is an invocation-level absorbing state reset only by a new native shown invocation.
- Preserve all existing user changes. In particular, `src-tauri/src/commands.rs` already contains an uncommitted exact `/` catalog fix; do not revert it or silently include that pre-existing diff in a favorite-feature commit.
- No real mouse, keyboard, foreground-window, or native-focus automation. Manual acceptance requires explicit user participation.

## Shared Contracts

- Rust state: `PluginStateDocument.favorite: bool` with `#[serde(default)]`; `EffectivePluginConfig.favorite: bool`; `PluginStateStore::prepare_set_favorite`; `PublicPluginManager::set_favorite`.
- Rust launcher activation: `LauncherResultActivation::PluginCompletion { completion_text, plugin_id, favorite }`.
- Search input: optional strict `completionOrigin: { phase: 'preview' | 'commit'; pluginId: string }`, paired with the existing `submit` value as specified.
- Frontend client: `setPublicPluginFavorite({ pluginId, favorite }): Promise<void>`.
- Frontend completion provenance: `armed -> committing -> consumed`, plus an invocation-level sequence-exhausted marker.

## Global Execution Rules

- Every task follows focused TDD once: add the distinctive failing tests, confirm the intended failure, implement the minimum contract, and rerun the focused verification.
- Each task produces one atomic commit containing only that task's changes. Do not absorb pre-existing user changes or unrelated generated files; review fixes, if any, use separate commits.
- Dependency order is `Task 1 -> Task 2 -> Task 3 -> Task 4`. Use one implementation agent sequentially; do not create per-task review agents unless the user explicitly requests them.
- Run the full Rust/frontend/build gates once after Task 4, not after every task.

---

### Task 1: Durable Favorite State And Activation Publication

**Files:** `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`, `src-tauri/src/public_plugins/activation.rs`

**Dependencies:** Approved specification sections `Favorite State`, `Persistent State`, and `Failure Behavior`.

- [x] Add the serde-defaulted state/config field and preserve it across install/upgrade, rename, settings, enable/disable, fault transitions, and other state rewrites; both uninstall retention modes clear it and reinstall starts false.
- [x] Add the installed-only `prepare_set_favorite` candidate builder and the idempotent manager fast path that performs no reservation, durable write, or revision increment when the value is unchanged.
- [x] Implement `PublicPluginManager::set_favorite` with the existing mutation lock, activation reservation, `make_state_durable`, `publish_prepared`, and `bundle.with_config` order. Publish success only after the running bundle contains the durable value, without scheduler or Runtime invalidation.
- [x] Preserve the existing fail-closed transaction behavior: known-not-committed rolls back to the old bundle; unknown durability or post-durable publication failure makes the manager terminal until restart.

**Distinct test coverage:** legacy state defaults false; canonical next write; idempotent no-op revision/write behavior; same-process catalog visibility; restart/rename/update/disable-reenable preservation; both uninstall modes and reinstall; known-not-committed rollback; unknown/post-durable failures enter terminal state without returning success.

**Verify:** `cargo test favorite`; `cargo test public_plugins::state_tests`; `cargo test public_plugins::activation::tests`

### Task 2: Backend Catalog, Strict Wire DTO, And Main-Only Command

**Files:** `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/main.json`, `src-tauri/permissions/autogenerated/set_plugin_favorite.toml`

**Dependencies:** Task 1; specification sections `Query Results And Ordering`, `Completion And Execution`, `Backend Catalog`, and `Launcher Wire Contract`.

- [x] Extend `PublicCommandSuggestion` with `plugin_id` and `favorite`; apply the full eligibility predicate, including Runtime-recovery exclusion, to plain catalogs and `/prefix` discovery.
- [x] Implement empty, exact `/`, and plain-text grouping/order/de-duplication in the backend. Favorites preserve the outer-trimmed plain query; matching nonfavorites complete to command plus trailing space; invalid per-row completions are omitted without removing built-ins or applications.
- [x] Add strict `PluginCompletion` serialization and construction for public-plugin catalog rows while retaining generic `Completion` for `/find` and `/web-search`.
- [x] Add strict `CompletionOriginInput` parsing and the preview/commit decision matrix. Validate phase/submit pairing and `pluginId == route.plugin_id` before registry admission or Runtime dispatch; preview may publish the required-input/manifest hint but never dispatches; commit begins the newer plugin-domain query before dispatching either activation mode.
- [x] Add the `set_plugin_favorite` Tauri command, invoke registration, generated narrow permission, and `main.json` allow entry. Keep all find/runtime/window capabilities unauthorized and retain the runtime `main` label guard before managed-state access.

**Distinct test coverage:** catalog ordering and favorite de-duplication; favorite/nonfavorite completion examples; exact `/` does not enumerate applications; `/d` excludes recovery-pending plugins; UTF-8/control-character completion boundaries; strict `PluginCompletion` identity; preview hints with zero Runtime calls; commit dispatches submit/live; table-driven rejection of phase/submit mismatch, malformed origin, built-in/application use, absent/ineligible plugin, and pluginId/route mismatch before dispatch; invoke/capability allow/deny matrix.

**Verify:** `cargo test favorite`; `cargo test public_plugin_prompt_and_dispatch_decisions_preserve_activation_modes`; `cargo test public_plugin_commands_have_non_overlapping_exact_capabilities`; `cargo test launcher_slash_query_publishes_the_command_catalog_without_reading_applications`; `cargo fmt --check`

### Task 3: Frontend Protocol, Completion State Machine, And Async Ownership

**Files:** `src/protocol.ts`, `src/main.ts`, `src/launcher-core.ts`, `src/launcher.test.tsx`

**Dependencies:** Task 2; specification sections `Completion And Execution`, `Launcher Wire Contract`, `Frontend Ownership And Data Flow`, and `Failure Behavior`.

- [x] Add the strict TypeScript `pluginCompletion` activation and `completionOrigin` request types, parse plugin identity/favorite without downgrade, and wire `setPublicPluginFavorite` to `set_plugin_favorite`.
- [x] Implement post-completion `armed` ownership before scheduling preview; trusted same-command argument edits rebind preview ownership, while command-identity edits restore independently typed behavior.
- [x] Implement the explicit Enter linearization: selected valid result first; otherwise allocate a newer query sequence/search token, rebind `armed -> committing`, invalidate the preview locally, and send commit. Settle only the still-current owner into `consumed`.
- [x] Implement committing-edit replacement (`A committing -> B armed`) so A may finish admitted side effects but cannot publish a hint/result/status, commit a plugin-window transfer, or alter B's completion state.
- [x] Make `consumed` block only plugin-command preview/submission fallback for the untouched value; preserve activation of returned main results. Implement the sequence-exhausted absorbing state and its fixed reopen status.
- [x] Add the opaque favorite-interaction token and favorite mutation owner. Selection movement, another menu, result activation, editing, view change, hide, or new shown invocation invalidates late UI continuation; durable success remains observable on a later ordinary query.

**Distinct test coverage:** malformed plugin completion drops one row and preserves siblings/built-ins; preview pending then commit with late preview success/failure; duplicate Enter while committing; commit A pending then edit/preview B with late A success/failure; ambiguous failure then third Enter is inert; argument edit/reselection creates a fresh arm; returned copy result executes without a second Runtime invocation; sequence exhaustion remains absorbing through edits/selections until new shown; late favorite success/failure after keyboard selection, another right-click, and hide/reopen is inert.

**Verify:** `npm.cmd test -- src/launcher.test.tsx`; `npm.cmd run build`

### Task 4: In-WebView Favorite Menu And Star Presentation

**Files:** `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`

**Dependencies:** Task 3; specification sections `Main-Launcher Context Menu`, `Frontend Ownership And Data Flow`, and `Acceptance Criteria`.

- [x] Render the custom menu with the existing Ant Design in-WebView components. Right-click selects only a host `pluginCompletion` row, prevents the browser menu, and shows exactly `设为常用` or `取消常用`.
- [x] Route menu open/dismiss/consume events through the core interaction-owner API so consuming the current command does not invalidate itself, while independent dismissal or another menu does.
- [x] Disable duplicate mutation commands while one favorite mutation is pending; never complete/execute the row, hide the launcher, invoke plugin JavaScript, or request native focus from the menu path.
- [x] Render a compact filled Lucide star after favorite plugin titles as a noninteractive status indicator. Do not show menu/star behavior for Find, Web Search, calculator, applications, or plugin-produced results.
- [x] Keep menu and star styling aligned with current light/dark tokens, within the launcher bounds, and without changing row height or causing text overlap.

**Distinct test coverage:** eligible right-click selection and exact label; set/cancel action; favorite star only on plugin completion; non-plugin context-menu rejection; duplicate pending action disabled; menu action preserves launcher visibility/focus/query; current mutation refreshes grouping/star; cancelling a nonmatching favorite removes it from the current plain query.

**Verify:** `npm.cmd test -- src/launcher.test.tsx`; `npm.cmd run build`

## Final Verification And Acceptance

- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features` and confirm zero failures.
- [x] Run `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features -- -D warnings`.
- [x] Run `npm.cmd test` and `npm.cmd run build` and confirm zero failures.
- [x] Compare the implementation against every item in the specification `Acceptance Criteria`, including persistence, disable/re-enable, uninstall/reinstall, `/prefix`, exact `/`, Find, Web Search, calculator, application execution, submit plugins, and live plugins.
- [ ] Ask the user before manual UI acceptance. The user performs right-click, keyboard, restart, disable/re-enable, and uninstall/reinstall checks; the implementation agent observes logs/results only and never controls mouse or keyboard.
