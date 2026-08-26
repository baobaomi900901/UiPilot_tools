# Host Fix Request: `PanelHideAdmitResult` camelCase Serialization

**Date:** 2026-08-26
**Status:** Implemented — manual QA required
**Requested by:** Third-party plugin developer (`com.uipilot.notes`)
**Related spec:** `docs/superpowers/specs/2026-08-26-public-plugin-panel-key-routing-and-hide-design.md` §3.2
**Host version:** 0.3.1
**Blocking plugin:** `com.uipilot.notes` v1.1.1 — list Enter copy + `requestHide()`

## Summary

`plugin_panel_request_hide_admit` returns JSON that violates the frozen panel
contract. Panel bootstrap rejects the response, so `requestHide()` fails even
when Rust admit succeeds. Third-party plugins cannot work around this in plugin
code alone.

## Observed behavior

1. Open `/notes` panel session (Host 0.3.1, notes 1.1.1).
2. Focus a list item; press **Enter** to copy note content.
3. Copy succeeds (`已复制`).
4. Launcher does **not** hide.

With notes 1.1.1 error handling enabled, the plugin shows:

> 复制成功，但无法隐藏窗口

That message means `window.uipilotPluginPanel.requestHide()` Promise **rejected**
after a successful copy — not a plugin copy failure.

## Root cause

### Contract (frozen)

Design §3.2 and bootstrap (`PUBLIC_PANEL_BOOTSTRAP_TEMPLATE` in
`src-tauri/src/plugin_panel.rs`) require:

```ts
type PanelHideAdmitResult =
  | { outcome: 'admitted'; hideTicketId: U64Decimal }
  | { outcome: 'noop' }
```

Bootstrap validation (abbreviated):

```javascript
if (
  result?.outcome !== 'admitted' || typeof result.hideTicketId !== 'string' ||
  !/^[1-9][0-9]*$/.test(result.hideTicketId)
) throw new Error('windowFailed');
```

### Actual Host output today

`PanelHideAdmitResult` in `src-tauri/src/commands.rs` uses:

```rust
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(crate) enum PanelHideAdmitResult {
    Admitted { hide_ticket_id: String },
    Noop,
}
```

`rename_all = "camelCase"` renames enum variants (`Admitted` → `"admitted"`) but
**does not** rename struct-variant fields. Serde therefore emits:

```json
{"outcome":"admitted","hide_ticket_id":"1"}
```

Bootstrap reads `result.hideTicketId` → `undefined` → throws `windowFailed` →
Promise rejects.

### Side effect on Host state

Admit may succeed in Rust (`PanelHideTicket` installed) before bootstrap rejects.
Subsequent `requestHide()` calls can return `{ outcome: 'noop' }` while the
launcher stays visible, producing silent failure (copy OK, no hide, no error).

### Precedent in the same file

`PluginPanelHostKeyEnqueueResult` already serializes correctly:

```rust
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PluginPanelHostKeyEnqueueResult {
    Enqueued { route_sequence: String },
    // ...
}
```

There is an existing unit test
`panel_host_key_enqueue_result_uses_camel_case_fields` proving
`routeSequence` (not `route_sequence`). `PanelHideAdmitResult` should follow
the same pattern.

## Required Host change

**File:** `src-tauri/src/commands.rs`

Add `rename_all_fields = "camelCase"` to `PanelHideAdmitResult`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PanelHideAdmitResult {
    Admitted { hide_ticket_id: String },
    Noop,
}
```

Expected serialized admit response:

```json
{"outcome":"admitted","hideTicketId":"1"}
```

No bootstrap, SDK, or plugin changes required once Host matches the contract.

## Required verification

Add a unit test mirroring `panel_host_key_enqueue_result_uses_camel_case_fields`:

```rust
#[test]
fn panel_hide_admit_result_matches_bootstrap_contract() {
    assert_eq!(
        serde_json::to_value(PanelHideAdmitResult::Admitted {
            hide_ticket_id: "42".into(),
        })
        .unwrap(),
        serde_json::json!({
            "outcome": "admitted",
            "hideTicketId": "42"
        })
    );
    assert_eq!(
        serde_json::to_value(PanelHideAdmitResult::Noop).unwrap(),
        serde_json::json!({ "outcome": "noop" })
    );
}
```

Run:

```bash
cargo test panel_hide --manifest-path src-tauri/Cargo.toml
```

### Manual QA (after Host rebuild)

1. Rebuild/run Host 0.3.1+ with the fix.
2. Install notes `package/` v1.1.1.
3. `/notes` → select list item → **Enter**.
4. Expect: content copied **and** launcher hidden (foreground restored best-effort).

## Out of scope for this request

- Notes plugin changes (already calls `requestHide()` after list Enter copy).
- Bootstrap changes (correct per approved design).
- Schema / SDK version bump (field names already documented as `hideTicketId`).

## Acceptance criteria

- [ ] `plugin_panel_request_hide_admit` success payload uses `hideTicketId` (camelCase).
- [ ] Unit test prevents regression (same class as host-key enqueue test).
- [ ] `com.uipilot.notes` list Enter → copy → hide works end-to-end on Windows dev build.
