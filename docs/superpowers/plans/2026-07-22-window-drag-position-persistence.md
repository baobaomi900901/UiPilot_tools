# 主窗口拖动与位置持久化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让无边框主窗口可从非交互空白区域拖动，并在每次隐藏和后续启动之间恢复最后位置。

**Architecture:** 复用 Tauri 原生拖动区域和现有 `SettingsStore`。共享隐藏入口在成功隐藏后原子保存一次物理坐标，共享显示入口校验坐标是否完整落在当前显示器工作区域内；有效则恢复，无效则在主显示器居中。

**Tech Stack:** React 19、TypeScript、Tauri 2.11、Rust、Serde、Vitest、Rust 内置测试。

## Global Constraints

- 基线为本地 `main@a932f69a52bb0f1b969c87c77711d0b2f69500be`。
- 只在窗口消失时写一次位置，不监听连续移动事件。
- 输入框、按钮、结果项和其他交互控件不得成为拖动区域。
- 保存位置无效时必须回退到主显示器居中。
- 位置读写失败不得阻止窗口显示、隐藏或结果清理。
- 不保存窗口大小、最大化、最小化或按显示器区分的位置。
- 不新增依赖，不改变 720x420、无边框、不可调整大小配置。

---

### Task 1: 在现有设置文件中持久化内部窗口坐标

**Files:**
- Modify: `src-tauri/src/settings.rs:17-225`
- Test: `src-tauri/src/settings.rs:533-547,725-788`

**Interfaces:**
- Produces: `WindowPosition { x: i32, y: i32 }`
- Produces: `SettingsStore::window_position() -> Option<WindowPosition>`
- Produces: `SettingsStore::set_window_position(WindowPosition) -> Result<(), SettingsError>`
- Preserves: `SettingsUpdate` and frontend settings wire contracts remain unchanged.

- [ ] **Step 1: Write the failing legacy/default and narrow-update test**

Add `window_position_defaults_and_updates_only_that_field` beside the existing file-preview and hotkey-only tests:

```rust
#[test]
fn window_position_defaults_and_updates_only_that_field() {
    let dir = TestDir::new("window-position");
    fs::write(
        dir.current(),
        br#"{"hotkey":"Ctrl+Space","autostart":true,"filePreviewEnabled":false,"researchId":"study_01","useCounts":{}}"#,
    )
    .unwrap();
    let store = SettingsStore::load(dir.path()).unwrap();
    let before = store.snapshot();

    assert_eq!(store.window_position(), None);
    let position = WindowPosition { x: -1280, y: 48 };
    store.set_window_position(position).unwrap();

    assert_eq!(store.window_position(), Some(position));
    assert_eq!(
        store.snapshot(),
        Settings {
            window_position: Some(position),
            ..before
        }
    );
    assert_eq!(read_current(&dir), store.snapshot());
}
```

- [ ] **Step 2: Run the focused Rust test and verify RED**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml settings::tests::window_position_defaults_and_updates_only_that_field
```

Expected: compilation fails because `WindowPosition`, `window_position`, and `set_window_position` do not exist.

- [ ] **Step 3: Add the minimal internal model and atomic update**

Add the type and optional settings field without changing `SettingsUpdate`:

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowPosition {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    #[serde(default = "default_file_preview_enabled")]
    pub(crate) file_preview_enabled: bool,
    pub(crate) research_id: Option<String>,
    #[serde(default)]
    pub(crate) use_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) window_position: Option<WindowPosition>,
}
```

Initialize it in `Settings::default()`:

```rust
window_position: None,
```

Add narrow accessors beside `set_file_preview_enabled` and `snapshot`:

```rust
pub(crate) fn set_window_position(
    &self,
    position: WindowPosition,
) -> Result<(), SettingsError> {
    let mut state = self.state.lock().expect("settings lock poisoned");
    let mut candidate = state.value.clone();
    candidate.window_position = Some(position);
    self.persist(&mut state, candidate)
}

pub(crate) fn window_position(&self) -> Option<WindowPosition> {
    self.state
        .lock()
        .expect("settings lock poisoned")
        .value
        .window_position
}
```

Add `window_position: None` only to existing full `Settings` literals that do not use `..Settings::default()`.

- [ ] **Step 4: Run focused and full settings tests and verify GREEN**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml settings::tests::window_position_defaults_and_updates_only_that_field
cargo test --manifest-path .\src-tauri\Cargo.toml settings::tests
```

Expected: focused test passes; all settings tests pass.

- [ ] **Step 5: Commit the settings slice**

```powershell
git add src-tauri/src/settings.rs
git commit -m "feat: 持久化主窗口位置"
```

---

### Task 2: 在共享隐藏入口保存最后位置

**Files:**
- Modify: `src-tauri/src/commands.rs:1-21,1130-1156`
- Test: `src-tauri/src/commands.rs:2231-2296`

**Interfaces:**
- Consumes: `SettingsStore::set_window_position(WindowPosition)` from Task 1.
- Preserves: `clear_and_hide(&ResultRegistry, &WebviewWindow) -> Result<(), CommandError>` so all current callers remain unchanged.
- Produces: shared order `read position -> clear registry -> hide -> best-effort persist`.

- [ ] **Step 1: Extend the shared-hide test with position and fail-soft persistence**

Replace `maintenance_shared_clear_and_hide_runs_once_in_registry_first_order` with a four-closure test:

```rust
#[test]
fn maintenance_shared_clear_and_hide_saves_after_successful_hide() {
    let trace = RefCell::new(Vec::new());
    let position = WindowPosition { x: 40, y: -20 };
    let result = clear_and_hide_with(
        || {
            trace.borrow_mut().push("position");
            Ok(position)
        },
        || trace.borrow_mut().push("clear"),
        || {
            trace.borrow_mut().push("hide");
            Ok(())
        },
        |saved| {
            assert_eq!(saved, position);
            trace.borrow_mut().push("save");
            Err(())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(*trace.borrow(), ["position", "clear", "hide", "save"]);
}
```

Add the failure boundary test:

```rust
#[test]
fn maintenance_shared_clear_and_hide_ignores_position_failure_but_not_hide_failure() {
    assert_eq!(
        clear_and_hide_with(
            || Err(()),
            || {},
            || Ok(()),
            |_| panic!("missing position must not be saved"),
        ),
        Ok(())
    );
    assert_eq!(
        clear_and_hide_with(
            || Ok(WindowPosition { x: 1, y: 2 }),
            || {},
            || Err(()),
            |_| panic!("failed hide must not be persisted"),
        ),
        Err(CommandError::window_failed())
    );
}
```

Update the existing simulated-show-failure call to pass `|| Err(())` as position reader and `|_| Ok(())` as saver.

- [ ] **Step 2: Run the focused command tests and verify RED**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml commands::tests::maintenance_shared_clear_and_hide
```

Expected: compilation fails because `clear_and_hide_with` still accepts two closures.

- [ ] **Step 3: Capture before hide and persist only after a successful hide**

Import `WindowPosition` with the existing settings imports, then change the production wrapper:

```rust
pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    let settings = window.state::<SettingsStore>();
    clear_and_hide_with(
        || {
            window
                .outer_position()
                .map(|position| WindowPosition {
                    x: position.x,
                    y: position.y,
                })
                .map_err(|_| ())
        },
        || registry.hide_and_clear(),
        || window.hide().map_err(|_| ()),
        |position| settings.set_window_position(position).map_err(|_| ()),
    )
}
```

Replace the helper with:

```rust
fn clear_and_hide_with<P, C, H, S>(
    read_position: P,
    clear: C,
    hide: H,
    save_position: S,
) -> Result<(), CommandError>
where
    P: FnOnce() -> Result<WindowPosition, ()>,
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
    S: FnOnce(WindowPosition) -> Result<(), ()>,
{
    let position = read_position();
    clear();
    hide().map_err(|_| CommandError::window_failed())?;
    if let Ok(position) = position {
        let _ = save_position(position);
    }
    Ok(())
}
```

- [ ] **Step 4: Run focused and full command tests and verify GREEN**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml commands::tests::maintenance_shared_clear_and_hide
cargo test --manifest-path .\src-tauri\Cargo.toml commands::tests
```

Expected: focused tests and all command tests pass; `hide_launcher` still uses only the shared wrapper.

- [ ] **Step 5: Commit the shared-hide slice**

```powershell
git add src-tauri/src/commands.rs
git commit -m "feat: 隐藏窗口时保存位置"
```

---

### Task 3: 显示前恢复有效位置或在主显示器居中

**Files:**
- Modify: `src-tauri/src/lifecycle.rs:9-29,564-573,1036-1128`
- Test: `src-tauri/src/lifecycle.rs:2081-2322`

**Interfaces:**
- Consumes: `SettingsStore::window_position()` and `WindowPosition` from Task 1.
- Produces: `position_fits_work_area(WindowPosition, PhysicalSize<u32>, PhysicalRect<i32, u32>) -> bool`.
- Produces: `centered_position(PhysicalSize<u32>, PhysicalRect<i32, u32>) -> Option<WindowPosition>`.
- Preserves: placement failure is ignored before the existing always-on-top, show, focus, registry and emit sequence.

- [ ] **Step 1: Add failing physical-coordinate tests**

Add beside the existing show tests:

```rust
#[test]
fn saved_window_position_must_fit_one_complete_work_area() {
    let size = PhysicalSize::new(720, 420);
    let left = PhysicalRect {
        position: PhysicalPosition::new(-1920, 0),
        size: PhysicalSize::new(1920, 1040),
    };

    assert!(position_fits_work_area(
        WindowPosition { x: -1920, y: 0 },
        size,
        left,
    ));
    assert!(position_fits_work_area(
        WindowPosition { x: -720, y: 620 },
        size,
        left,
    ));
    assert!(!position_fits_work_area(
        WindowPosition { x: -719, y: 620 },
        size,
        left,
    ));
    assert!(!position_fits_work_area(
        WindowPosition { x: -1920, y: 621 },
        size,
        left,
    ));
}

#[test]
fn invalid_position_falls_back_to_primary_work_area_center() {
    let primary = PhysicalRect {
        position: PhysicalPosition::new(100, 40),
        size: PhysicalSize::new(1920, 1040),
    };
    assert_eq!(
        centered_position(PhysicalSize::new(720, 420), primary),
        Some(WindowPosition { x: 700, y: 350 })
    );
    assert_eq!(
        centered_position(PhysicalSize::new(2000, 420), primary),
        None
    );
}
```

- [ ] **Step 2: Run the focused lifecycle tests and verify RED**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml lifecycle::tests::saved_window_position_must_fit_one_complete_work_area
cargo test --manifest-path .\src-tauri\Cargo.toml lifecycle::tests::invalid_position_falls_back_to_primary_work_area_center
```

Expected: compilation fails because the placement helpers do not exist.

- [ ] **Step 3: Implement checked placement helpers and production fallback**

Extend the Tauri and settings imports:

```rust
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, WebviewWindow,
};

use crate::settings::{Settings, SettingsStore, SettingsUpdate, WindowPosition};
```

Add the pure checked helpers near `ShowMainClosures`:

```rust
fn position_fits_work_area(
    position: WindowPosition,
    window_size: PhysicalSize<u32>,
    work_area: PhysicalRect<i32, u32>,
) -> bool {
    let left = i64::from(position.x);
    let top = i64::from(position.y);
    let right = left + i64::from(window_size.width);
    let bottom = top + i64::from(window_size.height);
    let area_left = i64::from(work_area.position.x);
    let area_top = i64::from(work_area.position.y);
    let area_right = area_left + i64::from(work_area.size.width);
    let area_bottom = area_top + i64::from(work_area.size.height);
    left >= area_left && top >= area_top && right <= area_right && bottom <= area_bottom
}

fn centered_position(
    window_size: PhysicalSize<u32>,
    work_area: PhysicalRect<i32, u32>,
) -> Option<WindowPosition> {
    let x_offset = work_area.size.width.checked_sub(window_size.width)? / 2;
    let y_offset = work_area.size.height.checked_sub(window_size.height)? / 2;
    Some(WindowPosition {
        x: i32::try_from(i64::from(work_area.position.x) + i64::from(x_offset)).ok()?,
        y: i32::try_from(i64::from(work_area.position.y) + i64::from(y_offset)).ok()?,
    })
}

fn place_main_window(
    window: &WebviewWindow,
    saved: Option<WindowPosition>,
) -> Result<(), ()> {
    let window_size = match window.outer_size() {
        Ok(size) => size,
        Err(_) => return window.center().map_err(|_| ()),
    };
    if let Some(saved) = saved {
        if window.available_monitors().is_ok_and(|monitors| {
            monitors
                .iter()
                .any(|monitor| position_fits_work_area(saved, window_size, *monitor.work_area()))
        }) && window
            .set_position(PhysicalPosition::new(saved.x, saved.y))
            .is_ok()
        {
            return Ok(());
        }
    }
    if let Ok(Some(primary)) = window.primary_monitor() {
        if let Some(position) = centered_position(window_size, *primary.work_area()) {
            if window
                .set_position(PhysicalPosition::new(position.x, position.y))
                .is_ok()
            {
                return Ok(());
            }
        }
    }
    window.center().map_err(|_| ())
}
```

Rename `ShowMainClosures.center` to `place_window`. In `show_main`, read the internal position once and wire the production closure:

```rust
let saved_position = app.state::<SettingsStore>().window_position();
let mut place_window = || place_main_window(&window, saved_position);
```

In `show_main_core`, keep the existing fail-soft behavior:

```rust
let _ = (operations.place_window)();
```

Update `run_show_case`, `ShowFailure::Center` to `ShowFailure::Placement`, traces from `center-N` to `place-N`, and both direct `ShowMainClosures` test constructions. Do not change the order after placement.

- [ ] **Step 4: Run focused and full lifecycle tests and verify GREEN**

Run:

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml lifecycle::tests::saved_window_position_must_fit_one_complete_work_area
cargo test --manifest-path .\src-tauri\Cargo.toml lifecycle::tests::invalid_position_falls_back_to_primary_work_area_center
cargo test --manifest-path .\src-tauri\Cargo.toml lifecycle::tests
```

Expected: coordinate tests and all lifecycle tests pass; trace order is `place -> always-on-top -> show -> focus -> registry -> emit`.

- [ ] **Step 5: Commit the restore slice**

```powershell
git add src-tauri/src/lifecycle.rs
git commit -m "feat: 显示窗口时恢复位置"
```

---

### Task 4: 启用并标记原生空白拖动区域

**Files:**
- Modify: `src/launcher-view.tsx:255-320,330-458,463-552`
- Modify: `src-tauri/capabilities/main.json`
- Test: `src/launcher.test.tsx:1252-1300,1651-1720`
- Test: `src-tauri/src/lib.rs:945-952`

**Interfaces:**
- Consumes: Tauri built-in `data-tauri-drag-region` behavior.
- Produces: main WebView permission `core:window:allow-start-dragging`.
- Preserves: no frontend window API import, no custom pointer state, and no drag attribute on controls or result rows.

- [ ] **Step 1: Add failing DOM and capability tests**

Add a launcher view test using the existing `startedCore` and `mountLauncherView` helpers:

```tsx
it('marks blank surfaces but not controls or result rows as native drag regions', async () => {
  installMatchMedia(false)
  const { core, client, emit } = await startedCore()
  vi.mocked(client.searchApps).mockResolvedValueOnce({
    requestId: 'drag-regions',
    items: [{ resultId: 'drag-result', title: 'Drag Result' }],
  })
  const mounted = await mountLauncherView(core)
  await act(async () => emit(shown('drag-regions')))
  await act(async () =>
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'drag', inputType: 'insertText' }),
  )
  await vi.waitFor(() => expect(document.querySelector('.result-row')).toBeInstanceOf(HTMLElement))

  for (const selector of ['.launcher-surface', '.launcher-view', '.result-list', '.status-region']) {
    expect(document.querySelector(selector)?.getAttribute('data-tauri-drag-region')).toBe('true')
  }
  for (const selector of ['input', '.result-row']) {
    expect(document.querySelector(selector)?.hasAttribute('data-tauri-drag-region')).toBe(false)
  }

  await act(async () => emit(shown('drag-settings', 'settings')))
  await vi.waitFor(() => expect(document.querySelector('.settings-form')).toBeInstanceOf(HTMLElement))
  for (const selector of ['.settings-view', '.settings-header', '.settings-form']) {
    expect(document.querySelector(selector)?.getAttribute('data-tauri-drag-region')).toBe('true')
  }
  for (const selector of ['input', 'button']) {
    expect(document.querySelector(selector)?.hasAttribute('data-tauri-drag-region')).toBe(false)
  }

  await mounted.unmount()
})
```

Extend `production_file_index_state_commands_and_permissions_are_exact`:

```rust
assert!(capability.contains("\"core:window:allow-start-dragging\""));
assert!(!capability.contains("\"core:window:default\""));
```

- [ ] **Step 2: Run focused frontend and Rust tests and verify RED**

Run:

```powershell
npm test -- --run src/launcher.test.tsx -t "marks blank surfaces"
cargo test --manifest-path .\src-tauri\Cargo.toml lib::tests::production_file_index_state_commands_and_permissions_are_exact
```

Expected: DOM test fails because drag attributes are absent; Rust test fails because permission is absent.

- [ ] **Step 3: Mark only blank containers and add the narrow permission**

Add the explicit native attribute to these existing elements:

```tsx
<section className="launcher-view" aria-label="应用启动器" data-tauri-drag-region="true">
<div id="launcher-results" className="result-list" role="listbox" aria-label="搜索结果" data-tauri-drag-region="true">
<section className="file-workspace" aria-label="文件搜索" data-tauri-drag-region="true">
<div id="file-results" className="result-list file-result-list" role="listbox" aria-label="文件结果" data-tauri-drag-region="true">
<section className="settings-view" aria-label="设置" data-tauri-drag-region="true">
<header className="settings-header" data-tauri-drag-region="true">
<div className="settings-loading" data-tauri-drag-region="true">
<Form component="div" layout="vertical" className="settings-form" data-tauri-drag-region="true">
<main className="launcher-surface" data-color-scheme={dark ? 'dark' : 'light'} data-tauri-drag-region="true">
<div className="status-region" role="status" aria-live="polite" aria-atomic="true" data-tauri-drag-region="true">
```

Do not add the attribute to `BoundInput`, Ant Design controls, `.result-row`, `.file-preview`, `.file-category-strip`, or `.file-toolbar`.

Add exactly one permission to `src-tauri/capabilities/main.json`:

```json
"core:window:allow-start-dragging"
```

- [ ] **Step 4: Run focused tests and verify GREEN**

Run:

```powershell
npm test -- --run src/launcher.test.tsx -t "marks blank surfaces"
cargo test --manifest-path .\src-tauri\Cargo.toml lib::tests::production_file_index_state_commands_and_permissions_are_exact
```

Expected: both tests pass.

- [ ] **Step 5: Commit the drag-region slice**

```powershell
git add src/launcher-view.tsx src/launcher.test.tsx src-tauri/capabilities/main.json src-tauri/src/lib.rs
git commit -m "feat: 支持从空白区域拖动窗口"
```

---

### Task 5: 完整验证并准备人工验收

**Files:**
- Verify only; do not add generated permission or build artifacts.

**Interfaces:**
- Consumes all previous tasks.
- Produces a clean feature branch ready for user-run `npm run tauri dev`.

- [ ] **Step 1: Format and run static checks**

Run:

```powershell
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
npm run build
```

Expected: all commands exit 0 with no warnings treated as errors.

- [ ] **Step 2: Run the complete automated suites**

Run:

```powershell
npm test
cargo test --manifest-path .\src-tauri\Cargo.toml
```

Expected: at least the baseline `106` frontend tests and `409` Rust tests pass, plus the new tests; the two established script-only Rust tests remain ignored.

- [ ] **Step 3: Remove build-only line-ending noise and verify the branch**

If Tauri regenerated only line endings under `src-tauri/permissions/autogenerated`, restore those generated files, then run:

```powershell
git diff --check
git status --short --branch
git log --oneline main..HEAD
```

Expected: no unstaged/generated changes; branch contains the plan and four focused feature commits.

- [ ] **Step 4: Hand off exact manual test cases**

Provide the worktree path and these cases:

```text
1. npm run tauri dev，唤起 Launcher。
2. 从输入框外的空白区域按住拖动；输入框、按钮、结果行、滚动仍正常。
3. 移到屏幕内新位置，点击别处触发隐藏，再用快捷键唤起；位置不变。
4. 关闭 dev 进程并重新启动，再次唤起；位置不变。
5. 打开设置页，从标题栏/表单空白处拖动；所有设置控件仍正常。
6. 多屏环境下将窗口放到副屏并隐藏；拔掉副屏后再次唤起，应在主屏幕居中。
```

Do not merge into `main` until the user reports manual acceptance.
