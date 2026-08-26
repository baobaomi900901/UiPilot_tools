# Public Plugin Demo Panel

Minimal `submit + panel` reference for UiPilot 0.3.0 and later.

- Command: `/demo-panel`
- Output: host-managed embedded child WebView
- Permission: `ui.panel`
- Runtime: returns `{ requestId, data }` on every Enter
- Content bridge: `window.uipilotPluginPanel.onUpdate`, `storage`, and no-argument `focusHostInput()`
- Focus: the host focuses the tagged argument input on entry; Ctrl+F in panel content returns focus to it

Validate the package from any Node.js 20+ environment with the public CLI:

```powershell
uipilot-plugin validate .\package --platform windows
```

Run the standalone example tests:

```powershell
node --test .\tests\runtime.test.js
```
