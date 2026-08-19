# UiPilot Public Plugin API v1

New to public plugin development? Start with the [third-party plugin developer tutorial](./public-plugin-developer-guide.md), then use this document as the complete contract reference.

Public plugins are static packages loaded by UiPilot. A plugin owns one slash command, one hidden Runtime, and optionally one singleton content window. Removing or disabling the package removes its command; the host does not provide command-specific fallbacks.

## Package Layout

The package root is strict:

```text
plugin.json
icon.png                  # optional, fixed 128x128 plugin identity icon
dist/
  runtime.js
  optional-window.html
  optional-window.js
  optional-window.css
```

Only `plugin.json`, `.html`, `.js`, `.css`, and the optional package-root `icon.png` are accepted. The icon must be a completely decodable, static 128x128 PNG no larger than 128 KiB; other PNG paths and additional PNG files are rejected. Basenames have exactly one extension. Paths are relative, normalized, at most eight components deep, and cannot contain traversal, symlinks, reparse points, remote URLs, `data:` resources, unknown MIME types, or unlisted files. Runtime and window entries must name package JavaScript and HTML files respectively.

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
- `command.summary` is an optional one-line discovery hint. It is limited to 512 Unicode scalar values; when omitted, the launcher uses `name`.
- `command.inputPlaceholder` is the command-input usage hint. It is distinct from the management-page `description` and discovery `summary`.
- `activationMode` is `live` or `submit`; `window` output requires `submit`.
- `outputMode` is static. `window` requires a window entry and `ui.window`; `mainResult` forbids both.

Installation and reload use staging. Static validation and Runtime readiness must succeed before the generation becomes active. A failed upgrade leaves the previous generation usable.

## Runtime Contract

The Runtime entry is an ES module exporting `onCommand` with the `PluginHandler` type from [uipilot-plugin-api-v1.d.ts](./uipilot-plugin-api-v1.d.ts). Each invocation and API object is new, deeply frozen, and bound to an immutable plugin ID, generation, and request ID. On Windows, a plugin that declares and receives `notifications.publish` may submit one notification action during that request: immediate `api.notifications.publish({ content })` or delayed `api.notifications.schedule({ content, delayMs })`.

`invocation.input` has command text and boundary whitespace removed while preserving internal whitespace. `context.invokedAt` is RFC 3339 with a local UTC offset. The API permits only plugin-scoped JSON storage, reads of declared non-secret settings, and the request-bound Windows message operation when granted; Runtime code cannot access Tauri, Shell, files, network, native input, another plugin, or secret plaintext.

Context errors are stable:

- `InvalidContext`: malformed, missing, or forged ownership.
- `ExpiredRequestError`: an issued request was superseded, completed, timed out, cancelled, disabled, uninstalled, upgraded, reloaded, or replaced.
- `InvalidOperation`: unsupported operation, key, value, or response.
- `StorageError`: an atomic storage operation failed.
- `RuntimeUnavailable`: the isolated Runtime is unavailable.
- `InvalidNotification`: notification content is not one non-empty, trimmed, single-line plain-text value of at most 500 Unicode scalar values.
- `InvalidDelay`: `delayMs` is not a JavaScript safe integer from 1,000 through 86,400,000.
- `ScheduleLimitExceeded`: the plugin already has 32 pending delayed messages.
- `AlreadyPublished`: the current request already committed its one allowed message.
- `MessageStoreUnavailable`: the host could not atomically persist the message.

Late responses from expired requests are discarded and cannot mutate newer UI or data.

Both notification methods are request-bound. `publish()` resolves at the atomic message-file commit. `schedule()` resolves when the host accepts one immutable task; it does not wait for delivery. `delayMs` must be a JavaScript safe integer in `1_000..=86_400_000`, and each plugin may have at most 32 pending tasks. Hiding windows does not cancel accepted tasks. Disabling, uninstalling, or updating the plugin cancels them, and process exit discards them without recovery. Later Windows toast or tray failures do not reject the completed call or remove a saved message.

`schedule()` runs no plugin code after the request. It is not a general timer or background-task API and provides no query, cancellation, repeating, calendar-time, retry, or cross-restart guarantee. Notification content remains plain text; actions, links, markup, and arbitrary payloads are unsupported.

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
- `notifications.publish`: on Windows only, submit one immediate or host-owned delayed plain-text message and ask the host to show its own notification and tray reminder.

Other parsed permission names are reserved and installation fails until the host implements them. Permission changes during reload require normal confirmation; no development-package bypass exists.

## Unsupported In v1

Arbitrary background execution, plugin-owned timers, repeating or persistent scheduling, multiple commands, multiple windows, streaming, pagination, large responses, network, arbitrary files, clipboard read, native binaries, Shell, input synthesis, plugin-to-plugin communication, remote media, dependencies, signing, marketplace delivery, and automatic updates are outside this MVP. The only delayed operation is the host-owned, process-local `notifications.schedule()` message described above.

The fixed-output reference packages are:

- `examples/public-plugins/com.uipilot.demo-win`: Windows-only `submit + window` with `ui.window` and a 10-second host-owned delayed message.
- `examples/public-plugins/com.uipilot.demo-return`: `submit + mainResult` with `clipboard.write`.

Each README documents its development-directory installation, focused verification, packaging command, and user-operated acceptance flow.
