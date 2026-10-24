# Task 1 — integrated Host panel Runtime actions

Status: **DONE_WITH_CONCERNS**. Implementation and approved automatic gates pass. The remaining concern is the expressly required human/native boundary: no Host launch, installer run, WebView2 UI exercise, or human functional acceptance was performed. No known source blocker remains. Independent source review precedes Task 2.

Worktree: `C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools`, branch `codex/local-plugin-foundation`. Review base: `2d216d0fa2ddc4c0b88a8146c98f15ae85823ad9`. Parent documentation-only commits `0c1d4b7` and `099120d` are already included in HEAD; this report and implementation form the next atomic local commit. The final handoff identifies its SHA.

## Result and actual Task 2 interfaces

- Host/root package/CLI validation target is **0.6.0**; CLI package remains **0.1.0**. API v1, state schema 2, and historical feature minima are unchanged.
- Manifest: legacy `command` with `submit` + `panel`, Windows-only supportedPlatforms, granted `ui.panel`, `minimumHostVersion >= 0.6.0`, and `panel.runtimeActions: true`. Absent/false retains legacy behavior; null/wrong types reject. Multi-command panels remain forbidden by the existing relationship contract. The Runtime readiness probe requires a function export `onPanelAction` only for opt-in; CLI validates metadata without executing exports.
- Panel: optional `window.uipilotPluginPanel.request({action, payload}): Promise<JsonValue>`, with no public plugin/session/generation selectors. Missing property is normal capability detection. Payload is required, including explicit null; only action/payload are accepted. Do not await a request in onUpdate/onHostKey; the Host returns Busy while acknowledgements are pending.
- Runtime: optional `onPanelAction(invocation, api)` returning `{requestId: invocation.requestId, data}`. Invocation is `{apiVersion:1, requestId, action, payload, context:{platform:'windows',theme:'dark'|'light',invokedAt}}`. Each invocation/API is fresh and deeply frozen. OnCommand/onUpdate wire shapes remain unchanged. Action responses settle separately from launcher results and never call onUpdate.
- `request` resolves with frozen data, with usable ordinary arrays (map/iteration). Serialized snapshots use null-prototype arrays internally to avoid inherited Array.prototype.toJSON; native IPC reconstructs ordinary arrays for Runtime invocation.payload. Executable tests assert both public array views and zero inherited toJSON/getter execution.
- Names match `^[a-z][a-z0-9.-]{0,63}$`, excluding __proto__/prototype/constructor. JSON is finite primitives, dense arrays and plain data records; reject getters/accessors, toJSON functions, cycles, symbols, undefined, BigInt, nonfinite numbers and prohibited record keys. Both envelopes are <=64 KiB compact UTF-8, <=32 nesting and <=4096 values. Host independently checks the request and full response, including exact reply ID.
- Fixed Error.name values: InvalidPanelRequestError, PanelRequestBusyError, ExpiredPanelSession, PanelRequestCancelledError, PanelRequestTimeoutError, InvalidPanelResponseError, PanelRequestFailedError, PanelRequestUnavailableError. Business failures are ordinary data. No plugin stack or arbitrary native error text crosses this bridge.
- The private async `plugin_panel_request` command is registered only in the panel-content capability. It derives identity from the actual Webview label, validates canonical decimal epoch, and accepts raw serde_json::Value before an exact-object guard. Positional arrays such as ['save',null], missing payload and extra top-level fields fail before struct deserialization.

## Ownership and lifecycle implementation

`PluginRequestOwner` explicitly distinguishes Command(PluginSubmissionOwner) and PanelAction(content label, session epoch, action/payload). Panel actions have no fabricated launcher intent, control text or submission token; retirement metadata uses Option<String> for actual command tokens. Existing command candidate routing fields remain available to legacy command callers.

Controller admission holds core through manager admission and registration. Manager confirms installed/enabled/nonfaulted bundle, ui.panel grant/declaration, opt-in, ready Runtime and exact generation/activation/admission identity under mutation -> network transition -> scheduler. Panel admission is idle-only and leaves existing running/waiting/rebuilding state untouched on Busy.

One session registration holds an exact context and weak service callback. Central invalidation retires it under core before every session removal/replacement. Logical retirement sets scheduler current=false and invalidates network context without releasing the physical Runtime slot or changing its original deadline. Completion or timeout releases/rebuilds that slot. Host submission cancels panel response ownership while preserving normal latest-wins command waiting. Duplicate/late settlement removes at most one pending sender.

One bounded completed-panel response-authority ticket per plugin closes the completion-to-controller-settlement gap. It keeps no Runtime execution slot. Credential/grant/lifecycle retirement marks that exact ticket; Host submission marks Cancelled. Under controller -> mutation -> network -> scheduler, completion rechecks confirmed owner and claims the ticket before selecting success. Every terminal settlement discards only its matching ticket. The deterministic regression completes the handler, removes an absent synthetic secret from an isolated temporary store, then proves success is rejected and the next action can complete. No real credentials are used.

The deadline observer is armed before any Runtime lookup or emit. Delivery failures revoke authority while retaining the original slot/deadline when delivery may be uncertain. Runtime busy is cleared before completion IPC, preserving synchronous next-dispatch ordering. Timeout/transport terminal errors keep their documented names rather than requiring a normal-completion ticket.

Actual navigation hooks distinguish the initial inert bootstrap document from the first content navigation; subsequent reload/navigation retires the captured epoch and queues native content cleanup. Native ProcessFailed/WindowCloseRequested callbacks retire that epoch. Explicit destruction retires the matching content registration before close. The pre-existing unmute callback now uses its captured epoch rather than teardown_session(None). Timeout/fault cleanup and late-failure panel settlement match old generation. Poisoned controller recovery is restricted to revoking already-registered authority on hide/teardown/destruction; new admission still fails closed. A direct regression covers that boundary.

Already-admitted effects may finish. Cancellation is not a rollback guarantee; after an ambiguous save, reload before deliberate retry. Existing Runtime storage/settings/notifications/optional HTTPS authority remains in force. No Window RPC, direct panel network, shell/files or clipboard expansion was introduced.

## Verification (actual commands/results)

Commands ran from the worktree with existing dependencies. Every cargo command used process-local `$env:CARGO_TARGET_DIR='D:/code/UiPilot_tools/src-tauri/target'` and --offline. No full-root verify suite, install, dependency upgrade, external network, push, merge, Host launch, installer run, real-credential or service operation was performed.

| Command | Final result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib panel_action` | 10 passed, 0 failed (latest final poison revocation recheck) |
| `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib public_plugins::scheduler` | 13 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib plugin_panel` | 84 passed, 0 failed (latest final recheck) |
| `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib public_plugins::activation` | 85 passed, 0 failed, including existing M4 credential/admission/fault barriers |
| `cargo check --manifest-path src-tauri/Cargo.toml --offline` | Passed, latest final recheck after poison change |
| `cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema` | Passed; canonical schema generated once |
| `cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema -- --check` | Passed |
| `node packages/plugin-cli/scripts/sync-plugin-schema.mjs` then `node packages/plugin-cli/scripts/generate-schema-validator.mjs` | Passed sequential generation |
| The same two node commands with `--check` | Passed parity checks |
| `npm test -- --run src/plugin-panel-action-bootstrap.test.ts src/plugin-panel-bootstrap.test.ts src/plugin-network-bootstrap.test.ts` | 21 passed across 3 files, 0 failed |
| `npm test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts tests/validate.test.ts` | 58 passed across 2 files, 0 failed |
| `npm test --workspace @uipilot/plugin-cli -- tests/pack.test.ts` | 23 passed, 0 failed |
| `node --test scripts/verify-local-foundation.test.mjs` | 18 passed, 0 failed; coordinator unit tests only, not full-root verify |
| `npm run check --workspace @uipilot/plugin-cli` | Passed, including final dependent release-fixture changes |
| `node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict --target ES2021 --module ESNext --moduleResolution Bundler --lib ES2021,DOM docs/plugin-sdk/uipilot-plugin-api-v1.d.ts` | Passed |
| `npm run build` | Passed (tsc + Vite); existing >500 kB chunk advisory remains |
| `git diff --check` | Passed |

During development, compile attempts exposed owner-type migration errors (missing qualification, old field access and a bool-returning helper's ? use); corrected before passing gates. Initial executable bridge tests exposed an incorrect async expectation for the unchanged legacy Runtime storage context() synchronous throw, plus a cross-realm test wrapper prototype mismatch; corrected the assertion and used a prototype-independent envelope. The new panel.request API remains async/rejecting. Initial CLI/coordinator/pack checks exposed old current-target literals/regexes and old too-new fixtures that became valid at 0.6.0; updated only current target expectations and changed deliberate incompatible fixtures to 0.6.1. An existing panel source-string test was updated for apiShape and the opt-in-aware bootstrap. No failing gate is being omitted.

## Manual boundaries / concerns

The supplied Host must still be launched and exercised by the user in Task 2's delivery cases. In particular, actual WebView2 native navigation/process/window-close callbacks, real panel mounting, theme/keyboard behavior, Notes/M2-A compatibility, ambiguous-save UX and restart persistence have not been manually passed here. The offline template example, install control labels, release installer/package artifact and human acceptance belong to Task 2/delivery. This Task 1 commit supplies an independently implemented Host capability and its reviewed SDK seam; it is not a claim of completed native acceptance.

Self-review checked exact session/activation authority, lock ordering and map-lock release before controller access, physical-slot preservation, bounded serializer behavior, generation matching, release minima, generated parity and unrelated source churn. Files touched only by initial CRLF normalization have no staged content differences; no original D: sources or user changes were modified.

## Changed files

- `docs/plugin-sdk/public-plugin-developer-guide.md`
- `docs/plugin-sdk/public-plugin-v1.md`
- `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`
- `docs/plugin-sdk/uipilot-plugin-v1.schema.json`
- `package-lock.json`
- `package.json`
- `packages/plugin-cli/README.md`
- `packages/plugin-cli/schema/uipilot-plugin-v1.schema.json`
- `packages/plugin-cli/scripts/smoke.mjs`
- `packages/plugin-cli/src/contracts.ts`
- `packages/plugin-cli/src/generated/manifest-validator.mjs`
- `packages/plugin-cli/src/manifest.ts`
- `packages/plugin-cli/tests/cli.test.ts`
- `packages/plugin-cli/tests/manifest.test.ts`
- `packages/plugin-cli/tests/pack.test.ts`
- `packages/plugin-cli/tests/validate.test.ts`
- `scripts/verify-local-foundation.test.mjs`
- `src-tauri/Cargo.lock`
- `src-tauri/Cargo.toml`
- `src-tauri/build.rs`
- `src-tauri/capabilities/plugin-panel-content.json`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/plugin_panel.rs`
- `src-tauri/src/public_plugins.rs`
- `src-tauri/src/public_plugins/activation.rs`
- `src-tauri/src/public_plugins/manifest.rs`
- `src-tauri/src/public_plugins/network/authority_gate.rs`
- `src-tauri/src/public_plugins/runtime.rs`
- `src-tauri/src/public_plugins/scheduler.rs`
- `src-tauri/src/public_plugins/tests.rs`
- `src-tauri/src/public_plugins/webview_audio_guard.rs`
- `src-tauri/tauri.conf.json`
- `src/plugin-network-bootstrap.test.ts`
- `src/plugin-panel-bootstrap.test.ts`
- `src-tauri/permissions/autogenerated/plugin_panel_request.toml`
- `src-tauri/src/public_plugins/panel_action.rs`
- `src-tauri/src/public_plugins/panel_action_json.js`
- `src/plugin-panel-action-bootstrap.test.ts`

The report itself is also included in the atomic local commit.
