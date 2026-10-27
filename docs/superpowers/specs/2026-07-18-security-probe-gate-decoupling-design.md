# 安全探针任务门禁与发布硬闸门解耦设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 基线：`foundation-security-probe-direct-eval-approved-plan`
  (`18eb565e57f3562883d2bef3e389b049c54960c0`)
- 分支：`codex/security-probe-gate-decoupling-design`
- 影响范围：Foundation 任务完成门禁、Task 4C 恢复条件与发布安全闸门

## 决策摘要

采用方案 A：runtime positive probe 不再阻断每个与安全信任输入无关的 Foundation
任务，但只允许在以下条件同时成立时解耦：

1. 任务相对获批 trust baseline 对冻结的 trust-input 集合零变化；
2. 精确静态配置检查和全部恶意 fixtures 通过；
3. production 构建仍不包含 probe 页面或 probe 代码；
4. 任务报告明确标记 runtime probe 为未解决的 release blocker，而不是通过或忽略。

runtime positive probe 继续作为签名安装包、公开试用、公开演示、下载和发布的硬闸门。
在探针通道经独立设计修复并通过 cold evidence 前，不得进行这些动作；唯一例外是用户在
收到具体风险说明后，对精确 commit/artifact 作出显式 security waiver。当前没有 waiver，
也不授权删除探针。

普通 Git 源代码推送不等同发布，不属于 `ReleaseSecurityBlocked` 的语义。它仍受当前协作
流程约束：未经用户明确授权不得推送；这项操作授权不能解释为 runtime security waiver。

开放的发布阻断编号固定为 `SEC-RUNTIME-PROBE-001`。所有后续 Task、Foundation 和
MVP-A 状态报告都必须携带该编号，直到技术修复或精确 waiver 正式关闭它。

这个决策只解耦“安全无关任务能否完成”和“产品能否发布”，不降低发布安全标准。

## 背景与证据

Foundation 的静态安全模型已具备精确 capability/config 白名单和恶意 fixture 回归。
runtime probe 原计划用 feature-only WebView 调用无权限的 `load_settings`，只有前端观察到
精确 ACL 拒绝时才退出 `73`。三条真实结果通道均未形成可发布的稳定证据：

1. native `WebviewWindow::url()` 依赖 WebView2 `Source`，fresh 运行不能稳定观察 fragment；
2. Wry `on_page_load` 在调用用户 handler 前同样读取 `Source`，fresh 产物分别以 `74`
   失败；sender guard 只把早期 disconnect 延迟为总 deadline；
3. direct `eval_with_callback` 的 fresh 产物
   `38A4C4ABB1A00863FFCA93F1F2CE4408772DE32AFD47696C89656DDBAE053DB7`
   在未预热首次运行返回 `82`。

第三次失败的框架边界已经定位。`tauri-runtime-wry 2.11.4` 的后台
`eval_script_with_callback` 只把 `EvaluateScriptWithCallback` 消息排入 event loop；
主线程随后调用 WebView2 时若失败，只记录错误并丢弃 callback，不把错误回传调用线程。
因此“callback 前同步 dispatch `Err` 可重试”在真实生产路径不可观测，单元测试的
dispatch seam 无法证明该 runtime 语义。

继续增加 timeout、sender guard、generation 或另一条 backchannel 都是在任务实现期扩张
测试架构，不能提高当前 Foundation 功能的产品证据。另一方面，因为 runtime probe 未
通过就直接删除发布闸门，会降低已经冻结的安全保证。需要把两个决策点分离。

## 目标

1. 允许不触碰安全信任输入的 Foundation 任务继续完成和接受代码复审。
2. 用 Task 4C 计划内一次性、失败闭合的 Git/.NET 范围门禁证明任务没有改变 trust
   inputs。
3. 保留精确静态配置检查和恶意 fixtures 作为每个解耦任务的必跑门禁。
4. 保持 runtime probe 对签名、公开试用、公开演示、下载和发布的硬阻断。
5. 为 Task 4C 定义唯一恢复路径，不带入两个失败 worktree 的代码。
6. 对任何安全相关变更自动取消解耦资格并要求独立安全复审。

## 非目标

- 不修复或重试 native URL、Wry page-load、direct eval、event-loop barrier 或 WebView2
  通道。
- 不删除 runtime probe、feature、脚本、页面、配置或 capability。
- 不把静态门禁声明为 runtime ACL 的等价证明。
- 不批准 Task 5+、`/find`、签名、外部试用或发布；源代码推送仍需用户单独授权。
- 不清理、提交或复用两个失败实现 worktree。

## 方案比较

### 方案 A：按 trust inputs 零变化解耦任务门禁

采用。安全无关任务可以得到 `TaskCodeGo`，但 runtime probe 保持
`ReleaseSecurityBlocked`。任务必须从获批 trust baseline 计算 committed、index、worktree、
untracked 和 ignored trust 候选的完整变化集；任一 trust-input 路径出现即失败闭合。

优点：保留发布保证，同时避免一个 test-instrumentation 通道阻塞纯持久化、导出等本地
业务代码。代价：需要冻结 trust-input manifest，并在 Task 4C 计划中写清一次性范围门禁
和双状态报告。

### 方案 B：删除 runtime probe，只保留静态门禁

拒绝。静态白名单只能证明配置文本符合预期，不能证明实际打包产物中的 WebView 无法调用
未授权命令。删除 runtime probe 会降低发布保证，必须由用户显式批准，当前不授权。

### 方案 C：继续开发通道修复

冻结。候选包括 `RunEvent::Ready`/event-loop barrier、多 generation eval、低层 WebView2
handler 和 command/event/file backchannel。它们都会新增生命周期、线程、native/unsafe
或攻击面合同。三条真实通道失败后，不再允许在 Foundation 任务实现期“再试一次”；若将来
重启，必须单独立项、先设计和计划、再 TDD。

## 双门禁状态模型

每个交付点必须分别报告两个状态，不得用一个 `Go` 覆盖：

| 状态 | 含义 | 可允许动作 |
|---|---|---|
| `TaskCodeGo` | 任务功能、质量门禁通过，且 trust inputs 零变化 | 本地任务提交、代码复审、获批后的本地非破坏性集成 |
| `TaskSecurityReviewRequired` | 任务触碰任一 trust input 或 diff oracle 不可认证 | 停止任务合并，进入独立安全设计/复审 |
| `ReleaseSecurityBlocked` | runtime positive probe 未通过或证据已过期 | 禁止签名、公开试用、公开演示、下载和发布 |
| `ReleaseSecurityGo` | 精确发布候选通过另行批准的 cold runtime evidence | 仍需发布计划和用户授权，不能自动发布 |

`TaskCodeGo + ReleaseSecurityBlocked` 是方案 A 的正常状态，不是矛盾。报告必须同时显示
二者，且完成 Foundation 子任务不得表述为“安全探针已通过”或“MVP-A 可发布”。

## Trust-input 集合

### 路径 manifest

匹配在 Git 路径上执行：统一 `/`，使用 ordinal ignore-case，目录规则包含 tracked 和
untracked 的所有后代。当前冻结集合至少包括：

| 类别 | 路径或规则 |
|---|---|
| Capability | `src-tauri/capabilities/**` |
| Command permissions | `src-tauri/permissions/**` |
| Tauri config/override | `src-tauri/` 根下匹配 `tauri*.conf.json`、`tauri*.conf.json5` 或 `tauri*.conf.toml` 的所有文件 |
| Static security oracle | `scripts/check-security-config.ps1`, `scripts/test-security-config.ps1` |
| Runtime probe build/test | `scripts/build-security-probe.ps1`, `scripts/test-security-probe.ps1` |
| Probe frontend | `security-probe.html`, `src/security-probe.ts` |
| Probe/runtime wiring | `src-tauri/src/security_probe.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/main.rs` |
| Rust feature/dependencies | `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/build.rs` |
| Frontend build wiring | `vite.config.ts`, `package.json`, `package-lock.json`, `tsconfig.json`, `index.html` |
| Git path controls | `.gitignore`, `.gitattributes`, `.gitmodules`（存在即纳入） |

以下规则同样失败闭合：

- 新增其他 package-manager lockfile、Vite config、TypeScript config、Tauri config override、
  capability 或 permission 文件，视为 trust-input 变化；
- 文件删除、重命名、大小写变化和从 trust 目录移出都视为变化；
- 新注册的 Tauri command 所在源文件必须在独立安全复审中加入 manifest。当前已注册的
  feature-only command 源由 `security_probe.rs` 覆盖；未来只改实现文件但不改 `lib.rs`
  不能绕过该规则；
- manifest 只能经独立安全设计/复审修改；普通任务不能缩小或排除路径。

路径集合是下限。安全复审可以扩大集合，但普通任务不得缩小或排除路径。

### Task 4C 一次性范围门禁

Task 4C 是当前唯一消费者，不新增通用 checker、manifest classifier 或 fixture 框架。获批
实施计划必须内联一段 PowerShell 门禁，直接使用现有 Git 和 .NET API，并固定以下合同：

1. 创建 immutable lightweight tag `foundation-task-4c-trust-baseline`，精确指向
   `6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`；门禁验证 tag 解析为该 SHA，且该 SHA 是
   当前 `HEAD` 的祖先。tag 名和 SHA 硬编码在获批计划中，不能替换或从环境变量读取；
2. 兼容仓库当前 Windows PowerShell 5.1/.NET Framework 环境：用
   `System.Diagnostics.ProcessStartInfo`，`FileName` 设为 `Get-Command git.exe` 解析并验证
   得到的绝对应用路径，`Arguments` 设为获批计划逐字冻结、完全硬编码且不接收外部输入的
   Windows 参数字符串，`RedirectStandardOutput = true`；从 `StandardOutput.BaseStream`
   读取二进制 stdout，并严格按 NUL 分隔解析。不得使用该环境不存在的 `ArgumentList`，
   也不新增 PowerShell 7 依赖；
3. 对同一 baseline、`HEAD`、index 和 worktree 分别计算两个视图。trust 视图的三次
   `git diff --no-renames --name-status -z` 只传入获批计划硬编码的 frozen trust pathspec，
   合并结果必须为空；完整任务视图的三次 diff 不传 pathspec，保留删除和重命名前旧路径，
   用于最后的精确 allowlist 断言；
4. `git ls-files --others --exclude-standard -z` 收集全部非 ignored untracked 路径并纳入最终
   变化集。另以 frozen manifest 的固定 literal/glob pathspec 运行
   `git ls-files --others --ignored --exclude-standard -z -- <trust-pathspecs>`，只检查 ignored
   trust 候选，避免枚举 `target` 等无关构建目录；返回任一路径即失败，不读取或哈希内容；
5. 严格解析 `git ls-files -v -z`。任一 trust 路径带 assume-unchanged 或 skip-worktree
   标记即失败；命令失败、未知状态字符或输出无法严格解码同样失败；
6. 先认证 `git rev-parse --show-toplevel` 得到的 worktree 根与当前工作目录一致。再从该根
   开始，到每个现存 trust 根、trust 文件、ignored/untracked candidate 为止，对每一级现存
   路径组件逐项调用 .NET `File.GetAttributes`；任一父目录或最终组件命中 `ReparsePoint`、
   类型不符、组件越出认证根或元数据读取失败都失败闭合；
7. Git 原始路径必须用严格 UTF-8 解码，规范为 `/` 后按 ordinal ignore-case 匹配 manifest；
   NUL 记录结构错误、绝对路径、`.`/`..` 段、反斜杠或控制字符都失败闭合；
8. 合并完整任务视图的 tracked 变化和非 ignored untracked 路径，排序、去重后的完整变化集
   必须精确等于两项：
   `docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md` 和
   `src-tauri/src/validation_export.rs`；trust 视图和 ignored trust 候选集必须为空。路径
   缺失、出现第三项、trust 命中或 Git/.NET 命令不可运行均返回非零；
9. 门禁不接收排除规则、`--force`、skip 或 waiver 参数。命令文本和预期成功标记直接写入
   获批 Task 4C 计划，由代码复审对实际执行证据逐项核对。

这里不计算 worktree 内容的 Git clean-filter blob。tracked trust 内容变化由三个 restricted
trust diff 直接检测，并通过拒绝 assume-unchanged/skip-worktree 关闭 stat
旁路；untracked、ignored 和 reparse 候选只按存在性与类型失败。若第二个真实任务需要重复
同一门禁，再单独评审是否抽取脚本，Task 4C 不预付这项抽象成本。

## 解耦任务门禁

当前只有 Task 4C 可以使用该解耦合同。它只有在下面全部通过时才能得到 `TaskCodeGo`：

1. 任务计划已经批准，并声明精确功能文件 allowlist；
2. 计划内一次性范围门禁验证 immutable baseline、完整变化集和 trust inputs，并输出固定
   成功标记；
3. `scripts/check-security-config.ps1` 退出 `0` 并输出 `security config ok`；
4. `scripts/test-security-config.ps1` 退出 `0` 并输出
   `security config regression tests ok`；
5. production `npm.cmd run build` 退出 `0`，且 `dist/security-probe.html` 不存在；
6. 任务自身 default/all-features tests、check、Clippy、范围和 worktree 门禁通过；
7. 报告附 baseline tag/SHA、完整变化集、trust 命中数 `0`、静态门禁输出和
   `ReleaseSecurityBlocked` 状态。

以下任一情况直接得到 `TaskSecurityReviewRequired`，不能用人工口头确认继续：

- 一次性范围门禁不可运行、输出不可解析、baseline 不匹配或不是祖先；
- 任一 trust input 变化；
- 现有静态 checker 或任一现有恶意 fixture 失败；
- production 构建包含 probe 页面；
- 任务需要修改 manifest 或内联门禁来让自己通过。

runtime positive probe 在解耦任务 completion gate 中记录为已知 release blocker，不运行
预热/重试来制造任务成功。它也不能被替换为 hostname 负例；负例只认证测试脚本的失败
oracle，不证明 ACL runtime positive path。

## Task 4C 恢复合同

Task 4C 当前分支 `codex/foundation-task-4` 固定在
`6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`，唯一未提交内容是
`src-tauri/src/validation_export.rs`。该文件不在当前 trust-input 集合。

Task 4C 只可按以下顺序恢复：

1. 本设计书面 Go；
2. 在独立文档分支更新
   `docs/superpowers/plans/2026-07-18-task-4c-validation-export-service.md`，内联一次性
   Git/.NET 范围门禁、精确 SHA/tag、PowerShell 5.1 命令和两个允许路径，再做书面复审；
3. 计划书面 Go 后，先创建 `foundation-task-4c-trust-baseline` 并验证它精确指向
   `6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`；
4. 把获批计划的精确单文件提交非破坏性集成进 `codex/foundation-task-4`，提交前后都验证
   现有 `validation_export.rs` 仍为唯一未暂存代码 diff，且计划文件是该次集成唯一提交项；
5. 从头运行 Task 4C completion gate、restricted trust 零变化门禁、两文件完整 allowlist、
   静态安全门禁和 production 隔离门禁；
6. Task 4C 代码级 Go 后只允许本地任务提交和获批的本地非破坏性集成。

任何一步触碰 trust input、无法保全现有 Task 4C diff，或要求清理两个失败 probe
worktree，立即停止并重新复审。Task 4C 完成不等于 Foundation 完成，更不等于 MVP-A
可发布。

## 发布硬闸门

以下动作继续禁止：

- 构建或分发签名安装包；
- 启动 12 人试用、公开下载、公开演示构建或任何外部发布；
- 声明 security gate、Foundation 或 MVP-A release ready。

源代码提交、合并或推送不由本安全状态自动禁止或授权。它们继续遵守各任务的代码复审和
用户操作授权；即使用户授权普通 source push，也不改变 `ReleaseSecurityBlocked`。

解除 `ReleaseSecurityBlocked` 需要二选一：

1. **技术修复路径**：新的独立 probe 设计和实施计划均获 Go；精确发布候选的 fresh
   feature-only 产物首次 cold run 通过，且同一 SHA 连续满足批准次数；app-data、profile、
   process、配置和签名链门禁全部通过。
2. **显式 waiver 路径**：用户明确写出接受 runtime ACL 未被 positive probe 证明的风险，
   waiver 绑定精确 commit、artifact SHA、分发范围和到期条件，并经独立安全复审记录补偿
   控制。普通的“继续”“同意”“合并”或任务 Go 不构成 waiver。

waiver 只处理 runtime probe 风险，不自动授权 Git push、签名或发布；这些外部动作仍需
各自明确授权。当前不选择 waiver 路径。

## 失败实现保全

以下 worktree 保持 unstaged、未提交、只读证据状态，直到独立清理决策：

- `D:\code\UiPilot_tools\.worktrees\security-probe-reliability`：page-load 失败 R1；
- `D:\code\UiPilot_tools\.worktrees\security-probe-direct-eval`：direct-eval fresh `82` R1。

不得 cherry-pick、部分复制或把其单元测试通过结果声明为 runtime 证据。Task 4C 不从这
两个 worktree 复制任何代码。

## 交付顺序

1. 本次只新增本文档并送书面复审。
2. 设计 Go 后只在独立文档分支更新 Task 4C 实施计划，写入一次性内联门禁；计划复审前不
   修改脚本、代码或 Task 4 工作区。
3. 计划 Go 后创建获批 baseline tag，再把获批计划的精确单文件提交集成进 Task 4 分支，
   保全未暂存代码，然后从头运行 Task 4C completion gate；不新增通用门禁代码或 fixtures。
4. Task 4C 代码级 Go 后仍保持 `ReleaseSecurityBlocked`，不得进入 Task 5+ 或发布流程，
   除非另有
   明确授权。

## 验收矩阵

| 场景 | Task gate | Release gate |
|---|---|---|
| 只改获批 Task 4C 计划和 `validation_export.rs`，trust/static/quality 全绿 | `TaskCodeGo` | `ReleaseSecurityBlocked` |
| 修改 capability/config/security script/probe wiring | `TaskSecurityReviewRequired` | `ReleaseSecurityBlocked` |
| 内联门禁 baseline 不匹配或漏算 tracked/untracked/ignored | Fail closed | `ReleaseSecurityBlocked` |
| 静态配置或恶意 fixture 失败 | Fail closed | `ReleaseSecurityBlocked` |
| runtime probe 仍为 74/82 | 不反向否决已隔离的 TaskCodeGo | `ReleaseSecurityBlocked` |
| 新 probe cold evidence 对精确候选通过 | 仍需任务各自 Go | 可申请 `ReleaseSecurityGo` 复审 |
| 用户未给精确 waiver | 不适用 | 保持阻断 |

## 完成边界

本文档获批只表示可以编写门禁解耦实施计划。它不解冻 Task 4C，不授权修改安全脚本，
不批准任何失败 probe 代码，也不允许签名、试用或发布；源代码推送仍需用户单独授权。
