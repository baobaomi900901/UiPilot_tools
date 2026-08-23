# Launcher Default Capabilities Design

**Status:** Draft - user-approved interaction, awaiting written-spec confirmation

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

## Architecture

The backend `search_apps` pipeline remains the single source of launcher rows. It will generate built-in capability rows, completion-only plugin rows, and executable application rows from one current query snapshot.

The frontend must not independently query or merge a second capability list. This preserves one ordering, one selected index, and the existing query-ownership rules.

Executable rows continue to receive result-registry authorization. Completion-only rows carry a backend-validated `completionText`; activating them updates the current input locally and does not call `execute_result`.

The public-plugin inventory uses each enabled plugin's effective activation command after user settings have been applied. Manifest defaults do not override the effective command. Disabled, failed, unavailable, or uninstalled plugins are absent.

## Data Flow

1. The frontend publishes the current trusted input using the existing invocation and query sequence.
2. The backend classifies empty input, plain text, reserved commands, plugin slash commands, and calculator expressions.
3. For empty input, it builds only built-in and enabled-plugin completion rows.
4. For plain text, it builds Find and browser actions, matching plugin completion rows, and matching application actions.
5. It publishes one ordered response for the current query owner.
6. The frontend accepts the response only if its invocation, sequence, view epoch, and control ownership are still current.
7. Activating a completion row replaces the input and starts a new query generation. Activating an executable row uses the existing authorized execution path.

## Failure Behavior

- A stale response never replaces newer text, selection, or results.
- Continuing to edit immediately invalidates the previous completion owner.
- If the enabled-plugin inventory cannot be read, Find and browser search remain available and plugin rows are omitted.
- Empty input does not start application discovery, file search, browser navigation, or plugin execution.
- `/web-search` with an empty argument never opens a browser.
- An invalid completion value is rejected by the existing frontend parser and cannot be executed as a result action.

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
- Enabled-only plugin inventory and effective-command ordering.
- Case-insensitive containment for `d`, `win`, and mixed-case text.
- Completion values for empty input, exact bare plugin names, exact slash plugin names, and unrelated plain text.
- Direct `/web-search <query>` execution with each persisted engine.
- Empty `/web-search` behavior and hint.
- Installation and settings rejection for a plugin activation name that collides with `web-search`.
- Preservation of calculator, Find, slash completion, plugin invocation, and application behavior.

Frontend-focused tests cover:

- Default selection of the first empty-input row.
- Arrow and mouse selection using the backend order.
- Completion rows update the input without hiding the launcher or invoking an action.
- The second Enter executes only after a complete command has been entered.
- A stale response or completion cannot overwrite newer input.

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
