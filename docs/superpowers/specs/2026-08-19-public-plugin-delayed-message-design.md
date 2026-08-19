# UiPilot 公开插件宿主延迟消息设计

## 1. 文档信息

- 日期：2026-08-19
- 状态：Draft；对话设计已确认，等待用户书面复核
- 产品阶段：公开插件平台后续 MVP
- 目标平台：Windows 11 x64
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 验收插件：`com.uipilot.demo-win`

## 2. 目标与范围

公开插件可以在一次仍有效的命令请求中，向宿主登记一条延迟纯文本消息。宿主接管任务后，插件立即
返回正常结果；即使主窗口和插件子窗口随后隐藏，UiPilot 仍在约定时间到期后通过现有消息中心发布
该消息。

本设计只增加宿主持有的单次延迟消息，不允许插件代码在请求结束后继续运行，也不引入常驻 Runtime、
通用后台任务、重复计时器或跨进程恢复。

## 3. 与既有设计的关系

本设计建立在
[公开插件消息中心与 Windows 通知](./2026-08-18-public-plugin-message-center-design.md)之上，并窄化地
替代其中“请求结束后的主动推送和定时任务不可用”的非目标：

- `notifications.publish()` 仍是当前请求内立即持久化一条消息；
- 新增 `notifications.schedule()`，只登记一条由宿主稍后发布的纯文本消息；
- `background.schedule`、插件后台代码、任意回调和跨重启任务仍不可用；
- 既有权限、消息内容、消息存储、Windows 通知、托盘、徽标和 unavailable 吸收终态合同不变。

## 4. 用户合同

当用户执行 `/demo-win str` 时：

1. 插件计算与子窗口 `Return text` 相同的消息正文；
2. 插件向宿主登记一条延迟 10 秒的消息；
3. 登记成功后，插件立即返回窗口响应，子窗口立即显示；
4. 主窗口和子窗口隐藏或关闭不取消任务；
5. 约 10 秒后，消息进入现有消息中心，并触发 Windows 通知、托盘提醒和未读徽标；
6. 连续执行多次命令时，每个请求分别登记和发送一条消息。

从托盘彻底退出 UiPilot 会丢弃尚未到期的任务。重新启动后不恢复这些任务。

## 5. 公开 API 合同

```ts
export interface PluginNotificationScheduleInput {
  readonly content: string
  readonly delayMs: number
}

export interface UiPilotPluginApiV1 {
  readonly notifications: {
    publish(input: Readonly<PluginNotificationPublishInput>): Promise<void>
    schedule(input: Readonly<PluginNotificationScheduleInput>): Promise<void>
  }
}
```

`schedule()` 的固定规则：

- 只在 Windows、清单声明并获准 `notifications.publish` 时可用，不新增权限名；
- 输入对象只允许 `content` 和 `delayMs`，未知字段拒绝；
- `content` 完全复用 `publish()` 的纯文本校验：去除边界空白后非空、最多 500 个 Unicode 标量值、
  不含换行、NUL 或其他控制字符，且不解析 HTML、Markdown、URL、图片或动作；
- `delayMs` 必须是 JavaScript 安全整数，范围为 `1_000..=86_400_000`；
- 每个插件最多同时拥有 32 条等待任务；
- `schedule()` Promise 在宿主接管任务后决议，不等待消息到期或最终发布；
- 一个请求最多成功提交一次通知动作；`publish()` 与 `schedule()` 共享该限制。为保持既有错误合同，
  第二次提交仍返回 `AlreadyPublished`，不产生额外消息或任务。

`Object.freeze()` 仍只防止前端误改。Rust 必须重新校验 API 门面携带的
`(pluginId, pluginGeneration, requestId)`、caller、权限、输入和当前请求所有权。

## 6. 调度任务模型

宿主在内存中保存：

```text
ScheduledPluginMessage
  scheduleId
  pluginId
  pluginGeneration
  pluginNameSnapshot
  requestId
  content
  dueAtMonotonic
```

- `scheduleId` 仅供宿主去重和诊断，不暴露给插件；
- `pluginNameSnapshot` 在登记时从已验证 generation 的 Manifest 获取；
- `dueAtMonotonic` 使用单调时钟计算，不受系统墙上时间修改影响；
- 队列按到期时间排序，由一个共享宿主调度器管理，不为每条任务建立专用线程；
- 任务只存在于当前 UiPilot 原生进程内，不写入消息存储或插件存储。

## 7. 登记线性化与请求所有权

登记顺序为：

```text
plugin schedule call
-> validate caller/context/permission/input/current request
-> verify message store is not already unavailable
-> reserve the request's single notification action
-> insert immutable task into the in-memory queue
-> release scheduler/request guards
-> resolve schedule Promise
-> plugin returns its normal window or main-result response
```

任务成功插入队列是 `schedule()` 的不可撤销成功线性化点：

- 请求在此之前失效时，返回 `ExpiredRequestError`，不登记任务；
- 插入成功后，新请求淘汰旧请求不取消已接管任务，也不能把成功改报为过期；
- 旧请求返回的 UI 结果仍受既有提交所有权约束；调度任务与 UI 结果是否仍可提交相互独立；
- 插入失败时必须释放通知动作预留，不能消耗请求额度或留下半任务；
- API Promise、Runtime 或 WebView 在插入后异常终止，不撤销已接管任务。

调度器锁、插件请求守卫和插件管理锁不得跨越消息持久化、原生通知、托盘操作或前端事件派发。

## 8. 到期、取消与竞态

到期时，宿主从队列中原子取出任务并释放队列锁，再验证插件仍已安装、已启用且 generation 与任务
一致。验证成功后，任务调用现有消息中心提交和派发路径。

固定生命周期规则：

- 隐藏或关闭主窗口、设置页、消息页或插件子窗口不取消任务；
- 每个普通新请求拥有独立任务，后来的请求不替换较早任务；
- 禁用、卸载或更新插件时，宿主主动删除该插件旧 generation 的等待任务；
- 到期领取与插件生命周期变化竞态时，以到期资格复核为线性化点：复核时已失效则取消；复核成功后
  开始的生命周期变化不撤回正在提交的消息；
- UiPilot 原生进程退出时，调度器停止并丢弃全部等待任务；
- Windows 休眠覆盖到期时间时，进程恢复后立即尝试领取已到期任务；
- 同一任务最多被领取一次，不能因唤醒、窗口重载或事件失败重复发布。

## 9. 发布与失败行为

到期发布完全复用既有消息中心合同：消息先原子持久化，再派发 main-only 状态事件、Windows 通知和
托盘提醒。

固定错误语义：

| 错误 | 含义 |
| --- | --- |
| `PermissionDenied` | 清单未声明或用户未授权消息权限 |
| `InvalidNotification` | `content` 不符合既有纯文本合同 |
| `InvalidDelay` | `delayMs` 类型、整数性或范围非法 |
| `ScheduleLimitExceeded` | 插件已有 32 条等待任务 |
| `AlreadyPublished` | 当前请求已经成功提交过立即或延迟通知动作 |
| `InvalidContext` | caller、身份形状或三元组伪造 |
| `ExpiredRequestError` | 请求或 generation 在登记线性化点前已失效 |
| `MessageStoreUnavailable` | 登记时已知消息存储处于 unavailable |

登记成功后的到期失败不能再回传给已经完成的插件请求：

- 插件在到期前失效：静默取消并记录受控诊断；
- 普通消息原子写入失败：丢弃本任务并记录错误，MVP 不自动重试，避免重复消息；
- 首次转入消息存储 unavailable：沿用既有 unavailable 状态事件和前端吸收终态；
- 消息已持久化但 Windows 通知、托盘或前端事件失败：消息仍成功，不回滚、不重试消息提交；
- 延迟失败不得重新打开主窗口、关闭插件子窗口或向已经被新输入拥有的主界面写入错误。

## 10. `demo-win` 修改

`com.uipilot.demo-win` 版本从 `1.0.3` 递增到 `1.0.4`，权限不变。Runtime 改为：

```js
export async function onCommand(invocation, api) {
  const returnText = createReturnText(invocation)
  await api.notifications.schedule({
    content: returnText,
    delayMs: 10_000,
  })
  return {
    requestId: invocation.requestId,
    data: { returnText },
  }
}
```

`schedule()` 只等待宿主接管，因此窗口响应不等待 10 秒。`demo-return` 不变。

## 11. 测试与验收

自动化测试使用可控时钟，不真实等待 10 秒或 24 小时：

- `1_000`、`10_000`、`86_400_000` 合法；零、负数、非整数、非安全整数和超过上限拒绝；
- caller、权限、身份、当前请求和共享单通知动作限制；
- 插入前请求失效不留任务，插入后请求淘汰不撤销任务；
- 每插件 32 条上限及失败不泄漏额度；
- 多请求任务独立、按期领取且各自最多一次；
- 窗口隐藏不参与取消；插件禁用、卸载和 generation 更新取消等待任务；
- 到期与 generation 变化竞态符合资格复核线性化点；
- 休眠式时间前进后立即领取到期任务；
- 普通存储失败不重试，unavailable 转换和原生副作用失败复用既有合同；
- SDK 类型、bootstrap 深冻结、API 请求序列化和 Demo mock 精确匹配新 DTO；
- `demo-win` 立即返回窗口数据，并只登记一次 10 秒延迟消息。

人工验收由用户操作，自动化不得控制鼠标、键盘或真实前台焦点：

1. 安装或更新 `demo-win`，执行 `/demo-win str`；
2. 确认子窗口立即出现，内容正确；
3. 隐藏主窗口并关闭子窗口；
4. 等待约 10 秒，确认 Windows 通知、托盘提醒和消息未读徽标出现；
5. 连续执行两次，确认最终产生两条独立消息；
6. 登记任务后禁用或更新插件，确认旧任务不发送。

## 12. 验收标准

1. `notifications.schedule()` 只接管一条合法、已授权、当前请求的纯文本延迟消息。
2. 合法延迟为 1 秒到 24 小时，每插件最多 32 条等待任务。
3. `/demo-win` 子窗口立即显示，消息约 10 秒后发送；隐藏两个窗口不影响发送。
4. 每次命令独立产生一条任务，后来的请求不淘汰已接管任务。
5. 插件禁用、卸载或更新取消旧 generation 等待任务；进程退出后不恢复。
6. 到期消息最多提交一次，失败不影响已完成窗口，也不劫持当前主界面。
7. 现有消息中心持久化、unavailable、Windows 通知、托盘和徽标合同没有回归。

## 13. 非目标

- 番茄时钟产品功能或任何新的插件 UI；
- `background.schedule`、常驻 Runtime、请求结束后的插件代码或任意后台回调；
- 周期任务、日历时间、跨重启恢复、持久化任务或任务历史；
- 插件查询、取消、修改或重新安排已接管任务；
- 到期失败自动重试或送达保证；
- macOS 通知或延迟消息；
- 自动化控制用户鼠标、键盘或真实前台焦点。
