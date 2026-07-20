# Hotkey Recorder + Double-Tap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 设置页可录制组合键与双击 Ctrl/Alt，保存进现有 `hotkey` 字段，重启后按类型重新注册并全局唤起。

**Architecture:** 新增 crate-private `hotkey` 解析层（`Chord` / `DoubleTap`）。组合键仍走 `global-shortcut`；双击用可单测的 400ms 状态机 + Windows LL hook 适配器。前端用纯录制 reducer 写入 canonical 字符串，经现有 `save_settings` 持久化；`LifecycleCoordinator` 在保存与启动 reconcile 时互斥装卸 chord/钩子。

**Tech Stack:** Rust、Tauri 2 `global-shortcut`、`windows` crate LL hook、React/AntD 设置页、Vitest。

**Source Design:** `docs/superpowers/specs/2026-07-21-hotkey-recorder-design.md`

## Global Constraints

- 单一 `hotkey` 字符串字段；规范双击值仅 `DoubleCtrl` / `DoubleAlt`（大小写敏感）
- 双击间隔固定 400ms；仅 Ctrl / Alt
- 录制：点击输入框开始；Esc / 失焦取消；录制中禁止手打；不自动 save
- Chord 走 `global-shortcut`；DoubleTap 走 Windows LL hook；切换与退出必须互斥卸干净
- 钩子回调不写按键内容日志；错误仅固定类别
- 不改 invocation / result registry / 失焦隐藏语义；不做可调间隔、多快捷键、非 Windows 双击
- 每个任务 TDD（RED→GREEN）并单独 commit；PowerShell 环境下 commit 用 `git commit -m "..."`（勿用 bash heredoc）

## File Map

| File | Responsibility |
|---|---|
| `src-tauri/src/hotkey.rs` | 解析/规范化 `HotkeyKind`；展示无关的 canonical 规则 |
| `src-tauri/src/double_tap.rs` | 纯 400ms 双击状态机（钩子与单测共用） |
| `src-tauri/src/hotkey_hook.rs` | Windows LL hook 安装/卸载；把按键喂给状态机并回调 |
| `src-tauri/src/commands.rs` | `prepare_settings_save` 接受 DoubleTap |
| `src-tauri/src/lifecycle.rs` | 保存事务 / reconcile / 退出时协调 chord↔hook |
| `src-tauri/src/lib.rs` | `mod` 声明与退出路径卸钩接线断言 |
| `src/hotkey-recorder.ts` | 前端录制 reducer + 展示文案 |
| `src/launcher-core.ts` | `setHotkeyCanonical`（绕过 insertText） |
| `src/launcher-view.tsx` | 快捷键录制控件替换 BoundInput |
| `src/hotkey-recorder.test.ts` / `src/launcher.test.tsx` | 前端测试 |

---

### Task 1: Rust `HotkeyKind` parse + normalize

**Files:**
- Create: `src-tauri/src/hotkey.rs`
- Modify: `src-tauri/src/lib.rs` — add `mod hotkey;` beside other production mods（同一 `cfg` 门闩）

**Interfaces:**
- Consumes: `tauri_plugin_global_shortcut::Shortcut`
- Produces:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DoubleTapModifier {
    Ctrl,
    Alt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HotkeyKind {
    Chord(Shortcut),
    DoubleTap(DoubleTapModifier),
}

pub(crate) const DOUBLE_CTRL: &str = "DoubleCtrl";
pub(crate) const DOUBLE_ALT: &str = "DoubleAlt";
pub(crate) const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

impl HotkeyKind {
    pub(crate) fn parse(raw: &str) -> Result<Self, ()>;
    pub(crate) fn canonical(&self) -> String; // Double* exact, or shortcut.to_string()
}
```

- [ ] **Step 1: Write failing tests** in `hotkey.rs` `#[cfg(test)]`:

```rust
#[test]
fn parses_double_tap_exact_and_rejects_aliases() {
    assert_eq!(
        HotkeyKind::parse(DOUBLE_CTRL),
        Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl))
    );
    assert_eq!(
        HotkeyKind::parse(DOUBLE_ALT),
        Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Alt))
    );
    for rejected in ["doublectrl", "Double Ctrl", "double-ctrl", "DOUBLECTRL", ""] {
        assert_eq!(HotkeyKind::parse(rejected), Err(()));
    }
}

#[test]
fn parses_chord_and_canonicalizes_via_shortcut() {
    let kind = HotkeyKind::parse("Ctrl+Space").unwrap();
    match &kind {
        HotkeyKind::Chord(shortcut) => {
            assert_eq!(kind.canonical(), shortcut.to_string());
        }
        _ => panic!("expected chord"),
    }
    assert!(HotkeyKind::parse("not a shortcut").is_err());
}
```

- [ ] **Step 2: Run RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml parses_double_tap_exact_and_rejects_aliases -- --nocapture
```

Expected: FAIL (module/type missing).

- [ ] **Step 3: Implement `hotkey.rs` + `mod hotkey`**

`parse`: exact `DoubleCtrl`/`DoubleAlt` first；else `raw.parse::<Shortcut>().map(HotkeyKind::Chord).map_err(|_| ())`.  
`canonical`: match Double* → const str；Chord → `shortcut.to_string()`.

- [ ] **Step 4: GREEN** — same tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/hotkey.rs src-tauri/src/lib.rs
git commit -m "feat: parse chord and double-tap hotkey kinds"
```

---

### Task 2: Save preflight accepts DoubleTap

**Files:**
- Modify: `src-tauri/src/commands.rs` — `prepare_settings_save` and callers/`save_settings_with` worker types that currently require `Shortcut`

**Interfaces:**
- Consumes: `hotkey::HotkeyKind`
- Produces: `prepare_settings_save(...) -> Result<(HotkeyKind, SettingsUpdate), CommandError>` where `update.hotkey == kind.canonical()`

- [ ] **Step 1: Extend failing/updated tests**

In `save_settings_preflight_rejects_invalid_input_before_worker_dispatch`, keep `"not a shortcut"` rejected；add rejection for `"doublectrl"`.

In `save_settings_preflight_accepts_valid_input_without_persisting`（或并列新测试）:

```rust
#[test]
fn save_settings_preflight_accepts_double_tap_without_shortcut_parse() {
    let dir = TestDir::new();
    let settings_store = settings_store(&dir, Some("study_01"));
    let cache = AppCache::from_apps(settings_applications());
    let (kind, update) = prepare_settings_save(
        user_settings("DoubleCtrl", None, &[]),
        SaveSettingsCache(&cache),
    )
    .unwrap();
    assert_eq!(kind, HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl));
    assert_eq!(update.hotkey, "DoubleCtrl");
    // worker not required — preflight only
    let _ = settings_store;
}
```

Update `save_settings_with` / worker签名：把 `Shortcut` 参数改为 `HotkeyKind`（所有测试里的 `move |_, _, _|` 闭包保持三参数即可）。

- [ ] **Step 2: RED** — run the new/updated preflight tests；Expect FAIL on DoubleCtrl path still using `parse::<Shortcut>()`.

- [ ] **Step 3: Minimal implementation**

```rust
fn prepare_settings_save(
    settings: UserSettingsUpdate,
    cache: SaveSettingsCache<'_>,
) -> Result<(HotkeyKind, SettingsUpdate), CommandError> {
    let kind = HotkeyKind::parse(&settings.hotkey).map_err(|_| CommandError::settings_failed())?;
    let update = SettingsUpdate {
        hotkey: kind.canonical(),
        autostart: settings.autostart,
        research_id: settings.research_id,
        aliases: settings.aliases,
    };
    SettingsStore::validate_user_settings(&update, cache.inner())?;
    Ok((kind, update))
}
```

Propagate `HotkeyKind` through `save_settings_with` → worker → `save_settings` 调用 `save_settings_transaction`（Task 3/4 会改 transaction；本任务若 transaction 仍要 `Shortcut`，可暂时在 `save_settings` 里：

```rust
let HotkeyKind::Chord(shortcut) = &kind else { return Err(()) }; // ONLY if Task 3 not done
```

**Prefer sequencing:** 本任务只改 `prepare_settings_save` + 测试；`save_settings` 生产路径在 Task 4 接通 DoubleTap 事务。若本任务结束时生产 `save_settings` 仍把 `HotkeyKind` 传给旧的 `save_settings_transaction(Shortcut)`，则：

```rust
match &kind {
    HotkeyKind::Chord(shortcut) => coordinator.save_settings_transaction(..., *shortcut, update),
    HotkeyKind::DoubleTap(_) => Err(()), // temporary — removed in Task 4
}
```

更好：Task 2 **仅**提取 `prepare_settings_save` 与单测 `prepare_settings_save` 直接调用；不改 async `save_settings` 生产行为直到 Task 4。这样避免临时 Err。

**本任务锁定范围：** 只改 `prepare_settings_save` 返回类型 + 所有直接调用它的测试；`save_settings_with` 若必须改泛型，同步改测试 worker，生产 `save_settings` 在 Task 4 再接 DoubleTap。

- [ ] **Step 4: GREEN** preflight tests.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/commands.rs
git commit -m "feat: accept DoubleCtrl/DoubleAlt in settings preflight"
```

---

### Task 3: Double-tap detector (pure, 400ms)

**Files:**
- Create: `src-tauri/src/double_tap.rs`
- Modify: `src-tauri/src/lib.rs` — `mod double_tap;`

**Interfaces:**

```rust
use crate::hotkey::{DoubleTapModifier, DOUBLE_TAP_WINDOW};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TapKey {
    Ctrl,
    Alt,
    Other,
}

#[derive(Debug, Default)]
pub(crate) struct DoubleTapDetector {
    pending: Option<(DoubleTapModifier, Instant)>,
}

impl DoubleTapDetector {
    pub(crate) fn on_key_down(&mut self, key: TapKey, now: Instant) -> Option<DoubleTapModifier> {
        // see Step 3
    }

    pub(crate) fn reset(&mut self) { self.pending = None; }
}
```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn double_ctrl_within_window_fires_once() {
    let mut d = DoubleTapDetector::default();
    let t0 = Instant::now();
    assert_eq!(d.on_key_down(TapKey::Ctrl, t0), None);
    assert_eq!(
        d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(399)),
        Some(DoubleTapModifier::Ctrl)
    );
    assert_eq!(d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(400)), None); // new first
}

#[test]
fn outside_window_restarts_pending() {
    let mut d = DoubleTapDetector::default();
    let t0 = Instant::now();
    assert_eq!(d.on_key_down(TapKey::Alt, t0), None);
    assert_eq!(d.on_key_down(TapKey::Alt, t0 + Duration::from_millis(401)), None);
    assert_eq!(
        d.on_key_down(TapKey::Alt, t0 + Duration::from_millis(500)),
        Some(DoubleTapModifier::Alt)
    );
}

#[test]
fn other_key_clears_pending() {
    let mut d = DoubleTapDetector::default();
    let t0 = Instant::now();
    assert_eq!(d.on_key_down(TapKey::Ctrl, t0), None);
    assert_eq!(d.on_key_down(TapKey::Other, t0 + Duration::from_millis(10)), None);
    assert_eq!(d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(20)), None);
}
```

- [ ] **Step 2: RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml double_ctrl_within_window_fires_once -- --nocapture
```

- [ ] **Step 3: Implement**

```rust
pub(crate) fn on_key_down(&mut self, key: TapKey, now: Instant) -> Option<DoubleTapModifier> {
    let modifier = match key {
        TapKey::Ctrl => DoubleTapModifier::Ctrl,
        TapKey::Alt => DoubleTapModifier::Alt,
        TapKey::Other => {
            self.pending = None;
            return None;
        }
    };
    match self.pending.take() {
        Some((pending, at)) if pending == modifier && now.duration_since(at) <= DOUBLE_TAP_WINDOW => {
            Some(modifier)
        }
        _ => {
            self.pending = Some((modifier, now));
            None
        }
    }
}
```

Note: first test’s “third Ctrl at +400ms” after a successful fire must see empty pending → starts new first (returns None). Ensure successful match clears pending (via `take`).

- [ ] **Step 4: GREEN**

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/double_tap.rs src-tauri/src/lib.rs
git commit -m "feat: add 400ms double-tap detector"
```

---

### Task 4: Hook adapter + lifecycle chord/double-tap coordination

**Files:**
- Create: `src-tauri/src/hotkey_hook.rs`
- Modify: `src-tauri/src/lifecycle.rs` — `save_settings_transaction`, `reconcile_runtime_settings*`, exit/tray quit cleanup
- Modify: `src-tauri/src/commands.rs` — production `save_settings` passes `HotkeyKind`
- Modify: `src-tauri/src/lib.rs` — `mod hotkey_hook;`, wiring asserts for uninstall on exit

**Interfaces:**

```rust
// hotkey_hook.rs
pub(crate) struct HotkeyHook;

impl HotkeyHook {
    pub(crate) fn install(modifier: DoubleTapModifier, on_match: Arc<dyn Fn() + Send + Sync>) -> Result<Self, ()>;
    pub(crate) fn uninstall(self); // or &mut self -> Result
}

// lifecycle: replace Shortcut-only transaction with HotkeyKind
pub(crate) fn save_settings_transaction(
    &self,
    app: &AppHandle,
    settings: &SettingsStore,
    cache: &AppCache,
    kind: HotkeyKind,
    update: SettingsUpdate,
) -> Result<(), ()>;
```

Runtime state must track either registered chord(s) **or** an installed hook, never both after a successful apply.

- [ ] **Step 1: Write failing coordination tests**（lifecycle，注入假 register/unregister/install/uninstall）

```rust
#[test]
fn switching_chord_to_double_tap_unregisters_shortcut_before_hook_install() {
    // trace order: unregister(old chord) -> install(DoubleCtrl) -> persist
}

#[test]
fn switching_double_tap_to_chord_uninstalls_hook_before_register() {
    // uninstall -> register(new chord) -> persist
}

#[test]
fn reconcile_double_alt_installs_hook_without_shortcut_register() {
    // parse DoubleAlt path: register_shortcut call count == 0, install == 1
}
```

实现可用 `reconcile_runtime_settings_with` 扩展签名，增加 `install_hook` / `uninstall_hook` 闭包；或新 `apply_hotkey_binding_with`.

- [ ] **Step 2: RED**

- [ ] **Step 3: Implement coordination + hook**

**Coordination algorithm (persist-after-side-effects, mirror existing transaction):**

1. Read persisted kind via `HotkeyKind::parse(&persisted.hotkey)`.
2. If requested is Chord: `uninstall_hook_if_any`; apply existing shortcut register/unregister transaction toward requested shortcut；persist `update`.
3. If requested is DoubleTap: unregister all registered shortcuts；`install_hook(modifier)`（若已是同一 modifier 则 no-op）；persist；on failure rollback hook/shortcuts like现有 rollback。
4. `reconcile_runtime_settings` on startup: same apply with `persisted == requested` empty-clear semantics — for DoubleTap only install hook + autostart reconcile（无 shortcut register）。

**Hook (`hotkey_hook.rs`):**
- `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`
- On `WM_KEYDOWN` / `WM_SYSKEYDOWN`: map `vkCode` — `VK_CONTROL`/`VK_LCONTROL`/`VK_RCONTROL` → Ctrl；`VK_MENU`/`VK_LMENU`/`VK_RMENU` → Alt；else Other。喂给进程内 `Mutex<DoubleTapDetector>`；若 `Some(mod)` 且等于 installed target，调用 `on_match`（主线程调度 `request_show(Launcher)` — 用 `app.run_on_main_thread` 或现有 show 路径）。
- `uninstall`: `UnhookWindowsHookEx`；clear detector。
- `test-instrumentation` / 安全探针构建：不要安装真钩子（与 settings load 隔离规则一致 — 跟现有 `cfg` 门闩）。

**Exit:** 在 `request_tray_quit` / `RunEvent::Exit` / session-end 已有清理旁路调用 `uninstall_hook_if_any`。加 lib 源码断言片段含 `uninstall` 或 `HotkeyHook` 卸装符号。

- [ ] **Step 4: GREEN** — coordination tests + `cargo test --manifest-path src-tauri/Cargo.toml --lib`（若过慢，至少 hotkey/double_tap/lifecycle 相关 + commands preflight）。

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/hotkey_hook.rs src-tauri/src/lifecycle.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: coordinate chord and double-tap hotkey runtime"
```

---

### Task 5: Frontend recorder reducer

**Files:**
- Create: `src/hotkey-recorder.ts`
- Create: `src/hotkey-recorder.test.ts`

**Interfaces:**

```ts
export type RecorderStatus = 'idle' | 'recording'

export type RecorderState = {
  status: RecorderStatus
  baseline: string // value to restore on cancel
  pendingTap?: { modifier: 'Ctrl' | 'Alt'; atMs: number }
}

export type RecorderEvent =
  | { type: 'start'; baseline: string }
  | { type: 'blur' }
  | { type: 'keydown'; key: string; code: string; ctrl: boolean; alt: boolean; shift: boolean; meta: boolean; repeat: boolean; nowMs: number }
  | { type: 'cancel' } // Escape

export type RecorderResult = {
  state: RecorderState
  commit?: string // canonical hotkey
  display: string // what the input shows
}

export const DOUBLE_TAP_WINDOW_MS = 400
export function formatHotkeyDisplay(canonical: string): string
export function reduceHotkeyRecorder(state: RecorderState, event: RecorderEvent): RecorderResult
```

Display mapping: `DoubleCtrl` → `双击 Ctrl`；`DoubleAlt` → `双击 Alt`；else insert spaces around `+` or show canonical；recording → `按下快捷键…`.

- [ ] **Step 1: Failing Vitest cases**

```ts
it('commits Ctrl+Space chord', () => {
  let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
  let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
  state = r.state
  r = reduceHotkeyRecorder(state, {
    type: 'keydown',
    key: ' ',
    code: 'Space',
    ctrl: true,
    alt: false,
    shift: false,
    meta: false,
    repeat: false,
    nowMs: 0,
  })
  expect(r.commit).toBe('Ctrl+Space') // or whatever Shortcut canonical the UI will save; use the same token the backend accepts — prefer building 'Ctrl+Space' from modifiers+key
  expect(r.state.status).toBe('idle')
})

it('commits DoubleCtrl within 400ms', () => { /* two Control keydowns */ })
it('does not commit DoubleCtrl outside 400ms on second press alone', () => { /* second becomes new pending */ })
it('Escape restores baseline without commit', () => {})
it('blur cancels without commit', () => {})
```

Chord canonical builder（前端）固定规则：修饰键顺序 `Ctrl`/`Shift`/`Alt`/`Meta`，主键 `Space` 对空格、单字母大写等 — 与常见 Tauri 解析兼容的 `Ctrl+Space` 形式；保存时 Rust 再 `canonical()`。

- [ ] **Step 2: RED** — `npx vitest run src/hotkey-recorder.test.ts`

- [ ] **Step 3: Implement reducer**

规则对齐 spec：单独修饰不 commit；双击仅 Ctrl/Alt；`repeat: true` 忽略；`Escape` → cancel；`blur`/`cancel` → idle + baseline display。

- [ ] **Step 4: GREEN**

- [ ] **Step 5: Commit**

```powershell
git add src/hotkey-recorder.ts src/hotkey-recorder.test.ts
git commit -m "feat: add hotkey recorder reducer"
```

---

### Task 6: Wire settings UI + core canonical setter

**Files:**
- Modify: `src/launcher-core.ts` — add `setHotkeyCanonical(value: string): void`
- Modify: `src/launcher-view.tsx` — replace hotkey `BoundInput` with recorder control
- Modify: `src/launcher.test.tsx` — update hotkey interactions to recorder flow（不再 `ordinaryInput` 打字改 hotkey）

**Interfaces:**
- `setHotkeyCanonical(canonical: string)`: requires `settingsEditable()`；sets hotkey TextControl draft；clears shownNotice；`publish(true)`；**does not** call `save_settings`.

- [ ] **Step 1: Failing tests**

```ts
it('records hotkey via canonical setter without saving', async () => {
  // open settings, call core.setHotkeyCanonical('DoubleCtrl')
  // expect draft/display path shows 双击 Ctrl
  // expect tauri save_settings not called until explicit save
})

it('save persists DoubleCtrl through save_settings payload', async () => {
  core.setHotkeyCanonical('DoubleCtrl')
  // trigger existing save action
  // expect invoke payload hotkey === 'DoubleCtrl'
})
```

Update old tests that used `ordinaryInput` on hotkey to use `setHotkeyCanonical` or simulated recorder commits.

- [ ] **Step 2: RED**

```powershell
npx vitest run src/launcher.test.tsx
```

- [ ] **Step 3: Implement**

`setHotkeyCanonical`:

```ts
function setHotkeyCanonical(value: string): void {
  if (!settingsEditable() || !model.settings) return
  const changed = setControlDraft(model.settings.hotkey.key, value)
  model.shownNotice = undefined
  publish(changed)
}
```

Export on core object beside `text` / `setAutostart`.

UI control（设置快捷键 Form.Item）:
- `readOnly` input；`value={recording ? '按下快捷键…' : formatHotkeyDisplay(settings.hotkey.value)}`
- `onFocus` / `onClick` → start recorder with baseline `settings.hotkey.value`
- `onKeyDown` → `preventDefault`；`reduceHotkeyRecorder`；若 `commit` → `core.setHotkeyCanonical(commit)`
- `onBlur` → cancel recorder
- Escape → cancel

Do not use `BoundInput` insertText for hotkey.

- [ ] **Step 4: GREEN** — vitest launcher + recorder tests；手动可选 `npm run tauri dev`：录制 Ctrl+Space、保存、重启、确认仍生效；再试双击 Ctrl。

- [ ] **Step 5: Commit**

```powershell
git add src/launcher-core.ts src/launcher-view.tsx src/launcher.test.tsx
git commit -m "feat: wire hotkey recorder into settings ui"
```

---

## Spec Coverage Self-Review

| Spec requirement | Task |
|---|---|
| Click-to-record / Esc / blur cancel / no typing | 5, 6 |
| Chord + DoubleCtrl/DoubleAlt | 1, 5 |
| 400ms window | 3, 5 |
| Canonical persist + restart register | 2, 4, 6 |
| Chord via global-shortcut | 4 |
| DoubleTap via LL hook | 4 |
| Mutual exclusion + exit uninstall | 4 |
| prepare_settings_save DoubleTap | 2 |
| Frontend bypass insertText | 6 |
| Non-goals respected | Global Constraints |

Placeholder scan: none intentional. `HotkeyKind` / `DoubleTapModifier` / `DOUBLE_TAP_WINDOW` names consistent across tasks.
