# 快捷键录制后即时持久化与激活设计

状态：设计已批准，等待规格审核

基线：

- main：`4d63bfc98f130ef2dc9c35dfc5d85ffb7040b52a`
- worktree：`D:\code\UiPilot_tools\.worktrees\settings-hotkey-persistence-activation`
- branch：`codex/settings-hotkey-persistence-activation`

## 背景与根因

设置页当前可以录制快捷键，但录制结果只停留在前端设置草稿里：

- `src/launcher-view.tsx` 的录制完成回调只调用 `core.setHotkeyCanonical(result.commit)`。
- `src/launcher-core.ts` 的 `setHotkeyCanonical` 只更新 `model.settings.hotkey` 的 draft/value 并 publish。
- 后端持久化与运行时切换只在 `saveSettings()` 里发生，且它会提交整份 `settingsUpdate()`。
- Rust 侧 `save_settings` 已经通过 `LifecycleCoordinator::save_settings_transaction` 处理 `HotkeyKind`、全局快捷键、double tap hook、autostart 与 `SettingsStore` 原子持久化。

所以两个用户现象来自同一根因：录制完成没有触发后端保存/激活路径。下次启动仍加载旧 `settings.json`，当前运行时也不会注册新快捷键。

## 目标

- 录制成功后立即且仅保存/激活快捷键。
- DoubleCtrl、DoubleAlt、普通 chord 都走同一规范化和事务逻辑。
- 成功后本次运行立即生效，重启后从 durable settings 加载一致。
- 失败时前端不得显示假成功：恢复旧 durable 快捷键显示，保留其他设置草稿，显示固定错误。
- 后续点击设置页“保存”必须使用最新 durable hotkey，不得把旧 hotkey 回写。

## 非目标

- 不迁移旧 `%LOCALAPPDATA%\UiPilot` 数据；现用 app id 路径已排除为根因。
- 不新增日志、依赖、通用 settings patch 框架或多字段局部保存系统。
- 不改变 aliases、Research ID、autostart 的保存语义。
- 不改快捷键录制组件的按键识别规则，除非测试证明它已经输出错误 canonical。

## UX 与状态机

录制完成后进入同一个 settings operation 所有权模型，新增操作名可为 `hotkey`：

1. idle：用户录制得到 canonical hotkey。
2. pending：前端调用专用 `saveHotkey({ hotkey })`；禁用重复录制提交和全量保存。
3. success：后端返回最新 `SettingsView` 或至少返回 canonical hotkey；前端把 hotkey 的 durable value/draft 同步为最新值，清除状态。
4. failure：如果 operation 仍拥有当前 settings view，则 hotkey 控件恢复到提交前的 durable 值，其他草稿不回滚，状态显示固定错误文案；如果 view 已过期，只标记需要 reload，不能覆盖新 view。

错误文案使用固定用户可见文本，例如“快捷键保存失败，请重新设置或重启后再试”。后端内部错误不透出。

## 前端、IPC 与 Rust 数据流

前端最小改动：

- `protocol.ts` 增加 `saveHotkey(input: { hotkey: string }): Promise<SettingsView>`，复用现有 `SettingsView` 类型。
- `main.ts` 映射到 Tauri command `save_hotkey`。
- `launcher-core.ts` 保留 `setHotkeyCanonical` 作为草稿更新函数，新增 `saveHotkeyCanonical(value)` 或让录制回调调用一个专用 core 方法。
- 专用方法只构造 `{ hotkey }`，绝不调用 `settingsUpdate()`，因此不会带出 aliases、Research ID、autostart 草稿。
- 成功返回后用 `replaceSettings(view, previewGeneration)` 或等价的局部同步，确保后续全量保存读取的是最新 durable hotkey。

Rust 最小改动：

- `commands.rs` 新增 `HotkeyUpdate { hotkey: String }` 和 `save_hotkey` command。
- `save_hotkey` 只解析 `HotkeyKind::parse(&hotkey)` 并 canonicalize。
- 读取 `SettingsStore::snapshot()`，构造只改 hotkey 的 `SettingsUpdate`，其他字段来自 snapshot，而不是来自前端草稿。
- 调用一个从现有 `save_settings_transaction` 提取出来的最小共享函数，避免复制热键事务。
- 返回 `load_settings_core(&settings_store, &cache)` 的最新 view，让前端更新 durable hotkey。

拟复用函数：

- `HotkeyKind::parse` / `HotkeyKind::canonical`
- `LifecycleCoordinator::save_settings_transaction` 内的热键切换、补偿、持久化顺序
- `SettingsStore::snapshot`
- `SettingsStore::update_user_settings`
- `save_settings_worker_with` / `map_save_worker_result`

## 事务顺序与补偿不变量

后端必须保持现有热键事务不变量：

- 先读取旧 durable snapshot，解析旧 hotkey。
- 运行时切换覆盖 chord 到 double tap、double tap 到 chord、DoubleCtrl 到 DoubleAlt 等互转。
- 持久化必须仍是最后一步；持久化后不再执行可能失败的旧热键清理。
- 任一运行时步骤失败时，尽力恢复提交前的实际运行时状态。
- 如果恢复也失败，保留 coordinator 观察到的真实运行时状态，返回失败，前端要求用户重试/重载。
- `save_hotkey` 不碰 autostart 的目标值：专用更新里的 autostart 来自 snapshot，不能来自前端草稿。

提取边界保持最小：可以把 `save_settings_transaction` 的“给定 HotkeyKind + SettingsUpdate 执行事务”抽成私有 helper；不要新增通用 settings patch 抽象。

## 并发与陈旧响应

- 专用 hotkey 保存使用现有 `settingsOperation` token、viewEpoch、view 检查。
- pending 时拒绝第二次 hotkey 保存、全量保存、rescan 等会改 settings 的动作。
- 成功或失败返回时必须检查 operation 是否仍拥有当前 settings view。
- 过期响应只能释放 operation 或标记 reload，不能覆盖新 settings view。
- 如果用户录制 A 后又很快录制 B，A 的结果不得覆盖 B；pending 期间默认禁用第二次提交。

## 错误处理

- 解析失败、事务失败、worker panic/join 失败都映射成固定 settings failed 错误。
- 前端失败路径恢复 hotkey 控件到 operation 开始前的 durable hotkey，保留 aliases、Research ID、autostart 等草稿。
- 若失败后状态不确定，显示 reload/restart 指引；不要把录制值留在 UI 里假装生效。
- 原全量保存失败行为保持不变。

## 自动化测试矩阵

前端：

- 录制 DoubleCtrl 后调用 `saveHotkey`，不调用 `saveSettings`。
- `saveHotkey` payload 只有 hotkey，不包含 aliases、researchId、autostart。
- success 后 settings hotkey value/draft 都更新为返回 view，随后 `saveSettings()` 使用最新 hotkey。
- failure 后 hotkey 恢复旧 durable，其他草稿保留，显示固定错误。
- pending 时全量保存和重复 hotkey 提交被拒绝。
- 陈旧 response 不覆盖新 view。

Rust commands：

- `save_hotkey` canonicalize 普通 chord。
- `save_hotkey` 支持 DoubleCtrl/DoubleAlt。
- `save_hotkey` 从 snapshot 复制 aliases、research_id、autostart，只改 hotkey。
- 无效 hotkey 返回 settings failed。
- worker 失败不写入错误成功状态。

Lifecycle：

- 复用或扩展现有 transaction 测试，覆盖 chord/double tap 互转、DoubleCtrl/DoubleAlt 互转。
- 验证持久化仍是最后一步。
- 验证 rollback failure 保留真实运行时状态。

人工验收：

- 设置为 DoubleCtrl 后，不点“保存”，直接双击 Ctrl 打开 Launcher。
- 重启 `npm run tauri dev` 后设置页仍显示 DoubleCtrl，双击 Ctrl 仍打开 Launcher。
- 切回 Alt+Space 后，不点“保存”，Alt+Space 立即打开 Launcher，DoubleCtrl 不再打开。
- 模拟/触发失败时，hotkey UI 回到旧值，其他草稿不被保存。

## 风险与拒绝方案

- 拒绝在录制回调里直接调用 `saveSettings()`：会误保存 aliases、Research ID、autostart 草稿。
- 拒绝新增通用 settings patch 框架：当前只有 hotkey 一个真实局部保存需求。
- 拒绝复制完整 Rust 热键事务：复制会让 double tap hook 补偿逻辑分叉。
- 主要风险是前端 success 后仍保留旧 hotkey 草稿；通过返回最新 `SettingsView` 并刷新 settings view 规避。
- 主要风险是后端局部更新误用前端全量 update；通过 `SettingsStore::snapshot()` 构造 update 规避。

## 自审清单

- 数据丢失：专用 payload 不包含其他字段，后端从 snapshot 填充其他字段。
- 误保存草稿：录制路径不调用 `settingsUpdate()` 或 `saveSettings()`。
- 运行时/磁盘分叉：复用现有事务，失败返回失败并保留真实状态。
- 陈旧异步覆盖：复用 operation token/viewEpoch/view 所有权。
- DoubleCtrl/Chord 漏测：自动化和人工验收都覆盖 double tap 与 chord。
