# UiPilot Public Plugin Demo

This fixture is an independently removable public plugin. UiPilot contains no host-side `/demo` implementation.

## Window Mode

The checked-in package uses `submit + window` and requests only `ui.window`.

1. In UiPilot's public plugin panel, choose **Development directory**.
2. Select the `package` directory beside this README.
3. Confirm the `ui.window` permission.
4. Run `/demo str` and press Enter.

The content page displays the input, platform, theme, singleton instance number, and `str yyyy-mm-dd`. Pin, close, drag, and position restore belong to the host shell, not the content page.

## Main-Result Mode

Output mode is static. Before reloading the development plugin:

1. Change `command.outputMode` in `package/plugin.json` to `mainResult`.
2. Remove the top-level `window` member.
3. Replace `permissions` with `["clipboard.write"]`.
4. Change `OUTPUT_MODE` in `package/dist/runtime.js` to `mainResult`.
5. Reload the plugin and confirm the changed permission.

The first Enter publishes one plain-text result. The second Enter runs its host-owned `copyText` default action. Reverse those four edits to return to window mode.

## Verify And Package

```powershell
node --test examples/public-plugins/com.uipilot.demo/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo/tests/sdk-contract.ts
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1
```

The packaging script writes `examples/public-plugins/com.uipilot.demo/com.uipilot.demo.uipilot-plugin` by default. Only `plugin.json` and the four files under `dist/` enter the archive.

See `docs/plugin-sdk/public-plugin-v1.md` for the complete contract.
