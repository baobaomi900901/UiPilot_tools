# 设置主题选择设计

## 目标

在设置页增加“风格”下拉选项，提供以下三个值：

- `system`：界面显示“跟随系统”；
- `dark`：界面显示“Dark”；
- `light`：界面显示“Light”。

选择后立即作用于整个 UiPilot 主窗体并立即持久化。下次启动恢复上次选择；选择“跟随系统”时，Windows 在程序运行期间切换深浅色，UiPilot 立即同步变化。“恢复初始化”同时把主题恢复为“跟随系统”。

## 范围

本需求包含：

- Rust 设置模型、兼容加载和原子持久化；
- 新的窄范围主题偏好命令；
- 现有全量用户设置事务携带主题，以便恢复初始化原子重置；
- 前端全局主题状态、即时切换、失败回滚和旧响应防护；
- 设置页下拉控件；
- Ant Design 与自定义 CSS 的统一主题投影；
- 自动测试与 worktree 人工验收。

本需求不包含：

- 自定义颜色或更多主题；
- 插件自行覆盖宿主主题；
- 修改插件 runtime 内容；
- 新的主题框架、动画或跨设备同步。

## 选型

采用“专用主题命令 + 现有全量设置事务携带主题”的组合方案。

单独选择主题时调用新的 `set_theme_preference`。该命令只更新主题，不重新注册快捷键，不变更开机启动，也不进入这些系统设置的事务。相比直接复用 `save_settings`，这能缩小副作用和失败范围。

现有 `save_settings` 的用户设置 payload 增加主题字段。开机启动保存携带当前主题；“恢复初始化”通过一次现有事务写入 `Shift+Space`、`false` 和 `system`，保证三个字段在同一个宿主设置事务内提交，不出现只恢复一部分的状态。

不使用 `localStorage`。主题属于宿主管理设置，应继续使用 `settings.json`、原子替换和备份恢复能力。

## 持久化模型

### 类型

Rust 增加 `ThemePreference` 枚举：

```rust
pub(crate) enum ThemePreference {
    System,
    Dark,
    Light,
}
```

序列化值固定为 `system`、`dark`、`light`。`ThemePreference::default()` 返回 `System`。

`Settings` 增加：

```rust
#[serde(default)]
pub(crate) theme: ThemePreference,
```

旧版 `settings.json` 缺少该字段时按 `system` 加载。字段存在但值不属于三个固定值时，继续使用现有“整个候选设置无效”的加载策略，不做字符串宽松兼容。

### 写入边界

`SettingsStore` 增加 `set_theme_preference(theme)`。它克隆当前设置，只替换 `theme`，再复用现有 `persist`；文件预览、使用次数、窗口位置、快捷键和开机启动均保持原值。`persist` 只有在 `commit_with_backup` 成功后才替换内存 snapshot，因此命令失败不会发布候选主题。

`SettingsUpdate` 增加 `theme`，`update_user_settings` 同时更新快捷键、开机启动和主题。`update_hotkey`、`set_file_preview_enabled`、`set_window_position` 和使用次数更新继续保留主题。

## 命令契约

`load_settings` 返回：

```text
hotkey
autostart
filePreviewEnabled
theme
```

`save_settings` 输入增加 `theme`。前端进行开机启动即时保存时发送当前主题；恢复初始化固定发送：

```json
{
  "hotkey": "Shift+Space",
  "autostart": false,
  "theme": "system"
}
```

新增命令：

```text
set_theme_preference({ preference: { theme } }) -> void
```

命令必须：

1. 首先执行现有主窗口 caller guard；
2. 只接受 `system | dark | light` 的严格反序列化输入；
3. 通过现有 critical reservation 和阻塞 worker 执行持久化；
4. 在 worker 内重新取得受管 `SettingsStore`，只调用 `set_theme_preference`；
5. 将 reservation、worker join 和存储失败统一映射为固定 `settingsFailed`；
6. 不调用快捷键或开机启动运行时事务。

命令加入 Tauri handler、ACL 生成输入、主窗口权限和前端 client 映射。安全探针权限集合不扩张。

## 前端状态所有权

### 全局偏好

主题影响 launcher、文件模式和设置页，不能只存在于设置页局部状态。`LauncherSnapshot` 暴露全局 `theme`；初始值为 `system`。设置页 `SettingsSnapshot` 也投影同一值供下拉框显示，不维护第二份可分叉状态。

core 维护：

- 当前可见主题偏好；
- 最后一次确认持久化的主题偏好；
- 主题 durable generation；
- 当前主题 mutation owner/token。

主题 mutation 与现有 settings operation 串行，避免与开机启动保存、快捷键保存、恢复初始化或设置加载并发提交。设置投影在 operation 存在时沿用现有短暂只读状态。

### 启动和设置加载

每个 `load_settings` owner 捕获请求开始时的主题 durable generation。

- 响应 generation 仍匹配时，可以更新全局主题偏好和最后确认值；
- generation 已变化时，响应中的旧主题不得覆盖更晚完成的主题保存；
- settings view 字段仍遵守现有 view epoch 和 operation token 所有权；
- startup load 即使不再拥有某个 settings view，也必须能在 generation 匹配时水合全局主题，避免 launcher 永远停留在默认 `system`；
- 重新进入 settings 仍发起当前 epoch 自己的 authoritative load，旧 startup 响应不能作为当前页面 owner。

该规则与现有文件预览偏好的 startup hydration 和 durable generation 规则保持一致。

## 即时切换和失败处理

用户选择不同主题时：

1. core 建立主题 mutation owner；
2. 立即把全局可见主题改为新值并发布 snapshot；
3. 调用 `set_theme_preference`；
4. 成功后将该值记为最后确认值，递增 durable generation，并按当前 settings epoch 规则完成或协调加载；
5. 失败后恢复最后确认主题，显示固定“无法保存风格设置”错误，并在当前为 settings view 时请求 authoritative settings load。

单独主题命令没有快捷键或开机启动运行时副作用。失败不得设置进程级 `settingsUncertain`，也不得把设置页锁成“必须重启”。若失败后的 authoritative load 也失败，则使用现有 settings load error/retry 状态；重新进入设置页仍会强制加载。

过期 mutation 完成时不得直接覆盖当前 settings view。若旧 operation 完成时当前已进入新的 settings epoch，沿用现有 reconciliation 规则，为最新 epoch 发起或合并一次 `load_settings`。全局主题只由匹配的 mutation token 和 durable generation 更新。

`save_settings` 或 `save_hotkey` 的失败语义不变：它们仍可能影响系统运行时设置，因此继续设置 `settingsUncertain`，直到进程重启。

## 恢复初始化

恢复初始化确认后，前端先建立现有 `save` operation，再乐观设置：

- 快捷键：`Shift+Space`；
- 开机启动：`false`；
- 主题：`system`。

随后只调用一次 `save_settings`。成功后通过现有 authoritative load 确认；失败时沿用现有事务不确定锁和加载恢复规则。主题不得通过第二条独立命令补写，以免产生部分成功。

## 设置页 UI

在开机启动控件之后、恢复初始化操作之前增加垂直表单项：

- 标签：`风格`；
- 控件：Ant Design `Select`；
- 选项顺序：`跟随系统`、`Dark`、`Light`；
- 值：`system`、`dark`、`light`；
- 选择后直接调用 core 的主题设置方法，不增加保存按钮；
- settings snapshot 为只读或存在 operation 时禁用；
- 控件具有稳定的可访问名称，键盘可操作。

不创建新的卡片或主题预览区域。

## 主题渲染

React 保留一个 `matchMedia('(prefers-color-scheme: dark)')` 监听器，维护当前系统是否为深色。最终颜色模式按下式计算：

```text
theme == dark  -> dark
theme == light -> light
theme == system && systemDark -> dark
otherwise -> light
```

系统主题变化时始终更新 `systemDark`，但只有偏好为 `system` 时才改变最终颜色模式。强制 `dark` 或 `light` 不响应系统颜色变化。

最终颜色模式同时驱动：

- Ant Design `theme.darkAlgorithm` 或 `theme.defaultAlgorithm`；
- `document.documentElement` 的 `data-color-scheme`；
- `.launcher-surface` 的 `data-color-scheme`；
- 原生控件使用的 CSS `color-scheme`。

现有 `@media (prefers-color-scheme: dark)` 内的宿主深色颜色规则改为显式 `[data-color-scheme="dark"]` 选择器。这样强制主题时，Ant Design、自定义背景、边框、文字、选中态和虚拟滚动条使用同一个最终模式。`forced-colors` 可访问性规则继续保持最高优先级。

组件卸载时移除 media listener，并清理根元素由组件写入的主题属性，避免测试或未来多窗口生命周期残留。

## 测试

### Rust 设置测试

- `ThemePreference` 三个值精确序列化和反序列化；
- 旧设置文件缺少 `theme` 时加载为 `system`；
- 非法主题值使当前候选无效并走现有备份/默认恢复；
- 三个主题值均可持久化并重载；
- `set_theme_preference` 只修改主题；
- 主题持久化失败不更新受管 snapshot；
- 用户设置更新同时写入主题并保留文件预览、窗口位置和使用次数；
- 快捷键、文件预览、窗口位置和使用次数更新保留主题。

### Rust 命令测试

- `SettingsView` 和 `UserSettingsUpdate` 的 wire shape 精确包含 `theme`；
- `set_theme_preference` caller guard 是命令体第一条语句；
- 输入严格拒绝未知字段和非法主题；
- command reservation、worker、join 和存储失败映射为固定错误；
- 专用命令只调用主题存储路径，不进入运行时快捷键事务；
- handler、ACL 和生产命令清单精确包含新命令。

### 前端 core 测试

- startup load 在 launcher 中水合持久化主题；
- 主题选择先发布乐观值，再发送精确命令；
- 成功后更新最后确认值和 durable generation；
- 失败后恢复最后确认值、显示错误且不设置 `needsReload`；
- 旧 startup/load 响应不能覆盖较新主题保存；
- 离开并重进 settings 时，旧 mutation 完成会协调当前 epoch 的 load，而不会直接覆盖当前视图；
- 开机启动保存携带当前主题；
- 恢复初始化一次发送 `Shift+Space / false / system`；
- 现有 save/hotkey 失败仍保持进程级不确定锁。

### React 和 CSS 测试

- 设置页渲染三个固定顺序选项，选中值来自 snapshot；
- 下拉选择调用 core 且只读时禁用；
- `system` 初始按系统模式选择 Ant Design algorithm；
- `system` 响应运行时 media change；
- 强制 `dark` 和 `light` 忽略 media change；
- 根元素和主 surface 的 `data-color-scheme` 与 Ant Design algorithm 一致；
- 卸载时清理 listener 和根元素属性；
- CSS 深色颜色与滚动条使用显式主题选择器，不再依赖系统 media query 决定宿主主题；
- forced-colors 规则继续存在。

## 人工验收

1. 在设置页依次选择 `Dark`、`Light`、`跟随系统`，每次整个窗口立即同步变化，无保存按钮。
2. 选择 `Dark`，关闭并重启 UiPilot，仍为深色；`Light` 同理。
3. 选择 `跟随系统`，在 UiPilot 运行期间切换 Windows 深浅色，窗口立即跟随。
4. 选择强制 `Dark` 或 `Light` 后切换 Windows 主题，UiPilot 保持用户强制选择。
5. 修改快捷键和开机启动后切换主题，其他设置不变化；切换主题后修改开机启动，主题不变化。
6. 点击“恢复初始化”并确认，快捷键为 `Shift+Space`、开机启动关闭、主题为“跟随系统”。
7. 重启后再次确认上述恢复值仍然生效。

## 不合理点记录

现有主题只由 React 内部的系统 media query 决定，而自定义 CSS 也直接依赖同一 media query。这在只有“跟随系统”时成立，但无法表达用户强制主题；如果只切换 Ant Design algorithm，会形成组件已深色、自定义区域仍浅色的混合界面。本需求必须把“用户偏好”和“最终颜色模式”分开，并让所有渲染层消费同一个最终模式。

现有全量 `save_settings` 会执行快捷键和开机启动运行时事务。用它处理每次纯主题切换会无谓扩大系统副作用和失败面，因此主题单独切换必须使用窄命令；只有恢复初始化和开机启动全量事务携带主题。
