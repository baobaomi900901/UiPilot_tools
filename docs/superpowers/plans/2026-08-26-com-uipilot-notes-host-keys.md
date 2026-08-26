# com.uipilot.notes Host Keys And Hide Integration

**Goal:** Upgrade `com.uipilot.notes` to Host `0.3.1` panel contracts (`hostKeys`,
`onHostKey`, Escape arbitration, `requestHide`) without changing Host/SDK or
Notes business layout.

**Architecture:** Keep Notes UI and `notes-logic.js`. Register one `onHostKey`
before ready; route `ArrowDown` / `ArrowUp` / `n` into existing
`moveListSelection` / `openNewDialog` as fire-and-forget so the Host ack settles
within 2s. Escape uses sync `preventDefault` only when canceling dialogs or
starting unsaved confirm; clean Escape is left to Host. Confirmed save/discard
on Escape-driven unsaved flow calls parameterless `requestHide()`.

**Governing specs:**  
`docs/superpowers/specs/2026-08-26-public-plugin-panel-key-routing-and-hide-design.md`  
(plus panel mode / focusHostInput specs). Fixture: `com.uipilot.demo-panel`.

## Constraints

- Touch only `examples/public-plugins/com.uipilot.notes/**` (and this plan).
- No Host/SDK/Schema/CLI/demo-panel/note changes; no push; preserve unrelated dirty files.
- No Tauri `invoke`, no plugin-made hide/focus identifiers, no `stopPropagation` on Escape.
- Do not await dialogs inside the Host key handler.

## Global execution

TDD per task → focused verify → one atomic commit for the notes upgrade (single
feature slice). Dependency: Task 1 → Task 2 → Task 3.

### Task 1: Manifest, README, and SDK contract tests

- **Files:** `package/plugin.json`, `README.md`, `tests/sdk-contract.ts`,
  `tests/runtime.test.js` (manifest assertions)
- **Distinct test coverage:** version `1.1.0`, `minimumHostVersion` `0.3.1`,
  `hostKeys: ["ArrowDown","ArrowUp","Primary+N"]`; SDK types for `onHostKey`,
  `PluginPanelHostKeyEvent`, `requestHide()`, `focusHostInput()` arity.
- **Implementation points:** bump versions; add `hostKeys` in frozen order.
- **Verify:** `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.notes/tests/sdk-contract.ts`

### Task 2: Host key and Escape behavior tests then panel.js

- **Files:** `package/dist/panel.js`, `tests/runtime.test.js`
- **Dependencies:** Task 1
- **Distinct test coverage:**
  - Exactly one `onHostKey` registration; second register not present in source.
  - Host ArrowDown/Up moves selection; Host `n` opens new dialog; dialog open →
    host keys no-op; host handler returns without awaiting dialogs.
  - Escape: ordinary dialog → sync `preventDefault` + cancel; dirty → sync
    `preventDefault` + unsaved; cancel keeps visible (no hide); save/discard →
    one `requestHide()`; clean Escape → no `preventDefault`.
  - Ctrl+F unchanged; no `invoke` / search box.
- **Implementation points:** register `onHostKey` before `onUpdate`; fire-and-forget
  navigation/new; Escape sync arbitration; Escape-unsaved path calls
  `requestHide()` once after save/discard.
- **Verify:**  
  `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.notes/tests/runtime.test.js`

### Task 3: Package validate and ship commit

- **Files:** none beyond Task 1–2 leftovers / README QA steps
- **Dependencies:** Task 2
- **Distinct test coverage:** CLI validate on Windows package path.
- **Implementation points:** record `SOURCE_INVALID` verbatim if CLI fails on
  Windows dir read; do not bypass via copy/delete icon/Host edits.
- **Verify:** full command set from the user prompt + `git diff --check` + local
  commit (notes-only).
