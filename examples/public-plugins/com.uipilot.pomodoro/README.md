# UiPilot Pomodoro Plugin

This independently removable example demonstrates the host-owned public plugin timer. UiPilot contains no built-in `/pomodoro` command.

## Install And Use

1. In UiPilot settings, open **Plugins** and choose the development-directory installer.
2. Select the `package` directory beside this README.
3. Confirm `ui.window`, `notifications.publish`, and `timer.control`.
4. Run `/pomodoro` or `/pomodoro reminder text`, then press Enter.
5. Choose 10, 15, 25, 30, or 45 minutes in the top-right selector, then press **Start**. Closing or hiding the window does not stop the host timer.

The selector defaults to 10 minutes and persists through window reopen, UiPilot restart, plugin upgrade, and retain-data reinstall. Changing it while a round is running or paused affects only the next round. A save failure restores the last stored value or 10 minutes. Pause preserves the remaining time, Resume continues the same round, and Reset returns to idle without starting. Completion is saved to UiPilot's message center before the host loops the plugin-owned `package/assets/sounds/timer-alarm.wav`. Opening the UiPilot main window stops the alarm without marking the completion message as read.

`timer.control` packages must contain exactly one alarm at that fixed path. UiPilot validates the complete PCM WAV during installation, keeps its bytes private from Runtime and window code, and plays the frozen bytes from memory. There is no host alarm fallback and no configurable audio path. Version `1.2.0` adds the persisted duration selector, so reinstall this example after changing the package or upgrading from an earlier package version.

The timer is process-local. Exiting UiPilot discards active rounds without recovery or replay. Disabling, uninstalling, fault-disabling, or updating the plugin cancels its current generation timer. Hiding the plugin window only revokes that window's control session; it does not cancel the timer.

## Verify

```powershell
node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.pomodoro/tests/sdk-contract.ts
```

See `docs/plugin-sdk/public-plugin-developer-guide.md` for the complete timer API and revision-merge contract.
