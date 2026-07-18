# 安全探针结果通道可靠性设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 基线：`6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`
- 分支：`codex/security-probe-reliability`
- 影响范围：Foundation Task 1A 的 feature-only 安全探针与门禁

## 背景

现有安全探针已保证只有前端收到精确错误
`Command load_settings not allowed by ACL` 时才把
`window.location.hash` 设置为 `acl-denied`。Rust 侧随后通过
`WebviewWindow::url()` 轮询 WebView2 native `Source`，仅把 fragment
`acl-denied` 映射为进程退出码 `73`。

该结果通道不能稳定工作：

- 同一 release 产物曾连续六次通过 smoke，但新的 feature-only 构建在首次
  smoke 中再次退出 `3`。
- 2026-07-18 12:57:48 UTC 开始的新构建成功，产物时间为
  12:58:08 UTC，SHA-256 为
  `80FEEBE6D0E476441F95488A802B26B656C3B70248CAFBABD17B3F33412501CE`；
  首次 smoke 仍返回 `3`，且正式 app-data 文件族没有变化。
- 当前 `probe.url().ok()` 丢弃 URL 查询错误，并把页面未加载、探针脚本未执行、
  invoke 结果异常和 native `Source` 未反映 same-document fragment 全部压成同一个
  超时结果。继续预热或重跑不能提供可审计证据。

因此 Task 4C completion gate 保持 No-Go。该问题先作为独立安全探针可靠性修订，
不得通过放宽退出码或跳过首次运行来绕过。

## 目标

1. 证明精确 ACL 拒绝可以通过可观察、可分类的 Rust 结果通道稳定传递。
2. 保持 `73` 的单向安全语义：只有精确 ACL 拒绝可以退出 `73`。
3. 让页面加载、前端执行、invoke 结果和 Rust 查询失败分别可诊断。
4. 用全新 feature-only 产物的首次运行验证结果，禁止以预热运行掩盖冷启动问题。

## 不变安全边界

- `security-probe.ts` 只有在错误字符串精确等于
  `Command load_settings not allowed by ACL` 时，才能执行
  `window.location.hash = 'acl-denied'`。
- 命令成功、其他错误、空值、无效结构、查询错误和超时都不得映射为 `73`。
- 不新增 Tauri command、capability、配置 override、网络访问、本地结果文件、
  dependency 或生产代码路径。
- 不设置新的 WebView `data_directory`，不创建需要额外所有权或清理协议的临时
  profile。
- 不修改正式 `main` capability，不扩大探针窗口权限。
- `std::process::exit(73)` 仍只存在于 `test-instrumentation` feature。
- `test-instrumentation` 构建不得查询或修改正式 app-data，也不得加载或托管
  `SettingsStore`、`ValidationStore` 或创建 session marker。
- 生产前端不包含 `security-probe.html` 或探针 JavaScript；生产 Rust 构建不包含
  探针模块。

## 方案比较

### 方案 A：继续轮询 `WebviewWindow::url()`

拒绝。WebView2 native `Source` 对 same-document fragment 的可见性已经出现不稳定
实证，且 `.ok()` 会吞掉查询错误。增加重试次数只能延长不确定性，不能证明 ACL
边界。

### 方案 B：页面加载完成后用 `eval_with_callback` 读取 DOM 状态

推荐，但必须先经独立诊断构建证明。Tauri 2.11.5 的
`eval_with_callback` 会把 JavaScript 返回值序列化为 JSON 字符串交给 Rust；Windows
上的 JavaScript exception 不会由 API 可靠上报，因此被执行的表达式必须自行捕获
异常并返回结构化结果。

该方案直接读取页面真实的 `window.location.hash`，不再假定 WebView2 native
`Source` 会同步 same-document fragment。

### 方案 C：新增 IPC、capability、文件或网络结果通道

拒绝。新增 command 或事件权限会改变被测 ACL，结果文件需要新的真实性和清理
协议，网络通道扩大攻击面。这些方案会让探针改变它正在验证的权限边界。

## 两阶段决策

本修订只能依次执行两个阶段，后续实施计划必须为两者设置独立审核检查点。

### 阶段一：诊断构建

诊断构建仅在 `test-instrumentation` 中增加可分类状态，不改变 capability 或
production build：

1. WebView builder 注册 page-load handler。只有目标窗口收到
   `PageLoadEvent::Finished` 后，Rust 才允许发起 DOM 查询。
2. 前端模块同步写入 feature-only 诊断状态
   `window.__UIPILOT_SECURITY_PROBE_STATE__ = 'invoke-pending'`，然后调用
   `load_settings`。同步 throw 与 Promise rejection 都必须进入固定分类。
3. invoke 成功时状态变为 `invoke-succeeded`；精确 ACL 拒绝时状态变为
   `acl-denied` 并设置 hash；其他错误变为 `invoke-other-error`。不得把原始错误文本
   写入状态或日志。
4. Rust 通过 `eval_with_callback` 读取一个自行捕获异常的结构化对象：
   `documentReadyState`、`probeState`、`hash` 和 `evalError`。
5. 只有 eval 已观察到精确 ACL 状态后，诊断阶段才执行一次显式
   `WebviewWindow::url()` 查询作对照。不得使用 `.ok()`，必须把 `Err` 分类为查询
   失败。该查询只用于证明旧通道是否漏报，不进入阶段二的最终判定。
6. 若 DOM 观察为精确 ACL 状态和 `#acl-denied`，而 native URL 没有该 fragment，
   分类为 `LegacyUrlChannelMiss`。这证明前端已设置状态但现有 Rust 结果通道没有
   观察到。

诊断构建使用固定退出码暴露结果，不写结果文件：

| 分类 | 退出码 |
|---|---:|
| `LegacyUrlChannelMatched` | `73` |
| `PageLoadTimeout` | `74` |
| `ProbeScriptNotStarted` | `75` |
| `InvokeTimeout` | `76` |
| `InvokeUnexpectedlyAllowed` | `77` |
| `InvokeOtherError` | `78` |
| `LegacyUrlChannelMiss` | `79` |
| `LegacyUrlQueryFailed` | `80` |
| `EvalDispatchFailed` | `81` |
| `EvalCallbackTimeout` | `82` |
| `InvalidEvalObservation` | `83` |
| `EvalScriptFailed` | `84` |

阶段一中的 `73` 只代表所有必需 ACL 条件成立，且 eval 与 native URL 两个观察通道
都精确观察到 ACL fragment；该阶段的分类名固定为 `LegacyUrlChannelMatched`，不得
复用阶段二的 `ExactAclDenied`。
`LegacyUrlChannelMiss` 或 `LegacyUrlQueryFailed` 只有在 eval 已观察到精确 ACL 状态时
才可分别退出 `79` 或 `80`，作为候选路线成立、旧通道失效的证据；它们不得被现有
smoke 当成通过。其他场景不得借用 `79` 或 `80`。

阶段一 Go 条件：构建一个 fresh 诊断产物，不预热地执行首次运行，再对同一未变化
SHA-256 累计执行至少 3 次。每次 page-load finished 都必须发生，前端状态必须为
`acl-denied`，DOM eval 必须返回精确 `#acl-denied`，且回调 JSON 必须严格解析。
native URL 观察结果逐次记录为匹配、漏报或查询失败，并分别产生 `73`、`79` 或
`80`。三次都只能产生这三个退出码；任一次产生 `74` 至 `78` 或 `81` 至 `84`，阶段一
立即 No-Go，不追加运行稀释失败。

三个允许结果都只证明 eval 候选路线在该次运行可观察精确 ACL。`79` 或 `80` 证明
该次旧通道漏报或查询失败；`73` 只表示该次未复现旧通道故障，不能据此声明已经
定位旧根因。三次中 `73`、`79`、`80` 发生变化本身是旧通道不稳定的直接证据。
若 DOM eval 不能连续满足上述条件，立即回到书面设计复审，不叠加第二种修复。

现有 `test-security-probe.ps1` 的安全 oracle 不变，始终只接受 `73`。阶段一诊断门禁
直接记录每次原始退出码；它可以把 `79` 或 `80` 计为候选路线证据，但不得输出
`security probe ok` 或把这两个退出码包装成安全 smoke 成功。

### 阶段二：最终结果通道

仅在阶段一 Go 后实施：

1. 删除 `WebviewWindow::url()` 轮询和对 native `Source` 的判定依赖。
2. page-load finished 只负责打开查询门，不在 UI 线程等待。
3. Rust 串行发起一次 eval 并等待该次 callback。只有 callback 已返回，且严格分类
   为可重试中间态，才允许下一次查询，禁止堆积并发 eval callback。
4. 每次查询都受单次 callback 超时和总时限约束。eval 立即返回 `Err` 是终态
   `EvalDispatchFailed`；callback 在 250ms 内未到达是本进程终态
   `EvalCallbackTimeout`，立即退出 `82`，不得再派发 eval。JSON 无效、字段无效或
   表达式错误也都是立即终止的非 `73` 结果。
5. 只有 page-load finished 已发生，结构化结果严格解析成功，
   `documentReadyState` 为 `interactive` 或 `complete`，`probeState` 精确为
   `acl-denied`，且 `hash` 精确为 `#acl-denied` 时，Rust 才执行
   `std::process::exit(73)`；该阶段的分类名固定为 `ExactAclDenied`。
6. `invoke-succeeded` 和 `invoke-other-error` 是立即终止的非 `73` 结果；
   空状态和 `invoke-pending` 是可重试的中间观察，只能在总时限耗尽后分别分类为
   `ProbeScriptNotStarted` 和 `InvokeTimeout`，不能视为成功。

## 诊断矩阵

| 阶段 | 场景 | 可观察证据 | 分类 | 退出码 |
|---|---|---|---|---:|
| 两阶段 | WebView 页面未完成加载 | 总时限内未收到目标窗口的 `PageLoadEvent::Finished` | `PageLoadTimeout` | `74` |
| 两阶段 | 页面已加载但 probe JS 未执行 | finished 已发生；eval 结构有效但 `probeState` 为空直到总时限 | `ProbeScriptNotStarted` | `75` |
| 两阶段 | invoke 尚未结束 | `probeState == invoke-pending` 直到总时限 | `InvokeTimeout` | `76` |
| 两阶段 | invoke 成功 | `probeState == invoke-succeeded` | `InvokeUnexpectedlyAllowed` | `77` |
| 两阶段 | 其他 invoke 错误 | `probeState == invoke-other-error` | `InvokeOtherError` | `78` |
| 阶段一 | 两通道匹配 | eval 为精确 ACL；native URL fragment 也精确为 `acl-denied` | `LegacyUrlChannelMatched` | `73` |
| 阶段一 | JS 已设置状态但旧 Rust 通道未观察 | eval 为精确 ACL；native URL 无 fragment 或 fragment 不同 | `LegacyUrlChannelMiss` | `79` |
| 阶段一 | Rust URL 查询自身失败 | eval 已观察精确 ACL；随后 `WebviewWindow::url()` 返回 `Err` | `LegacyUrlQueryFailed` | `80` |
| 阶段二 | 精确 ACL 拒绝 | page-load finished；严格 eval 为精确 ACL；不查询 native URL | `ExactAclDenied` | `73` |
| 两阶段 | Rust eval 派发失败 | `eval_with_callback` 立即返回 `Err` | `EvalDispatchFailed` | `81` |
| 两阶段 | eval callback 未返回 | 单次 callback 超时；不再派发 eval | `EvalCallbackTimeout` | `82` |
| 两阶段 | eval 返回空值、无效 JSON 或未知字段 | 严格反序列化失败 | `InvalidEvalObservation` | `83` |
| 两阶段 | eval 表达式内部异常 | 表达式自行捕获并返回固定错误结构 | `EvalScriptFailed` | `84` |

所有诊断输出只包含上述固定分类、计数和退出码，不包含 URL、路径、用户数据、原始
JavaScript 错误或 ACL 错误文本。

## Rust 分类边界

DOM callback 的原始字符串先由一个共享、无副作用的 Rust parser 处理。结构使用
`deny_unknown_fields`，字段只允许：

- `documentReadyState`: `loading`、`interactive` 或 `complete`。
- `probeState`: `null`、`invoke-pending`、`invoke-succeeded`、`acl-denied` 或
  `invoke-other-error`。
- `hash`: 字符串；只有精确 `#acl-denied` 有成功含义。
- `evalError`: `null` 或固定值 `script-failed`，用于表达式内部异常；出现该值时其他
  字段不得触发成功。

parser 之后必须使用两个互不混用隐式条件的 phase-specific classifier：

- `classify_diagnostic(page_load_finished, observation, legacy_url_result)` 只属于阶段一。
  只有严格 eval 为精确 ACL 且 legacy URL fragment 精确为 `acl-denied` 时返回
  `LegacyUrlChannelMatched/73`；legacy miss 和 `Err` 分别返回 `79` 与 `80`。
- `classify_final(page_load_finished, observation)` 只属于阶段二，不接收 legacy URL
  输入。只有严格 DOM eval 条件成立时返回 `ExactAclDenied/73`。

parser 和两个 classifier 都不访问 WebView、进程、文件系统或时钟，允许纯 Rust
单元测试穷举。以下输入在两个 classifier 中都必须为非 `73`：

- JSON `null`、空字符串、其他字符串和数组。
- 缺字段、额外字段、未知枚举值和无效 JSON。
- hash 大小写、前后空白、URL 编码或额外 fragment 不同。
- 只有 hash 正确但 `probeState` 不为 `acl-denied`。
- 只有状态正确但 hash 不为 `#acl-denied`。
- `documentReadyState == loading`，即使状态和 hash 正确。
- `evalError` 非空。
- eval API 返回错误或 callback 超时。

此外，阶段一必须测试 eval 精确 ACL 加 legacy URL 匹配、漏报和查询失败分别只产生
`73`、`79` 和 `80`；阶段二必须证明不存在 legacy URL 输入或分类分支。

## 时序与所有权

- 只处理 label 为 `security-probe` 的 page-load finished 事件。
- page-load handler 只发送内部信号，不执行 sleep、eval 等待或进程退出。
- 一个 worker 独占诊断状态机、总时限和进程退出决策。
- 每次 eval callback 绑定本次请求序号。callback 返回后，该请求先标记为已完成再
  分类；只有已完成请求返回空状态或 `invoke-pending` 时，才可等待 50ms 后派发下一个
  请求。晚到或重复 callback 丢弃，不能覆盖更新的观察。
- 从窗口 build 成功开始使用一个 10 秒 monotonic 总 deadline；page-load 等待和
  所有 eval 尝试共同消耗该 deadline。每次 callback 最多等待 250ms；超时立即以
  `82` 终止进程，不再派发请求，因此同时最多一个 eval 在途。只有 callback 已返回
  可重试中间态时才使用 50ms 间隔。外层 PowerShell 保持 15 秒硬超时。
- 所有失败路径最终退出非 `73`；不得因 worker panic 或 channel disconnect 误出
  `73`。

## 验证门禁

### 纯逻辑门禁

- Rust parser、`classify_diagnostic` 和 `classify_final` 分别覆盖自己的矩阵，禁止用
  一个含隐式 phase 条件的 classifier 同时承担两个阶段。
- 阶段一只有 page-load finished、严格 eval 精确 ACL 且 native URL fragment 精确
  匹配时返回 `LegacyUrlChannelMatched/73`。eval 相同但 URL 漏报或查询失败必须返回
  `79` 或 `80`。
- 阶段二只有 page-load finished 已发生，严格结构中的 `documentReadyState` 为
  `interactive` 或 `complete`、`probeState == acl-denied`、
  `hash == #acl-denied` 且 `evalError == null` 时返回 `ExactAclDenied/73`，并且不接收
  legacy URL 输入。
- 相同结构在 page-load finished 未发生时，两个 classifier 都不得映射 `73`。
- `null`、其他字符串、无效 JSON、未知字段、eval error 和 callback timeout 全部
  不映射 `73`。
- callback timeout 测试必须断言结果为终态 `82`，且 dispatch 计数保持 `1`；不得
  因请求序号过滤而派发第二个 eval。

### 安全回归

- `hostname.exe` 负例仍必须失败。
- 配置白名单与全部恶意 fixture 继续通过现有门禁。
- 不新增 command、capability、override、依赖或结果文件。
- production build 不包含探针页面、前端模块或 Rust 探针代码。

### 阶段一诊断证据

- 构建 fresh 诊断产物，记录构建开始 UTC、产物 UTC 和 SHA-256。
- 不进行预热；直接记录新产物首次运行的原始退出码和固定分类。
- 对同一未变化 SHA-256 的产物继续运行至少两次，累计至少 3 次。
- 三次中的每次都必须发生 page-load finished，且 eval 严格观察到
  `documentReadyState` 为 `interactive` 或 `complete`、`probeState == acl-denied`、
  `hash == #acl-denied` 与 `evalError == null`；阶段一 classifier 再根据 legacy URL
  对照分别返回 `73`、`79` 或 `80`。
- 每次结果都只能为 `73`、`79` 或 `80`。任一次为 `74` 至 `78`、`81` 至 `84`、
  超时或哈希变化，阶段一立即 No-Go，不追加运行稀释失败。
- 逐次报告退出码与分类；若 `73`、`79`、`80` 在三次间变化，明确记录为旧通道
  不稳定证据。三次均为 `73` 只证明 eval 候选可行，不能声称定位旧通道根因。
- 该诊断门禁不调用或放宽安全 smoke oracle；`test-security-probe.ps1` 始终只接受
  `73`。

### 阶段二冷启动与重复性

- 构建 fresh feature-only 产物，记录构建开始 UTC、产物 UTC 和 SHA-256。
- 不进行预热；该新产物的第一次 smoke 必须返回 `security probe ok`，内部退出码必须
  精确为 `73`。
- 对同一未变化 SHA-256 的产物继续运行至少两次，累计至少 3/3 通过。
- 任一次非 `73`、超时或哈希变化都失败，不以追加重跑稀释结果。
- 每次运行前后，正式 app-data 下 `settings.json*`、`validation-data.json*` 和
  `open-session.json*` 的根存在性、文件集合、长度、SHA-256 与最后写入 ticks 均不变。
- 每次结束后不存在 `uipilot` 探针进程，也不存在本轮创建后遗留的临时 WebView
  profile。本设计不新增 `data_directory` 或临时 profile；门禁禁止出现以
  `uipilot-security-probe-` 命名的新目录，且禁止扫描或删除任意用户 profile。

## 文件与交付顺序

本次提交只新增本文档，不修改代码、脚本或既有计划。

书面设计获批后：

1. 更新 Foundation Task 1A 实施计划，引用本文固定的退出码与状态机时限，并写明
   准确文件范围和 RED/GREEN 命令。
2. 计划复审 Go 后才允许按 TDD 实施阶段一诊断。
3. 阶段一证据 Go 后才允许实施阶段二最终结果通道。
4. 探针修复代码级 Go 后，才非破坏性集成批准提交回
   `codex/foundation-task-4`。
5. 从头运行 Task 4C completion gate。通过前不得提交当前 Task 4C Task 2，也不得
   进入 Task 5。

## 非目标

- 不修改 Task 4C 导出服务实现或合同。
- 不接线 Task 5 command、UI、协议或生命周期。
- 不实现 `/find`。
- 不合并 main、不推送、不签名、不发布。
