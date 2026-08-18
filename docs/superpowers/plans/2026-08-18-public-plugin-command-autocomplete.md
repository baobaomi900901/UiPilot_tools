# Public Plugin Command Autocomplete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add installed-public-plugin prefix discovery, keyboard completion, and manifest-owned command usage hints to the main launcher.

**Architecture:** Extend schema-v1 manifests with backward-compatible `command.summary`, generate host-owned completion results from the authoritative public-plugin activation snapshot, and carry completion/hint metadata through the existing owned launcher search path. Completion edits the input locally; usage hints remain outside the selectable result list, so existing submit routing remains the only plugin execution path.

**Tech Stack:** Rust, Tauri 2.11, Serde/Schemars, TypeScript 7, React 19, Ant Design 6, Vitest 4.

**Approved specification:** [`docs/superpowers/specs/2026-08-18-public-plugin-command-autocomplete-design.md`](../specs/2026-08-18-public-plugin-command-autocomplete-design.md), especially `Manifest Contract`, `Search Wire Contract`, `Query Classification`, `Frontend Completion`, and `Failure Behavior`.

## Global Constraints

- Preserve schema version `1`; `command.summary` is optional and missing values fall back to manifest `name`.
- A present summary is nonempty, single-line, control-free, and at most 512 Unicode scalar values.
- Suggestions include only installed, enabled, healthy, current-generation plugins and sort by effective name ascending.
- Completion is host-owned, canonical `/<effectiveName> `, and never receives a `ResultAction`.
- `commandHint` is unselectable and creates no result action or execution authorization.
- Preserve `invocationId + querySequence` stale-response ownership and existing live/submit scheduling.
- Do not synthesize input or control the user's mouse or keyboard. Real-window acceptance requires explicit user participation.
- Preserve all pre-existing worktree changes. Stage only task-owned hunks; never absorb unrelated changes into a commit.

## Shared Interfaces

- `PublicCommandV1.summary: Option<String>` serializes as optional `command.summary`.
- `PublicCommandSuggestion { effective_name: String, display_name: String, summary: Option<String> }` is host-internal discovery metadata.
- `PublicPluginManager::command_suggestions(&self, prefix: &str) -> Result<Vec<PublicCommandSuggestion>, PublicPluginManagementError>` returns already-filtered, sorted suggestions without Runtime dispatch.
- `ResultItem.completion_text: Option<String>` serializes as `completionText`.
- `SearchResponse.command_hint: Option<String>` serializes as `commandHint`.
- Frontend `ResultItem.completionText?: string` and `SearchResponse.commandHint?: string` mirror the Rust wire contract.

## Global Execution Rules

- Dependency order is `Task 1 -> Task 2 -> Task 3`.
- Each task follows focused TDD: confirm an intended failing test, implement the minimum contract, rerun focused verification, and inspect the task diff.
- Do not create per-task review Agents. Automated tests plus a local specification/diff review are the task gates.
- Commit only when the task's hunks can be isolated from pre-existing changes. Otherwise keep the verified hunks unstaged and report that boundary rather than committing unrelated work.

---

### Task 1: Manifest Summary And Demo Packages

**Files:** `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/tests.rs`, `docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `docs/plugin-sdk/public-plugin-v1.md`, `examples/public-plugins/com.uipilot.demo-win/package/plugin.json`, `examples/public-plugins/com.uipilot.demo-return/package/plugin.json`

**Dependencies:** Approved design sections `Manifest Contract` and `Failure Behavior`.

- [ ] Add optional `summary` to `PublicCommandV1` and validate its exact single-line/plain-text/512-scalar contract without rejecting manifests that omit it.
- [ ] Regenerate the checked-in JSON Schema and document the distinction between `description`, `command.summary`, and `command.inputPlaceholder` in the public SDK guide.
- [ ] Update both demo manifests with the approved Chinese summaries and `请输入信息回车`; bump each example version from `1.0.0` to `1.0.1` so an installed `1.0.0` can be upgraded rather than colliding at the same version.
- [ ] Extend repository-example staging assertions to verify the exact metadata and version while preserving each package's existing output mode, permission, and resource count.

**Distinct test coverage:** missing summary remains valid; valid summary round-trips; empty, multiline, control-character, and 513-scalar summaries reject staging atomically; both demo directories stage with exact approved metadata.

**Verify:** `cargo test public_plugins::manifest::schema_tests`; `cargo test repository_demo_examples_stage_as_independently_removable_public_plugins`; `cargo run --bin generate_public_plugin_schema -- --check` from `src-tauri`.

### Task 2: Authoritative Discovery And Search Wire Protocol

**Files:** `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/model.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/plugins.rs`, `src-tauri/src/result_registry.rs`

**Dependencies:** Task 1; approved design sections `Plugin Catalog`, `Search Wire Contract`, `Query Classification`, and `Failure Behavior`.

- [ ] Add `PublicCommandSuggestion` and `command_suggestions`, reading only current activation/config state and filtering installed/enabled/healthy/current-generation entries before matching and sorting.
- [ ] Match effective names by ASCII prefix and display names by Unicode-lowercased substring; always sort the returned vector by effective name.
- [ ] Extend every `ResultItem` and `SearchResponse` constructor with the optional wire fields, defaulting unrelated application, built-in, legacy-plugin, and test responses to `None`.
- [ ] In `search_apps`, route no-space slash input to discovery before exact plugin dispatch. Publish completion rows in `QueryDomain::Plugin` with opaque IDs, no action, `hasDefaultAction: false`, canonical `completionText`, and summary/name subtitle.
- [ ] For exact routed required input, publish an empty result list plus `commandHint`. Retain the hint for nonempty `submit` input when `submit` is false; dispatch only when submit is true. Preserve existing live dispatch after nonempty input.
- [ ] Keep hint/suggestion publication under the current registry token so stale queries and domain replacement have zero effect.

**Distinct test coverage:** `/` returns all eligible plugins; `/d` returns `demo-return` then `demo-win`; display-name matching cannot alter ordering; name override changes filtering/display/completion; disabled, faulted, and stale-generation entries are excluded without Runtime work; exact no-space input remains discovery; required empty and submit-preview queries return only the hint; submit true and live nonempty retain existing dispatch; stale tokens cannot publish suggestions or hints; resolving a completion result returns `UnknownResult`.

**Verify:** `cargo test command_suggestions`; `cargo test public_plugin_command_discovery`; `cargo test public_plugin_prompt` from `src-tauri`.

### Task 3: Launcher Completion And Unselectable Usage Hint

**Files:** `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`

**Dependencies:** Task 2; approved design sections `Search Wire Contract`, `Frontend Completion`, `Failure Behavior`, and `Acceptance`.

- [ ] Parse `completionText` only when it matches `^/[a-z][a-z0-9-]{0,31} $`; discard malformed completion rows instead of exposing an execution fallback.
- [ ] Add private result completion metadata and separate `commandHint` snapshot state. Clear both at the same ownership boundaries as ordinary results and query resets.
- [ ] Make Enter on a completion row apply the canonical text as an owned edit, advance/schedule search normally, retain focus, and skip `executeResult`, `hideLauncher`, and plugin submission.
- [ ] Keep submit-mode `commandHint` while the body changes. Because results remain empty and selected index remains `-1`, Enter follows the existing submit search path.
- [ ] Render the hint immediately below the query input and outside `launcher-results`; give it no listbox role, option role, tab stop, active-descendant ownership, or click action. Add restrained theme-token styling without changing launcher dimensions.

**Distinct test coverage:** first completion defaults selected; arrows select another completion; Enter changes `/d` to the exact selected command plus one space, leaves focus in the input, and calls neither execute nor hide; returned hint is visible but absent from the listbox and accessibility focus order; hint persists for submit body edits and Enter submits once; live nonempty responses replace the empty hint flow; edit/new shown/transfer success clears stale hints; delayed responses cannot overwrite new input; malformed completion metadata is absent and inert.

**Verify:** `npm test -- src/launcher.test.tsx -t "plugin command completion|command usage hint"`; `npm run build`.

## Final Verification And Acceptance

- [ ] Run `cargo fmt --check` and `cargo test` from `src-tauri`.
- [ ] Run `cargo run --bin generate_public_plugin_schema -- --check` from `src-tauri`.
- [ ] Run `npm test` and `npm run build` from the repository root.
- [ ] Run `git diff --check` and review that only task-owned hunks were added to pre-existing modified files.
- [ ] Ask the user to upgrade/reinstall both `1.0.1` demo packages, then manually verify the five acceptance steps in the approved design. Do not control input or foreground focus.
