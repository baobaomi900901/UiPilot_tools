# Host Fix Request: `requestHide()` Commit Path Silent Failure

**Date:** 2026-08-26  
**Status:** Open — blocks Notes manual QA  
**Requested by:** Third-party plugin developer (`com.uipilot.notes`)  
**Depends on:** `c2ff520` (`PanelHideAdmitResult` / `hideTicketId` serialization — merged)  
**Related:** `docs/superpowers/specs/2026-08-26-public-plugin-panel-key-routing-and-hide-design.md` §3.2

## Symptom (Notes 1.1.2 + Host `c2ff520`)

1. Host rebuilt with `c2ff520`; Notes **1.1.2** reinstalled.
2. Open `/notes`, focus list, press **Enter**.
3. Status shows **「已复制」** (copy path OK).
4. **Launcher stays visible** — no hide.
5. **No** plugin toast **「复制成功，但无法隐藏窗口」**.

## Interpretation

Plugin `requestHide().catch(...)` only runs when the **admit** invoke rejects or bootstrap throws `windowFailed`.

Seeing **「已复制」** without the error toast means:

```text
requestHide() Promise resolved (admit returned admitted or noop)
```

Bootstrap **resolves the public Promise on admit**; `plugin_panel_request_hide_commit` runs **fire-and-forget** (`setTimeout(0)`) and errors are **swallowed** in bootstrap:

```javascript
void invoke('plugin_panel_request_hide_commit', { ... }).catch(() => undefined);
```

So the plugin **cannot distinguish** “hide scheduled” from “hide failed silently”.

## Likely failure modes (Host-side)

### A. Admit returns `noop` (Promise still resolves)

`admit_hide` returns `None` when session/label mismatch **or** `core.hide_ticket.is_some()`:

```rust
if !current || core.hide_ticket.is_some() {
    return Ok(None);
}
```

**Stuck `hide_ticket`:** After a prior admit, if commit/hide fails without teardown, `hide_ticket` remains set. **All later `requestHide()` calls noop** while the launcher stays open. User still sees copy success every time.

`hide_ticket` is only cleared on `open_session`, `teardown_*`, and `host_hidden` — **not** after `claim_hide_commit`.

### B. Commit skipped with silent `Ok(())`

`plugin_panel_request_hide_commit`:

```rust
if controller
    .live_identity()
    .is_none_or(|session| session.content_label != webview.label())
{
    return Ok(());
}
```

If this guard fails after a successful admit, commit is dropped with **no error** and no hide.

### C. `commit_panel_hide` / `schedule_committed_panel_hide` swallow errors

- `claim_hide_commit` returns `None` → `commit_panel_hide` returns `Ok(false)` → command still maps to `Ok(())`.
- `schedule_committed_panel_hide` uses `let _ = clear_and_hide_reason(...)` inside `run_on_main_thread` — hide failure is discarded.
- `get_webview_window("main")` returning `None` exits the closure without propagating failure.

### D. Bootstrap epoch desync (admit `noop`)

Hide invokes embed `sessionEpoch` from mount-time bootstrap (`__SESSION_EPOCH__`).  
`__UIPILOT_PLUGIN_PANEL_PREPARE__` updates `storageSession.sessionEpoch` only for **storage**, not for hide/focus/ready invokes.

If a live session epoch ever diverges from baked bootstrap (webview reuse without re-injection), **storage can work** while **hide admit noops**.

## Required Host fixes

1. **End-to-end integration test (Windows dev):** mount notes panel → content `requestHide()` → assert main window hidden within 1s (not only unit tests on controller).
2. **Commit must not silent-Ok after admitted ticket:** If `live_identity`/`webview.label()` guard fails **after** admit allocated a ticket, fail loudly or run terminal cleanup — do not return `Ok(())` leaving a stuck ticket.
3. **Clear or supersede `hide_ticket`** on commit terminal outcome (success **or** unrecoverable failure) so a later `requestHide()` can admit again in the same session when hide did not occur.
4. **Propagate hide failures:** `schedule_committed_panel_hide` / `clear_and_hide_reason` errors must not be discarded when commit was claimed from an admitted ticket (log + 30s fallback is insufficient for UX).
5. **Bootstrap hide uses live epoch:** Hide/focus/ready invokes should use the same epoch as `storageSession` after `PREPARE` (or re-inject bootstrap on session replacement).
6. **Verify 30s admitted fallback** actually hides in real app when primary commit fails (manual QA currently suggests it does not within normal interaction).

## Acceptance criteria

- [ ] Notes 1.1.2: list focused → Enter → **「已复制」** and launcher hides **immediately** (same as Escape clean hide).
- [ ] Second Enter hide in the same session after a failed first attempt is not permanently blocked by stuck `hide_ticket`.
- [ ] Regression test for admit JSON (`hideTicketId`) remains green (`panel_hide_admit_result_matches_bootstrap_contract`).

## Plugin scope

Notes already calls `requestHide()` after successful list Enter copy. **No further plugin change** can force launcher hide if Host commit/noop fails after Promise resolve.

## Manual diagnostics for QA

| Observation | Implies |
|-------------|---------|
| 「已复制」 only, no error | Admit resolved (`admitted` or `noop`); commit/hide did not complete |
| 「复制成功，但无法隐藏窗口」 | Admit invoke rejected / `windowFailed` (pre-`c2ff520` serialization bug) |
| Hide after ~30s | Primary commit broken; fallback may work |
| Escape hides but Enter copy does not | Same `requestHide` — unlikely unless Enter never calls it (focus routing); if both call it, behavior should match |

Before retrying, **close the notes tag / panel session** to clear a stuck `hide_ticket`, then reopen `/notes`.
