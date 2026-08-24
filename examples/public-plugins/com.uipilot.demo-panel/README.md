# Public Plugin Demo Panel

Minimal `submit + panel` reference for UiPilot 0.3.0 and later.

- Command: `/demo-panel`
- Output: host-managed embedded child WebView
- Permission: `ui.panel`
- Runtime: returns `{ requestId, data }` on every Enter
- Content bridge: `window.uipilotPluginPanel.onUpdate` and `storage` only

Validate the package from any Node.js 20+ environment with the public CLI:

```powershell
uipilot-plugin validate .\package --platform windows
```

Run the standalone example tests:

```powershell
node --test .\tests\runtime.test.js
```
