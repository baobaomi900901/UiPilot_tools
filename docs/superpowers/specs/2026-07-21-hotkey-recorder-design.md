# 快捷键录制与双击修饰键设计

## 状态

- 日期：2026-07-21
- 状态：待书面复审
- 影响范围：设置页快捷键输入、`hotkey` 持久化/校验、启动时注册、退出清理；Windows 双击 Ctrl/Alt 低层钩子

## 目标与边界

设置里的快捷键从「手打字符串」改为**点击录制**：用户按下目标键后自动填入。除现有组合键（如 `Ctrl+Space`、`Ctrl+Shift+Space`）外，支持**双击 Ctrl** 与**双击 Alt** 作为全局唤起方式。

保存成功后写入现有 `settings.json` 的 `hotkey` 字段；**下次进程启动**加载同一字段并重新注册，保证持久化。

本变更：

- 保留单一唤起快捷键字段（不拆成两个设置项）
- 组合键继续走 Tauri `global-shortcut`
- 双击修饰键走 Windows 低层键盘钩子（固定 400ms 窗口）
- 不新增前端「任意路径 / 任意动作」能力；钩子只用于匹配已保存的双击配置并调用既有 `request_show(Launcher)`

## 已确认的产品决策

| 决策 | 选择 |
|---|---|
| 录制交互 | 点击输入框进入录制 → 捕获后自动填入并结束；Esc 取消并恢复原值 |
| 支持类型 | 组合键 + 双击 Ctrl / 双击 Alt |
| 双击间隔 | 固定 400ms（不可配置） |
| 双击键范围 | 仅 Ctrl、Alt（不含 Shift / Win / 字母键双击） |
| 持久化 | 同一 `hotkey` 字符串进 `settings.json`；重启后恢复并重新注册 |
| 录制中手打 | 不允许（录制态不接受普通文本编辑） |

## Hotkey 值模型

逻辑上解析为两种形态（持久化仍是一个字符串）：

```text
Hotkey =
  | Chord(normalized Tauri Shortcut string)   // e.g. "Ctrl+Space", "Ctrl+Shift+Space"
  | DoubleTap(Ctrl | Alt)                    // canonical: "DoubleCtrl" | "DoubleAlt"
```

规则：

- **Chord**：必须能被现有 `str::parse::<Shortcut>()` 接受；保存时用 `Shortcut::to_string()`（或等价规范化）写回，避免同义写法漂移。
- **DoubleTap**：仅允许精确规范串 `DoubleCtrl` / `DoubleAlt`（大小写敏感，禁止 `double-ctrl`、`Double Control` 等别名）。
- 空串、未知串、非法 chord → 保存预检失败，沿用既有 `settings_failed` 固定错误面；不写盘、不切换运行时注册。

展示文案（设置框，可与规范串分离）：

| 规范值 | 展示示例 |
|---|---|
| `Ctrl+Space` | `Ctrl + Space`（或规范化后的 Shortcut 显示） |
| `DoubleCtrl` | `双击 Ctrl` |
| `DoubleAlt` | `双击 Alt` |

录制占位：进入录制后显示固定文案（如「按下快捷键…」），不把中间态写入 draft，直到捕获成功或 Esc。

## 录制行为（前端）

1. 用户点击快捷键控件 → 进入 `recording`；draft 暂不改，UI 显示占位。
2. **组合键**：在至少一个修饰键按下的前提下，按下非修饰主键（Space、字母、功能键等）时提交 chord；修饰键集合取当前按下的 Ctrl/Shift/Alt/Meta（Win）。单独修饰键按下不提交。
3. **双击**：在 400ms 内连续两次**同一**修饰键（仅 Left/Right Ctrl 视为 Ctrl；Left/Right Alt 视为 Alt），且两次之间无其它键 → 提交 `DoubleCtrl` 或 `DoubleAlt`。
4. **Esc**：退出录制，控件恢复进入录制前的已提交值（含未保存 draft 策略与现有 TextControl 一致：取消录制不丢其它字段；快捷键恢复录制前该控件的值）。
5. 失焦：退出录制且不提交中间态（等同取消录制），避免焦点跑到别处后误捕获。
6. 录制仅在设置页可见且该控件处于录制态时启用；不在后台全局偷听按键来「录制」。

捕获结果写入现有 settings hotkey `TextControl` draft，随后仍走现有「保存设置」提交路径（不因录制自动 `save_settings`）。

## 保存与启动注册（Rust）

### 预检

扩展 `prepare_settings_save`（或等价预检）：

1. 若 `hotkey` 为 `DoubleCtrl` / `DoubleAlt` → 接受，持久化规范串，**不**要求 `parse::<Shortcut>()`。
2. 否则按现有逻辑 `parse::<Shortcut>()`；失败 → `settings_failed`。
3. 其余 `SettingsUpdate` 校验（aliases 等）不变。

### 运行时协调

`LifecycleCoordinator` 的 runtime settings 协调扩展为：

| 目标类型 | 动作 |
|---|---|
| Chord | 确保双击钩子已卸装；注销旧 shortcut；注册新 shortcut |
| DoubleTap | 确保旧 shortcut 已注销；安装/更新双击钩子为目标修饰键 |

启动路径 `reconcile_runtime_settings` / 首次 setup 与保存事务使用同一套协调逻辑，从而**重启后**从磁盘读出的 `DoubleCtrl` / `DoubleAlt` / chord 都能生效。

注册或钩子安装失败：不持久化（若发生在保存事务中按现有「先副作用成功再写盘」规则）；若启动加载时失败，保留磁盘值，走既有 settings/lifecycle 失败提示，不静默清空用户配置。

### 双击钩子约束

- 仅 Windows；仅在当前生效 hotkey 为 DoubleTap 时安装。
- 窗口隐藏/托盘常驻时钩子保持，以便全局唤起。
- 匹配成功 → 与全局快捷键相同：`request_show(app, ShowTarget::Launcher)`。
- 退出清理、`request_tray_quit`、会话结束钩子路径必须卸装 LL hook，避免进程退出后钩子残留。
- 钩子回调内不做按键内容日志；错误只报固定类别。

400ms 计时以钩子见到的两次 **keydown**（或等价 press）时间戳差为准；第二次超出窗口则把该次当作新的「第一次」。

## 前端与协议

- `protocol` / DTO 的 `hotkey: string` 形状不变。
- 设置页将 hotkey 的 `BoundInput` 换为（或包裹为）录制控件：点击进入录制、展示规范/本地化文案、禁用手打。
- 现有 `TextControl` 生命周期（retire、composition 边界等）若与「禁止手打」冲突，录制控件应绕过 insertText 路径，仅通过显式 `setHotkeyDraft(canonical)`（或等价 core API）写入。

## 测试要点

- 录制：chord 提交、DoubleCtrl/DoubleAlt 在 400ms 内/外、Esc/失焦取消、不自动 save。
- 预检：非法串拒绝；`DoubleCtrl`/`DoubleAlt` 接受；合法 chord 规范化写回。
- 协调：Chord↔DoubleTap 切换时 shortcut 与钩子互斥装卸顺序。
- 启动：磁盘为 `DoubleAlt` 时 setup 安装钩子而非 `global-shortcut`；为 chord 时相反。
- 退出：钩子卸装被源码或单元探针覆盖（沿用项目既有 wiring/生命周期测试风格）。

## 非目标

- 双击 Shift / Win / 任意键
- 可配置双击间隔
- 多个全局快捷键
- 录制态全局钩子偷听（仅设置页焦点录制）
- 改变失焦隐藏、invocation、result registry 语义
- 非 Windows 平台的双击实现（本阶段仅 Windows）

## 实现落点（预期）

```text
src/launcher-view.tsx / 新录制控件模块   # 录制 UX
src/launcher-core.ts                     # draft 写入 API（若需）
src-tauri/src/commands.rs                # prepare_settings_save 扩展
src-tauri/src/settings.rs                # 若需规范化辅助（可选）
src-tauri/src/lifecycle.rs               # 注册协调、启动 reconcile、退出卸钩
src-tauri/src/hotkey_hook.rs（新）        # DoubleTap LL hook（crate-private）
```

具体文件拆分以实现计划为准；本设计锁定行为与边界。
