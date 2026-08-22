# @uipilot/plugin-cli

Validate a UiPilot public plugin directory or `.uipilot-plugin` archive without installing UiPilot or Rust.

Install the supplied npm tarball, then run:

```text
uipilot-plugin validate <source> [--platform windows|macos] [--json]
```

The package requires Node.js 20 or newer. Validation is read-only: it does not install the plugin, execute Runtime/window code, contact UiPilot, use the network, or modify the selected source.

Exit codes are `0` for valid, `1` for an invalid/unsafe/incompatible package, and `2` for invalid CLI usage or an internal CLI failure. Use `--json` for the stable `PluginValidationReportV1` response.

`timer.control` is accepted only for a Windows `submit + window` plugin with a window entry, `ui.window`, `notifications.publish`, and the fixed validated `assets/sounds/timer-alarm.wav` resource.

The package is not yet published to the npm Registry. Registry installation instructions will be added only after publication.
