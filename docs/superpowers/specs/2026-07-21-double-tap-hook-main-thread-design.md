# 双击修饰键 Hook 主线程安装设计

## 状态

- 日期：2026-07-21
- 状态：已获用户口头批准，待书面复核
- 范围：保存快捷键后的运行时 hook 线程归属、既有设置持久化与恢复

## 问题与证据

用户把快捷键保存为 `DoubleCtrl` 后，当前会话双击 Ctrl 无法唤起主界面；完全重启后恢复正常。

生产保存路径在 `save_settings_worker_with` 中通过 `spawn_blocking` 执行整个 `save_settings_transaction`，因此 `HotkeyHook::install` 当前由 blocking worker 调用。现有测试 `save_settings_worker_state_uses_managed_singletons` 已断言 worker 与调用线程不同。Windows 的 `WH_KEYBOARD_LL` 回调通过消息发送到安装 hook 的线程，该线程必须持续运行消息循环；blocking worker 不满足此约束。

## 目标

- 保存 `DoubleCtrl` / `DoubleAlt` 后立即在当前会话生效。
- 快捷键继续由现有 `SettingsStore` 持久化，重启后恢复上一次成功保存的配置。
- hook 安装、卸载和事务回滚中的 hook 操作都在 Tauri 主线程执行。
- 文件校验、磁盘写入和其他既有保存事务继续在 blocking worker 执行。
- 主线程调度或 hook 操作失败时，保存返回现有固定错误，且不覆盖磁盘中的旧配置。

## 方案选择

采用主线程同步调度，而不新增专用 Windows 消息循环线程。

备选方案一是让 `HotkeyHook` 自己创建并管理专用消息线程；它更独立，但需要线程启动握手、`WM_QUIT`、join 和退出竞态处理，超出当前问题所需。备选方案二是只持久化并要求重启后生效；它不满足即时生效目标。

## 设计

在 `lifecycle.rs` 增加一个小型“调度并等待”辅助函数：调用方提供主线程 dispatcher 和一次性 operation。辅助函数使用标准库 channel 接收 operation 的 `Result<(), ()>`，dispatcher 拒绝、channel 断开或 operation 失败均返回 `Err(())`。不引入依赖、超时或后台线程。

`LifecycleCoordinator` 保留现有直接安装/卸载实现，并增加保存路径专用的主线程包装：

- 安装包装克隆 `Arc<LifecycleCoordinator>` 与 `AppHandle`，在 Tauri 主线程调用直接安装实现。
- 卸载包装同样在 Tauri 主线程调用直接卸载实现。
- `save_settings_transaction` 传给 `apply_hotkey_settings_transaction` 的 hook closures 改用这两个包装。
- 启动时的 `reconcile_runtime_settings` 已位于 Tauri setup 主线程，继续直接安装，不做同步自调度，避免主线程等待自身。
- 退出清理继续走现有直接卸载路径。

保存事务顺序保持不变：解析与校验 -> 运行时快捷键/hook 副作用 -> autostart -> `SettingsStore::update_user_settings`。只有最后一步成功后磁盘配置才更新。现有持久化 schema、字段、路径和启动恢复逻辑不变。

## 错误与并发

- `run_on_main_thread` 返回错误时，事务立即失败，不执行持久化。
- 主线程 operation 的错误通过 channel 返回 worker，沿现有 `Result<(), ()>` 路径触发回滚。
- channel 发送端异常关闭时，worker 返回错误，不写磁盘。
- 保存期间已有 `CriticalReservation` 阻止退出清理与事务交错；主线程仍运行 Tauri 事件循环，因此可以执行已调度 operation。
- 不在低级键盘回调中增加阻塞工作；匹配后仍只调度现有 `request_show`。

## 测试

1. 新增最小单元测试，证明“调度并等待”会在 dispatcher 选择的另一线程执行 operation，并把成功或失败返回调用线程。
2. 新增生产接线断言，证明动态保存的 install 与 uninstall closures 均经过主线程包装，而启动恢复仍使用直接安装路径。
3. 保留并运行现有事务测试，验证 `DoubleCtrl` 解析、chord -> double tap、double tap -> chord、安装失败回滚和持久化顺序。
4. 运行完整 Rust 库测试与 `cargo check --lib`，要求零失败、零 warning。
5. 人工验证：保存 `DoubleCtrl` -> 隐藏主界面 -> 双击 Ctrl 立即显示 -> 退出并重启 -> 无需重新保存即可再次唤起。

## 非目标

- 不改变 400ms 双击窗口、按键映射或 detector 算法。
- 不改变前端 DTO、设置 schema、磁盘路径、托盘、readiness 或窗口显示语义。
- 不新增 raw input、全局热键插件能力、依赖或专用 hook 线程。
