# H1/H4 host translation implementation report

Implemented in the authorized isolated worktree `C:/Users/moby/.codex/worktrees/translate-panel/UiPilot_tools`, branch `codex/translate-panel-implementation`.

## Scope and behavior

- Youdao source accepts `auto`, `en`, `zh-CHS`; target remains the fixed `en`, `zh-CHS` list. Existing input/response byte limits, NUL checks, credential secrecy and exact input properties remain.
- Provider requests add `strict=true`. Response metadata is normalized from the provider `l` pair, never guessed from text: `sourceLanguage`, `targetLanguage`, `unchanged`. Unsupported source returns null; a recognized response pair with a mismatched/unsupported target returns the existing controlled response error. Missing `l` remains backward compatible with legacy provider fixtures (null source, requested target).
- Explicit same-language requests preserve the original text and return `unchanged:true`. They still perform service grant/configuration checks and the full HTTPS admission path, then recheck credential revision and network authority before local delivery, releasing the admitted call without DNS or transport. They retain the existing request-budget behavior.
- New input forms and Panel translation require minimumHostVersion >=0.18.0. Legacy mainResult with old input remains supported. Panel calls require legacy submit+panel, runtimeActions, declared/granted ui.panel, no resultActions, and a current scheduler PanelAction owner. A Panel onCommand cannot call the extended service.
- Selection authority shares the narrow Panel shape and current ui.panel grant checks. Frontend selection resolution now opens a Panel with the exact captured initialArgument once, whether inputRequired is true or false, and preserves stable command identity and existing intent/edit retirement. Capture failures keep existing no-submit behavior.
- Final Panel Runtime result settlement is credential-revision fenced through the actual content sender. Existing scheduler/controller completion claims, permission revocation, session replacement, physical handler slots and credential-revision/publication leases remain intact.
- Published additive SDK language/response types and 0.18.0 contracts. Historical 0.15–0.17-only statements are version qualified.

## Files changed

- `src-tauri/src/public_plugins/host_services.rs`: input, strict request, normalized metadata, bounded parser tests.
- `src-tauri/src/public_plugins/activation.rs`: service and selection admission; fake-provider/local-response/revocation tests.
- `src-tauri/src/public_plugins/manifest.rs`: shared narrow translation Panel compatibility predicate.
- `src-tauri/src/public_plugins/scheduler.rs`: current request exposes PanelAction ownership bit.
- `src-tauri/src/public_plugins/network/{broker.rs,authority_gate.rs}`: authority-fenced local completion of an already-admitted reservation.
- `src-tauri/src/public_plugins/runtime.rs`: exact bounded service input and additive response snapshots.
- `src-tauri/src/public_plugins.rs`: credential fence at final Panel settlement/send.
- `src/launcher-core.ts`, `src/selection-translation.test.ts`: selected text opens the resolved Panel with the original captured argument, with duplicate and stale-edit coverage.
- `src/plugin-network-bootstrap.test.ts`: auto/same input, enriched frozen response, retained API expiry.
- `docs/plugin-sdk/{uipilot-plugin-api-v1.d.ts,public-plugin-v1.md,public-plugin-developer-guide.md}`: additive contract documentation.

## RED evidence

1. `npx vitest run src/plugin-network-bootstrap.test.ts`: 2 expected failures /17 passes. Auto input was rejected; enriched native response was rejected by the old facade.
2. With `TAURI_CONFIG={"bundle":{"resources":[]}}` and `CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target`:
   `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::host_services::tests --no-default-features`: 3 expected failures /2 passes (auto/same input, absent metadata, absent strict field).
3. Newly compiled Rust binary, filters `host_services_panel` and `selection_translation_accepts_authorized_panel`: 2/2 and1/1 failed as expected before admission changes (local input denied, Panel never reached fake provider, selection authority returned None).
4. `npx vitest run src/selection-translation.test.ts`: 2 expected failures /14 passes. Both Panel inputRequired settings were explicitly rejected by the launcher.

## GREEN verification

- `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::host_services::tests --no-default-features`: 5/5 pass, builds the complete lib-test target.
- Fresh compiled lib-test binary filters (same build):
  - `host_services`: 9/9 pass (parser/signing, Panel local completion, ui.panel revocation, legacy independent-grant/configuration and late JS completion checks).
  - `selection_translation`: 11/11 pass (new Panel grant checks plus existing native fake capture, restore, permission and delayed delivery coverage).
  - `panel_action_`: 12/12 pass (session close/replacement, cancellation, busy, exact old completion retirement, credential-change-before-settlement, lifecycle paths).
  - `credentials::tests::`: 14/14 pass (revision, invalidation, encryption and configuration boundary regressions).
- `npx vitest run src/selection-translation.test.ts src/plugin-network-bootstrap.test.ts src/plugin-panel-action-bootstrap.test.ts src/translation-selection-hotkey.test.tsx`: 43/43 pass.
- `npx tsc --noEmit`: pass.
- `git diff --check`: pass.

The first Rust attempt hit build.rs OS error32 because the running dev application locks bundled resources. The process-local empty-resource override above resolved it without touching source configuration, killing apps or deleting caches. One pre-existing screenshot dead-code warning remains. The controller poison regression deliberately prints its synthetic panic while passing. One intermediate filter matched zero tests; corrected exact filters above supplied the real RED/GREEN evidence. An intermediate duplicate accessor compile error was corrected before GREEN.

## Limits and follow-up

- No clipboard facade, speech, release version source, CLI target or plugin package changes. The current release remains0.17.0 until the next authorized H2/H3 step; tests explicitly use a0.18.0 host fixture.
- No real provider, system clipboard, speech, credentials or cross-app selection effects were used. Native end-to-end acceptance remains user-operated after the complete0.18.0 build is ready.
- The final credential send fence is reviewed in the existing completion path; automated coverage combines actual Panel completion claims and credential-change-before-settlement tests with credential revision regressions. The AppHandle-based complete_submission wrapper itself is not spun up in a real WebView.
- Same-language admission intentionally consumes the existing service/network admission budget, despite skipping provider transport.
