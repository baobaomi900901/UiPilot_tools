# 托盘「打开主界面」设计

## 状态

- 日期：2026-07-21
- 状态：待书面复审
- 影响范围：托盘菜单组装、`tray_action` 映射、既有 show 接线测试

## 目标与边界

在系统托盘菜单中增加「打开主界面」，与全局快捷键 / 单实例唤起使用同一条 `ShowTarget::Launcher` 路径。

本变更：

- 只增加托盘菜单项与 ID → `TrayAction` 映射
- 不改窗口尺寸、焦点、失焦隐藏、invocation / result registry 语义
- 不增加托盘图标左键单击打开主界面（图标点击行为保持现状，仍打开菜单）
- 不新增前端 command、不新增 UI 页面

## 已确认的产品决策

| 决策 | 选择 |
|---|---|
| 菜单顺序 | `打开主界面` → `打开设置` → `退出` |
| 文案 | 「打开主界面」 |
| 图标左键 | 不变（不额外绑定 show） |
| 实现路径 | 复用 `LifecycleCoordinator::request_show(..., ShowTarget::Launcher)` |

## 行为

点击「打开主界面」后：

1. `tray_action` 将 ID `uipilot.tray.open-launcher` 解析为 `TrayAction::Show(ShowTarget::Launcher)`
2. 托盘 `on_menu_event` 调用 `request_show(app, ShowTarget::Launcher)`
3. 后续与快捷键唤起相同：readiness 门闩、主线程 show、emit `launcher://shown`（`target: launcher`）、registry generation / invocation 规则不变

「打开设置」「退出」行为不变。未知或非精确 namespaced ID 仍映射为 `None` 并忽略。

## 实现落点

预计改动文件：

```text
src-tauri/src/lifecycle.rs   # TRAY_OPEN_LAUNCHER 常量、tray_action 分支、单元测试
src-tauri/src/lib.rs         # 菜单项组装顺序；Show 分支统一按 target 转发；接线测试计数
```

建议将托盘 menu event 中的 `Show` 臂从只匹配 `Settings` 改为匹配任意 `ShowTarget`，再把该 `target` 原样传给 `request_show`，避免为 Launcher / Settings 各写一份重复调用。

## 测试

- 扩展 `tray_accepts_only_exact_namespaced_ids`：断言 `TRAY_OPEN_LAUNCHER` → `Show(Launcher)`；继续拒绝近似 / 大小写 / 尾空格 ID
- 更新 `lib.rs` 中生产接线断言：单实例 + 快捷键的字面量 `request_show(app, ShowTarget::Launcher)` 仍为 2；托盘通过通用 `Show(target)` → `request_show(app, target)` 片段覆盖，并断言「打开主界面」与 `uipilot.tray.open-launcher`

## 非目标

- 托盘左键 / 双击打开主界面
- 新的生命周期状态机或 ShowTarget 变体
- 设置页与启动器页之外的第三种托盘入口
