# UiPilot Demo Window Plugin

This independently removable public plugin demonstrates UiPilot's host-owned singleton child window. UiPilot contains no host-side `/demo-win` implementation.

## Install And Use

1. In UiPilot's public plugin panel, choose **Development directory**.
2. Select the `package` directory beside this README.
3. Confirm the `ui.window` and `notifications.publish` permissions.
4. Run `/demo-win str` and press Enter.

The Runtime first publishes `str yyyy-mm-dd` to UiPilot's message center, then opens the content window with the same return text. Notification publishing is Windows-only, request-bound, and limited to one message per command request. Pin, close, drag, focus transfer, and position restore belong to the host shell, not the content page.

## Verify And Package

```powershell
node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-win
```

The packaging script writes `examples/public-plugins/com.uipilot.demo-win/com.uipilot.demo-win.uipilot-plugin` by default. Only `plugin.json` and the four files under `dist/` enter the archive.

See `docs/plugin-sdk/public-plugin-v1.md` for the complete contract.
