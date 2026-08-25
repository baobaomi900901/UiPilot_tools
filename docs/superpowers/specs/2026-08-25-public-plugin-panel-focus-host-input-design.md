# Public Plugin Panel Focus Host Input Design

**Date:** 2026-08-25  
**Status:** Approved  
**Related:** `docs/superpowers/specs/2026-08-24-public-plugin-panel-mode-design.md`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`

## 1. Goal

Add one host-owned focus bridge for public `panel` plugins:

```ts
window.uipilotPluginPanel.focusHostInput(): Promise<void>
```

The method transfers keyboard focus from the isolated panel child WebView to the
launcher argument input that owns the current plugin tag. It does not close the
panel, remove the tag, submit, or change the argument text.

## 2. Non-goals

- Implementing `com.uipilot.notes` or changing an existing plugin's behavior.
- Live keystroke delivery from the launcher to panel content.
- Allowing a panel to focus arbitrary host controls.
- Adding arguments to `focusHostInput()` or exposing `sessionEpoch` to plugin code.
- Adding a general cross-WebView event API.

## 3. Public Contract

`UiPilotPluginPanelApiV1` gains `focusHostInput(): Promise<void>`.

- The plugin passes no arguments.
- The bootstrap captures the mounted panel's `sessionEpoch` and includes it in the
  private Tauri command payload.
- Repeated calls while the tagged argument input is already focused succeed and
  leave it editable.
- A missing, torn-down, replaced, or epoch-mismatched session resolves successfully
  with no side effect.
- A stale or forged `plugin-panel-content-*` label that passes the capability
  pattern but does not own the current session resolves successfully with no side
  effect. It must not expose whether another plugin owns a live session.
- Main, find, plugin-window, and other labels are rejected by Tauri capability
  enforcement before the command reaches Rust.
- For a current authorized session, failure to focus the main WebView or deliver
  and confirm focus of the tagged argument input rejects with the existing
  `windowFailed` command error.
- The Promise resolves only after the launcher has confirmed that the exact
  tagged argument input is `document.activeElement`.

## 4. Authorization And Capability

The focus-request command is available only to `plugin-panel-content-*` WebViews
through the panel-content capability. A separate acknowledgement command is
available only to the exact `main` WebView capability. Authorization is fail
closed and ordered:

1. Parse the caller label as a panel-content label.
2. Resolve the plugin identity from that label.
3. Compare caller label, plugin identity, and bootstrap-supplied `sessionEpoch`
   with the current live `PanelSessionIdentity`.
4. If a panel-content caller passed capability enforcement but any comparison
   fails, return `Ok(())` without native focus or event emission.

Main, find, and plugin-window labels are denied at the capability layer. An
unrelated or stale panel-content label reaches the narrow Rust guard only if it
matches the capability label pattern, then receives the no-op result above.

## 5. Data Flow And Ownership

Two private decimal-string identifiers bind one focus request:

- `sessionEpoch`: the existing panel session identity captured by Bootstrap.
- `focusRequestId`: a host-allocated monotonic `u64`, serialized as a canonical
  decimal string and never converted to JavaScript `number`.

Only one focus request is current per panel session. A newer request supersedes an
older one; the older Promise resolves successfully with no side effect. For a
current session:

1. The panel bootstrap calls the private Tauri command with its captured epoch.
2. Under the panel-controller lock, Rust revalidates caller/session, allocates
   `focusRequestId`, increments `focusRevision`, and installs
   `HostInputFocusTicket { sessionEpoch, focusRequestId, focusRevision }` in the
   `Prepared` phase. Installing a newer ticket supersedes the prior ticket and
   wakes its waiter.
3. On the Tauri main thread, Rust performs a CAS-style claim of that exact ticket.
   If teardown, replacement, or a newer request won first, the command resolves as
   a no-op. A successful claim changes the phase to `NativeClaimed`; no controller
   lock is held across native focus or event emission.
4. Rust focuses the main WebView, changes the ticket to `AwaitingAck`, and emits a
   private event to `main` carrying `{ sessionEpoch, focusRequestId }`.
5. Launcher code accepts the event only when the current panel UI epoch matches,
   focuses the tagged argument `<input>`, and checks
   `document.activeElement === input`.
6. Launcher invokes the main-only acknowledgement command with
   `{ sessionEpoch, focusRequestId, focused }`.
7. A matching `focused: true` acknowledgement establishes main-content focus,
   invalidates pending panel blur tickets by advancing `focusRevision`, and
   resolves the original Promise. Matching `focused: false` rejects the current
   request with `windowFailed`.

The event is an internal invalidation-safe request, not a public plugin event. A
late event for a previous epoch is ignored and may send `focused: false`; the Rust
ack guard treats it as stale and performs no side effect.

The command waits off the Tauri main thread for at most **2 seconds**. At timeout,
a still-current ticket is cancelled and rejects with `windowFailed`; a ticket made
stale by teardown, replacement, or a newer focus request resolves successfully.

## 6. Focus And Hide Ordering

Panel-to-main transfer remains inside the same top-level UiPilot window.

- The ticket is prepared and then claimed before native focus changes.
- Teardown/replacement cancels any ticket and wakes its waiter. If it happens
  before the main-thread claim, native focus and event emission do not run. If it
  happens after the claim, the focus request owns that linearized main-thread
  operation; teardown still makes its later event/ack stale so it cannot revive UI.
- Panel `LostFocus` may create the existing delayed application-blur ticket. Main
  `GotFocus` or a successful frontend acknowledgement advances `focusRevision`,
  invalidating that blur ticket before it can hide the launcher.
- The main window remains visible and the panel session remains live.
- A later genuine application focus loss still tears down and hides through the
  existing panel lifecycle.
- Native-focus, emit, negative-ack, and timeout failure cancel the exact focus
  ticket and wake its waiter. Cancellation cannot leave a reusable suppression
  token that masks a later genuine blur.

## 7. Frontend Behavior

The launcher owns the argument input ref. The private focus event is routed to the
active panel UI and checked against both ticket identifiers. On acceptance it
calls `focus()` on that exact input, verifies `document.activeElement`, and always
acknowledges the matching request. Event registration occurs before launcher
readiness is reported so a valid request cannot be lost during frontend startup.

The handler must not:

- clear or rewrite the argument;
- remove or recreate the plugin tag;
- submit the current argument;
- close or remount the panel;
- focus the general launcher search input.

## 8. Failure Behavior

| Condition | Result |
|---|---|
| No live panel session | Resolve, no-op |
| Wrong/stale epoch | Resolve, no-op |
| Stale/forged panel-content caller within capability scope | Resolve, no-op |
| Main/find/plugin-window or other caller outside capability scope | Capability rejection |
| Event arrives after teardown/replacement | Frontend ignores it |
| Main WebView focus failure | Reject `windowFailed` |
| Private event emission failure | Reject `windowFailed` |
| Exact frontend ack reports `focused: false` | Reject `windowFailed` |
| Current request has no ack within 2 seconds | Cancel and reject `windowFailed` |
| Request is superseded while waiting | Resolve, no-op |

## 9. Testing And Acceptance

Automated coverage must prove:

- Bootstrap exposes a no-argument `focusHostInput()` and supplies its captured
  epoch privately.
- Current caller + current epoch focuses the tagged argument input while the
  session, tag, panel, and argument value remain unchanged.
- Calling again when already focused is successful.
- Missing session, teardown, wrong epoch, and forged caller are no-op successes.
- Main/find/plugin-window callers are denied by capability; stale or forged
  panel-content callers that pass the capability pattern are Rust-level no-op
  successes.
- A late frontend event for an old epoch cannot focus a new session's input.
- `focusRequestId` binds event, ack, waiter, and settlement; concurrent calls use
  latest-wins and the superseded Promise resolves as a no-op.
- Ordered race: prepare A -> teardown before main-thread claim -> A resolves no-op
  with zero native focus/event side effects.
- Ordered race: claim A -> teardown -> late event/ack -> no session revival and A
  resolves no-op once staleness is observed.
- Current request timeout and negative ack reject `windowFailed`; stale timeout or
  stale ack does not affect a newer session.
- Panel-to-main focus transfer does not hide the launcher; a later real blur still
  hides it.
- Capability tests prove only panel-content labels can invoke the command.
- `uipilot-plugin-api-v1.d.ts` and public panel documentation expose the method.

Manual acceptance controls real Windows focus only. The agent must not synthesize
mouse or keyboard input; the user triggers Ctrl+F or an equivalent test control.
