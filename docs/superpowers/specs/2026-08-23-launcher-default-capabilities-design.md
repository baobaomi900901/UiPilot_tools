# Launcher Default Capabilities Design

**Status:** Draft - revision 3 awaiting independent review

**Date:** 2026-08-23

## Goal

Make the launcher useful before the user types anything and make enabled public plugins discoverable without requiring a leading `/`.

The launcher will present built-in capabilities and enabled plugins through the existing result pipeline. It will not synthesize unowned executable rows in the frontend.

## User Contract

### Empty Input

When the launcher input is empty after removing leading and trailing whitespace, show these rows in order:

1. `/find`
2. `/web-search`
3. Every enabled public plugin, ordered by its effective activation command using case-insensitive A-Z ordering

Do not show computer applications for empty input. The first row is selected by default.

Activating a row only completes the input:

- `/find` becomes `/find `.
- `/web-search` becomes `/web-search `.
- A plugin becomes its effective command plus a trailing space, for example `/demo-win `.

Completion does not open a window, browser, application, or plugin. The user may add an argument and press Enter again.

### Plain Text

For non-command text, show rows in this order:

1. Find for the current text
2. Browser search for the current text
3. Enabled plugins whose effective activation command contains the text, ordered case-insensitively by activation command
4. Matching computer applications

Plugin matching is case-insensitive containment. Both `d` and `win` therefore match `demo-win`.

Find and browser-search rows retain their current direct action: choosing either one searches the current text immediately.

Choosing a plugin completes rather than executes:

- If the trimmed input equals `demo-win` or `/demo-win`, complete to `/demo-win `.
- Otherwise prepend the command and one space to the trimmed input. For example, choosing `demo-win` for `da` produces `/demo-win da`.
- Preserve internal spaces in the original input. Remove only leading and trailing whitespace.

The completed plugin command is invoked only after the user presses Enter again.

### Completion Text Contract

Every completion uses one of these two forms:

```text
"/<command> "
"/<command> <argument>"
```

`command` matches `[a-z][a-z0-9-]{0,31}`. The no-argument form ends with exactly one ASCII space. In the argument form, the argument is non-empty after trimming, has no leading or trailing whitespace, preserves internal spaces, and contains no NUL, carriage return, line feed, `U+2028`, `U+2029`, or Unicode control character. The complete `completionText` is at most 65,536 UTF-8 bytes.

The backend generator and frontend parser enforce this same contract. A result item that violates it is discarded as malformed rather than repaired or executed. Other valid items in the same response remain usable; one malformed completion does not discard the whole response or create a global error by itself.

### Command Text

The existing contracts remain valid:

- `/find <query>` opens the singleton Find window for `<query>`.
- Existing partial slash command completion such as `/d` continues to complete a plugin command without treating `/d` as the plugin argument.
- Existing exact plugin commands continue to invoke their plugins.

Add the reserved built-in command:

```text
/web-search <query>
```

It searches `<query>` with the search engine currently selected in persisted settings. `/web-search` without an argument does not open the browser and exposes the hint `请输入搜索内容`.

`web-search` is a host-reserved activation name. Plugin installation and activation-name updates must reject a collision with it using the same mechanism already used for other host-reserved commands.

Calculator recognition and its existing result priority remain unchanged.

Classification priority is fixed as:

1. A valid calculator expression.
2. Empty input after outer trimming.
3. Host-reserved `/find` and `/web-search` commands.
4. Public-plugin slash discovery or exact invocation.
5. Plain text.

A valid calculator expression returns only the existing calculator result. It is not combined with Find, browser-search, plugin, or application rows.

## Launcher Result Activation Protocol

Every item in the main-launcher `SearchResponse` carries one discriminated activation value:

```ts
type LauncherResultActivation =
  | { kind: 'completion'; completionText: string }
  | { kind: 'openFind'; query: string }
  | { kind: 'executeResult' }
```

The combinations are closed:

- `completion` requires a valid `completionText`, has no executable `ResultAction`, and must never call `execute_result`.
- `openFind` requires the plain-text query captured by that response, has no ordinary `ResultAction`, and invokes the dedicated `open_find_window` transaction.
- `executeResult` requires a non-empty result ID authorized by the current result-registry request and is the only kind that calls `execute_result`.

Empty-input Find, web-search, and plugin rows use `completion`. A plain-text Find row uses `openFind`. Plain-text browser search, calculator, plugin-produced main results, and computer applications use `executeResult`.

`openFind` deliberately remains outside `ResultAction`; the frontend may activate it only while the response invocation, query sequence, view epoch, control key, and control value are current. It passes that exact owner to `open_find_window`, whose existing conditional retirement/CAS path rejects a stale activation.

## Architecture

The backend `search_apps` pipeline remains the single source of launcher rows. It will generate built-in capability rows, completion-only plugin rows, and executable application rows from one current query snapshot. The frontend's current synthetic `localFindResult` is removed.

The frontend must not independently query or merge a second capability list. This preserves one ordering, one selected index, and the existing query-ownership rules.

Executable rows continue to receive result-registry authorization. Completion-only and `openFind` rows are published under the current response owner but receive no ordinary result action.

The public-plugin inventory uses each enabled plugin's effective activation command after user settings have been applied. Manifest defaults do not override the effective command. Disabled, failed, unavailable, or uninstalled plugins are absent.

## Data Flow

1. Every native show event establishes a backend-registered fresh invocation. When that event targets the Launcher, the frontend increments its application query sequence from zero to one and immediately requests the empty-input snapshot.
2. Every user edit increments the query sequence. Clearing a non-empty input or entering only whitespace still starts a new empty-input query; it is not treated as a local clear-only operation.
3. The control retains the user's raw value for ownership. The backend uses the outer-trimmed value for classification and completion arguments.
4. The backend applies the fixed classifier priority and builds one response. For empty input, it builds only built-in and enabled-plugin completion rows. For plain text, it builds Find and browser actions, matching plugin completion rows, and matching application actions.
5. It publishes one ordered response for the current query owner.
6. The frontend accepts the response only if its invocation, sequence, view epoch, control key, and raw control value are still current.
7. Activating a completion row replaces the input, increments the sequence, and starts a new query generation. Activating `openFind` uses the dedicated current-owner transaction. Activating `executeResult` uses the current registry request.
8. Local navigation between Settings and the Launcher during one native show keeps the same invocation and never resets its query sequence. Returning to the Launcher increments the current sequence and requests a new empty snapshot even if the previous Launcher view ended empty.
9. Hiding and later showing the main window is different from local navigation: the backend `on_show` path registers and supplies a fresh invocation, resets the frontend sequence to zero for that invocation, and the Launcher's first empty query uses sequence one.

## Failure Behavior

- A stale response never replaces newer text, selection, or results.
- Continuing to edit immediately invalidates the previous completion owner.
- A stale `openFind` row cannot begin a focus transfer; both the frontend owner check and backend CAS must reject it.
- If the enabled-plugin inventory cannot be read, Find and browser search remain available and plugin rows are omitted.
- Empty input does not start application discovery, file search, browser navigation, or plugin execution.
- `/web-search` with an empty argument never opens a browser.
- An invalid completion value is rejected by the shared completion-contract parser and cannot be executed as a result action.
- Rejecting one malformed completion item preserves all other valid items from the same current response and does not display an operation error solely for that item.
- Observing an empty or whitespace-only edit clears the old visible results immediately, then publishes only the new empty-snapshot response if it remains current.

## Compatibility

This change does not alter:

- The singleton Find-window protocol.
- Public-plugin invocation, window, result, permission, or lifecycle contracts.
- Application result execution authorization.
- Calculator parsing and execution.
- Persisted search-engine settings.
- The application identifier or user-data locations.

The reserved-command set expands only by `web-search`.

## Testing

Backend-focused tests cover:

- Empty-input contents, ordering, and omission of computer applications.
- Classifier priority, including a calculator expression returning only its existing result.
- Enabled-only plugin inventory and effective-command ordering.
- Case-insensitive containment for `d`, `win`, and mixed-case text.
- Completion values for empty input, exact bare plugin names, exact slash plugin names, and unrelated plain text.
- Completion grammar for `/demo-win `, `/demo-win da`, preserved internal spaces, outer trimming, byte exhaustion, NUL, line separators, and control characters.
- Direct `/web-search <query>` execution with each persisted engine.
- Empty `/web-search` behavior and hint.
- Installation and settings rejection for a plugin activation name that collides with `web-search`.
- Preservation of calculator, Find, slash completion, plugin invocation, and application behavior.

Frontend-focused tests cover:

- Default selection of the first empty-input row.
- Empty query on first show, non-empty then clear, whitespace-only edit, and a new empty query after reopening the Launcher.
- Multiple queries followed by local navigation to Settings and back, proving the shared invocation continues with a strictly higher sequence and the empty snapshot succeeds.
- Arrow and mouse selection using the backend order.
- Completion rows update the input without hiding the launcher or invoking an action.
- The second Enter executes only after a complete command has been entered.
- A stale response or completion cannot overwrite newer input.
- A mixed response drops one malformed completion item while retaining its valid Find, web-search, plugin, and application items.
- A stale `openFind` row cannot invoke `open_find_window`; a current row passes the exact owning invocation and sequence.

No real-window, foreground-focus, mouse, or keyboard automation is required.

## Acceptance Criteria

- Opening the launcher with empty input displays `/find`, `/web-search`, then all enabled plugins.
- Typing `d` displays Find, browser search, matching enabled plugins, then matching applications.
- Typing `win` can discover `demo-win` without a leading slash.
- Selecting `demo-win` for `da` produces `/demo-win da` and does not invoke the plugin yet.
- Selecting `demo-win` for `demo-win` or `/demo-win` produces `/demo-win `.
- Selecting an empty-input capability only completes its command.
- `/web-search UiPilot` searches with the persisted engine.
- Existing calculator, Find, plugin, and application workflows continue to pass their regression tests.
