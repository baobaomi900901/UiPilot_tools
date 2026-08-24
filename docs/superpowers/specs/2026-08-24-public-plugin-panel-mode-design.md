# Public Plugin Panel Mode Design

**Date:** 2026-08-24  
**Status:** Revised draft — P1/P2 closed; awaiting Codex re-review  
**Related:** `docs/plugin-sdk/public-plugin-v1.md`, `src-tauri/src/plugin_window.rs`, `src-tauri/src/public_plugins/manifest.rs`, `src/launcher-core.ts`  
**Supersedes:** earlier open questions that left webview placement, R1/R2, manifest names, and list-Enter behavior undecided.

## 1. Goal

Add a third public-plugin output shape that keeps the user inside the main launcher:

1. Activate a panel plugin (example command `/bbm`).
2. The main input shows a single command **tag** for that plugin.
3. The area below the input becomes an embedded plugin HTML surface.
4. Text after the tag is submitted to the panel on Enter.

This is **not** an independent window and **not** a `mainResult` list.

## 2. Non-goals

- Multiple concurrent tags or multi-plugin panel stacks.
- Persisting a panel session across main-window hide/show.
- Reusing or overloading `mainResult` to carry HTML/webview payloads.
- Giving panel content pin/drag/close ownership of the main launcher chrome.
- Background panel work after the main window is dismissed (session ends with dismiss).
- In-session switching to another command without first clearing the tag (MVP).
- Panel access to timer APIs in v1 (`timer.control` remains window-only).

## 3. Human–computer interaction

### 3.1 Entry

Either path activates the same session:

- Type `/bbm` (or the user-renamed command) and press Enter → **one Enter** opens the panel.
- Select the panel plugin row in the main result list and press Enter → **one Enter** opens the panel (does **not** go through ordinary plugin command completion).

Preconditions:

- Plugin declares `outputMode: "panel"`.
- Activation is submit-only (`activationMode: "submit"`).

On success:

- Main input shows a **command tag** labeled with the plugin’s effective command name (e.g. `bbm`).
- Focus moves to the argument suffix field.
- **Caret placement:** if the initial argument is empty, caret at `0`; if non-empty, caret at **end of suffix text** (so continued typing appends rather than prepends).
- The main result list is replaced by the plugin panel surface.
- Exactly one panel session exists host-wide.

Initial argument:

- **Slash submit** `/bbm` or `/bbm hello` → argument is the text after the command token (`""` or `"hello"`).
- **List `panelActivation` row** — depends on how the row was reached:
  - **Default / command discovery** (empty launcher home, favorites list, or slash-prefix search such as `/bb` with no free-text query): `initialArgument` is `""`.
  - **Plain-text match** (user typed free text such as `hello` and the selected row is a panel plugin matched from that query): `initialArgument` is the current launcher query string at activation time (e.g. `"hello"`).

The `panelActivation` payload must carry `initialArgument` explicitly so the host does not discard the user’s typed query when opening from a text match.

### 3.2 Command tag and input chrome

Implementation contract (a11y / editing):

- **Host-owned tag chrome** + **separate suffix `<input>`** (not one string with a synthetic non-editable prefix).
- Tag shows command label and × close control.
- Suffix input holds only the argument text.
- Screen readers: tag exposed as a discrete element (e.g. group/label “command bbm”); suffix input has its own accessible name such as “bbm argument”.

Close / clear:

- Click × → exit panel session → fresh empty launcher.
- In the suffix input, Backspace deletes the tag **only when** `selectionStart === selectionEnd === 0`.
- Any non-empty selection, or caret not at 0, Backspace edits argument text only.
- Home moves caret to 0 in the suffix (does not jump “into” the tag).
- Ctrl+A selects all suffix text only (not the tag).
- Paste inserts into suffix only.
- IME composition is confined to the suffix input; composing Backspace does not clear the tag.

While the tag is present:

- Free text is typed only in the suffix.
- Suffix text submits to the panel only on **Enter** (not live-on-keystroke).

### 3.3 Panel surface

- Occupies the same visual region normally used by the main result list.
- Renders plugin-authored HTML in an isolated child webview (see §5.1).
- Host owns outer launcher chrome. Panel content does not draw a window frame.

### 3.4 Submit while in session

- First Enter opens the session with the initial argument from §3.1.
- Later Enter sends the current suffix string as a new panel submission.
- Empty argument on later Enter is allowed (plugin decides no-op vs clear vs refresh).

### 3.5 Escape and focus loss

- **Escape** keeps today’s launcher behavior: hide the main window.
- Any main-window hide (Escape, blur-hide, tray hide, etc.) **discards** the panel session.
- Next show is a **fresh initialized launcher**: no tag, no panel webview, default empty/home results.

### 3.6 Exclusivity and command switching (MVP)

- Only one tag / panel session at a time.
- **While a tag is present, the user cannot activate another plugin or ordinary result** from the launcher: the result list is hidden and all typed text is the current panel argument.
- To run anything else, the user must clear the tag first (× or Backspace-at-0).
- Host may still **force-end** the session on plugin disable / uninstall / upgrade / replacement by another host-driven panel activation path if one is added later. MVP has no in-UI “replace with other plugin” path without clearing the tag.

### 3.7 HCI acceptance checklist

- `/bbm` + Enter shows tag + panel in one step; `/bbm hello` leaves suffix `hello` with caret at end.
- List select panel plugin + Enter shows the same in one step (no completion intermediate).
- Typing `hello` and selecting a matched panel plugin preserves `hello` as initial suffix with caret at end.
- Default/favorite list entry opens with empty suffix and caret at `0`.
- Typing after tag + Enter updates panel with that text.
- × closes to empty launcher.
- Backspace only at suffix caret 0 with empty selection removes tag.
- IME / Ctrl+A / paste never delete the tag accidentally.
- Hide via Escape/blur; show again → empty launcher.
- Never two tags at once.
- No way to invoke another command without clearing the tag first.

## 4. SDK and manifest contract (frozen)

### 4.1 Output mode

`PublicOutputMode` becomes:

- `mainResult`
- `window`
- `panel`

### 4.2 Manifest fields

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "minimumHostVersion": "<host-version-that-ships-panel>",
  "command": {
    "activationMode": "submit",
    "outputMode": "panel",
    ...
  },
  "panel": {
    "entry": "dist/panel.html"
  },
  "permissions": ["ui.panel"]
}
```

Frozen names:

- Top-level object: `panel`
- Entry field: `panel.entry` (HTML path; same path validation style as `window.entry`)
- Permission string: `ui.panel`

Illegal combinations (package validation must fail closed):

| Claim | Required | Forbidden |
|-------|----------|-----------|
| `outputMode: panel` | `activationMode: submit`, `panel.entry`, permission `ui.panel` | `window`, `ui.window`, `timer.control`, returning main-result lists as primary response |
| `outputMode: window` | existing window rules | `panel`, `ui.panel` |
| `outputMode: mainResult` | existing mainResult rules | `panel`, `window`, `ui.panel`, `ui.window` |

`timer.control` remains **window-only** and still requires `ui.window` + `notifications.publish` + `submit` + `window` as today. Panel packages cannot declare it in v1.

### 4.3 Version evolution

- Keep `schemaVersion: 1` and `apiVersion: 1` (additive enum/field under the existing v1 package schema).
- Panel-capable hosts accept old packages that omit `panel`.
- Old hosts reject panel packages via `deny_unknown_fields` / unknown `outputMode` / unknown permission (fail closed).
- Every panel package **must** set `minimumHostVersion` to the first released host version that implements panel mode.
- Ship in lockstep: JSON schema, `uipilot-plugin-api-v1.d.ts`, plugin CLI allowlist/validator, resource path rules, and host manifest validation.

### 4.4 Runtime response discriminant

Extend host response enum alongside existing variants:

- `MainResults(...)`
- `Window(...)`
- `Panel(...)`

`Panel` carries opaque JSON `data` within the existing serialized response budget (same ceiling as window responses). It must not encode result lists.

### 4.5 Permission semantics

`ui.panel` grants only:

- Host may create/mount this plugin’s panel content webview into the launcher panel slot.
- Panel bridge APIs defined in §5.4.

It does **not** grant detached windows, timer control, or notification publish.

## 5. Host architecture (frozen)

### 5.1 Webview isolation

**Frozen choice:** host-managed, **independently labeled child Webview** composited into the main window’s result slot.

Required properties (mirror plugin-window content isolation):

- Distinct webview label (e.g. `plugin-panel-content-*`), never the main launcher webview label.
- **Forbidden:** `<iframe>`, `srcdoc`, `innerHTML` injection of plugin HTML into the main webview, or navigating the main webview to plugin content.
- Load via the existing public-plugin custom protocol (`uipilot-public-plugin` / equivalent), not `file://` and not arbitrary https.
- Navigation allowlist equivalent to plugin window content (`on_navigation` host/scheme checks; deny unknown hosts/ports).
- Deny new windows and downloads.
- Apply the public plugin CSP used for isolated plugin content (`frame-src 'none'`, no unexpected connect targets, etc.).
- Tauri/command **caller guards** must reject panel content labels for any command not explicitly allowlisted for panel content (same posture as plugin-window content vs shell).
- Untrusted UI remains outside launcher DOM; launcher React/DOM must not read plugin DOM.

### 5.2 Identity sequences (must not be conflated)

Maintain **two independent monotonic sequences**:

1. `PanelSessionIdentity` / `sessionEpoch`  
   - Created when a panel session opens.  
   - Bumped whenever the session is torn down or replaced.  
   - Identifies the mounted webview generation and tag chrome.

2. `requestId` + `submissionToken` per Enter  
   - Created for every open/submit Enter.  
   - Scheduled through the existing public-plugin submission/latest-wins machinery.  
   - Bound to the current `sessionEpoch` at creation time.

Late completion rule:

- A Runtime response is applied only if its `submissionToken` is still the accepted token **and** its bound `sessionEpoch` equals the live session epoch.
- If the user submits B while A is in flight, hide the window, clear the tag, or replace/end the session, then A’s result is discarded (no UI mutation, no panel update).

### 5.3 Runtime channel for subsequent Enter

**Frozen choice: R1.**

- Every Enter (including the first) invokes plugin `onCommand` as a normal submit activation.
- Host passes **argument-only** input (command token already consumed by the tag).
- First response must be `Panel` and is used to mount/update the session.
- Later responses must also be `Panel` for that plugin generation; unexpected `MainResults` / `Window` fail the submission without changing launcher mode.

### 5.4 Linearization

Ordered pipeline for one session:

1. Create/bump `sessionEpoch`; show tag; create child webview (inert until ready).
2. Content ready / ack (same class of ready handshake as plugin window content).
3. Accept panel submissions only after ready for that `sessionEpoch`.
4. While not ready: buffer **at most one** latest pending argument (latest Enter wins); older pending args are dropped.
5. On accepted submission: dispatch Runtime with new `requestId`/`submissionToken` bound to `sessionEpoch`.
6. On success: fan out update to panel bridge for that epoch.
7. On hard failure (load error, invalid response, webview crash): roll back to empty launcher (clear tag, destroy webview, restore results) and surface host error via existing toast/message patterns.
8. Teardown always destroys the child webview and increments/drops `sessionEpoch` so in-flight work cannot revive it.

### 5.5 Panel content bridge

Frozen name: `window.uipilotPluginPanel` (parallel to `uipilotPluginWindow`).

Minimum surface:

- `onUpdate(handler)` — theme, argument payload, session identity metadata needed by content.
- `storage` — same plugin storage namespace/rules as window storage (no extra permission beyond existing storage policy for installed plugins).

Out of scope for v1 bridge:

- `close()` of the launcher
- timer APIs
- notifications publish/schedule
- arbitrary Tauri invoke

### 5.6 Launcher activation type

Add a dedicated launcher activation discriminant for inventory/search rows of panel plugins, e.g. `panelActivation`:

- One Enter calls host “open panel” directly.
- **Must not** route through `pluginCompletion` → `applyPluginCompletion` → armed second Enter (`commitArmedPluginCompletion`).
- Payload includes `initialArgument: string` computed per §3.1 (empty for default/command discovery; current free-text query for plain-text match).
- Window plugins keep today’s completion behavior; only `outputMode: panel` rows use `panelActivation`.

### 5.7 Teardown triggers

Tear down session + destroy child webview when:

- Tag × clicked
- Tag removed via Backspace-at-0
- Main window hide (Escape, blur-hide, tray hide, etc.)
- Plugin disabled / uninstalled / upgraded
- Session load/submit hard-failure rollback

## 6. Failure behavior (HCI-visible)

- Activation validation failure: no tag; keep current input/results; existing host error pattern.
- Panel content load / ready failure: revert to empty launcher + error toast/message-center; never leave a tag without a live surface.
- Submit while not ready: keep only the latest Enter argument; do not build a multi-item queue; do not silently ignore forever—dispatch that latest argument once ready, unless session ended first.
- Superseded Runtime results: no visible effect.

## 7. Testing focus

### HCI / product

- One-Enter open from slash and from list (`panelActivation`).
- Argument submit on Enter only.
- Tag close via × and Backspace-at-0 only.
- IME / selection / Ctrl+A / paste do not clear tag.
- Hide then show yields fresh launcher.
- No second command without clearing tag.

### Technical

- Child webview label isolation; no main-webview navigation to plugin HTML.
- CSP / protocol / navigation / download / new-window denials.
- Command caller guards reject non-allowlisted invokes from panel labels.
- `sessionEpoch` vs `submissionToken` independence; late A discarded after B/hide/teardown.
- Manifest matrix: legal panel package; illegal panel+window, panel+live, panel+timer.control, mainResult+ui.panel.
- Schema/CLI/type lockstep fixtures.
- Scheduler latest-wins with epoch binding.

## 8. Resolved decisions (was open / P1–P2)

| ID | Topic | Frozen decision |
|----|-------|-----------------|
| P1 | Webview isolation | Host-managed independently labeled **child Webview**; forbid iframe/DOM injection into main webview; protocol + navigation + CSP + caller guards |
| P1 | Runtime ownership | Separate `sessionEpoch` and per-Enter `requestId`/`submissionToken`; **R1** re-invoke `onCommand` each Enter; late results epoch/token gated; not-ready = single latest buffer |
| P1 | Manifest / permission | `panel.entry` + `ui.panel`; submit-only; mutex with window/timer; schema/api stay v1; panel packages bump `minimumHostVersion` |
| P1 | List Enter | New `panelActivation`; one Enter opens panel; bypasses plugin completion arming |
| P2 | Replace other plugin | MVP: clear tag first; no in-session replace path |
| P2 | Tag editing / a11y | Host tag + separate suffix input; Backspace clears tag only at `selectionStart === selectionEnd === 0` |

## 9. Summary

| Topic | Decision |
|-------|----------|
| Output mode | New `panel` |
| UI | Command **tag** + isolated embedded HTML |
| Arguments | Suffix text; submit on Enter |
| Concurrency | One session; clear tag before other commands |
| Escape / hide | Hide window; discard session; fresh show |
| Close | × or Backspace-at-0 |
| Isolation | Child webview, not main DOM |
| Runtime | R1 + dual identity sequences |

HCI owners re-validate §3 and §6. Codex re-reviews §4–§5 and §8; if accepted, this draft can move to implementation planning.
