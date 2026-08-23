# Favorite Public Plugins Design

**Status:** Draft - interaction approved, awaiting written-spec review

**Date:** 2026-08-23

## Goal

Let users mark enabled public plugins as favorites from the main launcher. Favorite plugins remain available for every plain-text query and treat that query as their command argument. Non-favorite plugins remain discoverable by activation-name matching, but matching text is used only to find and complete the command.

This resolves the ambiguity in which one plain-text value was simultaneously treated as both a plugin lookup term and a plugin argument.

## Relationship To Existing Contracts

This design narrows and supersedes the plain-text plugin completion rules in `2026-08-23-launcher-default-capabilities-design.md`:

- A favorite plugin preserves every nonempty plain-text query as its argument.
- A matching non-favorite plugin discards the lookup text and completes only its command.

It also narrows the exact `/` rule in `2026-08-18-public-plugin-command-autocomplete-design.md`:

- Exact `/` is the complete command catalog: `/find`, `/web-search`, then all eligible plugins.
- `/prefix` other than exact `/` remains plugin prefix discovery and does not add Find or Web Search rows.

All exact plugin invocation, Runtime, result action, window transfer, permission, timer, notification, and lifecycle contracts remain unchanged.

## User Contract

### Favorite State

Each installed public plugin has one host-owned `favorite` boolean. It defaults to `false` and is bound to the stable `pluginId`, not the effective activation name.

- Renaming or upgrading a plugin preserves `favorite`.
- Disabling a plugin preserves `favorite`, but the disabled plugin is absent from launcher results.
- Re-enabling the plugin restores its favorite behavior.
- Uninstalling a plugin clears `favorite`, including an uninstall that retains the plugin's private storage or secrets.
- Reinstalling a previously uninstalled plugin starts with `favorite: false`.
- The value survives application restarts.

Only the host may read or mutate this preference. It is not part of the public plugin manifest, settings schema, Runtime API, window API, permissions, or plugin-owned storage.

### Main-Launcher Context Menu

Only public-plugin result rows have the favorite context menu. Right-clicking one of those rows:

1. Prevents the WebView's default context menu.
2. Makes that row the current launcher selection.
3. Opens an in-window menu containing exactly one command:
   - `设为常用` when `favorite` is false.
   - `取消常用` when `favorite` is true.

Find, Web Search, calculator results, plugin-produced results, and computer applications do not expose this menu.

Using the menu never completes the query, executes a result, invokes plugin JavaScript, hides the main window, or changes native focus. Duplicate mutation attempts are disabled while one favorite mutation is pending.

A favorite plugin row displays a small filled star after its title. The star is a status indicator, not a separate button. Non-favorite rows do not display it.

### Query Results And Ordering

Eligibility remains unchanged: only installed, enabled, healthy plugins whose active generation is current and which are not awaiting Runtime recovery can appear.

For empty input after outer trimming, show:

1. `/find`
2. `/web-search`
3. Favorite eligible plugins ordered by effective activation name
4. Other eligible plugins ordered by effective activation name

Do not show computer applications for empty input.

Exact `/` produces the same ordered catalog as empty input. It does not execute or query any plugin and does not enumerate computer applications.

For non-command plain text, show:

1. Find for the current text
2. Browser search for the current text
3. Every favorite eligible plugin ordered by effective activation name
4. Matching non-favorite eligible plugins ordered by effective activation name
5. Matching computer applications

Plugin matching remains case-insensitive containment of the effective activation name. A plugin which is both favorite and name-matched appears exactly once in the favorite group. Effective activation names already follow the lowercase command grammar, so ascending activation-name order is deterministic; `pluginId` breaks any otherwise impossible tie defensively.

For `/prefix` other than exact `/`, preserve existing prefix-discovery behavior and order matching eligible plugins by effective activation name. Favorite status does not cause unrelated plugins to appear in `/prefix` discovery.

### Completion And Execution

Selecting a favorite plugin from a non-command plain-text query produces:

```text
/<effectiveName> <outer-trimmed-query>
```

The query is always preserved as the argument, even if it happens to contain or equal part of the activation name:

- Favorite `demo-win`, input `abc` -> `/demo-win abc`
- Favorite `demo-win`, input `win` -> `/demo-win win`
- Favorite `demo-win`, input `demo-win` -> `/demo-win demo-win`

Selecting a matching non-favorite plugin produces only:

```text
/<effectiveName> 
```

For example, non-favorite `demo-win` matched by `win` completes to `/demo-win `.

Empty-input, exact `/`, and `/prefix` plugin discovery also complete to the command plus one trailing ASCII space and do not create an argument.

The first Enter only applies the completion and keeps the main input focused. It never invokes the plugin. A second Enter follows the existing exact-command submission contract after the user has a complete command and any required argument.

Find and browser-search rows retain their existing direct actions for nonempty plain text. Their behavior does not depend on plugin favorite state.

## Persistent State

`PluginStateDocument` gains:

```rust
#[serde(default)]
favorite: bool
```

The existing internal state schema remains readable because missing values default to `false`. `EffectivePluginConfig` exposes the value to the activation manager. Every durable state write preserves it unless an explicit favorite mutation or uninstall changes it.

The state store adds a narrow idempotent favorite mutation. It requires an installed plugin, writes through the existing atomic current/backup path, and advances the inventory revision only when the value changes. Setting the current value again succeeds without another durable write or revision increment.

Uninstall prepares a document with `favorite: false` before committing either retention mode. Retained plugin data and secrets remain governed by their existing contracts and do not retain launcher favorite state.

## Backend Catalog

`PublicCommandSuggestion` gains the host-owned fields required by the launcher:

```text
pluginId
favorite
```

The launcher catalog query returns:

- all eligible favorites, regardless of a plain query;
- eligible non-favorites only when their effective name contains the plain query;
- all eligible plugins for empty input and exact `/`.

The backend performs grouping, de-duplication, ordering, and completion generation. The frontend must not merge a second favorite list or infer whether typed text is an argument.

Public-plugin Runtime responses cannot set favorite metadata. Plugin-produced result rows remain ordinary executable or informational results and never gain the favorite context menu.

## Launcher Wire Contract

Host-generated plugin completion rows gain optional context metadata:

```ts
interface LauncherPluginContext {
  pluginId: string
  favorite: boolean
}

interface ResultItem {
  // Existing fields omitted.
  pluginContext?: LauncherPluginContext
}
```

`pluginContext` is present only on host-generated public-plugin completion rows. `pluginId` must satisfy the existing public-plugin identifier contract. The frontend parser validates the object strictly and rejects unknown fields. Malformed context metadata removes context-menu and star behavior from that row; it does not grant an action, infer a plugin identity, or discard valid sibling rows. The row's existing completion activation remains independently validated.

The main-only client gains:

```ts
setPublicPluginFavorite(input: {
  pluginId: string
  favorite: boolean
}): Promise<void>
```

The corresponding Tauri command checks the `main` label before accessing managed state. It revalidates that the plugin is installed and maps storage, invalid-plugin, and revision-exhaustion failures to the existing fixed public-plugin management error surface. It is not added to plugin Runtime or content-window capabilities.

## Frontend Ownership And Data Flow

1. A current backend response publishes ordered rows and any validated `pluginContext`.
2. Right-clicking a plugin completion row selects it and opens the in-window context menu.
3. Choosing the menu item captures the current view epoch, invocation, query sequence, query control key, raw control value, plugin ID, and target favorite value.
4. The frontend sends one favorite mutation and disables repeated favorite commands until it settles.
5. On durable success, the frontend re-runs the captured query only when the captured launcher and control ownership are still current. That refresh receives the new grouping, completion, and star state.
6. If the user edits, navigates, hides, or reopens the launcher before completion, the durable mutation remains successful but its late UI continuation has zero effect. A later ordinary launcher query reads the new favorite state.
7. On failure, only the still-current owner may publish `操作不可用，请重试`. A stale failure has zero UI effect.

The context menu itself is rendered inside the existing WebView component tree. It must not open a native menu, request native focus, or trigger the launcher's focus-loss hide path.

## Failure Behavior

- Missing `favorite` in an existing state file loads as `false`.
- Corrupt plugin state follows the existing quarantine and recovery contract; favorite state does not introduce a second recovery mechanism.
- A nonexistent or uninstalled plugin cannot acquire a favorite record.
- Disabled, faulted, stale-generation, recovery-pending, and uninstalled favorites are omitted rather than shown disabled.
- A catalog read failure preserves Find and browser search and omits plugin rows, matching the existing launcher fallback.
- A stale favorite success or failure cannot replace newer input, results, selection, view, or status.
- A failed durable mutation leaves the previous favorite value and query results usable.
- Favorite mutation does not create, destroy, reload, focus, or invoke a plugin Runtime or plugin window.

## Testing

Backend-focused tests cover:

- Old state without `favorite` loads as false and the next write preserves a canonical boolean.
- Favorite set, idempotent set, restart reload, rename, update, disable/re-enable, both uninstall retention modes, and reinstall.
- The favorite command rejects non-main callers and nonexistent or uninstalled plugin IDs before mutation.
- Empty and exact `/` catalogs order Find, Web Search, favorites, then other enabled plugins without application enumeration.
- Plain queries order Find, Web Search, favorites, matching non-favorites, then applications.
- Favorite plus name-match is de-duplicated into the favorite group.
- Favorite `demo-win` produces `/demo-win win` for `win`.
- Non-favorite `demo-win` produces `/demo-win ` for `win`.
- `/d` prefix discovery remains matching-only and produces no argument.
- Disabled, faulted, stale-generation, and recovery-pending favorites are excluded.

Frontend-focused tests cover:

- Only plugin completion rows open the custom right-click menu.
- Right-click selects the row and shows the correct set/cancel label.
- The filled star appears only for favorite plugin completion rows.
- Menu actions do not complete, execute, hide, or blur the launcher.
- Successful set and cancel refresh the same current query and update ordering, visibility, and star state.
- Cancelling a nonmatching favorite removes it from a nonempty result list after refresh.
- Mutation failure keeps the query and results and shows the fixed error only for the current owner.
- Editing or navigating during an in-flight mutation makes its late UI continuation inert.
- First Enter completes; second Enter alone may invoke the completed command.

Full frontend tests, Rust library tests, TypeScript compilation, and the production frontend build remain required. Automated tests may dispatch DOM context-menu events but must not control the real mouse, keyboard, foreground window, or native UI.

## Acceptance Criteria

With `demo-win` installed and enabled:

1. Open the launcher with empty input. Right-click `demo-win`, choose `设为常用`, and observe its filled star without the launcher hiding.
2. Enter `abc`. The result order is Find, browser search, favorite plugins, matching non-favorite plugins, then applications; `demo-win` is present even though `abc` does not match its name.
3. Select favorite `demo-win` and press Enter. The input becomes `/demo-win abc`; the plugin is not invoked.
4. Press Enter again and observe the existing plugin invocation behavior.
5. Enter `win`, select favorite `demo-win`, and observe `/demo-win win`.
6. Right-click `demo-win`, choose `取消常用`, and observe the star disappear. It disappears from an unrelated plain-text query after the current-query refresh.
7. Restart UiPilot after setting the plugin favorite and confirm the state persists.
8. Disable then re-enable the plugin and confirm favorite behavior returns. Uninstall and reinstall it and confirm it starts non-favorite.
9. Find, browser search, calculator, `/prefix` plugin discovery, exact plugin commands, and application execution retain their existing behavior.
