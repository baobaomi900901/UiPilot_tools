# UiPilot 公开插件命令与单窗口 MVP 设计

## 1. 文档信息

- 日期：2026-08-13
- 状态：对话设计已批准，等待书面复核
- 产品阶段：公开插件平台第一阶段 MVP
- 目标平台：Windows 11 x64；公开合同预留 macOS
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 验收插件：`com.uipilot.demo`，默认启动名称 `/demo`

## 2. 决策摘要

UiPilot 第一阶段把现有内部插件能力扩展为可公开使用的、清单驱动的命令插件平台。任何开发者都可以
制作本地插件包；UiPilot 仍拥有命令路由、权限、结果渲染、系统动作、窗口外壳、故障隔离和插件生命
周期的最终控制权。

一个插件在清单中静态选择一种输出模式：

- `mainResult`：插件返回纯文本结构化结果，由主窗口统一渲染；
- `window`：插件返回 JSON 数据，由 UiPilot 打开或复用该插件唯一的子窗口。

`outputMode` 是插件开发者写入包内 `plugin.json` 的静态合同，不是用户设置。第一版每个插件只有一个
命令和一个子窗口。窗口模式只能在用户按 `Enter` 后运行，不能随输入变化自动弹窗。

新建独立 `/demo` 示例插件验证公开合同。现有 `/math` 的插件包和用户行为不修改；`/find` 继续是系统
保留指令，不在本阶段迁移成插件。

## 3. 与既有设计的关系

本设计建立在以下既有实现和设计上：

- [内部开发者插件 MVP](./2026-07-20-internal-plugin-mvp-design.md) 提供现有隐藏 WebView Runtime、请求
  时效、宿主结果渲染和动作注册基础；
- [开发插件版本管理](./2026-07-24-development-plugin-version-management-design.md) 继续作为安装、更新、
  reload、generation、事务恢复和回滚的来源；
- [单实例 `/find` 窗口](./2026-08-11-find-single-window-design.md) 提供经过验证的主窗口焦点交接、非置顶
  图钉、失焦隐藏和陈旧完成隔离原则。

本设计只在冲突处覆盖既有合同：

1. 新的公开插件使用 `schemaVersion` 清单；现有 `/math` 使用的内部 `manifest: 1` 清单继续由兼容加载器
   原样支持，不要求迁移或修改包。
2. 公开插件命令的包内名称不含 `/`，用户可以在 UiPilot 设置中改名；旧内部清单的 `feature.trigger`
   仍按既有规则工作。
3. 公开插件可以声明一个宿主管理的可见子窗口；旧内部插件默认仍只有隐藏 Runtime。
4. 新公开 API 的响应总上限为 64 KiB；该限制不追溯修改旧内部协议。

除上述覆盖项外，既有安装事务、路径身份校验、资源快照、generation、ResultRegistry 和 command caller
guard 约束保持有效。实现可以提取共用组件，但不得改变 `/math` 或 `/find` 的可观察行为。

## 4. 阶段与范围

### 4.1 第一阶段 MVP

第一阶段交付：

- 本地 `.uipilot-plugin` 包安装和开发目录加载；
- 严格清单校验、平台/API 兼容检查和权限确认；
- 每插件一个可改名的斜杠命令；
- `live | submit` 两种激活模式；
- `mainResult | window` 两种静态输出模式；
- 主窗口纯文本结构化结果和宿主执行的剪贴板写入动作；
- 每插件一个宿主管理的可见窗口；
- 插件设置的宿主统一渲染、私有存储和敏感值安全存储；
- 请求所有权、超时、崩溃隔离和连续故障自动停用；
- 公开 TypeScript 类型、JSON Schema、开发说明和独立 `/demo` 示例插件。

### 4.2 后续独立阶段

以下已确定方向不在第一阶段实现，必须分别形成后续规格和计划：

- 后台持久化定时任务；
- 主程序消息中心、托盘未读闪烁和通知限流；
- 在线插件市场、签名信任链和自动更新；
- 多窗口、多实例窗口及插件间通信；
- 网络代理、剪贴板读取、文件选择和全文件索引的可调用 API。

第一阶段会保留这些能力的权限名称和安全原则，但插件声明宿主尚未实现的权限时不得运行。

## 5. 用户合同

### 5.1 命令发现与解析

- 输入 `/` 时，主窗口列出所有已启用、平台兼容、API 兼容且健康的公开插件。
- 输入 `/d` 时，候选按当前启动名称和插件显示名称过滤。
- 输入完整 `/demo` 时，主窗口显示插件说明和输入提示。
- 输入 `/demo str` 后，不混入应用、其他插件或系统命令候选。
- 禁用、不兼容、清单无效或因连续故障自动停用的插件不出现在候选中。
- 一次输入最多路由到一个插件。

插件命令由 `/<effectiveName>` 或 `/<effectiveName><ASCII 空格><正文>` 构成。宿主移除命令和分隔
空格，删除正文首尾空白，但逐字保留正文内部空格。例如 `/fy   我是 杰克  ` 传给插件的 `input` 是
`我是 杰克`。第一版不解析子命令、选项、参数类型或参数 Schema，正文始终是一个原始字符串。

如果清单声明 `inputRequired: true`，只有命令没有正文时，主窗口显示宿主统一的输入提示，不调用插件。
如果输入可选，插件会收到空字符串。

### 5.2 激活模式

- `live`：主输入变化后由宿主执行约 150 ms 防抖并调用插件；结果立即预览。用户按 `Enter` 执行当前
  选中结果的默认动作。
- `submit`：输入阶段不调用插件；第一次 `Enter` 调用插件并展示结果或打开窗口。若输出为主窗口结果，
  后续 `Enter` 才执行选中结果的默认动作。

`outputMode: "window"` 只能与 `activationMode: "submit"` 组合。`mainResult` 可以与两种激活模式组合。

### 5.3 `/demo` 窗口模式

`/demo` 默认使用 `submit + window`。用户输入 `/demo str` 并按 `Enter` 后：

1. UiPilot 调用 `/demo` Runtime；
2. Runtime 计算 `str yyyy-mm-dd`，日期取本机调用时的当前日期；
3. UiPilot 创建或复用 `com.uipilot.demo` 唯一的子窗口；
4. 窗口显示以下字段：

   - 主窗口输入字符串：`str`
   - 主窗口系统类型：`windows` 或 `macos`
   - 主窗口 UI 风格：本次调用时实际生效的 `dark` 或 `light`
   - 当前子窗口编号：`1`
   - 返回值：`str yyyy-mm-dd`

5. 窗口成功更新并激活后，主输入清空，主窗口隐藏；
6. 窗口创建、更新或激活失败时，主窗口保持可见并保留原输入，显示宿主统一错误。

只输入 `/demo` 时不调用 Runtime，也不打开窗口。

再次提交 `/demo newstr` 时不创建第二个窗口，而是向编号 `1` 的窗口发送最新数据、应用本次主题并激活
它。旧请求的迟到结果不能覆盖 `newstr`。

### 5.4 `/demo` 主结果模式

开发者把包内 JSON 改为 `submit + mainResult`、声明对应权限并 reload 后：

1. 用户输入 `/demo str`，第一次 `Enter` 调用插件；
2. 主窗口显示一项 `str yyyy-mm-dd`；
3. 第二次 `Enter` 由 UiPilot 把该文本写入剪贴板；
4. 成功写入后沿用主窗口现有动作完成流程；失败时结果和窗口保持可见。

两种输出模式可以复用 `/demo` 内部的日期拼接函数，但 UiPilot 只调用清单当前模式对应的公开处理器。
插件不能在一次调用中动态选择模式。

## 6. 术语

- **插件身份（plugin ID）**：插件永久、稳定的包身份，例如 `com.uipilot.demo`。升级不能改变。
- **默认启动名称（default name）**：插件包声明的不带 `/` 的初始命令名。
- **有效启动名称（effective name）**：用户覆盖名称存在时使用覆盖值，否则使用默认值。
- **激活模式（activation mode）**：决定 Runtime 在输入变化时还是按 `Enter` 后运行。
- **输出模式（output mode）**：决定有效响应进入主结果区还是插件唯一窗口。
- **调用（invocation）**：宿主对一个插件处理器的一次请求，以 `requestId` 唯一标识。
- **插件 generation**：一次已提交包版本/Runtime 所有权。升级、reload 或重新启用产生新 generation。
- **提交所有者（submission owner）**：主窗口提交时捕获的视图世代、控件值和 token；只有所有者可以清空
  输入、显示错误或触发窗口交接。
- **插件窗口**：由 UiPilot 创建的宿主外壳，以及外壳内隔离的插件内容页。第一版每插件最多一个。
- **窗口更新**：一次已验证 `WindowResponse` 经宿主附加环境信息后发送给插件内容页的数据包。

这些标识不能互换。`requestId` 不作为窗口编号，generation 不作为用户可见版本，窗口编号在第一版固定为
整数 `1`。

## 7. 公开插件包与清单

### 7.1 包结构

`.uipilot-plugin` 是使用 ZIP 文件格式、扩展名固定为 `.uipilot-plugin` 的单插件单版本归档。
`plugin.json` 必须直接位于归档根，归档内不得再包一层 plugin ID 目录。开发目录本身视为归档根，使用
相同的相对路径和资源校验。TypeScript 源码必须由开发者预先构建为浏览器可执行的 JavaScript；UiPilot
不在用户机器上安装依赖或编译源码。

窗口模式的典型包结构为：

```text
plugin.json
dist/
├─ runtime.js
├─ window.html
├─ window.js
└─ window.css
```

归档解包、路径、重解析点、普通文件、资源预算、digest、事务提交和崩溃恢复沿用既有插件版本管理合同。
包内入口必须是包根内的规范相对路径，拒绝绝对路径、`.`、`..`、备用数据流、符号链接和重解析点。
ZIP 条目名统一使用 `/`，拒绝加密条目、重复规范路径、大小写折叠后冲突的路径和解压后超出既有包预算
的归档。先完整验证并解压到事务 staging，绝不直接从用户选择的归档执行资源。

### 7.2 清单示例

```json
{
  "schemaVersion": 1,
  "pluginId": "com.uipilot.demo",
  "version": "1.0.0",
  "apiVersion": 1,
  "minimumHostVersion": "0.2.0",
  "name": "Demo",
  "description": "UiPilot 插件接口示例",
  "supportedPlatforms": ["windows", "macos"],
  "command": {
    "defaultName": "demo",
    "activationMode": "submit",
    "outputMode": "window",
    "inputRequired": true,
    "inputPlaceholder": "请输入内容"
  },
  "runtime": {
    "entry": "dist/runtime.js"
  },
  "window": {
    "entry": "dist/window.html"
  },
  "permissions": ["ui.window"]
}
```

### 7.3 清单校验

- `schemaVersion` 必须精确为整数 `1`；未知字段一律拒绝。
- `pluginId` 为 1 到 64 个 ASCII 小写字母、数字、点或连字符，且不能以点或连字符开头或结尾。
- `version` 和 `minimumHostVersion` 使用规范 `major.minor.patch` 版本。
- `apiVersion` 是公开 API 主版本整数。宿主在同一主版本内保持向后兼容。
- `name` 必须是非空纯文本；`description` 是可选纯文本，不解析 Markdown 或 HTML。
- `supportedPlatforms` 是无重复的 `windows | macos` 非空数组。
- 第一版清单必须且只能声明一个 `command`。
- `defaultName` 必须匹配 `^[a-z][a-z0-9-]{0,31}$`，不含 `/`。
- `activationMode` 只能是 `live | submit`；`outputMode` 只能是 `mainResult | window`。
- `window` 模式必须是 `submit`、声明 `ui.window` 并提供唯一 `window.entry`。
- `mainResult` 模式不能声明 `window`；未使用的窗口资源可以存在于包中，但不会获得窗口能力。
- `inputRequired: true` 时必须提供非空纯文本 `inputPlaceholder`。
- `runtime.entry` 必须指向包内 `.js` 普通文件；`window.entry` 必须指向包内 `.html` 普通文件。
- 权限必须来自已知枚举、无重复，并满足当前输出和动作合同。
- 插件 ID、有效启动名称、系统保留名称或安装状态冲突时失败关闭，不使用扫描顺序选择胜者。

无效包不能运行。平台/API 不兼容或使用“已知但当前宿主未实现”的权限时，包可以保留安装和数据，但状态
必须是不可启用并显示具体原因。

### 7.4 API 兼容

- 插件必须声明 `apiVersion` 和最低宿主版本。
- 同一 API 主版本只允许兼容新增；删除字段、改变语义或收紧既有合法输入必须发布新主版本。
- UiPilot 可以同时支持有限数量的旧主版本；停止支持前必须在设置页和开发文档中提前提示。
- 不兼容插件保持安装、设置和私有数据，但不能创建 Runtime、注册路由或打开窗口。
- 旧内部 `manifest: 1` 与公开 `schemaVersion: 1` 由顶层鉴别字段区分，不把一个格式猜测成另一个格式。

## 8. 插件设置 Schema

插件可以在清单顶层声明可选的 `settings` 数组，由 UiPilot 统一渲染。插件不能向主设置页注入 HTML、
CSS 或脚本。公开定义为：

```ts
type PluginSettingDefinition =
  | {
      key: string
      type: "text"
      label: string
      default?: string
    }
  | {
      key: string
      type: "secret"
      label: string
    }
  | {
      key: string
      type: "number"
      label: string
      default?: number
      min?: number
      max?: number
      step?: number
    }
  | {
      key: string
      type: "boolean"
      label: string
      default?: boolean
    }
  | {
      key: string
      type: "select"
      label: string
      options: Array<{ value: string; label: string }>
      default?: string
    }
```

`key` 必须匹配 `^[a-z][a-z0-9.-]{0,63}$`，在插件内唯一且升级后保持稳定。`label`、选项标签和文本
默认值均为纯文本。数值必须有限，`min <= max`、`step > 0`，默认值必须满足范围。`select.options` 必须
非空且 `value` 唯一，默认值必须来自选项。`secret` 禁止默认值，避免凭据进入插件包。

普通值存入该插件的隔离数据域；`secret` 写入操作系统安全存储，Runtime 只能查询“是否已配置”，不能
重新读取明文。未来受控网络代理可以按字段引用密钥，但不能把密钥返回给插件 JavaScript。

`outputMode`、`activationMode`、权限和入口不属于用户设置，不能通过设置页或 Runtime 修改。`/demo`
本身不声明用户设置字段。

## 9. Runtime 与公开接口

### 9.1 Runtime 装载

每个启用插件有一个隔离的隐藏 Runtime。宿主启动只读 bootstrap，以 ES module 方式导入清单指定的
`runtime.entry`；禁止 Node.js、Electron、Tauri 全量 API、`eval`、`Function`、WebAssembly、原生模块、
`.exe`、`.dll` 和 Shell 命令。

bootstrap 在 Runtime ready 前确认清单当前模式所需导出函数存在。缺少处理器、导入失败或处理器类型
错误使当前 generation 不可用，不发布路由。

```ts
interface PluginRuntimeModule {
  onMainResult?: (
    invocation: PluginInvocation
  ) => Promise<MainResultResponse>

  onWindow?: (
    invocation: PluginInvocation
  ) => Promise<WindowResponse>
}
```

UiPilot 只调用清单 `outputMode` 对应的处理器。存在另一个处理器不会授予额外能力。

### 9.2 调用 DTO

```ts
interface PluginInvocation {
  apiVersion: 1
  requestId: string
  input: string
  context: {
    platform: "windows" | "macos"
    theme: "dark" | "light"
    invokedAt: string
  }
}
```

- `requestId` 是宿主生成的非空不透明字符串，只在当前 plugin ID + generation 内有效。
- `input` 是已移除命令和边界空白、保留内部空格的正文。
- `platform` 是当前操作系统，不从插件包或窗口 User-Agent 推断。
- `theme` 是调用时主窗口实际生效的配色，不暴露 `system` 偏好。
- `invokedAt` 是包含本地 UTC 偏移的 RFC 3339 时间。`/demo` 使用其本地日期部分产生 `yyyy-mm-dd`。

### 9.3 Runtime 宿主 API

Runtime 获得一个冻结、按 plugin ID + generation 绑定的窄宿主对象：

```ts
type JsonPrimitive = null | boolean | number | string
type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | { [key: string]: JsonValue }

interface UiPilotPluginApiV1 {
  storage: {
    get(key: string): Promise<JsonValue | null>
    set(key: string, value: JsonValue): Promise<void>
    remove(key: string): Promise<void>
  }
  settings: {
    get(key: string): Promise<JsonValue | null>
    isSecretConfigured(key: string): Promise<boolean>
  }
}
```

JSON 数值必须有限，拒绝 `NaN` 和无穷值；对象拒绝重复键和原型特殊键。`storage` 只能访问本插件数据
域，存储 JSON 值，序列化总量上限 5 MiB。达到上限时 `set` 原子失败，旧值保持。`settings.get` 只允许
清单声明的非敏感字段；`secret` 只能调用 `isSecretConfigured`。私有存储写入是前台处理器唯一允许的
有界内部状态变更；剪贴板、文件、网络、窗口和其他外部副作用仍只能通过宿主结构化动作完成。

Runtime 不能枚举、读取或调用其他插件，也不能获得真实插件路径。禁用、卸载、升级或 generation 失效后，
旧宿主对象的全部调用失败关闭。

### 9.4 主窗口结果 DTO

```ts
interface MainResultResponse {
  requestId: string
  results: PluginResult[]
}

interface PluginResult {
  id: string
  title: string
  subtitle?: string
  detail?: string
  defaultAction?: PluginAction
  actions?: PluginAction[]
}

type PluginAction = {
  type: "copyText"
  text: string
  label?: string
}
```

第一版只开放 `copyText`。插件返回动作意图，不能直接写剪贴板；Rust 把真实动作负载保存到当前结果注册
表，前端只回传不透明的请求/结果身份。执行时再次校验 generation、当前结果和 `clipboard.write` 权限。

结果合同：

- 一次响应 0 到 20 项；
- `id` 在响应内非空且唯一；
- `title` 最多 256 个 Unicode 标量值；
- `subtitle` 最多 512 个 Unicode 标量值；
- `detail` 的 UTF-8 编码最多 16 KiB；
- 所有文本都是纯文本，不解析 HTML、Markdown、脚本或远程图片；
- 无默认动作的结果只展示；有多个结果时沿用主窗口上下键选择；
- 任一字段或动作无效时拒绝整份响应，不截断、不发布部分结果。

### 9.5 窗口响应与更新 DTO

```ts
interface WindowResponse {
  requestId: string
  data: JsonValue
}

interface PluginWindowUpdate {
  requestId: string
  input: string
  platform: "windows" | "macos"
  theme: "dark" | "light"
  invokedAt: string
  instanceNumber: 1
  data: JsonValue
}
```

Runtime 只返回业务 `data`。宿主校验响应后，从原始 `PluginInvocation` 复制环境字段，构造
`PluginWindowUpdate`；插件不能伪造平台、主题、调用时间或窗口编号。

插件窗口内容页只获得以下单向桥接，不获得 Runtime 宿主 API、Tauri invoke 或任意窗口控制接口：

```ts
interface UiPilotPluginWindowApiV1 {
  onUpdate(
    handler: (update: PluginWindowUpdate) => void | Promise<void>
  ): () => void
}
```

`window.html` 首先加载 UiPilot 的只读 bootstrap。插件注册第一个 `onUpdate` handler 后，bootstrap 自动
向宿主确认内容页 ready；插件没有可伪造身份或指定窗口 label 的 ready API。每次更新由 bootstrap 校验
并按 `requestId` 串行交给 handler，待同步返回或 Promise 完成后自动发送内部 ack。宿主只接受当前
plugin ID + generation + request 的 ready/ack；处理器抛错、超时或旧 ack 均按窗口更新失败处理。窗口
内容 ready 和单次更新 ack 各自最多等待 5 秒。

内容页不能向主窗口返回结果，不能修改图钉/关闭状态，也不能创建第二个窗口。

### 9.6 响应预算

`MainResultResponse` 或 `WindowResponse` 的完整 UTF-8 JSON 序列化结果最多 64 KiB。宿主先检查字节数，再
进行完整结构和语义校验。超限或无效时拒绝整份响应。窗口更新中由宿主附加的固定环境字段不计入插件
响应预算，但最终更新仍必须通过宿主内部的有界序列化。

## 10. 主窗口数据流

### 10.1 路由和请求所有权

1. 主窗口解析系统保留指令、有效插件启动名称和正文。
2. 宿主确认插件已启用、健康、平台/API 兼容且权限满足。
3. 宿主捕获提交所有者并为当前 plugin ID + generation 分配 `requestId`。
4. 新调用立即在逻辑上淘汰该插件的旧调用和旧主结果映射。
5. Runtime 异步处理；宿主不在等待期间持有插件管理、结果注册或窗口控制锁。
6. 响应只有在 request、generation、提交所有者和清单模式仍匹配时才能提交。

用户在等待期间编辑输入、再次提交、改名/禁用插件、reload 或升级，都会使旧完成失效。旧成功不能清空
新输入或打开窗口，旧失败不能把错误显示到新输入。

### 10.2 `mainResult`

- `live` 输入使用约 150 ms 防抖；每次新输入淘汰旧请求。
- `submit` 只有第一次 `Enter` 创建调用。
- 有效响应经结果注册表转换后发布到主窗口；前端不接收权限或动作真实负载。
- 用户执行默认动作时，宿主重新解析当前注册项并执行。
- `copyText` 成功沿用现有清空结果和隐藏主窗口流程；失败保留结果并显示固定错误。

### 10.3 `window`

1. 当前提交所有者收到有效 `WindowResponse` 后，请求该插件的窗口协调器。
2. 协调器捕获主窗口可见、焦点和 topmost 状态，进入交接事务；从取消 main topmost 到成功提交或失败
   回滚期间，主窗口失焦处理器只消费该事务对应的预期失焦，不能执行普通 clear-and-hide。
3. 已有窗口时发送最新 `PluginWindowUpdate`；没有时创建宿主外壳并等待内容页 ready。
4. bootstrap 在调用插件 handler 前应用本次主题 token；handler ack 后，外壳恢复并校正位置，然后显示
   并聚焦窗口。
5. 原生前景/焦点快照确认插件窗口已激活后，提交主输入清空与主窗口隐藏，结束失焦抑制。主窗口下次
   由热键或托盘显示时，按既有 lifecycle 恢复其 topmost 策略。
6. 任一步失败都终止交接：尚未 ready/ack 的新窗口保持隐藏；主窗口恢复捕获的可见、焦点和 topmost
   状态并保留原输入。

窗口更新 ack 成功但聚焦失败不算提交成功，不能清空主输入。此时已确认的新数据可以留在插件窗口作为
该 request 的最新内容，但不能改变主窗口生命周期；用户再次提交会产生新 request 并幂等覆盖它。控制器
锁、插件管理锁和结果注册锁均不得跨越 emit、ack 等待或原生窗口调用。

## 11. 插件窗口合同

### 11.1 宿主外壳

UiPilot 拥有外壳、标题区域、拖拽区域、图钉按钮、关闭按钮、窗口尺寸策略和位置持久化。插件只渲染内容
区域，不能覆盖或替换宿主控件。外壳使用与主窗口相同的主题 token，至少提供背景、表面、正文、次要
正文、边框、强调色、危险色和字体 CSS 变量。

公开 token 名固定为 `--uipilot-color-background`、`--uipilot-color-surface`、
`--uipilot-color-text`、`--uipilot-color-text-muted`、`--uipilot-color-border`、
`--uipilot-color-accent`、`--uipilot-color-danger` 和 `--uipilot-font-family`。bootstrap 在每次窗口更新
handler 运行前把本次主题对应值设置到插件内容页 `documentElement`，因此隔离内容无需访问外壳 DOM。

插件内容运行在独立来源的隔离 frame/WebView 中：

- 不能访问外壳 DOM；
- 不能获得 Tauri 全量注入；
- 不能导航顶层窗口、提交表单、下载、打开新窗口或加载远程资源；
- 只能加载包快照中经验证的本地 HTML、JavaScript、CSS 和静态资源；
- 通过只读 `onUpdate` 桥接取得数据。

窗口具体像素尺寸不是公开 API；插件内容必须响应式适应宿主内容区。第一版清单不提供窗口尺寸参数。

### 11.2 唯一性与生命周期

- 每个已安装插件最多一个可见子窗口；窗口身份由 plugin ID 派生，插件不能选择原生 label。
- 不同插件可以各自拥有一个窗口。
- 重复调用复用窗口并更新最新数据，`instanceNumber` 始终为 `1`。
- 普通原生关闭事件被宿主转换为隐藏，不销毁窗口；宿主退出、插件卸载、升级或 generation 失效才销毁。
- 窗口内容未 ready 时不显示交互外壳；ready 超时按打开失败处理。
- 窗口隐藏后旧业务数据不可获得新的宿主权限；再次打开必须使用新 invocation 更新。

### 11.3 图钉与关闭

- 默认未固定；未固定窗口真实失去焦点时自动隐藏。
- 固定后失去焦点仍保持可见。
- 固定不等于 `always-on-top`，不能让插件窗口遮挡其他应用。
- 图钉状态由宿主维护，插件不能修改。
- 关闭按钮始终隐藏窗口并取消固定状态。
- 进程重启后默认未固定；图钉状态不持久化。
- 由宿主执行的预期隐藏只消费一次对应失焦事件，不能递归隐藏或清除新状态。

### 11.4 拖拽、位置与主题

- 外壳提供可拖拽区域，不模拟鼠标或键盘输入。
- UiPilot 按 plugin ID 保存最近一次有效窗口位置。
- 再次打开时恢复该位置；显示器变化或位置越界时把窗口校正到当前可用工作区。
- 位置属于插件私有宿主状态。禁用保留；卸载时遵循“删除/保留插件数据”的用户选择。
- 每次打开或重新激活窗口时，外壳和内容更新使用主窗口当时实际主题。
- 窗口保持显示期间，主窗口主题变化不实时推送；下次打开/激活才更新。

## 12. 权限与安全边界

### 12.1 第一阶段可用能力

| 能力 | 清单权限 | 规则 |
| --- | --- | --- |
| 平台、主题、调用时间 | 无 | 只读，由宿主写入 invocation |
| 私有 JSON 存储 | 无敏感权限 | 每插件隔离，总量 5 MiB |
| 唯一插件窗口 | `ui.window` | 仅 `outputMode: window`，宿主管理 |
| 写入剪贴板 | `clipboard.write` | 仅显式 `Enter`/点击动作后由宿主执行 |

权限在安装时显示。清单声明权限不等于获得通用 command；每个 Rust command 和桥接仍先验证调用窗口、
plugin ID、generation、当前 request 和具体权限。

### 12.2 已保留但未开放的权限

以下名称在公开枚举中保留，但第一阶段宿主把声明它们的插件标记为“不支持此权限”，不能启用：

- `clipboard.read`：未来只允许用户主动调用时读取，禁止后台监听；
- `network.https`：未来必须同时声明 HTTPS 域名并使用 UiPilot 网络代理；
- `files.userSelected`：未来只访问用户主动选择的文件或目录；
- `files.index.readAll`：未来用于 `/find` 类插件，安装时作为高风险权限突出展示；
- `notifications.publish`：未来只能向宿主消息中心发布结构化消息；
- `background.schedule`：未来只能使用宿主持久化调度器，不允许常驻插件进程。

未来网络代理只允许清单域名，默认拒绝未声明域名、HTTP、本机地址和局域网地址，并由宿主限制超时、响应
大小和重定向。开发模式的本机网络放宽必须另行显式设计，不能自动进入生产包权限。

### 12.3 禁止能力

- 插件不能直接读取任意文件、注册表、进程、窗口、剪贴板或系统凭据。
- 插件不能启动 `.exe`、加载 `.dll`、运行 Shell 或合成鼠标/键盘输入。
- 插件不能直接操纵托盘、抢焦点、自动弹出主窗口或操作其他插件。
- 插件之间不能直接通信，不能读取彼此数据、设置或健康状态。
- 插件不能在查询处理器中产生系统副作用；副作用只能作为结构化动作并在用户确认后由宿主执行。

### 12.4 来源与签名

第一版不强制开发者签名。未签名包必须标记“来源未验证”，安装确认展示全部权限，并禁止自动更新。
未来官方市场建立签名和审核合同；不能把第一版的本地安装等同于可信来源。

Runtime 和内容页继续使用独立来源、严格 CSP、资源快照、路径 no-follow 和最小 Tauri capability。隔离目标
是阻止插件通过正常 API 越权并限制单插件故障；不声称能抵抗浏览器引擎自身的未知安全漏洞。

## 13. 插件管理、名称与数据

### 13.1 设置页

主程序设置页的插件区域显示：

- 插件名称、plugin ID、版本和来源；
- 已安装、禁用、不兼容、故障停用等状态和原因；
- 当前有效启动名称；
- 清单权限及当前宿主支持状态；
- 安装/更新、reload、启用、禁用、改名、恢复默认、卸载操作。

只有插件清单声明的普通设置字段由 UiPilot 统一生成控件。`outputMode` 不显示为设置项。

### 13.2 启动名称

- 每个插件只有一个有效启动名称。
- 用户设置值不含 `/`，并使用与 `defaultName` 相同的正则。
- 名称按 ASCII 小写形式全局比较；即使未来输入校验放宽大小写，`math` 与 `Math` 仍冲突。
- 系统保留名称至少包含现有 `/find`、`/math` 及宿主声明的其他内置指令。
- 所有已安装且身份有效的插件都保留其有效名称；禁用插件的名称不能被另一个插件占用。
- 冲突时保存失败并明确显示冲突插件或系统指令。
- 改名采用原子校验和提交；成功后新名称立即生效，旧名称立即失效。
- 升级不覆盖用户改名；“恢复默认”在无冲突时删除覆盖值，否则拒绝。

### 13.3 安装、升级、禁用和卸载

- 本地 `.uipilot-plugin` 包从设置页由用户主动选择；开发者模式可以加载约定开发目录。
- 开发目录和安装包经过同一清单、权限、路径和 Runtime ready 校验。
- 同一插件由固定 `pluginId` 识别。升级保留用户改名、普通设置、秘密和私有数据。
- 新版本增加权限时必须重新确认；拒绝确认则旧版本继续活动。
- 安装、升级或 reload 只有在候选完整验证和 ready 后才原子提交；失败不留下半升级状态。
- 修改开发插件 JSON 后必须 reload；权限增加同样需要确认。
- 禁用插件保留全部数据，隐藏并销毁其窗口/Runtime，注销路由并使当前请求和结果失效。
- 卸载默认删除包和插件数据；确认框提供“保留数据”选项，以便相同 plugin ID 重装后恢复。
- 未签名插件只支持用户手动安装新版，不支持自动更新。

## 14. 并发、故障与资源

### 14.1 请求时效

- `live` 调用从派发起最多 5 秒；`submit` 调用最多 30 秒。
- 每次调用只接受一个最终响应；第一版不支持流式或增量结果。
- 新调用、禁用、升级、reload、卸载和主输入所有权变化都会逻辑取消旧调用。
- 宿主可以销毁卡死 Runtime，但不得在等待期间阻塞主 UI 线程。
- 一次有效成功调用重置该插件的连续故障计数。

### 14.2 故障隔离

以下情况计为一次插件异常：

- Runtime 导入、崩溃或意外退出；
- 调用超时或处理器抛出异常；
- request ID、输出模式或响应结构不匹配；
- 响应超过 64 KiB；
- 返回未授权动作或违反权限合同。

异常只终止当前插件调用，不能导致 UiPilot、其他插件、应用搜索、`/math` 或 `/find` 失败。当前请求显示
宿主固定错误，不把脚本堆栈、插件路径、输入正文或秘密暴露给普通 UI。

同一插件在 5 分钟窗口内连续异常 3 次时，宿主持久化自动停用状态，隐藏窗口、销毁 Runtime、注销路由
并使结果失效。成功调用打断“连续”计数。用户在设置页手动重新启用时重置计数并创建新 generation。

日志按字段记录插件 ID、版本、固定错误类别和时间，并在写入前移除秘密、授权头、剪贴板正文和设置明文。

### 14.3 MVP 资源边界

- 保留 5/30 秒调用超时、5 MiB 私有存储和 64 KiB 单响应上限。
- 第一版不承诺精确的每插件内存硬配额。
- 第一版不单独计量插件可见窗口资源。
- 卡死、崩溃或渲染进程退出必须能够只回收对应插件 Runtime/窗口；若当前 WebView2 拓扑无法证明这一点，
  公开插件发布为 No-Go，必须改用更强的独立进程 Runtime，而不能通过“来源未验证”警告绕过。

## 15. `/demo` 参考插件

`/demo` 必须作为仓库中的独立示例包交付，完全使用公开清单、Runtime、窗口桥接和权限；主程序不得包含
`/demo` 字面命令、日期拼接或显示字段的兜底逻辑。删除/禁用该插件后，`/demo` 功能必须完整消失。

窗口模式清单声明 `ui.window`。Runtime 的 `onWindow` 返回：

```json
{
  "requestId": "<echoed requestId>",
  "data": {
    "returnText": "str 2026-08-13"
  }
}
```

窗口内容页从 `PluginWindowUpdate` 读取并显示五个验收字段。图钉、关闭、拖拽和位置恢复不由示例页面
自行实现。

切换到主结果模式时，开发者把 `outputMode` 改为 `mainResult`，移除 `window` 声明，声明
`clipboard.write` 并 reload。`onMainResult` 返回一项纯文本结果及 `copyText` 默认动作。权限变化必须按正常
开发插件流程重新确认，不能给示例插件后门。

## 16. 测试

### 16.1 清单、包和兼容

- 公开/旧内部清单鉴别准确，不修改 `/math` 包。
- 未知字段、非法路径、非法版本、重复字段、非法模式组合和缺少处理器失败关闭。
- 平台/API 不兼容和已知未实现权限保留安装但禁止启用。
- plugin ID、有效名称、保留名称和大小写冲突稳定拒绝。
- 安装、升级、reload、权限增加确认和失败回滚保持既有事务不变量。

### 16.2 路由和主结果

- `/`、前缀发现、完整命令提示和正文状态符合用户合同。
- 必填空输入不调用 Runtime；可选空输入传空字符串。
- 正文删除边界空白并保留内部空格。
- `live` 防抖和 5 秒超时；`submit` 只在 `Enter` 后调用并使用 30 秒超时。
- `mainResult` 的项目、文本、ID、动作、20 项和 64 KiB 边界全部校验。
- `submit + mainResult` 第一次 `Enter` 显示、第二次执行；剪贴板失败保留结果。
- 提交 A 后编辑/提交 B 时，A 的迟到成功和失败都不能改变 B。

### 16.3 插件窗口

- 每 plugin ID 只创建一个窗口，不同插件窗口互不混用。
- 重复 `/demo` 调用只更新编号 `1`，旧响应不能覆盖新输入。
- 内容 ready、更新 emit、主题应用、位置恢复和聚焦全部成功后才清空/隐藏主窗口。
- 每个失败点都恢复主窗口和原输入；ack 后聚焦失败可以保留已确认数据，但不得提交主窗口成功收尾。
- 未固定真实失焦隐藏；固定失焦保持；固定不改变 always-on-top。
- 关闭隐藏并取消固定；预期程序化失焦只消费一次。
- 拖拽位置按 plugin ID 持久化并在显示器变化后校正。
- 主题只在打开/重新激活时更新，窗口显示期间不实时变化。
- 升级、禁用、卸载和自动停用销毁旧 generation 窗口并拒绝旧桥接调用。

### 16.4 权限、隔离和故障

- Runtime 和内容页不能调用主窗口命令、Shell、网络、文件、其他插件或原生输入 API。
- `copyText` 在发布和执行时均校验权限与当前 generation。
- 私有存储跨插件隔离，5 MiB 超限原子失败；secret 明文不可读。
- 超时、崩溃、异常响应、越权和 64 KiB 超限只影响当前插件。
- 成功重置连续故障；5 分钟内连续 3 次异常持久化停用，手动启用重置。
- 日志脱敏测试覆盖秘密、授权头和插件返回正文。
- 卡死/进程失败探针证明主窗口、其他插件、`/math` 和 `/find` 仍可响应。

### 16.5 回归与人工验收

自动回归必须包含现有 `/math` 和 `/find` 全部相关测试。真实窗口 harness 不合成鼠标或键盘输入；任何
会短暂改变前台焦点的测试在运行前必须通知用户并获得明确允许。

最终人工验收只由用户操作：

1. 以普通权限启动 UiPilot，通过开发目录加载 `/demo`；
2. 输入 `/demo str` 并按 `Enter`，确认唯一窗口显示输入、系统、主题、编号 `1` 和本地日期返回值；
3. 验证未固定失焦隐藏、固定后保持可见、关闭取消固定、拖拽和位置恢复；
4. 输入 `/demo newstr`，确认复用编号 `1` 并显示最新内容；
5. 修改开发包为 `mainResult` 和对应权限，reload 并确认权限；
6. 第一次 `Enter` 显示结果，第二次 `Enter` 复制结果；
7. 确认 `/math` 和 `/find` 行为没有变化。

## 17. 非目标

- 修改或迁移 `/math`。
- 把 `/find` 改造成插件。
- 运行时切换 `outputMode`，或为该字段提供设置 UI。
- 一个插件多个命令、多个窗口或多个窗口实例。
- 插件窗口向主结果区回传数据。
- 插件自定义主窗口 HTML、CSS、Markdown、脚本或远程图片。
- 流式结果、分页、大文件响应和精确内存配额。
- 插件间通信、依赖解析或跨插件动作。
- 常驻后台插件、调度、消息中心、托盘闪烁、市场、签名和自动更新。
- 直接网络、任意文件、剪贴板读取、原生二进制、Shell 或输入合成。
- 在自动化验收中控制用户鼠标或键盘。

## 18. 完成标准

第一阶段 MVP 只有在以下条件全部成立时完成：

1. 公开清单、TypeScript 接口和权限合同具有可验证实现，非法组合失败关闭。
2. `/math` 包和行为未修改，`/find` 保持系统保留且相关回归全部通过。
3. 命令改名、发现、正文传递、`live/submit` 和请求所有权符合本设计。
4. `mainResult` 只发布当前请求的有效纯文本结果，动作只由宿主在用户确认后执行。
5. `window` 只在 `submit` 后创建/复用每插件唯一窗口，成功提交前不清空或隐藏主窗口。
6. 图钉、关闭、拖拽、位置恢复和打开时主题同步完全由宿主外壳控制，固定不等于置顶。
7. 64 KiB 响应、5 MiB 存储、超时、连续故障和权限边界均有自动化覆盖。
8. 安装、升级、reload、禁用和卸载保留既有事务、generation 和失败回滚安全合同。
9. `/demo` 不依赖任何主程序硬编码，在两种静态输出模式下通过自动化与用户人工验收。
10. 自动化测试不合成用户输入；涉及真实焦点变化的 harness 只在用户明确允许后运行。
