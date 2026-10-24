# Panel Runtime Actions Implementation Plan

Implementation status: both tasks have passed focused checks and independent review at source125e9d7. The 0.6.0 executable and templates1.0.0 archive are ready under .superpowers/deliveries/uipilot-0.6.0-acceptance/. User confirmed all eight requested manual cases passed on 2026-10-25. Implementation, independent review and this bounded delivery acceptance are complete; the larger foundation roadmap remains open.

> For agentic workers: use subagent-driven-development with one source writer and a combined specification/quality review per cohesive task.

**Goal:** Deliver a Windows Host 0.6.0 and an offline template editor whose buttons call its existing Runtime for validation, preview and durable save.

**Architecture:** Extend the existing Runtime, scheduler and panel session owner with opt-in typed requests. Keep business data in the plugin Runtime and share existing authority checks. The governing contract is [approved design](../specs/2026-10-24-panel-runtime-actions-design.md), specifically Public contract, Actual seams and minimal ownership model, Cancellation and authority proof, and Runnable template editor.

**Technology:** Existing locked Rust/Tauri 2, React 19.2.7, TypeScript 7, Vite 8, Vitest 4 and plain plugin JavaScript. No dependency install, version upgrade or network required.

## Global Constraints

- Windows only. Host/CLI target 0.6.0; API v1, state schema 2, existing historical version minima remain unchanged.
- panel.runtimeActions is explicit opt-in on legacy submit/panel commands. No Window RPC, multi-command panel, general launcher action API, hotkey work, direct panel networking, shell/files or new clipboard API.
- One physical Runtime slot; panel requests admit only when idle. Retirement revokes exact-context authority but retains the physical slot and original deadline until completion or expiry.
- Lock order: controller core -> network transition -> scheduler. If a manager mutation lock is needed, it precedes network transition. Never enter controller while holding network/scheduler/pending-response locks.
- Session replacement/removal retires the registered request under the controller lock. Completion rechecks the live owner before success. Timeout/fault cleanup matches old generation.
- Arm the deadline observer before emit. Sanitized fixed bridge errors; no payload/credential logging. Already-admitted effects cannot be promised rolled back.
- Work only in C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools on codex/local-plugin-foundation. Original D:/code/UiPilot_tools source and user changes are read-only. Existing node_modules junction is read-only; shared Rust target cache is permitted.
- User explicitly requests manual testing as the primary functional acceptance. Keep build/type checks and the focused authority/concurrency/persistence regressions below; this overrides generic full-suite/per-file TDD ceremony. Do not claim manual pass from automation.
- Read docs/ui-guidelines.md before host UI work; read docs/plugin-sdk/public-plugin-developer-guide.md before plugin work. Task 1 explicitly authorizes Host/CLI/SDK changes; Task 2's plugin implementation stays in its package/tests/docs, with the separately assigned existing Host install-label change.

## Global Execution Rules

Task 1 -> reviewed Host contract -> Task 2 -> combined delivery review -> user native acceptance.
Each implementation task produces an atomic local commit plus a report of actual commands/results and remaining limits. Parent owns plan/ledger. No push, merge, installer execution, Host launch, credentials or service operations.
Use meaningful focused failing regressions for new boundary behavior where practical, then final focused passing checks; no repeated whole-root verify by habit.
Commands run from the worktree. Set process-local CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target; all cargo commands use --manifest-path src-tauri/Cargo.toml and --offline. Run source-mutating schema generation sequentially. Record exact commands, test counts and failures in the task report.

### Task 1: Integrated Host panel Runtime request capability

**Files:** Modify src-tauri/src/public_plugins/{manifest,runtime,scheduler,activation}.rs and their existing test modules; src-tauri/src/public_plugins.rs; src-tauri/src/plugin_panel.rs; src-tauri/src/{commands,lib}.rs; src-tauri/build.rs or existing capability definitions where command permissions are registered; packages/plugin-cli/src/{manifest,validate}.ts and existing validation modules as required; packages/plugin-cli/tests/{manifest,validate}.test.ts; docs/plugin-sdk/{public-plugin-v1.md,public-plugin-developer-guide.md,uipilot-plugin-api-v1.d.ts,uipilot-plugin-v1.schema.json}. Synchronize existing Host/CLI release constants, package.json/package-lock.json and src-tauri/{Cargo.toml,Cargo.lock,tauri.conf.json} only as needed. A focused public_plugins/panel_action.rs module and associated test module may own bounded JSON/types; do not refactor unrelated services.

**Dependencies:** Approved design sections Public contract, Actual seams and minimal ownership model, Cancellation and authority proof; existing M4 authority and M2-A identity semantics. Baseline 8176c42.

- [x] Implement opt-in validation/schema/CLI parity and missing-handler Runtime readiness rejection. Export the exact SDK handler/invocation/response types and document capability detection.
- [x] Implement tagged Command/PanelAction dispatch and owner without fabricated launcher ownership; immutable bounded JSON bridge and private caller-restricted async command.
- [x] Admit and register under the live controller lock; connect exact retirement to every session-removal/replacement path and actual navigation/destruction hooks.
- [x] Preserve physical Runtime ownership on logical retirement; settle exactly once, validate completion owner and response, arm watchdog before emit and match teardown generation.
- [x] Integrate lifecycle/permission/credential changes and existing API guards. Synchronize release version once; keep legacy behavior and historical minima.
- [x] Commit implementation and write .superpowers/sdd/local-foundation/panel-runtime-task-1-report.md with actual interfaces for Task 2.

**Distinct test coverage:** Opt-in absent/false/null/true and minimum version/handler readiness; invalid caller, epoch, activation, accessor/non-JSON/size/depth/count and mismatched reply ID must fail closed. Start a command, attempt panel admission, and verify Busy does not replace running/waiting state. Admit panel request, retire it, attempt replacement: old context loses API authority while physical slot remains until completion/deadline. Use deterministic barriers to serialize close against admission/late storage call; after invalidation no new effect is admitted. Host submit retires panel ownership but does not overlap Runtime handlers. Duplicate/late completion settles once. Complete physically, change credentials before controller settlement, then reject old success and allow a subsequent fresh request without a leaked ticket. Replace generation before old timeout; replacement panel survives. Relevant existing M4 credential/admission coverage remains passing. Execute bridge behavior tests; source-string assertions alone cannot prove these races.

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --offline --lib panel_action; cargo test --manifest-path src-tauri/Cargo.toml --offline --lib public_plugins::scheduler; cargo test --manifest-path src-tauri/Cargo.toml --offline --lib plugin_panel; cargo test --manifest-path src-tauri/Cargo.toml --offline --lib public_plugins::activation; cargo check --manifest-path src-tauri/Cargo.toml --offline; cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema -- --check; npm test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts tests/validate.test.ts; npm run check --workspace @uipilot/plugin-cli; npm run build. Add exact focused bootstrap/SDK commands used to report if housed outside these modules. Tests must actually be discovered, not zero-match passes.

### Task 2: Offline template editor, install labels and manual delivery

**Files:** Create examples/public-plugins/com.uipilot.templates/package/plugin.json, package/dist/{runtime.js,panel.html,panel.css,panel.js}, README.md and tests/{runtime.test.js,sdk-contract.ts}; reuse closest demo-panel package layout and an existing suitable icon if needed. Modify src/public-plugin-panel.tsx and its existing style rules for install labels, without changing handlers. Update plugin example discovery assertions only where fixed counts require it. Delivery files live under ignored .superpowers/deliveries/uipilot-0.6.0-acceptance/ and task report under .superpowers/sdd/local-foundation/panel-runtime-task-2-report.md.

**Dependencies:** Task 1 reviewed interfaces and approved design Runnable template editor, Two cohesive tasks and verification; docs/ui-guidelines.md and public plugin developer guide. Existing map .superpowers/sdd/local-foundation/template-panel-integration-map.md is UI reference only: use onPanelAction, never its obsolete placeholder onCommand business routing.

- [x] Implement exact load/preview/save payloads, literal grammar, missing-versus-empty samples, whole-catalog validation/capacity/revision-conflict handling and single storage write.
- [x] Implement editor with explicit new draft, variable discovery, busy/error/unsaved states, draft-revision ownership, explicit reload after ambiguous save, theme/focus/keyboard behavior and selectable output.
- [x] Add visible labels “选择插件包” and “选择开发目录” alongside existing icons, preserve compact 720px layout and handlers.
- [x] Package plugin with fresh CLI and build a new acceptance Host, preserving identifier com.uipilot.launcher.acceptance and old 0.5.0 delivery. No native launch/install. Copy required existing Everything/sound resources; do not run service or installer.
- [x] Provide README.zh-CN.md with exact executable/archive paths and numbered preconditions/steps/expected/feedback. Record artifact versions, SHA256, source commit and build evidence. Core old-flow regression is short and targeted.
- [x] Commit plugin/labels/docs; report manual tests as pending until user responds.

**Distinct test coverage:** Parser literal substitution/malformed syntax/missing versus deliberate empty values/duplicate names and nonrecursive replacement; conflict never overwrites; failed storage preserves prior catalog; invalid saved data is not reset; aggregate capacity rejection preserves draft/catalog. Human tests own primary UI interaction, keyboard/themes, rapid edit/close and restart persistence. Do not add exhaustive mocked UI matrices.

**Verify:** node --test examples/public-plugins/com.uipilot.templates/tests/runtime.test.js; npm run build; npm run build --workspace @uipilot/plugin-cli; node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.templates/package. Use existing SDK contract compiler invocation with the new contract file and record it. Build Host with process-local TAURI_CONFIG={"identifier":"com.uipilot.launcher.acceptance","productName":"UiPilot Acceptance"} and cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked; restore environment afterwards. Pack with the actual built CLI's documented syntax and record command and validation.

## Manual Handoff

The approved design's eight manual cases are the acceptance list, delivered in Chinese with actual artifact paths. The first batch should cover package selection, variable preview/save and restart; then invalid input, rapid/close, theme/keyboard and old Notes/multi-command regression. No real credentials needed. Overall foundation remains incomplete after this slice.