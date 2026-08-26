# Demo Panel Focus and Key Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `com.uipilot.demo-panel` visibly report panel-content focus and recent Host-routed or content-local keyboard events.

**Architecture:** Keep all diagnostic state inside the existing static panel. DOM focus events and capture-phase `keydown` events feed the same bounded renderer as `uipilotPluginPanel.onHostKey`; no Host, SDK, manifest contract, or persistent storage changes are needed.

**Tech Stack:** Static HTML/CSS/JavaScript, UiPilot Panel Bootstrap API, Node.js test runner, jsdom.

**Approved design:** [`docs/superpowers/specs/2026-08-26-demo-panel-focus-and-key-diagnostics-design.md`](../specs/2026-08-26-demo-panel-focus-and-key-diagnostics-design.md), especially `Scope`, `Presentation`, `Event Flow`, and `Boundaries`.

## Global Constraints

- Modify only the demo-panel Host-contract fixture and its focused test.
- Preserve the existing update, storage, `Ctrl+F` focus return, and `Ctrl+H` hide behavior.
- Keep diagnostics session-only and retain at most five recent events.
- Do not modify Host, SDK, schemas, `com.uipilot.notes`, the user's demo README edits, or the user's icon.
- Automated verification must not synthesize or control real mouse or keyboard input.

## Global Execution Rules

- Follow TDD: add focused failing behavior checks, confirm the intended failure, implement the minimum contract, and rerun focused checks.
- Produce one atomic implementation commit containing only the files named below; do not absorb pre-existing user changes.
- Dependency order: Task 1 only.

---

### Task 1: Render Focus and Keyboard Diagnostics

**Files:**
- Modify: `examples/public-plugins/com.uipilot.demo-panel/package/dist/panel.html`
- Modify: `examples/public-plugins/com.uipilot.demo-panel/package/dist/panel.css`
- Modify: `examples/public-plugins/com.uipilot.demo-panel/package/dist/panel.js`
- Test: `examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js`

**Dependencies:** Approved design sections `Scope`, `Presentation`, `Event Flow`, and `Boundaries`.

- [ ] Add jsdom-backed fixture checks for initial `document.hasFocus()` rendering and window `focus`/`blur` transitions.
- [ ] Cover Host `onHostKey` events with normalized key text, `Host route` source, event count, and visible `routeSequence`.
- [ ] Cover capture-phase content `keydown` events with modifier formatting, `Panel content` source, newest-first history, and a five-entry bound.
- [ ] Verify `Ctrl+F` still calls `focusHostInput()` and `Ctrl+H` still calls `requestHide()` after the event is recorded.
- [ ] Add compact semantic diagnostic markup without replacing the existing invocation fields.
- [ ] Implement one in-memory event recorder and renderer shared by Host and content events.
- [ ] Style stable focus states, latest-event fields, and the bounded history without changing the panel's overall visual language.

**Distinct test coverage:** A mocked panel starts unfocused, transitions on focus events, records a Host `ArrowDown` with its route sequence, records content modifier keys during capture, trims six ordered events to the newest five, and preserves both existing bridge commands.

**Verify:**
- `node --test examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js`
- `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-panel/tests/sdk-contract.ts`
- `node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-panel/package --platform windows`

## Final Checklist

- [ ] Focus state and both keyboard sources are visibly distinguishable.
- [ ] Recent history remains stable at five entries.
- [ ] Focus return, hide, update, and storage contracts still pass focused automation.
- [ ] User performs real-window focus and keyboard acceptance after rebuilding/reinstalling; automation does not control input.
