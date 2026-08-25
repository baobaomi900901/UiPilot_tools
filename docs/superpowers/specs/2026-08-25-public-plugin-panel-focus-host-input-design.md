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
- A forged caller label resolves successfully with no side effect. It must not
  expose whether another plugin owns a live session.
- For a current authorized session, failure to focus the main WebView or deliver
  the private launcher event rejects with the existing `windowFailed` command
  error.

## 4. Authorization And Capability

The command is available only to `plugin-panel-content-*` WebViews through the
panel-content capability. Authorization is fail closed and ordered:

1. Parse the caller label as a panel-content label.
2. Resolve the plugin identity from that label.
3. Compare caller label, plugin identity, and bootstrap-supplied `sessionEpoch`
   with the current live `PanelSessionIdentity`.
4. If any comparison fails, return `Ok(())` without native focus or event emission.

No main, find, plugin-window shell/content, or unrelated panel-content label can
focus the tagged input through this command.

## 5. Data Flow And Ownership

For a current session:

1. The panel bootstrap calls the private Tauri command with its captured epoch.
2. Rust revalidates the caller and live session without holding controller locks
   across native focus or event emission.
3. Rust marks the panel-to-main focus handoff as an expected internal transfer.
4. Rust focuses the main WebView.
5. Rust emits a private event to `main` carrying the decimal-string
   `sessionEpoch`.
6. Launcher code accepts the event only when the current panel UI epoch matches,
   then focuses the tagged argument `<input>` without changing its value or
   selection beyond the browser's normal focus behavior.

The event is an internal invalidation-safe request, not a public plugin event. A
late event for a previous epoch is ignored.

## 6. Focus And Hide Ordering

Panel-to-main transfer remains inside the same top-level UiPilot window.

- The transfer is registered before native focus changes.
- Panel `LostFocus` followed by main `GotFocus` must invalidate or suppress any
  pending application-blur hide ticket.
- The main window remains visible and the panel session remains live.
- A later genuine application focus loss still tears down and hides through the
  existing panel lifecycle.
- Failure after registering the transfer must not leave a reusable suppression
  token that masks a later genuine blur.

## 7. Frontend Behavior

The launcher owns the argument input ref. The private focus event is routed to the
active panel UI and checked against its `sessionEpoch`. On acceptance it calls
`focus()` on that exact input.

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
| Forged or unrelated caller | Resolve, no-op |
| Event arrives after teardown/replacement | Frontend ignores it |
| Main WebView focus failure | Reject `windowFailed` |
| Private event emission failure | Reject `windowFailed` |

## 9. Testing And Acceptance

Automated coverage must prove:

- Bootstrap exposes a no-argument `focusHostInput()` and supplies its captured
  epoch privately.
- Current caller + current epoch focuses the tagged argument input while the
  session, tag, panel, and argument value remain unchanged.
- Calling again when already focused is successful.
- Missing session, teardown, wrong epoch, and forged caller are no-op successes.
- A late frontend event for an old epoch cannot focus a new session's input.
- Panel-to-main focus transfer does not hide the launcher; a later real blur still
  hides it.
- Capability tests prove only panel-content labels can invoke the command.
- `uipilot-plugin-api-v1.d.ts` and public panel documentation expose the method.

Manual acceptance controls real Windows focus only. The agent must not synthesize
mouse or keyboard input; the user triggers Ctrl+F or an equivalent test control.
