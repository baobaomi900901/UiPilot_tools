# Clipboard History Host Capability Manual Acceptance

Scope: verify the UiPilot host-side clipboard history capability for Windows Panel plugins. This checklist intentionally avoids automated mouse or keyboard control.

## Preconditions

- Run UiPilot on Windows with host version `0.3.3` or later.
- Install a Panel plugin that declares `ui.panel`, `clipboard.history.read`, `clipboard.history.paste`, and host key `Enter`.
- Grant both clipboard history permissions during install.
- Keep an external target app open, such as Notepad, WeChat, or Explorer.

## Checks

1. Text capture: copy a normal text value while UiPilot is running and the plugin is enabled; open the plugin panel and confirm the entry appears as a redacted/short preview, not the complete raw text when long.
2. Image capture: copy an image; confirm the panel receives an image summary/thumbnail and does not receive original PNG bytes.
3. File-list capture: copy one or more files in Explorer; confirm the panel receives file count, first file name, and availability only, not full paths.
4. Paste admission: from an external text target, show UiPilot, open the panel, select an entry, and trigger the plugin's declared Enter host key. UiPilot should hide, focus should return to the external target, and one paste should occur.
5. WeChat target: repeat paste admission into an active WeChat input box; verify focus restoration and one paste.
6. Missing file: capture a file, delete or move it, then attempt paste. The plugin should receive `RecordUnavailable`, with no paste and no sensitive path in the error.
7. Permission revoke/disable: disable the plugin or revoke clipboard history permission, copy new content, then re-open the panel. New captures should stop for that plugin.
8. Restart persistence: restart UiPilot and re-open the authorized plugin. Existing retained entries should load from the plugin-isolated local store.
9. Session safety: try calling paste without a routed Enter `routeSequence`, with an old `routeSequence`, or after the panel is closed. The plugin should receive `ExpiredPanelSession`.

Expected fixed paste error names: `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, `PasteTargetUnavailable`, `ClipboardWriteFailed`.
