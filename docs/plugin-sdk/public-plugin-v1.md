# UiPilot Public Plugin API v1

New to public plugin development? Start with the [third-party plugin developer tutorial](./public-plugin-developer-guide.md), then use this document as the complete contract reference.

Public plugins are static packages loaded by UiPilot. A plugin owns one hidden Runtime and either one legacy slash command (optionally with a singleton content window or launcher panel), or Host 0.5.0+ main-result commands sharing the package. Removing or disabling the package removes all its commands; the host does not provide command-specific fallbacks.

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
- For legacy packages, `command.defaultName` is the initial slash command name. Users may rename it, but reserved and conflicting names fail closed.
- `command.summary` is an optional one-line discovery hint. It is limited to 512 Unicode scalar values; when omitted, the launcher uses `name`.
- `command.inputPlaceholder` is the command-input usage hint. It is distinct from the management-page `description` and discovery `summary`.
- `activationMode` is `live` or `submit`; `window` and `panel` output require `submit`.
- `outputMode` is static. `window` requires a window entry and `ui.window`; `panel` requires a panel entry and `ui.panel`; `mainResult` forbids window/panel entries and both UI permissions. Panel packages require `minimumHostVersion >= 0.3.0`; a non-empty `panel.hostKeys` declaration requires `minimumHostVersion >= 0.3.1`, the extended `Tab`, `Shift+Tab`, or `Enter` declarations require `minimumHostVersion >= 0.3.3`, and an explicit `panel.hostKeyFocus` requires `minimumHostVersion >= 0.3.4`.
- Windows Runtime HTTPS requires `minimumHostVersion >= 0.3.2`, exact permission `network.https`, and a non-null `network` object containing `httpsHosts`. The permission and object must either both be present or both be absent.
- Windows Panel clipboard history requires `minimumHostVersion >= 0.3.3`, permissions `clipboard.history.read` and optionally `clipboard.history.paste` (or `clipboard.history.copy` on Host 0.17.0+), plus `submit + panel + ui.panel`. `clipboard.read` remains reserved and unsupported; it is not an alias for clipboard history.

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

Windows Host `0.4.0+` also accepts optional `network.credentials` for Host-managed bearer consumption (ordinary HTTPS keeps its `0.3.2` minimum):

```json
{
  "minimumHostVersion": "0.4.0",
  "supportedPlatforms": ["windows"],
  "permissions": ["network.https"],
  "network": {
    "httpsHosts": ["api.example.com"],
    "credentials": [
      { "id": "service-token", "settingKey": "api-token", "host": "api.example.com", "type": "bearer" }
    ]
  },
  "settings": [{ "type": "secret", "key": "api-token", "label": "API token" }]
}
```

These are related fields, not a complete Manifest. When present, `credentials` must contain 1–8 entries; `null` and an empty array are invalid. Each entry contains exactly the four required fields shown above. `id` and `settingKey` match `^[a-z][a-z0-9.-]{0,63}$`; IDs are unique within the plugin, case-sensitive, and never trimmed. `settingKey` must reference a declared secret setting, `host` must be an exact member of `httpsHosts`, and `type` is only `bearer`. Different IDs may share a setting or host; binding one secret to two hosts requires two declarations. Unknown fields and duplicate JSON keys fail closed. The metadata type `PluginNetworkCredential` provides no token accessor.

The Host canonicalizes credentials by ID and grants complete `(id, settingKey, host, type)` tuples. Installation displays every binding and warns that the service receives the credential and may echo it. Adding or changing any tuple requires consent to the complete candidate network declaration; reordering does not. Removal narrows the grant; adding it again requires consent. An unchanged upgrade preserves a prior revocation. Cancelling installation/upgrade preserves the installed version. Old host-only grants never imply credential grants. The durable `networkCredentialsGrant` contains metadata only; reload requires exact agreement with current declarations. Invalid grants and backup recovery revoke network before publication; failure to persist revocation keeps network closed.

The installation prepare summary includes sorted `httpsHosts`, `addedHttpsHosts`, full `credentials` (always an array), `addedCredentialIds`, and `requiresNetworkConsent`. Its token binds the displayed candidate and previous configuration; commit accepts only that token and permission grants, not caller-supplied credential tuples. Inventory also exposes full credential metadata. Re-enabling credential network access requires the current displayed `expectedInventoryRevision`; stale/missing revisions fail and require a reload and fresh consent. Revocation is immediate and clears both host and credential grants.

### Multiple Main-Result Commands (Host 0.5.0+)

Exactly one of legacy `command` or new `commands` must be present; explicit null is invalid. Legacy `command` has no `id` field. A `commands` array contains 1–16 entries, including for a single-command array, and requires `minimumHostVersion >= 0.5.0`. Every entry requires a unique, case-sensitive ID matching `^[a-z][a-z0-9-]{0,31}$` and a unique defaultName using the ordinary startup-name rules. IDs are never trimmed or generated from names or positions. `constructor` is a valid ID; use explicit comparisons or a Map rather than inherited object properties.

Each entry otherwise uses the existing command fields, with `live` or `submit` activation and only `mainResult` output. New-shape packages forbid `window`, `panel`, `ui.window`, `ui.panel`, `timer.control`, and `clipboard.history.*`. Invalid entries reject the entire package; the Host never registers a partial array or chooses the first item as a legacy fallback.

Stable identity is `(pluginId, commandId)`. Legacy `command` internally uses `default`; a new package may explicitly retain that ID or omit it entirely. Version, startup name and array order do not define identity. One Runtime, onCommand, settings, storage, permissions, network grants, enable switch and package lifecycle serve the whole array. Each main-result command can be searched, renamed and favorited independently; undeclared result rows retain the existing optional copyText default action; Windows Host 0.10.0+ also supports the explicit resultActions opt-in described below.

Manifest schemaVersion and Runtime apiVersion remain 1. Internal state schema is 2; inventory schema and CLI report schema remain 1. These are independent of the plugin's own version, the Host release version, and the CLI npm package version (currently 0.1.0). Host current and the CLI's production Host target are 0.13.0 and accept valid commands packages, including one-item arrays. Old example minimums and the credential capability's 0.4.0 minimum remain unchanged. The [offline multi demo](../../examples/public-plugins/com.uipilot.demo-multi/README.md) provides its complete installable package under package/. Historically, the 0.5.0 local release passed full automated verification and final independent code review at b84bda3. Its core local installation/update, multi-command behavior, retained command names/favorites/configuration across restart/update, and Notes persistence were user-confirmed; this did not cover the full native/theme/credential-service/installer acceptance matrix. The historical 0.7.0 milestone recorded manual acceptance as pending; current 0.12.0 browser-action native/manual acceptance is pending.

Installation and reload use staging. Static validation and Runtime readiness must succeed before the generation becomes active. Validation/conflict rejection, or a definitely uncommitted persistence failure with verified previous bytes, preserves the previous usable configuration. An uncertain persistence outcome or failed previous-byte verification follows fail-closed recovery; failed updates do not universally guarantee availability or rollback.

## Runtime Contract

The Runtime entry is an ES module exporting `onCommand` with the `PluginHandler` type from [uipilot-plugin-api-v1.d.ts](./uipilot-plugin-api-v1.d.ts). Each invocation and API object is new, deeply frozen, and bound to an immutable plugin ID, generation, and request ID. On Windows, a plugin that declares and receives `notifications.publish` may submit one notification action during that request: immediate `api.notifications.publish({ content })` or delayed `api.notifications.schedule({ content, delayMs })`. A Manifest declaring `network.https` receives the optional frozen `api.network` facade; all authority is still revalidated by the Host for every call.

For a new `commands` package, every invocation includes the exact selected `commandId: string`. A legacy `command` invocation omits this key completely, including when its internal identity is default: it is never sent as null or a synthesized default. The shared SDK declares `commandId?: string` to describe both wire shapes. Dispatch inside one onCommand using this ID, reject missing/unknown IDs for a new package, and never infer identity from startup names, array order or response fields. Responses still return only requestId plus the existing output; they cannot retarget a request by supplying commandId. The Host binds pending input and ID to requestId and discards stale responses when switching sibling commands.

`invocation.input` has command text and boundary whitespace removed while preserving internal whitespace. `context.invokedAt` is RFC 3339 with a local UTC offset. The API permits only plugin-scoped JSON storage, reads of declared non-secret settings, the request-bound Windows message operation when granted, and bounded Host-managed HTTPS when declared and authorized. Runtime code cannot access Tauri, Shell, files, native input, another plugin, direct secret plaintext getters, raw sockets, or browser/WebView networking.

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

`api.network?.request(input)` is Windows-only since Host `0.3.2`; the optional `credential` field requires Host `0.4.0+`. It is available only to a Runtime whose Manifest declares `network.https`; the optional TypeScript property lets plugins detect an older or unavailable Host surface. It accepts:

```ts
api.network.request({
  url: 'https://api.example.com/translate',
  method: 'POST',
  credential: 'service-token', // requires the 0.4.0 Manifest binding above
  body: { type: 'json', value: { text: invocation.input } },
})
```

`method` is exactly `GET` or `POST`. GET rejects a body. POST may omit the body or use `{ type: 'json' | 'text' | 'form', value }`; the Host encodes compact UTF-8 JSON, UTF-8 plain text, or sorted `application/x-www-form-urlencoded` data and owns `Content-Type`. JSON rejects cycles, functions, BigInt, undefined object members, and non-finite numbers. Form values must be strings.

Request headers are an optional string map. Custom provider headers including `authorization`, `accept`, and `accept-language` are allowed. The Host rejects protected or connection-sensitive headers, including `host`, `content-length`, `content-type`, cookies, origin/referrer, user-agent, accept-encoding, hop-by-hop headers, proxy headers, every `sec-*`, `forwarded`, `via`, and every `x-forwarded-*`. Header names are case-insensitive and duplicates after lowercase normalization are invalid.

`credential`, when present, is one non-empty valid credential ID string, not a token, setting key, or object. `null`, extra arguments, accessors, symbols, prototype keys, and unknown fields are rejected. The ID must be in the current exact durable grant, and the normalized request host must equal that binding's host. Combining it with any case-insensitive user `Authorization` header is invalid, even with the same value. Omitting `credential` preserves ordinary requests, including an explicitly supplied Authorization header.

For an admitted reference, the Host reads only that plugin's declared secret and injects `Authorization: Bearer <token>` internally. Tokens must match ASCII `^[A-Za-z0-9._~+/-]+=*$`, contain 1–8192 bytes, and are never trimmed or Base64-converted. Do not include the `Bearer ` prefix, whitespace, CR/LF, or Unicode. Secret storage still accepts up to 64 KiB, so a saved value may be unsuitable for bearer use. The injected header counts toward the existing header budget. Every request reads the actual record; redirects use that admitted token snapshot and still require current authority.

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
| `InvalidNetworkRequestError` | Invalid URL/method/headers/body/credential shape, Authorization conflict, encoding, or request-size limit (including the injected header) |
| `PermissionDeniedError` | Permission absent/revoked/unsupported, unknown or ungranted credential ID, mismatched grants, or authority closed after record validation |
| `NetworkTargetDeniedError` | Scheme, credential-bound host, address, port, DNS answer, or redirect target denied |
| `CredentialUnavailableError` | Declared secret missing, unreadable/undecryptable, or unsuitable for bearer use; internal causes are intentionally indistinguishable |
| `NetworkTimeoutError` | Total ten-second deadline elapsed |
| `NetworkFailureError` | DNS, TLS, connect, write, or read failure without a narrower policy error |
| `NetworkResponseTooLargeError` | Response header or body limit exceeded |
| `NetworkResponseInvalidError` | Invalid framing, content encoding, header value, or UTF-8 body |
| `NetworkLimitExceededError` | Per-context call/concurrency or Host-wide concurrency limit exhausted |
| `ExpiredRequestError` | Request replaced/completed/cancelled or plugin disabled/upgraded/uninstalled/torn down |

Errors are deliberately redacted: they expose no URL, query, request/response headers, body, resolved address, certificate, provider payload, or native-library message. Disabling, uninstalling, upgrading, revoking permission, replacing/expiring the command, Runtime teardown, and Host shutdown cancel matching in-flight requests and discard stale responses. Calls do not survive the command context and cannot be used as background work.

Secret replacement or deletion cancels the plugin's outstanding network contexts, including ordinary requests, and discards stale responses. Old request IDs never revive; only a later command can use a newly verified value. The running handler keeps its scheduler slot until completion or the existing deadline, so a new command waits instead of overlapping it. Preparation failures leave prior requests unchanged; once cancellation has occurred, a save failure does not undo cancellation.

Settings saves are not atomic across keys or ordinary/secret files. Failure does not promise rollback: disk may contain the old or new value, and earlier keys may already be committed. A failed secret commit keeps network closed for that call. This is not a persistent failure latch: later startup, enable, explicit network grant, or Runtime recovery/replacement can reopen only after checking actual durable grants and records. A valid old/new value or missing record can permit ordinary network access; an existing invalid record keeps it closed. A missing record is a valid unconfigured state, so an otherwise authorized credential request returns `CredentialUnavailableError`. A closed authority returns `PermissionDeniedError` before secret consumption. Successful reconfiguration never revives an expired command. To revoke reliably, obtain a successful clear or network-disable result; a failed save is not confirmation of revocation.

Cancellation cannot recall bytes already sent or delivered results, and offers no remote transaction rollback. The destination service receives the token and can echo it, including encoded forms, in normal response headers/body; those responses retain the ordinary API contract and are not silently rewritten. Never display/log provider responses if your application cannot trust that boundary. Host errors, Debug output, and logs do not include secret values; the Host does not automatically copy request headers into responses. Sensitive values exist transiently during HTTPS processing; complete zeroization of every library copy is not guaranteed.

Runtime and content WebViews have no plaintext getter. Runtime can ask only `settings.isSecretConfigured()` (presence, not validity) or submit a declared credential ID to the Host; content cannot use the Runtime network API. Bundled tokens remain inspectable ordinary code, so do not ship production credentials. OAuth, refresh, signing, arbitrary credential-header templates, and raw secret access are unsupported. See the offline [credential reference demo](../../examples/public-plugins/com.uipilot.demo-credentials/README.md).

## Responses

`mainResult` returns zero to twenty plain-text items. IDs are non-empty and unique. Titles are at most 256 Unicode scalar values, subtitles at most 512, and details at most 16 KiB UTF-8. Without `runtime.resultActions`, an item may have one `copyText` default action when `clipboard.write` is declared and granted. Windows Host 0.10.0+ explicitly opted-in packages support the bounded named copy/Runtime action form below; Host 0.12.0+ adds explicit HTTP(S) browser actions. Inline links, arbitrary commands, HTML and Markdown remain unsupported.

`window` returns `{ requestId, data }`. The host creates or reuses one window for that plugin and sends a `PluginWindowUpdate` through `window.uipilotPluginWindow.onUpdate`. Content receives input, platform, theme, invocation time, singleton instance `1`, and plugin data. It cannot invoke commands or own pin, close, drag, focus, theme, or position behavior.

`panel` returns `{ requestId, data }`. The host mounts one launcher panel session for that plugin and sends a `PluginPanelUpdate` through `window.uipilotPluginPanel.onUpdate`. Panel content receives input, platform, theme, invocation time, session epoch, and plugin data. Its bridge exposes private storage plus `onHostKey(handler)`, `focusHostInput()`, `requestHide()`, and, when the manifest declares clipboard history permissions, `clipboardHistory`; it cannot own timers or publish notifications. The base panel contract requires host `0.3.0+`; the three `0.3.1` methods require `minimumHostVersion >= 0.3.1` when used.

`window.uipilotPluginPanel.focusHostInput()` moves keyboard focus to the live panel session's tagged launcher argument input. It preserves the panel, command tag, argument text, selection, and submission state. Repeated calls while that input is already focused succeed. Missing, replaced, torn-down, or stale sessions resolve without side effects; a current native-focus, event, acknowledgement, or timeout failure rejects with `windowFailed`. The method does not stream edits to panel content: argument changes reach `onUpdate.input` only after the user presses Enter.

### Panel Host Keys And Return

`panel.hostKeys` is optional, contains at most eight unique declarations, and accepts only `ArrowDown`, `ArrowUp`, `Primary+N`, `Tab`, `Shift+Tab`, and `Enter`. Validators reject unknown values, duplicates, non-arrays, wrong element types, and extra panel properties. Canonical order is `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter < Delete`. `Tab`, `Shift+Tab`, and `Enter` require `minimumHostVersion >= 0.3.3`.

`panel.hostKeyFocus` is optional and accepts only `content` or `host`. Omitted is equivalent to `content`, preserving the original behavior: before each Host Key delivery, the Host gives native focus to the Panel content WebView. `host` keeps native focus on the tagged launcher input while delivering the same frozen event and is intended for panels that use Host Keys only as list/category commands. An explicit value requires `minimumHostVersion >= 0.3.4`. Wrong types, `null`, unknown values, and extra properties fail closed. The setting does not change queue ordering, acknowledgement, timeout, teardown, or routed Enter paste authority.

| Declaration | Launcher input match | Delivered `event.key` |
| --- | --- | --- |
| `ArrowDown` | no modifiers | `ArrowDown` |
| `ArrowUp` | no modifiers | `ArrowUp` |
| `Primary+N` | Windows Ctrl-only+N; macOS Meta-only+N; no Alt/Shift | `n` |
| `Tab` | no modifiers | `Tab` with `shiftKey: false` |
| `Shift+Tab` | Shift-only | `Tab` with `shiftKey: true` |
| `Enter` | no modifiers and not IME composing | `Enter` |

IME composition, undeclared keys, ordinary characters, and extended chords are never routed. The launcher consumes a matching physical key before enqueue. Host delivery is strictly serial with a queue depth of eight and a two-second acknowledgement timeout. Queue-full presses remain consumed but are not delivered. A handler throw/rejection is acknowledged without retry; a hung handler, unsubscribe, sequence violation, or counter exhaustion ends the exact panel session instead of overlapping handlers.

When `hostKeys` is non-empty, content must register exactly one `onHostKey(handler)` before ready. A second registration throws `TypeError`. Calling `onHostKey` with omitted/empty `hostKeys` throws `TypeError` and permanently fails ready for that document even if plugin code catches it. The handler receives a deeply frozen event containing real modifier bits plus canonical-decimal `sessionEpoch` and `routeSequence`. Unsubscribe ends and hides the session.

`requestHide(): Promise<void>` takes no arguments. A current session resolves after hide admission and before teardown; the document may be destroyed on the next macrotask, so the resolving continuation must not start later DOM work. Missing, stale, replaced, or in-pattern unauthorized sessions resolve as no-ops. Admission failure rejects with `windowFailed`. If the renderer hangs or crashes before observing admission, the Promise may never settle; Host reclaims an unobserved admission after 30 seconds. Once observed, a 500 ms fallback protects the normal next-macrotask commit.

Escape in panel content uses capture-phase arbitration. A synchronous `preventDefault()`, active `dialog[open]`, or composition suppresses hide; `preventDefault()` after an `await` is too late. Explicit-return hides best-effort restore the external HWND+PID captured when UiPilot was shown. Blur and launch-handoff hides never restore, and foreground restore failure does not turn a successful hide into an error.

### Panel status bar (Host 0.16.0+)

Panel mode retains the Host footer. `window.uipilotPluginPanel.setStatus({ message, tone? })` returns a Promise and sends plain-text feedback to that footer. `tone` is `info` (default), `success`, or `error`; `message` is at most 512 Unicode characters, may not contain NUL, and an empty string clears it. It needs no permission beyond the active `ui.panel` session. No HTML, notification, focus change or persistent message is created. The method is bound to the session when obtained; old sessions and out-of-order messages cannot replace the current panel's feedback. Capture the method before asynchronous work when retaining callbacks across updates. Panel replacement/closure clears the status. Plugins own success-clear timing and must avoid a stale timer clearing newer feedback.

### Panel Clipboard History

`window.uipilotPluginPanel.clipboardHistory` is Windows-only and available only to `submit + panel + ui.panel` plugins declaring `clipboard.history.read`; paste also requires `clipboard.history.paste`, and copy requires `clipboard.history.copy` (Host 0.17.0+). The Host records text, image, and file-list clipboard changes while UiPilot is running, the plugin is enabled, and permission is granted. Panel code receives only summaries through `list()` and `onChanged(handler)`: text previews, image thumbnails, file names/counts, availability, `id`, `capturedAt`, and `revision`. Image summaries use displayable `data:image/png;base64,...` thumbnail URLs; the public-plugin CSP still rejects network, file, and `blob:` images. It never receives raw text, original PNG bytes, complete file paths, HWND, PID, arbitrary key names, or paste counts.

`clipboardHistory.paste({ id, routeSequence })` is accepted only from the current Panel session after a routed Enter event. It returns `{ outcome: 'admitted' }` before teardown, then the Host writes the selected record to the system clipboard, hides UiPilot, restores the previously captured external window when safe, and sends one platform paste chord. Failures expose stable `Error.name` values: `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, `PasteTargetUnavailable`, and `ClipboardWriteFailed`. Error messages are redacted.

Every plugin content window also sees the frozen `window.uipilotPluginWindow.timer` facade. Calls require the Windows-only `timer.control` permission together with `ui.window` and `notifications.publish`; unpermitted callers receive `PermissionDenied`. The host owns one process-local timer per active plugin generation, continues it while the window is hidden, and discards it on process exit.

Every valid plugin content window also receives the frozen `window.uipilotPluginWindow.storage` facade without an additional permission. It shares the Runtime `api.storage` namespace for that plugin. `get` is available while the window session is prepared or active; `set` and `remove` are active-only. A hidden, replaced, disabled, upgraded, or uninstalled session returns `ExpiredWindowSessionError`. Keys must match `^[a-z][a-z0-9.-]{0,63}$` and cannot be `__proto__`, `prototype`, or `constructor`; values are finite JSON and count toward the same 5 MiB plugin quota.

A `timer.control` package supplies its own fixed `assets/sounds/timer-alarm.wav`, while the host exclusively validates, freezes, and plays it. The WAV must be little-endian PCM with one or two channels, a 44.1 or 48 kHz sample rate, and 16- or 24-bit samples; it must not exceed 2 MiB or 15 seconds. Unknown or duplicate chunks, trailing bytes, additional WAV files, and Runtime/content access to the alarm are rejected. Timer completion loops the alarm frozen at round start until the main window opens. Ordinary plugin messages use the host's shared one-shot notification sound instead. Full session, revision, pause/reset, completion-message, and alarm behavior is documented in the developer guide.

The complete serialized response budget is 64 KiB. Unknown fields, invalid actions and over-budget responses reject the entire response. The opted-in bounded JSON contract below also rejects non-finite values and dangerous record keys at JS snapshot and Host validation. After framework conversion to a JSON Value, duplicate raw JSON keys cannot be recovered.

## State, Timing, And Faults

- Private JSON storage is shared by all commands in a package and by Runtime and content-window facades, limited to 5 MiB per plugin, and fails atomically at the limit.
- `live` dispatches use a 150 ms frontend debounce and a 5 second post-dispatch timeout.
- `submit` dispatches only after Enter and use a 30 second post-dispatch timeout.
- Each plugin generation has at most one running request and one latest waiting request.
- Content ready and update acknowledgement each have a 5 second timeout.
- Three consecutive runtime faults within five minutes persistently disable the plugin; a successful request or manual enable resets the relevant fault state.

Settings support `text`, `secret`, `number`, `boolean`, and `select`. Keys match `^[a-z][a-z0-9.-]{0,63}$` and remain stable across upgrades. Secrets have no default. Runtime can ask only whether one is configured; Windows Host `0.4.0+` may consume an explicitly declared, authorized bearer reference without exposing a plaintext getter. Host settings provide per-key password save and explicit clear actions; blank unedited inputs do not delete secrets. Saving/clearing can cancel current plugin requests; a failure requires refreshed status and carries the partial-commit/recovery rules above.

### Names, Upgrades And Recovery

Each command has at most one nameOverride. It replaces effectiveName and releases the previous route; it does not create an additional alias. Renaming one command leaves sibling names unchanged and retains its ID/favorite. An actual nameOverride state change invalidates package requests and result handles, even if effectiveName stays the same; setting exactly the already-saved override (distinguishing null from a string) is a no-op. A favorite change does not restart Runtime or cancel valid work.

A successful upgrade preserves overrides/favorites for retained IDs, adopts new manifest defaults, creates fresh state for added IDs, and removes deleted IDs atomically. Deleting an ID, restarting, and re-adding it creates fresh state. Changing an ID is deletion plus addition even when its name is unchanged. Legacy state is inherited only through an explicit default ID; no name or first-item migration occurs. A package rollback on the new Host follows normal validation, conflict, authorization and lifecycle checks; it preserves only IDs still present, so a previously deleted default is fresh on return to a legacy package.

Host binary downgrade is separate and unsupported once a normal transaction or required recovery has written state schema 2: an older binary that only understands schema 1 is not guaranteed to read that data directory. There is no automatic schema-1 writeback or backup-based downgrade feature. New Hosts accept valid old packages; old Hosts do not accept commands packages.

Only the old set_plugin_favorite / set_plugin_effective_name IPC callers may omit commandId to target an existing default. Explicit null, empty, invalid or unknown IDs fail, and omission fails if default is absent. This compatibility does not apply to new activation payloads or new-package Runtime invocations.

## Permissions

API v1 implements only:

- `ui.window`: create the host-owned singleton window.
- `ui.panel`: mount the host-owned launcher panel surface.
- `clipboard.write`: expose host-owned `copyText` actions.
- `url.open`: Windows Host `0.12.0+` explicit HTTP(S) browser result actions, with declared `runtime.resultActions` and current consent.
- `clipboard.history.read`: on Windows Host `0.3.3+`, allow the Host to collect this plugin's local clipboard history and expose redacted summaries to its Panel.
- `clipboard.history.copy`: on Windows Host `0.17.0+`, restore a full record to the clipboard while preserving the current Panel and focus; requires `clipboard.history.read`, explicit grant, and `submit + panel + ui.panel`.
- `clipboard.history.paste`: on Windows Host `0.3.3+`, allow a current Panel session to restore one selected clipboard history record and paste it once after a routed Enter event; requires `clipboard.history.read`.
- `network.https`: on Windows Host `0.3.2+`, expose bounded request-scoped Host-managed HTTPS to the exact authorized `network.httpsHosts` set.
- `notifications.publish`: on Windows only, submit one immediate or host-owned delayed plain-text message and ask the host to show its own notification and tray reminder.
- `timer.control`: on Windows only, control one host-owned plugin-window timer that can continue after the window hides; requires `ui.window`, `notifications.publish`, and `submit + window`.

Other parsed permission names are reserved and installation fails until the host implements them. Permission changes during reload require normal confirmation; no development-package bypass exists.

The host's **Settings → Permissions** page groups installed public plugins by capability and lets the user change each declared, supported grant. Grants are independent of the plugin's enable switch; granting one capability does not grant its dependencies. Plugins must handle permission denial after installation. Network regrant still requires confirmation of the complete current host and credential scope.

Revocation retires stale command work and denies subsequent capability admission. UI revocation retires and closes the affected surface; notification/timer revocation cancels the associated delayed work or timer. Regrant does not revive canceled work. Revoking clipboard-history read stops that plugin's recording while retaining its prior history. Permission changes do not erase plugin data. Restart, development refresh and replacement preserve revoked permissions that remain declared; newly declared permissions still require the normal confirmation. Recovery from a backup cannot restore stale capability grants. Already completed system effects cannot be undone.

## Unsupported In v1

Arbitrary background execution, plugin-owned timers, repeating or persistent scheduling, multi-command panel/window output, multiple windows, per-command hotkeys, independent alias collections, streaming, pagination, large responses, browser/WebView networking, raw sockets, arbitrary files, raw clipboard read, native binaries, Shell, input synthesis, plugin-to-plugin communication, remote media, dependencies, signing, marketplace delivery, and automatic updates are outside this MVP. External access is limited to request-scoped Host-managed HTTPS and the Host-managed clipboard history summary bridge described above. Delayed work is limited to the host-owned, process-local `notifications.schedule()` message and the single-generation window timer described above.

The fixed-output reference packages are:

- `examples/public-plugins/com.uipilot.demo-win`: Windows-only `submit + window` with `ui.window` and a 10-second host-owned delayed message.
- `examples/public-plugins/com.uipilot.demo-return`: legacy `submit + mainResult` with `clipboard.write`.
- `examples/public-plugins/com.uipilot.demo-multi`: Host `0.5.0+` live uppercase and submit lowercase commands, one offline Runtime and shared data; complete installable package/.
- `examples/public-plugins/com.uipilot.demo-panel`: Windows-only `submit + panel` with `ui.panel`, Host key routing, `focusHostInput()`, and `requestHide()`.
- `examples/public-plugins/com.uipilot.demo-credentials`: Windows-only Host `0.4.0+` bearer reference with synthetic offline provider tests, presence-only status, and manual Host acceptance steps.
- `examples/public-plugins/com.uipilot.demo-http`: Windows-only `submit + mainResult` with Host-managed `network.https` requests to `httpbin.org`.
- `examples/public-plugins/com.uipilot.pomodoro`: Windows-only `submit + window` with the three-permission host timer, pause/resume/reset, message-center completion, and a plugin-supplied finite alarm validated and played by the host.

Each README documents its development-directory installation, focused verification, packaging command, and user-operated acceptance flow.


## Panel Runtime actions (Windows Host 0.6.0+)

Legacy `command` packages with `activationMode: "submit"`, `outputMode: "panel"`, granted `ui.panel`, Windows-only `supportedPlatforms`, and `minimumHostVersion: "0.6.0"` or later may declare `panel.runtimeActions: true`. Absent or false preserves the existing API; null is invalid. Opt-in requires an exported `onPanelAction` function during Runtime readiness. CLI validates the declaration, not JavaScript exports. API v1 and historical version minima are unchanged.

Use capability detection: `if (window.uipilotPluginPanel.request) { ... }`. `await panel.request({action: "preview", payload: {...}})` invokes this panel's existing Runtime, whose `onPanelAction(invocation, api)` returns `{requestId: invocation.requestId, data: ...}`. The Promise resolves with frozen `data`; action replies do not call `onUpdate` or publish launcher results. Each invocation and its API are new and deeply frozen. Existing storage/settings/notification/optional HTTPS permissions and expiry checks apply. Direct content networking remains forbidden.

Action names match `^[a-z][a-z0-9.-]{0,63}$`. Payload is required (explicit null is allowed); extra envelope fields reject. Both directions accept finite JSON primitives, dense arrays and plain data records only. Cycles, accessors, symbols, undefined, functions, BigInt, nonfinite numbers and record keys `__proto__`, `prototype`, `constructor` reject without executing getters/toJSON. Each compact UTF-8 envelope is at most 64 KiB, nesting at most 32, and at most 4096 values. Reply ID must match exactly; no truncation.

One unresolved call and one physical Runtime slot per plugin are enforced by Host. Busy rejects rather than queueing/retrying. Do not await requests inside `onUpdate` or `onHostKey`; wait until Ready. Host command submission cancels action ownership but waits for the running handler to finish or its original 30-second deadline before dispatching another handler. Close, reload, replacement, disable, permission/credential changes and faults revoke the exact old API context. Already-admitted effects may finish: cancellation after save does not mean “not saved”; reload before deliberate retry.

Fixed Error.name values: `InvalidPanelRequestError`, `PanelRequestBusyError`, `ExpiredPanelSession`, `PanelRequestCancelledError`, `PanelRequestTimeoutError`, `InvalidPanelResponseError`, `PanelRequestFailedError`, `PanelRequestUnavailableError`. Business validation belongs in ordinary data (for example `{ok:false, code:"missing-variable"}`); unexpected handler errors expose no plugin stack or arbitrary native text. There is no multi-command panel, streaming, AbortSignal or new clipboard API. Window requests require the separate Host 0.9.0 opt-in below.


## Host 0.7.0：本地开发刷新、恢复和诊断

“刷新列表”只重新读取已安装清单。“重新载入开发目录”位于插件详情，需要重新选择 package 目录，将其复制、验证为不可变快照；它只接受当前插件 ID 和完全相同的规范版本。宿主不监听目录，也不记住源路径。准备后仍需确认完整权限、HTTPS 主机及凭据绑定。此前关闭且声明未变的网络访问不会自动开启。改版本（包括降级）请在列表使用“选择开发目录”或“选择插件包”。确认使用一次性令牌；尝试提交后需重新选择，不能重复提交旧确认。

刷新成功会关闭旧面板/窗口，请重新运行命令查看新代码。刷新保持停用/故障状态；修复代码后需显式“重新启用”。已经启用的插件再次启用不会重启健康 Runtime。静态验证或就绪失败保留旧包；只有验证了旧落盘字节的确定未提交情况才可继续使用旧配置。状态未确认时应重启宿主后核对；持久化后发布失败并不等于回滚。

插件包 semver、Host 版本、SDK API v1、插件私有数据 schemaVersion 相互独立。刷新不会主动清除插件数据；包替换/降级也不会转换或恢复私有 JSON。插件作者应验证完整数据文档，遇到未知/较新数据版本时阻止写入，不能静默重置。宿主没有自动数据迁移、包历史回滚或备份恢复功能。已获准的旧请求效果仍可能完成；取消或保存失败不能证明数据/远端效果未发生，重试前先重新读取。

详情中的“查看诊断”提供显式允许字段构造的 JSON 预览；“复制此诊断”仅复制当前预览快照，不自动更新、上传或读取剪贴板。报告含 Host/平台、插件 ID/版本、已确认配置状态及最新管理操作阶段/固定错误码，不含内容、路径、设置、私有数据、凭据、域名、请求 ID 或原始异常。状态 null 表示未知；enabled 不是 Runtime 健康证明。最新操作仅驻留内存，最多 64 个插件槽，重启清空。stateCommitOutcome 只描述 Host 配置持久化，不描述插件业务数据或远端效果。


## Window Runtime actions (Windows Host 0.9.0+)

Legacy `command` packages may declare `window.runtimeActions: true` with Windows-only `supportedPlatforms`, `activationMode: "submit"`, `outputMode: "window"`, a window entry, granted `ui.window`, and `minimumHostVersion: "0.9.0"` or later. Absent/false keeps the historical Window API with no `request` property; null and other values reject. Opt-in readiness requires an exported `onWindowAction` function; CLI performs static Manifest validation without executing Runtime. API v1, state schema and historical feature minima are unchanged.

```js
// Runtime: all business logic uses the existing request-bound API.
export async function onWindowAction(invocation, api) {
  const value = await api.storage.get('draft')
  return { requestId: invocation.requestId, data: value }
}
// Content: obtain the current capability when the user performs an action.
const request = window.uipilotPluginWindow.request
if (request) {
  const data = await request({ action: 'load', payload: null })
}
```

The action invocation contains exactly `apiVersion: 1`, `requestId`, `action`, `payload` and `context` (`platform: "windows"`, `theme`, RFC 3339 `invokedAt`). The handler returns exactly `{requestId, data}` with the same ID. `request` resolves to deeply frozen JSON data; replies do not trigger `onUpdate` or launcher results. Runtime invocation/API objects are fresh and frozen, with the existing storage/settings/notification/optional HTTPS permissions and expiry rules.

Window requests share Panel's bounded JSON contract: action names match `^[a-z][a-z0-9.-]{0,63}$` and reject `__proto__`, `prototype`, `constructor`; `payload` is required and may be null. Request/reply envelopes have exact fields, at most 64 KiB compact UTF-8, depth 32 and 4096 values. Only finite JSON primitives, dense arrays and plain data records are accepted. Accessors, toJSON execution, cycles, sparse arrays, symbols, functions, undefined, BigInt, nonfinite numbers and dangerous record keys reject. Host independently revalidates both directions.

The optional readonly `request` getter supplies a new function before each session's first `onUpdate`. Capture it in `onUpdate` if needed, but call only after that callback has returned and Host has activated the window. Calling during update/ACK returns Busy immediately; never await a request as part of `onUpdate`. A retained old function remains bound to its old full window owner, session and document, and expires after hide, submit replacement, navigation/reload, destruction or lifecycle revocation. Read the getter again for the new session. Internal bootstrap arguments cannot grant authority.

Each window permits one unresolved call and each plugin one physical Runtime handler. Busy rejects without queueing or retry. A new Host command cancels logical Window request ownership, then waits for the running physical handler to finish or its original 30-second deadline. Hide starts revocation immediately without waiting on the handler; hide failure does not revive ownership. Existing timer/storage leases and background timer completion keep their previous behavior. Disable, update, development reload, permission/settings/credential changes, faults and uninstall also revoke old contexts. Completion rechecks exact window identity and a single completion ticket before success. Already-admitted writes may finish; cancellation or failure is not proof that a save did not happen. Read current state before a deliberate retry.

Fixed `Error.name` values are `InvalidWindowRequestError`, `WindowRequestBusyError`, `ExpiredWindowSessionError`, `WindowRequestCancelledError`, `WindowRequestTimeoutError`, `InvalidWindowResponseError`, `WindowRequestFailedError`, `WindowRequestUnavailableError`. Business validation belongs in normal data such as `{ok:false, code:"revision-conflict"}`. Handler/native exception text and plugin stacks are hidden. There is no streaming, AbortSignal, automatic retry, arbitrary invoke or content networking capability. The private bridge is restricted to the actual plugin-window content WebView; main, shell, panel, runtime and find surfaces have no grant.


## Main-result actions (Windows Host 0.10.0+)

Declare `runtime.resultActions: true`, `minimumHostVersion: "0.10.0"` or later, exactly `["windows"]`, and only `mainResult` commands. Both `command` and `commands` support this opt-in. Absent/false preserves historical copy/hide behavior; null, unknown Runtime fields and incompatible declarations reject. Export callable `onCommand` and `onAction` even for a copy-only package. CLI validates statically and never evaluates JavaScript. API/schema v1, state schema 2 and historical feature minima remain unchanged.

Return `{requestId, results}`. An opted-in result is `{id, title, subtitle?, detail?, actions, defaultActionId?}`. `actions` has 0–8 entries: `{id,title,type:"copyText",text}`, `{id,title,type:"runtime",payload}`, or Host 0.12.0+ `{id,title,type:"openUrl",url}`. Empty actions means informational. Omit `defaultActionId` to require menu selection; present values must identify one action. Null is invalid. `actions` cannot coexist with `defaultAction`, including null. Legacy rows may be mixed in an opted-in response; their copy action follows the same snapshot consumption policy. Undeclared packages keep the legacy parser.

Action IDs match `^[a-z][a-z0-9-]{0,31}$` and are unique per row. Business IDs such as `constructor` are valid string values. Action titles contain 1–80 Unicode scalars, at least one non-whitespace scalar, and no C0/C1 controls. Result IDs are unique across the complete response, nonempty, at most 128 UTF-8 bytes, and free of C0/C1 controls. Row title/subtitle/detail retain their current limits (256 scalars/512 scalars/16 KiB). Copy text allows empty strings, at most 16 KiB UTF-8, no NUL; publication and copying both require current declared/granted `clipboard.write`. Runtime payload is required and may be null. Storage adds no permission.

The exact-key response has at most 20 rows, 64 KiB compact UTF-8, depth 32 and 4096 total values (root depth zero and root counted). Only finite JSON primitives, dense arrays and plain data records are supported. Cycles, accessors, symbols, undefined/functions/BigInt, nonplain objects and record keys `__proto__`, `prototype`, `constructor` reject without invoking getters or toJSON. Unknown fields, malformed variants/defaults, duplicates or overflow reject the whole response; no truncation or stringification fallback. Host independently validates after IPC conversion; duplicate raw JSON keys cannot be recovered at that stage.

`onAction(invocation, api)` receives exactly `apiVersion:1`, a fresh scheduler `requestId`, saved normalized `commandId` (including `default` for legacy command), business `resultId`/`actionId`, immutable saved `payload` and original `input`, and `context:{platform:"windows",theme,invokedAt}` sampled at dispatch. Invocation and API are fresh and deeply frozen. Retained APIs from completed commands are expired. Return only a bounded `{requestId,results}` matching the new ID. Success replaces the complete source snapshot, including an empty result list, with fresh Host handles; there is no patch, retained-old mode, command retargeting, hide/open request or content `data` reply.

One source snapshot permits one admitted copy, URL or Runtime action across all rows. Busy/pre-admission denial leaves it available; accepted success, failure, cancellation and uncertain transport consume it. Duplicate pending execution reports in-progress; replay reports already-executed. This is not durable exactly-once delivery. A fresh ordinary command or Host restart is new intent, so plugins own domain idempotency. Never automatically retry mutations. After an uncertain save, read current data before deliberate retry.

Runtime actions share the plugin's existing physical handler slot and 30-second deadline. They do not queue or interrupt JavaScript. A newer command retires old API authority and waits for the old handler's physical completion or deadline. Input edits, query/view replacement, hide/show and lifecycle/settings/permission changes revoke the source lease and block late publication and new API effects. Already-admitted effects may finish. Handler completion expires its API before parsing and uses one terminal ticket; malformed replies and handler throws have distinct fixed failures. Transport loss is settled once and the original autonomous deadline remains armed.

The main launcher exposes footer **动作 Ctrl+K**, right-click and a keyboard menu. Enter runs the explicit default, or opens the menu when there is no default. New copy actions keep the launcher open, show **已复制**, and disable the consumed source's actions. Admitted failure shows **操作未完成，请重新读取核对后再运行**. Stale source shows **结果已过期，请重新运行**. **重新运行** explicitly submits the ordinary command; it is never an automatic action retry. Selection, scrolling and theme changes alone do not cancel a source. Public result rows remain non-favoritable.

## Browser URL result actions (Windows Host 0.12.0+)

Declare `url.open`, `minimumHostVersion >= 0.12.0`, exactly `["windows"]`, `runtime.resultActions: true` and only `mainResult` commands. Installation/update requires the user's explicit grant; rejecting added permission preserves the old installation and data. Export `onAction` even for static-only actions. API/schema v1 and persisted state format stay unchanged. `network.https` does not authorize browser opening.

An action has exactly `{id,title,type:"openUrl",url:string}`. The existing action/response bounds, ID/title rules, explicit default and extra-field rejection apply. Legacy `defaultAction` remains copy-only. An opted-in action may be an explicit default; it executes only when the user selects it, never during query or publication. There is no direct Runtime, Panel or Window browser API.

The Host validates both the supplied text and its canonical URL at publication and immediately before opening. Each is at most 4096 UTF-8 bytes. Require an explicit case-insensitive `http://` or `https://` prefix, a nonempty raw authority immediately after it with no `@`, a parsed nonempty host, and no username/password. Literal whitespace, C0/C1 controls, DEL and backslash reject; no trimming or repair is performed. Relative/opaque URLs, other schemes and malformed hosts/ports reject the entire response. Unicode domains, ordinary escapes, queries and fragments are supported; percent-escaped whitespace/control bytes remain URL data. The canonical URL object goes to the existing Windows browser adapter, never a shell command string. Destination authority remains private; the renderer sees opaque action handles and titles only. Errors/logs do not include raw URLs.

Every capability used anywhere in the source snapshot requires current declaration and grant: mixed copy/URL/Runtime snapshots require both `clipboard.write` and `url.open`, including publication, action admission and Runtime replacement. The existing generation, activation, command, publication ticket and source lease checks apply. A single admitted copy, URL or Runtime action consumes all siblings across all rows. Pre-admission denial leaves the source available; native success and failure both consume it. Retired sources cannot launch; an already-admitted OS request may finish, while its stale completion cannot update a newer UI owner. Never retry automatically; explicit **重新运行** creates a fresh source.

Owned success is `{kind:"urlOpened"}` and displays **已请求打开链接**, closes the action menu and disables the source without changing launcher lifetime. This means Windows accepted the request, not that a page loaded. Native failure uses fixed `urlOpenFailed` with `sourceConsumed:true` and the existing safe failure/rerun feedback. The default browser owns redirects, authentication and network policy; this permission covers only the initial external URL and does not impose Host HTTPS redirect rules.

The [text-snippet 1.1 example](../../examples/public-plugins/com.uipilot.text-snippet/README.md) adds a secondary open action only for eligible original/latest text and keeps copy as default. Its uppercase row never opens a URL because path case can change the destination.


## User-selected text import (Windows Host 0.13.0+)

Declare `files.openText`, `ui.window`, exactly `["windows"]`, `minimumHostVersion >= 0.13.0`, and legacy `command` with `submit + window`. This permission does not require `window.runtimeActions`. Installation/update requires explicit permission consent; cancelling preserves the prior installation, configuration and data. API/Manifest v1 and persisted schemas stay unchanged.

Only opted-in Window content exposes optional readonly `window.uipilotPluginWindow.files`. Capture it in `onUpdate`, then call `files.openText()` with no arguments from an explicit user action after update/ACK. The frozen result is `{name, text, byteLength}` or `null` for cancellation. `name` is a basename, never a path. Maximum original file size is 8192 bytes including an optional leading UTF-8 BOM; the Host strips that BOM once, decodes strict UTF-8, rejects NUL and preserves other whitespace. Empty text and invalid JSON are allowed. The chooser accepts one ordinary local Windows file, with fixed text/all-file filters. No arbitrary path input, file writes, Runtime slot, persistent grant or handle is exposed.

One physical picker is allowed globally. Each facade captures its exact session/document; retained old functions and late success/cancel/error cannot borrow replacement authority. Explicit close, revocation and replacement remain nonblocking. Automatic blur hiding is suppressed only for the physical picker's document until completion; focus is restored only to its still-current authorized original shell.

Errors have sanitized names/messages: `FileOpenBusyError`, `ExpiredWindowSessionError`, `FileOpenPermissionError`, `FileOpenUnsupportedError`, `FileOpenTooLargeError`, `FileOpenEncodingError`, `FileOpenFailedError`. Passing any argument rejects `TypeError` before IPC. Host never logs the selected path, basename, text or native errors. Files outside ordinary local drives, reparse/device/directory/ADS targets and other platforms fail closed.

See [JSON workbench](../../examples/public-plugins/com.uipilot.json-workbench/README.md) for an explicit import/format/save workflow with edit/session ownership and inline dirty-draft confirmation. Native Windows behavior remains user-operated acceptance.

## Text paste result actions (Windows Host 0.14.0+)

Declare `clipboard.paste` **and** `clipboard.write`, exactly `supportedPlatforms: ["windows"]`, `minimumHostVersion >= 0.14.0`, and `runtime.resultActions: true`. Commands must use `mainResult`. API/Manifest and persisted schemas remain v1. Installation or an update adding this permission requires the full permission consent; cancellation retains the existing version, settings, command bindings and data. `clipboard.history.paste` does not authorize this capability.

The action has exactly `{id,title,type:"pasteText",text:string}`. Text must contain 1..8192 UTF-8 bytes and no NUL; Unicode, whitespace and newlines are preserved without trimming. Existing ID/title/action-count and whole-response 64 KiB limits apply; reject the entire invalid response. A plugin may explicitly set its action as default, but legacy `defaultAction` remains copy-only. The reference snippet keeps copy as default and offers paste only in the action menu. Query, publication and Runtime execution never send input on their own. There is no direct Runtime input API, arbitrary key sequence, target handle, Panel or Window paste bridge.

All capabilities anywhere in the immutable source snapshot require current declaration and grant during initial publication, every sibling action admission and Runtime replacement. A paste snapshot therefore requires both permissions even when selecting copy, URL or Runtime. Only opaque result/action handles and titles reach the renderer; text and target identity remain Host-private. `execute_result_action` accepts only the existing opaque request/result/action IDs and only from main.

A paste binds the external window captured before the current launcher show, including HWND/PID and an independent current show lease. Repeated visible shows invalidate older leases while preserving the original capture where applicable. The Host waits up to 500 ms off the UI thread for Ctrl/Shift/Alt/Win/Enter/Space release, then performs one main-thread transaction: revalidate visible main foreground, exact target/lease/owner/grants/publication; consume the source; write text; release all admission and manager locks; clear/hide with ExplicitReturn to restore the target; verify current show, exact external foreground/PID, released keys and clipboard sequence; submit one Ctrl+V input batch. No effect retries, forced key release, privilege elevation or fallback target are allowed.

Clipboard writes use the main HWND with RAII allocation ownership. After successfully closing the write guard, the Host reopens once to verify the owner and exact just-written UTF-16 text including terminator, captures a nonzero settled sequence, and successfully closes that guard before hiding. This never reads or backs up preexisting clipboard data. Any close/open/ownership/content/sequence failure stops the transaction. Final SendInput checks must still match the recorded sequence; an intervening copy or focus/key change aborts input. Successful writes remain on the clipboard even if hiding, restore or input fails; later user copies are never overwritten to restore data.

Success is `{kind:"pasteSent"}` and displays **已请求粘贴**: Windows accepted one input batch, without proving that an application inserted text or saved a document. Stable errors are `pasteTargetUnavailable` (no current original target), `pasteInputBusy` (keys not released), `pasteFailed` (hide/restore/final guard/partial input failure), plus `clipboardWriteFailed`. Preclaim failures report `sourceConsumed:false`; every postclaim success or failure consumes all siblings. Never retry automatically. Users must check the target and explicitly rerun for a fresh source. Ordinary hidden events retire the UI owner; a late outcome cannot reopen or restore old results.

The final foreground/sequence checks reduce races but do not establish an OS-wide atomic target lock. Target closure, elevated applications, secure desktop or a noneditable focus may prevent insertion. Native clipboard, window and keyboard acceptance remains user-operated; automated tests inject drivers and do not produce those effects.


## Host 0.15.0：主程序密钥业务服务

Host 0.15.0–0.17.x 的有道业务调用仅支持旧 `command` 形态的 `mainResult` 包，且 `runtime.resultActions` 必须省略或为 `false`。`commands` 多命令包、Panel、子窗口及启用结果动作的包均不能调用有道业务方法；即使权限已授予，也会拒绝 `InvalidServiceRequestError`。其他插件形态仍可声明和管理三项服务授权，以供未来能力使用。`api.services` 随 Runtime 网络能力提供，其存在不代表服务已授权，也不代表插件命令/输出形态兼容。该早期 Host 的百度 OCR 与 DeepSeek 没有公开业务方法；百度 OCR 从 Host 0.20.0 起支持 mainResult，并从 Host 0.21.0 起支持下述图片 Panel，DeepSeek 仍未开放。


Windows Runtime 的 `api.services?.youdao.translate({text, from, to})` 使用宿主配置的有道翻译密钥；插件不接收密钥、签名、URL 或认证参数。声明 `credentials.youdaoTranslation`、`network.https`，最低版本 `0.15.0`，并声明 `network.httpsHosts: ["openapi.youdao.com"]`。用户在设置 → 权限单独授权，在设置 → 密钥配置中配置并启用服务；两者独立，关闭配置不会清除授权。

Host 0.15.0–0.17.x 仅允许 `en` → `zh-CHS` 或反向。text 不 trim，须为 1..8192 UTF-8 字节且不含 NUL。只接受三个自有可枚举数据属性，不执行 getter、不接受额外字段。Host 0.15.0–0.17.x 返回深冻结的 `{translations: string[]}`；facade 与原请求绑定，处理结束后保留的方法拒绝 `ExpiredRequestError`，不能借用后续调用。

固定错误名：`InvalidServiceRequestError`、`ServicePermissionDeniedError`、`CredentialNotConfiguredError`、`CredentialDisabledError`、`CredentialUnavailableError`、`PermissionDeniedError`、`NetworkTargetDeniedError`、`NetworkTimeoutError`、`NetworkFailureError`、`NetworkResponseTooLargeError`、`NetworkResponseInvalidError`、`NetworkLimitExceededError`、`ExpiredRequestError`、`ProviderAuthenticationError`、`ProviderQuotaError`、`ProviderFailureError`。错误不包含真实凭据或供应商原始响应。

`credentials.baiduOcr` 和 `credentials.deepseek` 是独立的默认关闭授权，不能替代有道授权。声明服务权限仅支持 Windows，最低 Host 为 0.15.0；百度 OCR 业务方法从 Host 0.20.0 起提供，DeepSeek 尚无公开业务方法。翻译示例申报三项服务权限，但仅调用有道服务。
## Host 0.16.0：划词翻译入口

`selection.read` 是 Windows Host 0.16.0+ 的独立权限，表示用户主动按取词快捷键时允许宿主将本次选中文字作为插件输入。它不开放任意剪贴板读取、后台选区监控或输入模拟 API；`clipboard.read` 仍保留且未支持。安装、升级不默认授予该权限，已有明确授权/撤销保持不变。

Host 0.16.0–0.17.x 的宿主入口仅用于 `com.uipilot.translate` 的旧式单命令 `submit + mainResult`。快捷键与普通启动快捷键分开管理，初始 Alt+A，可在插件详情录制或清除。插件禁用、删除、未授权时不得捕获选区；快捷键冲突显示不可用，不覆盖其他绑定。

宿主先绑定外部前台窗口并捕获选区，再显示主界面，以稳定插件与命令身份填入并自动提交一次；插件仍只接收 `PluginInvocation.input`，沿用有道、网络授权及配置检查。无选区、失败、NUL 或超过 8192 UTF-8 字节时不发出服务请求，不将旧剪贴板内容当作选区。重复事件不重放，过期捕获不能覆盖更新的界面操作。

宿主优先读取 Windows 可访问性选区；没有 TextPattern 的控件可进入临时 Ctrl+C 兼容路径。宿主先在内存缓存原剪贴板格式，再读取本次新增复制的文本，仅在确认剪贴板仍属于本次操作时恢复旧内容；新的复制或输入会使旧操作失效。无法安全备份时在复制前终止。原剪贴板内容不交给插件，不把旧内容当作选区。临时复制/恢复被排除在 UiPilot 历史之外，Win+V 仅定向清理能够识别的新增记录，不删除旧记录或清空历史。系统历史访问失败或清理失败会显示相应提示；第三方历史管理器不在该合同内。主程序不在日志中记录选中文字或缓存内容，真实跨应用兼容性由用户主动验收。


### Panel contextual operations (Host 0.16.0+)

`onActions(provider)` registers one synchronous provider (returns unsubscribe). Called before menu focus with `{ source: 'hostInput' | 'content' }`; return null to retain modal ownership, otherwise `{ context?, global? }` with at most 12 actions each. Actions require unique ASCII id (`[a-z][a-z0-9-]{0,63}`, reserved `host-` prefix), title (80 Unicode scalars), and run callback; shortcut (32), disabledReason (160), and SDK icon enum are optional. Text rejects control characters and renders as text, never HTML. Disabled actions cannot execute. Callbacks stay inside that Panel document, grant no extra permissions, execute once after local focus restoration, and are invalidated on session/input update. Plugin callbacks must additionally check captured object identity. Exceptions produce a fixed Host status without exception details.

The Host owns the Ctrl+K footer entry and shared menu renderer in the appropriate WebView. Context actions precede global actions. Esc/Ctrl+K dismiss and restore focus/selection; menu keys do not reach plugin handlers. No instructional footer is shown inside the menu. Main-input Search uses a session-bound single-use ticket, accepted only while the input remains unchanged. Main-only menu-open and content-only submit commands retain caller-label checks; no raw invoke API is exposed.

### Panel Delete Host Key（Host 0.16.0+）

`panel.hostKeys` 新增 `Delete`，规范排序位于 `Enter` 之后；声明它需要 `minimumHostVersion >= 0.16.0`。SDK 的 `PluginPanelHostKeyEvent.key` 相应增加 `Delete`。

仅对声明此键的当前 Panel，在主输入框为空、无 Ctrl/Meta/Shift/Alt、非 IME composing 且非重复 keydown 时转发。主输入框含文字时保留原生 Delete 编辑行为。Ctrl+K 菜单拥有输入时不投递给背后列表。既有串行投递、会话/序号、ack、超时和焦点策略保持不变，Delete 不授予 Enter 粘贴票据。

`onActions` 的 `shortcut` 仍只负责展示，不注册快捷键；插件需要声明 hostKeys 并通过 onHostKey 执行相同动作。内容区直接处理 Delete 时同样保护可编辑控件、IME、修饰键、长按及已消费事件。删除回调必须捕获并复核选中对象/会话，进行中禁用重复操作。不可用时用 disabledReason 解释，反馈用 setStatus。

### Clipboard history search and copy (Windows Host 0.17.0+)

`clipboardHistory.list(input?: { query?: string })` accepts only the optional `query` field. No argument and empty/whitespace queries return all summaries. Queries are limited to 512 Unicode characters before trimming; invalid input rejects with `TypeError`. The Host matches a case-insensitive contiguous substring against the complete text or any file name (including extension, excluding parent directories). Nonempty queries exclude images. Results preserve recent-first ordering and the history revision. A filtered result never replaces the unfiltered `onChanged` snapshot cache; when history changes, query again and discard stale requests using the Panel session and a request generation.

`clipboardHistory.copy({ id })` requires declared and granted `clipboard.history.read` plus `clipboard.history.copy`, exact Windows support, `submit + panel + ui.panel`, and `minimumHostVersion >= 0.17.0`. New copy permission is never implicitly granted by an upgrade. It uses the current active Panel session and resolves to `{ outcome: 'copied' }` only after the Host writes the complete text, original image, or file list to the system clipboard. It preserves the Panel and focus and does not simulate paste. Unknown fields are rejected. Stable errors are `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, and `ClipboardWriteFailed`; error messages are redacted. There is no retry and no routed Enter ticket requirement. The existing `paste` API and permission retain their behavior.


### Translation Panel and detection (Windows Host 0.18.0+)

The Youdao service keeps legacy single-command `mainResult` support (without
`runtime.resultActions`). It additionally admits a legacy `command` with
`submit + panel`, `panel.runtimeActions: true`, declared and granted `ui.panel`,
and `minimumHostVersion >= 0.18.0`, only from its current `onPanelAction` Runtime
request. Multi-command, Window and result-action packages remain unsupported.
The content Panel uses its request bridge; it does not contact Youdao directly.

Host 0.18.x supports English (`en`) and Simplified Chinese (`zh-CHS`).
Host 0.19.0 adds all 112 canonical codes in [the Youdao language catalog](./youdao-languages.json);
requests using any added language require `minimumHostVersion >= 0.19.0`.
Source additionally accepts `auto`; target never accepts `auto`. The catalog records
which 83 languages the provider documents as auto-detectable; the remaining
languages require explicit source selection. Advertised languages do not imply
every language direction or account configuration is accepted by the provider.
Auto and explicit same-language inputs require `minimumHostVersion >= 0.18.0`.
The existing bounded input and independent `network.https`,
`credentials.youdaoTranslation`, HTTPS host allowlist and enabled Host credential
checks still apply. An explicit same-language call returns the original text
without provider transport after all admission checks; it cannot bypass revoked
permissions, a stale request or disabled credentials.

The frozen response is `{translations, sourceLanguage, targetLanguage, unchanged}`.
`sourceLanguage` is a canonical catalog code or `null`, based only on the provider `l` pair;
provider code casing is normalized, and unknown/unsupported detection is never
guessed from text. Host requests
`strict=true`, normalizes language spelling and rejects an actual target that
mismatches the requested target with `NetworkResponseInvalidError`. A missing
legacy provider language field yields `sourceLanguage: null`. The target is
always the requested supported language. `unchanged` is true for explicit
same-language calls or provider detection equal to target. Existing `translations`
remains available. Host 0.19.0 adds `ProviderLanguageUnsupportedError` for provider
error `102`; raw provider diagnostics and credentials are not exposed.

`selection.read` also admits this authorized Panel shape on Host 0.18.0+. The
Host still owns explicit selection capture and restoration, passes the captured
text as this Panel invocation's input, and invalidates late capture after intent,
plugin generation or permission changes. Failed captures never translate stale
clipboard or Panel text. The previous mainResult entry remains supported.


### Panel clipboard and local speech (Windows Host 0.18.0+)

A legacy `command` using `submit + panel + ui.panel`, exactly Windows support and
`minimumHostVersion >= 0.18.0` may declare `clipboard.write` and/or `speech.speak`.
The readonly optional `panel.clipboard` and `panel.speech` facades appear only for
their declared capability; each call independently checks its current grant and
captured Panel session. Runtime, main-window and Window callers are denied.

`clipboard.writeText({text})` preserves text including whitespace. It rejects
empty strings, NUL and more than 16384 UTF-8 bytes. It resolves after the OS
write succeeds, preserves focus and the Panel, and neither reads nor pastes.

`speech.speak` is an independent, default-off permission. Installation/update
consent lists it, existing explicit grants remain, and new grants require the
separate confirmation in Settings → Permissions. Speech has no network or
credential dependency. Call it only from an explicit user button/menu action.
`voices()` returns available local `{id,name,language}` entries. Host 0.18.x
exposes `en` and `zh-CHS`; Host 0.19.0 maps installed SAPI locales to the
canonical language catalog, preserving Chinese and Serbian script distinctions.
`play({text,language})` uses the same text limits and exact language allowlist.
Translation support alone never implies an installed matching voice. The Host selects a matching local SAPI voice; there is no `voiceId`
override, voice download, audio URL, file or XML/SSML input.

`play`, `stop({playbackId})` and `state()` return
`{playbackId:string|null,status:'idle'|'playing'|'completed'|'stopped'|'error',language:SpeechLanguage|null,errorCode?:string}`.
`onState(handler)` returns an unsubscribe function. States and events belong to
the captured session. Global playback has one owner; new playback stops old
playback, while old IDs and late completion cannot affect the replacement.
Hide, replacement, disable, reload, uninstall and permission changes revoke the
old owner before queued native work. Retained facades and late promises reject
`ExpiredPanelSession`. Canceled mutations may issue a fresh authorized lease;
old leases never become valid again. The bounded worker queue rejects overload
without blocking revocation. WebViews remain muted and media CSP unchanged.

Fixed sanitized errors: `PermissionDenied`, `ExpiredPanelSession`,
`InvalidPanelText`, `ClipboardWriteFailed`, `SpeechUnavailable`, `SpeechBusy`,
`SpeechVoiceUnavailable`, `SpeechFailed`. Missing local matching voices report
`SpeechVoiceUnavailable`; no fallback to another language or automatic retry.
Native diagnostics, text and OS errors are never returned to plugin code.


### Panel settled readiness (Host 0.18.0+)

The optional `panel.onReady(handler)` returns an unsubscribe function. Register
one receiver at boot, before the first `onUpdate` registration; registration
later is rejected and there is no replay. Capability-detect this additive API.
The receiver gets a frozen `{sessionEpoch: U64Decimal, requestId: string}` once
per current update, only after `onUpdate` returns, its ACK settles, and the Host
has restored Ready. Use `onUpdate` only to install state/DOM, and initiate an
automatic initial Runtime request from `onReady`. A timer or microtask scheduled
inside `onUpdate` cannot establish Host settlement. Requests during update/ACK
still reject Busy; existing permissions, ownership and Runtime slot checks apply.

Buffered or promoted newer input suppresses an older notification. A new
update, PREPARE (even with the same epoch), teardown or unsubscribe invalidates
queued callbacks. The Host does not await the receiver; thrown/rejected errors
are contained without exposing exception details, retrying, or ACKing again.
The hook grants no effect permission and does not authorize automatic speech.

## Host 0.20.0：图片输入与百度 OCR

Windows 的 `submit + mainResult` 命令可声明 `inputCapabilities: ["image.ocr"]`，要求 `minimumHostVersion >= 0.20.0`。单命令和多命令包均可使用；未知或重复能力、live、window 及其他平台组合被拒绝。Host 0.20.0 不支持图片 Panel；Host 0.21.0 的扩展见下一节。省略该字段保留原文字合同。此声明表示图片 OCR 输入能力，不代表网络或凭据授权。

主输入框粘贴图片后，宿主列出已启用且兼容的 OCR 命令，用户回车选择后才调用 Runtime。`PluginInvocation.input` 仍是字符串，图片请求另含只读 `image: {id, mimeType, width, height, byteLength}`。图片满足 OCR 命令的必需输入，无需额外输入文字；无图片时不能执行识别。

图片 ID 是宿主内存快照的不透明引用，绑定启动器会话、图片版本、插件身份/代际和本次 Runtime 请求。它不是路径或剪贴板 getter。插件不能枚举图片、读取其他插件图片或跨请求保留权限。替换/移除图片、退出图片流程、隐藏主窗口、插件失效或授权撤销会使旧操作失效；迟到结果不得发布或复制。

`api.services.baidu.ocr({imageId})` 返回深只读 `{text: string, lines: readonly string[]}`。`text` 按识别行顺序以换行连接。调用需要当前请求绑定的图片、`credentials.baiduOcr` 和 `network.https` 声明与授权，以及 `network.httpsHosts: ["aip.baidubce.com"]`。复制结果还需 `clipboard.write`。设置中已配置且启用的百度 OCR 服务由宿主使用，API Key、Secret Key、访问令牌和原始供应商响应不传给插件。

首版使用通用文字识别标准版 `general_basic`。每次显式识别最多一次认证和一次 OCR 请求，不自动重试，不因粘贴或选中列表项发送图片。此服务的有界图片上传能力不改变普通 `api.network.request` 的请求大小限制。

宿主接受 PNG/JPEG/BMP 单图，输入编码最大 8 MiB、最长边 4096px、最多 1600 万像素，标准化 PNG 最大 3 MiB；这些是产品边界。坏图片或超限导入不替换原有效图片。无文字时返回空文本/空行数组；插件应返回无默认动作的说明项，不复制空值。

服务使用现有固定错误名，包括权限/凭据不可用、认证失败、额度不足、网络失败/超时、无效或过大的响应及 `ExpiredRequestError`。错误不携带密钥、令牌、图片或原始响应。请求结束后保存的方法和旧图片 ID 均不能借用新请求。

OCR 结果沿用 `mainResult` 与 `defaultAction: {type: "copyText", text}`。识别完成不自动复制，用户再次按下 Enter 才执行复制。宿主在图片结果流程中按纯文字渲染详情，保留换行；插件应保留全文并检查现有 16 KiB 详情及 64 KiB 整体响应限制，超过时明确失败，不能静默截断后复制。

上述 mainResult 合同继续兼容。当前业务参考包已迁移为 [ocr Panel](../../examples/public-plugins/com.uipilot.ocr/README.md)，采用以下 Host 0.21.0 合同。

## Host 0.21.0：图片 OCR Panel

`image.ocr` 扩展到 Windows 单命令 `submit + panel`，要求 `minimumHostVersion >= 0.21.0`、`ui.panel` 和 `panel.runtimeActions: true`。API/schema 仍为 v1；不开放多命令 Panel、live 或 window。旧 mainResult 和没有图片能力的 Panel 保持原合同。

用户可先粘贴图片、从按能力匹配的列表选择插件，也可先执行 `/ocr` 打开空 Panel，再在宿主输入框粘贴。列表不优先某个插件 ID。图片交接后由 Panel 会话持有，主输入区显示命令 tag，预览在内容区；后续换图更新当前 Panel，不重开面板。坏图或超限导入保留原有效图片。退出、隐藏或生命周期失效后释放图片，重入为空。

### 图片通知与清除

符合上述形态的桥提供可选 `onImageChanged(handler)` 和 `clearImage()`。图片接收器必须在首次 `onUpdate` 注册前安装。每个当前会话在 Ready 后发送一次最新快照，此后只在图片修订变化时发送；未 Ready 时只保留最新修订，重复 Ready/主题更新不重复发送。取消订阅、退出及旧会话排队通知失效。

事件深只读：`{ sessionEpoch: U64Decimal, revision: U64Decimal, image: PanelImageSnapshot | null }`。`PanelImageSnapshot` 包含 `id`、`mimeType`、`width`、`height`、`byteLength` 与 `previewDataUrl`；修订号按十进制 u64 字符串处理。预览为最长边 512px、编码最多 40 KiB 的 PNG，整个事件最多 64 KiB。等比例显示且不放大超过实际像素；预览只用于显示，识别使用原标准化快照，原图资源限制不变。

`clearImage(): Promise<void>` 只清除当前会话图片并发布递增修订的空快照，不调用 OCR；旧会话拒绝 `ExpiredPanelSession`。插件应立即清空旧正文、禁用复制，并拦截清除后迟到的识别/复制反馈。

### Panel action 图片权限

`PluginPanelRequest` 新增可选 `imageId: string`，非空、不含 NUL，UTF-8 编码最多 512 字节。省略保留旧请求行为；该字段只接受当前有图片能力 Panel 的最新快照 ID。字段/类型错误为 `InvalidPanelRequestError`，旧会话为 `ExpiredPanelSession`，非当前图片引用为 `PanelRequestCancelledError`。业务 `payload` 不建立图片权限。

```javascript
const reply = await panel.request({
  action: 'recognize', imageId: snapshot.id, payload: { providerId: 'baidu' },
})
```

宿主接纳后向 `onPanelAction` 注入可选只读 `invocation.image`，其中 `id` 是本次请求专用引用，与事件里的 Panel 图片 ID 不同。Runtime 调用 `api.services.baidu.ocr({ imageId: invocation.image.id })`；未携带图片的普通 action 不获得此字段。调用仍要求当前 `credentials.baiduOcr`、`network.https`、`aip.baidubce.com` 声明与授权，以及宿主中已配置并启用的百度 OCR。复制另需 `clipboard.write`；服务源名称和凭据归宿不变，不把原图 Base64 或凭据放进业务 payload、存储或日志。

`onCommand` 只初始化 Panel；自动识别从匹配的 `onReady` 后发起，后续有效图片变更在会话 Ready 时请求。不得在 `onUpdate`/ACK 或 Host Key 回调中等待 Runtime request。Host 仍执行每插件一个物理 handler、Busy 拒绝、不排队/重试和原 30 秒期限。换图或清图使旧图片请求失效；已发出的远端请求可能完成，迟到结果不能发布或取得新图片权限。隐藏、退出、撤权、凭据变更及其他生命周期失效撤销旧上下文，重新授权不复活旧请求。

当前 `ocr` 插件最多保留一个在途识别和一个最新 pending 图片，A→B→C 时略过 B；换图立即清空旧文字，同源、重复 Ready 和主题更新不重识别。回复沿用 `{requestId, data}`，成功为 `{ok:true, text, lines}`，失败为有界固定 `{ok:false, code, message}`。正文最多 16 KiB UTF-8，整体回复最多 64 KiB，超限明确失败而不截断。

右侧文本域可编辑，复制使用 `panel.clipboard.writeText` 读取当前完整文本。无图、识别中、空结果和失败禁用复制；仅显式重试重新调用服务。空主输入的非重复 Enter 可复制，文本域 Enter 保留换行/IME；Tab/Shift+Tab 交接内容焦点，复制反馈使用宿主状态栏并保留 Panel。

### 新身份交付与验收

业务插件为 `com.uipilot.ocr@1.0.0`，显示名称/默认命令 `ocr`，要求 Host 0.21.0+；交付包 `artifacts/ocr-delivery/com.uipilot.ocr-1.0.0.uipilot-plugin`。正常安装确认新身份的权限与百度主机；旧 `com.uipilot.baidu-ocr` 的授权、收藏、快捷键不自动迁移，旧副本不自动卸载。需要时在设置中手动停用旧版，避免双条目；历史旧包保留。

离线假服务、DOM/桥、SDK 和归档校验不代表原生验收。Light/Dark、真实图片粘贴、跨 WebView 焦点、真实百度调用和系统复制由用户手动验收；本次不使用 Computer Use，不自动安装、授权或调用真实服务。
