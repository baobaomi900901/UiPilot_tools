# UiPilot Public Plugin API v1

Public plugins are static packages loaded by UiPilot. A plugin owns one slash command, one hidden Runtime, and optionally one singleton content window. Removing or disabling the package removes its command; the host does not provide command-specific fallbacks.

## Package Layout

The package root is strict:

```text
plugin.json
dist/
  runtime.js
  optional-window.html
  optional-window.js
  optional-window.css
```

Only `plugin.json`, `.html`, `.js`, and `.css` files are accepted. Basenames have exactly one extension. Paths are relative, normalized, at most eight components deep, and cannot contain traversal, symlinks, reparse points, remote URLs, `data:` resources, unknown MIME types, or unlisted files. Runtime and window entries must name package JavaScript and HTML files respectively.

Use [uipilot-plugin-v1.schema.json](./uipilot-plugin-v1.schema.json) to validate `plugin.json`. The schema is generated from the Rust DTOs by:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema -- --check
```

## Manifest Identity

- `schemaVersion` and `apiVersion` are `1`.
- `pluginId` is stable, lowercase, and globally identifies storage, permissions, generations, and windows.
- `version` and `minimumHostVersion` are canonical `major.minor.patch` numbers.
- `supportedPlatforms` contains `windows` and/or `macos` without duplicates.
- `command.defaultName` is the initial slash command name. Users may rename it, but reserved and conflicting names fail closed.
- `activationMode` is `live` or `submit`; `window` output requires `submit`.
- `outputMode` is static. `window` requires a window entry and `ui.window`; `mainResult` forbids both.

Installation and reload use staging. Static validation and Runtime readiness must succeed before the generation becomes active. A failed upgrade leaves the previous generation usable.

## Runtime Contract

The Runtime entry is an ES module exporting `onCommand` with the `PluginHandler` type from [uipilot-plugin-api-v1.d.ts](./uipilot-plugin-api-v1.d.ts). Each invocation and API object is new, deeply frozen, and bound to an immutable plugin ID, generation, and request ID.

`invocation.input` has command text and boundary whitespace removed while preserving internal whitespace. `context.invokedAt` is RFC 3339 with a local UTC offset. The API permits only plugin-scoped JSON storage and reads of declared non-secret settings; Runtime code cannot access Tauri, Shell, files, network, native input, another plugin, or secret plaintext.

Context errors are stable:

- `InvalidContext`: malformed, missing, or forged ownership.
- `ExpiredRequestError`: an issued request was superseded, completed, timed out, cancelled, disabled, uninstalled, upgraded, reloaded, or replaced.
- `InvalidOperation`: unsupported operation, key, value, or response.
- `StorageError`: an atomic storage operation failed.
- `RuntimeUnavailable`: the isolated Runtime is unavailable.

Late responses from expired requests are discarded and cannot mutate newer UI or data.

## Responses

`mainResult` returns zero to twenty plain-text items. IDs are non-empty and unique. Titles are at most 256 Unicode scalar values, subtitles at most 512, and details at most 16 KiB UTF-8. An item may have one `copyText` default action when `clipboard.write` is declared and granted. `actions[]`, custom labels, callbacks, links, commands, HTML, Markdown, and multiple actions are unsupported.

`window` returns `{ requestId, data }`. The host creates or reuses one window for that plugin and sends a `PluginWindowUpdate` through `window.uipilotPluginWindow.onUpdate`. Content receives input, platform, theme, invocation time, singleton instance `1`, and plugin data. It cannot invoke commands or own pin, close, drag, focus, theme, or position behavior.

The complete serialized response budget is 64 KiB. Unknown fields, duplicate keys, non-finite numbers, prototype keys, invalid actions, and over-budget responses reject the entire response.

## State, Timing, And Faults

- Private JSON storage is limited to 5 MiB per plugin and fails atomically at the limit.
- `live` dispatches use a 150 ms frontend debounce and a 5 second post-dispatch timeout.
- `submit` dispatches only after Enter and use a 30 second post-dispatch timeout.
- Each plugin generation has at most one running request and one latest waiting request.
- Content ready and update acknowledgement each have a 5 second timeout.
- Three consecutive runtime faults within five minutes persistently disable the plugin; a successful request or manual enable resets the relevant fault state.

Settings support `text`, `secret`, `number`, `boolean`, and `select`. Keys match `^[a-z][a-z0-9.-]{0,63}$` and remain stable across upgrades. Secrets have no default and Runtime code can only ask whether one is configured.

## Permissions

API v1 implements only:

- `ui.window`: create the host-owned singleton window.
- `clipboard.write`: expose a host-owned `copyText` default action.

Other parsed permission names are reserved and installation fails until the host implements them. Permission changes during reload require normal confirmation; no development-package bypass exists.

## Unsupported In v1

Background execution, timers, scheduling, multiple commands, multiple windows, streaming, pagination, large responses, network, arbitrary files, clipboard read, native binaries, Shell, input synthesis, plugin-to-plugin communication, remote media, dependencies, signing, marketplace delivery, and automatic updates are outside this MVP.

The reference package is under `examples/public-plugins/com.uipilot.demo`. Its README describes both static output modes and the user-operated acceptance flow.
