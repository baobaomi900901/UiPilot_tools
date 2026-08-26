# Demo Panel Focus and Key Diagnostics Design

**Status:** Approved

## Goal

Extend `com.uipilot.demo-panel` only as a Host-contract fixture so a developer can see whether the panel content owns focus and which keyboard path delivered the latest events.

## Scope

- Show a live panel-content focus state, initialized from `document.hasFocus()` and updated from window `focus` and `blur` events.
- Show the latest keyboard event and keep the five most recent events in newest-first order.
- Distinguish `Host route` events received through `window.uipilotPluginPanel.onHostKey` from `Panel content` events received through a capture-phase `keydown` listener.
- Format modifier combinations consistently, including `Ctrl+N`, while preserving the Host event's `routeSequence` for inspection.
- Preserve the existing panel update, storage, `Ctrl+F` focus return, and `Ctrl+H` hide behaviors.

## Presentation

The existing definition list gains compact diagnostic rows for content focus, latest key, source, count, and Host route sequence when present. A small recent-events list shows at most five entries and has stable dimensions so keyboard activity does not resize the panel unexpectedly.

Focus uses clear text states (`Focused` and `Not focused`) with a restrained status indicator. Keyboard entries include their source so the same visible key can be attributed to either the Host route or the panel content.

## Event Flow

1. On load, render the current focus state and an empty keyboard state.
2. Window `focus` and `blur` events refresh the focus state.
3. `onHostKey` records the normalized Host key, source, route sequence, and increments the event count.
4. The capture-phase `keydown` listener records content keys before applying the existing `Ctrl+F` and `Ctrl+H` commands.
5. Each recorded event updates the latest fields and trims the in-memory history to five entries.

The diagnostics are session-only and do not use plugin storage.

## Boundaries

- No Host, SDK, schema, manifest-contract, or `com.uipilot.notes` changes.
- No new plugin behavior beyond observable diagnostics.
- No persistence, analytics, editable controls, or expanded demo workflows.
- Existing user changes to the demo README and icon remain untouched.

## Verification

- Fixture tests assert the focus listeners, capture-phase key listener, Host-key subscription, source labels, five-entry limit, and preservation of `Ctrl+F`/`Ctrl+H` behavior.
- SDK type checking and public-plugin validation remain unchanged and must pass.
- Manual acceptance checks focus transitions plus Host-routed and panel-content key displays in a real window; automation must not control mouse or keyboard input.
