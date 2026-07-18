# 安全探针结果通道可靠性设计

## 状态

- 日期：2026-07-18
- 状态：direct-eval readiness 修订，待书面复审
- 基线：`157ad898c64abccfe26d1c76692011ea559b819e`
- 分支：`codex/security-probe-eval-readiness-design`
- 影响范围：Foundation Task 1A 的 feature-only 安全探针与门禁

## 背景

现有安全探针已保证只有前端收到精确错误
`Command load_settings not allowed by ACL` 时才把
`window.location.hash` 设置为 `acl-denied`。原 Rust 结果通道通过
`WebviewWindow::url()` 读取 WebView2 native `Source`，但新的 feature-only
产物在首次运行中反复不能稳定观察该 fragment。

第一次可靠性设计试图用 Tauri `on_page_load` 打开
`eval_with_callback` 查询门。该候选通过了纯逻辑测试，却在真实 fresh Windows
构建中失败：

1. 第一产物 SHA-256 为
   `2F1061EF23A8F2E30B5A66E9B3AC211079E4C567283F790856112E5AEB89457B`。
   未预热首次运行约 219ms 后退出 `74`；page-load channel 在事件到达前
   disconnected。
2. 在 worker 中保留一个不发送的 sender guard 后重新构建。新产物时间为
   `2026-07-18T20:40:59.0763971Z`，SHA-256 为
   `DC88EB7A7E899E11C9BF3D66382119E4B72E3303589C751BF90CEE097A39A8A3`。
   未预热首次运行在约 10.236 秒后仍退出 `74`。正式 app-data 文件族、产物哈希、
   临时 profile 与进程残留检查均通过，因此不能把结果归因于环境污染。

两次失败都发生在 parser、eval callback 和 ACL classifier 之前。单元测试全绿不构成
对真实 WebView navigation 通道的证明，失败实现不得提交或进入最终阶段。

## 根因边界

项目当前锁定 `wry 0.55.1`。本地依赖源码
`wry-0.55.1/src/webview2/mod.rs:647-670` 的
`ContentLoading` 与 `NavigationCompleted` handler 都先调用：

```rust
Self::url_from_webview(&webview)?
```

成功后才调用用户 `on_page_load_handler`。同文件
`1343-1346` 的 `url_from_webview` 直接调用 WebView2 `Source`：

```rust
unsafe { webview.Source(&mut pwstr)? };
```

因此 Tauri/Wry page-load handler 不是独立 readiness 信号。native `Source` 查询失败
时，用户 handler 不会收到 `Started` 或 `Finished`。sender guard 只能把早期
disconnect 延迟成完整 deadline timeout，不能消除该架构耦合。

## 目标

1. 不依赖 native URL、Wry page-load handler 或新增 IPC，直接证明 DOM 已可查询。
2. 通过同一结构化 observation 同时证明页面 readiness、前端状态和精确 ACL 拒绝。
3. 保持 `73` 单向语义：只有严格 DOM observation 中的精确 ACL 拒绝可以映射
   `73`。
4. 对启动期 eval 不可用、callback 丢失、脚本未执行、invoke 未完成和无效结果提供
   互斥分类。
5. 用 fresh feature-only 产物的未预热首次运行验证，不用追加重跑稀释失败。

## 不变安全边界

- `security-probe.ts` 只有在错误字符串精确等于
  `Command load_settings not allowed by ACL` 时，才能把状态和 hash 设为
  `acl-denied`。
- 命令成功、其他错误、loading、空状态、pending、无效 JSON、eval error 和超时均
  不得映射为 `73`。
- 不新增 Tauri command、event、capability、配置 override、dependency、网络访问、
  本地结果文件、环境变量协议或生产代码路径。
- 不设置新的 WebView `data_directory`，不创建临时 profile。
- 不修改正式 `main` capability，不扩大探针窗口权限。
- `std::process::exit(73)` 仍只存在于 `test-instrumentation` feature。
- feature-only 构建不得加载或托管 `SettingsStore`、`ValidationStore`，不得创建
  session marker 或修改正式 app-data。
- production 前端不包含探针页面或 JavaScript；production Rust 不包含探针模块。

## 方案比较

### 方案 A：窗口创建后直接重复结构化 DOM eval

推荐。窗口 build 成功后，单一 worker 立即进入
`eval_with_callback` loop。每次 callback 返回严格结构：
`documentReadyState`、`probeState`、`hash` 与 `evalError`。

该结构同时承担 readiness 和 ACL 结果，不注册 `on_page_load`，不维护 page-load
channel/guard，也不需要 `page_load_finished` 重复谓词。启动期
`eval_with_callback` 的同步 `Err` 在没有任何 callback 前是可重试的 readiness
中间态；若 callback 仍在途而总 deadline 到达，必须终止，不能重试。

### 方案 B：直接注册低层 WebView2 navigation event

拒绝。该路线需要取得低层 WebView2 接口、管理 COM event token 与 handler 生命周期，
引入 Windows-only native/unsafe 边界，并绕开当前 Tauri/Wry 抽象。它还需要单独验证
ABI、线程 apartment、窗口销毁、重复 navigation 和 handler 移除，复杂度与审计面
远高于 feature-only 探针价值。

### 方案 C：新增 command、event、文件或网络 backchannel

拒绝。新增 command/event 会改变被测 capability 和 ACL 面；文件需要真实性、原子性
和清理协议；网络扩大攻击面。探针不能通过改变被测权限边界来报告权限结果。

## 推荐架构

### 前端状态机

`security-probe.ts` 同步设置：

```text
invoke-pending
```

然后调用 `load_settings`：

- resolve -> `invoke-succeeded`
- 精确 ACL reject -> `acl-denied`，随后设置精确 `#acl-denied`
- 其他 reject 或同步 throw -> `invoke-other-error`

不得保存、返回或记录原始错误文本。只有精确 ACL 分支可以写 hash。

### 结构化 observation

Rust eval 表达式自行捕获 JavaScript exception，并只返回：

- `documentReadyState`: `loading | interactive | complete`
- `probeState`:
  `null | invoke-pending | invoke-succeeded | acl-denied | invoke-other-error`
- `hash`: string
- `evalError`: `null | script-failed`

Rust 使用 `deny_unknown_fields` 严格反序列化。JSON `null`、字符串、数组、缺字段、
额外字段、未知枚举和值类型错误均立即分类为
`InvalidEvalObservation/83`。

### 直接 eval readiness loop

窗口 build 成功后立即记录一个 10 秒 monotonic deadline，并启动唯一 worker：

1. deadline 前且没有 eval 在途时发起一次 `eval_with_callback`。
2. 在尚未收到任何 callback 前，eval 同步 `Err` 表示启动期 WebView 尚不可查询。
   记录 `StartupEvalUnavailable`，按受剩余 deadline 限制的 50ms 间隔重试。它从不
   映射 `73`。
3. 任一 callback 已成功返回后，后续 eval 同步 `Err` 立即终止为
   `EvalDispatchFailed/81`；不把已可工作的通道退化当作启动等待。
4. dispatch 成功后重新从同一 deadline 计算剩余时间。若已无剩余时间，立即终止为
   `EvalCallbackTimeout/82`；否则只等待这一次 post-dispatch remaining，不设置独立的
   callback timeout。
5. callback 未在总 deadline 前返回时终止为 `EvalCallbackTimeout/82`。请求可能仍在
   WebView 中执行，因此 callback 缺席期间 dispatch count 必须始终为 `1`，不得派发
   第二次 eval。只有所有启动期 dispatch 都同步返回 `Err`、从未产生在途 callback 时，
   总 deadline 才返回 `EvalDispatchFailed/81`。
6. callback 返回后先关闭本次请求，再 parse/classify。只有已完成 observation 被分类
   为可重试中间态，才允许等待受 remaining 限制的 50ms 后发起下一次。
7. 同时最多一个 eval callback 在途。晚到或重复 callback 不能覆盖已终止结果。

总 deadline 不因 dispatch、callback 或 retry 重新开始。外层 PowerShell 继续使用
15 秒硬超时。

### readiness 与结果分类

parser 后按以下互斥顺序分类：

1. `evalError == script-failed` -> `EvalScriptFailed/84`
2. `probeState == invoke-succeeded` 且 hash 为空 ->
   `InvokeUnexpectedlyAllowed/77`
3. `probeState == invoke-other-error` 且 hash 为空 -> `InvokeOtherError/78`
4. `probeState == null`、hash 为空且 `readyState == loading` ->
   retry `DocumentNotReady`
5. `probeState == null`、hash 为空且 ready -> retry `ProbeScriptNotStarted`
6. `probeState == invoke-pending` 且 hash 为空 -> retry `InvokePending`
7. `probeState == acl-denied`、hash 精确、但 readyState 仍为 loading ->
   retry `DocumentNotReady`
8. `probeState == acl-denied` 且
   `readyState == interactive | complete`、`hash == #acl-denied`、
   `evalError == null` -> 精确 ACL observation
9. 其他状态/hash/readiness 组合，包括 null、pending、invoke-succeeded 或
   invoke-other-error 携带非空 hash -> `InvalidEvalObservation/83`

`InvokeUnexpectedlyAllowed/77` 和 `InvokeOtherError/78` 不要求特定 readyState，但必须
认证 hash 为空；任何非空 hash 都不能被这两个终态接受。

总 deadline 到达时，最后一个已完成中间态固定映射：

| 中间态 | 最终分类 | 退出码 |
|---|---|---:|
| 启动期只有 eval dispatch `Err`，无 callback | `EvalDispatchFailed` | 81 |
| `DocumentNotReady` | `DocumentNotReady` | 74 |
| `ProbeScriptNotStarted` | `ProbeScriptNotStarted` | 75 |
| `InvokePending` | `InvokeTimeout` | 76 |

## 两阶段决策

### 阶段一：direct-eval 诊断

阶段一使用：

```text
classify_diagnostic(observation, legacy_url_result)
```

不得接收 `page_load_finished` 参数。只有 observation 已满足精确 ACL 条件后，才执行
一次 `WebviewWindow::url()` 对照：

- fragment 精确匹配 -> `LegacyUrlChannelMatched/73`
- Source 可读但 fragment 不匹配 -> `LegacyUrlChannelMiss/79`
- URL 查询返回 `Err` -> `LegacyUrlQueryFailed/80`

`79` 和 `80` 只证明 direct eval 已观察到精确 ACL，而旧 Source 通道漏报或失败；
它们不得被安全 smoke 包装成成功。`73` 只表示该次旧通道也匹配，不能证明旧根因已
消失。

阶段一退出码：

| 分类 | 退出码 |
|---|---:|
| `LegacyUrlChannelMatched` | 73 |
| `DocumentNotReady` | 74 |
| `ProbeScriptNotStarted` | 75 |
| `InvokeTimeout` | 76 |
| `InvokeUnexpectedlyAllowed` | 77 |
| `InvokeOtherError` | 78 |
| `LegacyUrlChannelMiss` | 79 |
| `LegacyUrlQueryFailed` | 80 |
| `EvalDispatchFailed` | 81 |
| `EvalCallbackTimeout` | 82 |
| `InvalidEvalObservation` | 83 |
| `EvalScriptFailed` | 84 |

### 阶段二：最终结果通道

仅在阶段一 fresh evidence Go 后实施：

1. 删除全部 runtime `WebviewWindow::url()` 与 legacy 分类。
2. 使用 `classify_final(observation)`，同样不接收 page-load 参数。
3. 只有严格 observation 的 readyState、state、hash 与 evalError 同时满足条件时，
   返回 `ExactAclDenied/73`。
4. 保留 direct eval readiness loop、single callback 所有权和全部非 `73` 分类。

两个阶段的 runtime 都禁止 `on_page_load`、`PageLoadEvent`、page-load channel、
sender guard 和 `page_load_finished` 谓词。

## 验证门禁

### 纯逻辑与所有权

- 严格 parser 覆盖 null、非对象、缺/多字段、未知枚举与无效 JSON。
- readiness classifier 覆盖 loading/null/pending/success/other/exact ACL/不一致组合。
- 启动期 dispatch `Err` 在首个 callback 前可重试；总 deadline 后固定为 `81`。
- 首个 callback 后的 dispatch `Err` 立即为 `81`。
- 时序测试证明 dispatch 成功后只等待 post-dispatch remaining，不存在独立 callback
  timeout，等待不越过总 deadline。
- 时序测试证明 callback 未在总 deadline 前返回固定为终态 `82`，且等待期间 dispatch
  count 始终为 `1`。
- dispatch 推进测试时钟后，callback wait 必须使用 post-dispatch remaining。
- retry delay 也必须受 remaining 限制；deadline 后不得再 dispatch。
- source gate 禁止
  `on_page_load`、`PageLoadEvent`、page-load sender/receiver/guard 与
  `page_load_finished`。
- 不得用 lint suppression 隐藏阶段接口或 dead code。

### 安全与范围

- `hostname.exe` 负例精确认证退出码 `1`、got-`0` 文本且无成功标记。
- 配置白名单和全部恶意 fixture 继续通过。
- production build 不包含探针页面、前端模块或 Rust 探针代码。
- 实现只允许修改 `src/security-probe.ts` 与
  `src-tauri/src/security_probe.rs`；tracked 与 untracked 都纳入断言。
- 不新增 command、capability、配置、依赖、脚本协议或结果文件。

### 阶段一 fresh 证据

1. 构建一个全新诊断产物，记录 build UTC、artifact UTC 与 SHA-256。
2. 不预热，直接运行该 SHA 的第一次 raw 诊断。
3. 同一未变化 SHA 累计运行 3 次；每次只能返回 `73`、`79` 或 `80`。
4. 任一次 `74-78`、`81-84`、超时、hash/app-data/profile/process 变化立即 No-Go，
   不追加运行稀释失败。
5. 逐次记录 raw 退出码与固定分类；现有安全 smoke 仍只接受 `73`。

### 阶段二 cold smoke

- 构建新的 feature-only 产物，不预热的首次 smoke 必须内部退出 `73`。
- 同一 SHA 连续至少 3/3 通过；任一次失败即 No-Go。
- 每次前后正式 app-data 文件族不变，无探针进程或临时 profile 残留。

## 方案退出条件

若 direct eval 在窗口 build 后持续同步返回 `Err`、callback 无法在总 deadline 前返回，
或 fresh 首次运行不能产生 `73/79/80`，则本候选 No-Go。不得重新引入
`on_page_load`、低层 WebView2 handler 或 backchannel 作为实现期补丁；必须回到
产品/架构决策，选择移除 runtime probe gate 或单独批准更高风险路线。

## 文件与交付顺序

本次修订只修改本文档。失败 R1 worktree 的两文件 diff 保持 unstaged、未提交，仅作
证据，不得 cherry-pick 或声明完成。

书面设计获批后：

1. 只更新 Foundation Task 1A-R 实施计划，删除 page-load 合同并加入 direct-eval
   readiness RED/GREEN 与 source gate。
2. direct-eval 实施计划书面 Go 后，创建新的 immutable lightweight tag
   `foundation-security-probe-direct-eval-approved-plan`，精确指向获批计划提交。所有
   direct-eval 实施范围门禁只以该新 tag 为基线。
3. 旧 tag `foundation-security-probe-reliability-approved-plan` 固定保留在 `157ad89`，
   仅作失败 page-load 方案的历史证据，不移动、不复用。
4. 从新 approved-plan tag 建立 clean 实现工作点，重新执行 TDD；不在失败 worktree 上
   叠加修复。
5. 阶段一代码与 fresh evidence Go 后才允许阶段二。
6. 探针修复代码级 Go 后，才允许非破坏性集成回 Task 4 分支并从头运行 Task 4C
   completion gate。

## 非目标

- 不修改失败 R1 源码、Task 4C 导出服务或当前实施计划。
- 不接线 Task 5 command、UI、协议或生命周期。
- 不实现 `/find`。
- 不合并 main、不推送、不签名、不发布。
