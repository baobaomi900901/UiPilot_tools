# Explicit local development refresh and safe recovery

Status: frozen for implementation under the user's autonomous development authorization, after scoped independent design review; no new manual acceptance claimed. Baseline `c4af7f4d20c73dbdf2d5dfdddb07b6733f431469`, Windows Host 0.6.0. Scope is one bounded M6/M7 delivery, not completion of the local-plugin foundation.

## Recommendation and user journey

Deliver an explicit **重新载入开发目录** action for an installed public plugin, recognizable fault recovery, a previewable sanitized diagnostic report, and an independent offline recovery fixture proving data preservation. A developer edits an ordinary package directory, selects it again from the plugin detail, sees same-version identity and permission confirmation, and activates a validated snapshot. They can fix a readiness error and retry without increasing the manifest version. Saved plugin data, compatible settings, retained command names/favorites and revoked network access remain intact under the existing contracts. A fault-disabled plugin remains disabled after replacement; **重新启用** explicitly probes and enables the corrected installed package.

Keep three concepts distinct: **刷新列表** rereads inventory; **重新载入开发目录** selects and snapshots local source again; **重新启用** recovers a disabled installed package. None implies a directory watcher. No source path is remembered across operations or exported. A successful refresh closes the old panel/window; the user runs the command again to see the new code and persisted data.

Alternatives considered:

1. **Chosen: explicit development-directory snapshot refresh**, reusing the existing prepare/commit transaction. Useful for edit/test cycles, preserves an explicit permission review, and avoids an additional activation engine.
2. Installed-Runtime restart loads the already installed digest and does not read edits. Useful later, but it is a different action and should not be implemented by `enable(true)`, which is a no-op when already enabled. Do not bundle it here.
3. Automatic watch/reload or host schema migration introduces unattended execution, source provenance, concurrent data conversion and recovery obligations. Neither is required to prove this local developer journey.

## Existing protections and actual gaps

Line references are relative to the baseline repository, not new APIs.

| Current seam | Evidence and consequence |
| --- | --- |
| `src-tauri/src/public_plugins/package.rs:59`, `:225`, `:314` | Directory and archive input are copied into a staged snapshot, validated, made read-only and digest-checked. Installed content is a digest-addressed snapshot, not a live directory. |
| `activation.rs:1023`, `:1116`, `:1130` | Prepare binds a one-use expiring token to caller, staged candidate and previous configuration; current summary lacks explicit operation/version relation. |
| `activation.rs:1191`, `:1237`, `:1241`, `:1332` | Commit checks caller, exact declared permission selection, unchanged prior config/network declarations, then Runtime readiness before reservation/persistence. Same-version and lower-version package replacements are already possible; there is no monotonic package-version check here. |
| `activation.rs:1377`, `:1392`, `:1405` | Persistence distinguishes committed, definitely not committed with prior-byte revalidation, and unknown. Only the definitely uncommitted verified case restores the previous usable configuration. Unknown outcomes fail closed. |
| `activation.rs:1450`, `:1483` | Published state/bundle becomes authoritative; an older different package digest is cleaned up. There is no retained package-history rollback UI. |
| `state.rs:341`, `:383`, `:404` | Upgrade reconciles retained command identities/settings and preserves enabled/fault state. A replacement alone cannot silently clear a fault or enable a disabled plugin. |
| `activation.rs:1519`, `:1543`, `:1606`; `state.rs:737` | Existing enable performs readiness and generation replacement for a disabled plugin; already-enabled input returns without restarting. |
| `public_plugins.rs:493`, `:638`; `activation.rs:3015` | Automatic Runtime recovery already exists for a specific recovery-needed activation. It is not a public healthy-runtime restart API. |
| `commands.rs:1273`, `:1297`, `:1361` | Existing public management commands are main-window-only; readiness is asynchronous, focus loss is suppressed, old Runtime/panel/window cleanup occurs after success. `plugins.rs:982` is the separate legacy plugin reload path and must not be reused. |
| `activation.rs:4450`; `data_call_gate.rs:80`, `:105`, `:128` | New generation/activation/admission identity prevents new admission from old contexts. Replacing the gate does not drain an already acquired lease. Uninstall has a separate explicit drain. Already-admitted effects may finish. |
| `storage.rs:87`, `:101`, `:169` | Private JSON is owned by pluginId, shared across its commands/content, with per-operation persistence and 5 MiB quota. There is no application-data schema migration API or atomic package-plus-data transaction. |
| `src/public-plugin-panel.tsx:655`, `:685`, `:731`; `src/main.ts:119`; `src/protocol.ts:401` | Directory selection, prepare and commit exist. Failures collapse into generic install messages; a consumed-token failure leaves the prepared UI visible. Detail already has mutation/reload/focus patterns. |
| `examples/public-plugins/com.uipilot.templates/package/dist/runtime.js:65`, `:157` | Existing application-owned schemaVersion 1 and single-snapshot writes demonstrate a convention, not host migration capability. |

The principal gaps are understandable developer actions, binding a detail-level replacement to the selected plugin, informative safe failure/recovery text, bounded diagnostics, and independent evidence of data/version behavior. Avoid claiming existing protections as newly built features.

## Exact scope and interface changes

All names below are proposed first-party Host interfaces, not additions to Runtime SDK v1.

Extend the existing `prepare_public_plugin_install` / `PublicPluginManager::prepare` input with optional `refreshTarget: {pluginId, expectedInventoryRevision}`. Both fields must be present together. Its absence preserves generic local install/update behavior. Its presence is valid only for `developmentDirectory`; require an installed target, matching current inventory revision, exact candidate pluginId and equal canonical package version. Reject other IDs or changed versions with fixed `refreshTargetMismatch` / `refreshVersionMismatch` / `staleInventory` management codes. A developer intentionally changing version uses the existing general **选择开发目录** or **选择插件包** flow instead. Do not infer a directory from inventory `source` (currently a generic local marker).

Bind refresh intent and target to the pending token on the Host; revalidate the target alongside the existing full prior-config equality at commit. The selected path is used only to stage source and is not persisted. Extend prepare summary with an explicit operation enum (`install`, `replace`, `developmentRefresh`) and optional previousVersion for replacement. UI calls the existing commit/cancel commands; no new execution bridge, manifest property or permission is required. Same-version refresh may change declared permissions and must show the complete existing permission/network/credential confirmation. Network grants previously disabled remain disabled when the unchanged declaration is refreshed; new declarations still require existing explicit consent.

The detail action is a secondary text button, with explanatory text “重新选择 package 目录；保存的数据不会被主动清除”. Do not promise absolute no-data-change: a running plugin can write, and new code may write on later invocation. During prepare/commit, disable duplicate mutation controls. Cancellation restores originating focus. After any commit attempt, clear the one-use prepared token even on failure, reload inventory and offer a fresh selection. Distinguish a successful mutation followed by inventory-refresh failure from a failed mutation. Re-enable is a separate visible action only when disabled/faulted, reusing existing enable semantics; ordinary disabled plugins retain the normal enable label. Generic package updates remain available and show previous/new versions, including lower-version replacement with concise data-compatibility text. No automatic downgrade, previous-package restore, data reset or backup restoration is added.

Use existing compact detail/prepare surfaces, semantic theme tokens, explicit accessible names, keyboard order, visible focus, stable status space and Escape/back behavior. No new launcher entry or favorites path is needed.

## Ordering, identity and failure behavior

1. Main Host window is the only management caller. Plugin Runtime/panel/window callers cannot prepare, refresh, enable or read diagnostics. There is no raw invoke capability added to a plugin bridge.
2. Selecting a directory produces a validated immutable candidate. Old activation continues through staging/readiness. The candidate cannot obtain a request-scoped storage/settings/network API merely by loading. Runtime readiness retains the current five-second limit (`public_plugins.rs:88`).
3. Prepare confirmation is bound to caller/token/target/previous state; permission confirmation is not cached across a re-prepare. If another operation changes state while a dialog is open, reject and reload current inventory; do not silently adopt changes.
4. Commit uses the existing reservation and durable outcome pipeline. Generation increases; fresh activation/admission identities prevent old requests from gaining new authority. Do not reimplement transaction publication in a UI-specific command.
5. After definite durable success, existing request retirement, network authority replacement, data admission replacement, delayed-message/timer cleanup and state/bundle publication run in their current order. Command integration destroys old Runtime and tears down matching panel/window, then focuses Host content. No old response can republish into the new session.
6. Static validation, identity mismatch, canceled confirmation and readiness failure leave prior active configuration available. A verified definitely-uncommitted state commit also preserves it. Unknown outcome or failed previous-byte verification blocks operation through existing fail-closed behavior. Display “状态未确认，请重启宿主后核对” for an observed uncertain outcome, never “已回滚”. Restart recovery must inspect durable state; no auto-enable, restore or replay is implied.
7. Refresh preserves disabled/fault state. After corrected code is installed into a fault-disabled package, explicitly re-enable; readiness failure leaves it disabled and data untouched by Host. A healthy refresh does not invoke commands or replay input automatically.
8. Already-admitted storage/network/notification effects may complete. This delivery adds no drain or migration barrier. A canceled action or failed save is not proof that data or external effects did not happen. Reopen/read before an intentional retry.

## Plugin data/version contract and independent fixture

Package semver, Host release, SDK manifest/API version and plugin-owned stored data schema are independent. Host state schema2 and storage envelope schema1 are not business-data schema. Package update does not convert or restore private JSON. Package downgrade remains allowed through general update; compatibility is the plugin author's responsibility. Host binary downgrade restrictions remain unchanged.

Implement com.uipilot.demo-recovery as a small independent submit/mainResult fixture using demo-multi as reference. Stable plugin/command IDs, one ordinary manifest-default setting and a bounded private snapshot {schemaVersion:1,count}. Read/status displays build label, stored count and default setting without writes. Explicit increment validates the entire document, computes a copy and performs one storage.set. Missing data means empty in-memory state; malformed/non-null/unknown/newer schema blocks writes/removes, never silently resets. Write failure reports save-uncertain; reopen/read before intentional retry. No new panel application, network, credentials, clipboard requirement, timers or Host capability.

Generate controlled local variants A and B (same ID and exact version, visible Runtime marker differs), valid-static-package C missing onCommand (readiness failure), controlled throwing variant, compatible semver upgrade D, and disposable schema2 writer/reader. Schema2 is written only by explicit test input in this dedicated test namespace; install the schema1 reader and attempt increment, then reinstall schema2 reader to show exact sentinel remains. Generator/test/docs stay outside installable package roots. Only synthetic data. Error, source-path, ordinary-setting and private-value sentinels exercise diagnostic exclusion.

## Sanitized diagnostic contract

Add main-window-only get_public_plugin_diagnostics({pluginId}) and copy_public_plugin_diagnostics({report}), plus strict first-party TypeScript decoding. Construct the report field-by-field; never serialize inventory/config/errors/manifest wholesale.

Exact report fields: schemaVersion:1, hostVersion (canonical actual Host semver), platform (existing platform enum), pluginId (existing validated ID grammar), state, lastOperation. No additional fields.
- state is null when unavailable, fail-closed, unknown durability or authoritative lookup cannot be confirmed. Cached old config alone is not authority.
- Otherwise state is either {status:"notInstalled",inventoryRevision} or {status:"installed",packageVersion,generation,inventoryRevision,enabled,fault}. Versions are canonical package semver; generation and inventoryRevision are canonical u64 decimal strings. State inventoryRevision is the global revision captured with the snapshot. fault is null or the existing fixed PublicPluginFault enum. Do not infer Runtime health from enabled alone.
- lastOperation is null or {operation,phase,errorCode,stateCommitOutcome}. operation is prepare|developmentRefresh|replace|enable; phase is validation|readiness|persistence|publication|complete; errorCode is null or a fixed PublicPluginManagementError wire code; stateCommitOutcome is notAttempted|committed|notCommitted|unknown. An ordinary install uses operation prepare through its commit. Null errorCode alone is not proof of success; only complete with null error is success.
- notCommitted is recorded only after previous-byte revalidation. Unknown outcome or failed previous-byte verification uses unknown and state null. Post-durable publication failure records committed plus publication error and unavailable state; it is not rollback. Capture observations at actual branches, not by guessing from pluginUnavailable. State-commit outcome concerns Host configuration, never plugin data or remote effects.

Store latest observation in memory only, with at most 64 plugin slots and oldest-entry eviction. Reserve a monotonic internal attempt sequence and replace slot with (seq,None) at operation start; clear previous observation. Only the still-current exact sequence can publish a whole observation; older completion and evicted completion are ignored. Keep observation lock short and release before other locks, readiness or persistence. Observation failure cannot alter management outcome. A failed generic prepare without an identified installed target may omit the record. Read-only inventory refresh does not change it. No event stream, timestamps, file logger or background sampling.

Never include content/results, inputs/panel payloads, storage keys/values, settings definitions/values, secret presence/identifiers/values, domains, URLs, headers, paths, plugin display names/descriptions, WebView labels, request/session/token values, raw errors/stacks, package digest or source. The plugin ID and package version themselves identify the plugin; show exact JSON preview before explicit copy.

Copy accepts the same typed preview DTO (no arbitrary text), validates unknown fields at every level, all scalar grammar/bounds, enum and cross-field invariants, and serialized UTF-8 <=16384 bytes. It serializes that exact snapshot deterministically using the same formatting/key order displayed by frontend and calls existing app.clipboard().write_text. Do not silently refetch before copy. Main-window authorization precedes state/clipboard access. No plugin bridge or permission is added; no automatic copy/transmission. Fixed copy failure feedback; unavailable state renders as unknown, never healthy. The Rust DTO and TS decoder reject noncanonical and extra fields.

## Acceptance and minimal automated checks

User-operated tests are primary; these new cases remain pending until user confirms:
1. Install A, increment counter, rename/favorite command; restart and verify state, default setting and command configuration.
2. Inventory refresh keeps installed A. Targeted directory refresh to B shows equal version and confirmation; rerun and verify marker B with retained counter/name/favorite.
3. Wrong plugin/version selection is rejected. Cancel source chooser/confirmation and verify installed copy still runs. Intentional version changes use general install/update.
4. Select readiness-broken C: safe readiness failure and diagnostic, prior version/marker/counter/default remains after rerun/restart. Reselect repaired B and succeed without version bump.
5. Run throw input three times within five minutes with no successful invocation between; observe fault/disable. Refresh corrected code while faulted; it remains disabled. Explicitly re-enable and verify retained data. Do not promise a specific consecutiveFailures code.
6. Upgrade D and verify persisted schema1. Explicit disposable schema2 fixture writes sentinel; older reader refuses mutation; reinstall schema2 reader and verify intact data.
7. Preview/copy diagnosis in healthy/readiness-failed/faulted states; synthetic setting/content/path/error sentinels absent. Check Light/Dark, keyboard, cancellation/focus/busy feedback.
8. Existing Notes/templates/multi-command core behavior remains as previously accepted.

Concurrent target-change, unknown persistence, unavailable clipboard and caller/old-owner checks use focused controlled tests where normal manual steps cannot safely reproduce them. No hidden IPC, filesystem corruption, network or service/credential operations.

Automation: types/build; target/caller/token binding and stale target rejection; readiness/verified-notCommitted/unknown/publication observations without false rollback; latest-observation sequencing and bounded eviction; strict diagnostic/copy allowlist; fixture unknown-schema/single-write/uncertain-save behavior. Use existing authority/permission tests only where touched. Avoid unrelated full-suite reruns.


## Frozen delivery decisions

This section resolves candidate alternatives and is normative where earlier background differs.

- Release is Host 0.7.0 and CLI production target 0.7.0. Manifest/Runtime API v1 and historical minimumHostVersion capability floors stay unchanged.
- The independent example is com.uipilot.demo-recovery, a small submit/mainResult fixture, not a checklist application. Use demo-multi as reference and stable commandId. It displays build label, counter and one ordinary manifest-default setting; explicit increment writes a single validated schema-1 snapshot. Read/status never writes. Unknown/newer/malformed non-null data blocks writes/removes. No new panel, network, credentials, clipboard requirement, migration or Host API.
- Generate A/B same-version, missing-onCommand readiness failure, controlled-throw, compatible semver upgrade and disposable explicit schema-2 writer/reader variants. All variants use controlled synthetic data and label their purpose. Existing Notes/templates remain compatibility acceptance, not new business development.
- Target expectedInventoryRevision is the global inventory revision shown by list_public_plugins, parsed as canonical u64 decimal. Check it with the mutation lock during prepare. Capture full target config in the pending token; commit uses existing exact target-config equality. An unrelated plugin change after prepare alone must not invalidate that target contract.
- Each observed management attempt reserves an internal monotonic sequence at start; only the current exact slot sequence may replace its observation. A late completion cannot overwrite a newer attempt, and completion after eviction cannot resurrect a slot. There are at most 64 slots; observation lock is short-lived and never held across other manager locks, readiness or persistence. Observational errors cannot affect management results.
- No first-party plain-text clipboard adapter is currently exposed. Add main-window-only copy_public_plugin_diagnostics({report}), accepting only the same strict allowlisted report DTO shown in preview, then serialize and call existing app.clipboard().write_text. Rust denies unknown fields at every nested shape and validates pluginId, canonical versions, decimal bounds, enum/cross-field invariants and UTF-8 size <=16384. No arbitrary string copy endpoint or plugin permission/bridge is added. Copy exactly the preview snapshot; do not silently fetch a newer report before copying.
- Manual cases requiring privileged failure injection or impossible concurrent UI manipulation stay focused automated checks. Do not instruct user to corrupt Host files or use hidden IPC. Clipboard copying is user initiated; agent does not perform it.
