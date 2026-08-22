# UiPilot Pomodoro Plugin

This independently removable example demonstrates the host-owned public plugin timer. UiPilot contains no built-in `/pomodoro` command.

## Install And Use

1. In UiPilot settings, open **Plugins** and choose the development-directory installer.
2. Select the `package` directory beside this README.
3. Confirm `ui.window`, `notifications.publish`, and `timer.control`.
4. Run `/pomodoro` or `/pomodoro reminder text`, then press Enter.
5. Press **Start** in the plugin window. Closing or hiding the window does not stop the host timer.

The example displays `00:10` before the first round, but it does not start until the user presses Start. Pause preserves the remaining time, Resume continues the same round, and Reset returns to the round duration without starting. Completion is saved to UiPilot's message center before the host loops the plugin-owned `package/assets/sounds/timer-alarm.wav`. Opening the UiPilot main window stops the alarm without marking the completion message as read.

`timer.control` packages must contain exactly one alarm at that fixed path. UiPilot validates the complete PCM WAV during installation, keeps its bytes private from Runtime and window code, and plays the frozen bytes from memory. There is no host alarm fallback and no configurable audio path. Reinstall this example after changing the package or upgrading from the earlier package version.

The timer is process-local. Exiting UiPilot discards active rounds without recovery or replay. Disabling, uninstalling, fault-disabling, or updating the plugin cancels its current generation timer. Hiding the plugin window only revokes that window's control session; it does not cancel the timer.

## Verify

```powershell
node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts
```

See `docs/plugin-sdk/public-plugin-developer-guide.md` for the complete timer API and revision-merge contract.
