# Host Fix Request: Forward Enter To Panel When List Selection Is Active

**Date:** 2026-08-26  
**Status:** Open — Host implementation required  
**Requested by:** Third-party plugin developer (`com.uipilot.notes`)  
**Related:** `docs/superpowers/specs/2026-08-26-public-plugin-panel-key-routing-and-hide-design.md`  
**Host version:** 0.3.1+  
**Plugin workaround:** Notes 1.1.2 refocuses list after Save; copy+hide requires focus in panel content WebView

## Problem

Notes expects **list Enter** → copy selected note body → `requestHide()`.

This works when keyboard focus is inside the **panel content WebView** (e.g. `#note-list`).

It fails when the user navigates the list via **Host-delivered** `ArrowDown` / `ArrowUp`
while focus remains on the **tagged main input**:

1. User opens `/notes`.
2. Focus stays on launcher suffix input (common case).
3. `ArrowDown` updates list selection through `onHostKey` (works).
4. User presses **Enter** expecting copy + hide.
5. Launcher handles Enter as **`submitPanel`** (panel argument submit), **not** panel copy.
6. Plugin may show stale status (e.g. earlier **「已保存」**) and launcher stays visible.

Panel JavaScript never receives Enter; `requestHide()` is not called.

## Required Host behavior (choose one)

### Option A — Focus transfer (preferred if already intended)

After delivering `ArrowDown` / `ArrowUp` host keys to panel content, Host **must**
ensure panel list (or declared focus target) receives native keyboard focus before
the next key event, so subsequent Enter is handled inside the content WebView.

Verify on Windows: `noteList.focus()` from panel bootstrap succeeds and is not
immediately pulled back to the main input after `plugin_panel_host_key_ack`.

### Option B — Declare `Enter` as optional host key

Extend frozen `PanelHostKeyDeclaration` with `"Enter"` (or `"Primary+Enter"` if
modifier semantics needed). When declared in manifest `panel.hostKeys`:

- Main-input Enter is routed through `onHostKey` (same serial ack path as arrows).
- Enter does **not** call `submitPanel` while the declaration is active and session matches.

Notes would opt in and handle `{ key: 'Enter' }` by copying + `requestHide()`.

## Acceptance criteria

- [ ] With notes 1.1.2+, user can ArrowDown in main input → Enter → copy + hide without clicking panel.
- [ ] Enter in main input without list navigation still submits panel argument (when Enter is not declared or not consumed by handler).
- [ ] Documented in public plugin developer guide.

## Out of scope

- Notes plugin cannot intercept main WebView Enter without Host routing.
- Unrelated to `PanelHideAdmitResult` serialization (fixed in `c2ff520`).
