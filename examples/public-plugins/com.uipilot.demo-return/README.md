# UiPilot Demo Return Plugin

This independently removable public plugin demonstrates an asynchronous main-window result and a host-owned copy action. UiPilot contains no host-side `/demo-return` implementation.

## Install And Use

1. In UiPilot's public plugin panel, choose **Development directory**.
2. Select the `package` directory beside this README.
3. Confirm the `clipboard.write` permission.
4. Run `/demo-return str` and press Enter.

The main window remains open while the plugin runs. The first Enter publishes and selects the single `str yyyy-mm-dd` result. Press Enter again to copy exactly that text through the host-owned `copyText` action.

## Verify And Package

```powershell
node --test examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-return/tests/sdk-contract.ts
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-return
```

The packaging script writes `examples/public-plugins/com.uipilot.demo-return/com.uipilot.demo-return.uipilot-plugin` by default. Only `plugin.json` and `dist/runtime.js` enter the archive.

See `docs/plugin-sdk/public-plugin-v1.md` for the complete contract.
