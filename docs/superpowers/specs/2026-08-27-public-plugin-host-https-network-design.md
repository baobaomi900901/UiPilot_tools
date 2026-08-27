# Public Plugin Host-Managed HTTPS Network Design

**Date:** 2026-08-27
**Status:** Approved - independent technical and security review passed with no findings; approved by the user on 2026-08-27

**Related:**
`docs/superpowers/specs/2026-08-20-public-plugin-validation-cli-design.md`,
`docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`,
`docs/plugin-sdk/uipilot-plugin-v1.schema.json`,
`docs/plugin-sdk/public-plugin-developer-guide.md`

## 1. Goal

Add a Host-managed HTTPS request capability to public plugin Runtime command
handlers. A plugin may request only exact HTTPS hosts declared in its Manifest
and authorized by the user. The Host owns URL policy, DNS resolution, address
validation and pinning, TLS, redirects, limits, cancellation, response parsing,
and error redaction.

The public Runtime interface is intentionally small:

```ts
api.network.request(input): Promise<PluginNetworkResponse>
```

The implementation behind that interface is the deep `PluginHttpsBroker`
module. Plugin Runtime WebViews retain their current CSP and never receive a
general `fetch`, socket, proxy, or Tauri invoke capability.

Host version becomes `0.3.2`. Public plugin `apiVersion` remains `1`; the new
Manifest field and Runtime interface are optional and backward compatible.

## 2. Scope

This design includes only UiPilot Host work:

- Rust Manifest DTOs and validation.
- Canonical JSON Schema and the plugin CLI's bundled/generated Schema.
- Install, update, inventory, authorization, revocation, and durable state.
- A Host-owned asynchronous HTTPS broker and lifecycle cancellation registry.
- Runtime bootstrap, command DTOs, capabilities, and SDK declarations.
- Main settings UI for authorization display, revocation, and reauthorization.
- Public plugin contract and developer documentation.
- Host, Schema, CLI, SDK, frontend, and deterministic network-policy tests.

## 3. Non-goals

- Implementing or changing a translation plugin, demo plugin, Notes, or any
  provider adapter.
- Embedding or committing translation-provider credentials.
- Host translation, language detection, translation history, or provider
  selection.
- Exposing plugin WebView `fetch`, XMLHttpRequest, WebSocket, EventSource, raw
  sockets, arbitrary Tauri invoke, or a browser-like networking surface.
- HTTP, custom ports, arbitrary IP targets, localhost, private networks, Unix
  sockets, file URLs, or system/environment HTTP proxies.
- Background requests, requests outside a live Runtime command, streaming
  responses, file upload, multipart bodies, binary responses, or downloads.
- Plugin-controlled `AbortSignal` / `AbortController` in the MVP.
- Cookie persistence or a cookie jar.
- Secure Runtime retrieval of secret settings. Secret consumption receives a
  separate design before a production translation plugin ships.
- macOS network support in Host `0.3.2`. `network.https` is Windows-only in this
  MVP and remains unsupported for a macOS validation target.

## 4. Manifest Contract

### 4.1 Shape

`PublicManifestV1` gains optional `network`:

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "minimumHostVersion": "0.3.2",
  "permissions": ["network.https"],
  "network": {
    "httpsHosts": ["openapi.youdao.com"]
  }
}
```

The Rust and TypeScript DTOs are:

```rust
pub(crate) struct PublicNetworkV1 {
    pub(crate) https_hosts: Vec<String>,
}
```

```ts
interface PublicNetworkV1 {
  httpsHosts: string[];
}
```

Both reject unknown fields. `network` is absent or a non-null object; explicit
`null` is invalid.

### 4.2 Hostname Grammar

Each `httpsHosts` entry is an exact DNS host, not a URL or origin. It must:

- Be non-empty canonical lowercase ASCII and at most 253 bytes.
- Contain at least two labels separated by `.`.
- Use labels of 1-63 bytes containing only `a-z`, `0-9`, and interior `-`.
- Have no leading/trailing `-`, leading/trailing `.`, empty label, underscore,
  wildcard, scheme, user information, path, query, fragment, or port.
- Not be an IPv4 or IPv6 literal.
- Not equal `localhost` or end in `.localhost` or `.local`.
- Not contain a label beginning `xn--`. Raw Unicode and every IDN form are
  deferred rather than relying on divergent Host/CLI IDNA implementations.

The array contains 1-8 unique entries. Schema rejects wrong types, empty and
oversized arrays, duplicates, and obvious grammar violations. Rust and CLI
semantic validation apply the complete identical grammar. Input order is not a
contract: prepare/inventory DTOs and durable authorization snapshots sort hosts
by ASCII bytes.

### 4.3 Cross-field and Version Rules

- `network` is present if and only if `permissions` contains exact
  `network.https`.
- A non-empty `httpsHosts` declaration requires `minimumHostVersion >= 0.3.2`.
- `network.https` is available only when the validation target is Windows.
- Old Manifests with neither field remain valid and receive no network object
  in Runtime.
- Unknown permissions, fields, host tokens, duplicates, excessive host counts,
  and illegal cross-field combinations fail package validation before install.
- Canonical Schema, Rust validation, and CLI validation produce the same accept
  / reject result for the shared host-policy fixture corpus.

## 5. Authorization And Durable State

### 5.1 Prepare Summary

`PublicPluginPrepareSummary` gains:

```ts
network: null | {
  httpsHosts: readonly string[];
  addedHttpsHosts: readonly string[];
  requiresNetworkConsent: boolean;
};
```

Fresh install with network access always sets `requiresNetworkConsent: true`
and treats every host as added. Update compares the prepared package against
the currently installed authorized-host snapshot:

- Adding `network.https` or any host requires new consent.
- Removing hosts never requires new network consent and narrows authority at
  update commit.
- Changing only order never requires new consent.
- A previously revoked network grant remains revoked when an update only
  removes hosts.
- If an update adds a host, the old installed version and old authorization
  remain active until the user explicitly confirms the update.

Prepare tokens freeze the candidate digest, declared permissions, normalized
host set, old authorization snapshot, and whether consent is required. Commit
revalidates all of them; frontend data is display-only and never authoritative.

### 5.2 Installation UI

The install/update confirmation displays:

- The plugin identity and version already shown today.
- A distinct `network.https` permission row.
- Every exact requested hostname, one host per line.
- Added hosts marked as newly requested during an update.
- Fixed text that requests are Host-managed HTTPS only and do not grant general
  WebView networking.

Consent is atomic for the complete host set. There are no per-host checkboxes.
Declining cancels the install/update. The Host never installs or updates a
network plugin with only a subset of its requested hosts.

### 5.3 Stored Grant

The durable plugin state adds a normalized `network_https_hosts_grant` set.
Valid state satisfies exactly one of:

1. Manifest has no network declaration, permission grant excludes
   `network.https`, and host grant is empty.
2. Manifest declares network, permission grant includes `network.https`, and
   host grant exactly equals the Manifest host set.
3. Manifest declares network, permission grant excludes `network.https`, and
   host grant is empty because the user revoked it after installation.

All non-network permissions retain the existing all-declared/all-granted
invariant. Old durable documents default the new set to empty. A document that
claims `network.https` without an exact host grant fails closed as revoked and
is repaired on the next durable mutation; it never gains network authority.

Install/update state, package activation, permission grant, and normalized host
grant use the existing durable three-state transaction contract:

- `NotCommitted` guarantees the old version/authority only after the old
  digest/state summary is revalidated.
- `Committed` must finish in-memory state/bundle publication for the new
  generation.
- `Unknown`, or failure after durable commit but before publication, closes
  network authority and makes the public plugin service terminal/fail-closed;
  restart recovers from durable state.

Network authority is never enabled from an uncertain transaction outcome. This
MVP does not redesign or promise rollback beyond the existing durable contract.

### 5.4 Inventory, Revocation, And Reauthorization

`PublicPluginInventoryItem` gains:

```ts
network: null | {
  httpsHosts: readonly string[];
};
```

The existing `permissions` row remains authoritative for whether
`network.https` is supported and granted. Settings renders a dedicated
`Network access` switch only for a plugin that declares network hosts:

- Turning it off atomically removes `network.https` from grants, clears the host
  grant, increments inventory revision, and cancels all in-flight requests for
  the plugin before the command resolves.
- Turning it on shows the complete current host list and requires confirmation.
  The Host re-derives the list from the active package, atomically grants the
  permission and exact host set, and increments inventory revision.
- The command accepts only plugin ID and desired granted state from the trusted
  `main` window. It never accepts hosts from the frontend.
- Disable, uninstall, upgrade replacement, fault-disable, and runtime teardown
  also invalidate active network authority and cancel matching requests.

## 6. Public Runtime Interface

### 6.1 TypeScript Contract

`PluginRuntimeApi` gains `network` only when the Manifest declares
`network.https`; old plugins cannot feature-detect it into authority they did
not declare. The Host instantiates the Runtime bootstrap template with an
immutable, manifest-derived capability bit. The Runtime cannot supply or
change that bit, and the command still revalidates every request.

```ts
type PluginNetworkRequestBody =
  | { type: "json"; value: unknown }
  | { type: "text"; value: string }
  | { type: "form"; value: Readonly<Record<string, string>> };

interface PluginNetworkRequest {
  url: string;
  method: "GET" | "POST";
  headers?: Readonly<Record<string, string>>;
  body?: PluginNetworkRequestBody;
}

interface PluginNetworkResponse {
  status: number;
  headers: Readonly<Record<string, readonly string[]>>;
  body: string;
}

interface PluginNetworkApi {
  request(input: PluginNetworkRequest): Promise<PluginNetworkResponse>;
}

interface UiPilotPluginApiV1 {
  readonly network?: Readonly<PluginNetworkApi>;
}
```

The static SDK property is optional because one `.d.ts` cannot narrow itself
from a plugin Manifest. A network plugin checks `api.network` once at command
entry and handles absence as unavailable Host capability. Runtime includes the
property only for a declaring Manifest; the Host command remains the final
authority even when the property exists.

The bootstrap recursively copies the supported input graph into fresh
Host-bridge-owned objects without freezing or mutating caller-owned values,
deep-freezes that snapshot, and calls the dedicated asynchronous Host command.
No Host request identifier, plugin ID, generation, activation ID, admission
epoch, authorization host, redirect policy, timeout, or limit is accepted from
plugin-authored input.

### 6.2 Body Encoding

- `GET` rejects any body.
- `POST` may omit body.
- `json` must be representable as bounded JSON without cycles, functions,
  BigInt, undefined object members, or non-finite numbers. Host encodes compact
  UTF-8 JSON and sets `application/json; charset=utf-8`.
- `text` accepts a string, encodes UTF-8, and sets
  `text/plain; charset=utf-8`.
- `form` accepts an ordinary object with unique string keys and string values.
  Host sorts keys by UTF-8 bytes, applies standard percent encoding, joins with
  `&`, and sets `application/x-www-form-urlencoded; charset=utf-8`.
- The plugin cannot supply or override `Content-Type`.
- Encoded body size, not the pre-encoding JavaScript value size, is limited.

Plugins that require an exact provider-specific byte representation outside
these three encodings are out of scope for the MVP.

## 7. Host Command And Identity

### 7.1 Dedicated Async Command

The Host registers `plugin_network_request` as an async Tauri command and adds
its capability only to the Public Runtime WebView. Window, panel, main, find,
private plugin, and public content WebViews do not receive it.

The command receives:

```rust
struct PluginNetworkCommandInput {
    context: PluginRequestContext,
    request: PluginNetworkRequest,
}
```

The command derives the plugin/generation identity from the caller's Runtime
window label and rejects any mismatch before allocating a quota or touching
the network. `PluginRequestContext` remains the scheduler-issued opaque
authority already delivered by the Host bootstrap; plugin-authored replacement
contexts fail exact parsing/currentness checks.

The long-running network path is separate from synchronous `plugin_api_call`.
Storage, settings, notification, and existing completion behavior remain
unchanged.

### 7.2 Admission Order

Admission applies in this fixed order:

1. Strict DTO parsing and structural limits.
2. Enter the common network authority gate with no scheduler, state, bundle,
   or registry lock already held.
3. Exact Runtime caller-label/context match.
4. Current scheduler request check.
5. Active installed package, generation, enabled, and fault checks.
6. Manifest `network.https` declaration.
7. Durable permission and exact host-grant checks.
8. Per-context call-count reservation.
9. URL, method, headers, and encoded body policy.
10. Per-context and global concurrency reservation.
11. Register the exact-context cancellation token before leaving the gate.

Malformed input rejected by Tauri deserialization does not consume quota.
After caller, context, active package, and permission admission succeed, every
attempt reserves one of the eight per-context calls; later URL/header/body
policy or concurrency rejection still consumes that call. A denied or failed
concurrency reservation does not increment active concurrency.

No authority gate, scheduler, package, durable-state, authorization, or
broker-registry lock is held during DNS, connect, TLS, write, redirect,
response-header, or body waits.

## 8. URL, Header, And Redirect Policy

### 8.1 URL

The complete URL is at most 2048 UTF-8 bytes and must:

- Parse through the Host URL parser as exact `https`.
- Have no username, password, or fragment.
- Have either no explicit port or explicit port `443`.
- Have a hostname whose canonical ASCII form exactly equals one granted host.
- Have a syntactically valid path and optional query.

The Host never logs or includes the path, query, fragment, or full URL in an
error. URL authorization uses parsed structured fields, never string prefix or
suffix matching.

### 8.2 Request Headers

`headers` is absent or an ordinary object containing at most 32 entries and at
most 16 KiB across UTF-8 names and values. Names must be valid HTTP field-name
tokens and values must reject CR, LF, NUL, and disallowed controls. Host
canonicalizes names to lowercase and rejects duplicates after canonicalization.

The plugin may set `accept`, `accept-language`, `authorization`, and
provider-specific custom headers. It may not set:

- `host`, `content-length`, `content-type`, `connection`, `keep-alive`,
  `transfer-encoding`, `te`, `trailer`, or `upgrade`;
- `cookie`, `origin`, `referer`, `user-agent`, or `accept-encoding`;
- `proxy-authenticate`, `proxy-authorization`, or any `proxy-*`;
- any `sec-*`, `forwarded`, `via`, or `x-forwarded-*`.

Host sets a fixed non-identifying User-Agent, the body-derived Content-Type,
Content-Length, and `Accept-Encoding: identity`. A cookie store is disabled.

### 8.3 Redirects

The Host disables automatic redirects and processes at most three same-host
redirect hops manually within the same total timeout. Cross-host redirects are
rejected in the MVP even when both hosts are declared. Only 301, 302, 303, 307,
and 308 with one valid `Location` are followed. Relative locations resolve
against the current URL.

Every hop repeats URL, granted-host, DNS, address, and TLS policy. A redirect to
a different host, HTTP, a disallowed port, user information, invalid location,
or denied address rejects as `NetworkTargetDeniedError`.

- 303 changes the next method to GET and drops body/body-derived headers.
- 301, 302, 307, and 308 preserve method and body.
- Host never synthesizes Referer or Origin.

## 9. DNS, Address, Proxy, And TLS Policy

For each initial target and redirect hop, the Host:

1. Resolves the exact hostname through a controlled resolver.
2. Rejects an empty answer.
3. Normalizes and checks every answer through the fixed address predicate below.
4. Rejects the entire host if any answer is denied; it never selects only the
   public-looking member of a mixed answer.
5. Freezes the accepted address set into the transport connection so the HTTP
   library cannot perform a second uncontrolled lookup.
6. Uses the original hostname for TLS SNI and certificate hostname validation.

The predicate does not call a platform/dependency `is_global` helper. It first
converts an IPv4-mapped IPv6 address to IPv4 and applies the IPv4 table. Every
other address matching one of these CIDRs is denied:

```text
IPv4:
0.0.0.0/8       10.0.0.0/8       100.64.0.0/10    127.0.0.0/8
169.254.0.0/16  172.16.0.0/12    192.0.0.0/24     192.0.2.0/24
192.31.196.0/24 192.52.193.0/24  192.88.99.0/24   192.168.0.0/16
192.175.48.0/24 198.18.0.0/15    198.51.100.0/24  203.0.113.0/24
224.0.0.0/4     240.0.0.0/4

IPv6 (after IPv4-mapped normalization):
::/96           64:ff9b::/96      64:ff9b:1::/48   100::/64
100:0:0:1::/64  2001::/23         2001:db8::/32    2002::/16
2620:4f:8000::/48  3fff::/20      5f00::/16        fc00::/7
fe80::/10       fec0::/10         ff00::/8
```

This deliberately rejects every block in the IANA IPv4/IPv6 special-purpose
registries as snapshotted on 2026-08-27, including entries marked globally
reachable, plus multicast and deprecated site-local space. The normative
registry sources are:

- `https://www.iana.org/assignments/iana-ipv4-special-registry/`
- `https://www.iana.org/assignments/iana-ipv6-special-registry/`

The committed CIDR table, not live registry data, is the Host `0.3.2` contract.
Changing it requires an explicit later Host contract change and fixture update.

Both IPv4 and IPv6 are allowed only when they survive the fixed predicate. DNS
resolution and all connection attempts share the one total request deadline.
Redirects resolve again under the same policy.

The native transport disables environment variables and operating-system HTTP
proxies. It permits TLS 1.2 or newer, validates against the Windows system trust
store, verifies certificate time and hostname, and exposes no option to relax
TLS verification. Plaintext fallback is impossible.

MVP transport instances do not pool or reuse connections across Host network
calls or plugins. A same-host redirect may reuse a connection only when its
connected peer remains in that hop's frozen validated address set; otherwise it
opens a new pinned connection. This keeps every call behind a fresh DNS/address
admission and avoids cross-plugin connection state.

DNS resolution must participate in cancellation and the total deadline. If the
chosen Windows resolver cannot cancel an underlying system lookup, its adapter
uses one bounded resolver executor: cancellation abandons delivery immediately,
the executor has a fixed worker/queue limit included in the Host-wide network
limit, and repeated abandoned lookups cannot create unbounded threads or queued
work.

The transport interface is an internal seam with two adapters:

- `NativeHttpsTransport` for production DNS/TLS/HTTP.
- `DeterministicHttpsTransport` for broker policy, race, and error tests.

Only the broker interface is visible to Runtime command code. Resolver,
connector, redirect, and HTTP-library details remain implementation-private.

## 10. Resource Limits

The fixed MVP limits are:

| Resource | Limit |
|---|---:|
| URL | 2048 UTF-8 bytes |
| Request headers | 32 fields / 16 KiB total |
| Encoded request body | 64 KiB |
| Response headers | 64 fields / 32 KiB total |
| Decoded response body | 1 MiB |
| Total deadline | 10 seconds |
| Redirects | 3 |
| Calls per command context | 8 |
| Concurrent calls per command context | 2 |
| Concurrent public-plugin calls Host-wide | 16 |

The deadline begins before DNS and covers every redirect and complete response
read. Queueing for a concurrency slot is not supported; a full limit rejects
immediately. Counters use checked non-wrapping arithmetic; exhaustion fails
closed for that context.

The Host requests identity content encoding. Any non-empty response
`Content-Encoding` other than `identity` is rejected as an invalid response, so
compressed bytes cannot expand outside the body limit. Response header limits
apply before filtering protected response fields. `NativeHttpsTransport` must
configure the HTTP protocol parser itself with the 64-field / 32-KiB hard
limits; parsing stops at the boundary rather than buffering an unbounded header
block for later Broker inspection. Body reading is streaming and stops as soon
as the next chunk would exceed 1 MiB.

## 11. Response Contract

Every final HTTP status, including 4xx and 5xx, resolves successfully as a
`PluginNetworkResponse`. The Host does not reinterpret provider business
errors.

- `status` is the final HTTP status as an integer.
- Header names are lowercase and sorted by ASCII bytes.
- Repeated values remain arrays in received order.
- Hop-by-hop headers, `set-cookie`, `set-cookie2`, and `proxy-authenticate` are
  silently omitted. After that filtering, any remaining invalid/non-UTF-8 field
  value rejects the whole response as `NetworkResponseInvalidError`.
- `body` is strict UTF-8 text. Invalid UTF-8 rejects; replacement decoding is
  forbidden.
- The Host does not parse JSON, retain cookies, cache responses, or persist
  response data.

## 12. Error Contract

The Runtime Promise rejects with an `Error` whose exact `name` is one of:

| Error name | Meaning |
|---|---|
| `InvalidNetworkRequestError` | Invalid URL, method, headers, body, encoding, or request-size limit |
| `PermissionDeniedError` | `network.https` absent, revoked, unsupported, or no exact durable grant |
| `NetworkTargetDeniedError` | Scheme, host, address, port, DNS answer, or redirect target denied |
| `NetworkTimeoutError` | The total 10-second deadline elapsed |
| `NetworkFailureError` | DNS/TLS/connect/write/read failure without a more specific policy error |
| `NetworkResponseTooLargeError` | Response header or body limit exceeded |
| `NetworkResponseInvalidError` | Invalid response framing, content encoding, header value, or UTF-8 body |
| `NetworkLimitExceededError` | Per-context call/concurrency or global concurrency limit exhausted |
| `ExpiredRequestError` | Context replaced, completed, failed, cancelled, disabled, upgraded, or torn down |

The command wire error is the exact object `{ code: PluginNetworkErrorCode }`
with no message or additional field. `PluginNetworkErrorCode` has the exact
camel-case values below and maps one-to-one to JavaScript names:

```text
invalidNetworkRequest   -> InvalidNetworkRequestError
permissionDenied        -> PermissionDeniedError
networkTargetDenied     -> NetworkTargetDeniedError
networkTimeout          -> NetworkTimeoutError
networkFailure          -> NetworkFailureError
networkResponseTooLarge -> NetworkResponseTooLargeError
networkResponseInvalid  -> NetworkResponseInvalidError
networkLimitExceeded    -> NetworkLimitExceededError
expiredRequest          -> ExpiredRequestError
```

The bootstrap accepts only an object with the one exact `code` field. Tauri
deserialization strings, missing/extra fields, unknown codes, malformed/private
errors, and command transport failure all become `NetworkFailureError`. It
never copies a Host message into the JavaScript Error. Errors expose no URL,
path, query, request/response headers, body, resolved address, certificate,
network-library text, Rust stack, or provider payload.

HTTP 4xx/5xx never reject merely because of status. A plugin maps those statuses
to its own user-facing main result.

## 13. Cancellation And Race Semantics

### 13.1 Common Network Authority Gate

`PublicPluginManager` owns one narrow `PluginNetworkAuthorityGate`. It is the
shared linearization seam for network admission, scheduler-current transitions,
generation/enable/fault/grant transitions, registry cancellation, and response
delivery. Network I/O never holds it.

Every network-sensitive Manager transition enters the gate while holding no
scheduler, state, bundle, or registry lock. Inside the gate it uses the fixed
lock order `authority gate -> scheduler -> state/bundle -> request registry`,
performs only bounded in-memory work, publishes the new network-authority
snapshot, and cancels invalidated tokens before release. Existing callers that
currently mutate scheduler/state first must be refactored through this Manager
operation; a later best-effort cancellation scan is insufficient.

Durable I/O remains outside the gate. After a durable outcome is known, the
bounded in-memory publish/close transition enters the gate. `Unknown` and
post-commit publication failure publish a closed network snapshot and cancel
matching calls before the service exposes its terminal failure.

Admission checks authority and registers its cancellation token in one gate
critical section. Lifecycle transitions update authority and cancel in one gate
critical section. Delivery rechecks the same authority snapshot and performs
its terminal registry compare-and-set in one gate critical section. Therefore
there is no interval in which a transition can miss an unregistered call or a
response can commit after revocation/replacement linearizes.

### 13.2 Registry Identity

`PluginNetworkRequestRegistry` keys each admitted call by:

- plugin ID;
- plugin generation;
- activation ID;
- admission epoch;
- request ID from `PluginRequestContext`;
- Host-allocated network call sequence.

The call sequence is non-zero, checked, and never wraps. It is not exposed as a
trusted plugin input.

### 13.3 Cancellation Sources

The Host cancels matching tokens when any of these becomes authoritative:

- A newer submission replaces the current command.
- The command completes or fails.
- Scheduler/context teardown or runtime recovery expires the request.
- Plugin disable or fault-disable.
- Package update replaces generation.
- Plugin uninstall.
- Network permission revocation.
- Runtime WebView destruction or application shutdown.

Scheduler and lifecycle transitions run through the common authority-gate
operation and cancel exact context/generation identities before releasing the
gate. Repeating cancellation is idempotent.

### 13.4 Terminal Linearization

After the transport has read a complete bounded response, delivery enters the
common authority gate and performs:

1. A current scheduler-context recheck.
2. Active package/generation/enabled/fault recheck.
3. Current Manifest permission and exact durable host-grant recheck.
4. Registry terminal compare-and-set from `inFlight` to `delivered` against the
   same authority snapshot.

If any recheck fails or cancellation won the terminal compare-and-set, the
command returns `ExpiredRequestError` and discards the complete response. If
delivery wins first, a later replacement may expire the command normally but
does not retroactively mutate the already-resolved Promise.

Timeout, policy denial after a redirect, transport failure, oversized response,
and invalid response each release concurrency exactly once and terminally
remove the call. Cancellation drops the transport future promptly; cleanup does
not wait for a socket timeout before disabling/uninstalling can finish.

No lifecycle path relies only on polling `context_status`; explicit token
cancellation is required to abort I/O.

## 14. Logging And Privacy

Production network logging is structured, bounded, and contains only:

- plugin ID;
- method;
- normalized exact hostname;
- terminal result category;
- HTTP status when a response was received;
- bounded elapsed milliseconds.

It never records full URL, path, query, fragments, any header name/value pair,
request/response bodies, form fields, JSON keys, authorization values,
signatures, resolved IP addresses, certificate details, or raw dependency
errors. Debug builds obey the same redaction. User-facing errors use fixed
localized text mapped from fixed error names.

## 15. Module Ownership

The implementation keeps these modules deep and ownership-local:

- `manifest`: public shape, hostname grammar, cross-field/version/platform
  validation, and canonical host ordering.
- `state`: durable network grant invariant and atomic grant/revoke transitions.
- `PluginHttpsBroker`: the sole external seam for admission, execution,
  terminal response, and cancellation. Callers do not implement URL, DNS,
  redirect, header, or size policy.
- `NativeHttpsTransport`: production adapter below the broker's internal
  transport seam.
- `PluginNetworkRequestRegistry`: quotas, call identities, cancellation tokens,
  and terminal compare-and-set.
- `runtime`: bootstrap DTO translation and fixed public errors, not HTTP policy.
- `activation`: package/runtime identity revalidation and orchestration, not
  transport implementation.
- `commands`: label-derived thin adapters only.

The deletion test for `PluginHttpsBroker` is intentional: deleting it would
force URL policy, authorization, DNS safety, redirects, limits, cancellation,
and redaction into every caller. Its single request/cancel interface keeps that
complexity local.

## 16. Frontend Behavior

- Prepared install/update UI displays exact hosts without links and without
  resolving or contacting them.
- Confirm is the only grant action. Cancel preserves the previous installed
  version and authorization.
- Installed plugin rows show sorted exact hosts and a network access switch.
- Reauthorization uses a confirmation surface listing the current hosts.
- Revocation disables the switch immediately while pending, reports a fixed
  local failure on error, and refreshes inventory from Host authority.
- Install, update, revoke, and regrant restore focus to the initiating control
  using the existing settings focus pattern.
- No frontend code handles credentials, provider requests, DNS, redirects, or
  response data.

## 17. Validation And SDK Synchronization

The following public artifacts remain byte/behavior synchronized:

- Rust Manifest DTO and generated canonical Schema.
- `docs/plugin-sdk/uipilot-plugin-v1.schema.json`.
- Plugin CLI bundled Schema and standalone validator.
- Plugin CLI Manifest types and semantic validation.
- `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`.
- Public contract and developer guide.

CLI validation reports illegal network shapes as existing bounded Manifest
Schema/semantic issues and reports `network.https` as unsupported for macOS.
The packed CLI remains a local validator with no network capability; adding a
network Manifest contract must not add Node network modules or calls to the CLI
artifact.

SDK documentation states that API keys embedded in plugin code are visible to
anyone who can inspect the package and are suitable only for local temporary
testing. It points production credential handling to the future Host secret
consumption contract without claiming that contract exists.

## 18. Testing

### 18.1 Manifest, Schema, And CLI

- Legal one-host and eight-host Windows Manifests.
- Old Manifest without network remains legal.
- Missing permission/field counterpart, empty list, nine hosts, duplicate,
  unknown field, wrong type, wildcard, uppercase, `xn--`, raw Unicode, IP
  literal, single label, localhost/local suffix, port, path, query, and bad
  label rejection.
- `minimumHostVersion` 0.3.1 rejection and 0.3.2 acceptance.
- Windows acceptance and macOS unsupported-permission rejection.
- Shared Rust/CLI fixture corpus produces identical outcomes and sorted hosts.
- CLI packed artifact still passes its no-network-capability audit and smoke
  test.

### 18.2 Authorization State

- Fresh install requires consent and atomically stores exact grant.
- Added host requires consent and failed/cancelled update preserves old version.
- Removed host narrows authority without new network consent.
- Revoked state survives restart and a host-removing update.
- Regrant derives current hosts from active package.
- Invalid legacy/corrupt permission-without-host-grant loads fail closed.
- Disable, fault-disable, update, uninstall, revoke, and regrant revision/order
  invariants.

### 18.3 Broker Pure Policy

- GET/POST and all three body encodings with exact bytes/content type.
- URL, header, body, call, concurrency, redirect, timeout, header-size,
  body-size, strict UTF-8, and content-encoding limits.
- Protected header rejection and lowercase duplicate detection.
- 4xx/5xx successful response contract.
- Cross-host redirect rejects before forwarding headers or body.
- Every redirect revalidates authority and DNS.
- All fixed error names and redacted error/log payloads.
- Counter near-exhaustion fails closed without wrap.

### 18.4 DNS/TLS Transport

- Deterministic resolver fixtures for empty, mixed public/private, loopback,
  link-local, multicast, unspecified, reserved, IPv4, and IPv6 answers.
- Prove a mixed answer rejects the entire host.
- IPv4-mapped IPv6 normalization and every fixed denied CIDR edge.
- Prove the connector uses only the validated frozen address set while TLS uses
  the original hostname.
- TLS hostname, Windows trust, and expiry failures; HTTP downgrade rejection;
  proxy environment ignored; no connection reuse across calls/plugins.
- DNS timeout/cancellation returns promptly, and repeated abandoned lookups stay
  within the fixed resolver worker/queue bound.
- Local TLS responses with one oversized header, too many fields, and a
  progressively oversized header block prove the native parser stops at its
  configured boundary before Broker-level buffering.
- A test-only local TLS harness exercises the native adapter with injected test
  trust/address policy. Production policy still rejects loopback.

No release test contacts the public Internet or a translation provider.

### 18.5 Cancellation And Concurrency

- Replacement, completion, failure, timeout, revoke, disable, fault-disable,
  update, uninstall, recovery, WebView teardown, and shutdown cancel exact
  calls.
- Stale cancellation cannot affect a newer generation/context/call sequence.
- Admission-check vs lifecycle-transition proves the authority gate cannot miss
  a call between validation and registration.
- Delivery-recheck vs revoke/replacement proves terminal CAS cannot resolve
  after the authority transition linearizes.
- Response-vs-cancel terminal compare-and-set covers both winners.
- Cancellation releases per-context/global slots exactly once.
- Two calls in one context may run; the third rejects immediately.
- Sixteen Host-wide calls may run; the seventeenth rejects immediately.
- No lock is held while a blocking test adapter is pending.

### 18.6 Runtime, Commands, Capabilities, And Frontend

- Runtime bootstrap exposes only the documented `network.request` shape to a
  declaring plugin and no network property to an undeclared plugin.
- Input is snapshotted/frozen; arity and unknown fields fail closed.
- Runtime-only capability and exact caller-label/context derivation.
- Host error codes map to exact JavaScript Error names without private detail.
- Prepare/update host lists, added-host indication, consent, cancellation,
  revoke/regrant confirmation, inventory refresh, fixed failures, and focus
  restoration.
- A non-plugin SDK compile fixture proves GET, POST, JSON, text, form, response,
  and error-narrowing types. No example/third-party plugin is modified.

## 19. Manual Host Acceptance

Manual acceptance uses an operator-controlled public HTTPS endpoint that
resolves only to addresses accepted by the fixed predicate, plus a synthetic
development package maintained outside committed example plugins:

1. Install a package that declares one exact test host; confirmation displays
   that host and grants it.
2. A Runtime command performs GET and POST and receives status, safe headers,
   and UTF-8 body as a main result.
3. Undeclared host, HTTP, IP literal, localhost/private resolution, protected
   header, cross-host credential redirect, timeout, and oversized response
   produce the fixed errors without hiding or crashing the launcher.
4. Replace the command while a response is pending; the Host aborts the socket
   and publishes no stale result.
5. Revoke network access while a response is pending; the call aborts before
   revocation resolves, later calls return `PermissionDeniedError`, and regrant
   lists the same current hosts.
6. Update with an added host; old version continues until explicit consent.
7. Disable and uninstall with pending calls; both complete without waiting for
   the network deadline and no response reaches Runtime afterward.

The user controls all manual keyboard/mouse actions. Automated tests do not
control user input or contact real third-party services.

## 20. Acceptance Criteria

- Host version and public artifacts consistently expose optional network
  support at `0.3.2` / API v1.
- Network is denied by default and impossible without exact Manifest,
  installed generation, durable grant, current Runtime context, and Windows
  target agreement.
- Install/update and settings surfaces show exact sorted host authority and
  implement atomic consent, revoke, and regrant.
- Runtime performs bounded Host-managed GET/POST JSON/text/form requests while
  WebView CSP remains network-closed.
- DNS/address/TLS/redirect/header/body/response policy matches this document.
- HTTP errors resolve by status; policy/transport/lifecycle failures reject with
  exact redacted names.
- Every request terminates on timeout or lifecycle cancellation, and stale
  responses cannot reach plugin logic.
- Plugin disable, fault-disable, update, uninstall, permission revoke, recovery,
  and teardown invalidate authority immediately.
- CLI itself remains network-incapable and validates the new Manifest contract.
- Relevant Rust, frontend, Schema, CLI artifact, and SDK type tests pass.
- No translation provider, plugin business logic, credentials, or plugin
  package changes are included.

## 21. Deferred Secret Consumption

Public plugin settings already permit secret configuration and expose only
`isSecretConfigured` to Runtime. This MVP does not add a way to read or inject a
secret into a network request. A later design must decide whether the Host
performs named secret substitution into protected headers/body fields or grants
a narrower opaque signing interface. It must preserve non-exportability,
redacted errors/logs, package-generation ownership, revocation, and uninstall
cleanup. That design is independent from, and cannot weaken, the network target
and lifecycle policy frozen here.
