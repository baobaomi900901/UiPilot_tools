# Stage 2A Single-Target Process Operation Kernel Design

**Date:** 2026-09-03
**Status:** Approved — accepted by the user on 2026-09-03

**Related:**

- [`Raycast-Inspired Plugin Platform Roadmap`](./2026-09-02-raycast-inspired-plugin-platform-roadmap-design.md)
- [`Stage 1 Process Identity And Operation Admission Design`](./2026-09-02-process-identity-operation-admission-design.md)
- [`Stage 1 Verification Record`](../verification/2026-09-02-process-identity-operation-admission.md)
- [`Raycast Kill Process 与 UiPilot 功能差异研究`](../../research/2026-09-02-raycast-kill-process-uipilot-comparison.md)

## 1. Goal

在 Stage 1 已验收的进程身份和准入 Module 后增加单目标操作内核，为一个已经确认并通过 Stage 1 准入的精确进程目标提供：

- Windows GUI 温和关闭；
- Windows 强制终止；
- 最终身份与保护策略复核；
- 按操作动态申请的最小句柄权限；
- 有界完成等待；
- 稳定、脱敏且能支持未来显式提权的失败分类。

本设计是路线图 Stage 2 的第一个独立子系统，简称 **Stage 2A**。它只建立单目标操作 Kernel，不提前实现 Stage 2 的目标展开、重启、提权或产品入口。

## 2. Scope

本子系统包含：

- crate-private `ProcessAuthority::terminate` 与 `ProcessAuthority::force_terminate`。
- 私有 `operation.rs` Operation Core。
- 私有 `ProcessOperationAdapter` 和确定性 Fake Adapter Seam。
- Windows 11 x64 的 `WM_CLOSE` 与 `TerminateProcess` Adapter 原语。
- operation HANDLE 的最终身份、存活、critical、PPL 和 self 复核。
- 单调时钟下的固定五秒完成期限。
- 稳定 `ProcessOperationError` 合同。
- 只针对测试自有子进程的真实 Windows 自动与人工验收。

本子系统不包含：

- 进程树或同名进程的目标展开与批量编排；
- Restart；
- Windows elevation broker、UAC 或提权重试；
- Tauri command、managed state、Launcher、Panel、确认 UI 或通知；
- Public Plugin Manifest、Capability、Runtime、SDK、Schema 或 CLI 变化；
- 配置、历史记录或其他持久化；
- macOS Adapter；
- 控制台控制事件、Windows Service Control Manager 或覆盖所有进程类型的温和退出矩阵。

## 3. Decisions

### 3.1 Separate Operation Core Inside The Existing Module

`ProcessAuthority` 继续作为唯一 Module Interface。新增私有 `operation.rs`，由 Operation Core 集中拥有最终准入顺序、最小权限、窗口选择、等待、错误优先级和重试边界。

Windows Adapter 和 Fake Adapter 只报告平台事实并执行平台原语。它们不能决定保护策略、业务成功条件或错误优先级。

该结构保持 Module 深度：调用者只需消费已绑定 intent 的租约并接收稳定结果，无需理解 HWND、HANDLE、访问掩码、窗口枚举、PID 复用或 Win32 错误。

### 3.2 Ordinary Termination Means GUI Graceful Close

`Terminate` 不调用 `TerminateProcess`。它只向精确目标进程中可见、无 owner 的顶层桌面窗口各投递一次 `WM_CLOSE`，并等待目标进程退出。

没有合格窗口、应用忽略或拒绝关闭、目标显示保存确认或者期限内未退出时，宿主不能静默升级为强制终止。用户必须在刷新目标状态后另行选择强制终止。

此语义有意不覆盖后台、托盘、控制台和服务进程。Windows 没有一个针对任意外部进程、同时保持明确身份边界的通用温和退出 API。

### 3.3 Success Means Exit Was Observed

两个方法的 `Ok(())` 都只表示 operation HANDLE 已进入 signaled 状态，即目标进程对象已退出。投递 `WM_CLOSE` 或成功调用异步的 `TerminateProcess` 本身都不算完成。

普通终止五秒内未观察到退出返回 `GracefulCloseTimedOut`。强制终止已经发起但五秒内未观察到退出返回 `TerminationPending`。两者都不能触发自动重试。

### 3.4 Fixed Five-Second Monotonic Deadline

Stage 2A 使用固定五秒期限，不引入设置项。期限从最终操作准入完成、admission HANDLE 关闭后开始：

- 普通终止的窗口枚举、投递和退出等待共享同一个 deadline；
- 强制终止的 `TerminateProcess` 调用和退出等待共享同一个 deadline；
- 等待只能使用剩余期限；
- deadline 到期时仍执行一次零时长存活检查，以区分刚刚退出和仍存活；
- 所有时间计算使用单调时钟，不使用墙上时间。

### 3.5 No Persistence Or Global Operation Registry

操作、deadline、窗口 token 和 HANDLE 都只存在于单次调用中。Stage 2A 不增加持久化、全局目标表、操作去重表或跨调用状态。

## 4. Module Interface

Stage 2A 在现有 crate-private facade 上增加：

```rust
impl ProcessAuthority {
    pub(crate) fn terminate(
        &self,
        admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError>;

    pub(crate) fn force_terminate(
        &self,
        admission: ProcessAdmission,
    ) -> Result<(), ProcessOperationError>;
}
```

Interface 合同：

- 两个方法都消费 `ProcessAdmission`；租约不能复用、克隆或序列化。
- `terminate` 只接受绑定 `ProcessIntent::Terminate` 的租约。
- `force_terminate` 只接受绑定 `ProcessIntent::ForceTerminate` 的租约。
- intent 不匹配返回 `IntentMismatch`，并在任何平台调用或副作用前关闭租约。
- `ProcessIntent::Restart` 在 Stage 2A 没有执行入口。
- `Ok(())` 唯一表示已在 operation HANDLE 上确认进程退出。
- HANDLE、HWND、访问掩码、平台失败结构和原始 Win32 code 不越过 Module Interface。

Stage 2A 不把 Module 注册为 Tauri managed state，也不增加前端或插件可调用入口。后续宿主产品入口必须先完成用户确认，再把相应 Stage 1 `ProcessAdmission` 交给这里的 intent-specific 方法。

## 5. Shared Final Operation Admission

两个方法共享以下不可拆分流程：

```text
consume ProcessAdmission
  → compare the bound intent with the requested method
  → select the approved minimum OperationAccess
  → while the admission HANDLE remains open, open one operation HANDLE for target.pid
  → compare operation-HANDLE creationTime with ProcessTarget.creationTime
  → require WaitForSingleObject(operation HANDLE, 0) == WAIT_TIMEOUT
  → re-read critical and PPL state on the operation HANDLE
  → compare the exact current UiPilot pid + creationTime identity
  → apply the fail-closed protection policy
  → close the admission HANDLE
  → immediately enter the operation-specific side-effect flow
```

最终复核必须保持 Stage 1 的优先级和安全边界：

- PID 只定位候选 operation HANDLE；完整 `pid + creationTime` 才识别目标。
- creationTime 不同立即返回 `TargetChanged`，不能检查新进程后继续执行。
- 初始最终复核发现进程已退出时返回 `TargetExited`。
- critical、PPL 或 UiPilot 自身返回 `ProtectedTarget`。
- 无法读取身份、存活或保护状态时返回 `InspectionDenied`。
- 任何失败都先关闭 admission 与 operation HANDLE，且副作用 Adapter 调用次数为零。
- 最终复核成功后，如果目标在实际副作用前并发自行退出，则返回 `Ok(())`，因为期望的最终状态已经满足；此时不再产生副作用。

确认、提权或其他用户交互不能插入最终操作准入和副作用之间。

## 6. Least-Privilege Operation Access

Operation Core 使用私有 `OperationAccess` 值对象，只允许构造两组已批准权限。

普通终止：

```text
PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE
```

强制终止：

```text
PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | PROCESS_TERMINATE
```

理由：

- limited query 支持 operation HANDLE 上的创建时间和保护属性复核；
- `SYNCHRONIZE` 支持零时长存活检查和最长五秒的退出等待；
- `PROCESS_TERMINATE` 只在调用 `TerminateProcess` 的强制终止路径申请；
- 普通终止通过 User32 窗口消息请求退出，不申请进程终止权限。

两个路径都禁止申请 `PROCESS_ALL_ACCESS`、`PROCESS_CREATE_THREAD`、VM 访问、token 操作权限或 `SeDebugPrivilege`。后续算法若改变，必须重新证明访问掩码，而不是扩宽当前值对象。

## 7. Ordinary Termination Flow

最终操作准入完成后，Operation Core 立即关闭 admission HANDLE，启动五秒 deadline，并执行：

```text
enumerate top-level desktop windows
  → retain candidates whose enumerated pid == target.pid
  → retain visible candidates
  → retain candidates with no owner window
  → if none remain, poll the operation HANDLE once:
       if signaled, return Ok
       otherwise return GracefulCloseUnavailable
  → for each retained candidate in enumeration order:
       poll operation HANDLE with zero timeout
       if signaled, stop and return Ok
       if the shared deadline is exhausted, stop before another post
       re-read the candidate HWND's current pid, visibility and owner
       if it is no longer an eligible target window, skip it
       PostMessageW(HWND, WM_CLOSE, 0, 0) once
  → if the process became signaled, return Ok
  → classify the aggregate post result
  → wait on the operation HANDLE for the deadline's remaining duration
  → perform a final zero-time poll at the deadline boundary
  → return Ok, GracefulCloseTimedOut, or CompletionUnknown
```

窗口资格定义：

- 使用 `EnumWindows` 枚举顶层桌面窗口；
- `GetWindowThreadProcessId` 的 PID 必须等于目标 PID；
- `IsWindowVisible` 必须为 true；
- `GetWindow(hwnd, GW_OWNER)` 必须返回无 owner；
- 同一进程的所有合格窗口各投递一次，不尝试推断单一“主窗口”；
- 不处理 child、message-only、隐藏、owned 或其他 PID 的窗口。

枚举时事实只用于生成候选集合。每次投递前必须重新读取 PID、可见性和 owner；任何一项不再满足时跳过该 HWND。若 deadline 在第一次成功投递前已经耗尽，最终零时长轮询仍未观察到退出，则返回 `OperationUnavailable`，表示宿主未能在有界时间内发出关闭请求。若至少一次投递已经成功，则 deadline 耗尽按 `GracefulCloseTimedOut` 处理。

`PostMessageW` 是非阻塞投递。投递成功只表示消息进入目标线程队列，不表示应用处理、同意关闭或已经退出。目标应用可以显示自己的保存确认；UiPilot 不在最终操作准入后增加新的宿主确认交互。五秒到期时，应用仍未退出即返回 `GracefulCloseTimedOut`。

聚合规则：

- 枚举后没有合格窗口：`GracefulCloseUnavailable`，窗口副作用为零。
- 枚举后没有合格窗口但 operation HANDLE 已经 signaled：`Ok(())`。
- 投递前目标已经退出：`Ok(())`，不再投递。
- 候选窗口在投递前全部消失或变为不合格，且目标仍存活：`GracefulCloseUnavailable`。
- 至少一次投递成功：继续等待；退出则 `Ok(())`，否则 `GracefulCloseTimedOut`。
- 所有投递都失败且全部为权限拒绝：`OperationAccessDenied`。
- 所有投递都失败且存在非权限类失败：`OperationUnavailable`。
- 已有投递成功后，后续窗口失败不覆盖最终退出结果。
- 已有投递成功后无法观察完成状态：`CompletionUnknown`。

### 7.1 HWND Identity Limitation

强制终止由 operation HANDLE 精确定位。`WM_CLOSE` 则只能以 HWND 为最终目标，Windows 没有把该消息直接发送给进程 HANDLE 的公开 API。

Stage 2A 通过以下措施压缩错误目标窗口：

- 最终复核后始终保持 operation HANDLE 打开；
- 每次投递前先确认该 HANDLE 尚未 signaled；
- 紧接着重新读取 HWND 当前所属 PID、可见性和 owner；
- 只在完整窗口资格仍匹配时立即调用 `PostMessageW`；
- HWND 失效或归属改变时跳过，绝不重新查找并改投另一个窗口。

仍然存在“最后一次 HANDLE/PID 复核与 `PostMessageW` 调用之间，目标退出且 HWND 与 PID 都被复用”的极窄竞态。该限制必须进入验证记录和用户可见的已知限制，不能声称普通终止与 HANDLE 定位的强制终止具有相同的身份保证。

## 8. Force-Termination Flow

最终操作准入完成后，Operation Core 立即关闭 admission HANDLE，启动五秒 deadline，并执行：

```text
poll operation HANDLE with zero timeout
  → if signaled, return Ok
  → TerminateProcess(operation HANDLE, 1) exactly once
  → if the call succeeds:
       wait on the same HANDLE for the deadline's remaining duration
       perform a final zero-time poll
       return Ok, TerminationPending, or CompletionUnknown
  → if the call fails:
       poll the same HANDLE once
       if signaled, return Ok
       if alive and the call failed for access, return OperationAccessDenied
       if alive and the call failed otherwise, return OperationUnavailable
       if completion cannot be observed, return CompletionUnknown
```

退出码 `1` 是宿主内部固定的异常终止码，不进入公共插件合同。`TerminateProcess` 返回成功后禁止再次调用、自动转入提权或把超时报告成未执行。operation HANDLE 一直保持到等待结束后再关闭。

## 9. Private Adapter And Clock Seams

`operation.rs` 在现有 `ProcessPlatformAdapter` 上定义私有子 trait：

```rust
trait ProcessOperationAdapter: ProcessPlatformAdapter {
    type WindowToken: Copy;

    fn open_operation(
        &self,
        pid: u32,
        access: OperationAccess,
    ) -> Result<Self::Handle, OperationFailure>;

    fn enumerate_top_level_windows(
        &self,
    ) -> Result<Vec<WindowCandidate<Self::WindowToken>>, OperationFailure>;

    fn window_facts(
        &self,
        window: Self::WindowToken,
    ) -> Result<WindowFacts, WindowFailure>;

    fn post_close(
        &self,
        window: Self::WindowToken,
    ) -> Result<(), WindowFailure>;

    fn terminate_process(
        &self,
        handle: &Self::Handle,
        exit_code: u32,
    ) -> Result<(), OperationFailure>;

    fn wait_for_exit(
        &self,
        handle: &Self::Handle,
        timeout: Duration,
    ) -> Result<ExitWait, OperationFailure>;
}
```

Exact Rust signatures may be mechanically adjusted during the implementation plan, but the semantic boundary is fixed:

- `WindowToken` is opaque outside the Adapter Seam；Windows 使用 HWND wrapper，Fake 使用确定 token；
- `WindowCandidate` 只携带 token 和枚举时的 PID、可见性、owner 事实；`window_facts` 在投递前重新读取同一组事实；
- `WindowFailure` 至少区分 `Gone`、`AccessDenied` 和 `Unavailable`；`Gone` 只淘汰该 candidate，其他类别进入 Core 的聚合规则；
- Core owns filtering, ordering, minimum access, error precedence, deadline and retry policy；
- Adapter owns Win32 calls and maps raw failures only to private platform failure classes；
- operation HANDLE 的 associated type 与 Stage 1 observation HANDLE 类型相同，使最终复核复用同一组对象读取原语。

时间使用另一个私有 Seam：

```rust
trait MonotonicClock {
    fn now(&self) -> Instant;
}
```

生产实现使用 `std::time::Instant`。Fake Clock 从固定基点按测试脚本推进时间，因此 deadline、剩余 timeout 和边界轮询均可确定验证，不让测试真实等待五秒。

Operation Core 作为 `CoreAuthority<A>` 在 `A: ProcessOperationAdapter` 时的私有扩展实现，消费 `CoreAdmission<A::Handle>`。它不创建第二个 authority、目标注册表或平行保护策略。

## 10. Resource Ownership And Concurrency

资源所有权：

```text
ProcessAdmission
  ├─ target + intent
  └─ admission HANDLE
          │ consume
          ▼
Operation Core
  ├─ admission HANDLE ── final revalidation succeeds → Drop before side effect
  └─ operation HANDLE ── retained through side effect and completion wait → Drop
```

- operation HANDLE 打开失败时，admission HANDLE 由 RAII 关闭。
- 最终复核失败时，两个 HANDLE 均在返回前关闭。
- 副作用阶段只保留 operation HANDLE。
- panic/unwind 也依赖 RAII；不增加裸 HANDLE 的手写清理分支。
- Window token 不拥有窗口，不能销毁窗口。

并发规则：

- 每个操作租约独立完成最终复核，不使用全局互斥或去重状态。
- 同一目标的多个租约可以并发执行。
- 一个调用先让目标退出后，另一个调用在最终准入期间观察到退出则返回 `TargetExited`。
- 如果另一个调用已经完成最终准入，随后观察到相同 operation HANDLE signaled，则返回 `Ok(())`。
- 普通终止发送过程中一旦观察到退出，立即停止发送剩余窗口。
- HWND 失效或归属改变只影响该 candidate，不能改投新窗口。
- 当前 Operation Core 是同步且有界的；后续宿主入口必须在非 UI 线程调用。请求取消和通用异步生命周期属于后续 Runtime 阶段。

## 11. Stable Error Contract

Stage 2A 新增独立 `ProcessOperationError`，不改写 Stage 1 已验收的 `ProcessError`：

| Code | Meaning | Caller action |
|---|---|---|
| `IntentMismatch` | 租约 intent 与调用方法不匹配 | 调用方缺陷；终止 |
| `TargetExited` | 最终操作准入期间目标已退出 | 刷新进程目录 |
| `TargetChanged` | PID 已指向不同 creationTime 的进程 | 刷新；禁止重定向 |
| `InspectionDenied` | 最终身份、存活或保护状态无法确认 | 失败闭合；未来可显式提权后从头执行 |
| `ProtectedTarget` | UiPilot 自身、critical 或 PPL | 终止；提权也不得绕过 |
| `OperationAccessDenied` | 无法取得最小操作权限，或窗口消息被权限边界拒绝 | 未来可显式提权后从头执行 |
| `GracefulCloseUnavailable` | 没有合格 GUI 顶层窗口 | 用户可另行选择强制终止 |
| `GracefulCloseTimedOut` | 已请求关闭但五秒内未确认退出 | 刷新；禁止自动重试或升级 |
| `OperationUnavailable` | 窗口枚举或平台操作发生非权限类失败 | 可人工重试 |
| `TerminationPending` | 强制终止已发起但期限内未确认退出 | 仅刷新状态 |
| `CompletionUnknown` | 副作用后无法可靠观察完成状态 | 仅刷新；禁止盲目重试 |
| `PlatformUnsupported` | 当前平台没有已验收 Adapter | 终止 |

错误优先级固定为：

```text
intent
→ open operation handle
→ creationTime
→ liveness
→ protection readability
→ protection policy
→ side-effect availability
→ side-effect invocation
→ completion observation
```

一旦任何副作用已经发生，后续不确定状态必须返回 `GracefulCloseTimedOut`、`TerminationPending` 或 `CompletionUnknown`，不能伪装成“未执行”。

只有副作用发生前的 `InspectionDenied` 和 `OperationAccessDenied` 可以交给未来的 elevation broker。broker 必须丢弃原租约，在用户明确同意后从确认和 Stage 1 准入链起点重新开始。Stage 2A 只提供分类，不调用 broker。

## 12. Logging And Data Handling

- `ProcessOperationError` 的普通 Display 和日志只输出稳定 code。
- 普通日志不得包含 PID、路径、命令行、窗口标题、HWND、HANDLE 或原始 Win32 code。
- Window candidate 不读取窗口标题或内容。
- 原始 Win32 code 只能在受控诊断上下文中转换为私有失败分类，不能进入普通调用结果。
- Stage 2A 不写入磁盘，不保留操作历史或进程身份。

## 13. Windows Adapter

Windows 11 x64 是 Stage 2A 的正式实现与验收基线。Adapter 使用公开 Win32 能力：

- `OpenProcess` 按 Operation Core 给出的精确访问掩码打开 non-inheritable operation HANDLE；
- `GetProcessTimes`、`WaitForSingleObject`、`IsProcessCritical` 和 `GetProcessInformation(ProcessProtectionLevelInfo)` 完成最终复核；
- `EnumWindows` 枚举顶层桌面窗口；
- `GetWindowThreadProcessId` 读取窗口当前 PID；
- `IsWindowVisible` 检查可见性；
- `GetWindow(hwnd, GW_OWNER)` 检查 owner；
- `PostMessageW(hwnd, WM_CLOSE, 0, 0)` 非阻塞请求 GUI 关闭；
- `TerminateProcess` 对精确 operation HANDLE 发起强制终止；
- `WaitForSingleObject` 在同一 operation HANDLE 上确认退出。

Windows 失败分类至少区分：目标不存在或已退出、访问拒绝、身份/保护检查不可用、窗口失效、窗口消息被 UIPI 拒绝和其他平台失败。`PostMessageW` 从 Windows Vista 起受 UIPI 约束，权限拒绝映射为 `OperationAccessDenied`。

Stage 2A 不使用 `EndTask`，因为 Microsoft 明确标注其不适用于一般用途；也不使用 `taskkill`、PowerShell、WMI、Shell fallback 或未记录的 NT 接口。

## 14. Verification

### 14.1 Fake Adapter Contract Tests

至少覆盖：

- 两种 operation access 的位集合精确匹配，并且没有额外权限。
- `IntentMismatch` 在任何平台调用前失败。
- operation HANDLE 打开失败、PID 复用、目标退出、保护状态变化、self、critical、PPL 和检查拒绝。
- admission HANDLE 在最终复核期间保持打开，并在第一次副作用之前关闭。
- 所有成功、失败、超时和 unwind 路径中的 HANDLE 都恰好关闭一次。
- 普通终止只选择可见、无 owner、PID 匹配的顶层窗口。
- 多个合格窗口各投递一次；每次投递前重新检查 PID、可见性、owner 和进程存活。
- 无窗口、候选全部变为不合格、全部权限拒绝、全部平台失败、部分投递失败、投递前退出和投递中退出。
- 五秒 deadline 扣除枚举与投递消耗；覆盖第一次投递前耗尽以及部分投递后耗尽；deadline 边界执行最终零时长轮询。
- 普通终止成功、超时和完成检查失败，并证明从不调用 `TerminateProcess`。
- 强制终止只调用一次 `TerminateProcess`，退出码为 `1`。
- 强制终止成功退出、`TerminationPending`、调用失败但目标已退出、访问拒绝和 `CompletionUnknown`。
- 所有错误都不自动重试或提权。
- 稳定错误文本不泄漏敏感字段。
- 两个租约并发操作同一目标时遵守第 10 节规则。

### 14.2 Real Windows Tests

真实测试只能操作测试自身创建并持有 HANDLE 的 helper 子进程：

1. 创建带可见、无 owner 顶层窗口和消息循环的 helper；收到 `WM_CLOSE` 后正常退出，验证普通终止。
2. 创建无窗口 helper，验证 `GracefulCloseUnavailable`。
3. 创建忽略 `WM_CLOSE` 的 helper，验证约五秒后返回 `GracefulCloseTimedOut`；测试夹具随后只清理该 helper。
4. 创建普通 helper，验证强制终止并在 operation HANDLE 上确认退出。
5. 修改目标 creationTime，验证任何副作用前返回 `TargetChanged`。
6. 验证当前测试宿主自身仍返回 `ProtectedTarget`。
7. 每项测试都在有界清理后确认无残留 helper 或 HANDLE。

helper 必须复用当前 Rust 测试可执行文件中的 ignored fixture 模式，不增加产品 helper、PowerShell 或 shell script。测试清理可以终止且只能终止自己创建并保留精确 `Child` HANDLE 的子进程。

真实测试不得终止现有用户或系统进程，不得尝试实际终止 critical/PPL 目标，不得启用 `SeDebugPrivilege`。

### 14.3 Regression And Manual Evidence

验证记录必须包含：

- focused `process_control` 测试、默认完整测试和 `cargo fmt --check` 成功；
- Clippy 在所有 `process_control` 路径中为零诊断；
- Stage 1 已记录的 all-features schema `E0425` 和 `process_control` 外 Clippy 基线已复现且未恶化，除非它们在本阶段之前已由其他工作修复；
- `Cargo.toml` 和 `Cargo.lock` 与 Stage 2A 起点 byte-identical；当前 `windows 0.61.3` 已启用 Threading 与 WindowsAndMessaging feature；
- 精确 Windows 11 build、架构、权限状态、Rust 和 Cargo 版本；
- 标准用户会话中的 GUI 温和关闭、无窗口拒绝、五秒超时和强制终止；
- HWND 身份限制、没有自动提权、没有残留 helper/HANDLE 和日志脱敏结果；
- 最终 changed-file list，以及无 Tauri、前端、公共插件合同、持久化、Restart、批量或 elevation 变化。

## 15. Exit Gate

Stage 2A 只有同时满足以下条件才可申请验收：

- 本设计规格和后续独立实施计划都已由用户验收；
- 两种操作的 Fake Adapter 合同测试通过；
- 真实 Windows 自有 helper 场景通过；
- 最小权限、最终复核、句柄关闭和副作用顺序具有确定证据；
- 所有完成结果和不确定副作用状态符合稳定错误合同；
- 无自动重试、自动强制终止或自动提权；
- 没有超出第 2 节的产品和平台改动；
- 回归、环境、已知限制和 changed-file scope 已记录；
- 用户审阅验证记录并正式验收 Stage 2A。

执行顺序仍为：

```text
written design acceptance
  → separate implementation-plan acceptance
  → development
  → automated verification
  → Windows manual acceptance
  → explicit Stage 2A acceptance
```

Stage 2A 正式验收前，不创建进程树、同名批量、Restart、elevation broker 或宿主产品入口的详细设计、实施计划或代码。

## 16. Official Windows References

- [TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)
- [Terminating a Process](https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process)
- [Process Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)
- [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)
- [EnumWindows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows)
- [GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid)
- [IsWindowVisible](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindowvisible)
- [GetWindow / GW_OWNER](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindow)
- [WM_CLOSE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close)
- [Closing the Window](https://learn.microsoft.com/en-us/windows/win32/learnwin32/closing-the-window)
- [PostMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew)
- [EndTask](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-endtask)
- [GenerateConsoleCtrlEvent](https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent)

## 17. Approval And Next Step

本文档获得用户书面验收后，Stage 2A 的设计门结束。下一步只能读取仓库实施计划规范并为 Stage 2A 编写独立实施计划。

实施计划再次获得用户验收前，不开发 Stage 2A 产品代码；Stage 2A 开发、验证和正式验收完成前，不进入 Stage 2 的下一个子系统。
