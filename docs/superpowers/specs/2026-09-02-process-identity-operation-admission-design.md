# Stage 1 Process Identity And Operation Admission Design

**Date:** 2026-09-02
**Status:** Review — conversational design approved; written specification awaits user acceptance

**Related:**

- [`Raycast-Inspired Plugin Platform Roadmap`](./2026-09-02-raycast-inspired-plugin-platform-roadmap-design.md)
- [`Raycast Kill Process 与 UiPilot 功能差异研究`](../../research/2026-09-02-raycast-kill-process-uipilot-comparison.md)
- [`UiPilot 跨平台启动器 MVP 设计`](./2026-07-17-cross-platform-launcher-mvp-design.md)

## 1. Goal

建立宿主拥有的进程身份与操作准入 Module，为 Stage 2 的终止、强制终止、进程树、同名批量和重启提供唯一安全入口。

本阶段同时交付最小只读 `ProcessCatalog`。它只枚举能够建立可靠目标身份的进程，不提供 CPU、内存、搜索、排序 UI 或任何进程副作用。

阶段完成后必须能够证明：调用者不能仅凭 PID 指向进程；宿主重启、PID 复用、目标退出、UiPilot 自身、Windows critical process、Protected Process/PPL 和无法检查的目标都具有确定、失败闭合的结果。

## 2. Scope

本阶段包含：

- crate-private `process_control` Module。
- 每次宿主生命周期唯一的 `hostInstanceId`。
- 最小只读进程快照和结构化遗漏统计。
- `ProcessTarget` 值对象和 PID 复用防护。
- 绑定 `Terminate`、`ForceTerminate`、`Restart` 意图的身份与保护策略准入租约。
- Windows 11 x64 Adapter。
- Fake Adapter 合同测试和无副作用的真实 Windows 验证。
- 稳定、脱敏、可分类的内部错误合同。

本阶段不包含：

- 终止、重启、提权或任何其他进程副作用。
- 进程树或同名批量的目标展开。
- Tauri command、Launcher 入口、设置、面板或其他 UI。
- Public Plugin Manifest、权限、Runtime、SDK、Schema 或 CLI 变化。
- AI Tool 或宿主确认 UI。
- CPU、内存、命令行、环境变量、图标、搜索或排序能力。
- macOS Adapter；它属于路线图 Stage 8。
- 用户配置的进程名称或路径禁止名单。

## 3. Decisions

### 3.1 Include The Minimal Catalog

仓库当前不存在通用只读 `ProcessCatalog`。可靠目标身份不能脱离目标发现独立验证，因此本阶段把最小目录作为直接业务依赖同时交付。该目录只承担安全身份签发所需的读取职责，不扩展为完整任务管理器。

### 3.2 Value Identity With Final Revalidation

采用值对象身份和最终复核，不使用宿主目标注册表，也不为整个快照长期保留 Windows HANDLE。

该方案避免注册表容量、过期和清理状态；同时通过 `pid + creationTime` 与最终 HANDLE 上的重复比较，防止 PID 复用把请求重定向到其他进程。

### 3.3 Fail Closed Protection

UiPilot 自身、Windows critical process、Protected Process/PPL 一律拒绝。无法读取保护属性时返回 `InspectionDenied`，不能假定目标安全。

### 3.4 Partial Snapshot

单个进程退出、改变或拒绝检查不会拖垮整个快照。目录只返回能够建立可靠身份的记录，并把其他候选归入结构化遗漏统计。系统快照本身无法创建或完整遍历时，整次快照失败。

### 3.5 Process Targets Are Ephemeral

目标不写入磁盘，也不能跨 UiPilot 宿主重启恢复。旧宿主签发的目标在新宿主中稳定返回 `ExpiredTarget`。

## 4. Module And Seam

新增 `src-tauri/src/process_control/`，其中 `mod.rs` 暴露唯一 crate-private Module Interface，平台实现和系统句柄保持私有。

调用者只需理解两个操作：

```rust
fn snapshot(&self) -> Result<ProcessSnapshot, ProcessError>;

fn admit(
    &self,
    target: &ProcessTarget,
    intent: ProcessIntent,
) -> Result<ProcessAdmission, ProcessError>;
```

`ProcessPlatformAdapter` 是 Module 内部的测试和平台接缝：

- `WindowsProcessAdapter` 完成真实枚举、打开、身份读取、存活检查和保护属性读取。
- `FakeProcessAdapter` 模拟目标变化、访问拒绝、保护状态、句柄生命周期和平台失败。

Windows HANDLE、Win32 错误码、平台结构和原始访问掩码不能越过 Module Interface。`ProcessAuthority` 持有不可变的当前 `hostInstanceId` 和 Adapter；调用者不能替换实例标识或直接调用 Adapter。

Stage 1 不把 Module 注册为 Tauri managed state，也不增加可由前端或插件调用的入口。真实 Windows 测试直接构造宿主内部 Module。Stage 2 在经过独立设计与计划验收后负责把同一个 Module 接入产品入口。

## 5. Domain Contract

### 5.1 Host Instance

`HostInstanceId` 是每次宿主生命周期生成的 128-bit 随机值。生产构造必须使用操作系统安全随机源；测试可以注入确定值。随机源失败时 Module 构造失败，不能退化为时间戳、PID 或固定值。

### 5.2 Process Target

`ProcessTarget` 是不可由普通调用者修改字段的值对象：

```text
ProcessTarget
├─ hostInstanceId
├─ pid
└─ creationTime
```

- `pid` 只定位候选进程，不能单独授权操作。
- Windows `creationTime` 使用 `GetProcessTimes` 返回的 64-bit `FILETIME` 值。
- 只有三项完全相同才表示同一目标。
- 名称、路径和父 PID 都是快照元数据，不参与目标身份等价判断。

### 5.3 Process Record

`ProcessRecord` 至少包含：

```text
target
pid
name
executablePath?
observedParentPid?
```

`observedParentPid` 只描述快照时观测结果。Stage 2 的进程树操作必须重新枚举并对展开后的每个具体目标执行最终准入，不能把旧父 PID 当作授权关系。

Windows Adapter 优先使用已打开进程对象查询到的完整可执行路径和 basename。若路径不可用，它必须在 HANDLE 仍打开时再次核对当前枚举名称，并在核对后重新确认原 HANDLE 的对象仍存活且创建时间不变；无法把展示元数据与该对象可靠对应时，省略该候选。

### 5.4 Process Snapshot

`ProcessSnapshot` 包含按确定规则排列的记录和只含计数的遗漏摘要：

```text
ProcessSnapshot
├─ processes: ProcessRecord[]
└─ omissions
   ├─ exitedOrChanged
   ├─ inspectionDenied
   └─ identityUnavailable
```

遗漏摘要不得包含 PID、路径、命令行或原始系统错误。目录刷新不创建新的授权 epoch；同一宿主内仍存活且创建时间不变的目标在刷新后继续有效。

快照创建失败、枚举器中途异常或无法确定遍历是否完整时返回 `CatalogUnavailable`，不发布部分记录。单个候选失败只增加对应遗漏计数。

## 6. Snapshot Flow

```text
ProcessAuthority.snapshot
  → ProcessPlatformAdapter creates one process enumeration snapshot
  → enumerate candidate PIDs
  → open each candidate with PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE
  → read creation time and current display metadata
  → reconcile metadata while the process object is pinned by the handle
  → append ProcessRecord or increment one omission category
  → sort records deterministically
  → close all temporary handles
  → return ProcessSnapshot
```

同一候选只能落入一个遗漏类别。分类优先级为：`exitedOrChanged`、`inspectionDenied`、`identityUnavailable`。全局枚举失败不计入遗漏摘要，而是返回错误。

`ProcessCatalog` 不缓存 HANDLE，也不承诺快照是系统全局原子视图。它承诺的是每条已返回记录在签发时具有可靠身份，且后续操作仍必须最终复核。

## 7. Stage 1 Admission Lease

### 7.1 Intent

Stage 1 定义：

```text
Terminate
ForceTerminate
Restart
```

进程树和同名批量是 Stage 2 的目标选择与展开方式，不是绕过单目标准入的新意图。每个展开目标必须携带自己的 `ProcessTarget` 并单独准入。

### 7.2 Admission Order

`admit(target, intent)` 按固定顺序执行：

```text
validate target structure
  → compare hostInstanceId
  → open one admission handle with PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE
  → compare creationTime on that process object
  → call WaitForSingleObject(handle, 0) to verify that the process object is nonsignaled
  → read critical and protection attributes
  → compare the exact current UiPilot host identity
  → apply protection policy
  → return ProcessAdmission owning the admission handle
```

创建时间不一致时必须返回 `TargetChanged`，不能自动为新进程签发目标或继续检查新进程是否允许操作。

Stage 1 的 `admit` 只证明目标身份、存活状态和当前保护策略，并把这一结论绑定到一个仍然打开的进程对象。它不声称已经取得未来操作所需权限，也不在本阶段打开第二个 HANDLE。

### 7.3 Least Access

Stage 1 的目录观察和 admission HANDLE 只申请 `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`。limited-query 用于身份与保护属性读取；`SYNCHRONIZE` 仅用于 `WaitForSingleObject(handle, 0)` 的无等待存活判断，不授予修改进程的能力。`ProcessIntent` 只绑定租约允许进入的后续操作种类，不改变 Stage 1 请求的 Windows 权限。

Stage 1 禁止 `PROCESS_ALL_ACCESS`、`SeDebugPrivilege` 和任何操作权限。Stage 2 只有在具体终止与重启算法通过独立设计验收后，才能逐项定义实际 Win32 调用、最小访问掩码和对应稳定错误。例如 `TerminateProcess` 需要 `PROCESS_TERMINATE`；新建替代进程不需要在旧目标中创建线程，因此 Restart 不得无理由申请 `PROCESS_CREATE_THREAD`。

Stage 2 若改变某个意图的执行算法，必须在同一阶段重新证明其访问掩码仍是该算法的最小集合。不得通过先申请宽权限来规避设计决策。

### 7.4 Process Admission

`ProcessAdmission`：

- 不实现 `Clone` 或序列化。
- 持有 Stage 1 身份、存活与保护策略复核使用的 observation HANDLE，其权限仅为 `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`。
- 绑定原始目标身份和单个 `ProcessIntent`。
- 只能由 `process_control` Module 内部的对应 Stage 2 操作准备流程消费一次。
- 不向 Module 外部暴露 PID 重开、原始 HANDLE 或访问掩码能力。
- Drop 时总是关闭句柄。

### 7.5 Stage 2 Handoff Contract

宿主确认必须发生在 Stage 2 的最终操作准入之前。确认结束后不得插入新的用户交互或无界等待。Stage 2 必须在 `ProcessAdmission` 的 HANDLE 仍然打开时，在 `process_control` Module 内执行以下不可拆分流程：

```text
consume ProcessAdmission
  → select observation access plus the minimum access mask from the approved operation algorithm
  → open one operation handle for the recorded PID
  → compare creationTime on the operation handle with ProcessTarget
  → repeat liveness, critical and protection checks on the operation handle
  → on any failure, close both handles and return a Stage 2 stable error
  → otherwise close the admission handle and immediately invoke the side-effect adapter with the operation handle
```

打开的 admission HANDLE 使旧进程对象在 Stage 2 交接期间仍可用于比较，但不能假设它阻止 Windows 在进程终止后复用 PID。安全性来自 operation HANDLE 上的完整 `creationTime` 和保护属性复核。若 PID 已复用，必须在任何副作用前返回目标变化错误。

Stage 2 必须为 operation HANDLE 打开被 ACL 拒绝、交接期间目标退出或改变、最终保护属性变化分别定义稳定结果，并明确哪些结果允许在用户确认后通过提权从头重试。这些错误名称和重试策略属于 Stage 2，不在 Stage 1 提前冻结。

Stage 1 和 Stage 2 的存活判断都使用 `WaitForSingleObject(handle, 0)`：`WAIT_TIMEOUT` 表示进程对象尚未 signal，`WAIT_OBJECT_0` 表示目标已退出，`WAIT_FAILED` 或对进程 HANDLE 不可能出现的其他结果在准入时返回 `InspectionDenied`，在目录中计入 `inspectionDenied`。不得使用 `GetExitCodeProcess == STILL_ACTIVE` 作为唯一存活判断，因为进程可以用数值 259 退出。

## 8. Protection Policy

准入拒绝：

- 当前 UiPilot 宿主进程的精确 `pid + creationTime` 身份。
- `IsProcessCritical` 返回 critical 的目标。
- `GetProcessInformation(ProcessProtectionLevelInfo)` 返回非 `PROTECTION_LEVEL_NONE` 的目标。
- 无法读取 critical 或 protection 属性的目标。
- PID 0 等不能表示普通可操作进程的特殊目标。

进程名称和路径不用于禁止策略，因为它们可变化、可重复且不能证明目标身份。本阶段不引入可配置禁止名单。

`InspectionDenied` 与 `ProtectedTarget` 不同。前者表示当前权限下无法建立安全结论，Stage 2 可以在用户明确同意提权后从头重新执行完整准入；后者表示宿主策略禁止，提权也不得绕过。

## 9. Stable Error Contract

| Code | Meaning | Caller action |
|---|---|---|
| `CatalogUnavailable` | 系统快照无法创建或完整遍历 | 可重试整个快照 |
| `InvalidTarget` | PID、实例标识或目标结构非法 | 终止请求 |
| `ExpiredTarget` | 目标来自其他宿主实例 | 重新获取快照 |
| `TargetExited` | 目标不存在或验证期间已退出 | 刷新快照 |
| `TargetChanged` | PID 的创建时间已经改变 | 刷新快照，不自动重定向 |
| `InspectionDenied` | 无法读取身份或保护属性 | 失败闭合；未来可显式提权后从头重试 |
| `ProtectedTarget` | UiPilot 自身、critical 或受保护进程 | 终止请求，禁止提权绕过 |
| `PlatformUnsupported` | 当前平台没有已验收 Adapter | 终止请求 |

准入错误优先级固定：

```text
target structure
→ host instance
→ open target
→ creation time
→ liveness
→ protection readability
→ protection policy
```

所有错误只向普通调用者和日志暴露稳定 code。原始 Win32 code 可以在受控诊断上下文中转换为分类，但普通日志不得包含可执行路径、命令行、用户数据、HANDLE 或原始系统错误。

所有 Stage 1 失败保证不返回 `ProcessAdmission`、不调用任何进程副作用，并在返回前关闭临时句柄。operation HANDLE 的打开和对应错误不属于本错误表；它们由第 7.5 节要求的 Stage 2 设计定义。

## 10. Concurrency And Security Invariants

- `hostInstanceId` 在 `ProcessAuthority` 生命周期内不可变。
- 快照签发和准入之间不共享可变目标注册表。
- PID 只定位，`pid + creationTime` 才识别目标。
- `ProcessAdmission` 保留原进程对象供 Stage 2 交接比较，但不假设 HANDLE 能阻止 PID 复用。
- Stage 2 只能在 admission HANDLE 仍打开时由 `process_control` 内部打开 operation HANDLE，并在任何副作用前比较完整身份和保护属性。
- operation HANDLE 命中新进程时返回目标变化；不能把租约转换成后来复用相同 PID 的进程。
- `ProcessAdmission` 不跨宿主重启、不序列化、不克隆且单次消费。
- 确认、提权或其他可能等待的流程必须在 Stage 2 最终操作准入之前完成；operation HANDLE 打开失败后若需要提权，必须关闭当前租约并从确认与准入链起点重新开始。
- Stage 2 的副作用必须紧跟 operation HANDLE 的最终复核，并使用该 operation HANDLE。
- 任何取消、拒绝、过期或最终检查失败都不得调用副作用 Adapter。
- overflow、随机源失败或无法分类的系统状态均失败闭合。

## 11. Windows Adapter

Windows 11 x64 是正式实现和验收基线。Adapter 使用公开 Win32 能力：

- Tool Help process snapshot 枚举候选 PID、名称和观测父 PID。
- `OpenProcess` 在 Stage 1 只以 `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` 打开候选。
- `QueryFullProcessImageNameW` 读取当前可执行路径。
- `GetProcessTimes` 读取创建时间。
- `WaitForSingleObject(handle, 0)` 对已打开进程对象执行无等待存活检查。
- `IsProcessCritical` 读取 critical 状态。
- `GetProcessInformation(ProcessProtectionLevelInfo)` 读取 Protected Process/PPL 状态。

Windows 10 22H2 只记录兼容性结果，不构成 Stage 1 正式支持门槛。任何目标若无法使用已验收的公开接口完成保护属性检查，必须返回 `InspectionDenied`，不能静默降级为允许。

Stage 1 不启用 `SeDebugPrivilege`，也不通过 PowerShell、WMI、Shell 命令或未记录的 NT 接口绕过访问控制。

## 12. Verification

### 12.1 Fake Adapter Contract Tests

至少覆盖：

- 正常快照、单项遗漏和全局枚举失败。
- 三种遗漏类别互斥且计数确定。
- 旧 `hostInstanceId` 返回 `ExpiredTarget`，且不打开 PID。
- 快照后 PID 复用返回 `TargetChanged`。
- 进程在枚举、打开或 Stage 1 准入检查阶段退出。
- 当前宿主、critical、Protected Process/PPL 和保护属性不可读。
- 多种错误并存时遵循固定优先级。
- 成功准入保留 observation HANDLE；随后 PID 复用不改变该 HANDLE 指向的原进程对象。
- 所有失败路径关闭临时 HANDLE，副作用调用计数为零。
- 目录和每个 intent 的 Stage 1 admission HANDLE 都精确申请 limited-query 与 synchronize，且从不申请操作权限或 all-access。
- 存活检查把 `WAIT_TIMEOUT`、`WAIT_OBJECT_0`、`WAIT_FAILED` 映射为确定结果，并覆盖“进程以 259 退出”的回归场景。
- 刷新目录不会使仍存活的同一身份失效。
- 快照顺序和遗漏摘要可重复。
- `ProcessAdmission` Drop 关闭 HANDLE，消费后不能再次使用。
- 错误显示和普通日志不包含路径、原始系统错误或 HANDLE。

### 12.2 Windows Tests

自动或显式平台测试只执行读取与准入：

- 快照识别当前测试进程和受控临时辅助进程。
- 当前宿主目标返回 `ProtectedTarget`。
- 修改创建时间后返回 `TargetChanged`。
- 辅助进程存活时可获得 `ProcessAdmission`；在测试夹具仍持有其原生子进程 HANDLE 的情况下让进程退出，旧目标稳定返回 `TargetExited`。
- 当前测试进程的 critical/PPL 查询成功并报告未保护。
- 如果验收机存在可检查的受保护目标，它返回 `ProtectedTarget`。
- 测试结束后没有残留辅助进程或 HANDLE。

真实 Windows 测试不能调用本 Module 之外的终止实现来证明产品能力。测试清理辅助进程属于测试夹具清理，必须限定为测试自己创建并持有的子进程。

### 12.3 Manual Acceptance

在 Windows 11 x64 标准用户会话记录：

- 精确 OS build 和运行权限。
- 快照成功、返回当前宿主和普通辅助进程，并产生合理的遗漏统计。
- 当前宿主稳定拒绝。
- 辅助进程退出后旧目标稳定失效。
- 普通日志脱敏。
- 无残留辅助进程和句柄。

若机器没有可检查的 Protected Process/PPL，真实保护目标项目记录为环境不适用；Fake Adapter 的保护合同测试仍是强制门槛。

## 13. Exit Gate

Stage 1 只有同时满足以下条件才可申请阶段验收：

- 本设计和后续独立实施计划均已验收。
- `ProcessCatalog` 与 `admit` 的 Fake Adapter 合同测试通过。
- Windows 11 真实只读/准入验证通过。
- PID 复用、目标退出、UiPilot 自身和保护目标具有确定结果。
- 所有失败路径无进程副作用且无句柄泄漏。
- 没有 Tauri command、前端、公共插件合同、终止、重启、提权或确认 UI。
- Public Plugin API v1、Host version、SDK、Schema 和 CLI 保持不变。
- 当前阶段改动和已知限制已记录。

Stage 1 人工验收前不得编写 Stage 2 的详细设计、实施计划或产品代码。

## 14. Official Windows References

- [CreateToolhelp32Snapshot](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot)
- [OpenProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess)
- [Process Handles and Identifiers](https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers)
- [Process Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)
- [GetExitCodeProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess)
- [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)
- [QueryFullProcessImageNameW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew)
- [GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
- [IsProcessCritical](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocesscritical)
- [GetProcessInformation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocessinformation)
- [PROCESS_PROTECTION_LEVEL_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_protection_level_information)
- [TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)
- [CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)

## 15. Approval And Next Step

本文档验收后，Stage 1 的“设计规格验收”门结束。下一步只能读取仓库的实施计划规范并为 Stage 1 编写独立实施计划。

在实施计划再次获得用户验收前，不开发 Stage 1 产品代码；在 Stage 1 开发、自动验证、人工验收和阶段验收全部完成前，不进入 Stage 2。
