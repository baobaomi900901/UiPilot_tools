# UiPilot 托管 Everything 生命周期设计

## 状态

- 日期：2026-09-04
- 状态：用户已批准架构及原生 Rust 安装器 Helper 修订，等待书面规格复核
- 范围：Windows x64 安装包与生产运行时
- 依赖版本：Everything 1.4.1.1032 x64 Stable

## 目标

正式安装版 UiPilot 在干净电脑上完成一次安装后，普通用户启动
`UiPilot_tools.exe` 即可使用 `/find`，不需要手动安装、配置或启动 Everything。

安装器负责 UiPilot 专用 Everything Service；UiPilot 运行时负责当前用户会话中的隐藏
Everything 客户端。两者使用私有 instance 和 pipe，不复用、修改或关闭用户自行安装的
Everything。

## 当前状态与缺口

- `Everything.exe`、License 和 lock 已列入 Tauri bundle resources，发布构建也会把它们暂存到
  `target/release/resources/everything/`。
- NSIS 已配置 `perMachine`，但尚未引用 installer hooks。
- 开发脚本会启动 Everything 客户端，生产程序不会。
- 生产 `/find` 当前通过 `EverythingClient::connect("")` 连接默认未命名实例。因此目标机没有
  已运行的默认 Everything 时，查询返回 unavailable。

本设计补齐安装与生产生命周期，并扩展 Query2 client 的连接身份与 readiness probe。现有 Query2
payload 编解码、文件结果协议、ResultRegistry 和路径认证边界保持不变。

## 已确认决策

1. 使用 UiPilot 专用 Service、named instance、pipe、配置和数据库。
2. 安装器为 per-machine，并在安装时请求一次 UAC。
3. 标准用户运行 UiPilot 时必须可用，即使安装时输入的是另一个管理员账户的凭据。
4. Service 随 Windows 启动，UiPilot 退出时不停止 Service。
5. UiPilot 启动隐藏客户端；退出时只终止自己拥有的客户端进程。
6. 不探测、复用、修改、关闭或卸载用户自己的默认 Everything。
7. Everything 启动和建库不得阻塞 UiPilot 主界面显示。

本设计取代 `2026-07-23-find-everything-managed-migration-design.md` 中两项尚未实现的合同：

- Service pipe 不再把提供 UAC 凭据的 elevated token 用户 SID 记录为 owner，而是记录并授权首次安装
  所在 Windows session 的 Explorer shell token SID（`desktop_user_sid`）；管理员仍通过 Built-in
  Administrators ACE 获得访问；
- 本次不引入 backend runtime epoch/generation、typed lifecycle transition 或 ResultRegistry runtime
  invalidation。已发布结果的执行不依赖 Everything 进程，并继续通过现有逐组件路径身份重新认证；客户端
  重启本身不使当前结果失效。下一次查询获准时，现有 `begin_query` 立即退役旧 ResultSet；成功时发布
  新集合，失败时保持为空。隐藏窗口或新 invocation 继续按现有 ResultRegistry 生命周期清理旧结果。

旧设计中的第三方校验、WebView 不提交路径/查询语法、私有 Everything 身份和逐组件路径认证边界继续
有效。以上取代关系是穷举的，不再笼统继承旧设计的其他生命周期合同。

## 固定身份与路径

- Everything instance name：`UiPilot`
- Windows Service name：`Everything (UiPilot)`
- Everything service pipe name：`\\.\PIPE\UiPilotEverythingService`
- Service pipe SDDL 模板：
  `D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;${desktop_user_sid})`
- 安装资源：`<UiPilot install>/resources/everything/Everything.exe`
- 受保护 Service runtime：
  `%ProgramData%/UiPilot_tools/everything/<version>/Everything.exe`
- 受保护安装清单：`%ProgramData%/UiPilot_tools/everything/managed-runtime.json`
- 受保护事务清单：`%ProgramData%/UiPilot_tools/everything/managed-transaction.json`
- 全机安装事务 mutex：`Global\UiPilotManagedEverythingInstaller`
- 全机 mutex SDDL：`O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)`
- 安装事务 activity event：`Global\UiPilotManagedEverythingInstallerActivity`
- activity event SDDL：`O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)`
- 每用户每会话配置目录：`%LOCALAPPDATA%/UiPilot_tools/everything/<session-id>/`
- 每用户配置：`Everything-UiPilot.ini`
- 每用户数据库：`Everything-UiPilot.db`

受保护清单固定版本、架构、SHA-256、Authenticode publisher、instance、Service name、完整 pipe、
`desktop_user_sid`、展开后的 SDDL 和 Service binary 路径。升级、修复和卸载只能操作与清单及固定根
目录同时匹配的对象。没有有效清单时若同名 Service 或受管 ProgramData 目录已经存在，安装失败闭合，
不接管、不覆盖也不删除该状态；唯一例外是“原生安装器 Helper 边界”中穷举定义、可认证为空的
bootstrap residue。

`${desktop_user_sid}` 是本文的运行时替换记号，不是 NSIS compile-time define。首装取得 canonical SID
字符串后构造 `expanded_sddl`，先用 Windows security descriptor API 解析并核对 3 个 ACE，再传给
Everything；清单只保存 canonical SID 与完整 `expanded_sddl`。

## 总体架构

```text
NSIS per-machine installer (elevated)
  ├─ atomically create an ACL-protected helper staging directory
  ├─ extract the Rust installer helper from this trusted Setup
  └─ invoke fixed helper actions
       ├─ verify bundled Everything version, hash and signature
       ├─ stage versioned Service binary under ProgramData
       ├─ install/start UiPilot private Everything Service
       └─ write protected managed-runtime manifest

UiPilot_tools.exe (interactive standard user)
  └─ EverythingRuntimeSupervisor
       ├─ resolve and verify packaged client binary
       ├─ create per-user config/database directory
       ├─ start hidden Everything -instance UiPilot
       ├─ own client with a Windows Job Object
       └─ publish Starting / Ready / Unavailable health

/find
  └─ EverythingClient::connect_owned("UiPilot", expected_pid)
       └─ private client -> UiPilotEverythingService pipe -> Service
```

## 子系统 1：NSIS Service 生命周期

Tauri 配置继续使用 `installMode: "perMachine"`，新增：

- `windows.nsis.installerHooks` -> `src-tauri/windows/nsis-hooks.nsh`；
- `windows.nsis.template` -> `src-tauri/windows/installer.nsi`。

`installer.nsi` 固定派生自 Tauri CLI 2.11.4 官方模板，只允许三个受控行为区域发生差异：

1. NSIS reinstall page 不允许“先卸载旧版再安装”的替换路径。升级、允许的降级以及同版本 repair 都
   直接进入原位覆盖，绝不调用旧 uninstaller；Tauri 自动更新的 `/UPDATE` 路径同样原位覆盖。只有同
   版本维护页明确选择“卸载应用”时才调用旧 uninstaller，调用时不带 `/UPDATE`，成功后立即退出当前
   Setup，不进入目录页或 Install Section。
2. Uninstall Section 先执行 Tauri 的 `CheckIfAppIsRunning`，确认 UiPilot 已退出且用户没有取消，再执行
   `NSIS_HOOK_PREUNINSTALL`。原模板中两者的顺序反转，不能用于有外部 Service 状态的卸载事务。
3. 安装在 `PREINSTALL` 前创建一个受保护 staging 并解出原生 Helper；同一 staging 和 journal 必须跨越
   `PREINSTALL`、Tauri 文件复制、`POSTINSTALL` 以及必要的 rollback，直到安装事务到达终态后才清理。
   真正卸载在 app-running gate 通过后同样创建一个 staging，跨越 `PREUNINSTALL` 和 `POSTUNINSTALL`。
   Helper 不安装到 `$INSTDIR`，也不成为运行时依赖。

Source gate 对模板来源版本、这三个行为区域做快照校验：替换安装不得执行旧 uninstaller，明确卸载
必须在旧 uninstaller 成功后退出，app-running gate 必须位于 `PREUNINSTALL` 前。任何其他模板差异都
使检查失败。升级 Tauri CLI 时必须显式重基模板。

### 原生安装器 Helper 边界

复杂的 JSON、SHA-256、Authenticode、token、security descriptor、SCM 和回滚逻辑不直接用 NSIS
栈语言实现。新增内部二进制 `uipilot-everything-installer.exe`，源码独立放在
`src-tauri/src/bin/uipilot-everything-installer/`，与宿主使用同一 Rust toolchain，但不链接到
`UiPilot_tools.exe`，也不暴露 Tauri command。发布构建在 NSIS bundling 前显式构建 x64 release
Helper；构建失败、目标架构不符或预期 artifact 缺失时禁止生成 Setup。

Helper 是受信任 Setup 的内部 payload，不单独安装或注册。NSIS 不通过环境变量解析 staging 根：它以
当前 elevated process token 调用 `SHGetKnownFolderPath(FOLDERID_LocalAppData)`，在该账户的 Temp 下用
`CoCreateGuid` 生成不可预测目录名和 128-bit transaction ID，并在第一次 `CreateDirectoryW` 时通过 `SECURITY_ATTRIBUTES` 原子
应用受保护 DACL：`D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)`。不得先建普通目录再补 ACL，也不得在调用者
可写的目录中先落 Helper。NSIS 解出文件后，Helper 启动时再次验证 staging canonical path、owner 和
DACL；owner 必须精确等于当前 elevated token 的 `TokenUser`，调用 token 必须是 high-integrity、
fully elevated 且包含 enabled Built-in Administrators SID。Helper 使用 `asInvoker` manifest，绝不自行
请求 UAC。其 executable、request 和 journal 都必须是 staging 的直属、非 reparse 普通文件，并通过
已打开 handle 验证，不允许路径组件替换。不满足时不读取 request/journal、不接触 Service 或 ProgramData。不同 SID 的原标准用户不能读取、替换
或删除由另一个 elevated administrator 创建的 Helper；同一 elevated administrator、Built-in
Administrators 和 LocalSystem 属于安装器既有信任边界。

Helper 的信任根与 NSIS 安装代码相同：只接受当前 Setup 在 bundle 时嵌入、在本次安装中解出的 bytes，
不接受 `$INSTDIR`、ProgramData、PATH、网络或先前 staging 中的同名文件。单独复制出的 Helper 不构成
可分发或可调用接口；替换整个受信任 Setup 已经等价于替换全部 elevated 安装代码，不属于 Helper
额外引入的边界。临时目录的原子 DACL 用于阻止 Setup 启动后的跨 SID 替换竞态。

安装与真正卸载在创建 staging 前取得 `Global\UiPilotManagedEverythingInstaller`。mutex 使用上述固定
BA owner/group 和 LocalSystem/Built-in Administrators DACL；NSIS 持有 ownership 和 handle，覆盖
prepare、recover、文件复制、commit、rollback、pre/post-uninstall 及最终 staging 清理。已存在但正在
占用的 mutex 使第二个 Setup/uninstaller 在任何受管状态读取或修改前失败。`CreateMutexW` 返回既有对象
时，NSIS 必须先通过 handle 验证对象类型、owner/group 和规范化 DACL 精确匹配；打开失败、不同对象类型
或 descriptor 不匹配都在读取受管状态前失败，不修改或接管该对象。此策略允许非管理员抢注造成
fail-closed 本地 DoS，但不允许其影响 Service/rollback；稳定错误要求用户关闭其他安装程序或重启后
重试。abandoned trusted mutex 只能在 Helper 先完成 durable recovery 后继续。

每次新 Setup/uninstaller 生成独立 128-bit `operation_id`，写入本次 staging request/journal。若
`prepare-install` 发现有效 durable journal，它将其中旧 `transaction_id` 作为
`recovered_transaction_id` 写入当前受保护 staging 输出，两个 ID 不要求相等。`recover-install` 同时验证
当前 `operation_id`、staging 中的 recovered ID 与 durable journal ID；恢复并删除旧 durable journal
后，重新执行 `prepare-install`，之后的新 commit 才用当前 `operation_id` 作为新 `transaction_id`。
同一安装事务内的 commit/rollback 则要求 operation/transaction ID 相等。Helper 启动时打开 mutex 并
确认它正被其他进程持有；若当前可立即取得 ownership，说明父 Setup 未持锁，Helper 释放后在读取输入
前失败。管理员属于明确的既有信任边界；该检查用于并发正确性，不试图隔离恶意 elevated administrator。

NSIS 在取得 mutex 后还创建并持有固定 activity event，覆盖同一完整事务。既有 event 与既有 mutex
执行相同的 object type、BA owner/group 和规范化 DACL 验证；任何仍存在的可信 event 表示另一个 Setup
或 Helper 仍活动，当前 Setup 只返回 busy，不开始 recovery。每次 Helper request 还固定记录父 NSIS 的
PID 与 process creation time。Helper 在读取 journal 或开始副作用前验证并打开精确父进程、打开并持有
activity event，再次确认父进程仍未 signaled；PID 已复用、父进程已退出或 event 不匹配都立即失败。

NSIS 正常存活时由其 event handle 覆盖各 Helper 之间的空档。NSIS 在 Helper 启动前退出时 event 消失，
Helper 因父进程验证失败而不能开始；NSIS 在 Helper 已打开 event 后退出时，Helper handle 维持 activity
lease，新 Setup 必须等待或返回 busy。活动 Helper 通过父 process handle 发现退出后，不开始新的 phase，
只完成当前原子写入并按 durable journal 尝试 rollback，然后释放 event 并退出；新 Setup 只有确认 event
已不存在后才能取得/接管 abandoned mutex 并执行 recovery。activity event 与 mutex 一样允许不可信
预创建造成 fail-closed DoS，但绝不把未知 object 当成可信 lease。

Helper 命令行只接受固定 action 枚举、NSIS 生成的 128-bit transaction ID、staging/journal 路径，以及
`commit-install` 专用的 `$INSTDIR` source 参数；不接受外部提供的 instance、Service name、pipe、SDDL、publisher、ProgramData
根或清理根。`$INSTDIR` 必须 canonicalize 后只用于拼接固定相对路径
`resources/everything/Everything.exe`、License、lock 和 INI 模板；它绝不能派生 Service binary、
ProgramData、manifest 或任何删除目标。Helper 对每个 source 使用不共享 write/delete 的 handle，先验证
final path、file identity 和类型并一直持有；Everything 从同一 handle 流式 hash/复制，路径型
Authenticode/version API 只在该 handle 阻止替换期间调用，复制到受保护 destination 后再次完整验证，
验证成功前绝不安装 Service。预先存在的不兼容 writer 使打开失败，不降级共享模式。

Helper 在编译时嵌入 reviewed lock、License 和 INI template 的精确 bytes。运行时 bundle lock 必须严格
解析且与嵌入 bytes 完全一致；Everything 使用嵌入 lock 中的 version、SHA-256 和 Authenticode publisher
验证；License 和 INI 必须与各自嵌入 bytes 完全一致。非 PE 文件不执行 version 或 Authenticode 检查。
以下 action 构成完整接口：

- `prepare-install`：只读校验既有清单、SCM 和旧 binary，或从同 session Explorer token 取得首装
  `desktop_user_sid`；把完整回滚快照写入 staging 内的严格 journal，不改变 Service、ProgramData 或
  应用目录；若发现受保护的未完成事务，只返回 `recovery-required`，不在此 action 中恢复；有效清单、
  匹配 binary 且 Service 已不存在是可恢复的 repair/uninstall 中间态，不与未知同名 Service 混同；
- `recover-install`：只在新 Install Section 已开始、Tauri 复制应用文件之前调用，根据受保护的 durable
  transaction journal 幂等恢复或确认上次 old/new Service 终态；恢复成功后重新执行 `prepare-install`，
  失败则在复制应用文件前终止并保留所有已验证恢复材料；
- `commit-install`：在 Tauri 已复制主程序和 resources 后验证 bundle Everything，按 journal 执行
  versioned staging、Service switch、Running 验证、清单原子替换和旧版本清理；
- `rollback-install`：只根据已验证 journal 和当前 live identity 幂等恢复旧 Service；未知、部分匹配或
  transaction ID/journal 不一致时保留所有已验证恢复材料并失败闭合；
- `pre-uninstall`：在 app-running gate 通过后先恢复或确认受保护的未完成安装事务，再验证并停止/移除
  private Service；有效清单精确匹配且 Service 已不存在时视为幂等成功，同名但不匹配 Service 时失败；
  `$UpdateMode == 1` 时 NSIS 不调用该 action；
- `post-uninstall`：只在 Service 已确认不存在后删除清单精确引用的受管版本目录；清单和引用目录已全部
  不存在时视为成功，部分状态只能沿清单记录继续幂等清理，不枚举扩大删除范围。它必须先删除并确认
  所有 manifest 引用目录不存在，最后才删除 runtime manifest；若 manifest 已删除而 managed root 是
  前述精确空 bootstrap residue，只删除空 root。

staging journal 使用固定 schema、拒绝未知/重复字段。`commit-install` 在第一个 ProgramData/Service
副作用前，将 old snapshot、new candidate 和 phase 原子写入受保护固定路径
`%ProgramData%/UiPilot_tools/everything/managed-transaction.json`；每个后续副作用前都先持久化并 flush
下一 phase。durable journal 只能引用固定 managed root 内的 old/new binary 和清单字段。成功验证 new
Service/manifest 后才删除它；rollback 验证 old Service/manifest 后才删除它。

首装创建 managed root 时同样使用原子 `SECURITY_ATTRIBUTES`，owner/group 固定为 Built-in
Administrators、DACL 只允许 LocalSystem 和 Built-in Administrators 完全控制，并先写同目录唯一命名
的 transaction temp 再 flush/atomic rename。只有同时满足以下条件的 bootstrap residue 可由持有全机
mutex 的 `recover-install` 清理或复用：canonical managed root 的 owner/DACL 精确匹配、无 reparse、无
Service、无 runtime manifest、无 version directory，且目录为空或只含一个符合命名约束的 transaction
temp 普通文件。任何额外内容、ACL/owner 差异或同名 Service 都按 foreign state 失败闭合。此例外只
覆盖“root 创建成功但 durable journal rename 尚未完成”的窗口，不允许接管一般的无清单目录。

Helper 自身在 `commit-install` 失败时先尝试同进程 rollback；NSIS 对非零退出再调用一次幂等
`rollback-install`，以覆盖 Helper panic/abnormal exit。若 Setup、Helper 或系统在 Service switch 中途
终止，下次可信 Setup 的 `prepare-install` 只读识别 durable journal，进入 Install Section 后先调用
`recover-install` 再复制应用文件；真正卸载则由 `pre-uninstall` 先恢复再移除 Service。journal、live SCM
或 binary identity 不能唯一证明 old/new 状态时失败闭合并保留全部材料，不接管未知对象。

Setup 只根据固定 exit code 映射稳定、无路径的用户错误；详细日志也只记录 reason code。staging 在安装
成功、rollback 完成及 uninstaller 完成后有界重试删除；删除失败不反转已验证的 Service 终态，残留仍
保持原 DACL、永不复用，也不包含搜索或结果数据。Helper 不落入安装目录。

不允许安装器执行 PowerShell、cmd 脚本、WMI 命令文本或下载 Helper。NSIS 只承担安全 staging、Helper
解包/调用、退出码分支和 hook ordering；所有受管对象的语义验证及删除范围仍由 Helper 重新校验。

安装流程：

1. `PREINSTALL` 调用 Helper `prepare-install`，读取并验证已有清单、Service name、SCM ImagePath 和旧
   版本 executable；只准备回滚
   信息，不停止或删除旧 Service。覆盖升级必须沿用清单中的 `desktop_user_sid`，不得改授本次安装
   session 或 elevated token。全新安装从当前 session 的 Explorer shell 获取 `desktop_user_sid`：
   `GetShellWindow` -> `GetWindowThreadProcessId` -> `OpenProcessToken(TOKEN_QUERY)` ->
   `GetTokenInformation(TokenUser)` -> `ConvertSidToStringSidW`，并要求 shell PID 与 installer PID 的
   `ProcessIdToSessionId` 相同。没有同 session shell、SID 无效或仅能读到 elevated token 时中止安装；
   不从环境变量、用户名或活动控制台猜测 SID。
2. 若 `prepare-install` 返回 `recovery-required`，自定义模板在 Install Section 内、复制应用文件前调用
   `recover-install`，成功后重新执行 `prepare-install`；任何恢复失败都在应用文件复制前中止。
3. Tauri 模板复制 UiPilot 主程序和 bundle resources。
4. `POSTINSTALL` 调用 Helper `commit-install`，以 canonical `$INSTDIR` 作为只读 bundle source，校验
   Everything 的固定版本、SHA-256 和
   Authenticode publisher，再将
   Service executable 复制到版本化 ProgramData 目录，并设置普通用户不可写的 ACL。
5. 使用固定命令安装 Service：
   `Everything.exe -instance "UiPilot" -install-service`
   `-install-service-pipe-name "\\.\PIPE\UiPilotEverythingService"`
   `-install-service-security-descriptor`
   `expanded_sddl`。
   覆盖升级在执行该命令前，先使用已验证的旧 executable 停止并卸载同名旧 Service；全新安装没有
   该步骤。
6. 查询 SCM，验证 Service name 精确为 `Everything (UiPilot)`；ImagePath 的 canonical executable 必须
   等于当前版本化路径，参数必须在语义上等于 `-svc`、instance、完整 pipe 和清单中的
   `expanded_sddl`。
7. 验证 Service 为 Running 后，原子替换受保护清单，再清理不再引用的旧版本目录。

Service pipe DACL 只允许：

- LocalSystem；
- Built-in Administrators；
- 首装同 session Explorer shell 的 `desktop_user_sid`。

不向 Everyone、Authenticated Users、INTERACTIVE、Network 或 Batch SID 单独授予权限。实际信任
主体是 LocalSystem、包含 Built-in Administrators enabled SID 的 elevated token，以及任何包含
`desktop_user_sid` enabled SID 的 token。访问检查不按登录类型区分同一用户，因此该用户的
Interactive、Batch 和 Network token 都属于同一信任边界；其他用户的非提升 token 不受信任。

Everything client 需要 read/write pipe access；Windows 将 pipe 的 write 与
`FILE_CREATE_PIPE_INSTANCE` 部分映射到同一权限，因此 `desktop_user_sid` 或 elevated administrator
可以创建同名 pipe instance。相同 SID 的进程能够修改该用户自己的进程、INI 和 DB，本设计不把它们
相互隔离作为安全边界；跨用户的非管理员或 UAC filtered token 创建和连接必须由 DACL 拒绝。

升级和 repair 都原位覆盖，因此在新 Install Section 开始前，旧应用文件、卸载入口、Service、清单和
ProgramData 保持原状。新 `POSTINSTALL` 暂存新版本后才停止并切换 Service。切换失败时由 Helper 删除部分创建
的新 Service，使用旧清单和旧 executable 重新安装并启动旧 Service，保留旧清单并删除新暂存版本，
然后让 Setup 失败。NSIS 此时可能已经写入新 UiPilot 文件，因此这里只承诺 Service runtime 回滚，
不承诺整个应用目录事务回滚；旧或新 uninstaller 入口始终保留，用户可以重新运行同一可信 Setup 或
执行真正卸载。

若 uninstaller 被外部工具显式传入 `/UPDATE`，`PREUNINSTALL` 不操作 Service、清单或 ProgramData；
本设计的手动替换和 Tauri 自动更新都不依赖该兼容分支调用旧 uninstaller。

真正卸载时 `$UpdateMode != 1`。自定义模板先完成 app-running gate；用户拒绝关闭 UiPilot 或取消时，
`PREUNINSTALL` 尚未执行，应用与 Service 均保持原状。通过 gate 后，`PREUNINSTALL` 调用 Helper 验证清单、Service
name、SCM ImagePath 和受管根目录，再停止并卸载 UiPilot private Service；失败则在 Tauri 删除应用文件
前中止。`POSTUNINSTALL` 只在 Service 已确认不存在时删除受管清单和版本目录。用户自己的 Everything
Service、进程、配置和数据库不在匹配范围内。若 `PREUNINSTALL` 成功后 Setup 或文件删除中断，重跑
uninstaller 时 Service 已不存在是合法中间态，继续幂等 `post-uninstall`；若旧 uninstaller 已损坏或
缺失，下一次可信 Setup 在 Install Section 内通过 durable journal/有效清单收敛该状态。任何同名但
identity 不匹配的 Service 仍失败闭合。

## 子系统 2：运行时客户端 Supervisor

新增 `EverythingRuntimeSupervisor`，在 Tauri production setup 中异步启动。主窗口和托盘初始化不等待
Everything；`/find` 在 runtime Ready 前显示稳定的“文件搜索正在准备”或“文件搜索服务不可用”状态。

Supervisor 启动步骤：

1. 通过 Tauri Resource 目录解析 packaged `Everything.exe`，不搜索 `PATH` 或注册表。
2. 验证资源 lock、SHA-256 和 Authenticode publisher。
3. 取得当前 session 的 `Local\UiPilotEverythingRuntime` mutex。拿不到 mutex 时进入 Unavailable；同一
   session 只有一个 owner 可以检查、配置和启动 private instance。
4. 枚举 `EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)` 窗口并读取 HWND 所属 PID。若存在遗留实例，
   只有 canonical executable、hash 和签名都匹配受管资源时才先发送 graceful quit 并有界等待，超时后
   终止精确 PID；身份不匹配时失败闭合。
5. 在确认 private instance 已退出后，创建当前用户、当前 session 专属配置和数据库目录，并在每次启动
   前从只读模板原子生成受管 INI；数据库保留，但 instance、pipe、网络入口、更新和 UI 行为不沿用
   用户修改值。session id 是目录组件前先解析为十进制 `u32`，不接受外部路径文本；同一账户的并发
   console/RDP session 不共享 DB 文件。
6. 先创建启用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Windows Job Object，再使用
   `STARTUPINFOEX` 的 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 把该 Job 作为创建时属性传给
   `CreateProcessW(CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT)`。客户端参数固定为 private
   instance、完整 Service pipe、INI、DB 和 `-startup`。Job attribute 初始化或进程创建失败时没有可运行
   child，并进入 Unavailable；成功返回时 child 已原子属于 Job，随后才恢复主线程。不得采用
   `CreateProcessW` 返回后再 `AssignProcessToJobObject` 的两阶段顺序。
7. `EverythingClient::connect_owned("UiPilot", expected_pid, ...)` 枚举同名 IPC 窗口，并用
   `GetWindowThreadProcessId` 只接受 PID 等于 child PID 的 HWND；每次重连都重复验证，不使用启动前检查
   代替连接时身份绑定。
8. 在 60 秒异步启动期限内验证 Service Running、owned IPC 可达、DB loaded，以及 Query2 probe 返回
   固定 request flags 和实际 date-modified sort。空索引是合法 Ready，`total` 只记录为诊断值。

固定 INI 禁用托盘图标、更新提示、自更新、HTTP、ETP、FTP、全局快捷键和非 UiPilot 所需的网络入口。
客户端以当前用户普通权限运行，不从应用内触发 UAC。

运行期间若客户端意外退出，Supervisor 清除 cached IPC client 和内部 client generation，最多执行 3 次
有界退避重启；超过预算后进入 Unavailable，
不无限拉起。Service 缺失、停止、pipe 不匹配、资源校验失败或未知进程占用均直接进入 Unavailable，
并提示用户重新运行可信安装包，不尝试应用内修复 Service。

UiPilot 正常进程退出时显式终止并等待自己持有的客户端；隐藏主窗口或关闭 `/find` 窗口不触发终止。
崩溃或强制退出时由 Job Object 自动清理。Service 保持运行。启动过但未被当前 Supervisor 拥有的
进程不能仅按进程名终止。

生产 `/find` 只使用 Supervisor 提供且已绑定 owned PID 的 client，不再连接默认实例。Runtime 未 Ready
时不得缓存无效 IPC client；内部 client generation 变化时清除旧 client，但该 generation 不进入
ResultRegistry 或文件 action。

## 状态与错误行为

Runtime 对外状态固定为：

- `Starting`：客户端启动、索引 probe、有界退避或重启尚未完成；不可执行搜索。
- `Ready`：Service、owned PID、DB loaded 和固定 Query2 capability probe 成功；允许 `/find`，索引可以
  合法为空。
- `Unavailable(reason)`：启动校验失败或重启预算耗尽；除重新安装可修复的缺失项外，本次进程不再
  自动重试。
- `Stopping`：UiPilot 正在退出；拒绝新查询。

任何 Everything 故障不得阻止 UiPilot 启动，也不得影响计算、插件、快捷链接或设置。错误信息不暴露
本地路径、pipe 名、SID 或原始系统错误；诊断日志可记录稳定 reason code，但不得记录搜索内容和结果
路径。

## 子系统 3：验证与发布门禁

自动化检查包括：

- Source gate：per-machine、受控 Tauri 2.11.4 template、installer hooks、替换安装原位覆盖、明确卸载
  后退出、资源、固定 Service/instance/完整 pipe/SDDL 模板、同 session shell SID 提取与升级保留、
  卸载 app-running gate 位于 `PREUNINSTALL` 之前、Helper release build/嵌入、原子安全 staging、固定 action
  协议、全机 mutex/activity-event/transaction ID、父 PID+creation-time lease、跨 hook staging 生命周期、`asInvoker`/high-integrity/owner/DACL/
  reparse gate、禁止脚本解释器以及禁用网络与更新配置全部存在。
- Installer Helper unit/source tests：严格 journal/清单解析、路径规范化、清单匹配、SCM ImagePath 参数、
  Service ownership、同 session shell SID、SDDL/ACL、自动更新、手动替换安装、幂等 Service-only rollback、
  bootstrap residue、durable phase recovery、operation/recovered-transaction ID 交接、mutex owner/DACL/
  type 验证与并发拒绝、activity lease/父进程退出顺序、句柄固定 source 复制、真正
  卸载 manifest-last 幂等续跑、拒绝非受管路径、标准用户直接调用 Helper 失败和稳定退出码映射。
- Runtime unit tests：异步启动、session mutex、只启动一次、owned HWND/PID、空索引 Ready、重启预算、
  `PROC_THREAD_ATTRIBUTE_JOB_LIST` 创建时所有权、未知进程拒绝和 shutdown ordering。
- Backend tests：生产只使用 Supervisor 提供的 owned client，runtime 未 Ready 时返回稳定 unavailable；
  client 重启后旧结果在下一次查询 admission 前仍通过现有路径认证执行；`begin_query` 立即退役旧集合，
  后端失败时保持为空，成功时发布新 ResultSet。
- 现有 `/find` Query2、ResultRegistry、文件执行和前端测试继续通过。

Windows 人机/VM 矩阵至少覆盖：

1. 干净 Windows 管理员账户首装，UAC 后 `/find` 可查询。
2. 标准用户启动安装器、输入另一管理员凭据，安装完成后原标准用户可查询；原标准用户在 Helper
   staging 存活期间不能读取、替换或删除 Helper/journal，且直接解出 Helper 以普通权限调用时在读取
   request 前失败。结束后 Helper 不存在于安装目录；正常删除成功，注入 AV/handle 占用导致删除失败时
   残留仍保持受保护、不可复用且不影响已验证 Service 终态。
3. 用户默认 Everything 同时运行，UiPilot 安装、搜索、退出和卸载均不影响它。
4. Tauri 自动更新和手动运行新版 Setup 两条覆盖升级路径均不调用旧 uninstaller；在进入 Install
   Section 前取消或注入失败时，旧应用、卸载入口、Service、清单和 ProgramData 均保持不变。进入
   Install Section 后 UiPilot 和私有客户端正在运行时不出现 executable write error，旧 Service 在
   commit 前保持可回滚。另在原标准用户可写的自定义 `$INSTDIR` 中并发替换 Everything source；Helper
   必须因 share violation 失败或从持续持有的同一 file identity 复制，受保护目标不得出现替换 bytes。
5. Service/客户端被终止后的有界恢复与明确 unavailable 状态。
6. 卸载后 private Service、ProgramData runtime 和受管清单删除，用户 Everything 保留。
7. 首装 `desktop_user_sid` 的 console/RDP session 可并发运行且各自客户端、INI 和 DB 不冲突；第二个
   本地非管理员用户和第二管理员的 UAC filtered token 不能连接 Service，也不能仅通过运行 Setup 覆写
   清单中的授权 SID；第二管理员的 elevated token 按明确的 administrator trust boundary 可以连接。
8. 在停止旧 Service 后、安装新 Service 后、启动新 Service 前后分别注入失败，验证旧 Service 回滚；
   回滚自身失败时保留旧清单和所有已验证 binary，并输出可诊断失败，不进行破坏性清理。
   在相同 phase 分别强制终止 Helper、终止 Setup 和模拟系统重启后，再运行可信 Setup，必须先依据
   durable journal 收敛到唯一可验证 old/new 状态再复制应用文件；另在 managed root 创建后、transaction
   temp 创建/flush/rename 各边界终止，只有精确 bootstrap residue 可恢复，不一致状态继续失败闭合。
   重跑时改用另一 elevated administrator 凭据，新的 operation ID 必须只通过受保护 staging 中的
   recovered transaction ID 接管旧 durable journal。
9. UiPilot 在 Job 创建、attribute list 初始化、`CreateProcessW` 返回、resume、ready probe、注销和快速
   关机各阶段退出时，不留当前会话客户端进程；注入 CreateProcess 失败时不存在 child，成功返回时
   child 已在 Job 中。
10. UiPilot 正在运行时启动真正卸载并在 app-running gate 取消，验证主程序、Service、清单和
    ProgramData runtime 均未改变；确认关闭后再次卸载才执行 Service 清理。另在 `pre-uninstall` 删除
    Service 后、每个引用目录删除边界和 manifest 删除边界分别终止 Setup，重跑卸载必须按
    manifest-last 顺序幂等完成；同名 foreign Service 仍不得作为“已删除”处理。
11. 安装完成后通过 `GetSecurityInfo` 读取 live pipe DACL 并规范化为清单中展开后的 SDDL。对该 security
    descriptor 分别用 `desktop_user_sid` 的 Interactive、Batch、`LOGON32_LOGON_NETWORK` token、
    LocalSystem token 和第二管理员的 elevated token 执行 Windows `AccessCheck`，read/write 均成功。
    第二个非管理员用户的 Interactive/Batch token、NetworkService token 和第二管理员的 UAC filtered
    token 对 read/write/create-instance 的 `AccessCheck` 均失败，实际创建同名 pipe instance 和本地连接
    也得到 AccessDenied。远程端到端连接可被 Everything 自身额外拒绝，单独记录结果，不把它误作 DACL
    登录类型证明；任何带 `desktop_user_sid` 的 token 均不作为负向断言。
12. 分别预置同名 `Everything (UiPilot)` Service、受管 ProgramData 目录、缺失清单和签名/字段无效清单；
    Setup 必须在修改 Service、目录或文件前失败，测试后原始 hash、SCM ImagePath 和运行状态完全不变。
13. 手动升级、降级和 repair 的 reinstall page 不启动旧 uninstaller；在目录页取消、Install Section 前
    失败和主文件复制前失败时，旧卸载入口仍有效且 private Service/清单/ProgramData 未改变。同版本
    明确选择“卸载应用”时，旧 uninstaller 成功后当前 Setup 立即退出，不进入安装流程。并发启动两个
    Setup，或 Setup 与真正 uninstaller 并发时，只允许一个持有全机 mutex；第二个在读取或修改受管状态
    前失败，abandoned owner 只能先完成 durable recovery 后继续。分别由标准用户预建、持有、abandon
    同名 mutex/activity event 和在运行中篡改 DACL；Setup 必须在读取受管状态前拒绝不匹配
    owner/DACL/type，且不把 fail-closed DoS 当作可恢复的可信锁。另在 `commit-install` Helper 仍运行时
    强制终止父 Setup 并立即启动另一 Setup：旧 Helper 持有的 activity event 必须阻止 recovery；旧
    Helper 完成 rollback/退出并释放 event 后，新 Setup 才可根据 durable journal 恢复。

## 实施拆分

1. **Installer Service task**：原生 Rust Installer Helper 及 pre-bundle build、受控 NSIS template、hooks、
   原子安全 Helper staging、固定 action/exit-code 协议、版本化 Service staging、受保护清单、INI 模板、
   更新/卸载分支、Service-only rollback 及 Source gate。
2. **Runtime Supervisor task**：Rust process abstraction、session mutex、`PROC_THREAD_ATTRIBUTE_JOB_LIST`
   创建时 Job 所有权、owned-PID IPC、DB/capability health、restart/shutdown、private instance connection
   和状态映射。
3. **Release verification task**：自动化测试、NSIS build、标准用户/UAC/升级/共存/卸载 VM 矩阵。

三个任务按顺序交付；前一个任务的验证结果是后一个任务的输入。Service 安装验证未通过时，不把仅能
启动客户端的中间版本作为可发布安装包。

## 非目标

- 不升级到 Everything 1.5 Beta。
- 不复用默认 Everything instance 或默认 Service。
- 不允许 WebView 提交 instance、pipe、INI、DB 或启动参数。
- 不在应用内安装、修复、启动或停止 Windows Service，也不产生第二次 UAC。
- 不改变 `/find` 的查询语义、结果协议或文件执行安全边界。

## 参考

- [Everything command-line options](https://www.voidtools.com/support/everything/command_line_options/)
- [Everything multiple instances](https://www.voidtools.com/support/everything/multiple_instances/)
- [Everything Service](https://www.voidtools.com/support/everything/everything_service/)
- [Tauri CLI 2.11.4 NSIS template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)
- `docs/superpowers/specs/2026-07-23-find-everything-managed-migration-design.md`
