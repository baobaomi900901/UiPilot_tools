# UiPilot Pomodoro 计时长度选择与插件窗口存储设计

## 1. 文档信息

- 日期：2026-08-22
- 状态：Draft，交互设计已确认，等待用户书面审阅
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

选择新值时立即开始原子保存，并在完成前禁用选择器。成功后该值成为下一轮时长；失败时恢复上一个已保存
值并显示“无法保存计时长度”。当前 `running` 或 `paused` 轮次不被重置、不改变剩余时间。暂停后的“继续”
恢复同一轮；从 `idle` 或 `fired` 点击“开始/重新开始”时才把最新保存值传给 `timer.start()`。

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
命名空间及以下既有合同：

- key 匹配 `^[a-z][a-z0-9.-]{0,63}$`；
- value 必须是受支持的有限 JSON 值；
- 单插件总配额为 5 MiB；
- `set`、`remove` 使用现有原子持久化，失败保留旧内存值和磁盘值；
- 升级保留数据；卸载选择“保留数据”后，同一 `pluginId` 重装继续读取；彻底卸载删除数据。

公开错误语义不新增枚举：失效或伪造的窗口会话使用 `ExpiredWindowSessionError`；非法 key/value 使用
`InvalidOperation`；配额、序列化、磁盘或原子提交失败使用 `StorageError`。错误消息不包含插件数据、key、
路径、generation、activationId 或 session generation。

Pomodoro 固定使用小写 key `pomodoro.duration-minutes`，只接受数值 `10 | 15 | 25 | 30 | 45`。读取到其他
JSON 类型、非整数或名单外整数时回退 10，但不自动写回；只有用户主动选择才覆盖旧值。

## 5. 身份、会话与授权

每个窗口存储调用携带当前 bootstrap 闭包冻结的 `sessionGeneration`。Rust 以后端事实重新解析调用者内容
WebView label，并通过 `PluginWindowController` 取得当前 `PluginWindowOwner`。授权必须同时满足：

1. caller 是当前内容 WebView，而不是 main、find、Runtime 或其他插件窗口；
2. `sessionGeneration` 是规范十进制 u64 且等于当前活动窗口会话；
3. owner 的 `pluginId + pluginGeneration + activationId` 仍属于当前已启用、无故障 ActivationBundle；
4. 存储作用域只从后端 owner 的 `pluginId` 派生，不接受 JavaScript 传入插件 ID 或磁盘路径。

会话只在 `onUpdate` 建立后允许存储调用。窗口隐藏、会话替换、插件禁用、故障停用、升级、卸载或 Runtime/
内容重建后，旧门面调用返回 `ExpiredWindowSessionError`。插件保存旧 facade 不能跨会话继续读写。

`window.uipilotPluginWindow`、`timer`、`storage` 及其方法继续深度冻结。bootstrap 不向插件暴露 Tauri internals、
真实命令名、路径、generation、activationId 或其他所有权令牌。

## 6. 后端命令与锁边界

新增窄命令分别处理窗口存储 `get/set/remove`。每条命令遵循：

1. 解析并验证 session generation；
2. 在 `PluginWindowController` 内建立一次活动调用 lease，复制后端 owner，然后释放 controller mutex；
3. 依据 owner 重新验证当前 ActivationBundle；
4. 不持有 controller、ActivationBundle、Timer 或消息中心锁地调用现有 `PluginStorageStore`；
5. 返回固定成功值或稳定错误，最后释放 lease。

存储 I/O 不取得 Timer 锁，不发布 Timer 状态事件，也不修改 inventory revision。窗口进入 closing 时阻止新调用，
但已经越过授权线性化点的原子写可以完成；前端仍必须用会话 epoch 丢弃迟到 UI 完成。

## 7. Pomodoro 窗口状态

Pomodoro 窗口维护三个互不混淆的值：

- `persistedDurationMinutes`：最后一次确认保存或成功读取的下一轮时长；
- `pendingDurationMinutes`：当前等待保存的选择，可为空；
- Timer state：宿主返回的当前轮权威状态及 revision。

窗口每次 `onUpdate` 都创建新的本地 `viewEpoch`，先订阅 Timer，再并行读取 Timer 基准状态和持久时长。任何
异步完成只有在 epoch 与当前会话相同时才能更新 DOM 或错误文本。新的 `onUpdate` 使旧读取、旧保存和旧 Timer
完成失去 UI 所有权。

当前轮的 `durationMs` 和 `remainingMs` 始终来自宿主 Timer。选择器不得用新的偏好覆盖运行、暂停或 fired 状态
的权威倒计时。新一轮从 `idle` 或 `fired` 启动时，把 `persistedDurationMinutes * 60000` 与当前完成消息一起
传给 `timer.start()`。继续 paused 轮次仍调用无参数 `timer.start()`。

## 8. 失败行为

- `get` 存储失败：显示 10 分钟和读取错误；用户仍可选择，后续 `set` 独立尝试。
- `set` 失败：恢复 `persistedDurationMinutes`，不改变当前 Timer，并显示保存错误。
- `remove` 不由 Pomodoro UI 使用，只作为通用窗口 API 与 Runtime storage 对称提供。
- 非法持久值：静默回退 10 分钟，不自动覆盖；这不是插件运行故障。
- stale/forged caller：返回 `ExpiredWindowSessionError` 或既有固定上下文错误，零存储访问。
- 配额、序列化或原子文件失败：返回既有存储错误，旧值保持不变，不停用插件。
- 已提交写入后的迟到 UI 完成：数据继续有效，但旧 epoch 不得更新新窗口 UI。

任何存储失败都不停止、重置、完成或取消 Timer，也不影响消息、Toast、托盘和闹铃。

## 9. 测试合同

### 9.1 Rust

- 当前内容 label 和 session 可在本插件作用域读、写、删除；main、find、Runtime、其他窗口及伪造 label 零访问。
- 旧 session、旧 generation、旧 activation 和关闭中会话被拒绝；保存的旧 facade 不能跨会话使用。
- 窗口与 Runtime 读取同一插件存储；不同插件相同 key 互不影响。
- 升级后值保留；保留数据卸载再安装后值恢复；彻底卸载后为空。
- 非法 key/value、5 MiB 配额和原子提交失败保持旧值。
- 存储调用不改变 timer revision，不持有窗口/Timer 锁跨文件 I/O。
- 生成命令权限与动态内容窗口 capability 只开放新增的三个窄命令。

### 9.2 Bootstrap 与 SDK

- `storage` facade 及方法被冻结；函数捕获当前 session generation，而不是从插件参数读取身份。
- 会话替换后旧 facade 返回 `ExpiredWindowSessionError`。
- TypeScript 严格合同包含 `get/set/remove` 与 `JsonValue`，Demo SDK contract 继续通过。

### 9.3 Pomodoro

- DOM 包含内容区右上角选择器，选项顺序、标签和值准确，初值为 10。
- 成功读取恢复 10/15/25/30/45；缺失和非法值回退 10。
- 保存期间禁用；成功更新下一轮；失败恢复旧值并显示固定错误。
- running/paused 中选择不改变当前状态、remaining 或 revision；下一轮使用新毫秒数。
- paused 的继续无输入；idle/fired 的新轮使用持久时长。
- 新 `onUpdate` 后旧读写完成不改变 DOM；当前 Timer 恢复与选择器持久值可同时正确显示。

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
6. SDK 合同、公开插件测试、Rust 构建及 Pomodoro 示例测试通过。
