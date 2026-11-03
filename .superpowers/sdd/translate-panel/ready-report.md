# Panel settled readiness report

## Scope and contract

Additive optional SDK `panel.onReady(handler): () => void`, available on Host
0.18.0. Register exactly one active receiver during boot **before the first call
to `panel.onUpdate`**. Late registration rejects, including after an onUpdate
unsubscribe. A ready unsubscribe invalidates already queued callbacks; no replay.
The plugin should register onReady first, then onUpdate; onUpdate installs state
and DOM, onReady starts initial translation. No plugin package was changed.

Native delivery waits for successful exact update ACK settlement, then publishes
through `publish_settled_ready` under the controller mutex. The method checks
complete current PanelSessionIdentity (including content label, generation,
activation/admission identity), epoch/request/submission token, Ready and pending
None, then claims an exact ticket marker once. Publication is protected by the
same mutex as teardown/replacement; it only queues JS and never waits for its
receiver. Initial mount and resubmit share deliver_panel_update/deliver_update.
No notification is emitted from ACK or content-ready handlers.

JS records a new current identity/generation before awaiting onUpdate, resets
ownership on PREPARE including unchanged epoch, and drops wrong identity,
duplicate, superseded and pagehide notices. Queued invocation rechecks both
update generation/object and receiver token. Receiver throw/rejection is
contained without reporting exception text, retrying, re-ACKing or awaiting it.
No request admission/security or clipboard/speech effect behavior changed.

## RED evidence

- `npm test -- src/plugin-panel-bootstrap.test.ts`: eight new executable
  readiness tests failed with `api.onReady is not a function`; prior 17 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml settled_ready --lib`: new
  exact-ticket tests failed compilation on missing `publish_settled_ready`.

## GREEN evidence

- `npm test -- src/plugin-panel-bootstrap.test.ts src/plugin-panel-action-bootstrap.test.ts`: exit 0, 32/32 passing. Covers delayed >500ms
  absence of any notice, wrong identity, frozen event, duplicate, queued newer
  UPDATE before asynchronous handler settlement, same-epoch PREPARE, unsubscribe,
  pagehide, synchronous throw/asynchronous rejection, and exactly one Runtime
  request from ready without an extra ACK.
- `npx tsc --noEmit`: exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml plugin_panel::tests --lib`: exit 0, 90/90 passing, including three new tests for exact ticket,
  owner mismatch, ACK-before-settlement suppression, duplicate settlement,
  A ACK with buffered B, promoted B, B once after its own ACK, teardown/replacement,
  delayed 510ms ACK and publication holding the teardown/replacement gate.
- `cargo test --manifest-path src-tauri/Cargo.toml panel_action_ --lib`: exit 0, 13/13 passing, including update/ACK Busy, lifecycle revocation, credential-change retirement, scheduler cancellation, timeout and forged/expired owner protections.
- `git diff --check`: exit 0.

Native environment: process-local `CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target`
and `TAURI_CONFIG={"bundle":{"resources":[]}}`; no user process was killed.
The existing screenshot model dead-code warning remains unrelated.

## Changed files

- src-tauri/src/plugin_panel.rs: hook bootstrap, guarded native publication,
  focused tests and public member extraction expectation.
- src-tauri/src/public_plugins/tests.rs: real request admission stays Busy during
  update and acknowledged-but-unsettled phases, then admits one request.
- src/plugin-panel-bootstrap.test.ts and src/plugin-panel-action-bootstrap.test.ts:
  executable SDK bridge regression coverage and fixture support.
- docs/plugin-sdk/uipilot-plugin-api-v1.d.ts, public-plugin-v1.md and
  public-plugin-developer-guide.md: additive API and integration timing.
- docs/superpowers/specs/2026-11-03-translate-panel-host-design.md: final hook contract.

## Concerns and integration notes

No known implementation blocker. Native WebView scheduling remains a real-Host
acceptance boundary; executable bootstrap and controller tests cover ownership
and ordering without opening a GUI or generating provider/speech effects.
Pagehide invalidates the document receiver; native teardown independently blocks
publication and stale requests. Do not schedule translation from onUpdate using
a timer/microtask: use the new hook and capture/check the current update object
in plugin code. No automatic retry is introduced.
