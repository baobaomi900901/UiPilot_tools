# Public Plugin Panel Key Routing And Hide Design

**Date:** 2026-08-26  
**Status:** Draft — awaiting review (revision 5; closes review round 5 P1/P2)  
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
- Starting a short hide-commit fallback from Rust admit time before Bootstrap
  has observed the admit response (forbidden; see §3.2).
- Guessing missing `clientSequence` values with a gap timer (forbidden; see
  §6.2.1).

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
  `PanelHideTicket { sessionEpoch, hideTicketId, phase: Admitted }`, return
  `admitted`. **Do not** start the short commit fallback on admit.
- Bootstrap applies the admit result in JS, then **resolves** the public
  Promise. Plugin `await requestHide()` continuations are therefore scheduled
  as microtasks while the WebView is still alive.
- Stale / wrong epoch / in-pattern unauthorized → `{ outcome: 'noop' }`;
  Promise resolves; no ticket; no hide.
- Cannot admit (hide owner conflict that must fail closed before terminal
  work) → command error → Promise rejects `windowFailed`.

#### Phase A′ — Admit observed (private; gates short fallback)

Frozen command name: **`plugin_panel_request_hide_admit_observed`**.

Input: `{ sessionEpoch, hideTicketId }`  
Capability: **panel-content only**.

After applying admit and resolving the public Promise, bootstrap **must**
invoke this command (same turn as apply, before scheduling commit). Rust marks
the ticket `Observed`. Only an `Observed` (or already-committing) ticket may
start the **short** commit fallback.

Until `Observed`, Host **must not** destroy the content WebView via hide
fallback. A main-thread stall that delays JS receipt of admit therefore cannot
lose observability to a 500ms admit-time timer.

#### Phase B — Commit (bootstrap-primary; timers as below)

Frozen command name: **`plugin_panel_request_hide_commit`**.

**Primary path (required):** After admit succeeds, public Promise resolved, and
`admit_observed` invoked, bootstrap schedules commit on the **next macrotask**
(`setTimeout(0)` or equivalent):

```text
admit result applied
  → resolve public Promise
  → plugin_panel_request_hide_admit_observed({ sessionEpoch, hideTicketId })
  → setTimeout(0) → plugin_panel_request_hide_commit({ sessionEpoch, hideTicketId })
```

**Timers (frozen):**

| Condition | Timer | Action |
|---|---|---|
| Ticket `Observed`, no commit yet | **500ms** from observation | Rust auto-commit (bootstrap hung after observe) |
| Ticket still `Admitted` (never observed) | **30s** from admit | Fault reclaim: auto-commit; treat as renderer hung/crash / lost invoke |

**SDK / public contract for the 30s path:** If the content document never
observes admit (hung/crashed renderer), the public `requestHide()` Promise
**may never settle**. Document this as a hung-renderer exception in
`docs/plugin-sdk` and cover with Host tests (observe-before-short-fallback;
admit-blocked-30s-reclaim). Happy-path and ordinary main-thread work must still
observe admit and settle the Promise before teardown.

Short fallback must **not** start at admit time. Idempotent commit: first wins.

Commit semantics:

1. Commit performs shared launcher hide with `HideReason::ExplicitReturn`,
   then teardown. Ticket phase → `Committed`.
2. Duplicate commit / stale ticket → no-op.
3. Admit success then Host crash before commit → session may linger until next
   hide/show; not plugin-visible.

**Duplicate `requestHide` after admit for the same epoch:** Promise resolves
`noop` (or admit returns `noop`); no second ticket.

**Timeout:** Public Promise never waits on teardown. If admit command itself
exceeds the normal invoke path, existing invoke failure mapping applies
(`windowFailed`).

Public docs state: after `await requestHide()` resolves with admission, the
panel document may be destroyed on the next macrotask (or shortly after via
short fallback); do not touch DOM afterward beyond that continuation.

### 3.3 Manifest `hostKeys`

```json
{
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Primary+N"]
  }
}
```

`PublicPanelV1` gains **optional** `hostKeys: PanelHostKeyDeclaration[]`.

Frozen declaration grammar:

```ts
type PanelHostKeyDeclaration = 'ArrowDown' | 'ArrowUp' | 'Primary+N'
```

Validation (Rust install, JSON Schema, and plugin-CLI validate — **must stay
in sync**):

- Field optional; omit or `[]` → no panel key routing.
- Every element ∈ frozen enum; **deny unknown** strings.
- **Deny duplicates** (set uniqueness).
- Max length **≤ 8**.
- `additionalProperties` remains false on `PublicPanelV1`.

| Declaration | Matches when |
|---|---|
| `"ArrowDown"` | `key === 'ArrowDown'`, ctrl/meta/alt/shift all false |
| `"ArrowUp"` | same for `ArrowUp` |
| `"Primary+N"` | `key` n/N, alt/shift false, and **platform-primary modifier only** (§3.3.1) |

Matching rules:

- Extended chords (Shift+Arrow, Ctrl+Shift+N, …) are **not** routed under base
  tokens; future explicit declarations only.
- Ordinary characters, IME composing (`isComposing`), undeclared chords → never
  routed.
- Delivery DTO reports real modifier bits.
- Separate `"Ctrl+N"` / `"Meta+N"` tokens are **not** in v1.

#### 3.3.1 Platform-primary matching for `"Primary+N"`

| Host platform | Match predicate |
|---|---|
| Windows | `ctrlKey === true && metaKey === false` |
| macOS | `metaKey === true && ctrlKey === false` |

Windows **must not** intercept Meta+N under `"Primary+N"`. macOS **must not**
intercept Ctrl+N under `"Primary+N"`.

#### 3.3.2 Schema / CLI / Rust sync surfaces

Implementations must update **all** of:

| Surface | Path |
|---|---|
| Rust `PublicPanelV1` | `src-tauri/src/public_plugins/manifest.rs` (+ manifest tests) |
| Canonical schema | `docs/plugin-sdk/uipilot-plugin-v1.schema.json` |
| CLI schema copy | `packages/plugin-cli/schema/uipilot-plugin-v1.schema.json` |
| CLI contracts / validate | `packages/plugin-cli` (reject unknown/duplicate/`hostKeys` shape) |
| SDK API types (as needed) | `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts` |

CLI `validate` failure examples: unknown token, duplicate token, non-array,
length > 8.

### 3.4 Launcher `hostKeys` path

1. Rust copies manifest `hostKeys` into panel open/submit results (canonical
   sorted copy; `[]` if none).
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
3. TS parser (`parsePluginPanelCommandResult`) validates each declaration
   against the frozen enum, rejects unknown strings, rejects duplicates, and
   rejects non-arrays.
4. `launcher-core` installs `model.panel.hostKeys` only for matching
   `sessionEpoch` + `pluginId`. Stale results must not overwrite.
5. `panelKeyDown` matches only current-epoch `hostKeys`. Empty → pre-0.3.1
   intercept set only (Enter / Escape / Backspace-at-0).

Submit responses carry the same `hostKeys` snapshot as open. Private events
that disable routing (§6.5) clear/replace launcher `hostKeys` under epoch guard.

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
SDK contract.

| Command | Caller | Role |
|---|---|---|
| `plugin_panel_host_key_enqueue` | **main only** | Enqueue one physical key match |
| `plugin_panel_host_key_deliver` | Host-internal | Deliver DTO into content bootstrap |
| `plugin_panel_host_key_ack` | **panel-content only** | Ack `routeSequence` after handler settle |
| `plugin_panel_request_hide_admit` | **panel-content only** | Hide phase A |
| `plugin_panel_request_hide_admit_observed` | **panel-content only** | Admit observed by bootstrap |
| `plugin_panel_request_hide_commit` | **panel-content only** | Hide phase B (bootstrap primary) |

Main must never accept host-key ack or hide admit/observed/commit.
Panel-content must never accept enqueue.

### 6.2 Enqueue DTO (main → Rust)

```ts
interface PluginPanelHostKeyEnqueueInput {
  sessionEpoch: U64Decimal
  /** Physical press order within this UI epoch; see §6.2.1 */
  clientSequence: U64Decimal
  declaration: PanelHostKeyDeclaration
  key: PluginPanelHostKey
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
}

type PluginPanelHostKeyEnqueueResult =
  | { outcome: 'enqueued'; routeSequence: U64Decimal }
  | { outcome: 'droppedQueueFull' }
  | { outcome: 'noop' }  // stale epoch / unarmed / teardown / duplicate seq
  | { outcome: 'protocolViolation' }  // out-of-order; session ending
```

Guards: `require_main_label`; live epoch must match; `receiverArmed`;
declaration ∈ current session `hostKeys`.

#### 6.2.1 Physical key order and `clientSequence`

**Launcher (required):** Assign monotonic `clientSequence` per panel UI epoch
on each matching keydown in the order the main WebView receives those events.
**Serialize enqueue invokes**: at most one in-flight
`plugin_panel_host_key_enqueue` per epoch; later presses wait until the prior
enqueue returns before sending the next.

Because the launcher is serialized, a **legal** stream never produces gaps or
reorder at Rust. Rust therefore **fail-closes** on reorder instead of guessing:

```text
nextExpectedClientSequence: U64  // starts at 1 each armed epoch
deliveryQueue: VecDeque<item>    // depth ≤ 8
```

Rules:

1. Duplicate `clientSequence` (already seen / `< nextExpected` that was
   consumed) → `noop`.
2. Arrival with `clientSequence == nextExpected` and queue has room → append to
   delivery queue, assign `routeSequence`, **advance** `nextExpected` by one,
   return `enqueued`.
3. Arrival with `clientSequence == nextExpected` and queue full →
   `droppedQueueFull` **and** still **consume** that sequence: advance
   `nextExpected` by one without enqueueing (consumed-but-not-delivered). Main
   has already `preventDefault`’d. Do **not** leave a hole that would make
   later presses look out-of-order.
4. Arrival with `clientSequence > nextExpected` → **protocol violation**:
   return `protocolViolation`, disarm routing, run **ExplicitReturn** teardown
   for the epoch. Do **not** hold, do **not** gap-skip, do **not** deliver N+1
   before N. Rationale: under serialized launcher this indicates a Host bug or
   hostile caller; timed gap skip would permanently drop a merely-late N.
5. There is **no** hold map and **no** gap timer.

#### 6.2.2 `clientSequence` exhaustion

`clientSequence` increments from 1 as `u64`. On overflow (next value would
wrap): disarm routing and run **ExplicitReturn** teardown; do **not** wrap.
Same class as `routeSequence` exhaustion (§6.6).

### 6.3 Queue, serial delivery, ack timeout = session end

```text
HostKeyRouteState {
  sessionEpoch, nextRouteSequence, nextExpectedClientSequence, receiverArmed,
  queue: delivery order,
  inFlight: Option<ticket>,
}

ticket phases: Prepared | NativeFocused | DeliveredAwaitingAck | Accomplished | Cancelled
```

- Max delivery queue depth **8**; overflow of the expected sequence →
  `droppedQueueFull` with expected advanced (§6.2.1 rule 3); no coalescing.
- Pump delivers **one** inFlight at a time.
- After DTO delivery, content bootstrap awaits handler (sync or Promise), then
  calls `plugin_panel_host_key_ack { sessionEpoch, routeSequence }`.
- Handler throw/reject still acks (no retry).
- **Ack timeout 2s:** Host does **not** start the next delivery. Timeout
  **disarms** routing and runs **ExplicitReturn** teardown for the epoch.
  Strict serial ⇒ hang ends the session rather than overlap uncancellable
  handler Promises.
- Matching ack → accomplish → pump next only if still armed.
- Stale ack → no-op.
- Teardown cancels queue + inFlight.

### 6.4 Delivery into content

Host delivers by private bootstrap hook (e.g. eval
`__UIPILOT_PLUGIN_PANEL_HOST_KEY__` or equivalent), **not** synthetic DOM
keydown. Delivery includes frozen `PluginPanelHostKeyEvent` with host-assigned
`routeSequence` (not `clientSequence`).

Native focus child WebView once per ticket before delivery.

### 6.5 Unsubscribe

Unsubscribe with non-empty armed `hostKeys` → disarm + ExplicitReturn hide
admit/observed/commit path; launcher clears panel under epoch guard. Never leave
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
3. The microtask runs after sync dispatch finishes and before macrotasks.
   Contract:
   - **In-sync** `preventDefault()` → hide suppressed.
   - **`preventDefault()` after `await`** → does **not** cancel hide.
4. Microtask body: if Escape && !composing && !hadOpenDialog &&
   !`defaultPrevented` → hide admit; then §3.2 observed + commit.
5. No alternate bubble-order or `setTimeout(0)` Escape scheme.

## 8. Windows foreground restore

### 8.1 `HideReason`

`ExplicitReturn` (Escape, requestHide, unsubscribe/host-key timeout teardown)
may restore. `Blur`, `LaunchHandoff`, `Other` must not.

### 8.2 Capture replacement policy

`ForegroundCapture { showGeneration, hwnd, pid }` updates by **show scenario**:

| Show scenario | Capture action |
|---|---|
| **Hidden → shown**, restorable external HWND | **Replace** with `{hwnd, pid}` |
| **Hidden → shown**, Shell / taskbar / desktop / non-restorable | **Clear** to empty |
| Already visible, foreground **UiPilot-owned** (repeat hotkey / tray) | **Keep** existing capture |
| Already visible, show entry, **external restorable** | **Replace** |
| Already visible, show entry, **Shell-class** foreground | **Clear** to empty |

Prefer clear over “unchanged” on Shell-class show so tray re-show cannot
resurrect a stale Notepad target.

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
  → plugin_panel_host_key_enqueue(clientSequence, …)
  → Rust: only clientSequence == nextExpected accepted
       (else protocolViolation → ExplicitReturn teardown)
  → native focus child WV → deliver DTO → onHostKey → ack
  → pump next OR (ack timeout → ExplicitReturn teardown)
```

### requestHide

```text
content requestHide()
  → plugin_panel_request_hide_admit → { admitted, hideTicketId } | noop
  → bootstrap resolves Promise
  → plugin_panel_request_hide_admit_observed
       // arms 500ms short fallback; 30s unrecovered path remains for never-observed
  → setTimeout(0) → plugin_panel_request_hide_commit
  → hide + teardown
```

### Escape

```text
capture keydown record + queueMicrotask
  → sync target/bubble (optional preventDefault)
  → microtask: if Escape && !defaultPrevented && !dialog && !composing
       → hide admit → observed → commit (§3.2)
```

## 10. Testing matrix (additions for revision 5)

- Frozen command names including `admit_observed` in capabilities.
- Serialized launcher enqueue; out-of-order `clientSequence > nextExpected` →
  `protocolViolation` + ExplicitReturn teardown (no hold / no gap skip).
- `droppedQueueFull` on expected sequence advances `nextExpected`; next press
  with next sequence enqueues (no false protocolViolation).
- `clientSequence` / `routeSequence` overflow → ExplicitReturn teardown.
- Ack timeout → session teardown; no overlapping second handler.
- `requestHide`: Promise settles before destroy on happy path; short 500ms
  fallback starts only after `admit_observed`; blocked/hung before observe does
  not short-fallback within 500ms; 30s unrecovered reclaim; SDK docs note Promise
  may never settle on hung renderer; duplicate admit → noop.
- Escape: capture + `queueMicrotask`; sync preventDefault blocks; post-await
  does not.
- Manifest/schema/CLI: unknown/duplicate/`hostKeys` length > 8 rejected;
  Rust `PublicPanelV1` matches schema.
- Empty hostKeys sticky violation; platform Primary+N; Shell clear capture;
  UiPilot-focused repeat keeps capture; Blur does not restore.

Retain prior matrix items (parser, stale epoch, dialog open, PID, capabilities,
manual Notepad ExplicitReturn).

## 11. Implementation touch list

- Manifest / schema / CLI / Rust `PublicPanelV1.hostKeys` (§3.3.2 surfaces)
- Permissions/capabilities for frozen commands (enqueue, ack, admit, observed,
  commit)
- `PluginPanelCommandResult` + parsers + launcher model
- Bootstrap: `onHostKey`, host-key ack, Escape `queueMicrotask`, hide
  admit → observed → next-macrotask commit
- Panel controller: serialized expected-only enqueue, dropped advances expected,
  fail-closed reorder, serial pump, ack timeout teardown
- Lifecycle: `HideReason`, HWND+PID capture/restore, Shell clear policy
- Host `0.3.1`; SDK hung-renderer Promise note; demo-panel contract only
  (not Notes) until Approved + planned

## 12. Out of scope

Notes business; guaranteed foreground restore; live search streaming;
concurrent host-key handlers; gap-timer reorder recovery.

## 13. Decisions closed in revision 5

| Topic | Decision |
|---|---|
| Cross-WV protocol | Frozen names including `admit_observed` |
| clientSequence order | Serialized launcher; Rust accept only `== nextExpected`; `>` → teardown; no hold/gap timer |
| droppedQueueFull | Consume expected sequence (advance `nextExpected`) without delivery |
| Ack timeout | Disarm + ExplicitReturn teardown |
| requestHide observability | Admit → resolve → `admit_observed` → next-macrotask commit; short 500ms fallback only after Observed; 30s unrecovered if never observed (Promise may never settle — SDK) |
| Escape | Capture + `queueMicrotask`; sync preventDefault only |
| Primary+N | Platform Ctrl **xor** Meta |
| Illegal onHostKey | Sticky violation |
| Shell / repeat capture | Clear on Shell-class show; keep on UiPilot-focused repeat |
| Manifest contract | Optional `hostKeys` enum; deny unknown/duplicate; ≤8; Schema/CLI/Rust sync |

## 14. Approval

Status remains **Draft** until review accepts revision 5. Do not implement
until Status is **Approved**.
