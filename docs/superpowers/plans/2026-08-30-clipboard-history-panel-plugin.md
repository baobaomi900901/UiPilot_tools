# Clipboard History Panel Plugin Implementation Plan

**Goal:** Add a Windows Panel public plugin that displays the Host-managed clipboard history by type and pastes the selected text, image, or file-list entry back into the previously active application.

**Architecture:** Create a dependency-free tracked example at `examples/public-plugins/com.uipilot.clipboard-history/`. The Runtime only opens the Panel; pure state helpers own revision ordering, filtering, and selection, while `panel.js` renders Host-provided summaries and translates declared Host keys into plugin intent. The Host remains the sole owner of capture, persistence, original content, target-window restoration, and paste injection.

**Technology:** Native ES modules, semantic HTML, host CSS tokens, Node `node:test`, JSDOM, TypeScript SDK contract checks, and `@uipilot/plugin-cli` validation.

## Global Constraints

- Target Windows Host `0.3.3+` and public API v1.
- Manifest uses `submit + panel`, permissions `ui.panel`, `clipboard.history.read`, `clipboard.history.paste`, and canonical Host keys `ArrowDown`, `ArrowUp`, `Tab`, `Shift+Tab`, `Enter`.
- Use only `window.uipilotPluginPanel` APIs. Do not use Tauri, Shell, browser clipboard, `fetch`, `WebSocket`, native input, or plugin storage for clipboard records.
- Render only Host summaries: text preview, image thumbnail and dimensions, or first file name/count/availability. Never log clipboard content or expose full paths.
- Merge snapshots by canonical decimal `revision`; preserve a still-visible selected ID and otherwise select the newest item in the active filter.
- `Tab` and `Shift+Tab` cycle `all -> image -> files -> text`; arrows clamp at list boundaries; Enter calls `paste({ id, routeSequence })` exactly once.
- Host acceptance remains user-operated under `docs/superpowers/checklists/2026-08-30-clipboard-history-host-manual-acceptance.md`; automated browser preview must not synthesize input into external applications.

## Global Execution Rules

- Task dependency order is `Task 1 -> Task 2 -> Task 3`.
- Each task uses focused TDD: establish the intended failing assertion, implement the smallest contract, then rerun the focused command.
- Each task produces one atomic commit and does not modify Host, SDK, CLI, or pre-existing user changes.
- Run package tests, SDK typing, CLI validation, `git diff --check`, and visual preview checks once after all tasks.

### Task 1: Package Contract And Pure History State

**Files:** `examples/public-plugins/com.uipilot.clipboard-history/package.json`, `examples/public-plugins/com.uipilot.clipboard-history/package/plugin.json`, `examples/public-plugins/com.uipilot.clipboard-history/package/dist/runtime.js`, `examples/public-plugins/com.uipilot.clipboard-history/package/dist/clipboard-history-logic.js`, `examples/public-plugins/com.uipilot.clipboard-history/tests/logic.test.js`, `examples/public-plugins/com.uipilot.clipboard-history/tests/sdk-contract.ts`

**Dependencies:** Public API contract sections `Manifest Identity`, `Panel Host Keys And Return`, `Panel Clipboard History`, and the manual acceptance preconditions.

- [ ] Add the exact Windows Panel manifest and request-preserving Runtime response.
- [ ] Implement canonical decimal revision comparison, four filters, clamped selection movement, filter cycling, and selection reconciliation without DOM dependencies.
- [ ] Add SDK type assertions for every clipboard-history summary branch, paste admission, Host key event, and invalid paste inputs.

**Distinct test coverage:** canonical revisions compare by length then lexical order; older snapshots are rejected; `all/image/files/text` filters retain Host order; switching filters selects the newest item; arrows clamp; a retained selected ID survives a newer snapshot.

**Verify:** `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/logic.test.js`

### Task 2: Panel Rendering And Keyboard Workflow

**Files:** `examples/public-plugins/com.uipilot.clipboard-history/package/dist/panel.html`, `examples/public-plugins/com.uipilot.clipboard-history/package/dist/panel.css`, `examples/public-plugins/com.uipilot.clipboard-history/package/dist/panel.js`, `examples/public-plugins/com.uipilot.clipboard-history/tests/panel.test.js`

**Dependencies:** Task 1 helpers; developer guide sections `分支 C：在启动器内挂载面板` and `剪贴板历史为空或粘贴被拒绝`.

- [ ] Build the approved two-column vertical tabs and dense list with text, image, file, empty, loading, and failure states.
- [ ] Register `onHostKey`, `onUpdate`, and `clipboardHistory.onChanged` before initial `list()` completion; apply snapshots only when revision is current.
- [ ] Route Tab/Shift+Tab, ArrowUp/ArrowDown, and Enter exactly as approved; bind Enter paste to the event `routeSequence` and stop DOM work after admission.
- [ ] Support pointer selection, per-item removal, and clear-all while returning focus to the Host input for the next routed key.
- [ ] Map the six fixed paste error names to concise redacted Chinese status messages.

**Distinct test coverage:** Host keys cycle/clamp without list growth; Enter with no item is inert; Enter with an item calls paste once with exact ID/sequence; admission does not render afterward; pre-hide errors keep the panel; stale snapshots cannot roll back selection; remove/clear refresh state; unavailable files disable paste; forbidden browser/native APIs are absent.

**Verify:** `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/panel.test.js`

### Task 3: Preview, Documentation, And Package Validation

**Files:** `examples/public-plugins/com.uipilot.clipboard-history/preview.html`, `examples/public-plugins/com.uipilot.clipboard-history/preview.js`, `examples/public-plugins/com.uipilot.clipboard-history/README.md`, `examples/public-plugins/com.uipilot.clipboard-history/tests/package.test.js`

**Dependencies:** Tasks 1-2; developer guide sections `使用独立 CLI 验证`, `使用开发目录安装`, and `发布前检查清单`.

- [ ] Add a package-external preview bridge with representative text, image, available file-list, and unavailable file-list summaries in light and dark themes.
- [ ] Document installation, permissions, privacy boundary, keyboard workflow, automated verification, browser preview, and user-operated Windows acceptance.
- [ ] Assert the strict package root, Manifest contract, Runtime response, required bridge usage, allowed assets, and absence of forbidden APIs.
- [ ] Visually verify desktop and narrow preview viewports for nonblank rendering, readable truncation, stable selection, dark/light contrast, and no overlap.

**Distinct test coverage:** package contains only allowed files; manifest minimum Host and permission dependency are exact; Runtime preserves `requestId`; preview DTOs cover every record kind and invalid-file state.

**Verify:** `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/package.test.js`

## Final Verification

- `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/*.test.js`
- `npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.clipboard-history/tests/sdk-contract.ts`
- `node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.clipboard-history/package --platform windows`
- `git diff --check`
- Local preview screenshots at desktop and narrow widths in both themes; external-app paste remains on the user-operated Host checklist.
