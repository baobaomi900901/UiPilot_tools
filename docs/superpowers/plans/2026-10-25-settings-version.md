# Settings version implementation plan

Goal: show the running UiPilot version in Settings and deliver a runnable build. Architecture and failure semantics: [design](../specs/2026-10-25-settings-version-design.md). Existing React/Tauri stack, no dependencies.

Global constraints: work in the local-plugin-foundation worktree; original D: source read-only, shared target cache allowed; main-only native version permission; no plugin API changes or settings persistence. Manual testing first per user instruction; retain build/type checks and existing focused navigation checks. Do not launch Host, overwrite previous kits, merge or push.

### Task 1: Show version and package the updated application

Files: src/app-version.tsx, src/launcher-view.tsx, src/styles.css, src-tauri/capabilities/main.json; ignored delivery and manual instructions.

- [x] Add metadata-backed header label, loading/error states and unmount guard.
- [x] Grant version read only to the exact main capability; verify other capability files unchanged.
- [x] Run npm run build and npm test -- src/launcher.test.tsx -t 'navigates between launcher and settings|routes the messages target'. No new mechanical tests.
- [ ] Review scoped diff, commit, then build a new embedded-frontend acceptance executable with the same data identity and five resources; record source and artifact hashes.
- [ ] Hand user manual checks: displayed 0.7.0, consistent tabs, Light/Dark, preserved old settings/plugins. These remain pending until user confirmation.

Native command: cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked with process-local acceptance TAURI_CONFIG and shared CARGO_TARGET_DIR; restore environment afterward.