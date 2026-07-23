# Plugin Management Settings Verification

Date: 2026-07-23

## Scope

- Worktree: `D:\code\UiPilot_tools\.worktrees\plugin-management-settings`
- Branch: `codex/plugin-management-settings`
- `main` baseline and merge base: `f1afa89fedc48b9ace1c5e61f821e2fcb71d6749`
- GUI was not started during automated verification. Manual acceptance remains required before merge.

## Automated Results

- `npm test -- --run`: 3 files, 119 tests passed.
- `npm run build`: TypeScript check and Vite production build passed. Vite reported only the existing advisory that the main bundle exceeds 500 kB.
- `cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check`: passed.
- `cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings`: passed.
- `cargo test --manifest-path .\src-tauri\Cargo.toml --quiet`: 367 passed, 2 ignored, 0 failed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-security-config.ps1`: passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-start-menu-boundary.ps1`: passed, including the ignored junction-boundary test.

The file-search performance/evidence and SystemIndex scripts were not run. They require feature-specific artifact roots, release executables, sentinels, or manual performance phases and do not exercise plugin management.

## Covered Contracts

- Removed Research ID and validation/rescan/export/clear surfaces remain absent from production code and permissions.
- Plugin inventory accepts only the exact `id`, `version`, `trigger`, and nullable `description` DTO.
- `README.md` is root-only, non-reparse, UTF-8, and limited to 16 KiB; invalid descriptions do not disable plugins.
- Routes, pending requests, results, clipboard actions, WebView labels, and runtime ownership carry a generation.
- Old queries cannot publish after a plugin-domain commit; already-resolved old copy actions cannot cross a generation switch.
- Runtime callbacks dynamically resolve staged, active, or absent ownership. Promotion removes staged ownership atomically.
- Reload has a fixed 500 ms readiness deadline and rolls back staged resources without replacing the active plugin.
- Delete pins and verifies the package with a no-follow Windows handle, then atomically moves it to same-volume quarantine without overwrite before removing active state.
- Settings list ownership is independent from settings drafts and row mutations. Stale list responses are dropped; stale reload/delete completion triggers a current-epoch authoritative list reconciliation.
- Markdown renders only headings, paragraphs, lists, emphasis, and code. Links, images, and raw HTML do not become interactive/rendered elements.

## Manual Acceptance

1. Stop any other UiPilot dev process so port `1420`, the global hotkey, and the app single-instance identity are free.
2. Install the example package:

   ```powershell
   cd D:\code\UiPilot_tools\.worktrees\plugin-management-settings
   $pluginRoot = "$env:APPDATA\com.uipilot.launcher\plugins"
   New-Item -ItemType Directory -Force $pluginRoot | Out-Null
   Remove-Item -Recurse -Force "$pluginRoot\internal.math" -ErrorAction SilentlyContinue
   Copy-Item -Recurse -Force .\examples\plugins\internal.math $pluginRoot
   npm run tauri dev
   ```

3. Open Settings. Confirm the plugin section shows `internal.math`, version, `/math`, rendered README description, Reload, and Delete.
4. Remove `README.md` from the installed package, click Reload, and confirm the plugin remains active while Settings shows the fallback description. Restore the package afterward.
5. Click Reload on an unchanged valid package. Confirm the row finishes without error, then enter `/math 1+1`; the panel must show `2`, and Enter must copy `2`.
6. While the app is running, make installed `plugin.json` invalid and click Reload. Confirm the row reports a fixed reload error and the already-active `/math 1+1` still computes/copies `2`. Restore a valid package before continuing.
7. Click Delete and cancel the confirmation. Confirm the package and `/math` remain available.
8. Click Delete and confirm. Confirm `%APPDATA%\com.uipilot.launcher\plugins\internal.math` no longer exists and `/math 1+1` no longer routes in the current process.
9. Restart the dev process and confirm the deleted plugin does not return. Quarantine byte cleanup is best effort; the contract is that the original plugin-root path disappears and cannot be loaded.
10. During a slow Reload/Delete test, leave Settings and reopen it. Confirm the final list automatically matches the backend and no old row error or old version overwrites the current view.
11. Edit hotkey/autostart while plugin list loading or a row mutation is pending. Confirm plugin activity does not reset the ordinary settings draft.
