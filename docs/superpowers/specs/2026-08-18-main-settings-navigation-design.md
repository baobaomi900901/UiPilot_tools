# Main And Settings Navigation Design

**Status:** Approved in conversation on 2026-08-18; written review pending

## Goal

Add direct, in-window navigation between the launcher and Settings without hiding,
reopening, or refocusing the native window.

## User Contract

- The launcher query input has a Settings icon button inside its right edge.
- Activating the Settings button switches the existing main window to Settings.
- The Settings title has a Back arrow button immediately to its left.
- Activating Back returns to the launcher, restores focus to the query input, retains
  the previous query, and refreshes the matching results.
- The existing Settings Close button remains on the right and still hides the window.
- `Escape` behavior remains unchanged and still hides the main window.

## Architecture

`LauncherCore` owns the view transition. A shared internal transition function is used
by both native `launcher://shown` events and the new in-window navigation methods so
request retirement, `viewEpoch`, settings loading, result clearing, and query refresh
keep one implementation.

The transition from launcher to Settings preserves the current invocation and query,
loads the current settings snapshot, and never invokes a new native show operation.
The reverse transition preserves the query, schedules a fresh application search when
the query is non-empty, and lets the existing view focus effect select the query input.

## UI

- Use Lucide `Settings` and `ArrowLeft` icons through existing Ant Design buttons.
- The Settings button is an `Input` suffix and has an accessible `aria-label` and tooltip.
- The Back button is grouped with the Settings heading and has an accessible label and tooltip.
- Existing dimensions, Close behavior, drag regions, theme tokens, and focus styling remain unchanged.

## Failure Behavior

In-window navigation is synchronous and does not add a new backend failure path. Settings
load failures continue to use the existing retry and status UI. Late searches or settings
operations are rejected by the existing epoch and ownership checks after a transition.

## Testing

- Core tests cover launcher to Settings and Settings to launcher transitions, including
  query preservation, result retirement, settings loading, and refreshed search.
- View tests cover the two Lucide buttons, labels, click handlers, input suffix placement,
  preserved Close behavior, and restored launcher input focus.
- Existing launcher and Settings regression tests remain green.

## Acceptance

1. Enter text in the launcher, click Settings, and confirm Settings opens in the same window.
2. Click Back and confirm the prior text, matching results, and input cursor return.
3. Confirm the Settings Close button and `Escape` still hide the window.
