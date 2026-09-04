# Stage 2B Process Target Expansion And Batch Orchestration Design

**Date:** 2026-09-04
**Status:** Approved — accepted by the user on 2026-09-04

**Related:**

- [Raycast-Inspired Plugin Platform Roadmap](./2026-09-02-raycast-inspired-plugin-platform-roadmap-design.md)
- [Stage 1 Process Identity And Operation Admission](./2026-09-02-process-identity-operation-admission-design.md)
- [Stage 2A Single-Target Process Operation Kernel](./2026-09-03-single-target-process-operation-kernel-design.md)
- [Stage 2A Verification Record](../verification/2026-09-03-single-target-process-operation-kernel.md)
- [Raycast Kill Process 与 UiPilot 功能差异研究](../../research/2026-09-02-raycast-kill-process-uipilot-comparison.md)

## 1. Goal

在已验收的 Stage 1 进程身份准入和 Stage 2A 单目标操作 Kernel 之上，建立宿主拥有的目标展开与批量编排 Module。该 Module 将一个精确根目标展开为进程树或同名进程集合，在宿主确认前冻结集合，并在确认后逐个复用现有准入与单目标操作。

Stage 2B 必须同时解决：

- 批量操作不能沿裸 PID 追踪或误杀确认后出现的新进程；
- 进程树必须以后代优先的确定顺序执行；
- 同名批量必须遵循平台名称等价语义，不按路径静默收窄；
- 不可回滚的部分完成必须通过逐目标报告表达；
- 单目标失败不能隐藏其他目标结果；
- 批次规模和宿主控制的等待时间必须有界；
- 任何目标仍需独立完成 Stage 1 和 Stage 2A 的完整安全检查。

本设计参考 Raycast Kill Process 的用户能力，但不采用其直接委托 taskkill /T 或 Stop-Process -Name 的宽泛执行模型。UiPilot 先生成精确身份集合，再逐目标执行宿主控制的操作。

## 2. Scope

### 2.1 Included

- Windows 11 x64 宿主内部的进程树展开。
- Windows 11 x64 宿主内部的同名进程展开。
- 普通终止与强制终止两种批次操作。
- 确认前生成不透明、只读、无 HANDLE 的冻结计划。
- 进程树最深后代优先、根目标最后的串行顺序。
- 同名集合按平台名称等价语义匹配。
- 每批最多 256 个可靠精确目标。
- 30 秒单调时钟调度期限。
- best-effort 逐目标执行和完整有序报告。
- 范围内遗漏、未尝试项和部分完成的稳定语义。
- Fake Adapter 确定性验证和仅针对测试自有 fixture 的真实 Windows 验收。

### 2.2 Excluded

- Restart 与 Force Restart。
- Windows elevation broker、UAC、SeDebugPrivilege 或提权重试。
- Tauri command、managed state、启动器结果、确认 UI、Toast 或设置项。
- Public Plugin API、Manifest、Schema、SDK、CLI、Runtime 或 AI Tool。
- 批次持久化、计划恢复、操作历史、全局任务表或后台任务。
- 确认后动态吸收新出现的同名进程或后代。
- 跨批次去重、互斥、事务或回滚。
- 并行批次执行。
- macOS Adapter。
- 通用 Shell、taskkill、PowerShell、WMI、Job Object 或未记录的 NT API。

## 3. Approved Product Decisions

以下决策由用户在 2026-09-04 逐项确认：

| Topic | Decision |
|---|---|
| Target-set timing | 确认前冻结；确认后新增目标不进入批次 |
| Partial failure | 继续其余目标并返回完整逐目标结果 |
| Tree order | 最深后代优先，根目标最后 |
| Same-name semantics | 仅按平台等价的进程名匹配，不按路径过滤 |
| Cardinality | 最多 256 个可靠目标；超出整体拒绝，不截断 |
| Batch timing | 30 秒调度期限；已进入 Stage 2A 的目标保留完整五秒期限 |
| Plan lifetime | 当前宿主实例内单次消费、仅内存、不设墙钟 TTL |
| Top-level result | 开始前失败返回顶层错误；开始后返回完整报告 |
| Already exited | 单独记录 AlreadyExited；状态已满足但不算宿主操作成功 |
| Interface shape | 不透明两阶段 BatchPlan，由 Module 内部编排 |

## 4. Core Invariants

1. PID 只定位候选；ProcessTarget 中的 hostInstanceId、pid 和 creationTime 才识别目标。
2. ProcessBatchPlan 中的目标集合是唯一获得确认的集合。
3. execute_batch 禁止重新展开目标或跟随新的 PID、父子关系或名称匹配结果。
4. 每个可靠目标只能来自同一次规划快照中的完整身份观察。
5. 每个目标在副作用前独立执行 Stage 1 准入和 Stage 2A 最终操作准入。
6. ProcessBatchPlan 不持有 admission HANDLE、operation HANDLE、snapshot HANDLE 或平台 token。
7. 调用者不能修改、重排、克隆、序列化或重复消费计划。
8. 任何目标失败都不能撤销已完成操作，也不能阻止期限内的后续目标。
9. 一旦进入逐目标执行阶段，顶层错误不得掩盖可能发生的部分副作用。
10. 所有普通日志和稳定结果默认脱敏。

## 5. Module Architecture

Stage 2B 深化现有 process_control Module，不建立第二条进程执行通道。

### 5.1 Existing Modules

- core.rs 继续拥有 HostInstanceId、ProcessTarget、ProcessRecord、进程快照、Stage 1 准入和保护策略。
- operation.rs 继续独占 Stage 2A 的最终准入、温和关闭、强制终止、五秒期限和稳定操作错误。
- windows.rs 继续作为 Win32 Adapter，提供只读枚举、身份观察、名称等价和单目标操作原语。
- mod.rs 继续作为唯一 crate-private facade。

### 5.2 New Batch Module

新增 batch.rs，独占：

- 批次值对象和稳定错误；
- 规划快照到冻结计划的转换；
- 进程树图验证和深度排序；
- 同名匹配；
- 256 目标上限；
- 30 秒调度期限；
- Stage 1 与 Stage 2A 的逐目标编排；
- 逐目标 outcome 与批次报告聚合。

batch.rs 不直接调用 Win32，不复制 operation.rs 的最终准入或副作用算法。删除 batch.rs 后，目标展开、排序、期限和部分结果复杂度会重新散落到所有调用者；因此该 Module 有实际深度。

### 5.3 Internal Expansion Snapshot

普通 ProcessSnapshot 只暴露可靠 ProcessRecord 和全局聚合遗漏，无法判断遗漏是否可能属于某棵树或某个名称集合。Stage 2B 在 process_control 内部增加 ExpansionSnapshot：

- 每次规划只进行一次完整平台枚举。
- 每个原始候选保留仅供内部归因使用的 pid、name hint 和 observedParentPid。
- 成功观察的候选同时携带可靠 ProcessRecord。
- 失败候选携带 Stage 1 的遗漏分类，不会生成 ProcessTarget。
- 原始 hint 永不越过 Module Interface，不能出现在 preview、错误或日志中。

该结构是 core.rs 与 batch.rs 之间的内部 seam，不是新的调用者 Interface。

## 6. Crate-Private Interface

建议的 facade Interface：

~~~rust
pub(crate) enum ProcessBatchScope {
    Tree,
    SameName,
}

pub(crate) enum ProcessBatchOperation {
    Terminate,
    ForceTerminate,
}

impl ProcessAuthority {
    pub(crate) fn plan_batch(
        &self,
        root: &ProcessTarget,
        scope: ProcessBatchScope,
        operation: ProcessBatchOperation,
    ) -> Result<ProcessBatchPlan, ProcessBatchError>;

    pub(crate) fn execute_batch(
        &self,
        plan: ProcessBatchPlan,
    ) -> Result<ProcessBatchReport, ProcessBatchError>;
}
~~~

ProcessBatchOperation 映射到现有 ProcessIntent：

| Batch operation | Stage 1 intent | Stage 2A method |
|---|---|---|
| Terminate | Terminate | terminate |
| ForceTerminate | ForceTerminate | force_terminate |

Restart 不能构造为 ProcessBatchOperation，因此不会产生运行时 Restart 分支或 IntentMismatch 调用路径。

### 6.1 ProcessBatchPlan

ProcessBatchPlan：

- 绑定创建它的 HostInstanceId；
- 绑定 ProcessBatchScope 和 ProcessBatchOperation；
- 持有按执行顺序冻结的内部目标；
- 持有确认所需 preview 和 ProcessBatchOmissions；
- 不实现 Clone、Copy、Serialize 或 Deserialize；
- 不写入磁盘或全局注册表；
- 不包含 HANDLE 或可执行回调；
- 只能按值交给 execute_batch 一次。

不设置墙钟 TTL。计划等待较久不会扩大授权集合；目标退出或 PID 复用由执行时的精确准入稳定处理。

### 6.2 Read-Only Preview

调用者可以只读访问：

- scope；
- operation；
- 有序目标数量；
- 每个目标的计划内 ordinal、展示用 pid、冻结 name、可选 executablePath 和树 depth；
- 聚合遗漏计数；
- completeness 提示。

精确 ProcessTarget 只保留在不透明计划内部，不通过 preview 交给调用者。ordinal、pid、name、path 和 depth 只服务于确认展示与报告关联，不构成可执行目标。preview 不暴露 creationTime、HostInstanceId、HWND、HANDLE、原始候选 PID、未验证名称、原始 Win32 code 或可变目标集合。

树目标 depth 中根为 0；同名目标没有 depth。执行顺序已经固化在计划中，调用者展示时可以分组，但不能改变执行顺序。

## 7. Planning Flow

### 7.1 Root Preflight

plan_batch 按固定顺序执行：

~~~text
validate root structure
  → validate root HostInstanceId
  → map ProcessBatchOperation to ProcessIntent
  → create one temporary Stage 1 admission for the root
  → while the read-only admission HANDLE remains open, take one ExpansionSnapshot
  → correlate the exact root in that snapshot
  → expand and validate the requested scope
  → freeze previews, order, operation, scope, omissions, and authority identity
  → drop the temporary admission HANDLE
  → return a handle-free ProcessBatchPlan
~~~

临时 admission 是 Stage 2 操作准备的一部分：

- 它只使用 Stage 1 已批准的 observation access。
- 它在确认前验证根目标仍是同一对象且不属于禁止目标。
- 它不跨越宿主确认。
- 它不会被后续 execute_batch 复用。
- 它不是 operation HANDLE，且不产生进程副作用。

### 7.2 Root Correlation And Error Priority

根目标关联优先级：

1. 无效结构返回 InvalidTarget。
2. HostInstanceId 不匹配返回 ExpiredTarget。
3. Stage 1 准入按既有优先级返回 TargetExited、TargetChanged、InspectionDenied、ProtectedTarget 或 PlatformUnsupported。
4. 平台快照创建或遍历失败返回 CatalogUnavailable，不返回部分计划。
5. 快照中同 PID 的可靠对象具有不同 creationTime 时返回 TargetChanged。
6. 根候选存在但无法形成可靠记录时返回对应的 InspectionDenied。
7. 根 PID 不在快照时，复查临时 admission HANDLE：已退出返回 TargetExited，观察失败返回 InspectionDenied，仍存活但目录缺失返回 CatalogUnavailable。

只有可靠的精确根记录可以进入目标展开。根目标失败属于顶层规划错误，不降级成只有遗漏的计划。

### 7.3 Candidate Normalization

ExpansionSnapshot 首先进行候选归一化：

- PID 0 无法形成目标。
- 非法或空名称无法形成可靠名称匹配。
- 同一快照内重复 PID 视为身份歧义；该 PID 的所有候选均不得进入可靠目标集合。
- 中途枚举失败使整个规划返回 CatalogUnavailable。
- 所有观察临时 HANDLE 在进入下一个候选目标前关闭一次。

可靠 ProcessRecord 继续使用 Stage 1 的完整 creationTime、liveness、名称与路径复核。Stage 2B 不建立弱化版目录。

## 8. Tree Expansion

Windows PROCESSENTRY32.th32ParentProcessID 只表示快照记录的创建者 PID，不是稳定对象身份。Stage 2B 将它限制为同一 ExpansionSnapshot 内的关系线索。

### 8.1 Valid Tree Edge

从父记录到子记录的边只有同时满足以下条件才可靠：

- 父、子候选 PID 均唯一且非零；
- 父、子均形成可靠 ProcessTarget；
- 子候选的 observedParentPid 等于父目标 pid；
- 父 creationTime 不晚于子 creationTime；
- 从根到该子节点的整条祖先链均满足同样条件；
- 加入该边不会形成自环或循环。

creationTime 比较只消除明显不可能的父子关系，不把父 PID 升级为授权身份。执行时仍只授权每个冻结 ProcessTarget 自身。

### 8.2 Unverified Branches

- 直接位于可靠树下但自身观察失败的候选按其 Stage 1 原因计入遗漏。
- 位于不可靠、歧义或缺失祖先之后的候选不能证明完整祖先链，计入 ancestryUnverified。
- creationTime 顺序无效、自环、循环或重复 PID 影响的分支计入 ancestryUnverified。
- 被阻断分支中的可靠进程也不能越过不可靠祖先重新接到根上。
- 每个候选最多计入一个遗漏分类。

遗漏不会生成 ProcessTarget，不会在执行时通过裸 PID 再尝试。

### 8.3 Order

目标集合始终包含根目标。可靠后代计算固定 depth 后按以下键排序：

~~~text
depth descending
→ pid ascending
→ creationTime ascending
~~~

因此最深后代先执行，根目标最后执行。同一输入快照必须产生相同顺序。

## 9. Same-Name Expansion

同名批量从快照中的精确根记录取得冻结名称，并调用现有平台 Adapter 的进程名称等价语义。

规则：

- 不接受调用者提供的任意名称字符串。
- 所有可靠且与根名称平台等价的记录进入集合。
- 可执行路径不参与匹配；不同目录中的同名进程会进入确认集合。
- 根目标始终进入集合。
- 具有等价名称 hint 但无法形成可靠 ProcessTarget 的候选按真实观察失败计入遗漏。
- 名称无效或无法可靠比较的候选计入 identityUnavailable，因为无法证明它不属于所选名称集合。
- 不同名称候选不进入集合，也不计为范围内遗漏。

同名目标按以下键排序：

~~~text
pid ascending
→ creationTime ascending
~~~

规划后启动的新同名进程不在计划内，execute_batch 禁止再次按名称枚举。

## 10. Cardinality And Completeness

可靠目标数量上限固定为 256：

- 1 至 256 个可靠目标可以生成计划。
- 第 257 个可靠目标使规划返回 TargetSetTooLarge。
- 超限时不截断、不返回 preview、不产生副作用。
- 遗漏计数不占用 256 个可靠目标配额。
- 计数溢出返回 CatalogUnavailable；不使用饱和后继续。

ProcessBatchOmissions 至少包含：

~~~rust
pub(crate) struct ProcessBatchOmissions {
    exited_or_changed: u64,
    inspection_denied: u64,
    identity_unavailable: u64,
    ancestry_unverified: u64,
}
~~~

completeness 由 Module 计算：

- omissions.total() == 0 表示 CompleteSelection。
- omissions.total() > 0 表示 IncompleteSelection。

IncompleteSelection 仍可交给宿主确认并执行可靠目标，但调用者不得把它描述为完整的树或完整的同名集合。

## 11. Confirmation Handoff

Stage 2B 不实现 UI，但冻结以下调用顺序：

~~~text
selected ProcessTarget
  → plan_batch
  → caller renders the exact frozen preview and completeness
  → host-owned confirmation
  → execute_batch consumes that same ProcessBatchPlan
~~~

确认后禁止：

- 调用 plan_batch 生成替代计划并直接执行；
- 修改 scope 或 operation；
- 增删、替换或重排目标；
- 将计划保存到磁盘、跨宿主重启恢复或转交公共插件；
- 在 execute_batch 内插入第二次确认。

后续宿主产品入口必须把确认显示的计划对象本身交还 execute_batch。跳过确认偏好、确认文案和 UI 状态属于后续产品入口设计。

## 12. Execution Flow

execute_batch 先消费计划并验证其 HostInstanceId 与当前 ProcessAuthority 一致。ExpiredPlan、PlatformUnsupported 或其他顶层验证失败发生在任何逐目标处理之前，副作用为零。

验证通过后：

~~~text
start a 30-second monotonic scheduling deadline
  → for each frozen target in plan order
      → if deadline reached, mark current and remaining targets NotAttemptedBatchDeadline
      → Stage 1 admit(exact target, bound intent)
      → on admission failure, append its normalized outcome and continue
      → check the batch deadline again
      → if reached, drop admission and mark current and remaining targets NotAttemptedBatchDeadline
      → immediately call the matching Stage 2A method
      → append the exact normalized outcome
  → derive summary and completion
  → return ProcessBatchReport
~~~

### 12.1 Serial Best-Effort

- 始终串行，任一时刻最多有一个目标进入操作链。
- 一个目标失败后，只要调度期限未到，继续下一目标。
- 子目标失败不阻止后续子目标或根目标。
- 不回滚已经完成的终止。
- 不重试失败目标。
- 不自动把普通终止升级为强制终止。
- 不自动提权。
- 不因相邻目标名称、路径或父 PID 相同而复用准入结果。

### 12.2 Thirty-Second Scheduling Deadline

- deadline 从计划所属 Authority 验证成功后开始。
- 使用单调时钟，不使用墙钟。
- 每个目标在 Stage 1 准入前和成功准入后各检查一次。
- 准入失败即使跨过 deadline，也保留已观察到的准入结果；下一目标在期限检查处停止。
- 准入成功但第二次检查已到期时，关闭 admission HANDLE，当前和剩余目标标记为未尝试。
- 一旦进入 Stage 2A，目标保留 Stage 2A 已验收的完整五秒期限。
- Stage 2B 不向 operation.rs 注入更短 deadline，不改变单目标错误语义。

因此协议控制的等待预算最多为 30 秒调度时间加一个已开始目标的五秒 Stage 2A 预算。同步 Win32 调用本身仍属于平台调用时间，不被虚假描述为可抢占的硬实时期限。

## 13. Target Outcomes And Report

### 13.1 Target Outcome

~~~rust
pub(crate) enum ProcessBatchTargetOutcome {
    ObservedExited,
    AlreadyExited,
    Failed(ProcessBatchTargetError),
    NotAttemptedBatchDeadline,
}
~~~

- ObservedExited：Stage 2A 返回 Ok；精确 operation HANDLE 已观察到目标退出。该结果不额外声称一定由本次副作用导致，因为目标可能在最终准入后并发自行退出。
- AlreadyExited：Stage 1 或 Stage 2A 最终准入返回 TargetExited。目标状态已经满足，但不计为宿主操作成功。
- Failed：保留稳定失败分类和副作用后的不确定状态。
- NotAttemptedBatchDeadline：当前目标没有进入 Stage 2A；不得报告为成功或操作失败。

### 13.2 Stable Target Errors

ProcessBatchTargetError 至少包含：

| Code | Meaning | Side-effect interpretation |
|---|---|---|
| TargetChanged | PID 已指向其他 creationTime | 无副作用 |
| InspectionDenied | 无法安全确认身份、存活或保护状态 | 无副作用 |
| ProtectedTarget | UiPilot 自身、critical 或 PPL | 无副作用；禁止提权绕过 |
| OperationAccessDenied | 最小操作权限或窗口消息被拒绝 | 无已成功发起的进程副作用 |
| GracefulCloseUnavailable | 没有合格 GUI 顶层窗口 | 无关闭消息成功 |
| GracefulCloseTimedOut | 已请求关闭但期限内未确认退出 | 副作用已发生 |
| OperationUnavailable | 平台操作不可用 | 保留 Stage 2A 精确语义 |
| TerminationPending | 强制终止已发起但期限内未确认退出 | 副作用已发生 |
| CompletionUnknown | 副作用后无法可靠观察完成状态 | 副作用可能已发生 |

TargetExited 统一变为 AlreadyExited。由不透明计划和绑定 operation 保证 InvalidTarget、ExpiredTarget 与 IntentMismatch 在逐目标执行中不可构造；内部测试必须锁定该不变量。未来回归若触发不可达的 IntentMismatch，按 OperationUnavailable 记录且不重试，不能返回会掩盖先前结果的顶层错误。

### 13.3 ProcessBatchReport

报告保持与计划相同的顺序，并包含：

- scope 和 operation；
- 每个冻结 preview 与对应 outcome；
- 原计划 ProcessBatchOmissions；
- observedExited、alreadyExited、failed、notAttempted 四类精确数量；
- Complete 或 Partial 的派生 completion。

Complete 只有在以下条件同时满足时成立：

- 每个可靠目标都是 ObservedExited 或 AlreadyExited；
- failed == 0；
- notAttempted == 0；
- omissions.total() == 0。

其他所有组合都是 Partial。Partial 不是顶层 Err；调用者必须读取报告。

## 14. Top-Level Error Contract

ProcessBatchError 只覆盖批量执行可能开始前的失败：

| Code | Meaning | Phase |
|---|---|---|
| InvalidTarget | 根目标结构无效 | plan |
| ExpiredTarget | 根目标不属于当前宿主实例 | plan |
| TargetExited | 规划时根目标已退出 | plan |
| TargetChanged | 根 PID 已被其他进程复用 | plan |
| InspectionDenied | 无法可靠规划根目标 | plan |
| ProtectedTarget | 根目标受宿主策略保护 | plan |
| CatalogUnavailable | 无法取得完整规划快照或关联可靠根记录 | plan |
| TargetSetTooLarge | 可靠目标超过 256 | plan |
| ExpiredPlan | 计划不属于执行它的 Authority | execute preflight |
| PlatformUnsupported | 当前平台没有已验收 Adapter | plan or execute preflight |

Display 只输出表中的稳定 code。错误不包含 PID、名称、路径、父 PID、HANDLE、原始 Win32 code 或内部异常文本。

进入逐目标循环后，所有预期平台和目标失败都必须归入 ProcessBatchReport。不得在已经可能产生副作用后返回 ProcessBatchError。

## 15. Handle And Resource Lifetime

### 15.1 Planning

- 一个临时根 admission HANDLE 保持到 ExpansionSnapshot 和根关联完成。
- 每个候选 observation HANDLE 只在观察该候选时存在并关闭一次。
- snapshot HANDLE 由 Windows Adapter RAII 封装并关闭一次。
- 规划成功或失败都不得把任何 HANDLE 放入 ProcessBatchPlan。

### 15.2 Execution

- 每次只为当前目标创建一个 Stage 1 admission。
- Stage 2A 最终准入期间最多同时持有当前目标的 admission HANDLE 与 operation HANDLE。
- admission HANDLE 在进入单目标副作用前关闭。
- operation HANDLE 保持到该目标完成、失败或 unwind 后关闭一次。
- 进入下一目标前，前一目标的所有 HANDLE 必须已释放。
- 正常、失败、deadline 和 unwind 路径都要求精确一次关闭。

最多 256 个计划目标只增加普通值对象，不导致同时持有 256 个进程 HANDLE。

## 16. Concurrency And Retry Policy

- ProcessBatchPlan 本身无全局注册，不引入全局互斥。
- 多个计划可以并发存在，但每个只能消费一次。
- 重叠计划可以包含同一 ProcessTarget；每次执行仍独立准入。
- 一个批次先终止目标后，另一批次可得到 AlreadyExited、TargetExited 或 Stage 2A 已定义的并发退出结果。
- 不在 Stage 2B 中跨批次去重、取消或协调。
- 通用取消、请求所有权和异步生命周期属于路线图后续 Runtime 阶段。
- 后续产品入口不得在 UI 线程同步运行 execute_batch。

## 17. Elevation Handoff

Stage 2B 不调用 elevation broker。

未来只有副作用前的 InspectionDenied 或 OperationAccessDenied 可以成为显式提权候选。提权必须：

1. 丢弃整个旧 ProcessBatchPlan 和所有结果外状态；
2. 结束当前调用；
3. 在新权限上下文重新规划；
4. 向用户展示新的精确集合、遗漏和操作；
5. 重新确认；
6. 从 Stage 1 准入重新执行。

禁止只对旧报告中的失败目标提权重试，因为目标集合、名称匹配和树关系可能已经变化。ProtectedTarget、GracefulCloseTimedOut、TerminationPending 与 CompletionUnknown 均不得盲目提权或重试。

## 18. Logging And Data Handling

- 普通日志只允许稳定 code、scope、operation 和聚合数量。
- 禁止记录 PID、creationTime、名称、路径、父 PID、树边、HANDLE、HWND、窗口标题、原始 Win32 code 或命令行。
- preview 中的名称和路径只在内存中供未来宿主确认使用。
- 未验证候选 hint 不得进入 preview、报告或日志。
- Stage 2B 不写磁盘、不保留历史、不创建遥测。
- 测试失败输出也应使用 fixture 标签或稳定事件，不输出真实系统进程信息。

## 19. Windows Adapter

Windows 11 x64 是正式实现和验收基线。

复用的公开 Win32 能力：

- CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)；
- Process32FirstW / Process32NextW；
- PROCESSENTRY32W.th32ProcessID、th32ParentProcessID 和 szExeFile；
- 已有 observation HANDLE 与 GetProcessTimes creationTime；
- 已有名称等价比较；
- Stage 2A 已验收的 EnumWindows、PostMessageW、TerminateProcess 和 WaitForSingleObject 路径。

Adapter 约束：

- snapshot HANDLE 不可继承并由 RAII 关闭。
- Process32 遍历中途失败使规划原子失败。
- 不增加 PROCESS_ALL_ACCESS、PROCESS_CREATE_THREAD 或批量专用访问掩码。
- 不使用 taskkill、PowerShell、WMI、Shell、Job Object 或未记录的 NT 接口。
- 非 Windows facade 返回 PlatformUnsupported，不假装空计划成功。

## 20. Deterministic Verification

Fake Adapter 与 Fake Monotonic Clock 必须覆盖：

### 20.1 Planning

- 根目标结构无效、宿主过期、退出、PID 复用、检查拒绝和受保护。
- 快照创建失败、中途失败和根关联缺失。
- 根临时 admission 在快照期间保持，返回前关闭一次。
- 树的根、子、孙深度与最深优先顺序。
- 同深度 pid + creationTime 稳定排序。
- 父 PID 复用、父创建时间晚于子、自环、循环、重复 PID 和缺失父节点。
- 不可靠祖先后的可靠候选仍计 ancestryUnverified，不越过祖先接树。
- 同名比较使用 Adapter 等价语义，不按路径过滤。
- 同名候选观察失败的范围内遗漏。
- 规划后的任何目录变化都不能修改计划；新增候选不会进入执行集合，已冻结目标的名称或父关系变化也不会移除该目标，执行资格只由其精确身份准入决定。
- 256 目标成功；257 目标 TargetSetTooLarge 且无计划、副作用为零。
- preview 只含可靠冻结数据；未验证 hint 不泄漏。
- 计划不实现 Clone 或序列化，不持有 HANDLE。

### 20.2 Execution

- 跨 Authority 计划返回 ExpiredPlan，调用次数为零。
- Tree 与 SameName 的精确执行顺序。
- 每个目标独立 Stage 1 和 Stage 2A 准入。
- 单个 TargetChanged、InspectionDenied、ProtectedTarget 或操作失败后继续。
- Stage 1 和 Stage 2A TargetExited 都映射 AlreadyExited。
- Stage 2A Ok 映射 ObservedExited。
- 副作用后的 timeout、pending 和 unknown 保留原分类。
- 30 秒期限在 admission 前到期。
- admission 成功后期限到期时 admission 关闭，当前与剩余目标未尝试。
- admission 失败跨过期限时保留该失败，下一目标停止。
- 已进入 Stage 2A 的目标获得完整五秒预算。
- Complete 与 Partial 的所有边界组合。
- 无回滚、重试、自动强制终止或提权。
- 正常、失败、deadline 与 catch_unwind 路径 HANDLE 恰好关闭一次。

## 21. Real Windows Verification

真实副作用只能作用于当前测试创建并明确控制的 fixture。禁止发现任意现有进程后对其执行终止。

测试资产必须满足：

- 控制器通过显式握手取得每个 fixture 的 PID 与 creationTime。
- 控制器为树中每个节点保留可用于有界清理的精确 HANDLE。
- 同名测试使用唯一的临时 fixture 可执行文件名；副作用前确认系统中该名称的每个可靠匹配都属于本测试，否则中止测试。
- Tree fixture 建立受控根、子、孙关系并报告全部节点。
- 所有真实操作串行运行。
- 每个 Child 或等价控制对象在正常、失败和 unwind 中有界清理。

真实场景至少覆盖：

1. 温和关闭进程树并观察后代优先、根最后。
2. 强制终止进程树。
3. 温和关闭多个同名 fixture。
4. 强制终止多个同名 fixture。
5. 规划后启动一个新的同名或后代 fixture；它必须保持存活且不在报告中。
6. 一个 fixture 忽略关闭、其他 fixture 正常退出；报告保持完整且继续执行。
7. 标准用户下的访问分类和错误脱敏。
8. 测试结束后 fixture 进程数与相关 HANDLE 残留为零。

目标上限、精确 30 秒边界、PID 复用、循环和受保护目标主要由 Fake 确定性测试证明。真实 PPL 目标不可安全取得时允许记录 N/A。

## 22. Regression And Evidence

实施验收必须记录：

- rustfmt；
- Stage 2B planning、tree、same-name、execution 和 deadline focused suites；
- 完整 process_control suite；
- 默认完整 Cargo tests；
- all-features tests；
- 默认与 all-features Clippy；
- Cargo.toml 与 Cargo.lock diff；
- Windows 版本、架构、提升状态、Rust 和 Cargo 版本；
- 真实 fixture 数量、清理结果和标准用户证据；
- 普通日志脱敏扫描；
- 最终 changed-file list；
- 无 Restart、elevation、Tauri、前端、公共插件、持久化或 macOS 变化。

若仓库级 all-features 或 Clippy 仍有 Stage 1/2A 已验收偏差，必须逐项对比并如实保持红色。Stage 2B 不得新增 process_control 诊断，也不得把继承失败写成通过。

## 23. Exit Gate

Stage 2B 只有同时满足以下条件才可申请正式验收：

- 本设计规格和独立实施计划分别获得用户验收；
- 两阶段不透明计划 Interface 与冻结集合合同实现；
- 树与同名展开仅生成可靠 ProcessTarget；
- 256 上限、30 秒调度期限和 best-effort 报告通过确定性测试；
- 所有目标复用 Stage 1 与 Stage 2A，不存在替代副作用通道；
- Windows 真实副作用只作用于测试自有 fixture；
- 所有 HANDLE 与 fixture 在所有路径清理；
- 零新增 process_control Clippy 诊断；
- 已知平台限制、遗漏和继承偏差完整记录；
- 独立审核没有未解决的 Critical 或 Important 问题；
- 用户审阅验证记录并明确接受 Stage 2B。

## 24. Known Limits

- Windows 父 PID 不是稳定对象身份；本设计通过同快照精确目标、creationTime 顺序和逐目标最终准入缩小风险，但不声称持有可由 Windows 验证的父子对象关系。
- 确认后父子关系或进程名称可以变化；执行仍只针对冻结身份集合。
- IncompleteSelection 不能保证覆盖所有实际后代或同名进程。
- 串行温和关闭可能在 30 秒后留下未尝试目标；报告必须明确列出。
- Stage 2B 不提供取消、后台任务或跨调用恢复。
- Stage 2A 已记录的 HWND/PID residual race 和 TerminateProcess 异步完成边界保持不变。

## 25. Official References

- [Raycast Kill Process platform implementation](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts)
- [Raycast Kill Process execution layer](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process.ts)
- [Raycast Kill Process UI flow](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx)
- [PROCESSENTRY32 structure](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/ns-tlhelp32-processentry32)
- [CreateToolhelp32Snapshot](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot)
- [Snapshots of the System](https://learn.microsoft.com/en-us/windows/win32/toolhelp/snapshots-of-the-system)
- [Process32First](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-process32first)
- [Process32Next](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-process32next)
- [GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
- [Process Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)
- [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)

## 26. Approval And Next Step

本文档获得用户书面验收后，Stage 2B 设计门结束。下一步只能读取仓库实施计划规范，并为 Stage 2B 编写独立实施计划。

实施计划再次获得用户验收前，不开发 Stage 2B 产品代码。Stage 2B 开发、验证和正式验收完成前，不设计或开发 Restart、elevation broker、宿主产品入口或 Stage 3。
