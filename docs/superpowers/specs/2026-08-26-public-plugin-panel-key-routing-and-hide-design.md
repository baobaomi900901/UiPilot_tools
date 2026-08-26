# Public Plugin Panel Key Routing And Hide Design

**Date:** 2026-08-26  
**Status:** Draft — awaiting review (revision 4; closes review round 4 P1/P2)  
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
- Treating Rust immediate auto-commit after hide admit as the primary
  observability path (forbidden; see §3.2).

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
continuation has run before WebView destruction. Freeze:

#### Phase A — Admit (observable to content)

Frozen command name: **`plugin_panel_request_hide_admit`**.

Input (bootstrap-supplied): `{ sessionEpoch }`  
Output:

```ts
type PanelHideAdmitResult =
  | { outcome: 'admitted'; hideTicketId: U64Decimal }
  | { outcome: 'noop' }
```

- Capability: **panel-content only**.
- Live matching epoch → allocate monotonic `hideTicketId`, install
  `PanelHideTicket { sessionEpoch, hideTicketId, phase: Admitted,
  admittedAtHostMs }`, return `admitted`.
- Bootstrap applies the admit result in JS, then **resolves** the public
  Promise. Plugin `await requestHide()` continuations are therefore scheduled
  as microtasks while the WebView is still alive.
- Stale / wrong epoch / in-pattern unauthorized → `{ outcome: 'noop' }`;
  Promise resolves; no ticket; no hide.
- Cannot admit (hide owner conflict that must fail closed before terminal
  work) → command error → Promise rejects `windowFailed`.

#### Phase B — Commit (bootstrap-primary; Rust delayed fallback only)

Frozen command name: **`plugin_panel_request_hide_commit`**.

**Primary path (required):** After admit succeeds and the public Promise has
been resolved, bootstrap schedules commit on the **next macrotask**
(`setTimeout(0)` or equivalent):

```text
admit result applied
  → resolve public Promise          // schedules plugin microtask continuations
  → setTimeout(0) → invoke
       plugin_panel_request_hide_commit({ sessionEpoch, hideTicketId })
```

This ordering guarantees plugin Promise continuations run before commit’s
terminal hide/teardown destroys the WebView. Content invokes commit while
still alive; commit does not wait on further plugin work.

**Rust delayed fallback (not the happy-path driver):** If a ticket remains
`Admitted` for **500ms** after `admittedAtHostMs` with no successful commit,
Host auto-commits that ticket. Fallback exists only for crashed/hung
bootstrap after admit; it must **not** run immediately on admit success and
must **not** race ahead of the bootstrap next-macrotask commit on the happy
path (idempotent commit: first wins).

Commit semantics:

1. Commit performs shared launcher hide with `HideReason::ExplicitReturn`,
   then teardown. Ticket phase → `Committed`.
2. Duplicate commit / stale ticket → no-op.
3. Admit success then Host crash before either commit path → session may
   linger until next hide/show; not plugin-visible.

**Duplicate `requestHide` after admit for the same epoch:** Promise resolves
`noop` (or admit returns `noop`); no second ticket.

**Timeout:** Public Promise never waits on teardown. If admit command itself
exceeds the normal invoke path, existing invoke failure mapping applies
(`windowFailed`).

Public docs state: after `await requestHide()` resolves with admission, the
panel document may be destroyed on the next macrotask (or shortly after via
fallback); do not touch DOM afterward beyond that continuation.

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

Command names below are **frozen** for permissions, capability generation, and
SDK contract (not illustrative).

| Command | Caller | Role |
|---|---|---|
| `plugin_panel_host_key_enqueue` | **main only** | Enqueue one physical key match |
| `plugin_panel_host_key_deliver` | Host-internal | Deliver DTO into content bootstrap (event or eval path; not a public plugin invoke) |
| `plugin_panel_host_key_ack` | **panel-content only** | Ack `routeSequence` after handler settle |
| `plugin_panel_request_hide_admit` | **panel-content only** | Hide phase A |
| `plugin_panel_request_hide_commit` | **panel-content only** | Hide phase B (bootstrap primary) |

Main must never accept host-key ack or hide admit/commit. Panel-content must
never accept enqueue.

### 6.2 Enqueue DTO (main → Rust)

```ts
interface PluginPanelHostKeyEnqueueInput {
  sessionEpoch: U64Decimal
  /** Physical press order within this UI epoch; see §6.2.1 */
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
  | { outcome: 'heldOutOfOrder' }  // accepted into hold; not yet deliverable
  | { outcome: 'droppedQueueFull' }
  | { outcome: 'noop' }  // stale epoch / unarmed / teardown / duplicate seq
```

Guards: `require_main_label`; live epoch must match; `receiverArmed`;
declaration ∈ current session `hostKeys`.

#### 6.2.1 Physical key order and `clientSequence`

**Launcher (required):** Assign monotonic `clientSequence` per panel UI epoch
on each matching keydown in the order the main WebView receives those
keydown events. **Serialize enqueue invokes**: at most one in-flight
`plugin_panel_host_key_enqueue` per epoch; later presses wait until the prior
enqueue returns before sending the next. This is the primary guard against
invoke reordering.

**Rust (required, even with serialized launcher):** Maintain

```text
nextExpectedClientSequence: U64  // starts at 1 each armed epoch
hold: Map<clientSequence, EnqueuedItem>  // out-of-order arrivals
```

Rules:

1. Duplicate `clientSequence` → `noop`.
2. Arrival with `clientSequence == nextExpected` → append to delivery queue,
   advance `nextExpected`, then flush any contiguous held sequences
   (`nextExpected`, `nextExpected+1`, …) into the delivery queue in order.
3. Arrival with `clientSequence > nextExpected` → store in `hold`, return
   `heldOutOfOrder`. **Do not** deliver or pump that item yet (fixes empty-queue
   N+1-first race).
4. Arrival with `clientSequence < nextExpected` → `noop` (already advanced
   past; duplicate or late after gap skip).
5. **Gap timeout 100ms** (host timer from first hold entry or from last
   advance that left a hole): if `hold` is non-empty and
   `nextExpected` is still missing, **skip** the missing sequence (treat as
   dropped; never delivered), advance `nextExpected` by one, flush contiguous
   hold, repeat until hold empty or next gap wait. Skipping does **not**
   teardown the session.
6. Hold + delivery queue combined depth still capped at **8** unmatched
   presses; overflow of a new arrival → `droppedQueueFull` (after main
   preventDefault); do not coalesce.

`routeSequence` is assigned only when an item enters the **delivery** queue
(not when merely held).

#### 6.2.2 `clientSequence` exhaustion

`clientSequence` increments from 1 as `u64`. On overflow (next value would
wrap): disarm routing and run **ExplicitReturn** teardown for the epoch; do
**not** wrap. Same class as `routeSequence` exhaustion (§6.6).

### 6.3 Queue, serial delivery, ack timeout = session end

```text
HostKeyRouteState {
  sessionEpoch, nextRouteSequence, nextExpectedClientSequence, receiverArmed,
  hold: Map<clientSequence, item>,
  queue: delivery order (clientSequence ascending),
  inFlight: Option<ticket>,
}

ticket phases: Prepared | NativeFocused | DeliveredAwaitingAck | Accomplished | Cancelled
```

- Max unmatched depth **8** (hold + delivery queue); overflow →
  `droppedQueueFull` after preventDefault on main; no coalescing.
- Pump delivers **one** inFlight at a time from the delivery queue only.
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
- Teardown cancels hold + queue + inFlight.

### 6.4 Delivery into content

Host delivers by private bootstrap hook (e.g. eval
`__UIPILOT_PLUGIN_PANEL_HOST_KEY__` or equivalent), **not** synthetic DOM
keydown. Delivery includes frozen `PluginPanelHostKeyEvent` with host-assigned
`routeSequence` (not `clientSequence`).

Native focus child WebView once per ticket before delivery.

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

**Only algorithm** (sync cancel only):

1. Bootstrap registers **one capture-phase** `keydown` listener that records
   `{ isComposing, hadOpenDialog, keyIsEscape }` on the event object and
   schedules **exactly one** `queueMicrotask` (not `setTimeout(0)`, not a
   bubble listener).
2. Synchronous target/bubble listeners may call `preventDefault()` during the
   same event dispatch.
3. The microtask runs **after** the current event dispatch finishes (all sync
   listeners done) and **before** any macrotask. It also runs **before**
   Promise/`await` continuations that plugin handlers schedule from this
   turn—those continuations are queued later than the Escape microtask if the
   plugin only `await`s after the sync handler returns. Freeze the contract:
   - **In-sync** `preventDefault()` during dispatch → hide suppressed.
   - **`preventDefault()` after `await` / later microtask** → **does not**
     cancel hide (may race after Host already decided). Plugins that need to
     cancel Escape must do so synchronously in their keydown handler.
4. Microtask body:
   - Returns if recorded key was not Escape, or `isComposing`, or
     `hadOpenDialog`.
   - Returns if `event.defaultPrevented === true` (same event object).
   - Otherwise runs hide admit; commit follows §3.2 (bootstrap next-macrotask
     commit + Rust delayed fallback).
5. No alternate bubble-order or `setTimeout(0)` Escape scheme. Capture +
   `queueMicrotask` does not depend on plugin listener registration order
   relative to bootstrap.

Rationale vs revision 3: `setTimeout(0)` allowed plugin `await` microtasks to
run first and flip `defaultPrevented`, contradicting “async preventDefault
无效”. `queueMicrotask` from capture is enqueued during dispatch, before those
async continuations, while still observing sync `preventDefault` after bubble.

## 8. Windows foreground restore

### 8.1 `HideReason`

`ExplicitReturn` (Escape, requestHide, unsubscribe/host-key timeout teardown)
may restore. `Blur`, `LaunchHandoff`, `Other` must not.

### 8.2 Capture replacement policy

`ForegroundCapture { showGeneration, hwnd, pid }` updates by **show scenario**:

| Show scenario | Capture action |
|---|---|
| **Hidden → shown**, and current foreground is a restorable external HWND (non-UiPilot, non-Shell) | **Replace** with `{hwnd, pid}` for that foreground |
| **Hidden → shown**, and current foreground is Shell / taskbar / desktop / non-restorable | **Clear** capture to empty (do **not** keep a prior Notepad/etc. target) |
| Already visible, foreground is **UiPilot-owned** (repeat hotkey / tray while focused) | **Keep** existing capture unchanged; do not bump a clearing generation; do not store UiPilot HWND |
| Already visible, explicit show entry, current foreground is **external restorable** (non-UiPilot, non-Shell) | **Replace** with that external capture |
| Already visible, explicit show entry (e.g. tray), current foreground is **Shell / taskbar / desktop** | **Clear** capture to empty — do **not** leave a stale prior external capture that would wrongly restore later |

“Empty” means restore is skipped on the next ExplicitReturn. Prefer clear over
“unchanged” whenever the show entry’s foreground is Shell-class, so tray
re-show cannot resurrect an old Notepad target after the user left that
context.

### 8.3 Restore

On ExplicitReturn hide commit: if capture empty → skip restore. Else
re-validate HWND + **PID match**, non-UiPilot, then normal foreground APIs;
failure does not affect hide admission.

## 9. Timing diagrams

### Host key

```text
main keydown (declared, physical order)
  → preventDefault
  → await prior enqueue (serialized)
  → plugin_panel_host_key_enqueue(clientSequence, …)  // main-only
  → Rust: hold until nextExpected, then delivery queue
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
  → bootstrap resolves Promise          // plugin microtasks may run
  → setTimeout(0) → plugin_panel_request_hide_commit
  → hide + teardown
  // parallel safety net: Rust auto-commit only if still Admitted after 500ms
```

### Escape

```text
capture keydown record + queueMicrotask
  → sync target/bubble (optional preventDefault)
  → microtask: if Escape && !defaultPrevented && !dialog && !composing
       → hide admit → (§3.2 commit)
```

## 10. Testing matrix (additions for revision 4)

- Main-only enqueue; panel-content-only ack; crossed callers denied.
- Frozen command names present in capabilities / permissions generation.
- Serialized launcher enqueue: two rapid ArrowDown never send overlapping
  enqueue invokes.
- Out-of-order: N+1 arrives before N while delivery empty → N+1 held; N then
  delivers N then N+1; gap timeout skips missing N and then delivers held N+1.
- `clientSequence` / `routeSequence` overflow → ExplicitReturn teardown.
- Ack timeout → session teardown; **no** overlapping second handler start.
- `requestHide`: Promise continuation observes resolution before WebView
  destroy on happy path; bootstrap commit is next macrotask after resolve;
  Rust 500ms fallback commits only if bootstrap commit omitted; duplicate
  admit → noop.
- Escape: only capture + `queueMicrotask`; sync bubble `preventDefault`
  blocks hide; `preventDefault` after `await` does **not** block hide.
- Empty hostKeys + caught TypeError on onHostKey → ready still fails
  (violation sticky).
- Windows Primary+N ignores Meta+N; macOS ignores Ctrl+N.
- Repeat hotkey while already focused UiPilot preserves prior capture.
- Hidden→shown with external app replaces capture; Hidden→shown / tray show
  with Shell foreground **clears** capture (does not keep stale Notepad);
  Blur hide does not restore.

Retain prior matrix items (parser, stale epoch hostKeys, queue full, dialog
open, PID check, capabilities, manual Notepad ExplicitReturn).

## 11. Implementation touch list

Freeze command names in permissions/capabilities; main serialized enqueue +
content ack; `nextExpectedClientSequence` hold + 100ms gap skip; hide admit +
bootstrap next-macrotask commit + 500ms Rust fallback; Escape
`queueMicrotask`; sticky registration violation; capture clear-on-Shell-show
policy.

## 12. Out of scope

Notes business; guaranteed foreground restore; live search streaming;
concurrent host-key handlers.

## 13. Decisions closed in revision 4

| Topic | Decision |
|---|---|
| Cross-WV protocol | Frozen names; main-only enqueue + panel-content-only ack |
| clientSequence order | Serialized launcher enqueue **and** Rust `nextExpected` + hold + 100ms gap skip; exhaustion → teardown |
| Ack timeout | Disarm + ExplicitReturn teardown (no concurrent next handler) |
| requestHide observability | Admit → resolve Promise → next-macrotask commit; Rust 500ms auto-commit **fallback only** |
| Escape | Capture record + single `queueMicrotask`; sync preventDefault only |
| Primary+N | Platform-specific Ctrl **xor** Meta |
| Illegal onHostKey | Sticky violation; ready always fails |
| Repeat / Shell show capture | Keep if UiPilot-focused repeat; **clear** on Shell-class show; replace on restorable external |

## 14. Approval

Status remains **Draft** until review accepts revision 4. Do not implement
until Status is **Approved**.
