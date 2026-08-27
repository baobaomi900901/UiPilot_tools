# Panel Tag Close Button Tab Order

**Date:** 2026-08-26
**Status:** Approved

## Goal

In Panel mode, pressing Tab from the Host input must not focus the command tag's close button.

## Frozen Behavior

- Set the Panel command tag close button to `tabIndex={-1}`.
- Keep the button visible and available to pointer activation.
- Keep its accessible name, disabled state, tooltip, and `core.closePanel()` click behavior unchanged.
- Do not intercept Tab. After the close button is removed from sequential focus navigation, the browser continues to the next focusable target using its normal order.
- Do not change Panel Host-key routing, Panel content focus APIs, or plugin code.

## Verification

- A Launcher view regression test asserts that the rendered close button has `tabIndex === -1`.
- The existing click-close assertion continues to prove that pointer activation closes the Panel and returns to a fresh Launcher.
- Run the focused Launcher view test and the frontend type/build check.
