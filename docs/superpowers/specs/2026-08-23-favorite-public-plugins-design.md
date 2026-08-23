# Favorite Public Plugins Design

**Status:** Approved - interaction confirmed and six-round independent review passed; ready for implementation planning

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

One launcher-only exception is required to preserve the approved two-Enter interaction for host-generated plugin completions. Applying such a completion creates a completion-origin state. While that state owns the current command, automatic searches are marked as preview-only: they may route the command and return its usage hint, but neither activation mode may dispatch Runtime. The next explicit Enter commits the completion. This does not change Runtime activation modes for independently user-typed commands or any plugin-facing API.

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

Eligibility is uniform across every launcher catalog path: only installed, enabled, healthy plugins whose active generation is current and which are not awaiting Runtime recovery can appear. The existing `/prefix` lookup must add the same Runtime-recovery exclusion already used by the plain-text catalog.

For empty input after outer trimming, show:

1. `/find`
2. `/web-search`
3. Favorite eligible plugins ordered by effective activation name
4. Other eligible plugins ordered by effective activation name

Do not show computer applications for empty input.

Exact `/` produces the same ordered catalog as empty input. It does not execute or query any plugin and does not enumerate computer applications.

For non-command plain text, show the following groups, subject to the per-plugin completion-validity rule below:

1. Find for the current text
2. Browser search for the current text
3. Every favorite eligible plugin ordered by effective activation name
4. Matching non-favorite eligible plugins ordered by effective activation name
5. Matching computer applications

Plugin matching remains case-insensitive containment of the effective activation name. A plugin which is both favorite and name-matched appears exactly once in the favorite group. Effective activation names already follow the lowercase command grammar, so ascending activation-name order is deterministic; `pluginId` breaks any otherwise impossible tie defensively.

Completion validity is checked independently for each plugin row using the existing `LauncherResultActivation::completion` contract. If the computed `/<effectiveName> <query>` exceeds 65,536 UTF-8 bytes or its argument contains a control character, U+2028, or U+2029, that plugin row is omitted. Find, Web Search, calculator, and eligible application rows continue to work, and the frontend must not synthesize a disabled plugin row. This deterministic degradation applies only to queries which cannot be represented by the frozen completion grammar; it does not weaken the ordinary favorite behavior described above.

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

The first Enter only applies the completion, keeps the main input focused, and installs an `armed` completion-origin state bound to the current view epoch, invocation, control key, exact completion value, result key, plugin ID, and effective command. In one synchronous frontend transition, the launcher applies the completed control value and its query sequence, installs `armed` ownership from that post-completion state, and only then schedules a search.

Automatic searches owned by `armed` send `submit=false` with `completionOrigin: { phase: 'preview', pluginId }`. Preview performs normal route and input-required validation and may return the manifest `inputPlaceholder`, but it never dispatches either `submit` or `live` Runtime. Trusted user edits to the argument keep the state `armed`, update its exact control value and query sequence, and issue another preview as long as the value still routes to the same effective command. Editing the command identity or leaving command syntax clears completion-origin state and restores the existing user-typed behavior.

The next Enter uses this synchronous linearization order:

1. Verify that the selected-result action does not take priority and that the `armed` owner still matches the current invocation, control, command, and exact value.
2. Allocate the next `querySequence` and a fresh frontend search-ownership token.
3. Rebind the completion owner to that sequence/token and transition it from `armed` to `committing`.
4. Invalidate the local preview owner before sending `submit=true` with `completionOrigin: { phase: 'commit', pluginId }`.
5. After Rust validates the origin and route, begin the plugin-domain registry query with the new sequence before Runtime dispatch, thereby invalidating the older preview publication in the backend as well.

Enter is inert while that owner is `committing`. The backend dispatches either activation mode after existing input-required checks. The old preview's late success or failure cannot change hint, results, status, search-pending state, or completion-origin state because both frontend ownership and registry sequence are stale. When the commit request settles, a still-matching owner transitions to `consumed` whether it returned a required-input hint, succeeded, failed, or had an uncertain outcome.

If no next query sequence can be allocated, the launcher sends no request, invalidates the preview owner, transitions the exact owner to `consumed`, marks the entire invocation sequence-exhausted, clears preview-pending state, and shows `查询次数已达上限，请重新打开主界面。`. Sequence-exhausted is an absorbing invocation-level state and takes precedence over every ordinary `consumed` edit/reselection rule below. While exhausted, trusted edits may update only the locally displayed control value; editing arguments or command identity, selecting or reselecting a result, automatic search, and Enter cannot create completion ownership, clear the fixed status, or send any request. Only a new native shown invocation resets query sequence, the exhausted marker, the tombstone, and the fixed status.

If the user edits the argument while `committing`, the frontend immediately replaces A with a new `armed` owner B bound to the new query sequence and exact value, then dispatches B's preview. A may finish already-admitted plugin side effects, but A's late success, failure, hint, main-result publication, window-transfer commit, status update, and state transition are all rejected by registry and frontend ownership. Editing the command identity instead clears completion-origin state and follows independently typed command behavior.

`consumed` is a tombstone bound to the same invocation, control, command, and exact value. It blocks preview and the key handler's plugin-command submission fallback for that untouched value; it does not block activation of a valid currently selected result returned by the committed request. Any trusted argument edit while `consumed` creates a fresh `armed` owner for the updated value; reselecting a host-generated plugin completion also creates a fresh owner. A different result activation, command-identity edit, view change, hide, or new launcher invocation clears the state.

A command which was typed independently and has no completion-origin state retains the existing activation matrix: `live + submit=false` dispatches, `live + submit=true` is ignored, and `submit + submit=true` dispatches.

Find and browser-search rows retain their existing direct actions for nonempty plain text. Their behavior does not depend on plugin favorite state.

## Persistent State

`PluginStateDocument` gains:

```rust
#[serde(default)]
favorite: bool
```

The existing internal state schema remains readable because missing values default to `false`. `EffectivePluginConfig` exposes the value to the activation manager. Every durable state write preserves it unless an explicit favorite mutation or uninstall changes it.

The state store adds `prepare_set_favorite`, a narrow favorite candidate builder which requires an installed plugin. `PublicPluginManager::set_favorite` owns the full transaction:

1. Under the existing mutation lock, load the current `ActivationBundle` and reject a missing or uninstalled plugin.
2. If `bundle.config.favorite` already equals the target, return success without reserving the bundle slot, writing storage, or advancing inventory revision.
3. Otherwise reserve the plugin's activation slot and prepare the state candidate while the same mutation serialization is held.
4. Release the mutation lock, make the candidate durable through the existing atomic current/backup path and `make_state_durable` reservation phases, then reacquire the mutation lock.
5. Publish the prepared state into `PluginStateStore`, then publish `bundle.with_config(updated_config)` through the same reservation. Client success is returned only after both publications succeed.

The mutation does not invalidate the scheduler, replace or reload Runtime, change generation/activation/admission identities, close data-call gates, or touch plugin windows. The successful reservation publication is the catalog visibility point: a query started afterward in the same process observes the new favorite value.

If persistence is known not to have committed, the reservation rolls back and the prior state and bundle remain current. If durability is unknown, or any step fails after durable commit but before bundle publication, the existing reservation safety contract makes the public-plugin manager terminal/unavailable for the rest of the process; it must not serve a stale bundle as a successful mutation. Restart reloads the durable state. This is the same fail-closed boundary used by other activation metadata mutations.

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

Both `launcher_command_suggestions` and `/prefix` `command_suggestions` apply the complete eligibility predicate, including `runtime_recovery_needed == false`. Favorite does not alter `/prefix` membership.

The backend performs grouping, de-duplication, ordering, and completion generation. The frontend must not merge a second favorite list or infer whether typed text is an argument.

Public-plugin Runtime responses cannot set favorite metadata. Plugin-produced result rows remain ordinary executable or informational results and never gain the favorite context menu.

## Launcher Wire Contract

Host-generated public-plugin rows use a distinct activation variant so plugin identity, completion provenance, and favorite state cannot be separated:

```ts
interface LauncherPluginCompletionActivation {
  kind: 'pluginCompletion'
  completionText: string
  pluginId: string
  favorite: boolean
}
```

Rust adds the corresponding `LauncherResultActivation::PluginCompletion` variant and constructs it only for host public-plugin catalog rows. Its `completionText` must pass the existing completion grammar, `pluginId` must pass the existing public-plugin identifier contract, and unknown fields are rejected. The frontend parser discards a malformed `pluginCompletion` item while preserving valid siblings; it never downgrades it to generic completion.

The existing `activation.kind === 'completion'` remains unchanged for host built-ins such as empty-catalog `/find` and `/web-search`. Generic completion never creates favorite UI or completion-origin plugin state. Plugin Runtime result DTOs cannot produce either host completion variant.

The main-only client gains:

```ts
setPublicPluginFavorite(input: {
  pluginId: string
  favorite: boolean
}): Promise<void>
```

The existing launcher search input gains one optional internal flag:

```ts
interface LauncherSearchInput {
  query: string
  invocationId: string
  querySequence: number
  submit?: boolean
  completionOrigin?: {
    phase: 'preview' | 'commit'
    pluginId: string
  }
}
```

The trusted main frontend may send `completionOrigin` only while its current completion-origin owner matches the exact query and plugin ID. The object is parsed strictly with unknown fields rejected. `phase: 'preview'` requires `submit: false`; `phase: 'commit'` requires `submit: true`. Rust cannot observe WebView-local state identity, so its security boundary is the main-window capability and label guard, but it validates `pluginId` and requires exact equality with the final currently eligible route's `plugin_id`. A missing, invalid, unavailable, recovery-pending, or differently routed plugin is rejected before Runtime dispatch.

For `preview`, `public_plugin_search_decision` returns the existing required-input or manifest-placeholder hint and never dispatches either activation mode. For `commit`, both activation modes dispatch after the existing required-input check. Calls without `completionOrigin` retain the current decision matrix.

The native favorite command name is frozen as `set_plugin_favorite`; the TypeScript adapter exposes it as `setPublicPluginFavorite`. Implementation must register `set_plugin_favorite` in the Tauri invoke handler, generate the narrow `allow-set-plugin-favorite` permission, and add that permission only to `src-tauri/capabilities/main.json`. The command also checks the `main` label before accessing managed state and revalidates that the plugin is installed. Find, plugin-runtime, plugin-window-shell, and plugin-window-content capabilities do not receive the permission. Storage, invalid-plugin, revision-exhaustion, reservation, and terminal failures map to the existing fixed public-plugin management error surface.

## Frontend Ownership And Data Flow

1. A current backend response publishes ordered rows and validated `pluginCompletion` activations.
2. Right-clicking a plugin completion row selects it and opens the in-window context menu. The launcher replaces an opaque interaction token whenever selection changes, a different context menu opens, any result is activated, the user edits, the view changes, the launcher hides, or a new shown invocation starts.
3. Choosing the menu item captures the current interaction-token identity, view epoch, invocation, query sequence, query control key, raw control value, result key, plugin ID, and target favorite value. Consuming and closing that same menu as part of starting the command does not replace the captured token.
4. The frontend sends one favorite mutation and disables repeated favorite commands until it settles.
5. On durable success, the frontend re-runs the captured query only when the captured launcher and control ownership are still current. That refresh receives the new grouping, completion, and star state.
6. If the user edits, moves selection by keyboard or pointer, opens a menu for another row, changes view, hides, or reopens the launcher before completion, the opaque token is replaced. The durable mutation remains successful but its late UI continuation has zero effect. A later ordinary launcher query reads the new favorite state.
7. On failure, only the still-current owner may publish `操作不可用，请重试`. A stale failure has zero UI effect.

The context menu itself is rendered inside the existing WebView component tree. It must not open a native menu, request native focus, or trigger the launcher's focus-loss hide path.

## Failure Behavior

- Missing `favorite` in an existing state file loads as `false`.
- Corrupt plugin state follows the existing quarantine and recovery contract; favorite state does not introduce a second recovery mechanism.
- A nonexistent or uninstalled plugin cannot acquire a favorite record.
- Disabled, faulted, stale-generation, recovery-pending, and uninstalled favorites are omitted rather than shown disabled.
- A catalog read failure preserves Find and browser search and omits plugin rows, matching the existing launcher fallback.
- A stale favorite success or failure cannot replace newer input, results, selection, view, or status.
- A favorite failure known to occur before durable commit leaves the previous favorite value and bundle usable.
- An unknown durable outcome or post-durable publication failure makes public-plugin management unavailable until process restart; the UI reports the fixed operation error and must not claim either favorite value was committed.
- A plugin row whose generated completion violates the frozen completion grammar is omitted while valid built-in and application rows remain available.
- A completion preview never invokes Runtime. A consumed completion owner blocks repeated plugin-command submission after any possibly dispatched request until trusted edit or reselection establishes fresh ownership, but it never blocks activation of a valid selected result.
- Query-sequence exhaustion before commit sends no request and enters an invocation-level absorbing state; edit, reselection, search, and Enter stay local/inert until a new native shown invocation.
- Favorite mutation does not create, destroy, reload, focus, or invoke a plugin Runtime or plugin window.

## Testing

Backend-focused tests cover:

- Old state without `favorite` loads as false and the next write preserves a canonical boolean.
- Favorite set, idempotent set, same-process catalog visibility, restart reload, rename, update, disable/re-enable, both uninstall retention modes, and reinstall.
- Persistence-known-not-committed rollback keeps the old bundle; unknown durability and post-durable state/bundle publication failures enter the existing terminal state and never return success.
- The favorite command is registered in the invoke handler, is allowed by `main.json`, and rejects non-main callers and nonexistent or uninstalled plugin IDs. Find and every plugin Runtime/window capability lack the permission.
- Empty and exact `/` catalogs order Find, Web Search, favorites, then other enabled plugins without application enumeration.
- Plain queries order Find, Web Search, favorites, matching non-favorites, then applications.
- Favorite plus name-match is de-duplicated into the favorite group.
- Favorite `demo-win` produces `/demo-win win` for `win`.
- Non-favorite `demo-win` produces `/demo-win ` for `win`.
- `/d` prefix discovery remains matching-only, excludes Runtime-recovery-pending plugins, and produces no argument.
- Disabled, faulted, stale-generation, and recovery-pending favorites are excluded.
- Per-row completion boundaries cover the largest accepted UTF-8 query, one-byte overflow, control characters, U+2028, and U+2029; invalid plugin rows are omitted without removing valid built-in/application rows.
- Submit and live completions with empty required input immediately publish their manifest placeholder through `completionOrigin.phase = 'preview'` while Runtime invocation count remains zero.
- Trusted argument edits retain preview-only ownership for the same completed command; neither activation mode runs until explicit `completionOrigin.phase = 'commit'`.
- The `armed -> committing -> consumed` matrix covers duplicate Enter while pending and `dispatch may have happened -> later failure -> third Enter`, with zero duplicate dispatch. Editing the argument or reselecting the completion creates a fresh arm; independently typed live commands retain automatic dispatch.
- `commit A pending -> edit argument B -> preview B -> A late success/failure` preserves B's state, hint, results, status, and window ownership while allowing only A's already-admitted side effects to finish.
- `preview P pending -> allocate commit sequence C -> P late success/failure` leaves only C authorized to control hint, results, status, pending state, and completion-origin phase.
- Commit sequence exhaustion sends no backend request, ignores the late preview, and keeps the fixed reopen message through argument edit, command edit, result reselection, automatic-search attempts, and Enter. A new native shown invocation resets the exhausted marker and restores normal search.
- Table-driven request rejection covers preview with `submit=true`, commit with `submit=false`, invalid phase, missing/invalid/unknown-field `pluginId`, Find/Web Search/application queries carrying completion origin, nonexistent or ineligible plugins, and pluginId/route mismatch. Every case rejects before Runtime dispatch.
- Missing, malformed, or unknown-field `pluginCompletion` identity discards only that row and cannot be downgraded to generic completion or produce a first-Enter Runtime call. Generic built-in completion remains valid without plugin identity.

Frontend-focused tests cover:

- Only plugin completion rows open the custom right-click menu.
- Right-click selects the row and shows the correct set/cancel label.
- The filled star appears only for favorite plugin completion rows.
- Menu actions do not complete, execute, hide, or blur the launcher.
- Successful set and cancel refresh the same current query and update ordering, visibility, and star state.
- Cancelling a nonmatching favorite removes it from a nonempty result list after refresh.
- Mutation failure keeps the query and results and shows the fixed error only for the current owner.
- Editing, moving selection by keyboard/pointer, right-clicking another row, changing view, or hiding and reopening during an in-flight mutation makes both late success and late failure inert.
- Ownership includes the opaque interaction token, result key, and plugin ID, so unchanged text alone cannot authorize a late continuation.
- First Enter completes and immediately shows the plugin usage hint without invoking submit or live Runtime. The second Enter alone may invoke the completed command. After an ambiguous failure, third Enter is inert for the consumed untouched value; editing the argument or reselecting creates a fresh arm.
- After a committed main-result plugin returns a selected copy action, the next Enter executes that result while Runtime invocation count remains unchanged; the consumed command tombstone applies only to submission fallback.

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
