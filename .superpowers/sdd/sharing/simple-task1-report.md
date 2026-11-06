# Single-Panel sharing Task 1 report

Status: implemented and targeted verification complete; ready for independent whole-flow review. No native/provider acceptance is claimed. Task 2 still owns release verification, archives and the CLI bundle guard failure below.

## Scope and preserved work

Implemented the approved `docs/superpowers/specs/2026-11-06-plugin-sharing-design.md` single-Panel flow on `codex/ocr-panel`, starting at `32ca396`. The former nine-task plan, restricted preparation and authority coordinator are not requirements for this delivery.

- Existing backup branch `codex/sharing-full-handoff-backup-20261106` points to `eff33ca666df5c1b2d154b10e9ecc6313ad580e3`.
- Existing binary working-diff backup `.superpowers/sdd/sharing/pre-simplification-working.patch` is 375386 bytes.
- Removed the sharing-only coordinator, dual-document delivery/preparation state, restricted `sharePrepare` Runtime paths, manager authority registry and associated lock/scheduler/network changes introduced after `3740a96`. Ordinary manager/scheduler/network files were composed from that exact earlier version, rather than resetting the worktree. Retained the already implemented manifest/schema/Host 0.22 contracts.
- `simple-task1-cleanup.log` lists the exact inverse-feature paths. The obsolete dirty authority-test module was intentionally removed with its superseded implementation, as authorized in the brief.
- `git apply --reverse --check` against the pre-simplification working patch passed for all unrelated user paths. Only the mixed activation file and explicitly obsolete authority-test file were excluded. The user's `assert_eq!(identity.name, "demo Window")` hunk remains in the working activation file and is excluded from the feature commit. User example renames, docs, fixture archive, previews and other unrelated changes were preserved. See `simple-task1-preservation.log`.
- No reset, clean, stash, install, grant, user-process manipulation, real provider, real clipboard, native automation, push or merge was performed. Completion push is disabled.

## Production flow

1. OCR `panel.share.onSource` reads the live edited editor only when the current result is valid/ready. The actual generated Panel bridge adds the host Share action to the existing Ctrl+K menu and preserves plugin `null`/dialog suppression. Snapshot validation preserves whitespace/newlines, rejects empty/NUL/invalid Unicode, and applies the 16 KiB and synchronous two-second limits.
2. `plugin_share_begin` binds the actual calling content Webview and current Panel epoch, verifies the manifest source declaration and ordinary UiPanel grants, then reads compatible receivers from validated bundles without starting candidate Runtimes. The one source child is hidden; the existing controller suppresses bounds-driven show while sharing owns input.
3. Main pulls the private state through `plugin_share_state`. `LauncherCore` and the focused `ShareSurface` render a searchable list using stable plugin/receiver IDs, no automatic execution, target-size disabled reasons, keyboard navigation and cancel. Only actual main can read/select/end. Receiver catalogs, body and private metadata never use the general event bus.
4. Explicit selection consumes the choosing state once and revalidates the receiver. The host uses the ordinary Panel mount and empty-input command submission. `mount_shared` is only a scope check around the existing one-session replacement path; there is no second Panel, restricted Runtime, authority registry or general presentation queue.
5. The target completes normal onUpdate/ACK and settled onReady. The generated bridge awaits the ordinary asynchronous Ready callback, privately claims the body once, invokes the synchronous receiver once, and reports strict acceptance. Native checks exact content label, epoch and initialization request. The body is taken before the claim response; no response retry occurs. Full incoming/outgoing JSON packets are bounded to 65536 bytes. Startup/delivery has a 30-second deadline.
6. The frontend adopts the already opened target descriptor without a second open or submit. Until accepted it disables main input/paste/ordinary host keys and renders target loading. Errors destroy/end the uncertain target session and retain the target label and host error. A stale bounds error cannot close this error surface or return to OCR. Cancellation before selection restores only the still-current source; cancellation after selection ends the target.
7. Translation 1.5.0 receives the exact text synchronously after preferences/Ready and schedules its existing model to translate once. Business errors preserve the received source in translation and permit its existing explicit retry. A successful translation is a new source. Same-request theme updates preserve accepted text; a new empty submission still clears it. OCR is 1.1.0; both packages require Host 0.22.0.

Main integration is in `src/main.ts`, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx` and `src/plugin-share.tsx`. Native registration/ACL is in `src-tauri/src/lib.rs`, `src-tauri/build.rs`, the main/content capabilities and generated seven command permissions. Public SDK now exposes only `onSource(() => Payload | null)` and synchronous `onReceive`; unpublished revision/invalidation/preparation types were removed.

## Whole-flow evidence

`src/launcher.test.tsx` contains two complete fake-service cases using the actual generated Panel bootstrap, actual OCR/translation Panel modules, actual Runtime modules, real LauncherCore/View and searchable receiver selector. They start ordinary OCR, send a synthetic image, edit multiline OCR output, invoke content Ctrl+K Share, show both draft and translation choices, filter/select translation, and execute the fake Youdao adapter.

Both success and business-failure branches passed. Assertions establish one explicit selection despite duplicate clicks, one target claim, one translation service call, exact edited whitespace/text, initialized target language preferences, empty host suffix/query, no clipboard operation, no second frontend open/submit on adoption, and current Panel remaining translation after service failure. Separate generated-bridge tests accept an independent draft without calling business APIs and reject missing/throwing/Promise/invalid receivers without retry. Plugin tests establish output-as-source, loading/empty-source rejection, no repeated translation on theme updates and ordinary entry/repaste behavior.

Native Tauri MockRuntime verification invokes the production generated command handlers with real main and multiple child Webviews under the same main parent. Source, target and unrelated Runtime are denied main catalog/state access; only the exact target can claim once. Wrong request/old source are rejected. An unrelated Any listener receives no sharing body/catalog. Native timeout/receive failure and old receipt/mount ownership tests retain the target error and prevent replacement of a newer document.

## Actual verification

Counts overlap; they are not a combined test total. All commands used the current dirty working tree, not a claimed clean-commit fixture.

| Command/scope | Latest result | Evidence |
| --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib plugin_share` | 5 passed | `simple-task1-rust-sharing.log` |
| Exact fresh lib-test executable, filter `plugin_panel` | 98 passed | `simple-task1-rust-panel.log` |
| Same executable, filter `commands::tests::plugin_panel` | 3 passed | `simple-task1-rust-ipc.log` |
| Vitest generated Panel action/bootstrap and context-action files, unfiltered | 48 passed | `simple-task1-frontend.log` |
| Vitest `src/launcher.test.tsx -t '[pP]anel'` | 69 passed, 361 intentionally filtered | `simple-task1-launcher-panels.log` |
| Complete fake success/failure + error/late ownership cases | 4 passed | `simple-task1-flow.log` and final launcher run |
| Node all OCR/translation `tests/*.test.js` | 146 passed | `simple-task1-plugin-regressions.log` |
| Plugin CLI workspace `tests/manifest-sharing.test.ts` | 59 passed | `simple-task1-manifest-tests.log` |
| Shared SDK + OCR + translation SDK TypeScript contracts | exit 0 | `simple-task1-sdk.log` |
| Root `tsc --noEmit` | exit 0; log explicitly replaced before final run | `simple-task1-types.log` |
| Current emitted production CLI, both plugin directories | both Valid | `simple-task1-manifests.log` |
| Affected source `git diff --check` | no whitespace errors | final self-review |

Only one Cargo process tree ran at a time. Process-local `TAURI_CONFIG={"bundle":{"resources":[]}}` was used; shipping config was not edited. Final reused executable: `D:\code\UiPilot_tools\src-tauri\target\debug\deps\uipilot_lib-6ab693a2655edd46.exe`, last write `2026-11-06 22:14:16` local, 80734720 bytes, SHA256 `013B5C8D71558048A42A2665D38B391109E3D80775634D3852B04E57B991CE98`. Native sources were unchanged for the reused filters.

Behavioral RED/GREEN evidence is retained in `simple-task1-bridge-red.log` / `simple-task1-bridge-green.log`, `simple-task1-plugins-red.log` / `simple-task1-plugins-green.log`, and `simple-task1-ownership-red.log` / final flow log. Initial process-sandbox `spawn EPERM` results were not counted as behavioral RED; focused local test workers were rerun through approved escalation. Old whitelist/version assertions were updated for the intended API and version change. A manifest regression caught a missing OCR share declaration during formatting cleanup; it was restored and the actual package contract and complete affected plugin suites were rerun.

## Self-review and remaining delivery checks

Concrete issues found and fixed before review: queued old bounds rejection closing target errors; theme preservation swallowing a new empty submission; delayed source/target receipt or mount changing a newer document; cleanup using an unscoped current session; and stale API/version assertions. The final implementation keeps these guards in the existing Panel controller/session path.

The local CLI dist was stale and initially rejected new share/0.22 declarations. `npm.cmd run build --workspace @uipilot/plugin-cli` emitted current validator/bundle output but **failed** its final guard with `Error: Ajv entered the runtime bundle`. Both directory validations using that current emitted code return Valid; this does **not** make the CLI build successful. `simple-task1-cli-build.log` records the failure. The coordinating agent assigned diagnosis/fix of this strict bundle boundary to Task 2; no guard or CLI source was weakened here.

Independent whole-flow review is next. Task 2 still needs the full repository/frontend/Rust verification, strict CLI build fix, package/archives and parity checks, final public docs and delivery acceptance record. Native focus, physical child visibility, Light/Dark native rendering, real image paste/clipboard, real providers, installation/grants and native keyboard acceptance remain explicitly unperformed. Mock/JSDOM tests do not establish those native outcomes.

Feature commit: `e767472` (`feat(sharing): deliver single-panel text handoff`), 62 scoped files, 1754 insertions / 5071 deletions. The activation fixture rename is the only unstaged difference in that mixed file; both plugin manifests have no remaining unstaged formatting differences. The separate report/evidence commit is recorded in the final handoff.
