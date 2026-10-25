# Settings application version

User request: show the installed application's version in Settings, such as 0.7.0. Existing autonomous authorization covers this small Host UI change; manual acceptance remains primary.

Display a quiet, noninteractive “版本 <version>” at the right of the shared Settings header, visible on General, Messages and Plugins. Use 12px semantic muted text and preserve header height, tab navigation and keyboard focus. It identifies the Host, not the selected plugin.

Read the running application's metadata through the existing Tauri getVersion API once per Settings mount. Do not hardcode 0.7.0 or put it in user settings. While pending show “版本 读取中…”; on failure show “版本 无法读取”, never a guessed version or raw error. Ignore completion after unmount. Grant only core:app:allow-version to the existing exact main WebView capability; no plugin permission expansion.

Alternatives considered: build-time package version risks divergence from native overrides; a new backend command duplicates Tauri metadata. Use the existing native metadata API.

Validation: existing focused Settings navigation tests, frontend type/build checks and scoped permission inspection. User checks actual 0.7.0 text, all three tabs and Light/Dark. No new tests for this reversible display-only change; no Host launch by the agent. Deliver a new standalone acceptance directory with unchanged acceptance identity and preserve older kits. Existing recovery cases remain pending.