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
- Windows Panel clipboard history requires `minimumHostVersion >= 0.3.3`, exact permissions `clipboard.history.read` and optionally `clipboard.history.paste`, plus `submit + panel + ui.panel`. `clipboard.read` remains reserved and unsupported; it is not an alias for clipboard history.

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

Stable identity is `(pluginId, commandId)`. Legacy `command` internally uses `default`; a new package may explicitly retain that ID or omit it entirely. Version, startup name and array order do not define identity. One Runtime, onCommand, settings, storage, permissions, network grants, enable switch and package lifecycle serve the whole array. Each main-result command can be searched, renamed and favorited independently; result rows retain the existing optional copyText default action.

Manifest schemaVersion and Runtime apiVersion remain 1. Internal state schema is 2; inventory schema and CLI report schema remain 1. These are independent of the plugin's own version, the Host release version, and the CLI npm package version (currently 0.1.0). Host current and the CLI's production Host target are 0.7.0 and accept valid commands packages, including one-item arrays. Old example minimums and the credential capability's 0.4.0 minimum remain unchanged. The [offline multi demo](../../examples/public-plugins/com.uipilot.demo-multi/README.md) provides its complete installable package under package/. Historically, the 0.5.0 local release passed full automated verification and final independent code review at b84bda3. Its core local installation/update, multi-command behavior, retained command names/favorites/configuration across restart/update, and Notes persistence were user-confirmed; this did not cover the full native/theme/credential-service/installer acceptance matrix. Host 0.7.0 manual acceptance remains pending.

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

`mainResult` returns zero to twenty plain-text items. IDs are non-empty and unique. Titles are at most 256 Unicode scalar values, subtitles at most 512, and details at most 16 KiB UTF-8. An item may have one `copyText` default action when `clipboard.write` is declared and granted. `actions[]`, custom labels, callbacks, links, commands, HTML, Markdown, and multiple actions are unsupported.

`window` returns `{ requestId, data }`. The host creates or reuses one window for that plugin and sends a `PluginWindowUpdate` through `window.uipilotPluginWindow.onUpdate`. Content receives input, platform, theme, invocation time, singleton instance `1`, and plugin data. It cannot invoke commands or own pin, close, drag, focus, theme, or position behavior.

`panel` returns `{ requestId, data }`. The host mounts one launcher panel session for that plugin and sends a `PluginPanelUpdate` through `window.uipilotPluginPanel.onUpdate`. Panel content receives input, platform, theme, invocation time, session epoch, and plugin data. Its bridge exposes private storage plus `onHostKey(handler)`, `focusHostInput()`, `requestHide()`, and, when the manifest declares clipboard history permissions, `clipboardHistory`; it cannot own timers or publish notifications. The base panel contract requires host `0.3.0+`; the three `0.3.1` methods require `minimumHostVersion >= 0.3.1` when used.

`window.uipilotPluginPanel.focusHostInput()` moves keyboard focus to the live panel session's tagged launcher argument input. It preserves the panel, command tag, argument text, selection, and submission state. Repeated calls while that input is already focused succeed. Missing, replaced, torn-down, or stale sessions resolve without side effects; a current native-focus, event, acknowledgement, or timeout failure rejects with `windowFailed`. The method does not stream edits to panel content: argument changes reach `onUpdate.input` only after the user presses Enter.

### Panel Host Keys And Return

`panel.hostKeys` is optional, contains at most eight unique declarations, and accepts only `ArrowDown`, `ArrowUp`, `Primary+N`, `Tab`, `Shift+Tab`, and `Enter`. Validators reject unknown values, duplicates, non-arrays, wrong element types, and extra panel properties. Canonical order is `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter`. `Tab`, `Shift+Tab`, and `Enter` require `minimumHostVersion >= 0.3.3`.

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

### Panel Clipboard History

`window.uipilotPluginPanel.clipboardHistory` is Windows-only and available only to `submit + panel + ui.panel` plugins declaring `clipboard.history.read`; paste also requires `clipboard.history.paste`. The Host records text, image, and file-list clipboard changes while UiPilot is running, the plugin is enabled, and permission is granted. Panel code receives only summaries through `list()` and `onChanged(handler)`: text previews, image thumbnails, file names/counts, availability, `id`, `capturedAt`, and `revision`. Image summaries use displayable `data:image/png;base64,...` thumbnail URLs; the public-plugin CSP still rejects network, file, and `blob:` images. It never receives raw text, original PNG bytes, complete file paths, HWND, PID, arbitrary key names, or paste counts.

`clipboardHistory.paste({ id, routeSequence })` is accepted only from the current Panel session after a routed Enter event. It returns `{ outcome: 'admitted' }` before teardown, then the Host writes the selected record to the system clipboard, hides UiPilot, restores the previously captured external window when safe, and sends one platform paste chord. Failures expose stable `Error.name` values: `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, `PasteTargetUnavailable`, and `ClipboardWriteFailed`. Error messages are redacted.

Every plugin content window also sees the frozen `window.uipilotPluginWindow.timer` facade. Calls require the Windows-only `timer.control` permission together with `ui.window` and `notifications.publish`; unpermitted callers receive `PermissionDenied`. The host owns one process-local timer per active plugin generation, continues it while the window is hidden, and discards it on process exit.

Every valid plugin content window also receives the frozen `window.uipilotPluginWindow.storage` facade without an additional permission. It shares the Runtime `api.storage` namespace for that plugin. `get` is available while the window session is prepared or active; `set` and `remove` are active-only. A hidden, replaced, disabled, upgraded, or uninstalled session returns `ExpiredWindowSessionError`. Keys must match `^[a-z][a-z0-9.-]{0,63}$` and cannot be `__proto__`, `prototype`, or `constructor`; values are finite JSON and count toward the same 5 MiB plugin quota.

A `timer.control` package supplies its own fixed `assets/sounds/timer-alarm.wav`, while the host exclusively validates, freezes, and plays it. The WAV must be little-endian PCM with one or two channels, a 44.1 or 48 kHz sample rate, and 16- or 24-bit samples; it must not exceed 2 MiB or 15 seconds. Unknown or duplicate chunks, trailing bytes, additional WAV files, and Runtime/content access to the alarm are rejected. Timer completion loops the alarm frozen at round start until the main window opens. Ordinary plugin messages use the host's shared one-shot notification sound instead. Full session, revision, pause/reset, completion-message, and alarm behavior is documented in the developer guide.

The complete serialized response budget is 64 KiB. Unknown fields, duplicate keys, non-finite numbers, prototype keys, invalid actions, and over-budget responses reject the entire response.

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
- `clipboard.write`: expose a host-owned `copyText` default action.
- `clipboard.history.read`: on Windows Host `0.3.3+`, allow the Host to collect this plugin's local clipboard history and expose redacted summaries to its Panel.
- `clipboard.history.paste`: on Windows Host `0.3.3+`, allow a current Panel session to restore one selected clipboard history record and paste it once after a routed Enter event; requires `clipboard.history.read`.
- `network.https`: on Windows Host `0.3.2+`, expose bounded request-scoped Host-managed HTTPS to the exact authorized `network.httpsHosts` set.
- `notifications.publish`: on Windows only, submit one immediate or host-owned delayed plain-text message and ask the host to show its own notification and tray reminder.
- `timer.control`: on Windows only, control one host-owned plugin-window timer that can continue after the window hides; requires `ui.window`, `notifications.publish`, and `submit + window`.

Other parsed permission names are reserved and installation fails until the host implements them. Permission changes during reload require normal confirmation; no development-package bypass exists.

## Unsupported In v1

Arbitrary background execution, plugin-owned timers, repeating or persistent scheduling, multi-command panel/window output, multiple windows, per-command hotkeys, independent alias collections, actions arrays/onAction, streaming, pagination, large responses, browser/WebView networking, raw sockets, arbitrary files, raw clipboard read, native binaries, Shell, input synthesis, plugin-to-plugin communication, remote media, dependencies, signing, marketplace delivery, and automatic updates are outside this MVP. External access is limited to request-scoped Host-managed HTTPS and the Host-managed clipboard history summary bridge described above. Delayed work is limited to the host-owned, process-local `notifications.schedule()` message and the single-generation window timer described above.

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
