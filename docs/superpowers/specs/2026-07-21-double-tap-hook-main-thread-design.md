# 双击修饰键 Hook 动态保存设计

## 状态

- 日期：2026-07-21
- 状态：锁与退出设计已修订，待书面复核
- 范围：保存快捷键后的 hook 安装线程、运行时绑定回滚、退出协调、既有设置持久化与恢复

## 问题与证据

用户把快捷键保存为 `DoubleCtrl` 后，当前会话双击 Ctrl 无法唤起主界面；完全重启后恢复正常。

生产保存路径在 `save_settings_worker_with` 中通过 `spawn_blocking` 执行整个 `save_settings_transaction`，因此 `HotkeyHook::install` 当前由 blocking worker 调用。现有测试 `save_settings_worker_state_uses_managed_singletons` 已断言 worker 与调用线程不同。[Microsoft 的 `LowLevelKeyboardProc` 文档](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)说明，`WH_KEYBOARD_LL` 回调通过消息发送到安装 hook 的线程，该线程必须有消息循环；短生命周期 blocking worker 不满足此约束。

另有两个独立的事务缺口：保存失败时，当前补偿逻辑不能在 chord 与 double tap 互转、`DoubleCtrl` 与 `DoubleAlt` 互转时恢复保存前的运行时绑定；`HotkeyHook::uninstall` 还忽略 `UnhookWindowsHookEx` 的失败结果并丢弃 handle。

## 目标与不变量

- 保存 `DoubleCtrl` / `DoubleAlt` 后立即在当前会话生效。
- 快捷键继续由现有 `SettingsStore` 持久化，重启后恢复上一次成功保存的配置。
- 所有 `WH_KEYBOARD_LL` 安装都在有消息循环的 Tauri 主线程执行；保存事务中的 hook 卸载、安装和补偿也统一调度到该线程，以保持同一事务内的顺序。
- 文件校验、磁盘写入和其他既有保存工作继续在 blocking worker 执行。
- 热键事务在开始变更前记录实际运行时绑定。事务自身任一步骤失败时，必须执行补偿；补偿所需主线程调度和平台调用成功后，返回前的运行时绑定必须与保存前完全一致。
- 持久化是最后一个事务步骤。任何主线程调度、热键、autostart 或持久化失败都返回现有固定错误；持久化开始前的失败不得覆盖磁盘旧配置。
- 若平台拒绝补偿操作，程序不能虚报恢复成功：保存仍返回错误，磁盘保持旧配置，可继续使用的 handle 和实际运行时状态必须被保留，以允许后续重试或重启恢复。本文不承诺在操作系统持续拒绝恢复调用时仍能强制恢复绑定。
- 运行时一致性保证适用于 `Running`、可能返回 `Running` 的托盘清理，以及仍在执行的保存事务。进入不可逆的 `Clean` 或 `SystemEnding` 后不再启动保存，退出卸载也不再改写只供事务使用的 `RuntimeSettings` 镜像。

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

## 锁与状态不变量

保存事务继续在 blocking worker 持有 `runtime_settings` 锁，以串行化并发保存。事务可以同步等待调度到 Tauri 主线程的 hook operation，但由此产生以下硬约束：

- 保存 worker 的等待边为 `runtime_settings -> Tauri 主线程`。
- 主线程 hook operation 只获取 `hotkey_hook`，不得获取 `runtime_settings`。
- 托盘退出、`WM_ENDSESSION` 和 `RunEvent::Exit` 均不得获取 `runtime_settings`。
- `CriticalReservation` 只记录在途关键工作。任何 Tauri 主线程路径都不得同步等待一个可能正在等待主线程的 reservation。
- 启动 reconcile 是唯一允许在 Tauri 主线程更新 `RuntimeSettings` 的路径；它发生在 Tauri setup 阶段，不做主线程自调度，也不与尚未启动的 save worker 并发。

因此运行时等待图只有 `save worker -> Tauri 主线程 -> hotkey_hook`，不存在返回 `runtime_settings` 或 reservation 的边。释放保存期间的 `runtime_settings` 锁需要额外的事务版本或租约才能防止并发保存交错，复杂度更高且没有必要，不采用该方案。

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
- 新增退出专用 `uninstall_hook_for_exit`，它只调用 `uninstall_production_hook`，不得锁定或清空 `runtime_settings`。

保存事务和补偿根据 hook operation 的返回值更新 `RuntimeSettings`：卸载失败时 handle 留在 `hotkey_hook` 槽位且 modifier 保持不变；卸载成功后才把 modifier 改为 `None`；补偿安装成功后才恢复旧 modifier。退出专用卸载只在生命周期不可返回 `Running` 后执行，此后不再读取 `RuntimeSettings` 发起新事务，因此退出路径无需也不得为了同步这份事务镜像而反向获取其锁。

对外仍沿用现有固定设置保存错误，不增加前端错误类型。

## 三条退出路径

退出清理不属于保存事务的主线程保证，按以下三条确定路径执行：

- 托盘退出：blocking worker 仍可等待 `CriticalReservation`，因为 Tauri 主线程没有被该等待占用，可以继续执行保存事务调度的 hook operation。clean marker 只调用 `ValidationStore::mark_clean_exit`。仅当 marker 成功、`complete_clean` 已把状态改为 `Clean` 并返回 `CleanDecision::Exit` 后，exit closure 才调用 `uninstall_hook_for_exit`，随后调度 `app.exit(0)`。reservation 超时或 marker 失败会返回 `Running`，这两种路径都不得卸载 hook。
- 系统结束：`WM_ENDSESSION` 在 Tauri 主线程执行非阻塞 system-end 转换。它获取 `exit_gate` 后立即把状态改为 `SystemEnding`；已有 `CleanAttempt::Calling` 或 `Finished` 时只观察；`Idle` / `Waiting` 且 `in_flight_critical == 0` 时转为 `Calling` 并可同步执行一次 validation marker；仍有 critical 时立即转为 `Finished(TimedOut)`、保留 session marker、通知其他 waiter 并返回。该路径不得调用 `wait_for_clean_change`，也不得卸载 hook。
- `RunEvent::Exit`：Tauri 事件循环调用 `uninstall_hook_for_exit`，执行最后一次尽力卸载。卸载失败时 handle 仍保留在槽位；进程退出负责最终回收。

`WM_ENDSESSION` 不等待也不提前卸载，是因为一个在途 save worker 可能正持有 `runtime_settings` 并等待 Tauri 主线程。窗口过程立即返回后，主线程才有机会执行已经排队的 hook operation，事务可据实际成功结果完成或补偿并释放 reservation。若 Windows 在事务完成前结束进程，原子设置文件保证旧值或完整新值，保留的 session marker 会在下次启动按未清理会话处理；不依赖返回 `WM_ENDSESSION` 后无法保证完成的异步清理。[Microsoft 的 `WM_ENDSESSION` 文档](https://learn.microsoft.com/en-us/windows/win32/shutdown/wm-endsession)说明，所有应用从该消息返回后，会话可随时结束。

## 错误与并发

- `run_on_main_thread` 返回错误时，主线程 operation 未执行，事务进入快照恢复且不持久化。
- 主线程 operation 的错误通过 channel 返回 worker；channel 发送端异常关闭也视为失败。
- blocking worker 可以同步等待，因为常规事件循环和托盘退出不会占用 Tauri 主线程等待它；辅助函数不得从主线程调用。
- 保存事务继续由 `CriticalReservation` 纳入退出协调。托盘退出可以在 worker 上有界等待，`WM_ENDSESSION` 在主线程只能非阻塞地将仍在途的工作记为超时，二者不得共享同一种等待策略。
- `uninstall_hook_for_exit` 与主线程 hook operation 只串行获取 `hotkey_hook`，不形成 `hotkey_hook -> runtime_settings` 或 `main -> reservation` 的反向等待。
- 低级键盘回调中不增加阻塞工作；匹配后仍只调用现有 `request_show`。

## 测试

1. 调度辅助函数：验证 operation 在 dispatcher 指定的另一线程执行；覆盖成功、operation 失败、dispatcher 拒绝和 channel 断开。
2. 生产接线：验证动态保存的 install/uninstall closures 均经过主线程包装，启动恢复仍走直接安装；退出接线只使用 `uninstall_hook_for_exit`，且 `WM_ENDSESSION` 不调用卸载或 `wait_for_clean_change`。
3. 卸载 API：验证卸载失败时 handle 留在 coordinator 槽位、运行时 modifier 不被清空，成功重试后才清空。
4. chord -> double tap：分别注入安装失败、旧 chord 部分注销失败、autostart 失败和持久化失败；每次断言运行时绑定等于快照且磁盘写入次数符合顺序。
5. double tap -> chord：分别注入注册失败、卸载失败、autostart 失败和持久化失败；每次断言旧 hook 恢复且新 chord 不残留。
6. `DoubleCtrl` <-> `DoubleAlt`：注入新 hook 安装失败和持久化失败，断言旧 modifier 恢复；另测补偿安装失败时返回错误并准确保留可观测运行时状态。
7. 确定性无环测试：worker 获取 reservation 和 `runtime_settings`，在模拟主线程 hook closure 上用 channel 阻塞；测试确认 worker 已进入等待后调用非阻塞 system-end，断言它在 reservation 释放前返回 `SystemEnding + Finished(TimedOut)` 且 marker 未执行；再回复 hook operation 并 join worker。使用 barrier/channel 固定相位，`recv_timeout` 只作防挂保护，超时时先释放并 join 所有线程再令测试失败，不使用 sleep。
8. 退出卸载锁测试：线程 A 持有 `runtime_settings`，线程 B 调用 `uninstall_hook_for_exit`；断言 B 在 A 释放锁前完成。超时保护必须先释放 A 并 join 两个线程，避免失败测试永久挂起。
9. 托盘转换测试：reservation 超时、marker 失败均断言 `ReturnRunning` 且卸载次数为 0；marker 成功断言先进入 `Clean`，随后 exit closure 只卸载一次。
10. 保留成功转换、解析、启动 reconcile、autostart、持久化及既有退出状态机测试；运行完整 Rust 库测试与 `cargo check --lib`，要求零失败、零 warning。
11. 人工验证由用户执行：保存 `DoubleCtrl` -> 隐藏主界面 -> 双击 Ctrl 立即显示 -> 退出并重启 -> 无需重新保存即可再次唤起。

## 非目标

- 不改变 400ms 双击窗口、按键映射或 detector 算法。
- 不改变前端 DTO、设置 schema、磁盘路径、托盘、readiness 或窗口显示语义。
- 不新增 raw input、全局热键插件能力、依赖或专用 hook 线程。
- 不保证 `WM_ENDSESSION` 返回后仍在途的保存一定能在 Windows 结束进程前完成；原子文件与 session marker 承担该失败恢复边界。
