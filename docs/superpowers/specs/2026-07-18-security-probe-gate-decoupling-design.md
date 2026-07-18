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

runtime positive probe 继续作为 main 推送、签名安装包、公开试用和发布的硬闸门。
在探针通道经独立设计修复并通过 cold evidence 前，不得进行这些动作；唯一例外是用户在
收到具体风险说明后，对精确 commit/artifact 作出显式 security waiver。当前没有 waiver，
也不授权删除探针。

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
2. 用可执行、失败闭合的 Git 范围门禁证明任务没有改变 trust inputs。
3. 保留精确静态配置检查和恶意 fixtures 作为每个解耦任务的必跑门禁。
4. 保持 runtime probe 对 main 推送、签名、公开试用和发布的硬阻断。
5. 为 Task 4C 定义唯一恢复路径，不带入两个失败 worktree 的代码。
6. 对任何安全相关变更自动取消解耦资格并要求独立安全复审。

## 非目标

- 不修复或重试 native URL、Wry page-load、direct eval、event-loop barrier 或 WebView2
  通道。
- 不删除 runtime probe、feature、脚本、页面、配置或 capability。
- 不把静态门禁声明为 runtime ACL 的等价证明。
- 不批准 Task 5+、`/find`、main 合并/推送、签名或发布。
- 不清理、提交或复用两个失败实现 worktree。

## 方案比较

### 方案 A：按 trust inputs 零变化解耦任务门禁

采用。安全无关任务可以得到 `TaskCodeGo`，但 runtime probe 保持
`ReleaseSecurityBlocked`。任务必须从获批 trust baseline 计算 committed、index、
worktree 和 untracked 的完整变化集；任一 trust-input 路径出现即失败闭合。

优点：保留发布保证，同时避免一个 test-instrumentation 通道阻塞纯持久化、导出等本地
业务代码。代价：需要冻结 trust-input manifest、实现结构化 diff checker，并在每个任务
报告中维护双状态。

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
| `ReleaseSecurityBlocked` | runtime positive probe 未通过或证据已过期 | 禁止 main 推送、签名、公开试用和发布 |
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
| Trust diff oracle | 后续获批的 `scripts/check-security-trust-inputs.ps1`, `scripts/test-security-trust-inputs.ps1` |
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
- manifest、checker 或 fixture 自身是 trust inputs，不能由使用解耦门禁的任务修改。

路径集合是下限。安全复审可以扩大集合，但普通任务不得缩小或排除路径。

### 结构化 diff oracle

书面设计获批后，先独立实现并代码复审
`check-security-trust-inputs.ps1` 及恶意 fixtures，再允许 Task 4C 使用。checker 必须：

1. 接收一个不可变 trust baseline tag；验证 tag 存在、解析为预期 commit，且为当前 HEAD
   祖先；
2. 用 `git diff --no-renames` 计算 baseline 到 HEAD、index 和 worktree 的 tracked 变化，
   并合并 `git ls-files --others --exclude-standard` 的 untracked 路径；
3. 独立通过 .NET 文件系统 API 枚举 manifest 的目录规则和根级 glob，包含 Git ignored
   文件；把当前 trust inventory 与 baseline 的 `git ls-tree` inventory 比较。每项包含
   规范路径、类型和经 Git clean filter 计算的 blob ID，不能只比较文件名。新增 ignored
   capability/config/lockfile、内容变化、删除文件、普通文件变为 reparse point，或无法
   读取元数据都必须失败；不得依赖 `.gitignore`、stat cache、assume-unchanged 或
   skip-worktree 判断安全路径是否变化；
4. 排序去重后先验证每个路径只含批准的安全字符；引号、控制字符、绝对路径、`.`/`..`
   段、反斜杠或无法无损解析的输出使整个门禁失败，不尝试猜测；
5. 对删除和重命名保留旧路径。使用 `--no-renames`，确保从 trust 集合移出表现为旧路径
   删除加新路径新增；
6. 对每个变化路径运行同一 manifest classifier；命中一个即以非零退出，并列出相对路径；
7. 只有 Git 变化集和文件系统 inventory 都证明 trust 命中数严格为零时输出固定成功
   标记；空变化集也可成功；
8. 不接受前端、环境变量或调用方提供的排除规则，不支持 `--force`、skip 或 waiver 参数。

checker 不能自证清白。每个任务的获批计划必须先用 Git 原生命令验证当前 checker 和
fixture 文件的 blob ID 与 baseline 中对应 blob ID 精确相同，且无 staged/worktree
变化；只有这个最小 bootstrap 通过后才执行 worktree 中的 checker。静态配置脚本也在
trust checker 通过后运行。调用顺序不得反转。

fixtures 至少证明：tracked 修改、staged 修改、已提交修改、删除、rename 出目录、大小写
变更、untracked 文件、嵌套 capability 的 JSON/JSON5/TOML、新 config override、新 lockfile、
被 `.gitignore` 隐藏的 trust 文件、reparse point、Git path-control 修改、checker 自修改和
不安全路径编码全部被拒绝；普通 `validation_export.rs` 变化被接受。

单纯在任务命令中写一组 `git diff --name-only` 不足以建立此安全边界；checker 与 fixtures
代码级 Go 是 Task 4C 解冻的前置交付物。

## 解耦任务门禁

一个任务只有在下面全部通过时才能得到 `TaskCodeGo`：

1. 任务计划已经批准，并声明精确功能文件 allowlist；
2. trust diff checker 对该任务的 immutable trust baseline 输出固定成功标记；
3. `scripts/check-security-config.ps1` 退出 `0` 并输出 `security config ok`；
4. `scripts/test-security-config.ps1` 退出 `0` 并输出
   `security config regression tests ok`；
5. production `npm.cmd run build` 退出 `0`，且 `dist/security-probe.html` 不存在；
6. 任务自身 default/all-features tests、check、Clippy、范围和 worktree 门禁通过；
7. 报告附 baseline tag/SHA、完整变化集、trust 命中数 `0`、静态门禁输出和
   `ReleaseSecurityBlocked` 状态。

以下任一情况直接得到 `TaskSecurityReviewRequired`，不能用人工口头确认继续：

- trust diff checker 不可运行、输出不可解析、baseline 不匹配或不是祖先；
- 任一 trust input 变化；
- 静态 checker 或任一恶意 fixture 失败；
- production 构建包含 probe 页面；
- 任务需要修改 manifest/checker 来让自己通过。

runtime positive probe 在解耦任务 completion gate 中记录为已知 release blocker，不运行
预热/重试来制造任务成功。它也不能被替换为 hostname 负例；负例只认证测试脚本的失败
oracle，不证明 ACL runtime positive path。

## Task 4C 恢复合同

Task 4C 当前分支 `codex/foundation-task-4` 固定在
`6b2348e3694f4a8f45df97d84ccf6f1de3c6d516`，唯一未提交内容是
`src-tauri/src/validation_export.rs`。该文件不在当前 trust-input 集合。

Task 4C 只可按以下顺序恢复：

1. 本设计书面 Go；
2. 更新门禁解耦实施计划，计划 Go 后实现 trust diff checker/fixtures；
3. checker/fixtures 代码级 Go；
4. 将获批门禁提交非破坏性集成到 Task 4 分支，精确暂存相关文件，不触碰现有
   `validation_export.rs` diff；
5. 在集成后的 clean trust inputs 上创建新的 immutable Task 4C trust baseline tag；
6. 从头运行 Task 4C completion gate、trust 零变化门禁、静态安全门禁和 production
   隔离门禁；
7. Task 4C 代码级 Go 后只允许本地任务提交和获批的本地非破坏性集成。

任何一步触碰 trust input、无法保全现有 Task 4C diff，或要求清理两个失败 probe
worktree，立即停止并重新复审。Task 4C 完成不等于 Foundation 完成，更不等于 MVP-A
可发布。

## 发布硬闸门

以下动作继续禁止：

- 向 `origin/main` 推送或把 Foundation 工作合入可发布主线；
- 构建或分发签名安装包；
- 启动 12 人试用、公开下载、公开演示构建或任何外部发布；
- 声明 security gate、Foundation 或 MVP-A release ready。

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

不得 cherry-pick、部分复制或把其单元测试通过结果声明为 runtime 证据。设计和 checker
实现都从获批 tag 的 clean worktree 开始。

## 交付顺序

1. 本次只新增本文档并送书面复审。
2. 设计 Go 后只更新门禁解耦实施计划；计划复审前不修改脚本或 Task 4。
3. 计划 Go 后在新 worktree 中 TDD 实现 trust checker/fixtures，并做代码级复审。
4. 门禁代码 Go 后才允许制定 Task 4C 保全式集成步骤和创建 trust baseline tag。
5. Task 4C 完成后仍保持 `ReleaseSecurityBlocked`，不得进入 Task 5+ 或发布流程，除非另有
   明确授权。

## 验收矩阵

| 场景 | Task gate | Release gate |
|---|---|---|
| 只改 `validation_export.rs`，trust/static/quality 全绿 | `TaskCodeGo` | `ReleaseSecurityBlocked` |
| 修改 capability/config/security script/probe wiring | `TaskSecurityReviewRequired` | `ReleaseSecurityBlocked` |
| checker baseline 不匹配或漏算 untracked | Fail closed | `ReleaseSecurityBlocked` |
| 静态配置或恶意 fixture 失败 | Fail closed | `ReleaseSecurityBlocked` |
| runtime probe 仍为 74/82 | 不反向否决已隔离的 TaskCodeGo | `ReleaseSecurityBlocked` |
| 新 probe cold evidence 对精确候选通过 | 仍需任务各自 Go | 可申请 `ReleaseSecurityGo` 复审 |
| 用户未给精确 waiver | 不适用 | 保持阻断 |

## 完成边界

本文档获批只表示可以编写门禁解耦实施计划。它不解冻 Task 4C，不授权修改安全脚本，
不批准任何失败 probe 代码，也不允许 main 推送、签名、试用或发布。
