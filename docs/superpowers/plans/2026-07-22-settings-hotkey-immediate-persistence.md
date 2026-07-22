# Settings Hotkey Immediate Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 设置页录制快捷键后，不点击全量“保存”也能立即且仅持久化/激活快捷键。

**Architecture:** 复用现有 settings operation 所有权、Tauri command、`CriticalReservation`、`LifecycleCoordinator` 热键事务和 `SettingsStore` 原子持久化。新增一条 hotkey-only 前后端调用，payload 只含 hotkey，成功只回传 canonical hotkey，不全量 reload settings view，避免覆盖 aliases、Research ID、autostart 草稿。

**Tech Stack:** React 19、Vitest、Tauri 2、Rust 1.96、`tauri-plugin-global-shortcut`、现有 Windows double-tap hook。

## Global Constraints

- 基线：本地 `main@4d63bfc98f130ef2dc9c35dfc5d85ffb7040b52a`，当前 worktree 分支 `codex/settings-hotkey-persistence-activation`。
- 规格：`docs/superpowers/specs/2026-07-22-settings-hotkey-immediate-persistence-design.md`。
- 不调用全量 `saveSettings()` 来处理录制完成。
- hotkey-only payload 只能包含 hotkey，不能包含 aliases、Research ID、autostart。
- 成功后本次运行立即生效，重启后 durable settings 一致。
- 失败时 hotkey UI 恢复旧 durable 值，其他草稿保留；状态不确定时 fail closed。
- 不新增依赖、不新增通用 settings patch 框架、不加临时 debug 日志、不 push/release。
- 每个任务先 RED 再 GREEN；提交必须是详细中文 commit。

---

## File Map

| File | Responsibility |
| --- | --- |
| `src/protocol.ts` | 新增 hotkey-only IPC 类型与 `LauncherClient.saveHotkey`。 |
| `src/main.ts` | 映射 `saveHotkey` 到 Tauri `save_hotkey`。 |
| `src/launcher-core.ts` | 新增 hotkey operation，录制后即时保存，保留其他草稿。 |
| `src/launcher-view.tsx` | 录制 commit 改走即时保存 API。 |
| `src/launcher.test.tsx` | 前端 RED/GREEN 覆盖 payload、失败、pending、旧响应。 |
| `src-tauri/src/settings.rs` | 新增 hotkey-only 原子更新，锁内只替换 hotkey。 |
| `src-tauri/src/lifecycle.rs` | 提取/新增 hotkey-only 事务入口，复用现有补偿。 |
| `src-tauri/src/commands.rs` | 新增 `save_hotkey` command 与 command 单元测试。 |
| `src-tauri/src/lib.rs` | 注册 `save_hotkey` Tauri command。 |

---

### Task 1: 前端 hotkey-only 客户端与 core 状态

**Files:**
- Modify: `src/protocol.ts`
- Modify: `src/main.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher-view.tsx`
- Test: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: recorder commit canonical string, existing `settingsOperation`.
- Produces:

```ts
export interface HotkeySettingsUpdate {
  hotkey: string
}

export interface HotkeySettingsView {
  hotkey: string
}

// LauncherClient
saveHotkey(input: { hotkey: HotkeySettingsUpdate }): Promise<HotkeySettingsView>

// LauncherCore
saveHotkeyCanonical(value: string): Promise<void>
```

- [ ] **Step 1: Write RED tests**

Add tests near existing settings save tests in `src/launcher.test.tsx`:

```ts
it('records hotkey through dedicated save without saving other drafts', async () => {
  const { core, client } = await settingsCore()
  const settings = core.getSnapshot().settings!
  core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
  core.setAutostart(true)

  await core.saveHotkeyCanonical('DoubleCtrl')

  expect(client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } })
  expect(client.saveSettings).not.toHaveBeenCalled()
  expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl')
  expect(core.getSnapshot().settings!.researchId.value).toBe('research_1')
  expect(core.getSnapshot().settings!.autostart).toBe(true)
})

it('restores durable hotkey and preserves other drafts after dedicated save failure', async () => {
  const { core, client } = await settingsCore()
  const settings = core.getSnapshot().settings!
  core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
  vi.mocked(client.saveHotkey).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })

  await core.saveHotkeyCanonical('DoubleCtrl')

  expect(core.getSnapshot().settings!.hotkey.value).toBe('Alt+Space')
  expect(core.getSnapshot().settings!.researchId.value).toBe('research_1')
  expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
})

it('keeps one settings operation while dedicated hotkey save is pending', async () => {
  const { core, client } = await settingsCore()
  const pendingHotkey = deferred<{ hotkey: string }>()
  vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)

  const pending = core.saveHotkeyCanonical('DoubleCtrl')
  void core.saveSettings()
  void core.saveHotkeyCanonical('DoubleAlt')

  expect(client.saveHotkey).toHaveBeenCalledOnce()
  expect(client.saveSettings).not.toHaveBeenCalled()
  expect(core.getSnapshot().settings).toMatchObject({ operation: 'hotkey' })
  pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
  await pending
})

it('does not let stale dedicated hotkey response overwrite a newer settings view', async () => {
  const { core, client, emit } = await settingsCore()
  const pendingHotkey = deferred<{ hotkey: string }>()
  vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)

  const pending = core.saveHotkeyCanonical('DoubleCtrl')
  emit(shown('new-settings', 'settings'))
  pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
  await pending

  expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
})
```

Also update the fake client in the test setup:

```ts
saveHotkey: vi.fn(async (input) => ({ hotkey: input.hotkey.hotkey })),
```

- [ ] **Step 2: Run RED**

Run:

```powershell
npm test -- src/launcher.test.tsx -t "dedicated hotkey|hotkey save"
```

Expected: FAIL because `saveHotkey`, `saveHotkeyCanonical`, and operation `'hotkey'` do not exist.

- [ ] **Step 3: Implement minimal frontend**

In `src/protocol.ts`:

```ts
export interface HotkeySettingsUpdate {
  hotkey: string
}

export interface HotkeySettingsView {
  hotkey: string
}
```

Add to `LauncherClient`:

```ts
saveHotkey(input: { hotkey: HotkeySettingsUpdate }): Promise<HotkeySettingsView>
```

Update settings operation union in both protocol snapshot and core private type:

```ts
operation?: 'load' | 'save' | 'hotkey' | 'rescan' | 'export' | 'clear'
type SettingsOperationKind = 'load' | 'save' | 'hotkey' | 'rescan' | 'export' | 'clear'
```

In `src/main.ts`:

```ts
saveHotkey: (input) => invoke<HotkeySettingsView>('save_hotkey', input),
```

In `src/launcher-core.ts`, expose `saveHotkeyCanonical`. Minimal shape:

```ts
async function saveHotkeyCanonical(value: string): Promise<void> {
  if (!settingsEditable()) return
  const settings = model.settings!
  const previous = settings.hotkey.value
  const operation = startSettingsOperation('hotkey')
  if (!operation) return
  setControlDraft(settings.hotkey.key, value)
  settings.hotkey.value = value
  publish(true)
  try {
    const result = await client.saveHotkey({ hotkey: { hotkey: value } })
    if (!ownsSettingsOperation(operation)) return
    if (!ownsSettingsView(operation)) {
      model.settingsNeedsReload = true
      releaseSettingsOperation(operation)
      publish(true)
      return
    }
    setControlDraft(settings.hotkey.key, result.hotkey)
    settings.hotkey.value = result.hotkey
    releaseSettingsOperation(operation)
    publish(true)
  } catch (error) {
    if (!ownsSettingsOperation(operation)) return
    const current = ownsSettingsView(operation)
    releaseSettingsOperation(operation)
    if (current) {
      setControlDraft(settings.hotkey.key, previous)
      settings.hotkey.value = previous
      model.status = errorText(error)
    } else {
      model.settingsNeedsReload = true
    }
    publish(true)
  }
}
```

If `setControlDraft` is unavailable for this use, inline the same TextControl assignment pattern already used by `setHotkeyCanonical`. Keep the existing `setHotkeyCanonical` for full-save tests and non-recorder code.

In `src/launcher-view.tsx`, change recorder commit:

```tsx
if (result.commit) void core.saveHotkeyCanonical(result.commit)
```

- [ ] **Step 4: Run GREEN**

Run:

```powershell
npm test -- src/launcher.test.tsx -t "hotkey"
```

Expected: hotkey-related frontend tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/protocol.ts src/main.ts src/launcher-core.ts src/launcher-view.tsx src/launcher.test.tsx
git commit -m "功能：前端录制快捷键后专用保存" -m "新增 hotkey-only 前端协议和 core operation。录制完成只调用 saveHotkey，不调用全量 saveSettings，避免误保存 aliases、Research ID 和 autostart 草稿。失败时恢复旧 durable hotkey 并保留其他草稿，pending 和陈旧响应沿用现有 settings operation 所有权。"
```

---

### Task 2: Rust hotkey-only 持久化与运行时事务

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lifecycle.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `HotkeyKind`, `SettingsStore`, existing `apply_hotkey_settings_transaction`.
- Produces:

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HotkeySettingsUpdate {
    hotkey: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HotkeySettingsView {
    hotkey: String,
}

impl SettingsStore {
    pub(crate) fn update_hotkey(&self, hotkey: String) -> Result<(), SettingsError>;
}

impl LifecycleCoordinator {
    pub(crate) fn save_hotkey_transaction(
        self: &Arc<Self>,
        app: &AppHandle,
        settings: &SettingsStore,
        kind: HotkeyKind,
        hotkey: String,
    ) -> Result<(), ()>;
}
```

- [ ] **Step 1: Write RED tests**

In `settings.rs` tests, add:

```rust
#[test]
fn update_hotkey_only_preserves_other_settings() {
    let dir = TestDir::new();
    let store = SettingsStore::load(dir.path()).unwrap();
    let cache = AppCache::from_apps(vec![Application { app_id: APP_A.into(), display_name: "A".into(), ..Application::default() }]);
    store.update_user_settings(update(Some("study_01"), &[(APP_A, &["alias"])]), &cache).unwrap();

    store.update_hotkey("DoubleCtrl".into()).unwrap();

    let snapshot = store.snapshot();
    assert_eq!(snapshot.hotkey, "DoubleCtrl");
    assert_eq!(snapshot.research_id.as_deref(), Some("study_01"));
    assert_eq!(snapshot.aliases[APP_A], ["alias"]);
}
```

Adapt the `Application` construction to the existing test helper style in `settings.rs`; do not add a new helper if an existing one already covers this.

In `commands.rs` tests, add:

```rust
#[test]
fn prepare_hotkey_save_accepts_double_tap_and_rejects_extra_fields() {
    let update: HotkeySettingsUpdate = serde_json::from_value(serde_json::json!({
        "hotkey": "DoubleCtrl"
    })).unwrap();
    let (kind, view) = prepare_hotkey_save(update).unwrap();
    assert_eq!(kind, HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl));
    assert_eq!(view.hotkey, "DoubleCtrl");

    assert!(serde_json::from_value::<HotkeySettingsUpdate>(serde_json::json!({
        "hotkey": "DoubleCtrl",
        "autostart": true
    })).is_err());
    assert!(prepare_hotkey_save(HotkeySettingsUpdate { hotkey: "doublectrl".into() }).is_err());
}
```

Add a worker/unit test parallel to `save_settings_worker_state_uses_managed_singletons` verifying the worker receives managed `SettingsStore` and calls `save_hotkey_transaction`.

- [ ] **Step 2: Run RED**

Run:

```powershell
$env:CARGO_INCREMENTAL='0'; cargo test --manifest-path src-tauri/Cargo.toml update_hotkey_only_preserves_other_settings prepare_hotkey_save -- --nocapture
```

Expected: FAIL because the new types/functions do not exist.

- [ ] **Step 3: Implement minimal Rust**

In `settings.rs`:

```rust
pub(crate) fn update_hotkey(&self, hotkey: String) -> Result<(), SettingsError> {
    let mut state = self.state.lock().expect("settings lock poisoned");
    let mut candidate = state.value.clone();
    candidate.hotkey = hotkey;
    self.persist(&mut state, candidate)
}
```

In `commands.rs`, add:

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HotkeySettingsUpdate {
    hotkey: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HotkeySettingsView {
    hotkey: String,
}

fn prepare_hotkey_save(update: HotkeySettingsUpdate) -> Result<(HotkeyKind, HotkeySettingsView), CommandError> {
    let kind = HotkeyKind::parse(&update.hotkey).map_err(|_| CommandError::settings_failed())?;
    let hotkey = kind.canonical();
    Ok((kind, HotkeySettingsView { hotkey }))
}
```

Add `save_hotkey_with` mirroring `save_settings_with`, but taking only `HotkeySettingsUpdate` and worker args `(reservation, kind, hotkey)`.

In `lifecycle.rs`, implement `save_hotkey_transaction` by using current snapshot for old durable hotkey and autostart:

```rust
let persisted = settings.snapshot();
let persisted_kind = HotkeyKind::parse(&persisted.hotkey).map_err(|_| ())?;
let autostart_enabled = persisted.autostart;
...
self.apply_hotkey_settings_transaction(
    HotkeyBindingChange {
        persisted: persisted_kind,
        requested: kind,
        autostart: autostart_enabled,
    },
    shortcut_ops,
    hook_ops,
    autostart_ops,
    || settings.update_hotkey(hotkey).map_err(|_| ()),
)
```

Reuse the exact shortcut/hook/autostart closures from `save_settings_transaction`; if duplication grows, extract a private helper that takes `HotkeyBindingChange` and persist closure. Do not introduce a generic settings patch abstraction.

In `commands.rs`, add command:

```rust
#[tauri::command]
pub(crate) async fn save_hotkey(
    window: tauri::WebviewWindow,
    hotkey: HotkeySettingsUpdate,
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, std::sync::Arc<LifecycleCoordinator>>,
    settings_store: tauri::State<'_, SettingsStore>,
) -> Result<HotkeySettingsView, CommandError> {
    require_main_window(&window)?;
    save_hotkey_with(hotkey, || {
        let reservation = coordinator.reserve_critical()?;
        Ok::<_, ReservationError>(reservation)
    }, {
        let app_for_worker = app.clone();
        let coordinator_for_worker = Arc::clone(coordinator.inner());
        move |reservation, kind, hotkey| {
            let _reservation = reservation;
            let settings = app_for_worker.state::<SettingsStore>();
            coordinator_for_worker.save_hotkey_transaction(&app_for_worker, &settings, kind, hotkey)
        }
    }).await
}
```

Return the canonical `HotkeySettingsView` from `save_hotkey_with` only after worker success.

In `lib.rs`, register `save_hotkey` in `tauri::generate_handler![...]`.

- [ ] **Step 4: Run GREEN**

Run:

```powershell
$env:CARGO_INCREMENTAL='0'; cargo test --manifest-path src-tauri/Cargo.toml save_hotkey update_hotkey hotkey_transaction -- --nocapture
```

Expected: targeted Rust tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/settings.rs src-tauri/src/lifecycle.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "功能：后端支持快捷键专用持久化" -m "新增 save_hotkey 命令和 hotkey-only SettingsStore 更新，只接受 hotkey 字段并返回 canonical hotkey。后端复用 CriticalReservation、LifecycleCoordinator 热键事务、主线程 hook 安装卸载、global shortcut 注册和原子持久化，保持持久化最后执行与失败补偿不变量。"
```

---

### Task 3: 集成回归与人工测试说明

**Files:**
- Modify only if needed: `src/launcher.test.tsx`, `src-tauri/src/commands.rs`, `src-tauri/src/lifecycle.rs`

**Interfaces:**
- Consumes: Task 1 and Task 2.
- Produces: 用户可在 worktree 执行的测试命令与人工验收步骤。

- [ ] **Step 1: Run full focused verification**

Run:

```powershell
npm test -- src/launcher.test.tsx src/hotkey-recorder.test.ts
$env:CARGO_INCREMENTAL='0'; cargo test --manifest-path src-tauri/Cargo.toml settings hotkey lifecycle double_tap hook commands -- --nocapture
npm run build
$env:CARGO_INCREMENTAL='0'; cargo test --manifest-path src-tauri/Cargo.toml -- --nocapture
```

Expected: exit code 0 for each command. Existing linker/stdout warnings are acceptable only if they already existed and no new warning is introduced by this branch.

- [ ] **Step 2: Prepare manual command for user**

Give the user this command from the worktree:

```powershell
cd D:\code\UiPilot_tools\.worktrees\settings-hotkey-persistence-activation
npm run tauri dev
```

Manual acceptance:

1. 打开设置，录制 `DoubleCtrl`，不要点击全量“保存”。
2. 隐藏主界面后双击 Ctrl，Launcher 应立即出现。
3. 停止 dev，再运行 `npm run tauri dev`，设置页仍显示 `DoubleCtrl`，双击 Ctrl 仍打开 Launcher。
4. 录制回 `Alt+Space`，不要点击全量“保存”；`Alt+Space` 应立即打开 Launcher，DoubleCtrl 不再打开。
5. 若同时改了 alias/Research ID/autostart 但没点全量“保存”，确认即时 hotkey 保存不会把这些草稿写入磁盘。

- [ ] **Step 3: Final branch status**

Run:

```powershell
git status --short --branch
git log --oneline -5
```

Expected: worktree clean, branch contains spec commits, plan commit, and implementation commits.

- [ ] **Step 4: Commit any final test-only adjustments**

Only if Step 1 required small test expectation updates:

```powershell
git add <changed-test-files>
git commit -m "测试：覆盖快捷键即时持久化回归" -m "补充最终回归覆盖，确保录制后专用保存不触发全量设置保存，失败保留草稿并恢复旧 hotkey，Rust 事务保持运行时与持久化一致。"
```

---

## Self-Review

- Spec coverage: plan covers immediate hotkey persistence, no other-draft persistence, current runtime activation, restart durability, failure rollback, stale response, DoubleCtrl/DoubleAlt/chord.
- Placeholder scan: no TBD/TODO placeholders; code names and commands are concrete.
- Type consistency: frontend uses `HotkeySettingsUpdate` / `HotkeySettingsView`; Rust uses same camelCase payload and response.
- Ponytail check: no generic settings patch framework; no new dependency; helper extraction only if it deletes duplication around the real second caller.
