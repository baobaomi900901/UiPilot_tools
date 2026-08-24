# UiPilot Demo Window Plugin

This independently removable public plugin demonstrates UiPilot's host-owned singleton child window. UiPilot contains no host-side `/demo-win` implementation.

## Install And Use

1. In UiPilot's public plugin panel, choose **Development directory**.
2. Select the `package` directory beside this README.
3. Confirm the `ui.window` and `notifications.publish` permissions.
4. Run `/demo-win str` and press Enter.

The Runtime asks UiPilot to publish `str yyyy-mm-dd` after 10 seconds, then immediately opens the content window with the same return text. Scheduling is Windows-only, request-bound, process-local, and limited to one notification action per command request. Hiding either window does not cancel an accepted task; disabling, uninstalling, or updating the plugin does, and pending tasks are lost when UiPilot exits. Pin, close, drag, focus transfer, and position restore belong to the host shell, not the content page.

## Verify And Package

```powershell
node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-win/tests/sdk-contract.ts
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-win
```

The packaging script writes `examples/public-plugins/com.uipilot.demo-win/com.uipilot.demo-win.uipilot-plugin` by default. Only `plugin.json` and the four files under `dist/` enter the archive.

See `docs/plugin-sdk/public-plugin-v1.md` for the complete contract.
