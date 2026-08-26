# Settings Escape Return Design

**Status:** Approved

## Goal

Pressing Escape anywhere in the main application's Settings view returns to the launcher instead of hiding the application window.

## Contract

- Settings Escape has exactly the same navigation semantics as the existing `Return to launcher` button: it uses local navigation to `launcher`, clears the launcher query through the established transition, and restores launcher input focus through the existing view focus behavior.
- The behavior applies to every Settings tab, including General, Messages, and Plugins.
- Escape during IME composition remains ignored.
- Escape in the launcher view continues to request window hide.
- Panel-content Escape behavior is unchanged.

## Implementation Boundary

The shared `launcher-core` Escape route owns the view distinction. The React Settings key handler continues to prevent the native event and forwards Escape to the core; it does not create a second navigation implementation.

## Verification

- Core coverage proves Settings Escape transitions to `launcher` without calling `hideLauncher`.
- React coverage proves Escape from a focused Settings tab returns to the launcher and restores focus to its combobox.
- Existing launcher Escape coverage continues to prove window hiding outside Settings.
