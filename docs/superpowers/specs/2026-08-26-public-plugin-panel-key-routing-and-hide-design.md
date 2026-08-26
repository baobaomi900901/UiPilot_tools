# Public Plugin Panel Key Routing And Hide Design

**Date:** 2026-08-26  
**Status:** Draft — awaiting review  
**Related:**  
`docs/superpowers/specs/2026-08-24-public-plugin-panel-mode-design.md`,  
`docs/superpowers/specs/2026-08-25-public-plugin-panel-focus-host-input-design.md`,  
`docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`

## 1. Goal

Extend the host-owned public `panel` contract so a live panel session can:

1. Receive a **small, declared** set of host-originated keys while the tagged
   argument input owns keyboard focus (ArrowDown / ArrowUp / Ctrl+N / Meta+N).
2. Request launcher hide and session discard from panel content
   (`requestHide()`).
3. Have **Escape** in the panel child WebView match Escape in the tagged
   argument input (hide + discard), subject to modal / `preventDefault`
   arbitration.
4. On hide, **best-effort** restore the foreground window that owned focus when
   UiPilot was shown, without failing an already-successful hide.

Host remains DOM-agnostic: it never names plugin widgets such as “list” or
“editor”, never queries plugin DOM, and never synthesizes DOM `KeyboardEvent`s
into the child WebView.

## 2. Non-goals

- Implementing or changing `com.uipilot.notes` (or any other plugin) in this
  design round.
- Live argument streaming or ordinary character forwarding while typing in the
  tagged input (Enter → submit → `onUpdate.input` remains the only argument
  path).
- Arbitrary key listening, global hotkeys inside panel content, or a general
  cross-WebView RPC bus.
- `focusPanelContent('list')` or any Host API that encodes plugin DOM roles.
- Guaranteeing Windows `SetForegroundWindow` success in every environment.
- Letting panel content call `hide_launcher`, Tauri `invoke`, or main-DOM APIs
  directly.
- Changing Escape / hide semantics for non-panel launcher modes beyond sharing
  the existing hide pipeline and foreground-restore capture.

## 3. Public API

### 3.1 TypeScript surface

```ts
export type PluginPanelHostKey =
  | 'ArrowDown'
  | 'ArrowUp'
  | 'n'

export interface PluginPanelHostKeyEvent {
  key: PluginPanelHostKey
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
  /** Canonical decimal session epoch for the delivery that produced this event. */
  sessionEpoch: U64Decimal
  /** Monotonic route sequence for this session; latest-wins / stale discard. */
  routeSequence: U64Decimal
}

export interface UiPilotPluginPanelApiV1 {
  onUpdate(
    handler: (update: Readonly<PluginPanelUpdate>) => void | Promise<void>,
  ): () => void
  onHostKey(
    handler: (event: Readonly<PluginPanelHostKeyEvent>) => void | Promise<void>,
  ): () => void
  focusHostInput(): Promise<void>
  requestHide(): Promise<void>
  readonly storage: Readonly<UiPilotPluginPanelStorageApiV1>
}
```

Rules for `onHostKey`:

- Exactly **one** handler may be registered per content document lifetime
  (same posture as `onUpdate`: second registration throws `TypeError`).
- Content that declares a non-empty `panel.hostKeys` **must** register
  `onHostKey` before content-ready is accepted (see §5).
- Content that declares empty / omitted `hostKeys` **must not** register
  `onHostKey` (registration throws `TypeError`) so unused plugins stay
  unchanged.
- The handler receives a **frozen DTO** only. Host must not dispatch synthetic
  DOM `keydown` / `keyup` into the child document (`isTrusted` synthesis is
  out of contract).
- Handler return / thrown errors do not retry delivery; host logging is
  optional and must not surface plugin internals to other plugins.

### 3.2 `requestHide()`

```ts
window.uipilotPluginPanel.requestHide(): Promise<void>
```

- No arguments; bootstrap privately supplies `sessionEpoch`.
- Authorized only for the live panel-content caller matching that epoch.
- Success runs the **shared** launcher hide pipeline (same teardown class as
  Escape / blur-hide / tray hide for a live panel): clear result registry as
  today, hide the main window, destroy the panel child WebView, clear tag /
  session UI, bump/drop session so in-flight work cannot revive it.
- Does not modify clipboard, argument text, or plugin storage.
- Idempotent for a still-current session that is already hiding / torn down
  after a successful hide commit (second call resolves as no-op).

### 3.3 Manifest field

```json
{
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Ctrl+N"]
  }
}
```

`PublicPanelV1` gains optional `hostKeys`:

```ts
hostKeys?: ReadonlyArray<PanelHostKeyDeclaration>
```

Frozen declaration grammar (string enum, deny unknown):

| Declaration | Matches when tagged input receives |
|---|---|
| `"ArrowDown"` | `key === 'ArrowDown'`, no Ctrl/Meta/Alt required; Shift ignored for matching |
| `"ArrowUp"` | `key === 'ArrowUp'`, same modifier rule |
| `"Ctrl+N"` | `key` is `n`/`N`, `ctrlKey === true`, `metaKey === false`, `altKey === false` |

Matching rules:

- `hostKeys` omitted or `[]` → Host intercepts **no** keys for panel routing
  (today’s behavior).
- Declarations are unique; duplicates are a package validation error.
- Only the table above is valid. No `"Meta+N"` token: on platforms where the
  host maps Command to `metaKey`, **`"Ctrl+N"` also matches**
  `metaKey === true && ctrlKey === false && key n/N && !altKey` so one
  declaration covers Windows Ctrl and macOS Command without a second token.
  Delivery DTO still reports the real `ctrlKey` / `metaKey` bits.
- Ordinary character keys, IME composition keys, and undeclared chords are
  never routed.
- Shift+Arrow and Shift+Ctrl+N are **not** distinct declarations; if the base
  declaration is present, Host still routes and sets `shiftKey` in the DTO.
  Plugins that dislike Shift may ignore the event.

Schema, Rust `PublicPanelV1`, CLI package policy, SDK d.ts, and developer
docs must stay aligned.

## 4. Host version and compatibility

| Field | Decision |
|---|---|
| Host release | Bump to **`0.3.1`** (Cargo / app version / CLI `PLUGIN_CLI_HOST_VERSION` / `PublicPluginHost::current`). |
| Plugin `minimumHostVersion` | Plugins that set non-empty `hostKeys`, or that call `onHostKey` / `requestHide` in shipped content, **must** declare `minimumHostVersion >= "0.3.1"`. |
| `schemaVersion` | Remains **`1`**. Additive optional `panel.hostKeys`; unknown keys still denied by `deny_unknown_fields`. |
| `apiVersion` | Remains **`1`**. New panel bridge methods are additive for hosts that advertise 0.3.1+; old hosts reject packages that require 0.3.1 via the existing minimum-host gate before install/activation. |

Compatibility argument:

- Old panel packages without `hostKeys` validate and run on 0.3.1 with
  **identical** keyboard and hide behavior aside from shared Escape-in-content
  handling (see §8), which is a host HCI fix required by the 2026-08-24 panel
  design §3.5 and applies to all panel sessions.
- New APIs are unused unless the plugin opts in via manifest and/or calls them.
- Keeping `apiVersion: 1` avoids forcing a parallel runtime module ABI; the
  host version floor carries the capability boundary.

## 5. Ready gate for `hostKeys`

Content ready handshake (existing `plugin_panel_content_ready`) gains an
additional host-side check when `hostKeys` is non-empty:

1. Bootstrap exposes `onHostKey` before ready may succeed.
2. Content must call `onHostKey(handler)` exactly once **before** invoking
   ready (same ordering expectation as `onUpdate` today).
3. If ready arrives with non-empty declared `hostKeys` and no handler
   installed → ready fails → session rolls back (destroy content, clear tag,
   surface existing runtime-not-ready / open failure path).
4. If `hostKeys` is empty/omitted and content calls `onHostKey` → throw in
   bootstrap; ready must not succeed with a forbidden handler.

This prevents “declared routing with a deaf panel” and keeps opt-in explicit.

## 6. Key routing ownership and sequence

### 6.1 Route ticket

Under the panel controller lock, Host maintains at most one current
`HostKeyRouteTicket` per live session:

```text
HostKeyRouteTicket {
  sessionEpoch: u64,
  routeSequence: u64,   // monotonic per session, non-zero
  declaration: PanelHostKeyDeclaration,
  phase: Prepared | NativeFocused | Delivered | Cancelled,
}
```

Latest-wins: a newer route supersedes an older undelivered ticket; the older
path becomes stale and must not deliver.

### 6.2 Tagged-input intercept order

When the tagged argument `<input>` receives a keydown that matches a declared
`hostKeys` entry and IME `isComposing === false`:

1. Confirm live panel UI/`sessionEpoch` still matches the mounted session.
2. `preventDefault()` (and `stopPropagation()` as needed) so the suffix input
   does not insert text or move caret for that chord.
3. Allocate / install a route ticket `{ sessionEpoch, routeSequence++ }` bound
   to the matched declaration.
4. Move **native** keyboard focus to the current panel child WebView (Host
   focuses the WebView itself only—not a plugin CSS selector).
5. Re-validate ticket + session still current after native focus returns.
6. Deliver `PluginPanelHostKeyEvent` through the panel bridge private update
   channel (bootstrap invokes the registered `onHostKey` handler with a frozen
   DTO). Delivery must not use synthesized DOM keyboard events.
7. Plugin handler decides focus inside its own document (list, dialog, etc.).

Stale epoch, teardown, session replacement, superseded `routeSequence`, or
native-focus failure after ticket install → **silent discard** (no handler
call). Failures that occur before hide/teardown must not leave a reusable
blur-suppression token (same discipline as `focusHostInput`).

Enter while focus is on the tagged input remains **submit**, never a host-key
route, even if a plugin wished otherwise.

### 6.3 Key routing table (tagged argument input)

| Key / chord | Declared? | Host action |
|---|---|---|
| ArrowDown / ArrowUp | yes | Route per §6.2 |
| ArrowDown / ArrowUp | no | No intercept; default input behavior (typically no-op in empty/single-line) |
| Ctrl+N / Meta+N (as §3.3) | yes | Route per §6.2 |
| Ctrl+N / Meta+N | no | No intercept |
| Enter | n/a | Submit panel argument (unchanged) |
| Escape | n/a | Hide + discard (unchanged on main) |
| Backspace at caret 0 | n/a | Close tag (unchanged) |
| Other keys / IME composing | n/a | Never routed |
| Ordinary characters | n/a | Never routed |

### 6.4 Why not synthetic DOM keydown

Synthetic events are `isTrusted: false`, inconsistently handled by browsers and
assistive tech, and couple Host to plugin key-listener style. An explicit
`onHostKey` DTO is deterministic, testable, and versioned.

## 7. `requestHide` authorization and outcomes

Authorization order (fail closed):

1. Capability: only `plugin-panel-content-*`.
2. Parse panel-content label → plugin id.
3. Compare label, plugin id, and bootstrap `sessionEpoch` to live
   `PanelSessionIdentity`.
4. Mismatch after capability pattern match → **resolve Ok no-op** (do not leak
   whether another session exists)—same privacy posture as
   `focusHostInput` stale callers.
5. Callers outside the capability pattern → capability denial.

Outcome table:

| Condition | Result |
|---|---|
| Live authorized session, hide pipeline succeeds | Resolve after hide committed |
| No live session / wrong epoch / stale content caller in-pattern | Resolve, no-op |
| Hide pipeline fails **before** hide is committed | Reject `windowFailed`; session remains unless an independent teardown raced |
| Hide committed; foreground restore fails | **Still resolve**; restore is best-effort (§9) |
| Concurrent second `requestHide` after commit | Resolve, no-op |
| Main / find / plugin-window caller | Capability rejection |

Hide commit boundary: once the shared hide function has successfully hidden the
main window (or observed it already hidden as part of the same linearized hide
owner) and panel teardown for this epoch has been requested, the Promise must
not reject solely because foreground restore failed.

Do not teardown the panel **before** hide ownership is acquired if that would
leave the user with a torn panel while the window stays visible; follow the
existing single hide-owner pattern in launcher-core / `hide_launcher`.

## 8. Escape in panel content

Bootstrap installs a capture-or-bubble listener on the content document for
`keydown` Escape with frozen arbitration:

1. If `event.isComposing === true` → ignore.
2. If `document.querySelector('dialog[open]')` is non-null at keydown time →
   **Host does not hide**. Content / UA closes or keeps the dialog.
3. After the event finishes propagating through content handlers, if
   `event.defaultPrevented === true` → **Host does not hide** (plugin opted
   out synchronously via `preventDefault()`).
4. Otherwise Host invokes the **same internal hide path** as `requestHide()`
   (shared pipeline; not a second semantics).
5. Repeated Escape while hide is in flight is idempotent.

Notes:

- Async `preventDefault` after an `await` cannot cancel hide; only synchronous
  prevention during the keydown turn counts.
- Multiple open dialogs: any `dialog[open]` suppresses Host hide.
- Closing the last dialog and pressing Escape again hides.
- Teardown races: Escape after epoch replacement is a no-op.

This closes the HCI gap in 2026-08-24 §3.5 where Escape was only wired on the
main WebView React tree.

## 9. Windows foreground restore (best-effort, shared)

### 9.1 Capture

On each **explicit show** of the main launcher (hotkey, tray, or other
host-owned show entry—not accidental focus), native code captures
`GetForegroundWindow()` **before** UiPilot becomes foreground, bound to that
show’s invocation / generation id.

Do **not** store:

- UiPilot main / find / panel-content HWNDs
- NULL / invalid handles
- Handles that fail a recoverability check at capture time (optional early drop)

A newer show generation invalidates older captures.

### 9.2 Restore

After a successful hide commit and panel teardown for a hide that should return
to the prior app (Escape, `requestHide`, blur-hide, tray hide—**shared**
lifecycle, not Notes-only):

1. Load capture for the show generation being dismissed.
2. Re-check: handle still valid, window visible (or iconically acceptable per
   existing Win32 norms chosen in implementation), not owned by UiPilot.
3. Call normal foreground APIs only (`SetForegroundWindow` / related allow
   calls already used elsewhere). **No** synthesized keyboard or mouse input.
4. If the system rejects foreground switch → record a downgraded result
   (log/metric); **do not retry**; **do not** affect hide Promise success.

### 9.3 Contract boundary

Foreground restore is **best-effort**. Design and docs must state that some
Windows focus policies can deny restoration; hide success is independent.
Manual acceptance includes the common Notepad → hotkey → hide path but does
not claim universality.

## 10. Event timing diagram (host key)

```text
Tagged input keydown (declared)
  → launcher preventDefault
  → install HostKeyRouteTicket(epoch, seq)
  → native focus panel child WebView
  → re-check ticket current
  → bootstrap onHostKey(DTO)
  → plugin focuses its own controls / opens dialogs

Meanwhile: teardown / newer seq / epoch bump
  → ticket Cancelled
  → no DTO delivery
```

## 11. Relationship to `focusHostInput`

| Direction | API |
|---|---|
| Panel content → tagged input | `focusHostInput()` (2026-08-25) |
| Tagged input → panel content (declared keys only) | `onHostKey` + native WebView focus (this design) |
| Panel content → hide launcher | `requestHide()` / content Escape (this design) |

`focusHostInput` remains unchanged. Host-key routing must compose with existing
focus-revision / blur-hide tickets so panel↔main focus transfers do not spuriously
hide during routing.

## 12. Testing matrix (frozen)

### Manifest / package

- Omitting `hostKeys` validates; Host intercepts nothing new for routing.
- Empty `hostKeys: []` same as omit.
- Valid `["ArrowDown","ArrowUp","Ctrl+N"]` accepted.
- Unknown declaration, duplicates, or `hostKeys` on non-panel packages rejected.
- CLI / schema / Rust manifest / d.ts agree.

### Ready gate

- Non-empty `hostKeys` without `onHostKey` before ready → ready failure + rollback.
- Empty `hostKeys` + `onHostKey` registration → bootstrap error; ready fails.
- Non-empty + single `onHostKey` + `onUpdate` → ready succeeds.

### Routing

- Undeclared ArrowDown/Ctrl+N not intercepted.
- Declared ArrowDown: focus moves to child WebView, then exactly one DTO with
  matching `sessionEpoch` / increasing `routeSequence`.
- Declared Ctrl+N and Meta+N produce precise modifier bits; no double delivery
  for one physical keydown.
- IME composing keydown never routes.
- Ordinary characters never route.
- Stale epoch, teardown, replacement, superseded route sequence → no delivery.
- Enter still submits; never routed as host key.

### `requestHide` / Escape

- Caller / session guards per §7.
- Successful hide clears tag, session, and child WebView.
- Hide failure before commit does not teardown early.
- Foreground restore failure does not reject hide.
- Escape + `dialog[open]` does not hide.
- Escape + sync `preventDefault` does not hide.
- Escape + composing ignored.
- Repeated Escape / concurrent `requestHide` idempotent.
- Capability only `plugin-panel-content-*` for content commands; main-only for
  any ack/private main events if introduced symmetrically.

### Compatibility

- Pre-0.3.1 host rejects packages with `minimumHostVersion: 0.3.1`.
- Old panel demos without `hostKeys` still stage and run.

### Manual

- User verifies Notepad (or equivalent) foreground restore after hide; agents
  must not drive the user’s mouse/keyboard.

## 13. Documentation and demo updates (implementation phase)

When coding (not this draft commit):

- Update `uipilot-plugin-api-v1.d.ts`, `uipilot-plugin-v1.schema.json`,
  `public-plugin-v1.md`, developer guide.
- Optionally extend `com.uipilot.demo-panel` with empty or minimal `hostKeys`
  contract tests—**not** Notes business.
- Host version `0.3.1` everywhere the floor is pinned.

## 14. Explicitly out of scope for implementation follow-ups of this design

- Notes list/editor behavior, storage, or Ctrl+F beyond existing
  `focusHostInput`.
- Forwarding arbitrary shortcuts or building a plugin-defined keybinding UI in
  Host.
- Guaranteeing foreground restore under every Windows focus-stealing rule.
- Changing R1 submit semantics or adding live search streaming.

## 15. Open points resolved by this draft

| Topic | Decision |
|---|---|
| `focusPanelContent('list')` | Rejected |
| Synthetic DOM keydown | Rejected; use `onHostKey` DTO |
| `hostKeys` opt-in | Required; default empty |
| Meta vs Ctrl for New | One declaration `Ctrl+N` matches Ctrl **or** Meta |
| `requestHide` vs restore failure | Hide success independent of restore |
| Escape vs dialog | Any `dialog[open]` blocks Host hide |
| Version | Host `0.3.1`; `schemaVersion`/`apiVersion` stay 1 |

## 16. Approval

This document is the source of truth for subsequent implementation planning.
Do not implement until Status is set to **Approved** after review.
