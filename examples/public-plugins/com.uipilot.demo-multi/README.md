# UiPilot Demo Multi Plugin

Two mainResult commands share one offline Runtime, one prefix setting and one plugin storage namespace. This example follows demo-return's copyText response contract. It contains no network, panel, window or background capability.

| Stable ID | Initial startup name | Activation | Result |
| --- | --- | --- | --- |
| upper | /demo-upper | live | Uppercase input |
| lower | /demo-lower | submit | Lowercase input after Enter |

Each invocation keeps internal spaces. The configured prefix (default `[demo] `) is added after conversion. Both commands return the same response-local result ID, text-copy, with a host-owned copyText action. Their shared invocation-count and last-command storage demonstrate package ownership; the counter appears in the result subtitle. Storage writes are separate operations, not a multi-key transaction, and live invocations may be cancelled by newer input.

## Host 0.5 release boundary

Manifest/API remain 1; this commands package requires Host 0.5.0+. Internal state schema is 2, inventory/report schemas remain 1, and the CLI npm package version is independently 0.1.0. Current Host and production CLI target are 0.5.0. Production CLI validation and packing are covered by local automated checks; final independent release review and native Host acceptance remain separate gates.

The complete installable package lives at `package/`. Node and Rust tests read its real bytes. The original reviewed fixture was moved without changing its manifest or runtime bytes. Normal reference discovery includes this package, its Node tests and its strict SDK contract.

## Offline verification

From the repository root with the existing local dependencies:

```powershell
node --test examples/public-plugins/com.uipilot.demo-multi/tests/runtime.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-multi/tests/sdk-contract.ts
cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests --offline
```

Runtime tests use frozen synthetic invocations and a fake settings/storage Host. They cover both outputs, requestId ownership, explicit missing/unknown ID rejection, changed versions/names/order and an in-memory constructor ID variant. The fixture itself has only two product commands and exactly one runtime.js. Rust staging tests use an explicit 0.5.0 Host to validate both the real directory and a temporary archive; they also stage the unchanged return/panel/window/Pomodoro packages. The new staging checks never start Runtime/WebViews or install into user data. The exact legacy invocation key golden remains a separate Host serialization test.

## Local static pack

Use a CLI whose declared Host target is 0.5.0+ (the npm package's own version does not establish that target). From the repository root, with an existing output directory outside package/:

```powershell
uipilot-plugin validate examples/public-plugins/com.uipilot.demo-multi/package --platform windows
uipilot-plugin pack examples/public-plugins/com.uipilot.demo-multi/package --output ./com.uipilot.demo-multi.uipilot-plugin --platform windows
uipilot-plugin validate ./com.uipilot.demo-multi.uipilot-plugin --platform windows
```

Historical 0.4.0 Hosts reject both multi-item and single-item commands arrays; do not lower minimumHostVersion to bypass that boundary. The CLI has no Host-version override flag. All packing is local and static; it neither installs nor executes the plugin.

## Host 0.5.0 manual acceptance

These steps are user-operated; they have not been performed by the automated tests.

1. Open Host 0.5.0+ Settings > Plugins > Public plugins. Choose the example's release package/ development directory, or the validated archive above. Review and grant clipboard.write.
2. Search for demo-upper and demo-lower independently. Enter `/demo-upper I am  Jack`: after the live debounce, expect `[demo] I AM  JACK`. Select its result and press Enter to copy through the Host.
3. Enter `/demo-lower I am  Jack` and press Enter: expect `[demo] i am  jack`. Press Enter again to copy. Confirm that the shared subtitle counter progresses across both commands; do not assume exact counts during live typing.
4. Change Result prefix once in package settings and invoke both commands; both must use the new prefix. Disable/re-enable the package and verify both entries follow the same switch.
5. Favorite both command entries separately. Rename upper in plugin details and confirm lower keeps its name. The override replaces the upper route and releases the old name; it does not keep a second alias. Confirm keyboard navigation, selected results and text in both light and dark themes.
6. Restart Host and confirm stored data, names and favorites persist. In an isolated development copy, raise package version, reorder commands and change default names while retaining IDs; update and confirm state remains attached to IDs. Switching commands while work is pending must not display or copy stale results.

Successful updates preserve retained ID overrides/favorites; changing an ID is deletion plus addition. A successfully removed ID, followed by restart and re-add, is fresh. Legacy state transfers only through explicit default; this fixture deliberately has no default. Package rollback on the new Host follows ordinary validation/authorization/conflict rules and retains only still-present IDs. Validation rejection or a definitely uncommitted write with verified old bytes preserves the old configuration; uncertain persistence outcomes follow fail-closed recovery and do not guarantee availability or rollback.

Binary downgrade is a separate unsupported operation: once normal mutation or required recovery writes state schema 2, older Host binaries that only understand schema 1 are not guaranteed to read that data directory. There is no schema-1 writeback promise. Multi-command panel/window, per-command hotkeys, independent alias collections, actions arrays and onAction are outside this example and M2-A.

See the [API contract](../../../docs/plugin-sdk/public-plugin-v1.md) and [developer guide](../../../docs/plugin-sdk/public-plugin-developer-guide.md).
