# H2/H3 Panel text effects and Host 0.18.0 report

Worktree: `C:/Users/moby/.codex/worktrees/translate-panel/UiPilot_tools`.
Branch: `codex/translate-panel-implementation`. Base: `c79f0ab`.

## Implemented

- Optional readonly, captured-session `panel.clipboard.writeText({text})`; preserves whitespace, rejects empty/NUL/>16384 UTF-8 bytes, rejects extra/accessor input fields, resolves after synchronous OS write. Authorization mutation gate precedes current-session guard and native write. No read, paste, hide, focus change or retry.
- Independent default-off `speech.speak`, legal only for Windows legacy submit Panel + ui.panel with Host >=0.18.0. Settings Permissions has a separate confirmation; installation/update lists the permission and preserves only prior explicit speech grants. SDK/schema/CLI/frontend permission unions and labels agree.
- Optional readonly speech voices/play/stop/state/onState. Exact en/zh-CHS languages and text bounds, no voiceId option; frozen snapshots, fixed sanitized error names, session-owned events and expired retained functions.
- One bounded (16 queued user operations) native speech worker. One global playback, host IDs, old-ID stop isolation for both same and different owners, stream-correlated completion. Single worker owns COM/SAPI interfaces, local voice enumeration, semicolon hex LANGID parsing (English primary 09; Simplified Chinese0804), CoTaskMemFree for returned strings, plain-text async/purge flags only. Interfaces drop before COM apartment cleanup. WebView muting/media CSP unchanged.
- Separate session and authorization leases. Every central controller invalidation revokes the session synchronously. Activation reservations revoke authorization synchronously inside manager mutation before lifecycle publication. Canceled pre-commit reservations can issue a fresh lease after the unchanged bundle is confirmed; old leases stay revoked. No-op edits preserve current authorization. Final native start serializes with both lease gates; async reply delivery rechecks them. Revocation does not enqueue into the bounded user queue: worker checks before each command and at20ms idle intervals. Failed purge retains old native ownership and prevents replacement start; revoked ownership stays tracked until native cleanup succeeds.
- Five content-only IPC registrations/capabilities/generated permission files. Main/Runtime/Window callers fail before protected service access. Host0.18.0 version sources include root package/lock, Cargo manifest/lock, Tauri config, PublicPluginHost::current and CLI target. apiVersion/schemaVersion remain1. Translation plugin package remains untouched.

## Main files

- `src-tauri/src/panel_effects.rs`, `panel_effects/native.rs`, `panel_effects.js`: bounded worker, DTOs, native driver, facades and fake tests.
- `plugin_panel.rs`, `public_plugins/activation.rs`, `public_plugins/activation_bundle.rs`, `commands.rs`: session/permission ownership and guarded native effects.
- `public_plugins/manifest.rs`, `build.rs`, `lib.rs`, capabilities and five generated permissions: version/shape validation and exact IPC isolation.
- `src/{protocol,host-credential-permissions,plugin-permissions-panel,public-plugin-panel}` and focused tests: opt-in authorization and capability exposure.
- `docs/plugin-sdk` contracts/types/schema and `packages/plugin-cli` target/semantic validator/generated schema/tests: public0.18 delivery. CLI golden target versions and genuinely-future minimum versions now track0.18; Delete-key test isolates its older key contract from the example's newer history-copy declaration.

## RED evidence

1. `npx vitest run src/plugin-panel-bootstrap.test.ts`:3 expected missing-facade failures /13 passes before implementation.
2. With the environment below, `cargo test --manifest-path src-tauri/Cargo.toml --lib panel_effects::tests -- --nocapture`:3 expected worker failures /1 text-contract pass against a stub returning SpeechUnavailable.
3. `node node_modules/vitest/vitest.mjs run src/translation-selection-hotkey.test.tsx src/plugin-permissions-panel.test.tsx`:2 expected failures /21 passes (speech was included in installation grants; no separate permission row/confirmation).
4. `cargo test --manifest-path src-tauri/Cargo.toml --lib speech_revocation_before_async_delivery -- --nocapture`:expected failure, received idle state after authorization revocation rather than ExpiredPanelSession.
5. `cargo test --manifest-path src-tauri/Cargo.toml --lib panel_effects::tests -- --nocapture`:2 expected final edge failures /5 passes: failed replacement purge lost current ownership; late async delivery accepted revoked state. Both are now fixed.

## Final GREEN evidence

All Rust commands use process-local PowerShell environment only:

```powershell
$env:CARGO_TARGET_DIR='D:/code/UiPilot_tools/src-tauri/target'
$env:TAURI_CONFIG='{"bundle":{"resources":[]}}'
```

- `cargo test --manifest-path src-tauri/Cargo.toml --lib panel_effects -- --nocapture`:8/8 pass, complete lib-test compilation including Windows native driver.
- Final `cargo test --manifest-path src-tauri/Cargo.toml --lib speech -- --nocapture`:9/9 pass, including same-session old playback ID, canceled/no-op authorization, asynchronous late-reply revocation and exact exported Rust schema-generator parity.
- Fresh final binary `D:/code/UiPilot_tools/src-tauri/target/debug/deps/uipilot_lib-3949deb779151fc7.exe`, rerun after the final build with each filter and `--quiet`:
  - `panel_effects`:8/8;
  - `text_effect`:3/3 (independent copy/speech grants, write failure, caller/epoch rejection, six controller teardown routes);
  - `plugin_panel::tests`:87/87;
  - `public_plugins::manifest::schema_tests`:19/19;
  - `public_plugin_commands_have_non_overlapping_exact_capabilities`:1/1.
- `node node_modules/vitest/vitest.mjs run src/plugin-panel-bootstrap.test.ts src/plugin-panel-action-bootstrap.test.ts src/plugin-permissions-panel.test.tsx src/translation-selection-hotkey.test.tsx`:46/46 pass. Final bootstrap-only rerun after ES-compatible test assertion adjustment:17/17 pass.
- From `packages/plugin-cli`: `node ../../node_modules/vitest/vitest.mjs run`:186/186 pass across15 files.
- Root and CLI `node .../typescript/bin/tsc --noEmit`:pass.
- `node packages/plugin-cli/scripts/sync-plugin-schema.mjs --check`:pass.
- `node packages/plugin-cli/scripts/generate-schema-validator.mjs --check`:pass.
- `git diff --check`:pass.

Intermediate build/import/type issues were corrected before these final checks. One CLI run used the repository cwd instead of the package cwd and therefore could not resolve fixtures; the full package-cwd run above is authoritative. The new exact-generator test initially used the Schema serializer instead of the exported Value generator; it now calls the actual public generator and matches the checked-in LF artifact. Existing screenshot dead-code warnings remain. A transient shared Cargo target lock belonged to an existing `cargo run`; it released naturally, with no process termination or cache copying.

## Limits and review notes

- No real speech, system clipboard writes, provider calls, credentials, input simulation or live WebViews were used. Native Windows end-to-end acceptance remains user-operated after integration; no installer/release binary is produced by this step.
- Available local SAPI voices are enumerated on worker initialization. There is no online voice installation or fallback to another language.
- Lease invalidation is deliberately conservative at any actual activation reservation (including configuration edits). It can stop existing speech, then a subsequent valid call obtains a fresh lease. Failed/canceled mutations do not permanently deny a still-authorized current Panel, and no revoked lease is resurrected.
- Revocation is synchronous; native purge follows on worker processing/polling without waiting under manager/controller locks. Native cleanup failure retains tracked ownership and blocks replacement until purge succeeds.
