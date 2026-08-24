# Public Plugin Command Autocomplete Design

**Status:** Draft - approved design, awaiting written-spec review

## Goal

Complete the public-plugin command discovery contract in the main launcher. Typing a slash prefix must show eligible installed plugins, Enter on a selected plugin must complete the input instead of executing it, and the completed command must show a manifest-owned usage hint until a submit-mode plugin is invoked.

This design narrows and extends the command-discovery requirements in `2026-08-13-public-plugin-command-window-mvp-design.md`. It does not change plugin Runtime invocation, result actions, window transfer, settings, permissions, or lifecycle contracts.

## User Contract

- `/` lists every eligible public plugin. `/prefix` lists plugins whose effective command name starts with the prefix or whose display name matches it.
- Only installed, enabled, healthy plugins whose active generation is current are eligible. Disabled or faulted plugins do not appear.
- Results are sorted by effective command name in ascending ASCII order. With the two demo plugins, `/demo-return` precedes `/demo-win`.
- Each result title is `/<effectiveName>`. Its subtitle is `command.summary`, falling back to the manifest `name` when `summary` is absent.
- The first result is selected by default. Existing Up and Down behavior changes the selected result.
- Enter on a plugin-completion result does not invoke the plugin and does not hide the main window. It replaces the complete input with `/<effectiveName><ASCII space>` and keeps the query input focused.
- A no-space exact command such as `/demo-win` remains a completion result. The trailing ASCII space is the explicit transition from discovery to command-input mode.
- For an input-required plugin with an empty body, command-input mode displays `inputPlaceholder` directly below the input as an unselectable usage hint. The hint is not part of the result list and owns no keyboard focus. Submit-mode retention and live-mode replacement follow the rules below.
- For a `submit` plugin, the usage hint remains visible while the user types the body. Enter submits the current command once the body is nonempty. Empty required input never invokes the Runtime.
- For a `live` plugin, empty required input shows the usage hint; after the body becomes nonempty, the existing debounced live invocation and preview behavior resumes.
- Matching, sorting, display, and completion use the host-configured effective name. A customized effective name supersedes the manifest `defaultName`.

## Manifest Contract

`PublicCommandV1` gains one optional field:

```json
{
  "defaultName": "demo-win",
  "summary": "打开演示子窗口",
  "activationMode": "submit",
  "outputMode": "window",
  "inputRequired": true,
  "inputPlaceholder": "请输入信息回车"
}
```

- `summary` is optional in schema version 1, preserving compatibility with already installed packages.
- When present, `summary` must be nonempty single-line plain text, contain no control characters, and contain at most 512 Unicode scalar values. An invalid value rejects the package during staging.
- When absent, discovery uses the manifest `name` as the subtitle. Absence never disables or rejects an otherwise valid existing plugin.
- `description` remains the plugin introduction shown in management UI. It is not used as the command usage hint.
- `inputPlaceholder` remains the command-input usage hint. It is distinct from both `description` and `summary`.
- Generated JSON Schema and public SDK documentation must expose the optional field.

The examples use:

| Plugin | `summary` | `inputPlaceholder` |
| --- | --- | --- |
| `com.uipilot.demo-win` | `打开演示子窗口` | `请输入信息回车` |
| `com.uipilot.demo-return` | `返回示例文本到主界面` | `请输入信息回车` |

## Architecture

### Plugin Catalog

The public-plugin activation manager adds a read-only prefix-query operation. It takes the text after `/` and returns host-owned completion metadata from the current activation snapshot:

- plugin ID and generation for eligibility checks only;
- effective name;
- manifest display name;
- optional command summary.

The operation does not create a plugin request, allocate a Runtime `requestId`, start or focus a window, or call plugin JavaScript. It holds no lock across I/O because discovery requires no I/O.

Eligibility uses the same authoritative state as exact command routing: installed, enabled, no persisted fault, active generation matches the snapshot, and effective name is current. Matching is an ASCII prefix match on effective names or a substring match after applying Unicode lowercase mapping to both the display name and typed prefix. Sorting is always by effective name, so display-name matching cannot change order.

### Search Wire Contract

The existing launcher search wire types gain two host-owned optional fields:

```typescript
interface ResultItem {
  // existing fields omitted
  completionText?: string
}

interface SearchResponse {
  // existing fields omitted
  commandHint?: string
}
```

`completionText` is present only on plugin discovery items and has the canonical form `/<effectiveName> `. The result registry assigns the item an opaque result ID but no `ResultAction`; the item has `hasDefaultAction: false` and cannot be executed through `execute_result`.

`commandHint` is presentation-only response metadata. It is rendered below the input outside the listbox, has no result ID, is not selectable, and creates no result action or execution authorization.

Public plugin Runtime responses cannot supply either field. They are constructed only by the host after manifest and activation-state validation.

The frontend accepts `completionText` only when it matches `^/[a-z][a-z0-9-]{0,31} $`. An item with malformed completion metadata is discarded rather than treated as executable.

### Query Classification

`search_apps` classifies slash input before ordinary application search:

1. `/` or `/prefix` containing no ASCII space is discovery mode. It returns eligible completion items and never dispatches a plugin.
2. `/<effectiveName><ASCII space><body>` is command-input mode and uses exact plugin routing. Other plugins, applications, `/find`, calculator, and browser-search items are not mixed into the response.
3. Required empty input returns `commandHint` and does not dispatch.
4. A nonempty `submit` body with `submit: false` returns the same `commandHint` and does not dispatch.
5. A nonempty `submit` body with `submit: true` follows the existing scheduler and output path.
6. A nonempty `live` body with `submit: false` follows the existing debounced scheduler and output path.

The existing `invocationId + querySequence` ownership governs discovery and hints. A stale response cannot replace newer text, newer completion results, a newer hint, or plugin output.

### Frontend Completion

When Enter activates an item with valid `completionText`, `LauncherCore` performs a normal owned edit using that exact value. It clears stale results and hint state, advances the query sequence, schedules the next search, and retains input focus. It does not call `execute_result`, `hide_launcher`, or any plugin command.

`commandHint` is stored separately from results. It remains visible when a current submit-mode command response contains the hint, while `results` stays empty and `selectedIndex` stays `-1`. Therefore the existing no-result Enter submission path can submit the current slash command without a fake selectable result intercepting the key.

Any user edit retires both the current result list and current hint under the same search owner. Native shown/reset and successful window transfer clear the hint with the other launcher query state.

## Failure Behavior

- Missing `summary` falls back to `name`; it is not an error.
- Invalid `summary` rejects staging atomically and leaves an installed version unchanged.
- Disabled, faulted, stale-generation, or uninstalled plugins disappear from subsequent discovery responses.
- Catalog or activation-state failure returns the launcher's fixed public error state and exposes no internal path, manifest body, plugin ID, or Runtime error.
- Malformed `completionText` is ignored locally and cannot reach `execute_result`.
- A stale discovery, hint, Runtime result, or failure has zero effect on newer input.
- Discovery and completion never hide the main window. Only the existing successful command/window execution contracts may change window lifecycle.

## Testing

Focused automated coverage must prove:

- schema version 1 accepts missing `summary`, accepts a valid summary, and rejects empty, multiline, control-character, and over-512-character values;
- demo manifests expose their exact summary and usage-hint values and pass generated-schema validation;
- `/d` returns `/demo-return` then `/demo-win`, selects the first item, and uses the summary subtitle;
- missing summary falls back to plugin `name`;
- customized effective names participate in filtering, ordering, display, and completion;
- disabled, faulted, and stale-generation plugins are excluded without Runtime dispatch;
- exact no-space commands remain completion items, while the trailing-space form enters command-input mode;
- Enter on a completion item produces the exact command plus one trailing ASCII space, preserves focus, schedules the hint query, and calls neither execute nor hide;
- required submit input shows and retains `commandHint` while typing, then Enter dispatches once; required empty input never dispatches;
- live input shows the hint while empty and resumes existing debounced dispatch when nonempty;
- stale responses cannot overwrite newer completion results, hints, text, or plugin output;
- malformed completion metadata is discarded and has no execution path.

Full frontend tests, Rust tests, schema generation checks, TypeScript compilation, and the production frontend build remain required. Real-window acceptance is manual and must not use synthesized mouse or keyboard input.

## Acceptance

With both demo plugins installed and enabled:

1. Entering `/d` shows `/demo-return` and `/demo-win` in that order, with the first item selected and each manifest summary visible.
2. Moving selection to `/demo-win` and pressing Enter changes the input to `/demo-win ` without hiding the main window or opening the plugin window.
3. The unselectable text `请输入信息回车` appears below the input and remains while a body is typed.
4. Entering a nonempty body and pressing Enter invokes `demo-win` through its existing singleton-window flow.
5. Repeating the flow with `demo-return` invokes its existing main-result and copy-action flow.
