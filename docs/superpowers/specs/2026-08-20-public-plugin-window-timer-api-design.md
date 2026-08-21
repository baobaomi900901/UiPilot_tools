# 公开插件窗口计时 API 设计

> **增量覆盖通知（2026-08-21）：** 本文关于 AudioTicket 的播放 admission、单次/循环音频、每票独立
> 播放或停止、多 Timer 声音合并、主窗口焦点确认和原生音频失败处理的合同，已由
> [Windows 原生提醒协调器设计](./2026-08-21-windows-native-attention-design.md) 覆盖。公开 Timer API、
> Timer 状态机、ClaimTicket、消息持久化、窗口会话、权限与 revision 合同继续有效。

## 1. 文档信息

- 日期：2026-08-20
- 状态：已批准，可进入实施计划
- 目标：为公开插件窗口提供宿主持有、可暂停、可重置、关窗后继续的一次性计时器
- 首个验收插件：独立的番茄钟示例插件，不修改 `demo-win` 的请求级延迟消息语义
- 平台：Windows 11 x64
- 技术基线：Tauri 2、Rust、TypeScript、WebView2

相关合同：

- [番茄钟宿主 API 需求评估](./2026-08-20-pomodoro-plugin-host-api-requirements.md)
- [公开插件 API v1](../../plugin-sdk/public-plugin-v1.md)
- [公开插件命令与单窗口 MVP](./2026-08-13-public-plugin-command-window-mvp-design.md)
- [公开插件消息中心与 Windows 通知](./2026-08-18-public-plugin-message-center-design.md)
- [公开插件宿主延迟消息](./2026-08-19-public-plugin-delayed-message-design.md)

## 2. 兼容与版本决策

UiPilot 尚未发布公开插件 API，因此本阶段直接修改预发布的 `apiVersion: 1`：

- 不新增 `apiVersion: 2`；
- 不提供旧窗口桥兼容层、迁移 DTO 或双写路径；
- 不改变 `notifications.publish()` 和 `notifications.schedule()` 的请求级合同；
- 未声明新权限的现有插件继续使用原有窗口桥，不获得计时能力；
- Schema、TypeScript SDK、开发者指南和示例插件以本设计为新的 v1 基线。

## 3. 目标与非目标

### 3.1 目标

1. 插件窗口显示后，内容页可以开始、暂停、恢复和重置一个宿主持有的计时器。
2. 每个插件 generation 最多一个计时器，插件窗口隐藏后计时继续。
3. 到期后宿主先写入现有消息中心，再播放一次固定的宿主闹铃。
4. 禁用、卸载、成功升级或故障停用插件时，取消其计时器、会话和闹铃。
5. 再次打开窗口时，内容页读取宿主权威状态，不依赖隐藏 WebView 的本地时钟。
6. 所有跨 Rust/JavaScript 的修订使用规范十进制 `u64` 字符串。

### 3.2 非目标

- 修改或扩展 `notifications.schedule()`；
- 多计时器、周期任务、日历时间、cron 或通用 `background.schedule`；
- 插件 Runtime 请求结束后的任意回调；
- 跨进程重启恢复或补发；
- 插件自定义音频、循环闹铃或远程音频；
- macOS 计时通知；
- 插件窗口以外的计时控制面；
- 让宿主证明内容页调用一定来自真实鼠标点击；
- 自动化控制用户鼠标、键盘或真实前台焦点。

## 4. 冻结决策

| 项目 | 决策 |
| --- | --- |
| API 版本 | 原地扩展预发布 `apiVersion: 1` |
| 新权限 | `timer.control` |
| 依赖权限 | 同时声明并获准 `ui.window`、`notifications.publish` |
| 清单模式 | Windows、`activationMode: submit`、`outputMode: window` |
| 计时数量 | 每个 `pluginId + pluginGeneration` 一个 |
| Stop | 暂停并保留 remaining |
| Reset | 回到本轮设定时长，不自动开始，丢弃未提交的完成数据 |
| 到期消息 | 复用消息中心，保存合法原文 |
| 闹铃 | 宿主固定音效，播放一次、有限时长、不循环 |
| 进程退出 | 丢弃未完成计时器，不恢复、不补发 |
| 窗口隐藏 | 撤销控制会话，但不取消已经 running 的宿主计时器 |
| `claiming` | 仅宿主内部状态，对外投影为 `running` 且 `remainingMs = 0` |

## 5. 术语与身份

- **timer service**：原生进程内管理全部公开插件计时器的宿主服务。
- **timer record**：某个 `pluginId + pluginGeneration` 的唯一权威计时状态。
- **round**：一次从新输入开始并最终暂停、重置、失败或到期的计时轮次。
- **window timer session**：一次插件窗口成功显示后获得的临时控制资格。
- **session generation**：宿主内部的窗口控制会话世代，不暴露给插件 DTO。
- **timerRevision**：某插件计时状态机的单调 `u64` 修订，跨边界使用十进制字符串。
- **ClaimTicket**：到期线程在锁内赢得 `running -> claiming` 后取得的内部提交票证。
- **AudioTicket**：消息保存成功并提交 `fired` 后签发的内部播放票证。
- **public projection**：内部状态转换成插件可见 `PluginTimerState` 的结果。

`requestId`、插件 generation、session generation、timer revision、ClaimTicket 和 AudioTicket 含义不同，不能互换。

## 6. Manifest 与权限

### 6.1 新权限

`PublicPermission` 新增：

```text
timer.control
```

权限说明固定表达为：允许插件窗口控制一个关窗后仍由 UiPilot 运行的计时器；到期后写入一条消息并播放固定提示音。

### 6.2 清单组合

使用计时 API 的插件必须满足：

```json
{
  "apiVersion": 1,
  "supportedPlatforms": ["windows"],
  "command": {
    "activationMode": "submit",
    "outputMode": "window"
  },
  "permissions": [
    "ui.window",
    "notifications.publish",
    "timer.control"
  ]
}
```

固定校验：

- `timer.control` 仅在 Windows 可用；
- 声明 `timer.control` 时必须同时声明 `ui.window` 与 `notifications.publish`；
- 命令必须是 `submit + window`，且必须存在合法 `window.entry`；
- 缺少依赖权限或模式不匹配返回 `InvalidPackage`；
- 当前平台不支持 `timer.control` 返回 `UnsupportedPermission`；
- Runtime ready、安装 staging 和原子激活规则不变。

安装或更新提交继续沿用现有全量确认合同：`permission_grants` 必须与 Manifest 声明权限集合完全相等。
用户未确认任一声明权限时，本次安装/更新不提交，不创建“已安装但部分授权”的记录；更新失败保留当前
generation。`PermissionDenied` 只作为运行时纵深防御，用于拒绝伪造、陈旧或内部状态不一致的调用。

## 7. 公开 TypeScript API

### 7.1 类型

```ts
export type U64Decimal = string

export type PluginTimerPhase = 'idle' | 'running' | 'paused' | 'fired'

export interface PluginTimerStartInput {
  readonly durationMs: number
  readonly completionMessage: string
}

export type PluginTimerState =
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'idle'
      durationMs: number | null
      remainingMs: number | null
    }>
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'running' | 'paused'
      durationMs: number
      remainingMs: number
    }>
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'fired'
      durationMs: number
      remainingMs: 0
    }>

export interface UiPilotPluginWindowTimerApiV1 {
  getState(): Promise<PluginTimerState>
  start(input?: Readonly<PluginTimerStartInput>): Promise<PluginTimerState>
  stop(): Promise<PluginTimerState>
  reset(): Promise<PluginTimerState>
  onStateChanged(handler: (state: Readonly<PluginTimerState>) => void): () => void
}

export interface UiPilotPluginWindowApiV1 {
  onUpdate(
    handler: (update: Readonly<PluginWindowUpdate>) => void | Promise<void>,
  ): () => void
  readonly timer: Readonly<UiPilotPluginWindowTimerApiV1>
}
```

API 对象、输入和状态 DTO 由 bootstrap 深冻结。`Object.freeze()` 只防误改，不是权限边界。

`timer` 字段固定存在于所有公开插件窗口的 `UiPilotPluginWindowApiV1`，不采用可选字段。未声明或未获准
`timer.control` 的普通窗口插件仍会看到该冻结对象，但调用任一方法都返回 `PermissionDenied`，且不能
读取或影响任何 timer record。这样 SDK 只维护一份确定的 `.d.ts`；Rust 的 caller、权限、session 和
generation 复核仍是唯一安全边界。

### 7.2 DTO 精确性

- 所有输入和返回对象拒绝未知字段；
- `durationMs` 必须是 JavaScript 安全整数，范围 `1_000..=86_400_000`；
- `completionMessage` 必须是字符串，`trim()` 后非空，最多 500 个 Unicode 标量值，不含控制字符；
- 合法消息保存原文，不裁掉边界空白；
- `pluginId`、generation、插件名称、session generation、revision 和票证均不能由内容页传入；
- `durationMs` 和 `remainingMs` 非 `null` 时始终是安全整数；running 快照按宿主当前时钟计算并夹在
  `0..=durationMs`；
- 首次开始前状态为 `idle`，`durationMs` 与 `remainingMs` 都是 `null`；
- Reset 后状态为 `idle`，二者都等于刚被重置轮次的 `durationMs`。

首次窗口中的 `00:10` 不是宿主默认计时状态。番茄钟示例从自己的 Runtime/window 数据取得
`10_000` 并在本地渲染；宿主在第一次 `start({ durationMs: 10_000, ... })` 成功时才冻结该时长。

### 7.3 `start()`

| 当前公开状态 | 调用 | 结果 |
| --- | --- | --- |
| 首次 `idle` | `start(input)` | 校验并冻结新轮次，进入 `running` |
| Reset 后 `idle` | `start(input)` | 重新冻结输入，进入 `running` |
| `paused` | `start()` | 使用 frozen remaining 恢复 `running` |
| `running` | `start()` | 幂等，返回当前状态 |
| 内部 `claiming` | `start()` | 幂等，返回 `running / remainingMs = 0` 投影 |
| `fired` | `start(input)` | 停止旧轮次尚未结束的音频，冻结新轮次并进入 `running` |

需要 `input` 时省略返回 `TimerInputRequired`；不允许 `input` 时传入返回 `TimerInputNotAllowed`。失败不改变计时状态、revision 或冻结数据。

### 7.4 `stop()`

- `running -> paused`，保存调用线性化时的 remaining，并取消该轮尚未取得 ClaimTicket 的到期任务；
- `paused`、`idle`、`fired` 幂等返回当前状态；
- 内部 `claiming` 时 Stop 已太迟，返回 `running / remainingMs = 0` 投影，不能变成 `paused`；
- Stop 不用于关闭已经开始的闹铃；Reset 或生命周期取消负责该行为。

### 7.5 `reset()`

- `running`、`paused`、`fired` 或内部 `claiming` 均可 Reset；
- 进入 `idle`，保留该轮 `durationMs` 作为显示初值，不自动开始；
- 丢弃尚未获得 delivery admission、尚未交给消息中心持久化的 frozen completion；
- 撤销仍未提交的 ClaimTicket 和尚未开始的 AudioTicket；
- 消息已保存时不删除历史消息；
- ClaimTicket 已获得 delivery admission、消息持久化已经开始时，Reset 不能保证阻止该条消息落盘；
  迟到成功仍不得提交 `fired` 或签发 AudioTicket；
- 已经开始但尚未结束的本轮闹铃立即停止；
- 从首次、尚无轮次的 `idle` Reset 为幂等空状态。

### 7.6 `getState()` 与 `onStateChanged()`

- `getState()` 返回调用时的完整权威投影；
- 新 session 的第一次 `getState()` 是基准，无论 revision 是否与旧页面内存相同都必须接受；
- running 的 `remainingMs` 是读取时采样值，不是独立状态转换。同一 session 的后续 `getState()` 若
  revision 相等，只能在 phase 与 `durationMs` 也完全相同时刷新本地倒计时锚点；不得用相等 revision
  改写 phase、duration 或 frozen round 所有权。低 revision 一律拒绝；
- `onStateChanged()` 每个 prepared/active session 只允许一个 handler，重复注册抛 `TypeError`；
- 订阅只发送完整状态，不发送差量；
- Start、Stop、Reset、`fired` 和消息失败回到 `idle` 后发送状态；
- 内部 `claiming` 可以不发送事件；后续公开 revision 允许跳号；
- handler 抛错不回滚计时状态，不影响消息或闹铃；
- 会话撤销后立即停止投递，旧 unsubscribe 仍可安全调用且幂等。

方法返回与状态事件可以乱序到达。它们携带同一权威转换时 revision 相同，前端接受先到者并忽略后到
的相同 revision；只有当前 session 主动发起且仍拥有最新本地读取 token 的 `getState()` 可以按上述规则
用相等 revision 刷新 running 的剩余时间锚点。

## 8. `timerRevision` 合同

- 宿主内部是 `u64`，初始值为 `"0"`；
- 每次权威状态机转换递增，包括内部 `claiming`；
- 内部转换可以不发事件，因此前端可观察到跳号；只保证严格单调，不保证连续；
- 跨边界必须是无前导零的规范十进制字符串，匹配 `^(0|[1-9][0-9]*)$`；
- 值域为 `0..=18446744073709551615`；
- TypeScript 必须复用现有 `parseU64Decimal` 与 `compareU64Decimal`；
- 禁止 `Number`、`parseInt`、隐式数值转换和直接字符串关系比较；
- 同一 session 中，权威状态转换只接受 `compareU64Decimal(next, seen) > 0`；当前 session 的最新
  `getState()` 可按 7.6 的窄规则接受相等 revision 的 running 采样；
- 新 session 第一次完整快照重建 `seen`，不沿用旧 session 的接收游标。

session generation 同样使用不回绕的宿主内部计数器。无法签发新 generation 时，不复用旧 session；
该插件窗口的计时控制失败关闭并销毁窗口，宿主已经 running 的计时器继续，直到到期、生命周期取消或
进程退出。新的原生进程可以重新建立 session。

计数器无法继续递增时，该插件计时服务进入会话内终态 `TimerUnavailable`：

- 不回绕、不复用 revision；
- 撤销 active session、ClaimTicket 和 AudioTicket；
- 取消 running/paused/claiming 轮次并停止本轮闹铃；
- 后续所有计时方法失败关闭；
- 只有新的 UiPilot 原生进程才能重新从 `"0"` 开始；
- 已保存消息不撤回。

## 9. 窗口计时会话

### 9.1 建立

每次插件窗口准备显示或已显示窗口收到新的当前 invocation 时，宿主准备新的 session generation。session
具有 `prepared | active | closing | revoked` 四个内部阶段：

1. 绑定当前 `pluginId + pluginGeneration + contentLabel + window owner`，建立 `prepared` session；
2. 内容页在本次 `onUpdate` 中先注册 `onStateChanged()`，再调用 `getState()`；prepared 阶段只允许这两个
   只读/订阅动作，`start`、`stop`、`reset` 均返回 `ExpiredWindowSessionError`；
3. 等待内容页 update ack；
4. 完成原生 show/focus 交接；
5. 成功提交窗口交接后把 session 转为 `active`，并向该 session 推送一次当前完整状态；内容页按 revision
   合并这次状态与 prepared 阶段取得的快照。

窗口交接失败必须撤销 prepared session，不得改变计时器。prepared 阶段的订阅不会赋予隐藏页面控制权。

### 9.2 每次调用的宿主守卫

Rust 对每个窗口计时调用重新校验：

- caller 是 `plugin-content-*` WebView，且 label 精确映射到当前插件；
- 插件仍安装、启用、健康，active generation 一致；
- Manifest 声明且用户授予 `ui.window`、`notifications.publish`、`timer.control`；
- 当前窗口 owner、content label 与 active session generation 一致；
- `getState` 与 `onStateChanged` 要求 session 为 prepared 或 active；`start`、`stop`、`reset` 要求 session
  为 active，且窗口尚未进入隐藏/关闭事务；
- 输入、状态和消息中心前置条件合法。

内容页不能提交身份字段。caller label 也不能单独构成授权，必须与 session 和 active generation 同时匹配。

### 9.3 撤销

以下事件撤销 active session：

- 显式关闭、失焦自动隐藏或宿主隐藏插件窗口；
- 新 invocation 替换窗口 owner；
- 插件禁用、卸载、故障停用、成功升级、reload/replacement；
- 插件改名或保存设置导致当前内容配置过期；
- timer revision 耗尽或计时服务进入不可用。

窗口隐藏事务先把 session 标记为 closing，使新调用失败，再执行原生 hide：

- hide 成功：提交撤销；
- hide 失败且窗口仍可用：不复活旧 session，签发新的 session generation，并要求内容页重新 `getState()` 和订阅；
- 无法恢复一致状态：销毁该插件窗口，宿主计时器按其原状态继续。

session 在 API 状态转换线性化点之前失效时，调用返回 `ExpiredWindowSessionError`，不能改变计时器。状态转换已经提交后才发生隐藏，不撤销该转换。

## 10. 宿主计时器服务

### 10.1 内部记录

每个 active plugin generation 最多一个记录：

```text
PluginTimerRecord
  pluginId
  pluginGeneration
  internalPhase: idle | running | paused | claiming | fired
  timerRevision: u64
  roundId
  durationMs?
  remainingMs?
  dueAtMonotonic?
  frozenCompletion?
  claimTicket?
  audioTicket?
  unavailable

FrozenCompletion
  completionMessage
  pluginNameSnapshot
  pluginId
  pluginGeneration
  durationMs
```

ClaimTicket 还包含内部 `deliveryAdmitted` 标志。`roundId` 和票证均为宿主内部单调身份，不跨
JavaScript 边界。它们耗尽时与 revision 耗尽相同，失败关闭。

### 10.2 调度器

- 一个共享宿主 worker 和按 due time 排序的内存队列；
- 不为每个插件建立线程；
- 队列项绑定 `pluginId + generation + roundId + due revision`；
- Stop、Reset 和生命周期取消可以移除队列项，也允许留下随后会因身份不匹配而被丢弃的惰性项；
- worker 唤醒后先在计时器锁内验证权威记录，再决定是否签发 ClaimTicket；
- 窗口可见性、内容页本地定时器和 Runtime 请求状态不参与领取。

### 10.3 时间来源

- due point 使用包含 Windows 睡眠经过时间的宿主单调时钟抽象；
- 用户修改系统墙上时间不改变 due point；
- 暂停保存剩余毫秒，恢复时使用 `now + remaining` 建立新 due point；
- 睡眠跨过 due point 时，进程恢复后立即尝试领取；
- 同一 round 最多取得一张 ClaimTicket。

生产代码不得直接把平台 `Instant` 的未验证睡眠语义当成公开合同。计时服务通过可替换 Clock 接口实现并测试上述规则。

## 11. 状态转换与线性化

### 11.1 公开状态表

| 当前状态 | Start | Stop | Reset | 到期 |
| --- | --- | --- | --- | --- |
| `idle` | 有 input 时进入 `running` | 幂等 | 幂等并保留显示初值 | 不可能 |
| `running` | 无 input 时幂等 | 进入 `paused` | 进入 `idle` | 内部进入 `claiming` |
| `paused` | 无 input 时恢复 `running` | 幂等 | 进入 `idle` | 不可能 |
| `fired` | 有 input 时开始新轮次 | 幂等 | 进入 `idle` | 不可能 |

### 11.2 Stop 与 claim

- Stop 的线性化点：计时器锁内 `running -> paused`；
- claim 的线性化点：同一把锁内 `running -> claiming`；
- 两者由同一状态机串行决定先后；
- Stop 先赢时，worker 不能取得 ClaimTicket；
- claim 先赢时，Stop 太迟，只能取得 `running / remainingMs = 0` 投影；
- `claiming` 不对外暴露，也不要求发送状态事件。

### 11.3 ClaimTicket 流程

1. worker 在计时器锁内验证 running、generation、roundId 和 due revision。
2. `running -> claiming`，revision 加一，签发唯一 ClaimTicket。
3. 立即释放计时器锁。
4. 锁外进入 delivery admission：
   - 先获取插件 mutation guard，只读复核插件仍安装、启用、健康，active generation 与权限一致；
   - 资格不通过时不调用消息中心。释放 guard 后重新取得 timer 锁：ticket 仍有效则
     `claiming -> idle`、revision 加一并保留本轮显示时长；ticket 已被生命周期撤销则不改变撤销方状态；
   - 资格通过时，保持 plugin mutation guard，按固定 `plugin mutation -> timer` 顺序取得 timer 锁，
     再次验证同一 ClaimTicket；仍有效才把 `deliveryAdmitted` 设为 true；
   - 释放 timer 锁和 plugin mutation guard。未取得 admission 时不得持久化。
5. 只有 admission 成功后，才在所有上述锁之外调用消息中心原子持久化。
6. 持久化返回后重新获取计时器锁，并凭同一票证提交：
   - 保存成功且票证有效：`claiming -> fired`，revision 加一，签发 AudioTicket；
   - 保存失败且票证有效：`claiming -> idle`，revision 加一，丢弃 frozen completion，但保留该轮
     `durationMs` 并令 `remainingMs = durationMs`，供 UI 显示和下一次 Start 重新冻结；
   - 票证已撤销：不改变撤销方已经提交的状态，不签发 AudioTicket。
7. 释放锁后发送计时状态事件、派发消息中心后置效果并尝试播放音频。

delivery admission 是 lifecycle 与消息持久化之间的资格线性化点：

- 生命周期先提交：资格复核失败或 ticket 已撤销，绝不调用消息中心；
- admission 先提交：随后 Reset 或生命周期操作可以撤销 ticket，但已经开始的持久化可能成功；成功消息
  不删除，不能提交 `fired` 或开始音频；
- 资格复核全过程不得在持有 timer 锁时获取 plugin mutation guard，避免
  `timer -> plugin mutation` 与生命周期 `plugin mutation -> timer` 形成反向依赖。

消息中心的 ready 事件、Windows 通知、托盘提醒始终与实际已保存消息保持一致。

## 12. 消息中心与闹铃

### 12.1 开始前置条件

新轮次 Start 和 paused Resume 都要求消息中心当前不是 unavailable：

- 新轮次失败时保持原状态，不冻结输入；
- Resume 失败时保持 `paused`；
- Start 成功后消息中心若在到期前变为 unavailable，到期提交按失败路径回到 `idle`。

消息中心 ready 检查不是未来成功保证，到期时仍必须执行真实提交。

### 12.2 消息提交

- 使用 `MessagePublishRequest` 同等校验和原子文件提交；
- `pluginNameSnapshot` 由宿主在新轮次 Start 时从当前已验证插件配置取得；
- 不需要当前 `onCommand requestId`，不伪造 Runtime 请求；
- 消息保存成功后，即使 ClaimTicket 随后被撤销也不删除记录；
- 消息中心状态事件、Windows 通知和托盘效果相互独立，失败不回滚消息；
- 到期提交失败不重试、不补发，不播放闹铃。

### 12.3 AudioTicket

- 只有有效 ClaimTicket 成功提交 `fired` 后才能签发；
- 绑定 `pluginId + generation + roundId + fired revision`；
- 音频在计时器锁外启动；启动前重新验证票证；
- Reset、从 fired 开始新轮次、禁用、卸载、故障停用、成功升级和退出使未开始票证失效；
- 上述操作也停止本轮已经开始但尚未结束的播放；
- 隐藏窗口本身不撤销 AudioTicket；
- 播放失败保留 `fired` 和消息，不重试；
- 固定音效播放一次且有界，不提供插件文件路径或音频数据参数。

多个插件同时到期时，每张 AudioTicket 独立尝试。宿主可以混音或串行播放，但不能因此重复消息；音频设备不可用只记录受控诊断。

## 13. 生命周期

| 事件 | session | timer | audio | 已保存消息 |
| --- | --- | --- | --- | --- |
| 窗口隐藏/关闭 | 撤销 | running/paused 保留；内部 claiming 按 running 处理，不撤销 ClaimTicket，不中断 admission/persist | 已签发票证保留 | 保留 |
| 插件改名/保存设置 | 撤销 | 保留，冻结数据不变 | 保留 | 保留 |
| 禁用/故障停用 | 撤销 | 取消 | 撤销并停止 | 保留 |
| 卸载 | 撤销并销毁窗口 | 取消 | 撤销并停止 | 按消息中心历史合同保留 |
| 成功升级/reload/replacement | 撤销旧 generation | 取消旧 generation | 撤销并停止 | 保留 |
| staging/ready/确认失败的升级 | 旧 session 不因失败提交而失效 | 旧 timer 保留 | 保留 | 保留 |
| 进程退出 | 终止 | 丢弃 | 停止 | 已落盘消息保留 |

成功 generation 切换在插件 mutation 提交边界内撤销旧 timer。失败升级不能提前取消当前 generation。

## 14. 锁顺序与异步边界

固定规则：

1. 插件 mutation 锁可以调用 session/timer 的纯内存生命周期转换，但失败的插件 mutation 不得提前撤销
   当前 generation 的 session 或 timer。
2. 窗口计时 API 按 `session controller -> timer record` 顺序获取短锁。
3. timer worker 的 claim 阶段只获取 timer 锁；释放后进入 admission 阶段时，严格按
   `plugin mutation guard -> timer record` 取锁，任何路径都不得按 `timer -> plugin mutation` 反向获取。
4. ClaimTicket 签发后释放 timer 锁；delivery admission 按
   `plugin mutation guard -> timer record` 复核资格和 ticket，再释放两者后调用消息存储。
5. 消息存储锁、计时器锁和 session 锁不得同时持有。
6. 任何锁不得跨越原生 show/hide/focus、消息文件 I/O、Windows 通知、托盘、音频、前端 evaluate/emit 或等待内容页 ack。

生命周期提交若需要同时变更多个服务，固定顺序为：

```text
plugin mutation guard
-> validate and commit plugin generation / enabled state
-> if and only if commit succeeded:
     revoke old window timer session in memory
     cancel old timer / ClaimTicket / AudioTicket in memory
-> release all guards
-> destroy/hide old window and stop old audio
```

状态提交成功后、timer 取消完成前，旧 session 的每次 API 调用仍会因 active generation/enabled 复核失败；
旧 claim 的锁外资格复核也必须失败。timer 取消在释放 mutation guard 前完成，因此新的插件管理操作看不到
半提交生命周期。计时器服务在锁内从不调用插件管理器，避免
`plugin mutation -> timer -> plugin mutation` 反向依赖。

## 15. 错误合同

| 错误名 | 条件 | 是否改变状态 |
| --- | --- | --- |
| `InvalidCaller` | 非插件内容 WebView 或 label 不匹配 | 否 |
| `PermissionDenied` | 运行时纵深复核发现 Manifest/授权与当前能力不一致 | 否 |
| `ExpiredWindowSessionError` | session 未激活、已撤销或被新 invocation 替换 | 否 |
| `InvalidTimerInput` | DTO、时长或消息非法 | 否 |
| `TimerInputRequired` | idle/fired Start 缺少 input | 否 |
| `TimerInputNotAllowed` | running/paused/claiming Start 带 input | 否 |
| `MessageStoreUnavailable` | 新轮次或 Resume 时消息中心已不可用 | 否 |
| `TimerUnavailable` | revision/内部身份耗尽、锁损坏或服务终止 | 进入失败关闭终态 |

原生通知、托盘和音频失败不通过已经完成的 API Promise 追溯返回。内容页只收到固定错误名，不接收磁盘路径、线程状态、原生错误码或其他插件信息。

## 16. Bootstrap 与 Tauri Capability

插件内容 WebView 的 capability 增加且仅增加以下窄命令：

- `plugin_window_timer_get_state`
- `plugin_window_timer_start`
- `plugin_window_timer_stop`
- `plugin_window_timer_reset`

`onStateChanged` 使用宿主 bootstrap 的私有、caller-bound 状态入口，不开放通用 Tauri event 订阅。bootstrap 继续删除内容页可见的 `__TAURI_INTERNALS__`，插件不能自行调用命令或指定 label。

私有状态入口必须：

- 只接受宿主构造并通过精确 DTO 校验的状态；
- 校验当前 session generation；
- 深冻结后调用唯一 handler；
- 丢弃旧 session 或非递增 revision；
- 捕获 handler 异常，不向宿主回传权限或业务调用。

Shell WebView 仍只拥有图钉、关闭和身份命令，不获得 timer API。Runtime WebView 仍只拥有请求期 API，不获得窗口 timer 命令。

## 17. 状态投影与插件 UI

- 内容页每次 `onUpdate` 先注册 `onStateChanged()`，再调用 `getState()`，两者在 prepared session 中可用；
- 订阅事件与读取快照按 revision 合并；session 激活时宿主再推一次当前完整状态，因此快照与订阅之间不会
  永久丢失状态；
- running UI 从最近权威 `remainingMs` 开始，用页面 `performance.now()` 做显示插值；
- 页面插值不改变宿主状态，不生成 revision；
- `claiming` 投影固定为 `running / remainingMs = 0`；
- 页面隐藏、节流、reload 或 handler 失败不影响宿主到期；
- 新 session 不沿用旧 subscription 或 revision cursor。

插件内容不能访问 frozen completion 中的宿主名称快照，也不能查询其他插件的 timer。

## 18. 失败与恢复

### 18.1 窗口失败

- session 准备失败：窗口交接失败，timer 不变；
- hide 失败：旧 session 不复活，签发新 session 或销毁窗口；
- 状态事件失败：不回滚状态，下次 `getState()` 恢复；
- 内容页 reload：当前 session 撤销，重新 ready 后建立新 session并读取快照。

### 18.2 调度失败

- worker 无法启动：计时能力全局不可用，普通公开插件功能仍可运行；
- 单插件 revision/identity 耗尽：仅该插件计时服务不可用；
- 队列中的旧 round 项：领取时身份不匹配并静默丢弃；
- 消息提交失败：有效 ClaimTicket 回 `idle`，保留本轮显示时长，无 AudioTicket；
- 消息已保存但 ticket 被 Reset/生命周期撤销：消息保留，不进入 fired，不响铃。

### 18.3 音频失败

- 固定资源缺失或设备不可用：保持 fired，记录受控诊断；
- stop playback 失败：不得恢复或重复播放；
- 进程退出不等待完整音频播放。

## 19. 实现边界

后续实施计划预计触及：

- `src-tauri/src/public_plugins/manifest.rs`：`timer.control` 与组合校验；
- 新的 `src-tauri/src/public_plugins/timers.rs`：Clock、队列、状态机和 tickets；
- 新的宿主 alarm adapter：固定资源、AudioTicket 播放与停止；
- `src-tauri/src/plugin_window.rs`：session generation、bootstrap 和状态投影；
- `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`：窄命令、manage 和生命周期接线；
- `src-tauri/capabilities/plugin-window-content.json`：四个 timer 命令；
- `src/protocol.ts` 与 SDK `.d.ts`：权限、DTO、解析与比较；
- Schema、开发者指南和独立番茄钟示例插件。

`DelayedMessageScheduler` 可以复用共享 worker/Clock 的实现技术，但不得复用或改变 `notifications.schedule()` 的公开任务模型、配额或取消语义。

## 20. 自动化测试合同

### 20.1 Manifest 与边界

- 合法的 Windows `submit + window` 三权限组合可安装；
- 缺任一依赖权限、错误输出模式、macOS 或未知权限拒绝；
- 安装/更新只接受与 Manifest 声明集合完全相等的 permission grants；少确认一个权限时不提交；
- `timer` 字段对所有窗口插件存在；未声明 timer 的内容调用返回 `PermissionDenied`；
- Runtime、shell、main、find 和伪造 content label 调用拒绝。

### 20.2 DTO 与 revision

- 时长边界 `1_000`、`10_000`、`86_400_000`；
- 零、负数、非整数、非安全整数、超上限和未知字段拒绝；
- 消息空白、控制字符、501 标量拒绝，合法边界空白原文保留；
- revision 比较覆盖 `9 -> 10`、`99 -> 100`、跨 `Number.MAX_SAFE_INTEGER` 和 `u64::MAX`；
- 前导零、负号、非十进制和越界 revision 拒绝；
- 内部 claiming 导致前端可见 revision 跳号时仍接受新状态。

### 20.3 状态机

- 表 11.1 的全部转换；
- running Start、paused Stop、idle Reset 幂等且不增加任务；
- paused Resume 保留 frozen completion，只更新 due point；
- fired Start 新轮次并停止旧音频；
- 首次 idle、Reset 后 idle 的 nullable/非 nullable 字段精确；
- 每插件只有一个队列任务和一个权威 timer record。

### 20.4 会话

- show 成功后才激活；prepare/ack/focus 失败不激活；
- prepared session 只允许订阅和读取，不能 Start/Stop/Reset；active 后立即推一次完整状态；
- hide、close、auto-hide、新 invocation、reload 和生命周期变化撤销；
- hide 失败签发新 session，不复活旧对象；
- 旧 session 调用、事件和 Promise 完成均不能改变新状态；
- session 调用与 hide 使用 barrier 测试“调用先提交”和“hide 先提交”。

### 20.5 claim 与音频竞态

- Stop-before-claim：paused，无消息、无音频；
- claim-before-Stop：Stop 返回 running/0，随后 fired 或失败 idle；
- lifecycle-before-admission：资格复核失败，不调用消息中心；
- 资格复核失败但 ClaimTicket 尚有效：凭 ticket 回到 idle，保留显示时长，不调用消息中心；
- admission-before-lifecycle：消息可能保存，旧 generation 不 fired、不响铃；
- Reset-before-admission：ticket 失效，不调用消息中心；
- Reset during persistence：旧 ticket 无 fired/audio，已落盘消息不删除；
- reset after fired before audio start：AudioTicket 失效；
- reset while audio active：立即停止；
- 迟到的旧 ticket 不改变新 round；
- 同一 round 最多保存一条消息、签发一张 AudioTicket。

### 20.6 时钟与生命周期

- 可控 Clock 前进未到期、刚好到期和跨过到期点；
- Windows 睡眠式前进后立即领取；
- 墙上时间跳变不影响；
- 禁用、卸载、故障停用、成功升级取消；
- 失败升级保留；改名/保存设置保留 timer 但撤销 session；
- 进程 shutdown 清空 timer 并先停止音频；下次启动不恢复。

### 20.7 前端/bootstrap

- API、输入、状态深冻结；
- `onStateChanged` 单 handler、unsubscribe 幂等、异常隔离；
- 快照/事件、Promise/事件乱序按 revision 收敛；
- 同 revision 的主动 `getState()` 只能刷新 running 剩余时间锚点，不能改写权威状态；
- claiming 不出现在公开 phase；
- 新 session 基准不继承旧 cursor；
- 禁止内容页获取 Tauri internals、任意 event、身份或票证。

## 21. 人工验收

人工验收只能由用户操作。自动化和 Agent 不得控制鼠标、键盘或真实前台焦点。

1. 安装独立番茄钟插件，确认权限包含窗口、消息和计时器。
2. 输入 `/pomodoro`，窗口从示例插件自身数据渲染 `00:10`；此时宿主首次 idle 的时长仍为 null，Enter
   本身不启动计时。
3. 点击开始，从点击时刻计时；关闭窗口后继续。
4. 暂停后等待原 due point，不出现消息和闹铃；恢复后从 remaining 继续。
5. Reset 回到 `00:10` 且不自动开始。
6. 开始后关闭窗口，到期产生一条消息并播放一次有限闹铃；主窗口不被抢占。
7. 再次打开窗口，显示权威 remaining 或 fired，而不是重新从初值运行。
8. 到期边缘操作 Stop/Reset，结果符合 ClaimTicket 合同且不重复消息。
9. 运行中禁用、卸载或成功更新插件，不再到期；失败更新不影响旧计时器。
10. 退出 UiPilot 后重新启动，旧计时器不恢复、不补发。

## 22. 验收标准

1. `timer.control` 只在合法、获授权的 Windows `submit + window` 插件内容会话中可用。
2. 每个插件 generation 只有一个宿主持有计时器，隐藏窗口不取消，隐藏页面不能继续控制。
3. Stop、Reset、claim、生命周期和新 round 的竞态由同一状态机及 tickets 决定，不持锁执行外部副作用。
4. 到期消息最多保存一次；保存失败不响铃，保存成功后的原生副作用失败不回滚消息。
5. AudioTicket 防止 Reset 或生命周期完成后迟到开始播放。
6. 所有状态 DTO 使用规范十进制 `timerRevision` 并在乱序下收敛。
7. 睡眠、墙钟修改、进程退出和 generation 变化符合本文合同。
8. 原有 `notifications.publish()`、`notifications.schedule()`、消息中心和普通插件窗口没有语义回归。

## 23. 最终结论

番茄钟不需要通用后台 Runtime。预发布 v1 新增 `timer.control` 和窗口内单例计时桥，宿主负责权威时钟、可取消状态机、到期消息及固定闹铃。窗口内容页只在 active session 中提交控制意图并投影状态；窗口隐藏后会话失效，宿主计时继续。ClaimTicket、AudioTicket、generation 和十进制 revision 共同封闭取消、迟到副作用与乱序状态。
