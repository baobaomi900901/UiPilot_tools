# Public Plugin Panel Key Routing And Hide Design

**Date:** 2026-08-26  
**Status:** Draft — awaiting review (revision 2; closes review P1/P2)  
**Related:**  
`docs/superpowers/specs/2026-08-24-public-plugin-panel-mode-design.md`,  
`docs/superpowers/specs/2026-08-25-public-plugin-panel-focus-host-input-design.md`,  
`docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`

## 1. Goal

Extend the host-owned public `panel` contract so a live panel session can:

1. Receive a **small, declared** set of host-originated keys while the tagged
   argument input owns keyboard focus (ArrowDown / ArrowUp / Primary+N).
2. Request launcher hide and session discard from panel content
   (`requestHide()`).
3. Have **Escape** in the panel child WebView match Escape in the tagged
   argument input (hide + discard), subject to modal / `preventDefault`
   arbitration.
4. On **explicit return hides**, **best-effort** restore the foreground window
   captured when UiPilot was shown, without failing an already-successful hide
   admission.

Host remains DOM-agnostic for plugin widgets: it never names “list” or
“editor”, never queries plugin-authored selectors, and never synthesizes DOM
`KeyboardEvent`s into the child WebView. The **sole** Host DOM probe in content
is the standard-element check `dialog[open]` used only for Escape arbitration
(§8)—an explicit exception, not a general DOM API.

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
- Restoring a prior foreground app after **blur-hide** or after hide caused by
  launching another app from a result (would steal the user’s new focus).

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
  /** Canonical decimal session epoch for this delivery. */
  sessionEpoch: U64Decimal
  /** Monotonic per-session route sequence; at-most-once + serial ack. */
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
  `onHostKey` before content-ready is accepted (§5).
- Content that declares empty / omitted `hostKeys` **must not** register
  `onHostKey` (registration throws `TypeError`).
- The handler receives a **frozen DTO** only—no synthetic DOM keydown.
- Unsubscribe behavior is frozen in §6.5 (not a silent detach).

### 3.2 `requestHide()` — admission Promise (not post-teardown settlement)

```ts
window.uipilotPluginPanel.requestHide(): Promise<void>
```

**Problem this freezes:** resolving “after hide + teardown” is not observable:
teardown destroys the child WebView that owns the Promise microtask queue.

**Frozen contract:**

- `requestHide()` is a **hide-admission** Promise.
- It resolves when the Host has **accepted and linearized** a hide request for
  the caller’s live `sessionEpoch` into the shared launcher hide owner (same
  owner as Escape on main)—**before** destroying the panel content WebView.
- After resolve, hide + teardown + optional foreground restore continue as a
  **terminal host operation**. The plugin must treat the call as fire-and-forget
  after admission; it must not await further settlement from the dying document.
- Reject `windowFailed` only if admission fails (cannot acquire hide owner /
  session already racing to a conflicting visible state) **before** the
  terminal path starts.
- Stale / wrong epoch / unauthorized-in-pattern caller → resolve no-op without
  starting hide.
- Does not modify clipboard, argument text, or plugin storage.
- Idempotent: a second admission after the first accepted hide for that epoch
  resolves no-op.

Internal Escape-in-content uses the same admission + terminal path; it does not
expose a Promise to plugin code.

### 3.3 Manifest `hostKeys`

```json
{
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Primary+N"]
  }
}
```

`PublicPanelV1` gains optional `hostKeys: PanelHostKeyDeclaration[]`.

Frozen declaration grammar (deny unknown; unique; max length small, e.g. ≤ 8):

| Declaration | Matches tagged-input keydown when |
|---|---|
| `"ArrowDown"` | `key === 'ArrowDown'`, `ctrlKey === false`, `metaKey === false`, `altKey === false`, **`shiftKey === false`** |
| `"ArrowUp"` | same for `ArrowUp` |
| `"Primary+N"` | `key` is `n`/`N`, `altKey === false`, **`shiftKey === false`**, and exactly one of (`ctrlKey && !metaKey`) or (`metaKey && !ctrlKey`) |

Matching rules:

- Omitted or `[]` → Host intercepts **no** keys for panel routing.
- Extended chords (e.g. Shift+Arrow, Ctrl+Shift+N) require **future explicit
  declarations**; this design does **not** route them under the base tokens.
- Ordinary characters, IME composing (`isComposing`), and undeclared chords are
  never routed.
- Delivery DTO reports the real modifier bits of the physical keydown.

`"Primary+N"` is the platform-primary accelerator for New (Ctrl on Windows,
Command on macOS). Separate `"Ctrl+N"` / `"Meta+N"` tokens are **not** in v1;
they may be added later without redefining `"Primary+N"`.

### 3.4 Launcher data path for `hostKeys` (frozen)

Tagged input cannot `preventDefault` correctly unless the main WebView knows the
live session’s declarations. Path:

1. **Rust** reads `hostKeys` from the installed/prepared manifest when opening
   or submitting a panel session (bound to that plugin generation).
2. **`PluginPanelCommandResult`** (open + submit responses) gains:
   ```ts
   export interface PluginPanelCommandResult {
     sessionEpoch: U64Decimal
     pluginId: string
     commandLabel: string
     /** Canonical sorted copy of manifest panel.hostKeys; empty array if none. */
     hostKeys: readonly PanelHostKeyDeclaration[]
   }
   ```
   Exact key set for parsers:  
   `['commandLabel', 'hostKeys', 'pluginId', 'sessionEpoch']`  
   (`hostKeys` always present; use `[]` when undeclared).
3. **TS parser** (`parsePluginPanelCommandResult`) validates each declaration
   against the frozen enum, rejects unknown strings, rejects duplicates, and
   rejects non-arrays.
4. **`launcher-core`** stores `model.panel.hostKeys` only when installing /
   refreshing panel UI for a matching `sessionEpoch` + `pluginId`. Stale
   command results (epoch ≠ live UI epoch, or pluginId mismatch) must not
   overwrite `hostKeys`.
5. **`panelKeyDown`** matches keydown against `snapshot`/`model.panel.hostKeys`
   for the **current** epoch only. If `hostKeys` is empty, behavior matches
   pre-0.3.1 intercept set (Enter / Escape / Backspace-at-0 only).

Submit responses must carry the same `hostKeys` snapshot as open (manifest
immutable for a generation) so UI refresh cannot drop declarations.

Private events that disable routing (§6.5) must clear or replace launcher
`hostKeys` under the same epoch guard.

## 4. Host version and compatibility

| Field | Decision |
|---|---|
| Host release | **`0.3.1`** |
| Plugin `minimumHostVersion` | Non-empty `hostKeys`, or shipped use of `onHostKey` / `requestHide`, requires `>= 0.3.1` |
| `schemaVersion` / `apiVersion` | Remain **`1`** (additive optional field + bridge methods; floor via host version) |

Old packages without `hostKeys` keep prior routing. Content Escape hide (§8) is
a host HCI fix for all panels on 0.3.1+ (2026-08-24 §3.5).

## 5. Ready gate for `hostKeys`

When `hostKeys` is non-empty:

1. Bootstrap exposes `onHostKey` before ready may succeed.
2. Content must register exactly one `onHostKey` handler **before** ready.
3. Ready without handler → fail + session rollback.
4. Empty/omitted `hostKeys` + `onHostKey` call → bootstrap `TypeError`; ready
   must fail if somehow invoked.

Bootstrap privately notifies Rust that the host-key receiver is **armed** for
this epoch (part of ready success). Launcher may only intercept declared keys
after the panel UI epoch is live **and** (if `hostKeys` non-empty) content
ready has armed the receiver.

## 6. Key routing: serial at-most-once delivery

### 6.1 Why not latest-wins

ArrowDown is not idempotent focus-transfer: dropping an intermediate press
loses a navigation step. Host-key routes are therefore **queued and serial**,
not focus-style latest-wins.

### 6.2 Route ticket and queue

Per live session:

```text
HostKeyRouteState {
  sessionEpoch: u64
  nextSequence: u64          // starts at 1; see exhaustion §6.6
  receiverArmed: bool
  queue: VecDeque<HostKeyRouteTicket>  // bounded, see below
  inFlight: Option<HostKeyRouteTicket>
}

HostKeyRouteTicket {
  sessionEpoch: u64
  routeSequence: u64
  declaration: PanelHostKeyDeclaration
  phase: Prepared | NativeFocused | DeliveredAwaitingAck | Accomplished | Cancelled
}
```

Bounds:

- Max queue depth **8**. If full when a new declared key arrives:  
  `preventDefault` still runs (key must not type into the suffix), but the new
  press is **dropped** (at-most-once; no coalescing into a single “jump”).
  Optional host metric; no plugin-visible error.
- Only **one** `inFlight` delivery at a time.

### 6.3 Delivery + ack protocol

Tagged-input keydown matching a declaration (`isComposing === false`):

1. Confirm live UI epoch / identity and `receiverArmed`.
2. `preventDefault` (+ stopPropagation as needed).
3. Enqueue ticket with `routeSequence = nextSequence++` (if queue not full).
4. If no `inFlight`, start pump (§6.4).

Pump for head / `inFlight`:

1. Phase `Prepared` → native-focus the panel child WebView (WebView only).
2. Re-validate epoch + ticket still head/`inFlight`.
3. Deliver frozen `PluginPanelHostKeyEvent` via bootstrap → `onHostKey`.
4. Phase `DeliveredAwaitingAck`.
5. Bootstrap **must** ack after the handler settles:
   - Sync return → ack immediately.
   - Returned Promise → ack on fulfill **or** reject (errors still ack;
     no retry).
   - Ack command carries `{ sessionEpoch, routeSequence }` (main or
     panel-content capability as implementation chooses; prefer
     panel-content private command parallel to storage, with bootstrap
     holding invoke).
6. Ack timeout: **2 seconds**. On timeout Host acks as cancelled for that
   sequence (clears `inFlight`) and continues the queue—**does not** teardown
   solely for handler hang.
7. Matching ack → `Accomplished`, clear `inFlight`, pump next.

Stale ack (wrong epoch / sequence / no in-flight match) → no-op.

Teardown / epoch bump → cancel queue + in-flight; no further deliveries.

### 6.4 Native focus during serial pump

Native focus to the child WebView runs **once per ticket** before that ticket’s
DTO delivery. If the child already has focus, focus call is still allowed and
must be cheap/success no-op. Blur-hide tickets must compose with
`focusHostInput` rules so routing does not spuriously hide.

### 6.5 Unsubscribe / disarm (frozen)

`onHostKey` returns an unsubscribe function. **Silent unsubscribe that leaves
Host intercepting keys into a dead receiver is forbidden.**

Frozen behavior when `hostKeys` was non-empty and a handler was armed:

- Calling unsubscribe **disarms** the receiver for this epoch and **starts the
  same terminal hide/teardown path as `requestHide` admission** for that epoch
  (panel session cannot remain “declared keys, no listener”).
- Unsubscribe after teardown → no-op.
- Host notifies main (private event or command result side channel) so
  `model.panel.hostKeys` is cleared / panel discarded under epoch guard
  **before or as** intercept stops—never leave a live tagged input still
  swallowing ArrowDown with nowhere to deliver.

Rationale: a panel that drops its only host-key handler has broken the ready
invariant; recovering by continuing to eat keys is worse than ending the
session.

### 6.6 `routeSequence` exhaustion

- `nextSequence` is `u64`, starts at `1`, increments by 1 per accepted enqueue.
- If `nextSequence` would overflow `u64::MAX`, Host **disarms routing** and
  runs requestHide-equivalent teardown for the epoch (pathological; must be
  tested as a unit branch). No wrap-around reuse within an epoch.

### 6.7 Key routing table (tagged argument input)

| Key / chord | Declared? | Host action |
|---|---|---|
| ArrowDown / ArrowUp (no Ctrl/Meta/Alt/Shift) | yes | Enqueue + serial deliver |
| ArrowDown / ArrowUp | no | No intercept |
| Primary+N (Ctrl xor Meta, no Shift/Alt) | yes | Enqueue + serial deliver |
| Primary+N | no | No intercept |
| Shift+Arrow / Ctrl+Shift+N / etc. | n/a in v1 | Never routed under base declarations |
| Enter | n/a | Submit (unchanged) |
| Escape | n/a | Hide admission (unchanged on main) |
| Backspace at caret 0 | n/a | Close tag (unchanged) |
| IME composing / ordinary characters | n/a | Never routed |

## 7. `requestHide` authorization

1. Capability: `plugin-panel-content-*` only.
2. Label → plugin id; compare with live identity + bootstrap epoch.
3. In-pattern stale → resolve no-op.
4. Outside capability → capability denial.
5. Live match → **admit** hide (resolve Promise) then terminal shared hide
   pipeline (§3.2).

| Condition | Promise | Terminal hide? |
|---|---|---|
| Live admit success | Resolve | Yes |
| Stale / no session | Resolve no-op | No |
| Admit failure before terminal start | Reject `windowFailed` | No |
| Foreground restore fails after admit | Already resolved | Restore best-effort only |
| Second call after admit | Resolve no-op | No duplicate hide owner |

Do not destroy content **before** Promise admission resolve is scheduled on the
content side (implementation: resolve/ack to bootstrap first, then teardown on
main/host timeline).

## 8. Escape in panel content

**DOM exception:** Host may read **only** `document.querySelector('dialog[open]')`
(standard HTML dialog). No other plugin DOM inspection.

Listener discipline (order matters):

1. Bootstrap registers a **capture-phase** `keydown` listener that runs first
   among Host listeners and **records**:
   - `isComposing`
   - `hadOpenDialog = Boolean(document.querySelector('dialog[open]'))`
   - It does **not** hide in capture.
2. Event propagates through target/bubble so plugin handlers may call
   synchronous `preventDefault()`.
3. Bootstrap registers a **bubble-phase** (or `setTimeout(0)` microtask **only if**
   equivalent to “after listeners of this turn”—prefer bubble on `window` /
   `document` with Host listener registered to run **after** plugin bubble
   handlers via registration order documented in bootstrap) that:
   - Ignores if recorded `isComposing`.
   - Ignores if recorded `hadOpenDialog` (dialog state sampled at capture,
     not re-queried after plugins close it mid-event—closing on this Escape
     is content’s job; a subsequent Escape hides).
   - Ignores if `event.defaultPrevented`.
   - Otherwise admits the shared hide path (same as `requestHide` terminal).

This prevents “Host capture hides before plugin `preventDefault`” races.

Async `preventDefault` after `await` cannot cancel hide.

## 9. Windows foreground restore

### 9.1 `HideReason`

```text
enum HideReason {
  ExplicitReturn,   // Escape (main or content), requestHide, unsubscribe-teardown
  Blur,             // app focus loss hide
  LaunchHandoff,    // hide because another app/result was launched
  Other,            // tray-only dismiss without return intent, etc.
}
```

**Restore prior capture only for `ExplicitReturn`.**  
`Blur` and `LaunchHandoff` **must not** restore—the user’s newly focused app
(or launched target) already owns the foreground.

### 9.2 Capture (show generation)

On each explicit show (hotkey, tray, …), before UiPilot takes foreground,
capture:

```text
ForegroundCapture {
  showGeneration: u64,
  hwnd: HWND,
  pid: u32,           // process id at capture time
}
```

Rules:

- Skip UiPilot-owned HWNDs, NULL, and failed recoverability checks.
- **Tray show:** if foreground at capture is Shell / taskbar / desktop class
  (implementation allowlist of non-restorable owners), store **no** capture
  (restore becomes no-op) rather than restoring Explorer spuriously.
- Hotkey show from an ordinary app (e.g. Notepad) stores that HWND+PID.
- Newer `showGeneration` invalidates older captures.

### 9.3 Restore

After terminal hide with `HideReason::ExplicitReturn`:

1. Load capture for the show generation being dismissed.
2. Re-validate: HWND still valid; **PID still matches** the captured pid for
   that HWND (mitigate HWND reuse); not UiPilot-owned; suitable visibility.
3. Normal foreground APIs only—no input synthesis.
4. Denial → log/metric; **no retry**; never fails hide admission already
   resolved.

## 10. Timing diagrams

### Host key

```text
keydown (declared) on tagged input
  → preventDefault
  → enqueue ticket(seq)
  → pump: native focus child WV
  → onHostKey(DTO)
  → handler settle
  → ack(epoch, seq)
  → pump next

unsubscribe(onHostKey)
  → disarm + ExplicitReturn hide admit
  → launcher clears panel/hostKeys under epoch guard
```

### requestHide

```text
content requestHide()
  → authorize
  → admit (Promise resolve)     // still in live content WV
  → terminal hide (HideReason::ExplicitReturn)
  → teardown content WV
  → optional foreground restore
```

## 11. Relationship to `focusHostInput`

| Direction | API |
|---|---|
| Content → tagged input | `focusHostInput()` (2026-08-25) |
| Tagged input → content (declared) | `onHostKey` + serial ack (this design) |
| Content → hide | `requestHide` admission / Escape (this design) |

## 12. Testing matrix (frozen)

### Launcher `hostKeys` path

- Open/submit `PluginPanelCommandResult` includes validated `hostKeys`.
- Parser rejects unknown declarations / wrong shapes.
- Stale epoch or pluginId result cannot overwrite live `model.panel.hostKeys`.
- Empty `hostKeys` → no ArrowDown intercept.

### Serial routing

- Two quick ArrowDown presses → two deliveries with consecutive sequences when
  handler acks promptly; no dropped middle press while queue has capacity.
- Queue full → preventDefault but no enqueue; prior items still drain.
- Handler throw / reject → still ack; next item pumps.
- Ack timeout clears inFlight and continues.
- Stale ack no-op.
- `routeSequence` exhaustion branch tears down (unit).

### Unsubscribe

- Unsubscribe with live non-empty `hostKeys` → session teardown; tagged input
  no longer swallows keys; launcher panel identity cleared.

### requestHide

- Promise resolves on admission while content WV still alive (test via
  bootstrap probe / ordering assertion).
- Teardown follows admission; caller need not observe post-teardown settle.
- Stale caller no-op resolve.

### Escape

- Capture records `dialog[open]`; plugin bubble `preventDefault` prevents hide.
- Host does not hide in capture before plugin handlers run.
- Composing ignored.

### Foreground

- ExplicitReturn restore attempts PID+HWND check.
- Blur hide does not restore.
- LaunchHandoff does not restore.
- Tray capture of Shell → empty capture.
- Restore failure does not affect hide admission.

### Compatibility / capability

- As revision 1, plus new result field exact-key parsers.
- Content commands remain panel-content-only.

### Manual

- Notepad → hotkey → Notes/panel Escape or requestHide → focus returns when
  OS allows; agents do not drive user input.

## 13. Implementation-phase doc/code touch list

- Manifest / schema / CLI / Rust `PublicPanelV1`
- `PluginPanelCommandResult` + parsers + launcher model
- Bootstrap: `onHostKey`, ack, Escape capture/bubble, `requestHide` admit
- Panel controller: queue, serial pump, disarm
- Lifecycle: `HideReason`, HWND+PID capture/restore
- Host `0.3.1`; demo-panel contract only (not Notes)

## 14. Out of scope

- Notes business behavior.
- Guaranteed foreground restore.
- Live search streaming / arbitrary host keys.
- Redefining Enter submit.

## 15. Decisions closed in revision 2

| Topic | Decision |
|---|---|
| Launcher knowledge of `hostKeys` | Via `PluginPanelCommandResult.hostKeys` + epoch-guarded model |
| Key delivery | Serial queue, at-most-once, ack, not latest-wins |
| Unsubscribe | Disarm + ExplicitReturn teardown |
| `requestHide` Promise | Admission resolve; terminal hide after |
| Blur-hide restore | Forbidden; only `ExplicitReturn` |
| Tray capture | Skip non-restorable Shell/desktop owners |
| Escape vs DOM-agnostic | Sole `dialog[open]` exception; capture record + post-bubble check |
| New shortcut token | `Primary+N`; Shift=false required |
| HWND reuse | Persist HWND+PID; revalidate PID on restore |

## 16. Approval

Status remains **Draft** until review accepts revision 2. Do not implement
until Status is **Approved**.
