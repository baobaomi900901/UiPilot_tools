# Demo Plugin Examples Design

**Status:** Approved approach, awaiting written-spec confirmation  
**Date:** 2026-08-17

## Goal

Replace the combined `/demo` reference package with two independent, static public-plugin examples:

- `/demo-win` demonstrates a singleton plugin child window.
- `/demo-return` demonstrates an asynchronous main-window result followed by a host-owned copy action.

The existing installed public plugins are completely removed before the new examples are installed manually.

## Installed Data Cleanup

After the user closes UiPilot, remove the complete public-plugin data root under the UiPilot application-data directory. This includes installed packages, activation state, settings, plugin storage, secrets, and staging transactions. Repository source files and built-in launcher functions are not removed.

The implementation must not terminate UiPilot or control user input. If UiPilot is running, cleanup waits for the user to close it.

## `/demo-win`

The current `com.uipilot.demo` example becomes an independent window example:

- source directory: `examples/public-plugins/com.uipilot.demo-win`
- plugin ID: `com.uipilot.demo-win`
- default command name: `demo-win`
- activation mode: `submit`
- output mode: `window`
- required input: yes
- permission: `ui.window` only

Submitting `/demo-win str` opens or reuses the plugin's one child window. The content retains the existing input, platform, theme, instance number, and `str yyyy-mm-dd` fields. Pinning, closing, dragging, focus transfer, and position restoration remain host-owned behavior.

## `/demo-return`

Create a separate main-result example:

- source directory: `examples/public-plugins/com.uipilot.demo-return`
- plugin ID: `com.uipilot.demo-return`
- default command name: `demo-return`
- activation mode: `submit`
- output mode: `mainResult`
- required input: yes
- permission: `clipboard.write` only
- no window manifest member or window assets

Submitting `/demo-return str` keeps the main window open while the plugin request is pending. On success, it publishes one result with text `str yyyy-mm-dd`; because it is the only result, it is selected by default. A second Enter executes its `copyText` default action and copies exactly that text.

The runtime always returns a main-result response. It has no output-mode constant or window branch.

## Source And Packaging

Each example owns its manifest, runtime, README, and focused tests. The window example additionally owns its window HTML, CSS, JavaScript, and SDK window-contract test. Packaging support must address each plugin by its own ID and package root; neither package depends on the other at runtime.

Historical approved specifications remain unchanged. Current SDK documentation and executable tests that point to the reference example are updated to the two new examples where required.

## Validation

Automated validation covers:

- both manifests pass the host package validator;
- IDs, command names, output modes, permissions, and resource sets are exact;
- `/demo-win` returns the existing window payload;
- `/demo-return` returns one `copyText` result containing `input + local yyyy-mm-dd`;
- both TypeScript SDK contracts compile;
- both development directories can be packaged and staged.

Manual installation and UI acceptance are performed by the user. No automated test controls the mouse or keyboard.

## Acceptance

1. The old installed public-plugin data is absent after cleanup.
2. Installing the `demo-win` package exposes only `/demo-win`; submitting input opens its singleton child window.
3. Installing the `demo-return` package exposes only `/demo-return`; the first Enter waits and publishes the selected result, and the second Enter copies it.
4. The two plugins have independent identities, packages, permissions, runtime generations, and uninstall behavior.
