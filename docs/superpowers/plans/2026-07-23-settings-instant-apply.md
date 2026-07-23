# 设置即时生效与恢复初始化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除设置页显式保存流程，让开机启动即时持久化，并提供带确认的恢复初始化，同时保证 settings load、启动水合、生命周期失败和跨 view 响应具有正确所有权。

**Architecture:** 只修改现有前端状态机。`launcher-core` 用一个 settings operation、一个最新 settings epoch 请求和一个进程级 uncertainty latch 串行所有加载与 mutation；startup 的 settings 页面字段按 view owner，文件预览后台水合继续按 durable generation。视图复用 Ant Design `Popconfirm`，不新增 IPC、Rust 代码或依赖。

**Tech Stack:** TypeScript、React 19、Ant Design、Vitest；现有 Tauri `load_settings`、`save_settings`、`save_hotkey` IPC。

## Global Constraints

- 基线：`main@21cc09b`，分支 `codex/settings-instant-apply`。
- 不修改 Rust 生产代码、Tauri IPC、权限、capability 或插件模块。
- 默认设置仅为快捷键 `Alt+Space` 与开机启动 `false`。
- 插件、文件预览、窗口位置和使用次数不得被恢复初始化重置。
- 任意有效 `LauncherShown.notice = settingsFailed` 都设置进程级 uncertainty，直到新 core/进程重启。
- 所有生产代码必须先有对应失败测试；每个 GREEN 后提交。
- 已知 Windows 插件目录移动基线失败只记录，不在本分支修复。

---

### Task 1: 统一 settings load owner、startup 水合与 uncertainty 投影

**Files:**
- Modify: `src/protocol.ts`
- Modify: `src/launcher-core.ts`
- Test: `src/launcher.test.tsx`

**Interfaces:**
- Produces: `SettingsSnapshot.loadStatus: 'loading' | 'ready' | 'error'`。
- Produces: 进程级 `settingsUncertain`、最新 `pendingSettingsLoadEpoch`、统一 `SettingsOperation`。
- Consumes: 现有 `LauncherClient.loadSettings()` 与 `previewPreferenceDurableGeneration`。

- [ ] **Step 1: 写 lifecycle notice 与 load 状态 RED 测试**

在 `settings ownership` 中增加：

```ts
it('keeps lifecycle settings failure latched across launcher and settings loads', async () => {
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings)
    .mockResolvedValueOnce(settingsFixture)
    .mockResolvedValueOnce(settingsFixture)
  const core = createLauncherCore(fake.client)
  await core.start()
  fake.emit(shown('notice-launcher', 'launcher', 'settingsFailed'))
  fake.emit(shown('notice-settings', 'settings'))
  await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
  expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
})

it('latches a settings-target lifecycle failure before applying its owner load', async () => {
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings)
    .mockResolvedValueOnce(settingsFixture)
    .mockResolvedValueOnce(settingsFixture)
  const core = createLauncherCore(fake.client)
  await core.start()
  fake.emit(shown('notice-settings', 'settings', 'settingsFailed'))
  await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
  expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
})
```

- [ ] **Step 2: 写 startup owner/queue 与 preview generation RED 测试**

增加四个测试：startup pending -> settings B；startup pending -> B -> launcher -> C（启动成功与失败）；B -> launcher 后后台预览水合；较新 preview write 防旧 startup 覆盖。核心断言：

```ts
const startup = deferred<SettingsView>()
const current = deferred<SettingsView>()
vi.mocked(fake.client.loadSettings)
  .mockReturnValueOnce(startup.promise)
  .mockReturnValueOnce(current.promise)
const starting = core.start()
await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
fake.emit(shown('settings-b', 'settings'))
expect(core.getSnapshot().settings).toBeUndefined()
startup.resolve({ ...settingsFixture, hotkey: 'DoubleCtrl', filePreviewEnabled: false })
await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
expect(core.getSnapshot().settings).toBeUndefined()
current.resolve({ ...settingsFixture, hotkey: 'DoubleAlt' })
await starting
await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleAlt'))
```

在 B -> launcher 路径进入文件模式，断言 startup `filePreviewEnabled: false` 被后台水合；先成功执行 `setFilePreviewPreference(true)` 时断言旧 startup `false` 不覆盖；随后进入 C 必须产生新 `loadSettings` 调用并只应用 C 返回值。

- [ ] **Step 3: 运行 RED**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings ownership|older settings load"`

Expected: FAIL，原因包括缺少 `loadStatus`、lifecycle notice 未设置 latch、startup 响应覆盖错误 owner 或 settings epoch load 被吞掉。

- [ ] **Step 4: 实现最小状态与投影**

在 `src/protocol.ts` 增加：

```ts
export type SettingsLoadStatus = 'loading' | 'ready' | 'error'

export interface SettingsSnapshot {
  hotkey: TextControlView
  autostart: boolean
  loadStatus: SettingsLoadStatus
  readOnly: boolean
  operation?: 'load' | 'save' | 'hotkey'
  needsReload: boolean
}
```

在 `launcher-core.ts` 的 `Model` 增加 `settingsUncertain` 与 `settingsLoadStatus`；将 `SettingsOperation` 的 `viewEpoch/view` 改成可表达 startup owner 的联合 owner，并增加 `pendingSettingsLoadEpoch`。投影固定为：

```ts
const settings = model.settings
  ? Object.freeze({
      hotkey: Object.freeze({ key: model.settings.hotkey.key, value: model.settings.hotkey.draft }),
      autostart: model.settings.autostart,
      loadStatus: model.settingsLoadStatus,
      readOnly:
        model.settingsUncertain ||
        model.settingsLoadStatus !== 'ready' ||
        model.settingsOperation !== undefined,
      ...(model.settingsOperation === undefined ? {} : { operation: model.settingsOperation }),
      needsReload: model.settingsUncertain,
    })
  : undefined
```

`shown()` 在处理 target 前执行：

```ts
if (event.notice === 'settingsFailed') model.settingsUncertain = true
```

settings 目标先设置 `settingsLoadStatus = 'loading'` 和最新 epoch，再调用统一 drain；launcher 目标清除已离开的待加载 epoch。settings 状态文本从 latch 投影固定重启提示，不能依赖 notice 仍可见。

- [ ] **Step 5: 统一 startup 与 view load owner**

删除 `startupSettingsPending` 短路。startup 创建唯一 `load` operation 并捕获 `previewPreferenceDurableGeneration`。完成规则：

```ts
if (model.view !== 'settings') {
  replaceSettings(view, capturedPreviewGeneration)
}
releaseSettingsOperation(operation)
void drainSettingsLoad()
```

当前是 settings 时不应用 startup settings 页面字段；只释放 owner 并启动最新 epoch load。startup 失败不得写入不匹配 settings epoch 的 error。`replaceSettings` 不再清除 uncertainty/load state；preview 更新继续由现有 generation 比较保护。

- [ ] **Step 6: 运行 GREEN 并提交**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings ownership|older settings load"`

Expected: 新增与既有相关测试全部 PASS。

```powershell
git add src/protocol.ts src/launcher-core.ts src/launcher.test.tsx
git commit -m "refactor: own settings loads by view epoch"
```

---

### Task 2: 即时 mutation、失败恢复与重试

**Files:**
- Modify: `src/launcher-core.ts`
- Test: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: Task 1 的 `settingsUncertain`、`settingsLoadStatus`、load queue/drain。
- Produces: `setAutostart(checked)` 即时保存、`resetSettings()`、错误态 `reloadSettings()`。

- [ ] **Step 1: 写即时 autostart 与 reset RED 测试**

```ts
it('persists autostart immediately and confirms with the authoritative snapshot', async () => {
  const { core, client } = await settingsCore()
  vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: true })
  core.setAutostart(true)
  expect(client.saveSettings).toHaveBeenCalledWith({
    settings: { hotkey: 'Alt+Space', autostart: true },
  })
  await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(true))
})

it('resets only visible settings through one existing save command', async () => {
  const { core, client } = await settingsCore()
  vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
  await core.resetSettings()
  expect(client.saveSettings).toHaveBeenCalledWith({
    settings: { hotkey: 'Alt+Space', autostart: false },
  })
})
```

- [ ] **Step 2: 写跨 epoch 成功/失败和重试 RED 测试**

分别对 autostart 与 hotkey 覆盖：epoch A mutation pending -> 离开 -> settings B；成功后 B 新 token load 且可编辑，失败后 B 恢复 load 应用 authoritative snapshot 但 `needsReload/readOnly` 保持。另覆盖普通 load 失败：`loadStatus = error`、`needsReload = false`，调用 `reloadSettings()` 成功后恢复 `ready`；uncertainty 下重试成功仍只读。

- [ ] **Step 3: 运行 RED**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings ownership"`

Expected: FAIL，因为 autostart 仍只更新草稿、reset API 不存在、旧 mutation 成功被错误标记为 reload-required。

- [ ] **Step 4: 实现共享 save mutation 完成路径**

保留 IPC client 的 `saveSettings`，删除 core 对外手动 `saveSettings()`。新增私有：

```ts
async function persistSettings(update: UserSettingsUpdate): Promise<void>
```

mutation 成功：释放旧 operation；当前为 settings 时为当前 epoch 排队新 authoritative load，否则等待下次进入。mutation 失败：设置 `settingsUncertain = true`，释放 operation，并仅为当前 settings epoch排队恢复 load。旧 epoch 的返回值/错误不直接落 UI。

`setAutostart` 改为乐观更新后 `void persistSettings(settingsUpdate())`。新增：

```ts
async function resetSettings(): Promise<void> {
  if (!settingsEditable() || !model.settings) return
  setControlDraft(model.settings.hotkey.key, 'Alt+Space')
  model.settings.hotkey.value = 'Alt+Space'
  model.settings.autostart = false
  publish(true)
  await persistSettings({ hotkey: 'Alt+Space', autostart: false })
}
```

`saveHotkeyCanonical` 使用相同成功/失败 reconciliation；成功响应不得覆盖新 epoch。`reloadSettings()` 只为当前 error state 或 settings 入页调度 load，不再受 startup 布尔值阻断。

- [ ] **Step 5: 运行 GREEN 并提交**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings ownership"`

Expected: 全部 PASS。

```powershell
git add src/launcher-core.ts src/launcher.test.tsx
git commit -m "feat: apply settings changes immediately"
```

---

### Task 3: 设置页移除保存并增加恢复初始化

**Files:**
- Modify: `src/launcher-view.tsx`
- Test: `src/launcher.test.tsx`

**Interfaces:**
- Consumes: `LauncherCore.resetSettings()`、`SettingsSnapshot.loadStatus`。
- Produces: 无常驻保存/重载按钮、错误态重试、带确认恢复初始化。

- [ ] **Step 1: 写视图 RED 测试**

挂载 settings view，断言：

```ts
expect(mounted.host.textContent).not.toContain('保存')
expect(mounted.host.textContent).not.toContain('重新加载设置')
expect(mounted.host.textContent).toContain('恢复初始化')
```

点击恢复初始化，先取消并断言 `saveSettings` 零调用；再次确认后断言只调用一次默认 payload。将 load state 置为 error 时断言只显示“重试”，点击后调用当前 epoch load。

- [ ] **Step 2: 运行 RED**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings view"`

Expected: FAIL，旧保存/重新加载按钮仍存在，恢复初始化不存在。

- [ ] **Step 3: 复用 Popconfirm 实现最小 UI**

删除保存按钮；将常驻重新加载按钮替换为：

```tsx
<Popconfirm
  title="恢复初始化设置？"
  description="快捷键将恢复为 Alt+Space，并关闭开机启动。"
  okText="恢复"
  cancelText="取消"
  onConfirm={() => void core.resetSettings()}
  disabled={locked}
>
  <Button danger disabled={locked}>恢复初始化</Button>
</Popconfirm>
```

当 `settings.loadStatus === 'error'` 时额外显示“重试”，调用 `core.reloadSettings()`；无 settings 且加载失败时也只显示“重试”。不新增 CSS。

- [ ] **Step 4: 运行 GREEN 并提交**

Run: `npm.cmd test -- --run src/launcher.test.tsx -t "settings view"`

Expected: 全部 PASS。

```powershell
git add src/launcher-view.tsx src/launcher.test.tsx
git commit -m "feat: replace settings save with reset"
```

---

### Task 4: 全量验证与人工测试交付

**Files:**
- Verify only; no production changes expected.

- [ ] **Step 1: 格式与前端回归**

Run: `npm.cmd test -- --run`

Expected: 3 test files、全部测试 PASS。

Run: `npm.cmd run build`

Expected: TypeScript 与 Vite build exit 0；允许既有 chunk-size warning。

- [ ] **Step 2: Rust 回归**

Run: `cargo test --manifest-path .\src-tauri\Cargo.toml --quiet`

Expected: 除已记录的 Windows 插件目录移动基线波动外无新增失败；若该同一用例失败，单独记录，不修改插件实现。

Run: `cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check`

Expected: exit 0。

- [ ] **Step 3: diff 与工作树检查**

Run: `git diff --check`

Expected: exit 0。

Run: `git status --short`

Expected: 仅有本计划明确修改的前端文件；测试生成的权限文件若内容无差异，只刷新 index stat，不提交。

- [ ] **Step 4: 提供人工验收步骤**

交付 worktree 路径和以下用例：快捷键即时生效；开机启动勾选/取消即时生效；恢复初始化取消/确认；重启持久化；插件清单与文件预览不变。
