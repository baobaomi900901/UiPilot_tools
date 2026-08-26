# Public Plugin Panel Key Routing And Hide Design

**Date:** 2026-08-26  
**Status:** Draft — awaiting review (revision 3; closes review round 3 P1/P2)  
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
(§8).

## 2. Non-goals

- Implementing or changing `com.uipilot.notes` in this design round.
- Live argument streaming or ordinary character forwarding (Enter → submit →
  `onUpdate.input` remains the only argument path).
- Arbitrary key listening or a general cross-WebView RPC bus.
- `focusPanelContent('list')` or any Host API that encodes plugin DOM roles.
- Guaranteeing Windows `SetForegroundWindow` success.
- Letting panel content call `hide_launcher` / Tauri `invoke` / main DOM APIs.
- Restoring a prior foreground app after blur-hide or launch-handoff hide.
- Claiming “strict serial handler execution” while allowing timed-out handlers
  to keep running alongside a newer delivery (forbidden; see §6.3).

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
  sessionEpoch: U64Decimal
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

- Exactly one handler per content document lifetime (second register →
  `TypeError`).
- Non-empty `panel.hostKeys` requires register before ready (§5).
- Empty/omitted `hostKeys`: register is a **persistent violation** (§5.1), not
  a catchable soft error alone.
- Handler receives a frozen DTO only.
- Unsubscribe → §6.5.

### 3.2 `requestHide()` — two-phase admission / commit

```ts
window.uipilotPluginPanel.requestHide(): Promise<void>
```

Tauri command return alone does **not** guarantee the content JS Promise
continuation has run before WebView destruction. Freeze an explicit protocol:

#### Phase A — Admit (observable to content)

Private panel-content command, e.g. `plugin_panel_request_hide_admit`:

Input (bootstrap-supplied): `{ sessionEpoch }`  
Output:

```ts
type PanelHideAdmitResult =
  | { outcome: 'admitted'; hideTicketId: U64Decimal }
  | { outcome: 'noop' }
```

- Capability: **panel-content only**.
- Live matching epoch → allocate monotonic `hideTicketId`, install
  `PanelHideTicket { sessionEpoch, hideTicketId, phase: Admitted }`, return
  `admitted`. Bootstrap resolves the public Promise **only after** this
  command result is applied in JS (continuation scheduled).
- Stale / wrong epoch / in-pattern unauthorized → `{ outcome: 'noop' }`;
  Promise resolves; no ticket; no hide.
- Cannot admit (hide owner conflict that must fail closed before terminal
  work) → command error → Promise rejects `windowFailed`.

#### Phase B — Commit (host terminal; content may already be gone)

After admit returns to content:

1. Bootstrap immediately invokes private panel-content command
   `plugin_panel_request_hide_commit` with
   `{ sessionEpoch, hideTicketId }` **while the WebView is still alive**,
   or Host schedules commit from Rust when admit succeeds **without** waiting
   for a second content round-trip—**frozen choice: Rust auto-commits on the
   host timeline after a successful admit**, using the ticket installed in
   phase A. Content must not be required to survive until hide completes.
2. Commit performs shared launcher hide with `HideReason::ExplicitReturn`,
   then teardown. Ticket phase → `Committed`.
3. Duplicate commit / stale ticket → no-op.
4. Admit success then Host crash before commit → session may linger until
   next hide/show; not plugin-visible. Tests cover admit-then-commit ordering
   on the happy path.

**Duplicate `requestHide` after admit for the same epoch:** Promise resolves
`noop` (or admit returns `noop`); no second ticket.

**Timeout:** there is no content-side wait for commit. Public Promise never
waits on teardown. If admit command itself exceeds the normal invoke path,
existing invoke failure mapping applies (`windowFailed`).

Public docs state: after `await requestHide()` resolves with admission, the
panel document may be destroyed at any moment; do not touch DOM afterward.

### 3.3 Manifest `hostKeys`

```json
{
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Primary+N"]
  }
}
```

| Declaration | Matches when |
|---|---|
| `"ArrowDown"` | `key === 'ArrowDown'`, ctrl/meta/alt/shift all false |
| `"ArrowUp"` | same for `ArrowUp` |
| `"Primary+N"` | `key` n/N, alt/shift false, and **platform-primary modifier only** (§3.3.1) |

#### 3.3.1 Platform-primary matching for `"Primary+N"`

| Host platform | Match predicate |
|---|---|
| Windows | `ctrlKey === true && metaKey === false` |
| macOS | `metaKey === true && ctrlKey === false` |

Windows **must not** intercept Meta+N under `"Primary+N"`. macOS **must not**
intercept Ctrl+N under `"Primary+N"`. Delivery DTO still reports actual
modifier bits.

Undeclared / Shift variants / ordinary characters / IME composing → never
routed.

### 3.4 Launcher `hostKeys` path

1. Rust copies manifest `hostKeys` into panel open/submit results.
2. `PluginPanelCommandResult`:
   ```ts
   {
     sessionEpoch: U64Decimal
     pluginId: string
     commandLabel: string
     hostKeys: readonly PanelHostKeyDeclaration[]  // always present; [] if none
   }
   ```
   Parser exact keys: `['commandLabel', 'hostKeys', 'pluginId', 'sessionEpoch']`.
3. `launcher-core` installs `model.panel.hostKeys` only for matching
   `sessionEpoch` + `pluginId`.
4. `panelKeyDown` matches only current-epoch `hostKeys`.

## 4. Host version

Host **`0.3.1`**. Non-empty `hostKeys` or use of `onHostKey` / `requestHide`
requires `minimumHostVersion >= 0.3.1`. `schemaVersion` / `apiVersion` stay
**1**.

## 5. Ready gate

Non-empty `hostKeys`: exactly one `onHostKey` before ready; else ready fails
and session rolls back. Ready success arms `receiverArmed` for the epoch.

### 5.1 Empty `hostKeys` registration violation

If content calls `onHostKey` when `hostKeys` is empty/omitted:

1. Bootstrap throws `TypeError` (may be caught by buggy plugin code).
2. Bootstrap **also** sets a sticky `hostKeyRegistrationViolation = true` on
   the content document lifetime (not clearable by plugin JS).
3. Any subsequent content-ready attempt for this document **must fail** and
   roll back the session—even if the plugin catches the TypeError and later
   calls `onUpdate` + ready.

## 6. Host-key cross-WebView protocol (frozen)

### 6.1 Commands and capabilities

| Command (names illustrative; freeze in impl) | Caller | Role |
|---|---|---|
| `plugin_panel_host_key_enqueue` | **main only** | Enqueue one physical key match |
| `plugin_panel_host_key_deliver` (internal event or main→Rust→eval path) | Host-internal | Deliver DTO into content bootstrap |
| `plugin_panel_host_key_ack` | **panel-content only** | Ack `routeSequence` after handler settle |

Main must never accept host-key ack. Panel-content must never accept enqueue.

### 6.2 Enqueue DTO (main → Rust)

```ts
interface PluginPanelHostKeyEnqueueInput {
  sessionEpoch: U64Decimal
  /** Physical press order within this UI turn; see §6.2.1 */
  clientSequence: U64Decimal
  declaration: PanelHostKeyDeclaration
  /** Normalized bits mirrored into PluginPanelHostKeyEvent */
  key: PluginPanelHostKey
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
}

type PluginPanelHostKeyEnqueueResult =
  | { outcome: 'enqueued'; routeSequence: U64Decimal }
  | { outcome: 'droppedQueueFull' }
  | { outcome: 'noop' }  // stale epoch / unarmed / teardown
```

Guards: `require_main_label`; live epoch must match; `receiverArmed`;
declaration ∈ current session `hostKeys`.

#### 6.2.1 Physical key order

Launcher assigns monotonic `clientSequence` per panel UI epoch on each
matching keydown **in the order the main WebView receives keydown events**.
Rust enqueues in `clientSequence` order (not arrival-reorder across awaits):
if enqueue N+1 is processed before N completes, Rust still inserts by
`clientSequence` so delivery order equals physical press order. Duplicate
`clientSequence` → no-op. Gap after drop-on-full is allowed (dropped presses
never appear).

### 6.3 Queue, serial delivery, ack timeout = session end

```text
HostKeyRouteState {
  sessionEpoch, nextRouteSequence, receiverArmed,
  queue: ordered by (clientSequence → routeSequence),
  inFlight: Option<ticket>,
}

ticket phases: Prepared | NativeFocused | DeliveredAwaitingAck | Accomplished | Cancelled
```

- Max queue depth **8**; overflow → `droppedQueueFull` after preventDefault on
  main; no coalescing.
- Pump delivers **one** inFlight at a time.
- After DTO delivery, content bootstrap awaits handler (sync or Promise), then
  calls `plugin_panel_host_key_ack { sessionEpoch, routeSequence }`.
- Handler throw/reject still acks (no retry).
- **Ack timeout 2s:** Host does **not** start the next delivery. Timeout
  **disarms** routing and runs **ExplicitReturn** teardown for the epoch
  (same class as unsubscribe). Rationale: the prior handler Promise cannot be
  cancelled; continuing the queue would run handlers concurrently and violate
  serial execution. Strict serial ⇒ hang ends the session rather than overlap.
- Matching ack → accomplish → pump next only if still armed.
- Stale ack → no-op.
- Teardown cancels queue + inFlight.

### 6.4 Delivery into content

Host delivers by private bootstrap hook (e.g. eval
`__UIPILOT_PLUGIN_PANEL_HOST_KEY__` or equivalent), **not** synthetic DOM
keydown. Delivery includes frozen `PluginPanelHostKeyEvent` with host-assigned
`routeSequence` (not `clientSequence`).

Native focus child WebView once per ticket before delivery (§ prior revision).

### 6.5 Unsubscribe

Unsubscribe with non-empty armed `hostKeys` → disarm + ExplicitReturn hide
admit/commit path; launcher clears panel under epoch guard. Never leave
tagged input swallowing keys without a receiver.

### 6.6 `routeSequence` exhaustion

Increment from 1; overflow → disarm + ExplicitReturn teardown; no wrap.

### 6.7 Routing table

| Chord | Declared? | Action |
|---|---|---|
| ArrowDown/Up (no mods) | yes | main enqueue → serial deliver |
| Primary+N (platform rule §3.3.1) | yes | same |
| Shift / other mods variants | — | not routed in v1 |
| Enter / Escape / Backspace-at-0 | — | existing launcher behavior |
| Other / IME / chars | — | never routed |

## 7. Escape arbitration (single implementation)

**Only algorithm:**

1. Bootstrap registers **one capture-phase** `keydown` listener that records
   `{ isComposing, hadOpenDialog, keyIsEscape }` and does not hide.
2. Event continues through target and bubble so plugins may synchronously
   `preventDefault()`.
3. Capture listener schedules **exactly one** `setTimeout(0)` **macrotask**
   (not a microtask; not a bubble listener) that:
   - Returns if recorded key was not Escape, or `isComposing`, or
     `hadOpenDialog`.
   - Returns if `event.defaultPrevented === true` (same event object).
   - Otherwise admits ExplicitReturn hide (auto-commit path shared with
     `requestHide`).
4. No alternate bubble-order scheme. Bootstrap may load before plugin scripts;
   capture + `setTimeout(0)` does not depend on listener registration order
   relative to plugins.

Async `preventDefault` after `await` cannot cancel hide.

## 8. Windows foreground restore

### 8.1 `HideReason`

`ExplicitReturn` (Escape, requestHide, unsubscribe/host-key timeout teardown)
may restore. `Blur`, `LaunchHandoff`, `Other` must not.

### 8.2 Capture replacement policy

`ForegroundCapture { showGeneration, hwnd, pid }` is replaced **only when**:

1. The main window transitions **hidden → shown** (true show), **or**
2. UiPilot was already visible but an explicit show entry runs and the
   **current foreground HWND is not UiPilot-owned** (user gained external
   focus then re-invoked—rare; still safe to refresh).

If already visible and foreground is UiPilot-owned (repeat hotkey / tray while
focused): **keep the existing capture**; do **not** bump a generation that
clears the prior Notepad (etc.) target; do **not** store UiPilot HWND.

Tray show while foreground is Shell/taskbar/desktop → leave capture empty /
unchanged per non-restorable rules; do not write Shell as restore target.

### 8.3 Restore

On ExplicitReturn hide commit: re-validate HWND + **PID match**, non-UiPilot,
then normal foreground APIs; failure does not affect hide admission.

## 9. Timing diagrams

### Host key

```text
main keydown (declared, physical order)
  → preventDefault
  → plugin_panel_host_key_enqueue(clientSequence, …)  // main-only
  → Rust queue by clientSequence
  → native focus child WV
  → deliver DTO to bootstrap
  → onHostKey handler
  → plugin_panel_host_key_ack(routeSequence)  // panel-content-only
  → pump next OR (ack timeout → ExplicitReturn teardown)
```

### requestHide

```text
content requestHide()
  → plugin_panel_request_hide_admit → { admitted, hideTicketId } | noop
  → bootstrap resolves Promise
  → Rust auto-commit ticket → hide + teardown
```

## 10. Testing matrix (additions for revision 3)

- Main-only enqueue; panel-content-only ack; crossed callers denied.
- Two rapid ArrowDown: two enqueues with increasing `clientSequence`; delivery
  order matches physical order even if invoke ordering races.
- Ack timeout → session teardown; **no** overlapping second handler start.
- `requestHide` admit result observed in content before teardown; commit does
  not require surviving Promise after destroy.
- Duplicate requestHide after admit → noop.
- Escape: only capture + `setTimeout(0)` path; plugin bubble preventDefault
  blocks hide; no Host bubble-order dependency.
- Empty hostKeys + caught TypeError on onHostKey → ready still fails
  (violation sticky).
- Windows Primary+N ignores Meta+N; macOS ignores Ctrl+N (platform tests /
  conditional).
- Repeat hotkey while already focused UiPilot preserves prior capture.
- Hidden→shown replaces capture; Blur hide does not restore.

Retain prior matrix items (parser, stale epoch hostKeys, queue full, dialog
open, PID check, capabilities, manual Notepad ExplicitReturn).

## 11. Implementation touch list

Unchanged in spirit from revision 2, plus: freeze command names in
permissions/capabilities; main enqueue + content ack; hide admit ticket +
Rust auto-commit; sticky registration violation; capture replacement policy.

## 12. Out of scope

Notes business; guaranteed foreground restore; live search streaming;
concurrent host-key handlers.

## 13. Decisions closed in revision 3

| Topic | Decision |
|---|---|
| Cross-WV protocol | main-only enqueue + panel-content-only ack; ordered by clientSequence |
| Ack timeout | Disarm + ExplicitReturn teardown (no concurrent next handler) |
| requestHide observability | Admit command result → resolve Promise; Rust auto-commit after |
| Escape | Capture record + single `setTimeout(0)` macrotask only |
| Primary+N | Platform-specific Ctrl **xor** Meta |
| Illegal onHostKey | Sticky violation; ready always fails |
| Repeat show capture | Replace only on hidden→shown or external foreground re-show |

## 14. Approval

Status remains **Draft** until review accepts revision 3. Do not implement
until Status is **Approved**.
