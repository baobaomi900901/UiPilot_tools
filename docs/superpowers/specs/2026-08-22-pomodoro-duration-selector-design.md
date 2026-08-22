# UiPilot Pomodoro 计时长度选择与插件窗口存储设计

## 1. 文档信息

- 日期：2026-08-22
- 状态：Approved，四轮独立审核通过，可进入实施计划
- 范围：公开插件窗口私有存储 API、Pomodoro 计时长度选择、持久化与下一轮计时语义
- 公开 JavaScript API：扩展 `UiPilotPluginWindowApiV1`
- Manifest 字段：不变
- 新权限：无

本设计增量扩展以下已批准合同：

- [公开插件命令与单窗口 MVP 设计](./2026-08-13-public-plugin-command-window-mvp-design.md)
- [公开插件窗口计时 API 设计](./2026-08-20-public-plugin-window-timer-api-design.md)
- [公开插件计时闹铃资源设计](./2026-08-22-public-plugin-timer-alarm-assets-design.md)

除本文明确新增的窗口存储门面和 Pomodoro 交互外，既有插件身份、ActivationBundle、窗口会话、Timer
状态机、`timerRevision`、消息提交、闹铃资产和焦点合同继续有效。

## 2. 目标与非目标

### 2.1 目标

1. 在 Pomodoro 插件内容区右上角提供固定计时长度选项。
2. 默认使用 10 分钟，并让选择跨窗口关闭、UiPilot 重启、插件升级及保留数据重装持续存在。
3. 当前运行或暂停轮次冻结原时长；选择变化只影响下一轮。
4. 为所有公开插件内容窗口提供通用、插件隔离、会话绑定的私有存储门面。
5. 复用现有 `PluginStorageStore` 的键、JSON、配额、原子持久化和卸载保留合同。

### 2.2 非目标

- 不把计时长度加入 Manifest 设置页。
- 不给 Timer 状态机增加默认时长、偏好或持久化职责。
- 不持久化正在运行的 Timer，不在进程重启后恢复轮次。
- 不允许插件窗口访问其他插件、文件路径、秘密明文或宿主任意设置。
- 不增加多个计时器、自定义分钟数、秒级输入或自由文本时长。
- 不改变 `notifications.publish()`、闹铃资源或普通消息提示音合同。

## 3. 用户合同

Pomodoro 内容区右上角显示一个下拉选择器，固定选项及值为：

| 显示文本 | 持久值 | 启动毫秒数 |
|---|---:|---:|
| 10分钟 | 10 | 600000 |
| 15分钟 | 15 | 900000 |
| 25分钟 | 25 | 1500000 |
| 30分钟 | 30 | 1800000 |
| 45分钟 | 45 | 2700000 |

首次使用、键不存在或持久值非法时显示 10 分钟。读取期间选择器显示 10 分钟并禁用。读取成功后显示保存
值；读取失败时保留 10 分钟、恢复可操作状态，并显示“无法读取计时长度”。

选择新值时立即开始原子保存，并在完成前禁用选择器以及 `idle` 的“开始”/`fired` 的“重新开始”。成功后该值
成为下一轮时长；失败时恢复上一个已保存值或默认 10，并显示“无法保存计时长度”。当前 `running` 或 `paused`
轮次不被重置、不改变剩余时间；暂停后的“继续”在保存期间仍可用并恢复同一轮。从 `idle` 或 `fired` 点击
“开始/重新开始”时才把当前 effective 值传给 `timer.start()`。

窗口恢复时，如果宿主已有运行轮次，倒计时显示当前轮剩余时间，选择器显示下一轮保存时长。打开、选择、
保存或失败不得改变 `timerRevision`。

## 4. 公开窗口存储 API

在 `uipilot-plugin-api-v1.d.ts` 中新增：

```ts
export interface UiPilotPluginWindowStorageApiV1 {
  get(key: string): Promise<JsonValue | null>
  set(key: string, value: JsonValue): Promise<void>
  remove(key: string): Promise<void>
}

export interface UiPilotPluginWindowApiV1 {
  onUpdate(
    handler: (update: Readonly<PluginWindowUpdate>) => void | Promise<void>,
  ): () => void
  readonly timer: Readonly<UiPilotPluginWindowTimerApiV1>
  readonly storage: Readonly<UiPilotPluginWindowStorageApiV1>
}
```

`storage` 对所有合法公开插件内容窗口存在，不要求新权限。它与 Runtime 的 `api.storage` 共享同一个插件私有
命名空间。当前 Runtime storage 只有 prototype-key 拒绝规则；本次在未发布阶段统一收紧 Runtime 与 Window
storage，冻结以下唯一合同：

- key 匹配 `^[a-z][a-z0-9.-]{0,63}$`，且不得为 `__proto__`、`prototype` 或 `constructor`；
- value 必须是受支持的有限 JSON 值；
- 单插件总配额为 5 MiB；
- `set`、`remove` 使用现有原子持久化，失败保留旧内存值和磁盘值；
- 升级保留数据；卸载选择“保留数据”后，同一 `pluginId` 重装继续读取；彻底卸载删除数据。

`valid_storage_key` 必须成为 Runtime 与 Window 两个门面的共同 validator。现存开发数据中包含非法 key 的
document 按既有无效文档隔离规则处理；本预发布版本不增加旧 key 兼容分支。

不新增公开 JavaScript 错误名，但允许实现增加内部 `WindowStorageError` 或等价映射层。唯一映射为：

- 无法解析的内容 WebView label：`InvalidCaller`；
- 空、非规范、溢出、非当前或阶段不允许的 session：`ExpiredWindowSessionError`；
- 非法 key 或 JSON value：`InvalidOperation`；
- 配额、序列化、磁盘或原子提交失败：`StorageError`；
- 合法调用在 Window lease 后因 generation、activationId、admission epoch 或 ActivationBundle 已被生命周期替换：
  `ExpiredWindowSessionError`，零存储访问；
- `InvalidContext` 只用于后端不变量损坏或合法调用不可能产生的伪造 plugin scope/ActivationBundle 身份，
  零存储访问。

Runtime storage 同步采用 `InvalidOperation` 与 `StorageError` 的相同分类，不再把所有 `PluginStorageError` 无差别
映射为 `StorageError`。错误消息不包含插件数据、key、路径、generation、activationId 或 session generation。

Pomodoro 固定使用小写 key `pomodoro.duration-minutes`，只接受数值 `10 | 15 | 25 | 30 | 45`。读取到其他
JSON 类型、非整数或名单外整数时回退 10，但不自动写回；只有用户主动选择才覆盖旧值。

## 5. 身份、会话与授权

每个窗口存储调用携带当前 bootstrap 闭包冻结的 `sessionGeneration`。Rust 以后端事实重新解析调用者内容
WebView label，并通过 `PluginWindowController` 取得当前 `PluginWindowOwner`。授权必须同时满足：

1. caller 是当前内容 WebView，而不是 main、find、Runtime 或其他插件窗口；
2. `sessionGeneration` 是规范十进制 u64 且等于当前 Prepared 或 Active 窗口会话；
3. owner 的 `pluginId + pluginGeneration + activationId` 仍属于当前已启用、无故障 ActivationBundle；
4. 存储作用域只从后端 owner 的 `pluginId` 派生，不接受 JavaScript 传入插件 ID 或磁盘路径。

窗口 bootstrap 在 `onUpdate` 前建立 Prepared session。`storage.get` 与 Timer `getState` 一样可在 Prepared 或
Active 阶段获准，使 `onUpdate` handler 能在 ACK 前读取基准值；`storage.set/remove` 只可在 Active 阶段获准。
窗口隐藏、会话替换、插件禁用、故障停用、升级、卸载或 Runtime/内容重建后，旧门面调用返回
`ExpiredWindowSessionError`。插件保存旧 facade 不能跨会话继续读写。

`window.uipilotPluginWindow`、`timer`、`storage` 及其方法继续深度冻结。bootstrap 不向插件暴露 Tauri internals、
真实命令名、路径、generation、activationId 或其他所有权令牌。

## 6. 后端命令与锁边界

新增窄命令分别处理窗口存储 `get/set/remove`。`PluginWindowController` 把现有 Timer session/call lease 泛化为
同一个 Window session/call lease；Timer 与 storage 共用 phase、session generation、in-flight 计数和关闭等待，
不建立第二套窗口会话状态。

为保证彻底卸载不会被任何已获准的旧存储写重新创建目录，manager 还维护每插件的
`PluginDataCallGate`（或等价准入/排空器）。Runtime 与 Window 的 `storage.get/set/remove` 在各自身份和会话校验
后都必须取得绑定当前 ActivationBundle 的 `PluginDataCallLease`，才可进入 `PluginStorageStore`；lease 覆盖完整
存储 I/O，并在完成后释放。它不替代 Window call lease：Window storage 调用同时持有 Window call lease 与
PluginDataCallLease，前者约束窗口 session，后者建立卸载排空边界。

每个 ActivationBundle 内部拥有单调 `admissionEpoch`；它由 manager 分配，不暴露给插件。`PluginWindowOwner`
与 `ScheduledPluginRequest` 在建立时捕获当前 epoch。Data gate 只接受完全匹配的
`pluginId + pluginGeneration + activationId + admissionEpoch`，关闭后拒绝新 lease；失败恢复准入必须分配新
epoch，旧窗口 owner 和旧 Runtime request 因此不能跨 epoch。

每条 Window storage 命令遵循：

1. 解析并验证 session generation；
2. 在 `PluginWindowController` 内按只读/可变操作建立一次调用 lease，复制后端 owner，然后释放 controller
   mutex；`get` 传只读，`set/remove` 传可变；
3. 在同一次 manager 临界区内依据 owner 重新验证当前 ActivationBundle，并取得该 bundle 的
   `PluginDataCallLease`，然后释放 manager 锁；验证与 lease 签发之间不得存在卸载可插入的窗口；
4. 不持有 controller、manager mutation gate、ActivationBundle、Timer 或消息中心锁地调用现有
   `PluginStorageStore`；
5. 返回固定成功值或稳定错误，最后释放 data lease 与 Window call lease。

Runtime storage 不取得 mutation gate。它在 `scheduler.with_current()` 持有当前请求守卫期间，从
`ScheduledPluginRequest` 读取 activationId 与 admissionEpoch，直接调用只取得 data gate 自身 mutex 的
`try_acquire(pluginId, generation, activationId, admissionEpoch)`；gate 在自身临界区原子校验 tuple、open 状态并
签发 lease。Runtime 不得先退出 `with_current` 再取 data lease，也不得在 scheduler mutex 内获取 mutation gate。

存储 I/O 不取得 Timer 锁，不发布 Timer 状态事件，也不修改 inventory revision。两个 lease 的线性化含义不得
混淆：

- Window call lease 只线性化窗口 session/phase 准入；普通 hide/close 进入 Closing 后不再签发，并等待已签发
  Window lease 释放；
- 当前 ActivationBundle 复核与 `PluginDataCallLease` 的原子签发才是存储访问的最终授权线性化点；
- 卸载可以拒绝只持有 Window lease、尚未取得 data lease 的调用；data lease 一旦签发，卸载必须等待其完整
  存储 I/O 完成。

前端仍必须用会话 epoch 丢弃迟到 UI 完成。

### 6.1 完全卸载顺序

彻底卸载必须保证数据删除晚于所有旧窗口写入，不得沿用当前“先删除数据、后 teardown 窗口”的顺序：

1. `PublicPluginManager` 在 plugin mutation gate 内签发唯一卸载事务并关闭 manager 新命令/窗口 transfer 准入；
   随后在同一 mutation 临界区依次取得 scheduler mutex，淘汰/阻止该插件 Runtime request，再取得 data gate
   mutex 关闭新 data lease 准入，最后按逆序释放。唯一锁顺序是 `mutation -> scheduler -> data gate`；Runtime
   只走 `scheduler -> data gate`，Window 只走 `mutation -> data gate`；任何路径禁止反向获取；
2. `PluginWindowController` 把现有 session 转为 Closing，拒绝新的 Window `get/set/remove` 与 Timer 调用，
   并等待同一 in-flight 计数归零；等待期间不持有 manager、storage 或 Timer 锁；
3. 等待 `PluginDataCallGate` 的既有 Runtime/Window data lease 归零；等待期间同样不持有 manager、Window
   controller 或 storage 锁；已取得 Window call lease 但尚未取得 data lease 的调用会被关闭的 gate 拒绝；
4. 卸载事务重新取得 mutation gate 并验证仍为当前 owner；持久化提交“已卸载 + plugin owner cleanup pending”
   `PluginOwnerCleanupReceipt`，发布无 ActivationBundle 状态，然后释放 mutation gate；该提交是不可回滚的
   卸载线性化点；
5. 销毁 Runtime 与插件窗口；旧 generation/ActivationBundle、facade、request 和 lease 永久失效；
6. 执行 receipt 的完整 owner cleanup：普通 storage 内存记录与目录、secret owner、卸载后的 state owner、已安装
   package tree 和窗口位置；只有每个必需目标都幂等删除成功并持久化移除 receipt 后，命令才报告彻底卸载完成。

receipt 持久化在所有待清理 owner root 之外，只保存 manager 已验证的 pluginId、卸载事务 ID、
generation/activation 身份和固定 root 下可重建的目标标识，不接受调用者路径。每次重试可重复执行全部目标；目标
不存在视为成功。任一目标失败时 receipt 整体保留，不得仅因 storage 已删除而解除同 ID 阻塞。Timer、延迟消息
和 Runtime fault 等仅存内存的 generation 资源在卸载提交时撤销，不属于磁盘 cleanup receipt。

选择保留数据时仍执行准入关闭和两个 drain，但不写 `PluginOwnerCleanupReceipt`；只保留普通 storage、secret 和
既有保留数据合同要求的 state 配置，package tree、Runtime/window 与窗口位置仍按既有保留数据卸载流程清理。
若 drain 或持久卸载提交在线性化点前失败，事务关闭状态不得留下一扇可继续调用的旧窗口或 Runtime：销毁当前
实例，并只为重新验证后的当前 ActivationBundle 以新 admission epoch 恢复准入；插件记录保持已安装，用户下次
调用时建立新 Runtime/window session。旧 facade、旧 request 和旧 lease 不能进入新 epoch。

若完全卸载已提交而任一 owner cleanup 或 receipt 清除失败，不得恢复旧插件。内部管理错误变体为
`PublicPluginManagementError::DataCleanupPending`，Tauri `CommandError.code` 固定序列化为
`dataCleanupPending`；卸载命令以该错误结束而不是返回成功。设置页必须结束 loading、刷新 inventory（插件已从
列表消失），并在页面级显示“插件已卸载，数据清理将在下次启动时重试”，不得映射为“操作不可用”。启动时必须
先重试 receipt 清理；同一 `pluginId` 的安装、更新和激活在 receipt 清除前被拒绝。WebView 重载不算启动重试。

任何路径都不得同时持有 plugin mutation gate、Window controller mutex、data gate mutex 或 storage mutex 等待
另一个锁；尤其不得持有 mutation/scheduler/data gate mutex 等待 Window 或 data lease 归零。

## 7. Pomodoro 窗口状态

Pomodoro 窗口维护四个互不混淆的值：

- `effectiveDurationMinutes`：始终有效的下一轮时长，初始化为 10；
- `persistedDurationMinutes`：最后一次确认保存或成功读取的合法值；缺失、读取失败或非法值时为 `null`；
- `pendingDurationMinutes`：当前等待保存的选择，可为空；
- Timer state：宿主返回的当前轮权威状态及 revision。

窗口每次 `onUpdate` 都创建新的本地 `viewEpoch`，先订阅 Timer，再并行读取 Timer 基准状态和持久时长。任何
异步完成只有在 epoch 与当前会话相同时才能更新 DOM 或错误文本。新的 `onUpdate` 使旧读取、旧保存和旧 Timer
完成失去 UI 所有权。

成功读取合法值或保存成功时同时更新 effective 与 persisted；key 缺失、读取失败或非法值时 effective 保持 10、
persisted 保持 `null`，不得声称 10 已落盘。保存失败时 effective 恢复到
`persistedDurationMinutes ?? 10`。

running、paused 和 fired 的 `durationMs/remainingMs` 始终来自宿主 Timer。idle 阶段的大号时间明确显示
`effectiveDurationMinutes`，即使 `timer.reset()` 的权威 idle record 仍保留上一轮 duration；选择器显示的也始终
是 effective 下一轮值。新一轮从 idle 或 fired 启动时，把 `effectiveDurationMinutes * 60000` 与当前完成消息
一起传给 `timer.start()`。继续 paused 轮次仍调用无参数 `timer.start()`。

`pendingDurationMinutes` 非空时禁用 idle 的“开始”和 fired 的“重新开始”，直到保存成功或失败完成，避免用旧
effective 值启动新一轮；paused 的“继续”保持可用，因为它恢复当前轮且不读取下一轮时长。

## 8. 失败行为

- `get` 存储失败：effective 保持 10、persisted 为 `null`，显示读取错误；用户仍可选择，后续 `set` 独立尝试。
- `set` 失败：恢复 `persistedDurationMinutes ?? 10`，不改变当前 Timer，并显示保存错误。
- `remove` 不由 Pomodoro UI 使用，只作为通用窗口 API 与 Runtime storage 对称提供。
- 非法持久值：静默回退 10 分钟，不自动覆盖；这不是插件运行故障。
- malformed caller、stale session 与后端身份不一致按第 4 节固定映射，零存储访问。
- 配额、序列化或原子文件失败：返回既有存储错误，旧值保持不变，不停用插件。
- 已提交写入后的迟到 UI 完成：数据继续有效，但旧 epoch 不得更新新窗口 UI。
- 完全卸载与已签发写 lease 竞态：卸载等待写入完成后再删除，最终目录与内存记录均不存在。
- 完全卸载提交后的任一 owner 清理失败：插件保持已卸载，返回 `dataCleanupPending`；启动重试成功前同 ID 不能
  安装或激活，设置页结束 loading、刷新 inventory 并显示固定页面级提示。

任何存储失败都不停止、重置、完成或取消 Timer，也不影响消息、Toast、托盘和闹铃。

## 9. 测试合同

### 9.1 Rust

- Prepared `get` 成功而 Prepared `set/remove` 拒绝；Active 三项成功；main、find、Runtime、其他窗口及伪造
  label 零访问。
- 旧 session、旧 generation、旧 activation 和关闭中会话被拒绝；保存的旧 facade 不能跨会话使用。
- 窗口与 Runtime 读取同一插件存储；不同插件相同 key 互不影响。
- 升级后值保留；保留数据卸载再安装后值恢复；彻底卸载后为空。
- 分别暂停已签发的 Window write 与 Runtime write，再并发彻底卸载；两组测试都证明卸载等待 data lease 后
  提交并删除，旧调用不能重建目录；已取得 Window lease 但未取得 data lease 的调用被拒绝。
- Runtime 持 current guard 并进入 data gate 时与卸载并发，证明固定锁顺序无死锁；旧 Runtime request 不能跨
  admission epoch 获取 lease，卸载不得持 data gate 或 scheduler mutex 等待 data lease。
- drain/持久提交在线性化点前失败时销毁旧实例并以新 admission epoch 恢复已安装插件；持久卸载提交后分别模拟
  storage 成功但 secret、state owner、package tree 或窗口位置清理失败，证明插件不恢复、receipt 保留且同 ID
  安装/激活被阻止；重启幂等重试全部目标成功后 receipt 消失。
- Runtime 与 Window 共同拒绝不匹配统一正则或 `__proto__`/`prototype`/`constructor` 的 key；非法 key/value 映射
  `InvalidOperation`，5 MiB 配额和原子提交失败映射 `StorageError` 且保持旧值。
- 存储调用不改变 timer revision，不持有窗口/Timer 锁跨文件 I/O。
- 生成命令权限与动态内容窗口 capability 只开放新增的三个窄命令。

### 9.2 Bootstrap 与 SDK

- `storage` facade 及方法被冻结；函数捕获当前 session generation，而不是从插件参数读取身份。
- 会话替换后旧 facade 返回 `ExpiredWindowSessionError`。
- TypeScript 严格合同包含 `get/set/remove` 与 `JsonValue`，Demo SDK contract 继续通过。

### 9.3 Pomodoro

- DOM 包含内容区右上角选择器，选项顺序、标签和值准确，初值为 10。
- 成功读取恢复 10/15/25/30/45；缺失、读取失败和非法值使 effective 为 10、persisted 为 `null`。
- 保存期间禁用；成功同时更新 effective/persisted；失败恢复 persisted 或默认 10 并显示固定错误。
- running/paused 中选择不改变当前状态、remaining 或 revision；下一轮使用新毫秒数。
- idle 大号时间显示 effective 下一轮值；running/paused/fired 显示宿主当前轮权威值。
- 选择 25 后保存未完成时 idle/fired 不能启动新轮；保存成功后启动 25 分钟；保存失败后按恢复值启动。
- paused 在保存未完成时仍可继续当前轮，且不读取 pending/effective 下一轮时长。
- paused 的继续无输入；idle/fired 的新轮使用持久时长。
- 新 `onUpdate` 后旧读写完成不改变 DOM；当前 Timer 恢复与选择器持久值可同时正确显示。

### 9.4 管理界面

- 卸载返回 `dataCleanupPending` 时设置页结束 loading、刷新 inventory、移除插件行并显示固定页面级提示；不显示
  “操作不可用”。

## 10. 人工验收

人工验收必须由用户操作，Agent 不控制鼠标或键盘：

1. 安装并启用最新 Pomodoro，打开窗口，确认右上角默认显示 10 分钟。
2. 选择 15 分钟，关闭并重新打开窗口，仍显示 15 分钟。
3. 重启 UiPilot 后打开窗口，仍显示 15 分钟。
4. 开始 15 分钟轮次，运行中改为 25 分钟；当前轮继续 15 分钟，下一轮以 25 分钟开始。
5. 升级插件后选择仍保留。
6. 卸载时选择保留数据，再安装后选择恢复；彻底卸载后重装恢复 10 分钟。
7. 人为触发存储失败的自动化证据通过即可；不要求用户破坏本机数据文件。

## 11. 验收标准

1. 五个固定分钟选项、默认值、位置和下一轮语义符合用户合同。
2. 选择跨关闭、重启、升级和保留数据重装持久化。
3. 公开窗口存储 API 与现有插件存储共享作用域和原子/配额合同，但受窗口会话额外约束。
4. 当前 Timer 状态、revision、消息和闹铃不被偏好读写改变。
5. 失败回退、乱序完成和旧会话全部有自动化覆盖。
6. 完全卸载的准入关闭、双 drain、持久提交、数据清理与启动重试形成可恢复的单向事务。
7. SDK 合同、公开插件测试、Rust 构建及 Pomodoro 示例测试通过。
