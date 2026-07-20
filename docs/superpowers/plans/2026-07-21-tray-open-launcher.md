# Tray Open Launcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 托盘菜单增加「打开主界面」，复用现有 `ShowTarget::Launcher` 唤起路径。

**Architecture:** 在 `lifecycle::tray_action` 增加 namespaced ID `uipilot.tray.open-launcher` → `TrayAction::Show(Launcher)`；`lib.rs` 托盘菜单按「打开主界面 → 打开设置 → 退出」组装，并将 `Show(target)` 统一转发给 `request_show`。不改窗口生命周期、不绑定托盘图标左键。

**Tech Stack:** Rust、Tauri 2 tray/menu API、现有 `LifecycleCoordinator::request_show`。

**Source Design:** `docs/superpowers/specs/2026-07-21-tray-open-launcher-design.md`

## Global Constraints

- 菜单顺序与文案固定：`打开主界面` → `打开设置` → `退出`
- 只复用 `ShowTarget::Launcher`；不新增 ShowTarget / lifecycle 状态
- 不增加托盘图标左键 / 双击打开主界面
- 不新增前端 command、capability、UI
- 每个任务先 RED 再 GREEN，任务结束单独 commit

## File Map

| File | Responsibility |
|---|---|
| `src-tauri/src/lifecycle.rs` | `TRAY_OPEN_LAUNCHER` 常量、`tray_action` 映射、精确 ID 单元测试 |
| `src-tauri/src/lib.rs` | 托盘菜单项组装、`on_menu_event` Show 转发、接线源码断言 |

---

### Task 1: Map tray ID to `Show(Launcher)`

**Files:**
- Modify: `src-tauri/src/lifecycle.rs`（常量区约 L35–50；测试 `tray_accepts_only_exact_namespaced_ids` 约 L3110–3126）
- Test: same file `#[cfg(test)]` module

**Interfaces:**
- Consumes: existing `ShowTarget`, `TrayAction`, `tray_action`
- Produces:
  - `pub(crate) const TRAY_OPEN_LAUNCHER: &str = "uipilot.tray.open-launcher";`
  - `tray_action(TRAY_OPEN_LAUNCHER) == Some(TrayAction::Show(ShowTarget::Launcher))`

- [ ] **Step 1: Write the failing test assertions**

In `tray_accepts_only_exact_namespaced_ids`, add the launcher mapping assertion and keep existing Settings / Quit / rejection cases. Exact shape:

```rust
#[test]
fn tray_accepts_only_exact_namespaced_ids() {
    assert_eq!(
        tray_action(TRAY_OPEN_LAUNCHER),
        Some(TrayAction::Show(ShowTarget::Launcher))
    );
    assert_eq!(
        tray_action(TRAY_OPEN_SETTINGS),
        Some(TrayAction::Show(ShowTarget::Settings))
    );
    assert_eq!(tray_action(TRAY_QUIT), Some(TrayAction::Quit));
    for rejected in [
        "open-settings",
        "open-launcher",
        "quit",
        "uipilot.tray.open",
        "UIPILOT.TRAY.QUIT",
        "uipilot.tray.quit ",
        "uipilot.tray.open-launcher ",
        "",
    ] {
        assert_eq!(tray_action(rejected), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml tray_accepts_only_exact_namespaced_ids -- --nocapture
```

Expected: FAIL — `TRAY_OPEN_LAUNCHER` not found, or mapping not equal to `Show(Launcher)`.

- [ ] **Step 3: Minimal implementation**

Near existing tray constants / `tray_action`:

```rust
pub(crate) const TRAY_OPEN_LAUNCHER: &str = "uipilot.tray.open-launcher";
pub(crate) const TRAY_OPEN_SETTINGS: &str = "uipilot.tray.open-settings";
pub(crate) const TRAY_QUIT: &str = "uipilot.tray.quit";

pub(crate) fn tray_action(id: &str) -> Option<TrayAction> {
    match id {
        TRAY_OPEN_LAUNCHER => Some(TrayAction::Show(ShowTarget::Launcher)),
        TRAY_OPEN_SETTINGS => Some(TrayAction::Show(ShowTarget::Settings)),
        TRAY_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}
```

Do not change any other lifecycle behavior in this task.

- [ ] **Step 4: Run test to verify it passes**

Run the same `cargo test` command as Step 2.

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/lifecycle.rs
git commit -m "feat: map tray open-launcher action id"
```

---

### Task 2: Wire tray menu item and Show forwarding

**Files:**
- Modify: `src-tauri/src/lib.rs`（托盘菜单组装约 L107–136；接线测试中 `request_show(..., Launcher)` 计数约 L423–428）
- Test: `src-tauri/src/lib.rs` `#[cfg(test)]` 源码片段断言

**Interfaces:**
- Consumes: `lifecycle::TRAY_OPEN_LAUNCHER`, `TrayAction::Show(ShowTarget)`, `LifecycleCoordinator::request_show`
- Produces: tray menu order `打开主界面` → `打开设置` → `退出`; menu event forwards any `Show(target)` via `request_show(app, target)`

- [ ] **Step 1: Write the failing wiring assertions**

In the production wiring test that asserts tray / `request_show` fragments (the one that already checks `"tauri::tray::TrayIconBuilder::new()"`):

1. Keep the literal launcher count at `2` (single-instance + shortcut only — tray will use `request_show(app, target)`, not a third literal).
2. **Remove** the old assert that counts `request_show(app, ShowTarget::Settings)` as `1` (that literal disappears once Show is generalized).
3. **Add** these asserts (or equivalent fragments in the existing list):

```rust
assert_eq!(
    production
        .matches("request_show(app, ShowTarget::Launcher)")
        .count(),
    2
);
assert!(production.contains("lifecycle::TRAY_OPEN_LAUNCHER"));
assert!(production.contains("打开主界面"));
assert!(production.contains("Some(lifecycle::TrayAction::Show(target))"));
assert!(production.contains("tray_coordinator.request_show(app, target)"));
assert!(production.contains("lifecycle::TRAY_OPEN_SETTINGS"));
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml production_lifecycle_wires_one_coordinator_and_exact_event_sources -- --nocapture
```

Expected: FAIL — missing `TRAY_OPEN_LAUNCHER` / `打开主界面` / `Show(target)` fragments (finish Step 1 fully — including removing the old Settings literal count — before this run).

- [ ] **Step 3: Minimal implementation**

Replace the tray menu construction / event handler with:

```rust
let open_launcher = tauri::menu::MenuItem::with_id(
    app,
    lifecycle::TRAY_OPEN_LAUNCHER,
    "打开主界面",
    true,
    None::<&str>,
)
.map_err(|_| lifecycle_setup_error())?;
let open_settings = tauri::menu::MenuItem::with_id(
    app,
    lifecycle::TRAY_OPEN_SETTINGS,
    "打开设置",
    true,
    None::<&str>,
)
.map_err(|_| lifecycle_setup_error())?;
let quit =
    tauri::menu::MenuItem::with_id(app, lifecycle::TRAY_QUIT, "退出", true, None::<&str>)
        .map_err(|_| lifecycle_setup_error())?;
let menu = tauri::menu::Menu::with_items(app, &[&open_launcher, &open_settings, &quit])
    .map_err(|_| lifecycle_setup_error())?;
let icon = app
    .default_window_icon()
    .cloned()
    .ok_or_else(lifecycle_setup_error)?;
let tray_coordinator = Arc::clone(coordinator);
tauri::tray::TrayIconBuilder::new()
    .icon(icon)
    .menu(&menu)
    .on_menu_event(
        move |app, event| match lifecycle::tray_action(event.id().as_ref()) {
            Some(lifecycle::TrayAction::Show(target)) => {
                let _ = tray_coordinator.request_show(app, target);
            }
            Some(lifecycle::TrayAction::Quit) => tray_coordinator.request_tray_quit(app),
            _ => {}
        },
    )
    .build(app)
    .map_err(|_| lifecycle_setup_error())?;
```

Do **not** add `on_tray_icon_event` / left-click handlers.

- [ ] **Step 4: Run tests to verify they pass**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml tray_accepts_only_exact_namespaced_ids -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --nocapture
```

Expected: all pass. If full `--lib` is too slow in this environment, at minimum run the specific wiring test from Step 2 plus `tray_accepts_only_exact_namespaced_ids`.

- [ ] **Step 5: Manual smoke (optional but recommended on Windows)**

```powershell
npm run tauri dev
```

Right-click tray icon → confirm menu order `打开主界面` / `打开设置` / `退出` → click `打开主界面` → launcher search UI appears (same as hotkey).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat: add tray open-launcher menu item"
```

---

## Spec Coverage Self-Review

| Spec requirement | Task |
|---|---|
| Menu order / 文案「打开主界面」 | Task 2 |
| ID `uipilot.tray.open-launcher` → `Show(Launcher)` | Task 1 |
| `request_show(..., Launcher)` path (via shared Show forwarding) | Task 2 |
| No tray icon left-click | Task 2 explicitly forbids `on_tray_icon_event` |
| Extend exact-ID unit test | Task 1 |
| Update wiring assertions (`Show(target)` + menu fragments; literal Launcher count stays 2) | Task 2 |
| No new ShowTarget / frontend / commands | Global Constraints + both tasks |

Placeholder scan: none. Type names match existing `ShowTarget` / `TrayAction` / `tray_action`.
