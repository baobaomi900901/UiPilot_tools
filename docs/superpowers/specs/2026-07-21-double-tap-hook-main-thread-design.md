# 双击修饰键 Hook 动态保存设计

## 状态

- 日期：2026-07-21
- 状态：阻塞意见已修订，待书面复核
- 范围：保存快捷键后的 hook 安装线程、运行时绑定回滚、既有设置持久化与恢复

## 问题与证据

用户把快捷键保存为 `DoubleCtrl` 后，当前会话双击 Ctrl 无法唤起主界面；完全重启后恢复正常。

生产保存路径在 `save_settings_worker_with` 中通过 `spawn_blocking` 执行整个 `save_settings_transaction`，因此 `HotkeyHook::install` 当前由 blocking worker 调用。现有测试 `save_settings_worker_state_uses_managed_singletons` 已断言 worker 与调用线程不同。[Microsoft 的 `LowLevelKeyboardProc` 文档](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)说明，`WH_KEYBOARD_LL` 回调通过消息发送到安装 hook 的线程，该线程必须有消息循环；短生命周期 blocking worker 不满足此约束。

另有两个独立的事务缺口：保存失败时，当前补偿逻辑不能在 chord 与 double tap 互转、`DoubleCtrl` 与 `DoubleAlt` 互转时恢复保存前的运行时绑定；`HotkeyHook::uninstall` 还忽略 `UnhookWindowsHookEx` 的失败结果并丢弃 handle。

## 目标与不变量

- 保存 `DoubleCtrl` / `DoubleAlt` 后立即在当前会话生效。
- 快捷键继续由现有 `SettingsStore` 持久化，重启后恢复上一次成功保存的配置。
- 所有 `WH_KEYBOARD_LL` 安装都在有消息循环的 Tauri 主线程执行；保存事务中的 hook 卸载、安装和补偿也统一调度到该线程，以保持同一事务内的顺序。
- 文件校验、磁盘写入和其他既有保存工作继续在 blocking worker 执行。
- 热键事务在开始变更前记录实际运行时绑定。事务自身任一步骤失败时，必须执行补偿；补偿所需平台调用成功后，返回前的运行时绑定必须与保存前完全一致。
- 持久化是最后一个事务步骤。任何主线程调度、热键、autostart 或持久化失败都返回现有固定错误；持久化开始前的失败不得覆盖磁盘旧配置。
- 若平台拒绝补偿操作，程序不能虚报恢复成功：保存仍返回错误，磁盘保持旧配置，可继续使用的 handle 和实际运行时状态必须被保留，以允许后续重试或重启恢复。本文不承诺在操作系统持续拒绝恢复调用时仍能强制恢复绑定。

## 方案选择

采用主线程同步调度和补偿式事务，不新增专用 Windows 消息循环线程。

让 `HotkeyHook` 自己管理专用线程需要启动握手、`WM_QUIT`、join 和退出竞态处理，超出当前问题所需。只持久化并要求重启后生效则不满足即时生效目标。

## 主线程调度

在 `lifecycle.rs` 增加一个小型“调度并等待”辅助函数：调用方提供主线程 dispatcher 和一次性 operation。辅助函数使用标准库 channel 接收 operation 的 `Result<(), ()>`；dispatcher 拒绝、channel 断开或 operation 失败均返回 `Err(())`。不引入依赖、超时或新后台线程。

`LifecycleCoordinator` 保留直接安装/卸载实现，并增加保存路径专用的主线程包装：

- 安装包装克隆 `Arc<LifecycleCoordinator>` 与 `AppHandle`，在 Tauri 主线程调用直接安装实现。
- 卸载包装同样在 Tauri 主线程调用直接卸载实现。
- `save_settings_transaction` 传给热键事务的 hook closures 使用这两个包装，补偿也复用相同 closures。
- 启动时的 `reconcile_runtime_settings` 已位于 Tauri setup 主线程，继续直接安装，避免主线程同步等待自身。

线程保证刻意限定为“hook 安装”和“保存事务内的 hook 重配”。[Microsoft 的 `UnhookWindowsHookEx` 文档](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-unhookwindowshookex)定义了失败返回和并发回调语义，但没有要求只能由安装线程卸载，因此不把所有进程退出清理迁移到主线程。

## 热键事务与回滚

`RuntimeSettings::apply_hotkey_binding` 在第一项副作用前克隆保存前的 `registered` 与 `installed_hook`。状态字段只在对应平台操作成功后更新，不能在调用卸载前先 `take()` 或清空。

成功路径按以下顺序执行，并确保持久化之后没有可失败的热键清理：

1. chord -> double tap：先安装新 hook，再注销旧 chord。
2. double tap -> chord：先注册新 chord，再卸载旧 hook。
3. double tap -> 不同 double tap：卸载旧 hook，再安装新 hook。
4. chord -> 不同 chord：先注册新 chord，再注销旧 chord，继续遵守现有最多两个临时注册项的上限。
5. 读取并按需修改 autostart。
6. 调用 `SettingsStore::update_user_settings` 持久化；成功后不再执行平台热键操作。

任一步骤失败时，调用单一恢复函数，把实际运行时状态与保存前快照对齐：卸载事务新增的 hook、重装保存前的 hook、注销事务新增的 chord、重新注册保存前被注销的 chord。恢复函数记录每个成功的补偿操作并据此更新 `RuntimeSettings`；若某项补偿失败，继续执行其余独立补偿，最终返回错误。autostart 沿现有方式恢复为读取到的旧值，其恢复失败同样不能转成成功。

这覆盖至少以下原有缺口：

- chord -> double tap：安装、autostart 或持久化失败后恢复旧 chord，并移除新 hook。
- double tap -> chord：注册、卸载、autostart 或持久化失败后恢复旧 hook，并移除新 chord。
- `DoubleCtrl` -> `DoubleAlt`（及反向）：新 hook 安装失败或后续步骤失败后重装旧 modifier。
- 部分注销失败：重新注册此前已成功注销的旧 chord，不能留下半更新状态。

## 卸载失败语义

`HotkeyHook::uninstall` 改为可失败 API，例如 `Result<(), HotkeyHook>`：

- `UnhookWindowsHookEx` 成功后才清理全局 hook 状态并销毁对象。
- `UnhookWindowsHookEx` 失败时返回仍持有原 handle 的 `HotkeyHook`，不清理回调状态。
- `LifecycleCoordinator::uninstall_production_hook` 获取 `hotkey_hook` 锁失败时返回 `Err(())`；取出对象后若卸载失败，立即放回同一槽位并返回错误，以便重试。
- `RuntimeSettings::installed_hook` 只在卸载成功后变更。保存事务能据此区分“旧 hook 仍在”与“需要重装旧 hook”。

对外仍沿用现有固定设置保存错误，不增加前端错误类型。

## 三条退出路径

退出清理不属于保存事务的主线程保证，保持现状并明确为尽力执行：

- 托盘退出：`request_tray_quit` 的 blocking worker 在 clean marker 中直接调用卸载。
- 系统结束：`WM_ENDSESSION` 的窗口 subclass 回调在等待 clean reservation 前先直接卸载，`run_system_end` 的 marker 还会幂等重试。
- `RunEvent::Exit`：Tauri 事件循环执行最后一次幂等卸载。

这些调用可使用新的可失败卸载 API，但进程确定退出时只记录/忽略清理错误，不把退出变成保存事务。`CriticalReservation` 只让 clean marker 有界等待正在进行的关键操作，并不能阻止 `WM_ENDSESSION` 的提前卸载，也不保证所有退出路径与保存事务互斥；设计不再作此声明。托盘 clean 失败后继续运行时可能已卸载 hook，是既有退出恢复问题，不纳入本次动态保存修复。

## 错误与并发

- `run_on_main_thread` 返回错误时，主线程 operation 未执行，事务进入快照恢复且不持久化。
- 主线程 operation 的错误通过 channel 返回 worker；channel 发送端异常关闭也视为失败。
- blocking worker 可以同步等待，因为 Tauri 主线程继续运行事件循环；辅助函数不得从主线程调用。
- 保存事务继续由 `CriticalReservation` 纳入退出协调，但不依赖它提供不存在的完全互斥保证。
- 低级键盘回调中不增加阻塞工作；匹配后仍只调用现有 `request_show`。

## 测试

1. 调度辅助函数：验证 operation 在 dispatcher 指定的另一线程执行；覆盖成功、operation 失败、dispatcher 拒绝和 channel 断开。
2. 生产接线：验证动态保存的 install/uninstall closures 均经过主线程包装，启动恢复仍走直接安装。
3. 卸载 API：验证卸载失败时 handle 留在 coordinator 槽位、运行时 modifier 不被清空，成功重试后才清空。
4. chord -> double tap：分别注入安装失败、旧 chord 部分注销失败、autostart 失败和持久化失败；每次断言运行时绑定等于快照且磁盘写入次数符合顺序。
5. double tap -> chord：分别注入注册失败、卸载失败、autostart 失败和持久化失败；每次断言旧 hook 恢复且新 chord 不残留。
6. `DoubleCtrl` <-> `DoubleAlt`：注入新 hook 安装失败和持久化失败，断言旧 modifier 恢复；另测补偿安装失败时返回错误并准确保留可观测运行时状态。
7. 保留成功转换、解析、启动 reconcile、autostart 和持久化测试；运行完整 Rust 库测试与 `cargo check --lib`，要求零失败、零 warning。
8. 人工验证由用户执行：保存 `DoubleCtrl` -> 隐藏主界面 -> 双击 Ctrl 立即显示 -> 退出并重启 -> 无需重新保存即可再次唤起。

## 非目标

- 不改变 400ms 双击窗口、按键映射或 detector 算法。
- 不改变前端 DTO、设置 schema、磁盘路径、托盘、readiness 或窗口显示语义。
- 不新增 raw input、全局热键插件能力、依赖或专用 hook 线程。
- 不在本次修复中重构托盘 clean 失败后的运行时恢复；该问题已在退出路径章节明确记录。
