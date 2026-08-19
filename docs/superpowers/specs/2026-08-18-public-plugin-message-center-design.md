# UiPilot 公开插件消息中心与 Windows 通知设计

## 1. 文档信息

- 日期：2026-08-18
- 状态：Approved; final written review passed on 2026-08-18; tray attention and dual-badge revision approved on 2026-08-19
- 产品阶段：公开插件平台后续 MVP
- 目标平台：Windows 11 x64
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 验收插件：`com.uipilot.demo-win`

## 2. 目标

为公开插件增加一个请求期消息发布能力。插件在有效的 `onCommand` 调用中发布一条纯文本消息后，
UiPilot 负责持久化消息、维护未读状态、显示 Windows 通知、在用户注意到主窗口前持续闪烁托盘图标，
并在主窗口设置按钮和设置页“消息”页签上显示同一未读徽标。

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
- 新增宿主管理的消息中心、双位置未读徽标、Windows 通知和进程内托盘注意提醒。

除后续[宿主延迟消息设计](./2026-08-19-public-plugin-delayed-message-design.md)窄化开放的宿主持有
`notifications.schedule()` 单次纯文本消息外，`background.schedule`、常驻 Runtime、通用定时任务和
请求结束后的插件主动推送仍然不可用；插件代码不能在请求结束后运行。其他既有权限、请求所有权、
generation、安装原子性、窗口交接和 UI 焦点合同不变。

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

主窗口输入框右侧的设置图标和设置页“消息”页签显示同一未读数量：

- 未读为零时不显示；
- 1 到 99 显示准确数字；
- 超过 99 显示 `99+`。

设置页关闭或主窗口隐藏时，未读状态仍由宿主持久化并在下次显示时恢复。

程序运行期间消息存储从可用转为不可用时，当前已经显示的数字徽标必须立即切换为 `!`；不能等待用户
重新打开主窗口或再次读取摘要后才发现状态变化。此后到达的旧 ready 事件或成功响应不能把 `!` 改回
数字，也不能恢复旧消息列表。

### 4.4 Windows 通知与托盘

每条成功持久化的消息都尝试显示一张 Windows 通知，不根据主窗口或插件窗口是否可见进行抑制。通知标题
使用插件名称，正文使用消息内容。

用户点击通知时，UiPilot 打开并聚焦主窗口，直接进入“设置 -> 消息”，并执行与手动进入该页相同的
已读操作。消息到达本身绝不显示、隐藏、聚焦或移动任何窗口，也不清空或改写用户当前输入。

消息到达且主窗口当时没有原生焦点时，宿主启动进程内托盘注意提醒：原始图标和同尺寸全透明图标每
500 毫秒切换一次，不使用另一张提醒图标，也没有固定时间截止点。任意时刻只有一个闪烁循环；提醒期间
收到新消息只保持该循环，不创建线程或叠加计时器。

主窗口获得原生焦点时，托盘提醒立即停止并恢复原始图标，但该动作绝不读取消息、修改 `readAt` 或减少
未读数。如果消息到达时主窗口已经拥有原生焦点，本次消息不启动托盘提醒，两个未读徽标仍正常增加。
进入“设置 -> 消息”才执行第 4.2 节的标记已读操作并让两个徽标同步清零。程序退出或适配器失败都使
当前提醒停止或进入终态，并尽力恢复原始图标；原生图标 API 失败时不承诺恢复成功。

Windows 通知被系统关闭、权限被拒绝或发送失败时，已保存的消息、未读徽标和托盘闪烁不受影响。

## 5. 术语

- **消息（message）**：由一个当前公开插件请求发布、由宿主持久化的纯文本记录。
- **发布（publish）**：`api.notifications.publish()` 成功完成一次消息持久化提交。
- **未读消息（unread message）**：`readAt` 仍为 `null` 的持久化消息。
- **托盘注意提醒（tray attention）**：仅存在于当前原生进程的瞬时状态；它和持久化未读状态独立，主窗口
  获得焦点会确认提醒但不会把消息标记为已读。
- **消息修订（message revision）**：宿主消息文档每次成功发布或产生状态变化的标记已读、清空后递增的
  持久化序号。
- **摘要修订游标（summary revision cursor）**：前端已观察到的最新消息摘要修订，只用于未读徽标和判断
  完整快照是否过期。
- **快照修订游标（snapshot revision cursor）**：前端当前消息列表对应的完整快照修订；ready 状态事件
  不能推进该游标。
- **宿主状态事件（host state event）**：只发送给 `main` 的 `MessageHostStateChangedEvent`。其 ready
  分支携带摘要修订和未读数，其 unavailable 分支只通知消息子系统进入会话终态。
- **前端会话状态（frontend session state）**：主前端维护的 `unknown | ready | unavailable` 状态。
  `unavailable` 是吸收终态；同一 UiPilot 原生进程中不能再转回 ready。
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

`<`、`>`、`&`、双引号和单引号是合法纯文本字符，宿主必须按原语义保存并显示相同文本。DOM 序列化器
为生成合法 XML 所做的实体转义是必要编码，不是正文改写；这些字符不能被解释为 XML 结构。

一次 request ID 最多成功发布一条消息。第二次发布返回 `AlreadyPublished`，不产生第二条记录。插件普通
新请求可以再次发布一条。Promise 已决议后，或者请求在到达发布提交点前被新请求淘汰、超时、取消、
禁用、卸载、升级、reload 或 Runtime generation 替换时，旧 API 调用返回 `ExpiredRequestError`。

宿主在持有当前请求守卫时完成最终身份复核和消息原子提交。原子文件替换成功是本次 `publish()` 的
不可撤销成功线性化点：

- 请求在提交点之前失效时，调用返回 `ExpiredRequestError`，且不能写入消息；
- 提交点已经完成后，新请求只能在当前请求守卫释放后淘汰旧请求；该淘汰不得把已提交的调用改报为
  `ExpiredRequestError`；
- 提交点之后不再为了决定 `publish()` 返回值重新校验请求身份；ready 宿主状态事件、Windows 通知和托盘
  闪烁只做
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
新文档的 `revision` 为 `"0"`，`nextMessageId` 为 `"1"`；首次消息 ID 为 `"1"`。

消息 ID、`nextMessageId`、消息修订和进入截止点共用同一套规范十进制 `u64` 比较语义：

- Rust 侧验证规范格式后解析为 `u64` 比较；
- TypeScript 业务调用点必须调用唯一的 `compareU64Decimal(a, b)` 协议 helper，禁止自行使用 `Number`、
  `parseInt`、隐式数值转换或直接的字符串关系运算符；
- helper 先验证 `^(0|[1-9][0-9]*)$`，再拒绝大于 `18446744073709551615` 的值；合法值先按字符串长度
  比较，长度相同时由 helper 内部按 ASCII 数字字典序比较，返回负数、零或正数；最大值校验也使用同一
  长度优先规则，不能先转换为 `number`；
- 规范字符串的相等判断可以直接比较字符串，但所有高低、范围和截止点判断必须使用该 helper。

因此 `"10"` 必须高于 `"9"`，超过 `Number.MAX_SAFE_INTEGER` 的合法 `u64` 也必须保持精确顺序。

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

在截止点之后发布的消息不属于本次标记，即使持久化完成时页面仍然打开，也保持未读。ready 宿主状态
事件与打开
操作可以乱序到达前端；前端必须分别维护摘要修订游标和快照修订游标：

- ready 事件修订高于摘要修订游标时更新该游标与未读徽标，并把更低修订的完整列表标记为过期；等于
  当前游标的重复事件幂等忽略，低于当前游标的事件丢弃；ready 事件不能推进快照修订游标；
- 完整快照的修订低于摘要修订游标时必须丢弃，并立即重新读取；
- 完整快照的修订等于摘要修订游标时必须接受，以便 ready 状态事件先到后，同修订的列表、空列表或
  清空结果仍能补全界面；
- 完整快照的修订高于摘要修订游标时同时推进两个游标；同修订完整快照可幂等替换当前列表。

以上修订规则只适用于前端会话状态仍为 `unknown` 或 `ready` 时。一旦状态为 `unavailable`，不再比较任何
迟到 ready 事件或成功快照的修订；这些结果无条件丢弃。

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

type MessageHostStateChangedEvent =
  | {
      status: 'ready'
      revision: U64Decimal
      unreadCount: number
    }
  | {
      status: 'unavailable'
      error: 'MessageStoreUnavailable'
    }
```

主窗口消息管理命令的错误 DTO 固定为：

```ts
type MessageHostCommandErrorDto =
  | {
      code: 'MessageOperationFailed'
      storeStatus: 'ready'
    }
  | {
      code: 'MessageStoreUnavailable'
      storeStatus: 'unavailable'
    }
```

`revision` 和 `id` 必须是规范无前导零的十进制 `u64` 字符串。`unreadCount` 必须是 0 到 100 的
JavaScript 整数，不使用字符串。读取摘要返回 `MessageSummaryDto`；打开、清空和重新读取返回完整
`MessageCenterSnapshotDto`。本节所有修订游标高低判断以及消息 ID 排序必须复用第 8 节的
`compareU64Decimal`，不能在 UI 层自行实现另一套比较规则。

消息存储具有 `ready` 和 `unavailable` 两种会话状态。current/backup 都损坏或计数器耗尽时进入
`unavailable`；所有消息管理命令统一拒绝并返回固定错误码 `MessageStoreUnavailable`，不能返回空数组或
未读数 `0` 来伪装成功。前端必须把该错误映射为独立的“消息不可用”状态。`unavailable` 是本次进程会话
的终态，不在后台自动恢复；恢复只能在用户解决存储问题并重启 UiPilot 后重新执行 current/backup 恢复。

启动时已经不可用的状态由首次摘要或消息命令的 `MessageStoreUnavailable` 返回给前端。运行期间任何操作
因计数器耗尽或不可恢复的完整性错误首次执行 `ready -> unavailable` 转换时，宿主必须在消息存储锁和请求
守卫释放后，向 `main` 恰好发出一次 `status: 'unavailable'` 的 `MessageHostStateChangedEvent`。普通原子
写入失败如果仍保留有效旧快照，只使本次操作失败，不执行终态转换，也不发送 unavailable 事件。这类
主窗口操作返回 `MessageOperationFailed` 并保留当前列表；只有终态分支返回 `MessageStoreUnavailable`。
公开插件的 `publish()` 仍按第 7 节把任何无法完成的消息持久化映射为 `MessageStoreUnavailable`，但只有
宿主状态实际转换时才发送 unavailable 事件。

主前端必须实现以下单向状态机：

```text
unknown -> ready
unknown -> unavailable
ready -> unavailable
unavailable -> unavailable
```

收到 `status: 'unavailable'` 事件，或任一消息管理命令返回 `storeStatus: 'unavailable'`，都是进入前端
unavailable 吸收终态的线性化点。从该点起直到 UiPilot 原生进程重启，前端必须忽略：

- 所有迟到的 `status: 'ready'` 事件，无论 revision 多高；
- 所有迟到的成功摘要和 `MessageCenterSnapshotDto` 响应；
- 所有迟到的 `MessageOperationFailed` / `storeStatus: 'ready'` 错误响应；
- 任何会恢复数字徽标、消息列表、摘要游标或快照游标的异步完成。

前端 WebView 在同一原生进程中 reload 后必须先从宿主读取状态，再渲染徽标或消息页；不能把 WebView
内存重置视为 UiPilot 进程重启。只有新的原生进程完成 current/backup 恢复后，新的前端会话才能从
`unknown` 进入 ready。

消息提交、已读和清空后，宿主向 `main` 发出 `status: 'ready'` 且只含消息修订和未读数的状态事件。
ready 事件不是消息持久化的完成条件，也不是完整列表快照；它只是一条摘要更新和快照失效提示。
unavailable 事件没有修订或未读数，前端收到后必须立即停止显示旧数字徽标、显示 `!`，并把已打开的消息
页切换为不可用状态。任何状态事件丢失时，下次读取摘要、打开消息页或再次显示主窗口必须从宿主恢复
当前状态。插件 Runtime、插件窗口、`find` 窗口和其他 label 不能订阅这些事件、消息正文或调用消息管理
命令。

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
-> ready state event
-> Windows notification
-> enqueue TrayAttentionEvent::MessageArrived
```

主窗口的每个 `WindowEvent::Focused(focused)` 在任何“预期 blur”提前返回和既有窗口生命周期处理之前，
向同一个托盘注意控制器排入 `TrayAttentionEvent::MainFocusChanged(focused)`。`MessageArrived` 与
`MainFocusChanged` 共用一个串行事件队列和一个状态机；消息发布路径不得先查询焦点再发送无条件开始意图。
状态机事件入队成功是托盘注意顺序的线性化点：

- `MessageArrived -> MainFocusChanged(true)`：允许短暂开始，但后一个事件必须停止并恢复原图；
- `MainFocusChanged(true) -> MessageArrived`：后一个事件观察到当前聚焦状态，不得开始；
- `MainFocusChanged(false)` 只更新当前焦点状态，不自行开始；之后到达的新消息可以开始；
- 同一事件队列中已经确认聚焦后，不存在能绕过当前焦点状态的“迟到开始”命令。

控制器在主窗口仍隐藏、生产焦点钩子已经安装且 readiness 尚未放行时创建，初始焦点状态固定为
`false`。此后焦点真值只由上述 `Focused(bool)` 事件更新，避免焦点查询与事件投递之间的竞态。

发布路径在提交前触发 `ready -> unavailable` 时使用独立失败顺序：

```text
request scheduler current guard
-> message store lock
-> mark session unavailable without changing the persisted snapshot
-> release message store lock
-> release request guard
-> unavailable state event to main
-> return MessageStoreUnavailable
```

主窗口的打开、读取或清空命令触发同一转换时没有 request scheduler guard，但仍必须先释放消息存储锁，
再发 unavailable 状态事件并返回错误。

以下乱序是合法的，前端吸收终态必须保证最终 UI 仍为 unavailable：

```text
operation A commits or prepares a ready response
-> operation B transitions store to unavailable
-> frontend observes unavailable event or command error
-> delayed ready event / summary / snapshot / ready error from A arrives
-> frontend discards A completion and keeps the ! badge
```

消息存储代码不能反向获取 scheduler、插件管理或窗口生命周期锁。任何锁都不能跨越 frontend emit、
Windows 通知、托盘图标操作、窗口 show/focus 或等待前端 ack。

Windows 通知、ready 状态事件或托盘副作用失败不回滚已持久化消息。消息持久化失败不调用 ready 事件、
Windows 通知或托盘；如果失败同时产生首次 `ready -> unavailable` 转换，则 unavailable 状态事件是唯一
允许的前端副作用。
三个宿主副作用相互独立，一个失败不能阻止后续副作用。`publish()` 在持久化成功并完成这些有界派发尝试
后返回成功，不等待用户看到、点击或关闭 Windows 通知。请求守卫释放后发生的淘汰不再参与返回值判定，
因此不允许出现“消息已保存但 `publish()` 返回 `ExpiredRequestError`”的部分失败。

## 12. UI 设计

设置页继续使用现有垂直 Tabs、主题令牌和 OverlayScrollbars。新增 `messages` 页签，不创建新原生窗口。

消息页是一个可滚动的非卡片嵌套列表。每行具有稳定图标尺寸和三块内容：插件名称、时间、正文。正文
允许正常换行显示，但不解释任何标记。插件当前仍安装且当前图标 URL 可用时使用现有 `PluginIcon`；
插件升级后历史记录使用升级后的当前图标，卸载或图标加载失败时使用默认插件图标。

“清空全部”是明确文字命令，只在列表非空且没有清空操作进行时可用。操作期间不关闭设置页；失败时
如果宿主仍为 ready，则保留列表并显示固定的 `MessageOperationFailed`，不泄露磁盘路径；如果宿主已经
转为 unavailable，则切换到下述不可用状态。

消息存储不可用时，消息页显示独立的“消息不可用，请重启 UiPilot”，不提供重试命令，禁用“清空全部”，
不能显示“暂无消息”。前端观察到终态后，设置按钮和“消息”页签都显示固定尺寸的非数字 `!` 状态徽标，
而不是隐藏徽标或显示未读为零；同一前端会话后续显示主窗口时不重复调用消息读取命令。已有的最后成功
列表不得被伪装成当前空列表。

只有尚未观察到终态的前端才能在首次启动、事件可能丢失后的主窗口显示或 WebView reload 初始化时读取
宿主状态。一旦读取结果确认 unavailable，本次原生进程内不再提供普通重试；用户必须重启 UiPilot 才能
触发下一次存储恢复。

启动器设置按钮继续使用现有图标按钮，“消息”继续使用现有页签文字。两处通过现有 Ant Design `Badge`
呈现同一个消息摘要：零隐藏，1 到 99 显示准确数字，100 显示 `99+`，unavailable 显示 `!`。徽标使用
稳定容器，不改变输入框或页签布局，也不遮挡设置按钮的可点击区域和焦点轮廓。

## 13. Windows 通知与托盘适配器

原生通知由宿主自有的 Windows 适配器统一发送，插件不能直接访问 Tauri notification API。本阶段不使用
官方 Tauri notification 插件：其桌面接口不能同时满足 Windows 点击回调、活动通知取消和异步失败观测
合同。适配器直接通过项目已有的 `windows` crate 使用 WinRT `Windows.UI.Notifications` 能力，并保持
其余代码只依赖宿主定义的窄 trait。

适配器固定职责如下：

- 使用 `ToastNotificationManager` 为 UiPilot 的已安装应用身份创建 `ToastNotifier`；
- 在发送前读取 `ToastNotifier.Setting()`，系统禁用通知时返回可诊断的受控失败；
- 使用不含动态插值的宿主常量创建固定 `XmlDocument` 模板。`LoadXml()` 只能接收该宿主常量，模板只包含
  `toast/visual/binding/text` 等本设计需要的固定节点，不包含 `actions` 或插件可控占位片段；
- 插件名称快照和消息正文一律通过 `CreateTextNode()` 后 `AppendChild()`，或等价的 DOM `InnerText`
  写入既有 `text` 节点。禁止把它们拼接进 XML 字符串、元素名、属性名、属性值或 `launch` payload；
- `launch` 路由由宿主设置为固定目标；如携带消息 ID，只能使用已经验证的规范十进制宿主 ID，并通过
  DOM `SetAttribute()` 写入。接收端仍重新验证，不信任回调字符串；
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

托盘适配器只接受 `MessageArrived`、`MainFocusChanged(bool)` 和 `Shutdown` 三类事件。一个工作线程同时
拥有 `mainFocused`、`active`、当前视觉帧和 `running | degraded | terminal` 会话状态，并按第 11 节的
单一顺序域处理事件和 500 毫秒 tick。`MessageArrived` 仅在 `mainFocused == false` 且状态为 `running` 时
开始或保持单循环；`MainFocusChanged(true)` 停止循环并尝试恢复原图；重复事件均幂等。适配器不持久化
提醒状态，也不读取或修改未读消息。

正常托盘图标成功创建后，注意控制器的工作线程或事件通道构造失败不得让 UiPilot setup 失败。宿主改为
安装一个本次进程固定为 `terminal` 的 no-op 托盘注意端口：保留正常托盘图标，所有消息和焦点事件幂等
忽略，`Shutdown` 可重复调用，并只记录一次受控诊断。正常托盘图标本身创建失败仍属于既有应用生命周期
初始化失败，不在该降级规则内。MVP 不支持运行期间替换托盘注意适配器；不存在控制器间焦点或提醒迁移。

失败行为固定如下：

| 失败或边界 | 托盘注意状态 | 其他合同 |
| --- | --- | --- |
| 注意控制器工作线程或通道构造失败 | 安装 `terminal` no-op 端口并保留正常托盘图标；本次进程不再尝试启动提醒 | UiPilot 继续启动；消息、徽标和通知仍可用 |
| 向控制器发送事件失败 | 记录一次受控诊断，不重试；控制器已经退出时保持其终态 | 不改变消息、未读、窗口或插件结果 |
| 切换到透明帧或原图失败 | 当前控制器进入 `degraded`，停止 tick，立即尽力设置一次原图；后续 `MessageArrived` 全部忽略 | 消息和两处徽标保持成功 |
| `degraded` 后收到 `MainFocusChanged(true)` | 再尽力设置一次原图，仍保持 `degraded` | 不标记消息已读 |
| `Shutdown` | 进入 `terminal`，尽力设置原图并拒绝队列中或之后的 `MessageArrived` | 不改变已保存消息和未读状态 |
| 任意恢复原图调用失败 | 只记录脱敏诊断；不能声称图标已经恢复 | 不回滚消息、通知或徽标 |

上述事件发送、图标更新和线程收尾均发生在消息、插件、请求和窗口生命周期锁之外。消息到达路径不得调用
主窗口 show/focus。只有经过验证的用户通知点击动作可以请求显示“设置/消息”。

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
- 运行中首次进入 unavailable：释放所有锁后只向 `main` 发一次 unavailable 状态事件；事件失败时，下次
  摘要读取或主窗口显示仍必须发现不可用，旧数字徽标不能被当作当前状态。
- 前端已观察到 unavailable 后收到迟到 ready 事件、成功摘要、完整快照或 `storeStatus: 'ready'` 错误：
  全部丢弃，保持 `!` 和不可用页面；只有 UiPilot 原生进程重启可以解除该状态。
- unavailable 页面不提供重试命令，固定提示“消息不可用，请重启 UiPilot”；重复显示主窗口不能在同一
  进程内重新初始化消息存储。
- Windows 通知失败：消息、未读和插件响应保持成功。
- Windows 通知点击或关闭回调失败：释放可释放的活动句柄，消息和未读状态保持。
- 干净退出取消活动通知失败：记录脱敏诊断并继续退出，不改变已保存消息。
- 托盘闪烁失败：消息、未读、通知和插件响应保持成功，并尽力恢复正常图标。
- 宿主状态事件发送失败：持久化状态保持；下次摘要或消息页读取恢复正确状态。
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
- ready 状态事件先于同修订打开/清空响应时接受完整快照；更高 ready 修订先到时丢弃低修订快照并重新读取；
- 规范十进制比较器覆盖 `9 -> 10`、`99 -> 100`、跨越 `Number.MAX_SAFE_INTEGER`、`u64::MAX`，并拒绝
  前导零、非数字、负数和超过 `u64::MAX` 的输入；
- 禁用、升级、reload 和卸载不删除历史；
- 插件升级后历史使用当前图标，卸载或图标不可用时回退默认图标；
- current/backup 都损坏时摘要、打开、读取和清空统一返回 `MessageStoreUnavailable`；
- 计数器在运行中耗尽时，不提交消息、不发送 ready/通知/托盘副作用，释放锁后只发送一次 unavailable
  状态事件；后续操作保持终态且不重复发送转换事件；
- 可恢复的原子写入失败保留 ready 状态和旧快照，主窗口命令返回 `MessageOperationFailed`，不发送
  unavailable 状态事件；
- 通知和托盘失败不回滚消息；
- Windows 通知适配器使用 fake trait 覆盖系统禁用、同步发送失败、异步 `Failed`、`Activated` 固定路由、
  `Dismissed` 清理和退出 `Hide`；
- Toast DOM 测试覆盖 `<>&"'`、`</text><actions>` 和伪造 `launch` 片段，断言通知文本节点的 `InnerText`
  与输入逐字符一致、DOM 不出现 `actions` 节点且路由属性仍完全由宿主生成；
- 托盘单循环持续闪烁、重复消息不叠加、所有 `Focused(bool)` 在预期 blur 返回前入队，以及以下确定性
  顺序：`MessageArrived -> MainFocusChanged(true)` 最终停止，`MainFocusChanged(true) -> MessageArrived`
  不启动，失焦后的新消息重新启动；
- fake worker/托盘适配器覆盖控制器构造失败降级为 no-op、透明帧失败、原图失败、事件发送失败、
  `degraded` 后确认、退出以及终态后迟到 `MessageArrived`，断言应用启动继续、停止 tick、只做规定的
  原图恢复尝试且不改变消息状态；
- 通知点击 payload 只能路由到固定的设置消息目标。

前端聚焦覆盖：

- 设置页签顺序、消息空状态、最新优先列表、时间和插件回退图标；
- 打开页签触发已读；ready 事件摘要与完整快照分别维护游标，同修订完整快照能够补全列表；
- 事件先于命令响应、同修订空列表、低修订响应和重读四种乱序路径；
- 设置按钮和“消息”页签徽标的 0、1、99、100 边界以及同步更新；
- 消息存储不可用显示独立错误页和 `!` 徽标，不显示“暂无消息”或未读零；
- unavailable 页面显示“消息不可用，请重启 UiPilot”，没有重试按钮或重试命令；
- 主窗口已显示数字徽标时收到 unavailable 状态事件，立即切换为 `!` 并使已打开消息页进入不可用状态；
- `MessageOperationFailed` 保留当前列表和数字徽标，不能误切换成 unavailable；
- operation A 的 ready 事件或命令响应延迟，operation B 先使前端进入 unavailable 后，A 的 ready 事件、
  成功摘要、完整快照和 `storeStatus: 'ready'` 错误均不能恢复数字徽标或列表；
- 清空成功和失败都不关闭设置页；
- 主窗口获得焦点只停止托盘提醒，不清除两处徽标；进入“消息”页才让两处徽标同步清零；
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
3. 主窗口未激活时确认消息入库、托盘使用原图/透明帧持续闪烁，并确认设置按钮与“消息”页签显示相同
   未读徽标；Windows 通知仅作为开发态冒烟观察。
4. 激活主窗口，确认托盘立即恢复原始图标且两个徽标继续存在；主窗口已经激活时再次产生消息，确认托盘
   不开始闪烁、窗口不被消息到达抢焦点且徽标增加。
5. 进入“设置 -> 消息”，确认历史按最新优先显示且两个徽标同步清零。
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
5. 打开消息页只标记进入截止点前消息已读；ready 状态事件与同修订完整快照乱序时不会丢列表或误清
   新未读；
   所有 ID 与修订高低判断在 `u64` 全范围内准确。
6. 设置消息页、清空全部、持久化历史和插件卸载保留符合用户合同；历史使用当前安装图标或默认图标。
7. 设置按钮和“消息”页签徽标准确显示同一未读数量并在 100 条时显示 `99+`；存储在运行中转为不可用
   时通过 main-only 状态事件同时显示 `!`，而不是继续显示旧数字或未读零；同一进程内所有迟到 ready
   结果都不能解除 unavailable 吸收终态。
8. unavailable 页面明确要求重启 UiPilot，不提供当前会话内必然失败的重试入口。
9. 主窗口未激活时，消息触发原图/透明帧单循环托盘闪烁，直到主窗口获得焦点；主窗口已激活时不启动。
   停止提醒不会清除未读，只有进入“消息”页才让两个徽标同步清零。
10. Windows 原生适配器能观测系统禁用、发送失败和点击，并在干净退出时尽力取消活动通知；正式身份、
   图标和点击路由通过普通权限安装产物验收。
11. 插件名称和正文只能作为 Toast DOM 文本节点写入；XML 敏感字符和伪造标签不能改变节点、动作或路由。
12. 消息到达绝不抢焦点；用户点击通知才打开并聚焦设置消息页。
13. `demo-win` 的消息内容与子窗口 `Return text` 完全相同，消息失败时窗口不部分成功。
14. `demo-return`、`/find`、计算、浏览器搜索及现有窗口行为没有回归。

## 18. 非目标

- 除[宿主延迟消息设计](./2026-08-19-public-plugin-delayed-message-design.md)定义的
  `notifications.schedule()` 外，后台 Runtime、定时器、番茄时钟、`background.schedule` 或请求完成后的
  插件发布；
- 远程推送、云同步、跨设备通知或插件间消息；
- Markdown、HTML、图片、附件、链接、按钮、自定义动作或声音选择；
- 一次请求发布多条消息、批量发布、消息更新或撤回；
- 运行期间替换托盘注意适配器、在控制器之间迁移焦点状态或恢复已经确认/失败的提醒；
- 逐条删除、筛选、搜索、分页、导出或超过 100 条的归档；
- 插件指定消息 ID、插件名称、时间、已读状态、Windows 标题或托盘图标；
- 在 macOS 上开放消息权限或实现原生通知；
- 用户退出 UiPilot 后通过历史 Windows 通知冷启动应用；
- 自动化控制用户鼠标、键盘或真实前台焦点。
