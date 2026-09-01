# 剪贴板历史公开插件：宿主能力请求

## 1. 文档信息

- 日期：2026-08-30
- 状态：已完成产品设计，等待宿主程序设计与实现
- 读者：UiPilot 宿主程序开发者、公开插件 SDK 维护者
- 目标平台：Windows
- 插件场景：Panel 类型的剪贴板历史插件

相关合同与既有设计：

- [公开插件开发指南](../../plugin-sdk/public-plugin-developer-guide.md)
- [公开插件 API v1 类型](../../plugin-sdk/uipilot-plugin-api-v1.d.ts)
- [公开插件 Manifest Schema](../../plugin-sdk/uipilot-plugin-v1.schema.json)
- [Panel Host Key 路由与隐藏设计](./2026-08-26-public-plugin-panel-key-routing-and-hide-design.md)
- [Panel 列表 Enter 转发请求](./2026-08-26-panel-list-enter-host-key-forwarding-request.md)

## 2. 用户需要

用户需要一个 Panel 类型的公开插件，在 UiPilot 运行期间持续记录最近 50 次剪贴板变化，包括：

- 纯文本；
- 图片；
- 单文件或多文件列表。

历史跨 UiPilot 重启保留。用户打开插件后，通过分类标签和键盘选择一条记录；按 Enter 后，UiPilot 隐藏、焦点返回打开 UiPilot 前的外部窗口，并自动粘贴所选内容。

本能力不读取 Windows `Win+V` 已有历史，只记录 UiPilot 运行且插件已启用、权限已授予期间发生的变化。

## 3. 当前公开 API 缺口

公开插件 API v1 无法实现该插件：

1. Runtime 和 Panel 桥均不能读取系统剪贴板或监听剪贴板变化。
2. 公开插件不能在命令请求结束后常驻后台采集事件。
3. `copyText` 默认动作只能写入文本，不能恢复图片或文件列表。
4. Panel 的 `requestHide()` 可以请求隐藏并尽力恢复外部窗口，但不能执行粘贴。
5. `panel.hostKeys` 当前只支持 `ArrowDown`、`ArrowUp` 和 `Primary+N`，无法把主输入框中的 `Tab`、`Shift+Tab` 和 `Enter` 路由给插件。
6. Panel 存储只接受有限 JSON，且每插件配额为 5 MiB，不适合保存最多 500 MiB 的图片历史。

因此，插件不能通过 WebView、Tauri 私有对象、Shell 或自行模拟键盘绕过宿主限制。宿主能力交付前，公开插件实现应保持阻塞。

## 4. 产品合同

### 4.1 采集生命周期

- 宿主只在插件已启用且用户授予剪贴板历史读取权限时采集。
- 插件面板关闭不停止采集；插件禁用、故障停用或卸载后立即停止采集。
- 再次启用后从新的剪贴板变化继续记录，不补采停用期间或 UiPilot 退出期间的内容。
- 完全卸载删除历史；保留数据卸载保留历史，重新安装并授权后可以恢复。

### 4.2 记录归一化

一次系统剪贴板变化最多生成一条记录。剪贴板同时暴露多种格式时，按以下顺序选择：

1. 文件列表；
2. 图片；
3. 纯文本。

连续两次内容完全相同不新增记录。恢复旧记录会把它移动到历史最前面，但监听回流不得再生成第二条副本。

`capturedAt` 表示该记录首次从系统剪贴板采集的时间，重启和恢复粘贴后都保持不变。列表顺序不从 `capturedAt` 推导，而由宿主内部维护的 recency 排名决定；恢复旧记录只更新 recency 排名并递增 `revision`。

去重 fingerprint 由宿主生成，按记录类型固定：

- 文本：以剪贴板提供的 Unicode text 字符串逐代码点比较，不做 trim、换行归一化、大小写折叠或 Unicode normalization。
- 图片：以宿主解码后的像素内容、宽高和像素格式归一结果比较，不以原始 PNG/DIB 字节比较，避免同一图片因编码差异重复入库。
- 文件列表：以剪贴板文件列表的原始顺序和 Windows 规范化绝对路径比较；同一卷路径按 Windows 文件系统语义做大小写不敏感比较，不排序。

剪贴板同时包含 HTML/RTF 与 Unicode text fallback 时，第一版只按“纯文本”记录 Unicode text fallback；HTML/RTF 富文本本身不入库。

### 4.3 容量与持久化

- 每插件最多保存 50 条，按宿主内部 recency 排名倒序排列。
- 文本原文仅由宿主保存，用于后续恢复粘贴；宿主不得静默修改文本内容。Panel 只接收宿主生成的 `textPreview`，不得收到完整原文。
- `textPreview` 最多 120 个 Unicode 标量值；连续空白和换行折叠为单个空格；空文本或纯空白文本显示为空字符串但仍可作为记录恢复。
- 图片由宿主转为 PNG 保存，单张最多 10 MiB，当前历史中的图片总量最多 500 MiB。
- 超过单张限制的图片不进入历史；超过总量或条目数量时，从最旧记录开始淘汰，直到两个限制都满足。
- 文件记录只保存绝对路径，不复制文件内容。源文件移动或删除后，记录保留但标记为不可用。
- 文件记录的 `available` 表示粘贴前全部路径仍可访问；单个文件失效、目录失效、权限拒绝、UNC 路径暂不可达或快捷方式目标不可解析时均为不可用。只要列表中任一项不可用，整条文件记录不可粘贴。
- 历史索引和图片文件位于插件隔离的用户级应用数据目录；写入必须原子化，剪贴板内容不得进入日志。

### 4.4 Panel 交互

面板为固定双栏布局：

```text
tabs  | list
全部  | - item
图片  |
文件  |
文字  |
```

列表按宿主返回顺序展示，即 recency 排名倒序；相对时间基于 `capturedAt` 展示。条目展示规则：

- 文字：最多两行文字预览；
- 图片：缩略图和图片尺寸；
- 文件：文件类型图标、首个文件名；多文件记录同时显示总数；
- 所有类型：相对时间；
- 文件路径失效：显示“文件不存在”状态。

键盘合同：

- `Tab` 按“全部 → 图片 → 文件 → 文字 → 全部”循环；
- `Shift+Tab` 反向循环；
- `ArrowUp` / `ArrowDown` 移动列表选中项，到首尾后停止；
- 切换分类后选中该分类最新记录；
- `Enter` 对当前选中项执行一次恢复并粘贴。

### 4.5 隐私边界

第一版不尝试识别剪贴板内容来源，也不承诺自动排除密码框或敏感输入框产生的内容。安装确认页和插件说明必须明确提示：授权后，UiPilot 运行、插件启用且权限有效期间的文本、图片和文件列表剪贴板变化会被宿主记录到本机插件隔离数据目录。

用户侧控制包括：撤销权限、禁用插件、手动删除单条历史、清空历史，以及完全卸载时删除历史。宿主日志、错误消息、测试快照和 Panel DTO 均不得包含原始剪贴板内容、原始 PNG 或完整文件路径。

## 5. 建议的宿主能力

### 5.1 权限

建议新增两个独立、可在安装确认页展示的权限：

- `clipboard.history.read`：允许宿主为该插件采集和持久化历史，并向其 Panel 提供展示摘要。
- `clipboard.history.paste`：允许当前 Panel 会话在明确的 Enter 操作中恢复指定记录，并向打开 UiPilot 前捕获的外部窗口发送一次粘贴。

只授予读取权限时，插件可以浏览和删除历史，但不能自动粘贴。宿主不得把任一权限扩展为通用剪贴板读取、通用输入模拟或任意窗口控制能力。

命名冻结前必须处理现有 Manifest/Schema 中已预留但当前不可用的 `clipboard.read`。宿主实现应二选一：

- 复用 `clipboard.read`，并在 SDK 文档中把它严格定义为本能力的宿主管理历史读取权限；
- 或继续保留 `clipboard.read` 为不可用/未来用途，新增 `clipboard.history.read`。

不得让 `clipboard.read` 与 `clipboard.history.read` 同时表达可用且语义重叠的剪贴板读取能力。最终权限名冻结后，Rust、Schema、CLI、TypeScript 类型、安装确认页和开发者文档必须一致。

### 5.2 Panel 桥

建议在 `window.uipilotPluginPanel` 下增加宿主管理的窄接口。以下名称表达能力形状；最终字段名应在宿主 API 设计中统一冻结：

```ts
interface UiPilotPluginPanelClipboardHistoryApi {
  list(): Promise<Readonly<ClipboardHistorySnapshot>>
  onChanged(
    handler: (snapshot: Readonly<ClipboardHistorySnapshot>) => void,
  ): () => void
  paste(input: Readonly<{
    id: string
    routeSequence: U64Decimal
  }>): Promise<Readonly<{ outcome: 'admitted' }>>
  remove(input: Readonly<{ id: string }>): Promise<void>
  clear(): Promise<void>
}

interface ClipboardHistorySnapshot {
  revision: U64Decimal
  entries: readonly ClipboardHistoryEntrySummary[]
}

type ClipboardHistoryEntrySummary =
  | Readonly<{
      id: string
      kind: 'text'
      capturedAt: string
      textPreview: string
    }>
  | Readonly<{
      id: string
      kind: 'image'
      capturedAt: string
      previewDataUrl: string
      width: number
      height: number
    }>
  | Readonly<{
      id: string
      kind: 'files'
      capturedAt: string
      firstFileName: string
      fileCount: number
      available: boolean
    }>
```

边界要求：

- `list()` 最多返回 50 条，顺序固定为最新到最旧。
- `id` 是插件内 opaque 字符串，重启后保持稳定；删除或淘汰后不得复用。
- `capturedAt` 是原始采集时间，不随恢复粘贴或移动到最前而变化；Panel 不应依赖 `capturedAt` 自行重新排序。
- `revision` 使用规范的无前导零 `u64` 十进制字符串，并随索引持久化；采集、删除、清空、淘汰和恢复旧记录移动到最前面都必须递增。
- Panel 丢弃旧 revision，避免 `list()` 和 `onChanged()` 乱序覆盖。
- 插件只获得展示摘要。文本原文、原始 PNG、完整文件路径和用于恢复的原始内容留在宿主侧。
- 图片缩略图统一为 PNG，长边不超过 256 px，编码后不超过 256 KiB；宿主按需继续缩小直到同时满足两个限制。不得把最多 10 MiB 的原图编码进 Panel DTO。
- `onChanged(handler)` 注册后异步投递当前 snapshot；之后按 `revision` 单调递增投递，可在高频变化时合并为最新 snapshot，但不得跳回旧 revision。
- `handler` 抛错不得断开宿主监听或影响后续投递；`unsubscribe` 返回后不得再向该 handler 投递。
- 所有方法绑定当前 `pluginId`、插件 generation 和 Panel session epoch；旧引用安静停止订阅，操作调用返回会话失效错误。
- `remove()` 和 `clear()` 同时删除宿主索引与对应图片数据。

### 5.3 Host Key 扩展

复用既有 `panel.hostKeys` 串行路由、ack、超时和会话销毁协议，新增可选声明：

```ts
type AdditionalPanelHostKeyDeclaration = 'Tab' | 'Shift+Tab' | 'Enter'
```

匹配规则：

- `Tab` 只匹配无修饰键的 Tab；
- `Shift+Tab` 只匹配仅带 Shift 的 Tab；
- `Enter` 只匹配无修饰键、非 IME composing 的 Enter；
- 声明并成功匹配后，主 WebView 必须同步 `preventDefault()`；Enter 不再触发 `submitPanel`，Tab 不再执行默认焦点遍历；
- 未声明、过期会话或未 armed 时保持现有行为；
- 新 token 追加到既有规范排序之后；完整总顺序为 `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter`，Rust、Schema、CLI 和 TypeScript parser 必须一致。

建议继续使用 DOM 语义交付 `PluginPanelHostKeyEvent.key`，并通过修饰键字段区分 `Shift+Tab`：

| Manifest 声明 | 主输入框匹配 | 交付给 Panel 的事件 |
| --- | --- | --- |
| `Tab` | 无修饰键 Tab | `key: 'Tab'`, `shiftKey: false` |
| `Shift+Tab` | 仅带 Shift 的 Tab | `key: 'Tab'`, `shiftKey: true` |
| `Enter` | 无修饰键、非 IME composing 的 Enter | `key: 'Enter'` |

本请求选择 [Panel 列表 Enter 转发请求](./2026-08-26-panel-list-enter-host-key-forwarding-request.md) 的 Option B，并在其基础上额外扩展 `Tab` 与 `Shift+Tab`。8 月 26 日文档本身只覆盖 Enter 转发；本剪贴板历史能力需要宿主把三种按键一起冻结。该请求在其他插件上的兼容行为保持不变。

### 5.4 一次性恢复并粘贴

`paste({ id, routeSequence })` 是原子接纳流程，不等同于插件自行依次调用“写剪贴板、隐藏、模拟按键”：

1. 校验当前插件权限、generation、Panel session、记录 ID 和前台窗口捕获仍有效。
2. 校验 `routeSequence` 对应当前尚未消费的 `Enter` Host Key ticket；每张 ticket 最多接纳一次粘贴。
3. 文件记录在隐藏前重新检查全部路径；任一路径失效则拒绝，Panel 保持显示。
4. 宿主把原始文本、PNG 或文件列表写回系统剪贴板。
5. 接纳显式返回并让 Promise 在文档销毁前 resolve；随后隐藏 UiPilot。
6. 重新校验被捕获窗口仍存在、PID 匹配、不是 UiPilot 自有窗口，并尝试恢复前台焦点。
7. 只有目标窗口确实成为前台窗口时，宿主才发送一次平台粘贴 chord；Windows 为一次 `Ctrl+V`。

禁止接受插件提供 HWND、PID、按键或粘贴次数。该能力不能扩展为通用 `sendKeys`。

`paste()` 成功接纳后会原子消费当前 Enter ticket，并作为该 ticket 的终止确认；Panel bootstrap 随后发送的普通 ack 必须幂等 no-op。会话进入隐藏流程时取消同 epoch 的排队 Host Key，不得在新会话中迟到交付。

成功写入剪贴板但隐藏后的焦点恢复或粘贴失败时，不再尝试第二次，也不恢复旧剪贴板；所选内容保持为当前剪贴板内容，用户可以手动粘贴。

`paste()` 失败必须返回稳定、可文档化的错误名，便于 Panel 显示可预期提示。建议至少包含：

| 错误名 | 含义 |
| --- | --- |
| `PermissionDenied` | 缺少或已撤销读取/粘贴权限 |
| `ExpiredPanelSession` | Panel session、plugin generation 或 Host Key ticket 已失效 |
| `RecordNotFound` | 记录已被删除或淘汰 |
| `RecordUnavailable` | 文件记录路径失效，或图片/索引数据不可用 |
| `PasteTargetUnavailable` | 打开 UiPilot 前捕获的外部窗口已失效、PID 不匹配或不允许接收粘贴 |
| `ClipboardWriteFailed` | 宿主无法把记录写回系统剪贴板 |

JS 暴露形态应至少保证 `Error.name` 等于上表错误名；是否追加 `...Error` 后缀必须与现有 SDK 错误风格统一并在类型文档中冻结。错误消息必须保持脱敏；不得包含文本正文、完整路径、图片内容、HWND、PID 或原生系统错误细节。

## 6. 宿主组件边界

建议拆为五个职责单一的组件：

| 组件 | 职责 |
| --- | --- |
| Clipboard observer | 订阅 Windows 剪贴板变化，读取并归一化一次变化 |
| Per-plugin history store | 管理 50 条环形历史、图片文件、revision、原子持久化与生命周期清理 |
| Panel clipboard-history bridge | 校验权限和会话，提供摘要快照、订阅、删除和清空 |
| Paste coordinator | 消费 Enter ticket，写剪贴板，接纳隐藏，恢复已捕获窗口并发送一次粘贴 |
| Manifest/SDK contract | 同步权限、Host Key、Schema、CLI、TypeScript 类型和开发者文档 |

剪贴板监听与持久化是宿主服务；插件 Runtime 和 Panel JavaScript 只渲染宿主快照、维护当前筛选与选择，并提交用户意图。

## 7. 失败与恢复

- 剪贴板暂时被其他进程占用：在后台最多短重试 3 次，总预算不超过 250 ms；仍失败则跳过本次变化，不阻塞 UI 线程。
- 图片超过限制：不写入历史，不产生截断图片。
- 文件失效：保留摘要并标记不可用；粘贴前失败，Panel 显示错误且不隐藏。
- 历史记录在列表后被淘汰或删除：`paste()` 返回记录不存在，Panel 刷新最新快照。
- 写入剪贴板失败或外部目标在隐藏前失效：拒绝 `paste()`，Panel 保持显示。
- 隐藏后目标焦点或输入发送失败：停止自动操作，所选内容保留在剪贴板。
- 持久化索引损坏：隔离损坏文件并以空历史启动；不得循环崩溃或记录内容日志。
- 插件禁用、故障停用、升级、卸载或 Panel 会话替换：撤销旧订阅和未接纳的操作；迟到结果不得改写新会话。

## 8. 验收检查

### 8.1 宿主自动化测试

- [ ] 在授权且启用时采集文本、图片、单文件和多文件；禁用或撤权后停止采集。
- [ ] 最终冻结的读取权限名在 Rust、Schema、CLI、TypeScript 类型、安装确认页和开发者文档中一致；未选用的读取权限名不能作为同义权限通过 manifest 验证。
- [ ] 混合格式按“文件 → 图片 → 文本”只生成一条记录。
- [ ] HTML/RTF 与 Unicode text fallback 共存时，第一版只记录 Unicode text fallback。
- [ ] 文本、图片和文件列表按第 4.2 节 fingerprint 规则去重；连续重复不新增；恢复旧记录后只移动一次。
- [ ] 最多保留 50 条。
- [ ] Panel 文本项只收到宿主生成的 `textPreview`，最大长度、空白折叠和空值行为符合第 4.3 节。
- [ ] 历史跨重启恢复，完全卸载删除，保留数据卸载保留。
- [ ] `id` 重启后稳定，删除或淘汰后不复用；`revision` 随索引持久化，并在采集、删除、清空、淘汰和移动到最前时递增。
- [ ] 恢复旧记录移动到最前时不修改 `capturedAt`；`list()` 返回顺序来自宿主 recency 排名，不要求 `capturedAt` 倒序。
- [ ] 单张图片和图片总量限制分别为 10 MiB 和 500 MiB；淘汰同时清理图片文件。
- [ ] Panel 只收到文字预览、缩略图、文件名与数量，不收到原图或完整路径。
- [ ] `revision` 严格递增；乱序 `list()` / `onChanged()` 不能回滚 Panel 状态。
- [ ] `onChanged()` 注册后异步投递当前 snapshot；高频变化可合并但必须保持 revision 单调；handler 抛错不影响后续投递；unsubscribe 后不再投递。
- [ ] `Tab`、`Shift+Tab`、`Enter` 只在声明、当前 epoch 且 receiver armed 时路由。
- [ ] `Tab` 和 `Shift+Tab` 使用同一个 `event.key === 'Tab'` 交付，并以 `shiftKey` 区分方向；`Enter` 使用 `event.key === 'Enter'` 交付。
- [ ] 声明的 Enter 不调用 `submitPanel`；未声明 Enter 保持现有提交行为。
- [ ] 过期、伪造、重复或非 Enter 的 `routeSequence` 不能触发粘贴。
- [ ] `paste()` 失败返回稳定错误名，且所有错误消息脱敏。
- [ ] 自动粘贴顺序为“写剪贴板 → 隐藏 → 恢复捕获窗口 → 确认前台 → 一次粘贴”。
- [ ] HWND 失效、PID 不匹配、Shell 窗口或 UiPilot 自有窗口不会收到输入。
- [ ] 隐藏后的粘贴失败保留所选剪贴板内容，不重试第二次。
- [ ] 存储损坏、剪贴板占用、图片超限和文件失效均按第 7 节退化。

### 8.2 插件自动化测试

- [ ] `Tab` / `Shift+Tab` 循环切换“全部、图片、文件、文字”。
- [ ] `ArrowUp` / `ArrowDown` 更新选择且在首尾停止。
- [ ] 切换分类后选择该分类的最新记录。
- [ ] `Enter` 只调用一次 `paste({ id, routeSequence })`。
- [ ] 文本、图片、文件、空状态和失效文件按第 4.4 节显示。
- [ ] `onChanged()` 刷新时保留仍存在的当前选择；条目消失时选择最新可用项。
- [ ] 粘贴前失败显示错误并保持 Panel；接纳成功后不再启动 DOM 工作。

### 8.3 Windows 手工验收

- [ ] 在记事本复制文本，打开插件并粘贴回记事本。
- [ ] 在图片应用复制图片，打开插件并粘贴到微信聊天输入区。
- [ ] 在资源管理器复制单文件和多文件，打开插件并粘贴到微信聊天输入区。
- [ ] 从微信打开 UiPilot 后，Tab 切分类、方向键选记录、Enter 隐藏 UiPilot 并自动粘贴回同一微信窗口。
- [ ] 重启 UiPilot 后仍能查看最近 50 条记录。
- [ ] 删除历史中的源文件后，插件显示失效状态且不会隐藏或发送粘贴。

## 9. 非目标

- 读取或同步 Windows `Win+V` 历史；
- 采集 UiPilot 未运行、插件禁用或未授权期间的内容；
- 保存文件副本；
- HTML、RTF、音频或自定义剪贴板格式；
- 搜索、收藏、固定条目、云同步或跨设备同步；
- 根据来源自动识别并排除密码框或敏感输入框内容；
- 向公开插件开放任意前台窗口、任意按键或通用输入模拟；
- macOS 实现；
- 修改现有 `copyText`、`requestHide()` 或未声明 Enter 的提交语义。

## 10. 交付顺序与完成条件

这是宿主能力请求，不授权在插件任务中修改 `src/`、`src-tauri/`、`packages/plugin-cli/` 或 SDK 合同来绕过缺口。

1. 宿主程序任务先冻结最终 API DTO、权限、版本和 Windows 安全实现，并让第 8.1 节通过。
2. SDK/CLI 合同与开发指南随宿主能力交付，公开插件开发目录可以通过验证。
3. 宿主能力发布后，再单独实现 Panel 插件并让第 8.2、8.3 节通过。

当以上三项全部完成时，用户可以在微信等外部窗口中打开插件，仅用 Tab、方向键和 Enter 选择最近 50 条文本、图片或文件记录并自动粘贴。
