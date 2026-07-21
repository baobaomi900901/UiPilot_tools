# 双击修饰键 Hook 动态保存 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按已批准的[双击修饰键 Hook 动态保存设计](../specs/2026-07-21-double-tap-hook-main-thread-design.md)，让动态保存的低级键盘 hook 在 Tauri 主线程立即生效，并在保存失败和退出竞争中保持可恢复、无锁循环。

**Architecture:** 保存事务继续由 blocking worker 持有 `runtime_settings` 串行执行，只把 hook install/uninstall 通过标准库 channel 同步调度到 Tauri 主线程。`HotkeyHook` 和 coordinator 槽位保留失败的 handle；`RuntimeSettings` 用保存前快照补偿。退出路径只操作 `hotkey_hook`，托盘在 `Clean` 后卸载，`WM_ENDSESSION` 非阻塞进入 `SystemEnding`，`RunEvent::Exit` 最终尽力卸载。

**Tech Stack:** Rust 2021、Tauri 2、Windows `WH_KEYBOARD_LL`、`std::sync::{mpsc, Mutex, Condvar}`、现有内联单元测试。

## Global Constraints

- 基线提交为 `52722620443d74a03fbcc429e6c6ee24390f7e8a`，父 main 为 `12fed01f7ecb8df3ea9d2e9ebe6e2023abdf6ea9`。
- 不增加依赖、raw input、专用 hook 线程、前端或设置 schema 变更。
- 所有 `WH_KEYBOARD_LL` 安装都在有消息循环的 Tauri 主线程执行；启动 reconcile 直接安装，动态保存通过主线程包装安装。
- 保存 worker 可以持有 `runtime_settings` 等待主线程；主线程和所有退出路径不得获取 `runtime_settings` 或等待 `CriticalReservation`。
- 持久化必须是最后一个事务步骤，成功后不得再执行可失败的热键清理。
- 并发测试使用 barrier/channel 固定相位，禁止 `sleep`；`recv_timeout` 只能防挂，失败前必须释放并 join 全部线程。
- 不启动 dev，不控制鼠标键盘，不集成或推送 main。

---

### Task 1: 标准库主线程调度与生产包装

**Files:**
- Modify: `src-tauri/src/lifecycle.rs:1-30,615-752`
- Test: `src-tauri/src/lifecycle.rs` 内联 tests

**Interfaces:**
- Produces: `dispatch_and_wait(dispatch, operation) -> Result<(), ()>`
- Produces: `LifecycleCoordinator::install_production_hook_on_main(self: &Arc<Self>, app: &AppHandle, modifier: DoubleTapModifier) -> Result<(), ()>`
- Produces: `LifecycleCoordinator::uninstall_production_hook_on_main(self: &Arc<Self>, app: &AppHandle) -> Result<(), ()>`
- Consumes: `AppHandle::run_on_main_thread`、现有 direct install/uninstall 方法。

- [ ] **Step 1: 写四个失败测试**

在 `lifecycle.rs` tests 中增加：

```rust
#[test]
fn dispatch_and_wait_returns_operation_result_from_dispatch_thread() {
    let caller = thread::current().id();
    let observed = Arc::new(Mutex::new(None));
    let observed_for_operation = Arc::clone(&observed);
    assert_eq!(
        dispatch_and_wait(
            |operation| {
                thread::spawn(operation).join().map_err(|_| ())?;
                Ok(())
            },
            move || {
                *observed_for_operation.lock().unwrap() = Some(thread::current().id());
                Ok(())
            },
        ),
        Ok(())
    );
    assert_ne!(*observed.lock().unwrap(), Some(caller));
}

#[test]
fn dispatch_and_wait_propagates_operation_failure() {
    assert_eq!(dispatch_and_wait(|operation| { operation(); Ok(()) }, || Err(())), Err(()));
}

#[test]
fn dispatch_and_wait_stops_on_dispatch_rejection() {
    let called = Arc::new(AtomicBool::new(false));
    let called_for_operation = Arc::clone(&called);
    assert_eq!(
        dispatch_and_wait(|_| Err(()), move || {
            called_for_operation.store(true, Ordering::Relaxed);
            Ok(())
        }),
        Err(())
    );
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn dispatch_and_wait_maps_dropped_operation_to_error() {
    assert_eq!(dispatch_and_wait(|operation| { drop(operation); Ok(()) }, || Ok(())), Err(()));
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test --lib lifecycle::tests::dispatch_and_wait -- --nocapture`

Expected: 编译失败，提示 `dispatch_and_wait` 未定义。

- [ ] **Step 3: 写最小 helper**

在 production tests 均可见的位置增加：

```rust
type MainThreadOperation = Box<dyn FnOnce() + Send>;

fn dispatch_and_wait<D, O>(dispatch: D, operation: O) -> Result<(), ()>
where
    D: FnOnce(MainThreadOperation) -> Result<(), ()>,
    O: FnOnce() -> Result<(), ()> + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    dispatch(Box::new(move || {
        let _ = sender.send(operation());
    }))?;
    receiver.recv().map_err(|_| ())?
}
```

增加两个保存专用包装；包装克隆 `AppHandle` 与 coordinator，在 `run_on_main_thread` operation 内调用 direct 方法并等待结果。`save_settings_transaction` 的 hook closures 使用包装；`reconcile_runtime_settings` 继续在 setup 主线程调用 direct install。

- [ ] **Step 4: 运行 GREEN**

Run: `cargo test --lib lifecycle::tests::dispatch_and_wait -- --nocapture`

Expected: 4 个测试通过。

- [ ] **Step 5: 增加生产接线断言并验证 RED/GREEN**

测试读取 `include_str!("lifecycle.rs")` / `include_str!("lib.rs")`，断言动态保存同时出现 `install_production_hook_on_main`、`uninstall_production_hook_on_main`，启动 reconcile 使用 direct install，`RunEvent::Exit` 后续使用 Task 4 的退出 helper。先写断言观察失败，再完成接线并运行：

Run: `cargo test --lib production_wiring -- --nocapture`

Expected: PASS。

### Task 2: 可失败卸载与 coordinator handle 重试

**Files:**
- Modify: `src-tauri/src/hotkey_hook.rs:64-138`
- Modify: `src-tauri/src/lifecycle.rs:658-689`
- Test: 两个文件的内联 tests

**Interfaces:**
- Produces: `HotkeyHook::uninstall(self) -> Result<(), HotkeyHook>`
- Produces: `HotkeyHook::uninstall_with<U>(self, unhook: U) -> Result<(), HotkeyHook>`，其中 `U: FnOnce() -> Result<(), ()>`，作为私有测试 seam。
- Produces: `uninstall_slot_with(slot, uninstall) -> Result<(), ()>`，失败对象放回原槽位。

- [ ] **Step 1: 写底层失败保留测试并运行 RED**

```rust
#[test]
fn failed_uninstall_keeps_handle_and_callback_state_for_retry() {
    *hook_state().lock().unwrap() = Some(HookState {});
    let hook = HotkeyHook {};
    let hook = hook.uninstall_with(|| Err(())).unwrap_err();
    assert!(hook_state().lock().unwrap().is_some());
    hook.uninstall_with(|| Ok(())).unwrap();
    assert!(hook_state().lock().unwrap().is_none());
}
```

Run: `cargo test --lib hotkey_hook::tests::failed_uninstall -- --nocapture`

Expected: 编译失败，旧 `uninstall` 返回 `()` 且没有 `uninstall_with`。

- [ ] **Step 2: 实现底层 GREEN**

`uninstall_with` 先调用注入的 Win32 unhook。失败返回 `Err(self)` 且不清理 `HOOK_STATE`；成功才清理并返回 `Ok(())`。production `uninstall` 把 `UnhookWindowsHookEx` 的 `Result` 传入；test/instrumentation 使用 `Ok(())`。

- [ ] **Step 3: 写槽位失败重试测试并运行 RED**

```rust
#[test]
fn uninstall_slot_reinserts_failed_handle_and_retries() {
    let slot = Mutex::new(Some("handle"));
    assert_eq!(uninstall_slot_with(&slot, |handle| Err(handle)), Err(()));
    assert_eq!(*slot.lock().unwrap(), Some("handle"));
    assert_eq!(uninstall_slot_with(&slot, |_| Ok(())), Ok(()));
    assert_eq!(*slot.lock().unwrap(), None);
}
```

Run: `cargo test --lib lifecycle::tests::uninstall_slot -- --nocapture`

Expected: 编译失败，helper 未定义。

- [ ] **Step 4: 实现槽位 GREEN**

```rust
fn uninstall_slot_with<T, U>(slot: &Mutex<Option<T>>, uninstall: U) -> Result<(), ()>
where
    U: FnOnce(T) -> Result<(), T>,
{
    let mut slot = slot.lock().map_err(|_| ())?;
    let Some(installed) = slot.take() else { return Ok(()); };
    match uninstall(installed) {
        Ok(()) => Ok(()),
        Err(installed) => {
            *slot = Some(installed);
            Err(())
        }
    }
}
```

`uninstall_production_hook` 调用此 helper。运行两个定向测试，确认失败保留、成功重试。

### Task 3: 快照补偿与持久化终点

**Files:**
- Modify: `src-tauri/src/lifecycle.rs:123-348`
- Test: `src-tauri/src/lifecycle.rs:2553-3257,3780-3986` 附近内联 tests

**Interfaces:**
- Produces: `RuntimeSettings::restore_hotkey_snapshot<R, U, IH, UH>(&mut self, before: &Self, register: &mut R, unregister: &mut U, install_hook: &mut IH, uninstall_hook: &mut UH) -> Result<(), ()>`；closure bounds 与 `apply_hotkey_binding` 相同。
- Updates: `RuntimeSettings::apply_hotkey_binding(...)` 保持既有调用签名。
- Updates: `RuntimeSettings` 派生 `Clone, Debug, Eq, PartialEq`，供快照与实际状态断言使用。
- Test helper: `FailurePoint::{Register, Unregister, Install, Uninstall, ReadAutostart, ChangeAutostart, Persist}`，带调用序号的变体保存 `usize`。
- Test helper: `HotkeyBindingProbe::from_runtime(&RuntimeSettings, &[FailurePoint])` 和 `actual_runtime() -> RuntimeSettings`。
- Test helper: `chord_to_ctrl(old)`, `alt_to_chord(requested)`, `ctrl_to_alt()` 返回字段完整的 `HotkeyBindingChange`，autostart 测试目标均为 `true`。

- [ ] **Step 1: 扩展 probe 并写失败矩阵测试**

让 `HotkeyBindingProbe` 用 `Vec<FailurePoint>` 按调用序号注入 register、unregister、install、uninstall、autostart、persist 失败，并维护 `actual_registered` 与 `actual_hook`。增加以下测试，每个测试先断言旧实现失败：

```rust
#[test]
fn hotkey_transaction_chord_to_double_tap_failures_restore_snapshot() {
    let old: Shortcut = "Alt+Space".parse().unwrap();
    for failures in [
        vec![FailurePoint::Install(1)],
        vec![FailurePoint::Unregister(1)],
        vec![FailurePoint::ReadAutostart],
        vec![FailurePoint::ChangeAutostart(1)],
        vec![FailurePoint::Persist],
    ] {
        let before = RuntimeSettings { registered: vec![old], installed_hook: None };
        let mut runtime = before.clone();
        let probe = HotkeyBindingProbe::from_runtime(&before, &failures);
        assert_eq!(apply_hotkey_binding_with_probe(&mut runtime, chord_to_ctrl(old), &probe), Err(()));
        assert_eq!(runtime, before);
        assert_eq!(probe.actual_runtime(), before);
    }
}

#[test]
fn hotkey_transaction_double_tap_to_chord_failures_restore_snapshot() {
    let requested: Shortcut = "Ctrl+Space".parse().unwrap();
    for failures in [
        vec![FailurePoint::Register(1)],
        vec![FailurePoint::Uninstall(1)],
        vec![FailurePoint::ReadAutostart],
        vec![FailurePoint::ChangeAutostart(1)],
        vec![FailurePoint::Persist],
    ] {
        let before = RuntimeSettings { registered: Vec::new(), installed_hook: Some(DoubleTapModifier::Alt) };
        let mut runtime = before.clone();
        let probe = HotkeyBindingProbe::from_runtime(&before, &failures);
        assert_eq!(apply_hotkey_binding_with_probe(&mut runtime, alt_to_chord(requested), &probe), Err(()));
        assert_eq!(runtime, before);
        assert_eq!(probe.actual_runtime(), before);
    }
}

#[test]
fn hotkey_transaction_modifier_failures_restore_snapshot() {
    for failures in [
        vec![FailurePoint::Uninstall(1)],
        vec![FailurePoint::Install(1)],
        vec![FailurePoint::ReadAutostart],
        vec![FailurePoint::ChangeAutostart(1)],
        vec![FailurePoint::Persist],
    ] {
        let before = RuntimeSettings { registered: Vec::new(), installed_hook: Some(DoubleTapModifier::Ctrl) };
        let mut runtime = before.clone();
        let probe = HotkeyBindingProbe::from_runtime(&before, &failures);
        assert_eq!(apply_hotkey_binding_with_probe(&mut runtime, ctrl_to_alt(), &probe), Err(()));
        assert_eq!(runtime, before);
        assert_eq!(probe.actual_runtime(), before);
    }
}

#[test]
fn hotkey_transaction_rollback_failure_keeps_observed_actual_state() {
    let before = RuntimeSettings { registered: Vec::new(), installed_hook: Some(DoubleTapModifier::Ctrl) };
    let mut runtime = before.clone();
    let probe = HotkeyBindingProbe::from_runtime(
        &before,
        &[FailurePoint::Install(1), FailurePoint::Install(2)],
    );
    assert_eq!(apply_hotkey_binding_with_probe(&mut runtime, ctrl_to_alt(), &probe), Err(()));
    assert_eq!(runtime, probe.actual_runtime());
    assert_ne!(runtime, before);
}

#[test]
fn hotkey_transaction_persistence_is_last() {
    let old: Shortcut = "Alt+Space".parse().unwrap();
    let before = RuntimeSettings { registered: vec![old], installed_hook: None };
    let mut runtime = before.clone();
    let probe = HotkeyBindingProbe::from_runtime(&before, &[]);
    apply_hotkey_binding_with_probe(&mut runtime, chord_to_ctrl(old), &probe).unwrap();
    assert_eq!(probe.trace.borrow().last().map(String::as_str), Some("persist"));
}
```

每个可恢复失败都断言 `RuntimeSettings == before`、probe 平台状态等于 before、persist 前失败时 persist 调用为 0。补偿自身失败时断言返回 `Err(())`，并断言 `RuntimeSettings` 等于 probe 的实际剩余状态，而不是虚报快照。

Run: `cargo test --lib lifecycle::tests::hotkey_transaction_ -- --nocapture`

Expected: 至少一个断言失败，证明旧实现丢失旧绑定或在 persist 后清理。

- [ ] **Step 2: 实现统一快照恢复**

在第一项副作用前 `let before = self.clone()`。成功转换顺序固定为：

```text
chord -> double: install requested hook, unregister old chords
double -> chord: register requested chord, uninstall old hook
double -> double: uninstall old hook, install requested hook
chord -> chord: register requested chord, unregister all other chords
all: read/change autostart, persist last, return without more hotkey calls
```

每个平台调用成功后才更新 `registered` / `installed_hook`。任一步失败时先尽力恢复 autostart，再调用单一 `restore_hotkey_snapshot`：先补回缺失的旧 chord/旧 hook，再删除事务新增项；如果恢复旧 hook 需要替换 modifier，则先卸载当前 hook。记录每次成功补偿的实际状态；任何补偿失败都返回 `Err(())`。

- [ ] **Step 3: 运行 GREEN 并重构旧测试**

更新只断言旧顺序的测试，使其断言新设计顺序；删除已被统一快照测试覆盖的旧 ad-hoc rollback helper 测试，不保留两套补偿实现。

Run: `cargo test --lib lifecycle::tests -- --nocapture`

Expected: lifecycle 全部通过，无 warning。

### Task 4: 无环退出状态机与确定性并发测试

**Files:**
- Modify: `src-tauri/src/lifecycle.rs:1099-1460`
- Modify: `src-tauri/src/lib.rs:231-240`
- Test: 两个文件的内联 tests

**Interfaces:**
- Produces: `LifecycleCoordinator::uninstall_hook_for_exit(&self)`，只操作 `hotkey_hook`。
- Produces: `begin_system_end_nonblocking(now: Instant) -> CleanDecision`。
- Produces: `run_system_end_nonblocking_with(now, marker) -> CleanDecision`。

- [ ] **Step 1: 写退出行为与无环失败测试**

增加：

```rust
#[test]
fn exit_coordination_system_end_does_not_wait_for_worker_waiting_on_main() {
    let coordinator = coordinator_for_test();
    let reservation = coordinator.reserve_critical().unwrap();
    let (operation_ready_tx, operation_ready_rx) = mpsc::channel();
    let (operation_result_tx, operation_result_rx) = mpsc::channel();
    let worker_coordinator = Arc::clone(&coordinator);
    let worker = thread::spawn(move || {
        let _reservation = reservation;
        worker_coordinator.apply_hotkey_settings_transaction(
            chord_to_ctrl("Alt+Space".parse().unwrap()),
            (|_| Ok(()), |_| Ok(())),
            (move |_| { operation_ready_tx.send(()).unwrap(); operation_result_rx.recv().unwrap() }, || Ok(())),
            (|| Ok(false), |_| Ok(())),
            || Ok(()),
        )
    });
    operation_ready_rx.recv().unwrap();
    let (system_done_tx, system_done_rx) = mpsc::channel();
    let system_coordinator = Arc::clone(&coordinator);
    let system = thread::spawn(move || {
        system_done_tx.send(system_coordinator.run_system_end_nonblocking_with(
            Instant::now(),
            || panic!("in-flight system end must preserve the session marker"),
        )).unwrap();
    });
    let system_result = system_done_rx.recv_timeout(Duration::from_secs(1));
    operation_result_tx.send(Ok(())).unwrap();
    assert_eq!(worker.join().unwrap(), Ok(()));
    system.join().unwrap();
    assert_eq!(system_result, Ok(CleanDecision::ObserveOnly));
    assert!(matches!(exit_snapshot(&coordinator), (ExitState::SystemEnding, 0, CleanAttempt::Finished(CleanResult::TimedOut))));
}

#[test]
fn exit_coordination_uninstall_does_not_wait_for_runtime_settings() {
    let coordinator = coordinator_for_test();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder_coordinator = Arc::clone(&coordinator);
    let holder = thread::spawn(move || {
        let _runtime = holder_coordinator.runtime_settings.lock().unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.recv().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let exit_coordinator = Arc::clone(&coordinator);
    let exit = thread::spawn(move || { exit_coordinator.uninstall_hook_for_exit(); done_tx.send(()).unwrap(); });
    let completion = done_rx.recv_timeout(Duration::from_secs(1));
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    exit.join().unwrap();
    assert_eq!(completion, Ok(()));
}
```

`exit_coordination_tray_unloads_only_after_clean_commit` 使用以下三个确定分支；生产 exit closure 将相同位置的计数操作替换为 `uninstall_hook_for_exit`：

```rust
#[test]
fn exit_coordination_tray_unloads_only_after_clean_commit() {
    let timed_out = coordinator_for_test();
    let reservation = timed_out.reserve_critical().unwrap();
    let deadline = Instant::now();
    let uninstall_count = Cell::new(0);
    assert_eq!(
        timed_out.run_tray_quit_with(
            timed_out.begin_tray_clean(deadline),
            |_| deadline,
            || panic!("timed out clean must not call marker"),
            || uninstall_count.set(uninstall_count.get() + 1),
            |_| {},
        ),
        CleanDecision::ReturnRunning,
    );
    assert_eq!(uninstall_count.get(), 0);
    drop(reservation);

    let failed = coordinator_for_test();
    assert_eq!(
        failed.run_tray_quit_with(
            failed.begin_tray_clean(deadline),
            |_| unreachable!(),
            || CleanResult::Failed,
            || uninstall_count.set(uninstall_count.get() + 1),
            |_| {},
        ),
        CleanDecision::ReturnRunning,
    );
    assert_eq!(uninstall_count.get(), 0);

    let succeeded = coordinator_for_test();
    assert_eq!(
        succeeded.run_tray_quit_with(
            succeeded.begin_tray_clean(deadline),
            |_| unreachable!(),
            || CleanResult::Succeeded,
            || {
                assert_eq!(exit_snapshot(&succeeded).0, ExitState::Clean);
                uninstall_count.set(uninstall_count.get() + 1);
            },
            |_| {},
        ),
        CleanDecision::Exit,
    );
    assert_eq!(uninstall_count.get(), 1);
}
```

`exit_coordination_session_wiring_is_nonblocking` 读取 production source，定位 `session_subclass_proc` 和 `run_system_end` 的函数文本，断言前者只调用 `run_system_end`，后者不含 `wait_for_clean_change`、`uninstall_production_hook` 或 `uninstall_hook_for_exit`。

无环测试必须按规格在超时后先回复模拟 hook operation、释放锁并 join，再断言超时错误；测试代码不得出现 `thread::sleep`。

Run: `cargo test --lib lifecycle::tests::exit_coordination_ -- --nocapture`

Expected: 旧实现超时或断言失败，但测试进程正常结束。

- [ ] **Step 2: 实现非阻塞 system-end**

在一个 `exit_gate` 临界区内先设 `SystemEnding`：

```rust
match gate.clean_attempt {
    CleanAttempt::Calling { .. } | CleanAttempt::Finished(_) => ObserveOnly,
    CleanAttempt::Idle | CleanAttempt::Waiting { .. } if gate.in_flight_critical == 0 => {
        gate.clean_attempt = CleanAttempt::Calling { owner: CleanOwner::System, deadline: now };
        CallMarker
    }
    CleanAttempt::Idle | CleanAttempt::Waiting { .. } => {
        gate.clean_attempt = CleanAttempt::Finished(CleanResult::TimedOut);
        ObserveOnly
    }
}
```

替换等待式 `run_system_end_with`；若把 `Waiting` 改为 `Finished`，释放锁后通知 condvar。marker 只调用 `mark_clean_exit`，不得卸载或等待。

- [ ] **Step 3: 接线托盘和最终退出**

`uninstall_hook_for_exit` 只忽略 `uninstall_production_hook` 的返回值。托盘 marker 删除卸载；仅 `CleanDecision::Exit` 的 exit closure 先调用该 helper，再调度 `app.exit(0)`。`session_subclass_proc` 删除提前卸载。`RunEvent::Exit` 改用该 helper。

- [ ] **Step 4: 运行 GREEN**

Run: `cargo test --lib lifecycle::tests -- --nocapture`

Expected: lifecycle 全部通过，无 sleep、无永久等待。

### Task 5: 全量验证、自审与实现提交

**Files:**
- Verify only: `src-tauri/src/hotkey_hook.rs`
- Verify only: `src-tauri/src/lifecycle.rs`
- Verify only: `src-tauri/src/lib.rs`

- [ ] **Step 1: 格式化并检查**

Run: `cargo fmt --all`

Run: `cargo fmt --all -- --check`

Expected: exit 0，无输出。

- [ ] **Step 2: 完整测试与检查**

Run: `cargo test --lib`

Expected: exit 0，0 failed，允许既有 ignored 测试，不允许 compiler warning。

Run: `cargo check --lib`

Expected: exit 0，0 warning。

- [ ] **Step 3: 恢复测试生成的无关文件**

检查 `git status --short`。只允许计划、规格和上述三个 Rust 文件；若 Tauri 重写 `src-tauri/permissions/autogenerated/*.toml`，用精确 `git restore -- <files>` 恢复，不得批量恢复任务文件。

- [ ] **Step 4: 调用者与锁序自审**

Run: `rg -n "HotkeyHook::install|\.uninstall\(|install_production_hook|uninstall_production_hook|uninstall_hook_for_exit|runtime_settings\.lock|wait_for_clean_change|WM_ENDSESSION|RunEvent::Exit" src-tauri/src`

逐条确认：install 只在 setup main 或动态 main wrapper；失败 handle 不丢；状态不先清空；退出不碰 `runtime_settings`；主线程不等 reservation；persist 后无热键调用。

- [ ] **Step 5: 提交详细中文实现**

```powershell
git add -- src-tauri/src/hotkey_hook.rs src-tauri/src/lifecycle.rs src-tauri/src/lib.rs
git commit -m "修复：让双击快捷键动态保存安全生效" `
  -m "根因：保存工作线程安装低级键盘钩子且退出路径形成运行时锁与主线程的循环等待。" `
  -m "实现：主线程同步调度钩子操作，保留卸载失败句柄，按快照补偿运行时绑定，并拆分托盘、系统结束和最终退出清理。" `
  -m "测试：覆盖调度错误、转换回滚、持久化终点、卸载重试和无休眠的确定性退出并发。"
```

- [ ] **Step 6: 提交后验证元数据**

Run: `git status --short`

Expected: 空。

Run: `git log --oneline --decorate -3`

Expected: 当前分支包含计划提交、实现提交，祖先仍为 `5272262` / `12fed01`。
