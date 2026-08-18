# UiPilot 公开插件消息中心与 Windows 通知设计

## 1. 文档信息

- 日期：2026-08-18
- 状态：Approved in conversation; written review pending
- 产品阶段：公开插件平台后续 MVP
- 目标平台：Windows 11 x64
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 验收插件：`com.uipilot.demo-win`

## 2. 目标

为公开插件增加一个请求期消息发布能力。插件在有效的 `onCommand` 调用中发布一条纯文本消息后，
UiPilot 负责持久化消息、维护未读状态、显示 Windows 通知、短时闪烁托盘图标，并在主窗口设置按钮上
显示未读徽标。

本设计不引入常驻插件、定时器或后台调度。插件只能在一次仍然有效的用户命令调用中发布消息。

## 3. 与既有设计的关系

本设计建立在以下合同上：

- [公开插件命令与单窗口 MVP](./2026-08-13-public-plugin-command-window-mvp-design.md) 继续定义插件包、
  Runtime 身份、请求调度、权限确认、单窗口和失败隔离；
- [主窗口与设置页导航](./2026-08-18-main-settings-navigation-design.md) 继续定义同一主窗口内的视图切换；
- [公开插件 PNG 图标](./2026-08-18-public-plugin-png-icon-design.md) 继续定义插件图标的宿主读取和回退；
- [公开插件 API v1](../../plugin-sdk/public-plugin-v1.md) 继续是第三方接口基线。

本设计只覆盖旧公开插件 MVP 中以下既有非目标：

- `notifications.publish` 在 Windows 宿主上从“已知但不可用”变为可安装、可授权和可调用；
- 新增宿主管理的消息中心、未读徽标、Windows 通知和托盘短时闪烁。

`background.schedule`、常驻 Runtime、定时任务和请求结束后的主动推送仍然不可用。其他既有权限、
请求所有权、generation、安装原子性、窗口交接和 UI 焦点合同不变。

## 4. 用户合同

### 4.1 插件发布消息

`demo-win` 收到 `/demo-win str` 后：

1. Runtime 计算与子窗口 `Return text` 相同的文本；
2. Runtime 在当前请求内向宿主发布一次该文本；
3. 宿主持久化消息；
4. Runtime 返回原有窗口响应；
5. UiPilot 按原有单窗口流程创建或更新并激活 `demo-win` 子窗口。

消息持久化失败时，本次插件命令整体失败，不创建或更新子窗口。消息已经持久化后，即使 Windows 通知
或托盘闪烁失败，插件命令和子窗口仍然成功。

### 4.2 消息页

设置页签按以下顺序显示：

```text
通用
消息
插件
```

“消息”页按最新优先显示最近 100 条记录。每条记录显示：

- 发布时保存的插件名称；
- 插件当前仍安装时显示当前版本的插件图标，否则显示默认插件图标；
- 宿主生成并按本地时区格式化的时间；
- 插件发布的纯文本内容。

没有记录时显示“暂无消息”。页面顶部只提供“清空全部”，不支持逐条删除。插件卸载后，其历史消息继续
显示发布时保存的名称，但图标回退为默认插件图标。历史记录不快照插件 generation 或图标资产；插件
升级后，既有记录随当前安装版本显示新图标。

进入“消息”页签时，进入瞬间已经存在的消息全部标记为已读。进入操作之后到达的新消息保持未读，即使
用户仍停留在消息页，也会重新产生未读徽标。

### 4.3 未读徽标

主窗口输入框右侧的设置图标显示未读数量：

- 未读为零时不显示；
- 1 到 99 显示准确数字；
- 超过 99 显示 `99+`。

设置页关闭或主窗口隐藏时，未读状态仍由宿主持久化并在下次显示时恢复。

### 4.4 Windows 通知与托盘

每条成功持久化的消息都尝试显示一张 Windows 通知，不根据主窗口或插件窗口是否可见进行抑制。通知标题
使用插件名称，正文使用消息内容。

用户点击通知时，UiPilot 打开并聚焦主窗口，直接进入“设置 -> 消息”，并执行与手动进入该页相同的
已读操作。消息到达本身绝不显示、隐藏、聚焦或移动任何窗口，也不清空或改写用户当前输入。

每条消息还会启动托盘短时闪烁：正常图标和宿主自有的提醒图标每 500 毫秒切换一次，持续 6 秒。闪烁
期间收到新消息时，从新消息开始重新计时。任意时刻只有一个闪烁计时器；结束、失败或程序退出时必须
恢复正常托盘图标。

Windows 通知被系统关闭、权限被拒绝或发送失败时，已保存的消息、未读徽标和托盘闪烁不受影响。

## 5. 术语

- **消息（message）**：由一个当前公开插件请求发布、由宿主持久化的纯文本记录。
- **发布（publish）**：`api.notifications.publish()` 成功完成一次消息持久化提交。
- **未读消息（unread message）**：`readAt` 仍为 `null` 的持久化消息。
- **消息修订（message revision）**：宿主消息文档每次成功发布或产生状态变化的标记已读、清空后递增的
  持久化序号。
- **摘要修订游标（summary revision cursor）**：前端已观察到的最新消息摘要修订，只用于未读徽标和判断
  完整快照是否过期。
- **快照修订游标（snapshot revision cursor）**：前端当前消息列表对应的完整快照修订；摘要事件不能推进
  该游标。
- **发布提交点（publish commit point）**：消息文档原子替换成功的时刻。到达该点后，本次发布不可撤销
  且 `publish()` 的结果固定为成功。
- **进入截止点（open cutoff）**：打开消息页操作在线性化点观察到的最高消息 ID；只有不高于该 ID 的
  既有消息会被本次进入操作标记已读。
- **原生通知（native notification）**：宿主在消息持久化后请求 Windows 显示的系统通知卡片。

消息 ID、消息修订、插件 generation 和插件 request ID 含义不同，不能互换。

## 6. 权限与平台合同

`notifications.publish` 继续使用现有清单权限名称，不新增别名。

- Windows 宿主把 `notifications.publish` 标记为可用权限。
- 插件必须在 `plugin.json.permissions` 中显式声明它。
- 安装或更新时，用户必须像其他公开权限一样明确授权。
- 更新版本新增该权限但未获授权时，更新不能提交，旧版本继续运行。
- 没有声明或没有获准该权限的调用返回 `PermissionDenied`。
- 当前 macOS 合同仍不开放该权限；声明此权限的参考插件应把 `supportedPlatforms` 限定为 `windows`。

`demo-win` 因此把 `supportedPlatforms` 调整为 `["windows"]`。`demo-return` 不修改，也不获得消息权限。

## 7. Runtime API 合同

公开 API v1 兼容新增只读 `notifications` 门面：

```ts
interface PluginNotificationPublishInput {
  content: string
}

interface UiPilotPluginApiV1 {
  readonly storage: {
    get(key: string): Promise<JsonValue | null>
    set(key: string, value: JsonValue): Promise<void>
    remove(key: string): Promise<void>
  }
  readonly settings: {
    get(key: string): Promise<JsonValue | null>
    isSecretConfigured(key: string): Promise<boolean>
  }
  readonly notifications: {
    publish(input: Readonly<PluginNotificationPublishInput>): Promise<void>
  }
}
```

宿主 bootstrap 深度冻结 `notifications` 及其输入快照。每次调用沿用现有 API 门面的不可伪造
`(pluginId, pluginGeneration, requestId)` 身份，并在进入 Rust 以及持久化提交前重新校验。

发布输入必须满足全部条件：

- 输入对象只能包含 `content`；未知字段拒绝；
- `content` 必须是字符串；
- 去除边界空白后必须非空，但宿主保存合法原文，不重写内部空格；
- 最多 500 个 Unicode 标量值；
- 必须是单段纯文本，不允许换行、NUL 或其他控制字符；
- 不解析 HTML、Markdown、URL、图片、动作或宿主命令。

一次 request ID 最多成功发布一条消息。第二次发布返回 `AlreadyPublished`，不产生第二条记录。插件普通
新请求可以再次发布一条。Promise 已决议后，或者请求在到达发布提交点前被新请求淘汰、超时、取消、
禁用、卸载、升级、reload 或 Runtime generation 替换时，旧 API 调用返回 `ExpiredRequestError`。

宿主在持有当前请求守卫时完成最终身份复核和消息原子提交。原子文件替换成功是本次 `publish()` 的
不可撤销成功线性化点：

- 请求在提交点之前失效时，调用返回 `ExpiredRequestError`，且不能写入消息；
- 提交点已经完成后，新请求只能在当前请求守卫释放后淘汰旧请求；该淘汰不得把已提交的调用改报为
  `ExpiredRequestError`；
- 提交点之后不再为了决定 `publish()` 返回值重新校验请求身份；前端事件、Windows 通知和托盘闪烁只做
  有界的尽力派发，失败不改变成功结果；
- 旧请求随后返回的窗口或主结果仍受既有调度所有权约束，可能被新请求丢弃；这不撤销已经发布的消息。

固定错误语义：

| 错误 | 含义 |
| --- | --- |
| `PermissionDenied` | 清单未声明或用户未授权消息权限 |
| `InvalidNotification` | 输入 DTO 或内容不符合合同 |
| `AlreadyPublished` | 当前 request ID 已成功发布过一条消息 |
| `InvalidContext` | 身份形状、caller 或三元组伪造 |
| `ExpiredRequestError` | 曾有效的请求或 generation 已失效 |
| `MessageStoreUnavailable` | 消息持久化无法完成 |

## 8. 持久化模型

消息中心使用独立于插件私有存储和主设置的宿主持久化文档：

```ts
interface MessageStoreV1 {
  schema: 1
  revision: string
  nextMessageId: string
  messages: MessageRecord[]
}

interface MessageRecord {
  id: string
  pluginId: string
  pluginNameSnapshot: string
  createdAt: string
  content: string
  readAt: string | null
}
```

`revision`、`nextMessageId` 和 `id` 使用规范无前导零的十进制 `u64` 字符串，跨 Rust/JavaScript 边界时
不得转换为 JavaScript `number`。计数器耗尽时消息中心进入不可用状态，不回绕或复用 ID。
新文档的 `revision` 为 `"0"`，`nextMessageId` 为 `"1"`；首次消息 ID 为 `"1"`。ID 大小比较必须按
解析后的 `u64` 值进行，不能按字符串字典序比较。

`createdAt` 和 `readAt` 由宿主使用 UTC RFC 3339 时间生成。插件不能指定 ID、插件名称、时间或已读
状态。`pluginNameSnapshot` 来自本次已验证 generation 的 Manifest。

持久化记录不保存 `pluginGenerationSnapshot`、图标 URL 或图标字节。宿主构造消息页 DTO 时按
`pluginId` 查询当前已安装插件：查询成功则提供当前 generation 的安全图标 URL，插件已卸载或图标不可
用时提供 `null`，由前端显示默认图标。这是当前图标投影，不是发布时图标快照。

存储使用现有原子文件 current/backup 机制。发布在一个提交中完成：

1. 分配消息 ID 和下一修订；
2. 追加新消息；
3. 若超过 100 条，删除 ID 最小的旧消息；
4. 原子替换持久化文档；
5. 只有替换成功后，内存快照和未读数才对外可见。

写入失败保留旧文档、旧修订和旧未读数。启动时优先恢复有效 current，其次恢复有效 backup；两者都
无效时消息中心本次运行不可用，但不能阻止 UiPilot、文件搜索、应用搜索或没有消息权限的插件启动，也
不能静默创建空文档覆盖损坏数据。

插件禁用、故障、升级、reload 或卸载不删除历史。只有超过 100 条的自动淘汰或用户“清空全部”删除
消息。

## 9. 已读与清空线性化

打开消息页使用一个 main-caller-only 的宿主操作。该操作在同一消息存储临界区内：

1. 捕获当前最高消息 ID 作为 `open cutoff`；
2. 给所有 `id <= cutoff` 且未读的消息写入同一个宿主 `readAt`；
3. 递增修订并原子持久化；
4. 返回最新列表、修订和未读数。

在截止点之后发布的消息不属于本次标记，即使持久化完成时页面仍然打开，也保持未读。消息事件与打开
操作可以乱序到达前端；前端必须分别维护摘要修订游标和快照修订游标：

- 摘要事件修订高于摘要修订游标时更新该游标与未读徽标，并把更低修订的完整列表标记为过期；等于当前
  游标的重复事件幂等忽略，低于当前游标的事件丢弃；摘要事件不能推进快照修订游标；
- 完整快照的修订低于摘要修订游标时必须丢弃，并立即重新读取；
- 完整快照的修订等于摘要修订游标时必须接受，以便摘要事件先到后，同修订的列表、空列表或清空结果
  仍能补全界面；
- 完整快照的修订高于摘要修订游标时同时推进两个游标；同修订完整快照可幂等替换当前列表。

如果截止点前没有未读消息，打开操作直接返回当前快照，不写文件也不递增修订。

“清空全部”也是 main-caller-only 原子操作：删除当前全部消息、递增修订并返回空快照。清空线性化点
之后发布的新消息必须保留且为未读。空列表上的清空是无状态变化的成功，不写文件也不递增修订。

## 10. 宿主内部接口与事件

宿主提供窄的主窗口命令，不把全量设置读写能力扩大给插件窗口：

- 读取消息摘要，用于启动器徽标；
- 打开消息中心并标记截止点前消息已读；
- 清空全部消息；
- 在消息页已打开时重新读取最新快照。

跨 Rust/TypeScript 的 DTO 固定为：

```ts
type U64Decimal = string

interface MessageSummaryDto {
  revision: U64Decimal
  unreadCount: number
}

interface MessageViewDto {
  id: U64Decimal
  pluginId: string
  pluginNameSnapshot: string
  pluginIconUrl: string | null
  createdAt: string
  content: string
  readAt: string | null
}

interface MessageCenterSnapshotDto extends MessageSummaryDto {
  messages: MessageViewDto[]
}

interface MessageSummaryChangedEvent extends MessageSummaryDto {}
```

`revision` 和 `id` 必须是规范无前导零的十进制 `u64` 字符串。`unreadCount` 必须是 0 到 100 的
JavaScript 整数，不使用字符串。读取摘要返回 `MessageSummaryDto`；打开、清空和重新读取返回完整
`MessageCenterSnapshotDto`。

消息存储具有 `ready` 和 `unavailable` 两种会话状态。current/backup 都损坏或计数器耗尽时进入
`unavailable`；所有消息管理命令统一拒绝并返回固定错误码 `MessageStoreUnavailable`，不能返回空数组或
未读数 `0` 来伪装成功。前端必须把该错误映射为独立的“消息不可用”状态。

消息提交、已读和清空后，宿主向 `main` 发出只含消息修订和未读数的受控事件。事件不是消息持久化的
完成条件，也不是完整列表快照；它只是一条摘要更新和快照失效提示。前端丢失事件时，下次读取摘要或
打开消息页必须从持久化状态恢复。插件 Runtime、插件窗口、`find` 窗口和其他 label 不能订阅消息正文
或调用消息管理命令。

Windows 通知点击通过现有主窗口生命周期协调器增加一个明确的“设置/消息”显示目标。进程正在运行时，
点击动作复用 readiness、show、focus 和视图路由，不合成输入。干净退出时，宿主取消仍由 UiPilot 拥有的
活动通知；本阶段不承诺用户退出进程后通过旧通知冷启动应用。

## 11. 锁顺序与副作用顺序

消息发布沿用 Runtime API 的当前请求守卫。固定顺序为：

```text
request scheduler current guard
-> message store lock
-> atomic file commit [publish success linearization point]
-> release message store lock
-> release request guard
-> frontend event
-> Windows notification
-> tray flash request
```

消息存储代码不能反向获取 scheduler、插件管理或窗口生命周期锁。任何锁都不能跨越 frontend emit、
Windows 通知、托盘图标操作、窗口 show/focus 或等待前端 ack。

Windows 通知、前端事件或托盘副作用失败不回滚已持久化消息。消息持久化失败则不调用任何这些副作用。
三个宿主副作用相互独立，一个失败不能阻止后续副作用。`publish()` 在持久化成功并完成这些有界派发尝试
后返回成功，不等待用户看到、点击或关闭 Windows 通知。请求守卫释放后发生的淘汰不再参与返回值判定，
因此不允许出现“消息已保存但 `publish()` 返回 `ExpiredRequestError`”的部分失败。

## 12. UI 设计

设置页继续使用现有垂直 Tabs、主题令牌和 OverlayScrollbars。新增 `messages` 页签，不创建新原生窗口。

消息页是一个可滚动的非卡片嵌套列表。每行具有稳定图标尺寸和三块内容：插件名称、时间、正文。正文
允许正常换行显示，但不解释任何标记。插件当前仍安装且当前图标 URL 可用时使用现有 `PluginIcon`；
插件升级后历史记录使用升级后的当前图标，卸载或图标加载失败时使用默认插件图标。

“清空全部”是明确文字命令，只在列表非空且没有清空操作进行时可用。操作期间不关闭设置页；失败时
保留列表并显示固定的宿主错误，不泄露磁盘路径。

消息存储不可用时，消息页显示独立的“消息不可用，请重试”状态和重试命令，禁用“清空全部”，不能显示
“暂无消息”。摘要读取失败时，设置按钮显示固定尺寸的非数字 `!` 状态徽标，而不是隐藏徽标或显示未读
为零；后续主窗口显示或用户重试时重新读取宿主状态。已有的最后成功列表不得被伪装成当前空列表。

启动器设置按钮使用现有图标按钮，并在固定尺寸容器内叠加未读徽标，避免徽标出现时改变输入框尺寸。
徽标不遮挡设置图标的可点击区域和焦点轮廓。

## 13. Windows 通知与托盘适配器

原生通知由宿主自有的 Windows 适配器统一发送，插件不能直接访问 Tauri notification API。本阶段不使用
官方 Tauri notification 插件：其桌面接口不能同时满足 Windows 点击回调、活动通知取消和异步失败观测
合同。适配器直接通过项目已有的 `windows` crate 使用 WinRT `Windows.UI.Notifications` 能力，并保持
其余代码只依赖宿主定义的窄 trait。

适配器固定职责如下：

- 使用 `ToastNotificationManager` 为 UiPilot 的已安装应用身份创建 `ToastNotifier`；
- 在发送前读取 `ToastNotifier.Setting()`，系统禁用通知时返回可诊断的受控失败；
- 为每条通知创建 `ToastNotification`，在 `Show()` 前注册 `Activated`、`Failed` 和 `Dismissed` 处理器；
- `Show()` 的同步错误和 `Failed` 的异步错误都记录脱敏诊断，但不能回滚消息；
- `Activated` 只产生固定的“打开设置消息页”宿主意图；payload 只含不透明消息 ID，不能选择任意窗口、
  命令或 URL；
- 在通知关闭、失败或激活后移除事件处理器和活动句柄；干净退出时对仍由本进程持有的通知调用 `Hide()`
  做尽力取消，然后释放处理器；取消失败只记录诊断。

宿主必须使用与打包配置一致的 Windows 应用身份/AUMID。`tauri dev` 没有正式安装身份，只能验证消息
持久化、前端路由、托盘和通知适配器的开发态冒烟行为，不能作为生产通知标题、图标、点击或取消合同的
验收证据。上述原生合同必须再用普通权限安装的打包产物验收。

操作系统通知被禁用、`Show()`/`Failed` 错误或 `Hide()` 失败时记录脱敏诊断，但 `publish()` 仍以持久化
成功返回。消息内容不得写入普通诊断日志。

托盘适配器只接受“开始或重新开始 6 秒提醒”和“恢复正常图标”两种意图。新消息替换旧截止时间，不创建
新线程或叠加计时器。退出、托盘重建和适配器错误都执行恢复正常图标的幂等清理。

消息到达路径不得调用主窗口 show/focus。只有经过验证的用户通知点击动作可以请求显示“设置/消息”。

## 14. `demo-win` 参考插件

`com.uipilot.demo-win` 从当前 `1.0.2` 递增到 `1.0.3`。
Manifest 做以下修改：

- `supportedPlatforms` 为 `["windows"]`；
- `permissions` 同时包含 `ui.window` 和 `notifications.publish`；
- 其他命令、窗口、图标和设置合同不变。

Runtime 参考逻辑：

```js
export async function onCommand(invocation, api) {
  const returnText = createReturnText(invocation)
  await api.notifications.publish({ content: returnText })
  return {
    requestId: invocation.requestId,
    data: { returnText },
  }
}
```

Demo 测试 API mock 必须实现 `notifications.publish`，精确断言一次调用及内容与 `returnText` 相同。发布
Promise 拒绝时，Runtime Promise 必须拒绝且不能返回窗口响应。

`demo-return` 保持现有主结果和复制行为，不增加消息权限或消息发布。

## 15. 失败行为

- 权限未声明或未授权：拒绝发布，不写消息，不触发原生副作用。
- 输入非法：返回 `InvalidNotification`，不截断、转换或部分保存。
- 同一请求重复发布：第二次返回 `AlreadyPublished`，第一条记录保持。
- 请求失效：返回 `ExpiredRequestError`，不能在淘汰、超时或 generation 替换后写入消息。
- 消息提交后请求才被淘汰：本次 `publish()` 保持成功；旧请求的后续窗口或主结果按既有所有权合同丢弃。
- 持久化失败：返回 `MessageStoreUnavailable`，`demo-win` 本次命令失败且窗口不更新。
- current/backup 都损坏或计数器耗尽：消息管理命令返回 `MessageStoreUnavailable`，消息页显示“消息不可用”，
  徽标显示 `!`，不能伪装成空列表或未读为零。
- Windows 通知失败：消息、未读和插件响应保持成功。
- Windows 通知点击或关闭回调失败：释放可释放的活动句柄，消息和未读状态保持。
- 干净退出取消活动通知失败：记录脱敏诊断并继续退出，不改变已保存消息。
- 托盘闪烁失败：消息、未读、通知和插件响应保持成功，并尽力恢复正常图标。
- 前端事件失败：持久化状态保持；下次摘要或消息页读取恢复正确状态。
- 通知点击后的窗口激活失败：消息保持未读，现有窗口状态不被回滚或隐藏。
- 清空失败：旧列表和未读状态保持，设置页保持打开。

## 16. 测试策略

### 16.1 自动化

后端聚焦覆盖：

- 消息字段校验、500 字符边界、控制字符和未知字段；
- 权限、caller、插件 ID、generation、request ID、过期请求和单请求一次限制；
- 100 条上限、最旧淘汰、ID/修订耗尽、重启恢复和 current/backup 损坏；
- 发布失败不改变旧文档，持久化成功后才调用原生副作用；
- 请求 A 提交消息后释放守卫、请求 B 淘汰 A、A 的派发稍后完成时，A 的 `publish()` 仍成功且消息只保存一次；
- 打开截止点与并发新消息，清空与并发新消息；
- 摘要事件先于同修订打开/清空响应时接受完整快照；更高摘要先到时丢弃低修订快照并重新读取；
- 禁用、升级、reload 和卸载不删除历史；
- 插件升级后历史使用当前图标，卸载或图标不可用时回退默认图标；
- current/backup 都损坏时摘要、打开、读取和清空统一返回 `MessageStoreUnavailable`；
- 通知和托盘失败不回滚消息；
- Windows 通知适配器使用 fake trait 覆盖系统禁用、同步发送失败、异步 `Failed`、`Activated` 固定路由、
  `Dismissed` 清理和退出 `Hide`；
- 托盘单计时器、新消息重新计时和退出恢复正常图标；
- 通知点击 payload 只能路由到固定的设置消息目标。

前端聚焦覆盖：

- 设置页签顺序、消息空状态、最新优先列表、时间和插件回退图标；
- 打开页签触发已读；摘要与完整快照分别维护游标，同修订完整快照能够补全列表；
- 事件先于命令响应、同修订空列表、低修订响应和重读四种乱序路径；
- 设置按钮徽标的 0、1、99、100 边界；
- 消息存储不可用显示独立错误页和 `!` 徽标，不显示“暂无消息”或未读零；
- 清空成功和失败都不关闭设置页；
- 消息到达不改变当前视图、焦点、查询或输入值。

SDK 与 Demo 覆盖：

- TypeScript API 类型包含冻结的 `notifications.publish` 合同；
- Schema/安装测试证明 Windows 上权限可用、未授权更新不提交、其他当前未实现权限仍拒绝；
- `demo-win` 精确发布一次 `returnText` 后返回原窗口 DTO；
- `demo-return` 和现有 `/find`、计算、浏览器搜索回归不变。

### 16.2 人工 Windows 验收

自动化通过后，由用户分两次在普通权限环境执行真实验收。任何需要用户点击、前台焦点或观察系统 UI 的
步骤必须提前通知用户；自动化和 Agent 绝不控制鼠标或键盘，也不要求 Windows VM。

开发态验收使用 `npm run tauri dev`，覆盖宿主业务合同：

1. 安装或更新 `demo-win`，确认新增消息权限并授权。
2. 输入 `/demo-win hello` 并回车，确认子窗口显示预期 `Return text`。
3. 确认消息入库、托盘短时闪烁和设置按钮未读徽标；Windows 通知仅作为开发态冒烟观察。
4. 在主窗口或子窗口可见时再次调用，确认窗口不被消息到达抢焦点。
5. 进入“设置 -> 消息”，确认历史按最新优先显示且徽标清零。
6. 消息页保持打开时再次调用，确认新消息保持未读并重新出现徽标。
7. 重启 UiPilot，确认历史和未读状态恢复。
8. 点击“清空全部”，确认设置页不关闭且列表清空。
9. 卸载 `demo-win` 前先保留一条历史，确认卸载后历史仍显示名称快照和默认图标。

发布态验收使用普通权限安装的打包产物，是 Windows 原生通知合同的放行条件：

1. 确认通知显示 UiPilot 的正式应用名称和图标，而不是开发进程身份。
2. 点击通知，确认运行中的 UiPilot 打开并聚焦到“设置 -> 消息”，且执行同一已读合同。
3. 在 Windows 设置中禁用 UiPilot 通知后发布消息，确认消息、徽标和托盘仍成功，日志只记录受控错误。
4. 重新允许通知，发布一条后从托盘干净退出，确认宿主尝试取消仍由当前进程持有的活动通知且正常退出。
5. 本阶段不验收退出后的旧通知冷启动；该行为仍是非目标。

## 17. 验收标准

1. `notifications.publish` 只在 Windows、已授权且当前的公开插件请求中可用。
2. 一次请求最多成功发布一条 1 到 500 字符的单段纯文本消息。
3. 消息在任何通知、事件或托盘操作前原子持久化，并最多保留最近 100 条。
4. 原子提交后发生的请求淘汰不能把已保存消息改报为过期；旧请求的后续 UI 结果仍会被丢弃。
5. 打开消息页只标记进入截止点前消息已读；摘要事件与同修订完整快照乱序时不会丢列表或误清新未读。
6. 设置消息页、清空全部、持久化历史和插件卸载保留符合用户合同；历史使用当前安装图标或默认图标。
7. 设置按钮徽标准确显示未读数量并在 100 条时显示 `99+`；存储不可用时显示 `!` 而不是未读零。
8. 每条消息尝试显示 Windows 通知并触发 6 秒单计时器托盘闪烁。
9. Windows 原生适配器能观测系统禁用、发送失败和点击，并在干净退出时尽力取消活动通知；正式身份、
   图标和点击路由通过普通权限安装产物验收。
10. 消息到达绝不抢焦点；用户点击通知才打开并聚焦设置消息页。
11. `demo-win` 的消息内容与子窗口 `Return text` 完全相同，消息失败时窗口不部分成功。
12. `demo-return`、`/find`、计算、浏览器搜索及现有窗口行为没有回归。

## 18. 非目标

- 后台 Runtime、定时器、番茄时钟、`background.schedule` 或请求完成后的发布；
- 远程推送、云同步、跨设备通知或插件间消息；
- Markdown、HTML、图片、附件、链接、按钮、自定义动作或声音选择；
- 一次请求发布多条消息、批量发布、消息更新或撤回；
- 逐条删除、筛选、搜索、分页、导出或超过 100 条的归档；
- 插件指定消息 ID、插件名称、时间、已读状态、Windows 标题或托盘图标；
- 在 macOS 上开放消息权限或实现原生通知；
- 用户退出 UiPilot 后通过历史 Windows 通知冷启动应用；
- 自动化控制用户鼠标、键盘或真实前台焦点。
