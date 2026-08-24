# Public Plugin Panel Mode Design

**Date:** 2026-08-24  
**Status:** Draft — HCI pending user review; technical contract pending Codex review  
**Related:** `docs/plugin-sdk/public-plugin-v1.md`, `outputMode: mainResult | window`

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

## 3. Human–computer interaction (user-validated)

### 3.1 Entry

Either path activates the same session:

- Type `/bbm` (or the user-renamed command) and press Enter.
- Select the `bbm` row in the main result list and press Enter.

Preconditions:

- Plugin declares `outputMode: "panel"`.
- Activation is submit-only (`activationMode: "submit"`).

On success:

- Main input replaces the typed slash command with a **command tag** labeled with the plugin’s effective command name (e.g. `bbm`).
- Focus remains in the main input, caret after the tag, ready for optional arguments.
- The main result list is replaced by the plugin panel surface.
- Exactly one panel session exists host-wide.

### 3.2 Command tag

Visual:

- Chip/tag in the input leading edge showing the command name.
- Tag includes a small close affordance (×).

Close / clear:

- Click × on the tag → exit panel session → return to a fresh empty main launcher state (empty input, default result surface).
- With caret at the start of the free-text region (immediately after the tag), Backspace deletes the tag and exits the session the same way.
- Deleting characters inside the argument text does **not** remove the tag until the caret reaches the tag boundary as above.

While the tag is present:

- The user may type free text **after** the tag.
- That free text is the panel argument / query string.
- Argument text is submitted to the panel only on **Enter** (not live-on-keystroke).

### 3.3 Panel surface

- Occupies the same visual region normally used by the main result list.
- Renders plugin-authored HTML (custom UI), not a host-owned result list.
- Host owns outer launcher chrome (input, tag, window show/hide). Panel content does not draw its own window frame.

### 3.4 Submit while in session

- First Enter (from slash/list) opens the session; initial argument is whatever remained after the command token (may be empty).
- Later Enter while the tag is active sends the current argument string to the active panel as an update/submit.
- Empty argument on later Enter is allowed (plugin decides no-op vs clear vs refresh).

### 3.5 Escape and focus loss

- **Escape** keeps today’s launcher behavior: hide the main window. It does **not** specially “pop” the tag while leaving the window visible.
- When the main window hides because of Escape, blur, or other existing hide paths, the panel session is **discarded**.
- The next time the main window is shown, the UI must be a **fresh initialized launcher**: no tag, no panel webview content, default empty/home results. No restoration of the previous `/bbm` session.

### 3.6 Exclusivity

- Only one tag / panel session at a time.
- Activating another panel plugin while one is open replaces the session (old tag+panel torn down, new tag+panel created).
- Activating `mainResult` / `window` commands while a panel session is open follows host command routing rules; recommended default for v1: exiting panel first is not required if the other command’s normal path would replace the main surface, but panel session must end before a non-panel main surface is shown. Codex should confirm the exact precedence against existing launcher admission rules.

### 3.7 HCI acceptance checklist

- `/bbm` + Enter shows tag + panel.
- List select `bbm` + Enter shows the same.
- Typing after tag + Enter updates panel with that text.
- × closes to empty launcher.
- Backspace from argument head removes tag and closes session.
- Hide via Escape/blur; show again → empty launcher, no tag/panel residue.
- Never two tags at once.

## 4. Product shape in the SDK

### 4.1 New output mode

Extend `PublicOutputMode`:

- `mainResult`
- `window`
- `panel`

Rules:

- `panel` requires `activationMode: "submit"`.
- `panel` requires a panel entry HTML path in the manifest (name TBD in implementation; suggested `panel.entry`).
- `panel` forbids `window` entry and forbids returning `mainResult` item lists as the primary response.
- `mainResult` continues to forbid panel/window entries.
- `window` continues to forbid panel entry.

### 4.2 Permission

Prefer a dedicated permission:

- `ui.panel` — allow the host to mount this plugin’s content into the main launcher panel slot.

Rationale: panel content shares process/UI adjacency with the launcher more tightly than a detached window; do not silently reuse `ui.window` without Codex security review. If Codex prefers reuse for MVP, document the security delta explicitly.

### 4.3 Runtime response (sketch for Codex)

Panel activation response should bind the request to the panel session, carrying opaque JSON `data` within the existing response size budget, analogous to `window`’s `{ requestId, data }` rather than `mainResult` items.

Subsequent Enter submits are new invocations (or a dedicated panel-submit channel). Codex should choose one:

- **Option R1 (preferred sketch):** each Enter is a normal `onCommand` invocation with `activationMode: submit` semantics while session is open; host passes argument-only input (command token already consumed by tag).
- **Option R2:** first open uses `onCommand`; later Enter uses a panel content bridge update without re-entering Runtime.

HCI does not depend on R1 vs R2; Codex picks based on isolation and existing Runtime lifecycle.

### 4.4 Panel content bridge (sketch)

Panel page receives a frozen host bridge similar in spirit to `uipilotPluginWindow`, e.g. `uipilotPluginPanel`:

- `onUpdate(handler)` for theme / argument submit / session identity.
- `storage` sharing plugin storage namespace (same rules as window storage if granted by existing policy).
- No ownership of launcher hide/show, tag chrome, or global shortcuts.

Exact API surface is Codex-owned; HCI only requires: panel can render custom UI and react to Enter-submitted argument text.

## 5. Host architecture (for Codex)

### 5.1 Session object

Host keeps at most one `PanelSession`:

- `pluginId`
- `generation`
- `commandLabel` (effective name for tag)
- `webview` / content label for the embedded surface
- `requestId` / session epoch for update fan-out

### 5.2 Input model

Launcher input enters **PanelTagged** state:

- Prefix: non-editable command tag.
- Suffix: editable argument buffer.
- Serialize for Runtime as argument-only string (not including `/command` prefix).

### 5.3 Mount point

Reuse the main window’s result region as the panel host slot. When session starts, hide/unmount the normal results list; when session ends, restore it.

### 5.4 Teardown triggers

Tear down panel session and destroy/reset embedded content when any of:

- Tag × clicked
- Tag removed via Backspace at boundary
- Main window hide (Escape, blur-hide, tray hide, etc.)
- Plugin disabled / uninstalled / upgraded
- Replaced by another panel activation

After teardown, next show of main window presents initialized launcher state.

### 5.5 Security notes for Codex

- Panel content is untrusted plugin UI adjacent to launcher chrome.
- Apply the same content lockdown posture as plugin windows where applicable (no arbitrary Tauri invoke, navigation allowlist, no extra privileges beyond declared permissions).
- Confirm whether panel webview is a child of the main window or a separately labeled webview composited into the slot.
- Confirm clipboard / notification / timer APIs are out of scope for v1 unless explicitly added.

## 6. Failure behavior (HCI-visible)

- Activation validation failure: no tag, keep current input/results, show existing host error pattern.
- Panel content load failure: show host error in the panel slot or revert to empty launcher; do not leave a zombie tag without a surface. Preferred HCI: revert to empty launcher + error toast/message-center per existing patterns.
- Submit while content not ready: ignore or queue one latest Enter; Codex chooses; HCI preference is “latest Enter wins,” no multi-queue.

## 7. Testing focus

### HCI / product

- Entry from slash and from list.
- Argument submit on Enter only.
- Tag close via × and Backspace.
- Hide then show yields fresh launcher.
- Single-session exclusivity.

### Technical (Codex / implementation)

- Manifest schema + permission gating.
- Session teardown on all hide paths.
- No leaked webview after hide.
- Contract tests forbidding `panel` + `live`, `panel` + `window` entry, etc.

## 8. Open questions for Codex

1. Final manifest field names (`panel.entry` vs reuse `window.entry` with mode switch — design recommends separate `panel.entry`).
2. New permission `ui.panel` vs reuse `ui.window`.
3. Runtime channel for subsequent Enter (R1 vs R2 above).
4. Embedded webview implementation choice under Tauri multi-webview constraints on Windows.
5. Precedence when a panel session is open and user selects a non-panel result/command.

## 9. Summary

| Topic | Decision |
|-------|----------|
| Output mode | New `panel` (not a `mainResult` retrofit) |
| UI | Command **tag** + embedded custom HTML below input |
| Arguments | Text after tag; submit on Enter |
| Concurrency | One tag/session globally |
| Escape | Hide main window (existing behavior) |
| After hide/show | Fresh initialized launcher; no session restore |
| Close | Tag × or Backspace-at-tag-boundary |

HCI owners validate §3 and §6. Codex reviews §4–§5 and §8 before implementation planning.
