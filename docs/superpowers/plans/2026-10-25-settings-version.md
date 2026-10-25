# Settings version implementation plan

Goal: show the running UiPilot version in Settings and deliver a runnable build. Architecture and failure semantics: [design](../specs/2026-10-25-settings-version-design.md). Existing React/Tauri stack, no dependencies.

Global constraints: work in the local-plugin-foundation worktree; original D: source read-only, shared target cache allowed; main-only native version permission; no plugin API changes or settings persistence. Manual testing first per user instruction; retain build/type checks and existing focused navigation checks. Do not launch Host, overwrite previous kits, merge or push.

### Task 1: Show version and package the updated application

Files: src/app-version.tsx, src/launcher-view.tsx, src/styles.css, src-tauri/capabilities/main.json; ignored delivery and manual instructions.

- [x] Add metadata-backed header label, loading/error states and unmount guard.
- [x] Grant version read only to the exact main capability; verify other capability files unchanged.
- [x] Run npm run build and npm test -- src/launcher.test.tsx -t 'navigates between launcher and settings|routes the messages target'. No new mechanical tests.
- [x] Review scoped diff, commit, then build a new embedded-frontend acceptance executable with the same data identity and five resources; record source and artifact hashes.
- [x] Hand user manual checks: displayed 0.7.0, consistent tabs, Light/Dark, preserved old settings/plugins. These remain pending until user confirmation.

Native command: cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked with process-local acceptance TAURI_CONFIG and shared CARGO_TARGET_DIR; restore environment afterward.
Delivery2026-10-25: source2b2d005, independent SpecPASS/QualityPASS/no findings; navigation2/type/frontendbuild passed. Nativebuildexit0,688inputs unchanged,5resources match, old0.7kit unchanged. Newkit uipilot-0.7.0-settings-version, sameacceptanceidentity. Actualmanualchecks and recovery9cases remainpending.

## 2026-10-25 用户确认版本显示通过

用户回复“版本显示通过”，对应本轮明确请求的三项人工检查：设置右上角显示版本0.7.0；切换通用/消息/插件均可见；浅色/深色文字清楚且不重叠。以上记录为用户确认通过，未由代理执行。此确认不代表旧插件数据回归或0.7.0恢复用例1–9通过；这些仍待测试。下一步使用现有新版exe，安装recovery A并验证只读状态、单次递增与重启保存。
