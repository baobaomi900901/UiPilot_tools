# UiPilot Public Plugin API v1

New to public plugin development? Start with the [third-party plugin developer tutorial](./public-plugin-developer-guide.md), then use this document as the complete contract reference.

Public plugins are static packages loaded by UiPilot. A plugin owns one slash command, one hidden Runtime, and optionally one singleton content window. Removing or disabling the package removes its command; the host does not provide command-specific fallbacks.

## Package Layout

The package root is strict:

```text
plugin.json
icon.png                  # optional, fixed 128x128 plugin identity icon
assets/
  sounds/
    timer-alarm.wav       # required only for timer.control
dist/
  runtime.js
  optional-window.html
  optional-window.js
  optional-window.css
```

Only `plugin.json`, `.html`, `.js`, `.css`, the optional package-root `icon.png`, and the permission-bound fixed alarm path are accepted. The icon must be a completely decodable, static 128x128 PNG no larger than 128 KiB; other PNG paths and additional PNG files are rejected. A plugin declaring `timer.control` must contain exactly one WAV at `assets/sounds/timer-alarm.wav`; a package without that permission must not contain the WAV, and all other WAV paths are rejected. Basenames have exactly one extension. Paths are relative, normalized, at most eight components deep, and cannot contain traversal, symlinks, reparse points, remote URLs, `data:` resources, unknown MIME types, or unlisted files. Runtime and window entries must name package JavaScript and HTML files respectively.

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
- `activationMode` is `live` or `submit`; `window` and `panel` output require `submit`.
- `outputMode` is static. `window` requires a window entry and `ui.window`; `panel` requires a panel entry and `ui.panel`; `mainResult` forbids window/panel entries and both UI permissions. Panel packages require `minimumHostVersion >= 0.3.0`; a non-empty `panel.hostKeys` declaration requires `minimumHostVersion >= 0.3.1`.
- Windows Runtime HTTPS requires `minimumHostVersion >= 0.3.2`, exact permission `network.https`, and a non-null `network` object containing `httpsHosts`. The permission and object must either both be present or both be absent.

`network.https` declares exact destination hosts, not URLs, origins, wildcard suffixes, or redirect groups:

```json
{
  "minimumHostVersion": "0.3.2",
  "supportedPlatforms": ["windows"],
  "permissions": ["network.https"],
  "network": {
    "httpsHosts": ["api.example.com"]
  }
}
```

`httpsHosts` contains one to eight unique canonical lowercase ASCII DNS hosts and is serialized in ASCII order. Each host is at most 253 bytes, has at least two valid DNS labels, and excludes IP literals, ports, schemes, paths, wildcards, raw Unicode/IDN, `localhost`, `.localhost`, and `.local`. Unknown fields, `network: null`, invalid values, and macOS validation fail closed. Installation shows the complete sorted host list and requires explicit consent. Adding hosts on update requires new consent; removing hosts narrows authority without new consent. Settings can revoke access immediately or regrant it only after confirming the current complete host list.

Installation and reload use staging. Static validation and Runtime readiness must succeed before the generation becomes active. A failed upgrade leaves the previous generation usable.

## Runtime Contract

The Runtime entry is an ES module exporting `onCommand` with the `PluginHandler` type from [uipilot-plugin-api-v1.d.ts](./uipilot-plugin-api-v1.d.ts). Each invocation and API object is new, deeply frozen, and bound to an immutable plugin ID, generation, and request ID. On Windows, a plugin that declares and receives `notifications.publish` may submit one notification action during that request: immediate `api.notifications.publish({ content })` or delayed `api.notifications.schedule({ content, delayMs })`. A Manifest declaring `network.https` receives the optional frozen `api.network` facade; all authority is still revalidated by the Host for every call.

`invocation.input` has command text and boundary whitespace removed while preserving internal whitespace. `context.invokedAt` is RFC 3339 with a local UTC offset. The API permits only plugin-scoped JSON storage, reads of declared non-secret settings, the request-bound Windows message operation when granted, and bounded Host-managed HTTPS when declared and authorized. Runtime code cannot access Tauri, Shell, files, native input, another plugin, secret plaintext, raw sockets, or browser/WebView networking.

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

### Host-Managed HTTPS

`api.network?.request(input)` is Windows-only in Host `0.3.2`. It is available only to a Runtime whose Manifest declares `network.https`; the optional TypeScript property lets plugins detect an older or unavailable Host surface. It accepts:

```ts
api.network.request({
  url: 'https://api.example.com/translate',
  method: 'POST',
  headers: { authorization: 'Bearer test-token' },
  body: { type: 'json', value: { text: invocation.input } },
})
```

`method` is exactly `GET` or `POST`. GET rejects a body. POST may omit the body or use `{ type: 'json' | 'text' | 'form', value }`; the Host encodes compact UTF-8 JSON, UTF-8 plain text, or sorted `application/x-www-form-urlencoded` data and owns `Content-Type`. JSON rejects cycles, functions, BigInt, undefined object members, and non-finite numbers. Form values must be strings.

Request headers are an optional string map. Custom provider headers including `authorization`, `accept`, and `accept-language` are allowed. The Host rejects protected or connection-sensitive headers, including `host`, `content-length`, `content-type`, cookies, origin/referrer, user-agent, accept-encoding, hop-by-hop headers, proxy headers, every `sec-*`, `forwarded`, `via`, and every `x-forwarded-*`. Header names are case-insensitive and duplicates after lowercase normalization are invalid.

The URL must use HTTPS, exact port 443, no credentials or fragment, and one currently granted exact host. HTTP, IP literals, localhost/private/special-use DNS answers, environment/system proxies, and TLS verification bypass are denied. Redirects are manual, limited to three, and must remain on the original exact host even if another host is declared. TLS uses the Windows trust store and TLS 1.2 or newer. The WebView CSP remains network-closed and no generic `fetch` proxy is exposed.

The Promise resolves for every final HTTP status, including 4xx and 5xx, as `{ status, headers, body }`. Header names are lowercase; repeated safe values remain arrays. Cookies, hop-by-hop fields, and other protected response headers are omitted. The body must be strict UTF-8 text; the Host does not parse JSON, cache responses, or retain cookies.

Fixed resource limits are:

| Resource | Limit |
| --- | ---: |
| URL | 2048 UTF-8 bytes |
| Request headers | 32 fields / 16 KiB total |
| Encoded request body | 64 KiB |
| Response headers | 64 fields / 32 KiB total |
| Response body | 1 MiB |
| Total deadline | 10 seconds |
| Same-host redirects | 3 |
| Calls per command context | 8 |
| Concurrent calls per command context | 2 |
| Concurrent public-plugin calls Host-wide | 16 |

The Promise rejects with an `Error` whose exact `name` is one of:

| Error name | Meaning |
| --- | --- |
| `InvalidNetworkRequestError` | Invalid URL, method, headers, body, encoding, or request-size limit |
| `PermissionDeniedError` | Permission absent/revoked/unsupported, or no exact durable host grant |
| `NetworkTargetDeniedError` | Scheme, host, address, port, DNS answer, or redirect target denied |
| `NetworkTimeoutError` | Total ten-second deadline elapsed |
| `NetworkFailureError` | DNS, TLS, connect, write, or read failure without a narrower policy error |
| `NetworkResponseTooLargeError` | Response header or body limit exceeded |
| `NetworkResponseInvalidError` | Invalid framing, content encoding, header value, or UTF-8 body |
| `NetworkLimitExceededError` | Per-context call/concurrency or Host-wide concurrency limit exhausted |
| `ExpiredRequestError` | Request replaced/completed/cancelled or plugin disabled/upgraded/uninstalled/torn down |

Errors are deliberately redacted: they expose no URL, query, request/response headers, body, resolved address, certificate, provider payload, or native-library message. Disabling, uninstalling, upgrading, revoking permission, replacing/expiring the command, Runtime teardown, and Host shutdown cancel matching in-flight requests and discard stale responses. Calls do not survive the command context and cannot be used as background work.

Bundled test credentials are ordinary plugin code and can be inspected. API v1 still exposes only `settings.isSecretConfigured()` for secret settings; it does not let Runtime read or inject secret plaintext into a network request. Production secret consumption is deferred to a later, separately versioned Host contract.

## Responses

`mainResult` returns zero to twenty plain-text items. IDs are non-empty and unique. Titles are at most 256 Unicode scalar values, subtitles at most 512, and details at most 16 KiB UTF-8. An item may have one `copyText` default action when `clipboard.write` is declared and granted. `actions[]`, custom labels, callbacks, links, commands, HTML, Markdown, and multiple actions are unsupported.

`window` returns `{ requestId, data }`. The host creates or reuses one window for that plugin and sends a `PluginWindowUpdate` through `window.uipilotPluginWindow.onUpdate`. Content receives input, platform, theme, invocation time, singleton instance `1`, and plugin data. It cannot invoke commands or own pin, close, drag, focus, theme, or position behavior.

`panel` returns `{ requestId, data }`. The host mounts one launcher panel session for that plugin and sends a `PluginPanelUpdate` through `window.uipilotPluginPanel.onUpdate`. Panel content receives input, platform, theme, invocation time, session epoch, and plugin data. Its bridge exposes private storage plus `onHostKey(handler)`, `focusHostInput()`, and `requestHide()`; it cannot own timers or publish notifications. The base panel contract requires host `0.3.0+`; the three `0.3.1` methods require `minimumHostVersion >= 0.3.1` when used.

`window.uipilotPluginPanel.focusHostInput()` moves keyboard focus to the live panel session's tagged launcher argument input. It preserves the panel, command tag, argument text, selection, and submission state. Repeated calls while that input is already focused succeed. Missing, replaced, torn-down, or stale sessions resolve without side effects; a current native-focus, event, acknowledgement, or timeout failure rejects with `windowFailed`. The method does not stream edits to panel content: argument changes reach `onUpdate.input` only after the user presses Enter.

### Panel Host Keys And Return

`panel.hostKeys` is optional, contains at most eight unique declarations, and accepts only `ArrowDown`, `ArrowUp`, and `Primary+N`. Validators reject unknown values, duplicates, non-arrays, wrong element types, and extra panel properties. Canonical order is `ArrowDown < ArrowUp < Primary+N`.

| Declaration | Launcher input match | Delivered `event.key` |
| --- | --- | --- |
| `ArrowDown` | no modifiers | `ArrowDown` |
| `ArrowUp` | no modifiers | `ArrowUp` |
| `Primary+N` | Windows Ctrl-only+N; macOS Meta-only+N; no Alt/Shift | `n` |

IME composition, undeclared keys, ordinary characters, and extended chords are never routed. The launcher consumes a matching physical key before enqueue. Host delivery is strictly serial with a queue depth of eight and a two-second acknowledgement timeout. Queue-full presses remain consumed but are not delivered. A handler throw/rejection is acknowledged without retry; a hung handler, unsubscribe, sequence violation, or counter exhaustion ends the exact panel session instead of overlapping handlers.

When `hostKeys` is non-empty, content must register exactly one `onHostKey(handler)` before ready. A second registration throws `TypeError`. Calling `onHostKey` with omitted/empty `hostKeys` throws `TypeError` and permanently fails ready for that document even if plugin code catches it. The handler receives a deeply frozen event containing real modifier bits plus canonical-decimal `sessionEpoch` and `routeSequence`. Unsubscribe ends and hides the session.

`requestHide(): Promise<void>` takes no arguments. A current session resolves after hide admission and before teardown; the document may be destroyed on the next macrotask, so the resolving continuation must not start later DOM work. Missing, stale, replaced, or in-pattern unauthorized sessions resolve as no-ops. Admission failure rejects with `windowFailed`. If the renderer hangs or crashes before observing admission, the Promise may never settle; Host reclaims an unobserved admission after 30 seconds. Once observed, a 500 ms fallback protects the normal next-macrotask commit.

Escape in panel content uses capture-phase arbitration. A synchronous `preventDefault()`, active `dialog[open]`, or composition suppresses hide; `preventDefault()` after an `await` is too late. Explicit-return hides best-effort restore the external HWND+PID captured when UiPilot was shown. Blur and launch-handoff hides never restore, and foreground restore failure does not turn a successful hide into an error.

Every plugin content window also sees the frozen `window.uipilotPluginWindow.timer` facade. Calls require the Windows-only `timer.control` permission together with `ui.window` and `notifications.publish`; unpermitted callers receive `PermissionDenied`. The host owns one process-local timer per active plugin generation, continues it while the window is hidden, and discards it on process exit.

Every valid plugin content window also receives the frozen `window.uipilotPluginWindow.storage` facade without an additional permission. It shares the Runtime `api.storage` namespace for that plugin. `get` is available while the window session is prepared or active; `set` and `remove` are active-only. A hidden, replaced, disabled, upgraded, or uninstalled session returns `ExpiredWindowSessionError`. Keys must match `^[a-z][a-z0-9.-]{0,63}$` and cannot be `__proto__`, `prototype`, or `constructor`; values are finite JSON and count toward the same 5 MiB plugin quota.

A `timer.control` package supplies its own fixed `assets/sounds/timer-alarm.wav`, while the host exclusively validates, freezes, and plays it. The WAV must be little-endian PCM with one or two channels, a 44.1 or 48 kHz sample rate, and 16- or 24-bit samples; it must not exceed 2 MiB or 15 seconds. Unknown or duplicate chunks, trailing bytes, additional WAV files, and Runtime/content access to the alarm are rejected. Timer completion loops the alarm frozen at round start until the main window opens. Ordinary plugin messages use the host's shared one-shot notification sound instead. Full session, revision, pause/reset, completion-message, and alarm behavior is documented in the developer guide.

The complete serialized response budget is 64 KiB. Unknown fields, duplicate keys, non-finite numbers, prototype keys, invalid actions, and over-budget responses reject the entire response.

## State, Timing, And Faults

- Private JSON storage is shared by Runtime and content-window facades, limited to 5 MiB per plugin, and fails atomically at the limit.
- `live` dispatches use a 150 ms frontend debounce and a 5 second post-dispatch timeout.
- `submit` dispatches only after Enter and use a 30 second post-dispatch timeout.
- Each plugin generation has at most one running request and one latest waiting request.
- Content ready and update acknowledgement each have a 5 second timeout.
- Three consecutive runtime faults within five minutes persistently disable the plugin; a successful request or manual enable resets the relevant fault state.

Settings support `text`, `secret`, `number`, `boolean`, and `select`. Keys match `^[a-z][a-z0-9.-]{0,63}$` and remain stable across upgrades. Secrets have no default and Runtime code can only ask whether one is configured.

## Permissions

API v1 implements only:

- `ui.window`: create the host-owned singleton window.
- `ui.panel`: mount the host-owned launcher panel surface.
- `clipboard.write`: expose a host-owned `copyText` default action.
- `network.https`: on Windows Host `0.3.2+`, expose bounded request-scoped Host-managed HTTPS to the exact authorized `network.httpsHosts` set.
- `notifications.publish`: on Windows only, submit one immediate or host-owned delayed plain-text message and ask the host to show its own notification and tray reminder.
- `timer.control`: on Windows only, control one host-owned plugin-window timer that can continue after the window hides; requires `ui.window`, `notifications.publish`, and `submit + window`.

Other parsed permission names are reserved and installation fails until the host implements them. Permission changes during reload require normal confirmation; no development-package bypass exists.

## Unsupported In v1

Arbitrary background execution, plugin-owned timers, repeating or persistent scheduling, multiple commands, multiple windows, streaming, pagination, large responses, browser/WebView networking, raw sockets, arbitrary files, clipboard read, native binaries, Shell, input synthesis, plugin-to-plugin communication, remote media, dependencies, signing, marketplace delivery, and automatic updates are outside this MVP. External access is limited to request-scoped Host-managed HTTPS described above. Delayed work is limited to the host-owned, process-local `notifications.schedule()` message and the single-generation window timer described above.

The fixed-output reference packages are:

- `examples/public-plugins/com.uipilot.demo-win`: Windows-only `submit + window` with `ui.window` and a 10-second host-owned delayed message.
- `examples/public-plugins/com.uipilot.demo-return`: `submit + mainResult` with `clipboard.write`.
- `examples/public-plugins/com.uipilot.pomodoro`: Windows-only `submit + window` with the three-permission host timer, pause/resume/reset, message-center completion, and a plugin-supplied finite alarm validated and played by the host.

Each README documents its development-directory installation, focused verification, packaging command, and user-operated acceptance flow.
