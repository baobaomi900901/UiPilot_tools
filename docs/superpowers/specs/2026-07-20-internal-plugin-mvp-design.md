# UiPilot 内部开发者插件 MVP 需求与设计

## 1. 文档信息

- 日期：2026-07-20
- 最近修订：2026-07-22
- 状态：书面复核稿
- 产品阶段：MVP-A 达到 Go 后的候选阶段
- 目标平台：Windows 11 x64
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 参考产品：uTools 插件应用框架

## 2. 决策摘要

本阶段交付一个仅供受控内部开发者使用的插件 MVP。插件使用普通 HTML 和 JavaScript 在无界面的隔离
WebView 中运行，只计算并返回结构化结果；搜索框、结果列表、错误提示和所有系统动作仍由 UiPilot 宿主
拥有并渲染。

首个验收插件为 `math`：用户输入 `/math 1+1`，结果面板实时显示 `2`，按 `Enter` 后由 Rust 宿主把
字符串 `2` 写入剪贴板并隐藏启动器。

UiPilot 宿主与 `internal.math` 必须是两个独立交付物。宿主不包含 `/math` 注册、表达式解析器或计算
兜底；删除 `internal.math` 整个插件目录并重启 UiPilot 后，输入 `/math` 或 `/math 1+1` 不得再出现
计算功能或计算结果。

本阶段借鉴 uTools 的清单驱动、触发词路由、Web 技术开发和插件生命周期，不复制其完整 Node.js、
Electron、多窗口、动态指令、云同步或离线分发能力。

插件 MVP 不是 MVP-A 的默认承诺。只有 [Windows 桌面启动器 MVP-A](./2026-07-17-cross-platform-launcher-mvp-design.md)
达到 Go，且产品负责人把“插件技术验证”选为唯一后续候选时，才允许开始实施。

## 3. 目标与非目标

### 3.1 目标

1. 内部开发者可以把一个符合清单约束的插件目录放入固定开发目录，并在重启 UiPilot 后加载。
2. 删除整个插件目录并重启后，该插件的触发词、结果和动作完全消失，不留下宿主内置兜底。
3. 插件可以注册一个静态斜杠触发词，接收实时查询并返回最多 20 个宿主渲染结果。
4. 插件只能申请版本化、可枚举的宿主动作；首版唯一动作是 `clipboard.writeText`。
5. 宿主保持对路由、结果时效、动作执行、权限、窗口和错误展示的最终控制。
6. 插件损坏、超时、崩溃或越权不能破坏应用搜索、主窗口或现有本地数据。
7. 用 `math` 插件完成安装可用、移除消失、恢复后重新可用的自动化端到端链路。

### 3.2 非目标

- 公开 SDK、插件市场、付费、审核、签名、自动更新或远程撤销。
- 插件自定义可见 UI、主题、设置页、独立窗口或多窗口。
- Node.js、Electron API、原生二进制、Python、WebAssembly、`eval` 或动态代码生成。
- 文件、目录、进程、Shell、任意协议、网络、通知、模拟按键或窗口控制权限。
- 动态注册或删除触发词、跨插件跳转、插件依赖或插件间通信。
- 云同步、远程遥测、插件使用统计或插件数据迁移。
- 热更新、插件管理界面或类似 uTools 开发者工具的独立产品。
- 把 `math` 发展成完整科学计算器。
- 在宿主 TypeScript、Rust 或内置资源中保留 `/math` 注册、表达式解析器或缺包兜底。

## 4. uTools 框架研究

### 4.1 框架模型

uTools 插件是 Electron 宿主加载的 Web 应用。插件目录以 `plugin.json` 为入口，使用 `main` 指定 HTML，
使用 `features` 声明一个或多个功能及其指令。指令既可以是固定文本，也可以匹配文本、图像、文件或当前
系统窗口。插件前端由 HTML、CSS 和 JavaScript 构成，源码框架需要先编译为普通 Web 资源。

可选 `preload.js` 在窗口加载前运行。官方文档明确允许它使用 Node.js 16.x 原生模块、第三方 CommonJS
模块和 Electron 渲染进程 API，并通过 `window` 向页面暴露自定义方法。

插件通过 `onPluginEnter`、`onPluginOut` 等事件接收进入、隐藏和结束生命周期；可以使用宿主主窗口的
子输入框，也可以创建独立 `BrowserWindow`。动态指令 API 允许插件在运行时增加和删除功能。

uTools 还提供系统、剪贴板、文件、窗口、本地数据库和可选云同步能力。开发阶段由 uTools 开发者工具
关联 `plugin.json`，可以连接本地 Vite 服务热更新；交付时可以生成无需审核的 UPXS 离线包，或提交
应用市场审核。

### 4.2 值得采用的设计

| uTools 设计 | UiPilot 决策 | 原因 |
| --- | --- | --- |
| 清单作为插件入口 | 采用 | 便于静态校验、发现和兼容判断 |
| 功能编码与触发词分离 | 采用并收紧为每插件一个功能 | 保留稳定身份，减少首版冲突面 |
| Web 技术编写插件 | 采用 | 复用现有 WebView2，不引入新语言运行时 |
| 进入与退出生命周期 | 采用查询、超时、禁用和宿主退出四种事件 | 满足资源回收和失败隔离 |
| 插件返回宿主搜索结果 | 采用宿主渲染 | 保持统一交互、可访问性和动作控制 |
| 预加载代码必须可读 | 采用未压缩、可审查源码原则 | 内部评审仍需要可读输入 |

### 4.3 不适合直接照搬的设计

以下项目是 uTools 面向成熟生态的便利性取舍，不等同于对 uTools 的完整安全评价；它们不满足 UiPilot
当前的最小权限和可验证性要求，因此记录为本项目的不采用项。

| 观察 | 对 UiPilot 不合理之处 | MVP 决策 |
| --- | --- | --- |
| `preload` 可使用完整 Node.js 和 Electron | 插件可绕过 Rust 权限边界访问文件、网络和进程 | 不提供 Node/Electron，只保留窄 IPC |
| 文档指定 Node.js 16.x | Node.js 16 已结束上游安全维护 | 不嵌入该运行时 |
| `createBrowserWindow` 暴露大量 Electron 选项和 `executeJavaScript` | 扩大导航、代码执行、网络和崩溃面 | 插件只有隐藏运行时，无窗口 API |
| `shellOpenPath` 和 `shellOpenExternal` 接受路径或多种协议 | 插件输入可能直接变成系统动作目标 | 首版只允许宿主写入纯文本剪贴板 |
| 动态指令可在运行时增删 | 冲突、撤销、取消、版本和审计规则不明确 | 触发词只来自启动时验证的静态清单 |
| 核心配置页未展示权限清单和协议兼容合同 | 无法在加载前证明最小能力和升级影响 | 清单显式声明版本、宿主下限和权限 |
| 插件数据库可选云同步 | 与当前本地优先、无自动外发原则冲突 | 插件 MVP 不提供存储和同步 |
| UPXS 可不经审核分发 | 安装警告不能替代来源、权限和运行时边界 | 仅扫描固定内部开发目录，不产出安装包 |
| 跨插件跳转可以依赖可读名称和指令 | 名称冲突会使路由结果不稳定 | 不实现跨插件跳转 |
| 生命周期只区分隐藏和结束 | 缺少实时查询的过期、超时和取消语义 | 宿主使用请求 ID、时限和连续超时禁用 |

## 5. 插件包与清单

### 5.1 目录

插件根目录是宿主应用数据目录下的 `plugins`。宿主只扫描该目录的直接子目录，不递归查找插件，也不
加载散落文件。每个插件最少包含：

```text
plugins/
└─ internal.math/
   ├─ plugin.json
   ├─ runtime.html
   └─ runtime.js
```

插件目录变更只在下次启动时生效。MVP 不监控文件、不提供重载按钮，也不复制或修改开发者文件。
UiPilot 安装包不捆绑 `internal.math`；内部开发者单独取得并放置该插件包。插件根删除后，下次启动扫描
不得从设置、缓存、历史结果或编译期资源恢复该插件。

### 5.2 最小清单

```json
{
  "manifestVersion": 1,
  "id": "internal.math",
  "name": "Math",
  "version": "0.1.0",
  "minHostVersion": "0.2.0",
  "runtime": "runtime.html",
  "feature": {
    "code": "calculate",
    "trigger": "/math",
    "explain": "实时计算"
  },
  "permissions": ["clipboard.writeText"]
}
```

### 5.3 校验规则

- `manifestVersion` 必须精确等于宿主支持的整数 `1`。
- `id` 为 3 到 64 个 ASCII 小写字母、数字、点或连字符，首尾必须是字母或数字；完整格式为
  `^[a-z0-9][a-z0-9.-]{1,62}[a-z0-9]$`。
- `name` 为 1 到 64 个 Unicode 标量值，只用于展示，不参与身份或路由。
- `version` 和 `minHostVersion` 使用不含预发布后缀的 `major.minor.patch` 三段格式；每段必须匹配
  `0|[1-9][0-9]*` 且不超过无符号 32 位整数。
- `minHostVersion` 高于当前宿主版本时禁用插件，不尝试降级协议。
- `runtime` 必须是插件根内的相对 `.html` 普通文件。
- `feature.code` 为 1 到 64 个 ASCII 小写字母、数字、点、下划线或连字符。
- `feature.trigger` 为 2 到 32 个 ASCII 字符，以 `/` 开头，不含空白，区分大小写。
- `permissions` 只接受已知、无重复的枚举值；首版唯一值为 `clipboard.writeText`。
- 未知字段、未知权限、缺失字段、类型错误或超出限制都使该插件整体禁用。
- 插件 ID 或触发词冲突时，所有参与冲突的插件均禁用，不使用目录顺序决定胜者。
- 插件根、运行入口和被请求资源不得是符号链接或 Windows 重解析点。
- 资源解析拒绝绝对路径、前缀路径、空组件、`.`、`..`、备用数据流和插件根外目标。

## 6. 系统架构

### 6.1 Rust 插件加载器

- 定位唯一插件根，枚举直接子目录并读取 `plugin.json`。
- 使用结构化 JSON 反序列化和显式字段校验，不做字符串拼接解析。
- 两阶段建立插件注册表：先校验单个插件，再检测全局 ID 和触发词冲突。
- 只把已启用插件加入不可变的进程内注册表。
- 每次进程启动都仅根据当前插件目录重建注册表，不持久化插件 ID、触发词、入口或动作。
- 记录插件 ID、版本、运行入口、功能和权限；不把真实路径发送到主 WebView。

### 6.2 插件运行时协调器

- 每个启用插件创建一个无边框、不可见、不可导航的 WebView。
- WebView 标签由宿主从插件 ID 派生，插件不能自行选择。
- 每个运行时绑定唯一插件 ID、当前请求和连续超时次数。
- 运行时只接收宿主定向发送的查询事件，只能调用提交结果命令。
- 宿主退出时销毁全部插件运行时，不等待插件确认。

### 6.3 查询路由

- 输入精确等于触发词，或以 `触发词 + ASCII 空格` 开头时，路由到该插件。
- 精确触发词后没有正文时仍进入插件路由，插件返回空结果。
- 宿主移除触发词并修剪其后的前导 ASCII 空格，把剩余文本原样作为插件正文。
- 不匹配任何触发词的非空输入继续执行现有应用搜索。
- 插件包不存在时，宿主不能识别或特殊处理其旧触发词；在固定空应用测试缓存中，`/math` 因此返回
  空结果。
- 一次查询只路由到一个插件，不合并插件结果与应用结果。
- 主 WebView 不知道插件入口、权限或动作负载，只展示 Rust 返回的 `ResultItem`。

### 6.4 结果与动作注册表

现有 `SearchResponse` 和 `ResultItem` 继续作为宿主到主 WebView 的私有合同。插件专用合同独立版本化，
不得直接复用主 WebView 类型作为公开 SDK。

```ts
type PluginQueryRequest = {
  protocolVersion: 1
  requestId: string
  input: string
}

type PluginQueryResponse = {
  protocolVersion: 1
  requestId: string
  items: PluginResult[]
}

type PluginResult = {
  title: string
  subtitle?: string
  action: {
    type: "copyText"
    text: string
  }
}
```

Rust 接收有效响应后，为每项生成不透明 `resultId`，并在现有结果注册表中保存插件 ID、请求 ID、权限
快照和真实 `copyText` 负载。主 WebView 执行时仍只回传当前 `requestId + resultId`。新查询、窗口隐藏、
插件禁用或宿主退出立即使对应动作失效。

### 6.5 剪贴板动作

- 插件只能返回结构化 `copyText`，不能调用剪贴板 API。
- Rust 在接收响应和执行结果两个时点都校验插件声明了 `clipboard.writeText`。
- 写入内容必须与当前注册表中保存的文本逐字一致，前端不能提交或覆盖文本。
- 成功写入纯文本后，复用现有统一清空结果并隐藏窗口流程。
- 写入失败时保持窗口和当前结果有效，返回固定错误，不清空或伪装成功。
- 插件动作不增加现有应用启动、激活或使用次数，也不写入验证数据。

## 7. 查询协议与限制

### 7.1 请求时效

- 每次插件查询由 Rust 生成新的不透明请求 ID。
- 主输入每次变化立即产生新请求，不增加 300ms 防抖。
- 宿主只接受标签绑定插件对其当前请求提交的响应。
- 旧请求、未知请求、重复响应和禁用后的响应直接丢弃，不改变当前结果。
- 单次响应时限为 500ms。超时返回空结果，不在实时输入期间弹出错误。
- 任一按时、格式有效的响应把连续超时计数清零。
- 同一插件连续 3 次超时后禁用至下次启动，并使其全部请求和结果失效。

MVP 不要求中断已经开始的 JavaScript 计算；请求 ID 和结果失效提供逻辑取消。插件必须自行避免长任务。

### 7.2 输入与响应上限

- 插件输入正文最多 256 个 Unicode 标量值，超出时不派发插件请求并显示固定输入过长错误。
- 每次响应最多 20 项。
- `title` 最多 256 个 Unicode 标量值，`subtitle` 最多 512 个 Unicode 标量值。
- `copyText.text` 的 UTF-8 编码最多 4 KiB。
- 完整序列化响应最多 128 KiB。
- 任一项无效时拒绝整份响应，不发布部分结果。
- 错误 DTO 只包含固定 `code` 和固定 `message`，不包含输入、结果文本、插件路径或脚本错误正文。

## 8. 隔离与权限

### 8.1 来源和资源

- 每个插件使用与插件 ID 绑定的独立本地来源，不继承主 WebView 来源。
- 自定义资源处理器只提供插件根内已验证的普通文件。
- MVP 只提供 `.html` 和 `.js` 资源，MIME 分别固定为 `text/html` 和 `text/javascript`；不信任插件
  提供的扩展 MIME 或响应头。
- 顶层导航、表单提交、下载、新窗口和远程 URL 全部拒绝。

### 8.2 Tauri Capability

- 所有插件运行时只获得精确的 `publish_plugin_results` 命令及接收定向事件所需的最小事件权限。
- 现有八个生产命令继续只授权 `main`，且 Rust 侧 main-label guard 保持不变。
- `publish_plugin_results` 的 Rust guard 必须在读取注册表或产生副作用前验证插件窗口标签。
- 窗口标签解析得到的插件 ID 必须与当前请求注册的插件 ID 精确一致。
- 不提供通用命令转发、任意事件广播、路径、Shell、HTTP 或 JavaScript 执行命令。

### 8.3 CSP

插件来源使用独立 CSP，最低要求为：

```text
default-src 'none';
script-src 'self';
style-src 'self';
img-src 'self' data:;
connect-src ipc: http://ipc.localhost;
object-src 'none';
frame-src 'none';
worker-src 'none';
base-uri 'none';
form-action 'none'
```

不得启用 `'unsafe-eval'`、`'wasm-unsafe-eval'`、远程脚本、远程样式或远程图片。CSP 只是纵深防御，
不能替代 Capability、Rust guard、路径校验和动作权限检查。

### 8.4 信任模型与 No-Go 条件

内部插件代码被视为受信任、可评审的团队代码。MVP 的隔离目标是防止错误配置、意外越权和单插件故障，
不声称抵抗有意使用浏览器漏洞的恶意代码。

实施前必须用卡死、崩溃和越权测试插件验证 WebView2 行为。出现以下任一情况时，隐藏 WebView 方案为
No-Go，不得通过标记插件为“内部可信”绕过：

- 插件无限循环使主输入、结果列表或关闭操作失去响应。
- 插件崩溃导致主 WebView、Rust 宿主或其他插件退出。
- 插件可以调用任一现有主窗口命令。
- 插件可以绕过 CSP、资源根、插件 ID 或声明权限校验。
- 宿主无法检测插件运行时退出并使其结果失效。

No-Go 后只允许比较独立进程 JavaScript 运行时与 WebAssembly 运行时，不继续扩张当前 WebView 方案。

## 9. `math` 验收插件

### 9.1 用户流程

1. 用户唤起 UiPilot 并输入 `/math 1+1`。
2. 宿主识别 `/math`，向 `internal.math` 发送正文 `1+1`。
3. 插件返回标题 `2`、副标题 `复制结果` 和 `copyText("2")`。
4. 宿主发布一项结果，主 WebView 按现有列表和键盘语义展示。
5. 用户按 `Enter`。
6. Rust 从当前结果注册表解析动作，把 `2` 写入纯文本剪贴板。
7. 成功后结果失效且启动器隐藏；失败时窗口和结果保持可见。

### 9.2 表达式范围

`math` 是插件平台样例，不是宿主功能。它必须使用明确解析器，禁止 `eval`、`Function` 构造器或动态
脚本。MVP 语法只包含：

- 十进制整数和小数。
- ASCII 空格。
- 括号。
- 二元 `+`、`-`、`*`、`/`。
- 一元正号和负号。

运算遵守括号、一元符号、乘除、加减的通常优先级，并按从左到右结合。数值使用 JavaScript `Number`；
有限结果以 `Number.prototype.toString()` 输出，负零输出为 `0`。输入不完整、语法错误、除零、`NaN` 或
无穷结果返回空列表，不在逐字输入时显示错误。

变量、函数、常量、单位、百分号、幂、隐式乘法、进制、科学计数法和任意精度计算不属于 MVP。

### 9.3 可插拔性负向合同

- `/math` 字面触发词和表达式解析实现只属于 `internal.math` 插件包；宿主通用代码不得引用它们。
- 宿主可以提供通用清单校验、触发词路由、结果注册和 `clipboard.writeText` 动作，但不能判断或计算
  数学表达式。
- 删除 `plugins/internal.math` 并重启后，插件注册表中不存在 `internal.math` 或 `/math`。
- 此时输入 `/math` 或 `/math 1+1` 只按普通应用查询处理，不创建插件 WebView、不派发插件请求、
  不显示计算结果，也不提供剪贴板动作。
- 把原插件包完整放回并再次重启后，`/math 1+1` 恢复返回 `2`；恢复不得依赖修改宿主文件。

## 10. 生命周期与错误语义

对主 WebView 返回的新增错误沿用现有 camelCase `CommandError` 约定：

| code | 固定 message |
| --- | --- |
| `pluginInputTooLong` | `plugin input is too long` |
| `pluginRuntimeFailed` | `plugin runtime failed` |
| `pluginResponseInvalid` | `plugin response is invalid` |
| `pluginPermissionDenied` | `plugin permission is denied` |
| `clipboardWriteFailed` | `clipboard write failed` |

旧结果继续复用现有 `staleRequest` 和 `unknownResult`。启动阶段的
`pluginManifestInvalid`、`pluginConflict`、`pluginHostUnsupported` 和 `pluginRuntimeFailed` 只进入受限日志，
不向尚未打开的主窗口发送错误。超时中的前两次静默返回空结果；第三次记录 `pluginTimedOut` 并禁用插件。

| 场景 | 宿主行为 |
| --- | --- |
| 单个清单或资源无效 | 禁用该插件，其他插件和应用搜索继续工作 |
| 插件 ID 或触发词冲突 | 禁用全部冲突参与者，不选择隐式胜者 |
| 宿主版本不兼容 | 禁用插件并记录固定兼容错误 |
| 插件启动失败或运行时退出 | 禁用插件并使其请求、结果和动作失效 |
| 响应超时 | 本次为空；连续 3 次后禁用至重启 |
| 响应格式或大小无效 | 拒绝整份响应，不发布部分结果 |
| 请求 ID 过期或未知 | 静默丢弃，当前新结果不变 |
| 插件缺少动作权限 | 拒绝整份响应并记录固定权限错误 |
| 主 WebView 提交旧结果 ID | 返回现有 `staleRequest` 或 `unknownResult`，不执行剪贴板 |
| 剪贴板写入失败 | 保持窗口和结果，显示固定错误 |
| 宿主退出 | 先使插件结果失效，再销毁全部插件 WebView |

日志可以包含固定错误码、插件 ID 和版本，但不得包含查询正文、结果文本、剪贴板文本、真实路径、脚本
堆栈或 WebView 原始错误正文。插件 MVP 不增加远程遥测或导出字段。

## 11. 性能与可访问性

### 11.1 性能

沿用 MVP-A 的 Windows 11 参考环境、Release 构建和测试时钟约束。新增测试事件：

- `plugin_query_dispatched_rust`：Rust 向目标插件发出查询。
- `plugin_response_accepted_rust`：Rust 完成响应校验并发布结果。
- `plugin_results_committed_ui`：主 WebView 把插件结果提交到 DOM。

主指标直接计算同一 UI 时钟域内 `query_input_ui` 到 `plugin_results_committed_ui` 的差值。
`plugin_query_dispatched_rust` 到 `plugin_response_accepted_rust` 只在 Rust `Instant` 时钟域单独报告，并以
请求 ID 关联；两组时钟不得混算。对 1,000 条固定有效表达式运行基准，前 50 条预热不计，主指标 P95
不超过现有应用搜索门槛 100ms。每次只保留最后一次输入的结果。

### 11.2 可访问性

- 插件结果继续使用主列表的 `listbox`、`option` 和 `aria-selected` 语义。
- 插件不能注入 DOM、样式、ARIA 或焦点行为。
- 结果选择和剪贴板错误继续通过现有无障碍状态区域通知。
- 副标题只写 `复制结果`，不增加依赖键盘快捷键说明的可见文本。

## 12. 验收与测试合同

### 12.1 单元测试

1. 清单字段、未知字段、版本、ID、触发词、权限和相对入口校验。
2. 符号链接、重解析点、绝对路径、`..`、备用数据流和插件根逃逸拒绝。
3. 两阶段冲突检测同时禁用重复 ID 或触发词参与者。
4. 精确触发词、触发词加空格、相似前缀和普通应用查询路由。
5. 请求 ID 的最新响应、旧响应、重复响应、隐藏和禁用竞态。
6. 500ms 超时、成功重置计数和连续 3 次超时禁用。
7. 结果数量、字段长度、动作文本和完整消息大小上限。
8. 权限在响应接收和动作执行两个时点都被检查。
9. stale/unknown 结果不触达剪贴板适配器。
10. `math` 的优先级、一元符号、括号、小数、无效输入、除零和有限结果格式。

### 12.2 集成与安全测试

1. 插件窗口只能调用 `publish_plugin_results` 和所需的最小事件 API。
2. 插件调用八个现有生产命令均在任何状态访问或副作用前被拒绝。
3. 无权限插件伪造 `copyText`、插件 ID、请求 ID 或窗口标签全部被拒绝。
4. 安全测试插件尝试 Node、Electron、网络、文件、`eval`、WebAssembly、iframe、导航和新窗口均失败。
5. 资源处理器不能读取另一个插件、宿主应用数据或插件根外文件。
6. 插件运行时退出后，所有相关结果立即失效且应用搜索仍可用。
7. 无限循环和崩溃测试证明主输入、主结果、关闭和 Rust 宿主仍然响应；失败即触发 No-Go。

### 12.3 端到端验收

1. 启动时从固定开发目录加载 `internal.math`。
2. 输入 `/math 1+1` 后出现唯一结果 `2`；延迟门槛只由第 11.1 节的批量基准裁决。
3. 快速输入 `/math 1+1`、随后改为 `/math 2+2`，最终只能显示 `4`。
4. 按 `Enter` 后剪贴板精确等于 `2`，结果映射失效且启动器隐藏。
5. 自动测试在执行前保存测试环境剪贴板，结束后恢复，避免污染开发机数据。
6. 模拟剪贴板失败时窗口保持可见，结果仍可重试，错误通过无障碍状态区域呈现。
7. 插件查询和动作不改变应用启动、激活、使用次数或验证数据。
8. 关闭 UiPilot，删除整个 `plugins/internal.math` 目录并重新启动；固定空应用缓存下输入 `/math` 和
   `/math 1+1` 均返回空结果，且没有插件运行时、插件请求或剪贴板动作。
9. 恢复同一插件目录并再次重启；不修改宿主文件即可重新得到 `/math 1+1 -> 2`。
10. 构建和启动不含 `internal.math` 的宿主独立交付物，证明宿主不依赖样例插件才能运行。

## 13. 进入与退出门槛

### 13.1 进入条件

- MVP-A 已按主设计第 3.4 节判定为 Go。
- 产品负责人已把插件选为唯一后续候选。
- WebView2 卡死、崩溃和 Capability 技术 Spike 已排期为实施第一步。
- 当前主窗口八命令、CSP、ResultRegistry 和隐私合同保持为插件工作的基线。

### 13.2 Go 条件

- 第 12 节全部自动化测试通过。
- 隐藏 WebView 的卡死和崩溃测试未影响主窗口或 Rust 宿主。
- 所有越权探针被 Capability、Rust guard、CSP 或资源边界明确拒绝。
- `/math` 端到端流程与 P95 100ms 门槛通过。
- 删除 `internal.math` 后 `/math` 消失、恢复插件包后功能恢复的负向可插拔性验收通过。
- 插件失败不改变现有应用搜索、设置、验证数据和生命周期行为。
- 代码复审确认没有通用 Shell、路径、网络或命令转发接口。

### 13.3 No-Go 条件

- 任一插件能调用主窗口命令或绕过声明权限。
- 插件卡死或崩溃会使主窗口、Rust 宿主或其他插件不可用。
- 无法可靠阻止插件根外资源、远程导航、网络或动态代码执行。
- 旧响应可以覆盖新查询，或旧结果可以执行剪贴板动作。
- `/math` 在参考环境中无法达到 P95 100ms，且一次不超过两周的针对性修正仍失败。

No-Go 后不交付内部插件 MVP，不增加例外开关。后续只能另写独立进程或 WebAssembly 运行时设计。

## 14. 已确认决策

- 目标用户是受控内部开发者，不是公开生态开发者。
- 插件 UI 全部由宿主渲染。
- 插件代码在隔离的隐藏 WebView 中以普通 JavaScript 运行。
- 每个 MVP 插件只声明一个静态功能和触发词。
- 首版唯一宿主动作是声明权限后的纯文本剪贴板写入。
- `math` 插件是平台验收样例，不进入宿主内置应用搜索逻辑。
- UiPilot 宿主和 `internal.math` 是独立交付物；删除插件包并重启后 `/math` 必须完全消失。
- 宿主不持久化插件注册，不包含 `/math` 特判、表达式解析器或缺包兜底。
- 不照搬 uTools 的 Node/Electron、动态指令、多窗口、云同步和分发模型。
- 插件目录变化在重启后生效，不建设开发者工具或热更新。
- 隐藏 WebView 无法隔离卡死或崩溃时，当前方案直接 No-Go。

## 15. 参考资料

- [uTools 第一个插件应用](https://www.u-tools.cn/docs/developer/basic/first-plugin.html)
- [uTools 插件应用目录结构](https://www.u-tools.cn/docs/developer/information/file-structure.html)
- [uTools plugin.json 核心配置](https://www.u-tools.cn/docs/developer/information/plugin-json.html)
- [uTools preload 预加载脚本](https://www.u-tools.cn/docs/developer/information/preload.html)
- [uTools 事件](https://www.u-tools.cn/docs/developer/utools-api/events.html)
- [uTools 窗口](https://www.u-tools.cn/docs/developer/utools-api/window.html)
- [uTools 系统 API](https://www.u-tools.cn/docs/developer/utools-api/system.html)
- [uTools 数据存储](https://www.u-tools.cn/docs/developer/utools-api/db.html)
- [uTools 动态指令](https://www.u-tools.cn/docs/developer/utools-api/features.html)
- [uTools 调试插件应用](https://www.u-tools.cn/docs/developer/basic/debug-plugin.html)
- [uTools 离线安装包](https://www.u-tools.cn/docs/developer/basic/offline-plugin.html)
- [uTools 发布到应用市场](https://www.u-tools.cn/docs/developer/basic/publish-plugin.html)
- [Node.js 发布与维护状态](https://nodejs.org/en/about/previous-releases)
- [UiPilot Windows 桌面启动器 MVP-A](./2026-07-17-cross-platform-launcher-mvp-design.md)

以上网页资料访问日期均为 2026-07-20。
