<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call - the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep cannot follow. Name a file or symbol in the query to read its current line-numbered source. If it is listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely - indexing is the user's decision.
<!-- CODEGRAPH_END -->

## Implementation Plans

When creating or updating files under `docs/superpowers/plans/`, read and
follow `docs/agents/implementation-plan-guidelines.md`.

This repository guideline overrides generic planning templates that require
repeating the full TDD cycle, implementation snippets, verification steps, or
commit commands inside every task. Approved design specifications remain the
source of truth for technical contracts, event ordering, security boundaries,
and failure behavior.

## UiPilot Host Built-in Launcher Features

Before creating or changing a UiPilot host-owned built-in feature, read
`docs/ui-guidelines.md` and apply its launcher, settings, panel, theme, state,
keyboard, and accessibility rules to every user-facing surface in scope.

When adding a host-owned built-in feature that appears in the main launcher
result list, decide whether the result is a reusable launcher entry. Reusable
entries should be favoritable by default. Wire the full favorite path in the
same change: `BuiltinFeature`, backend `ResultFavorite::builtin(...)`, frontend
`BuiltinFeature` / `safeResultFavorite`, `favoriteMatchesResult`, and launcher
context-menu tests. Recent lesson: `/quicklinks` initially opened correctly but
could not be right-clicked as a favorite because its result had `favorite: None`.

## UiPilot Public Plugin Development

When creating, updating, or testing a UiPilot public plugin, first read
`docs/plugin-sdk/public-plugin-developer-guide.md` and `docs/ui-guidelines.md`.
Apply the UI guidelines to every plugin-owned panel or window while keeping
implementation inside the plugin package. Use
`docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`,
and `docs/plugin-sdk/uipilot-plugin-v1.schema.json` as contract references when
the task touches manifest fields, Runtime APIs, window/panel bridges, network,
settings, permissions, packaging, or validation. Pick the closest tracked example
under `examples/public-plugins/` before scaffolding new plugin code.

Plugin-development tasks keep UiPilot host/app sources read-only. Implement the
plugin inside its package, tests, and plugin-owned docs; do not patch `src/`,
`src-tauri/`, `packages/plugin-cli/`, or SDK contract docs to make the plugin
work unless the user explicitly assigns host-program development in that task.
When the existing public plugin API cannot satisfy the plugin requirement, stop
at the plugin boundary and write a host capability request under
`docs/superpowers/specs/YYYY-MM-DD-<short-slug>-host-capability.md` covering the
user need, plugin scenario, current API gap, proposed host/API behavior, and
acceptance checks. Tell the user the document is ready to hand to the
host-program development agent.
