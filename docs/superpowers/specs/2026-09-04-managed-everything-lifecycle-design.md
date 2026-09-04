# UiPilot 托管 Everything 生命周期设计

## 状态

- 日期：2026-09-04
- 状态：用户已批准架构，第八轮独立审核无实质问题
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
- 每用户每会话配置目录：`%LOCALAPPDATA%/UiPilot_tools/everything/<session-id>/`
- 每用户配置：`Everything-UiPilot.ini`
- 每用户数据库：`Everything-UiPilot.db`

受保护清单固定版本、架构、SHA-256、Authenticode publisher、instance、Service name、完整 pipe、
`desktop_user_sid`、展开后的 SDDL 和 Service binary 路径。升级、修复和卸载只能操作与清单及固定根
目录同时匹配的对象。没有有效清单时若同名 Service 或受管 ProgramData 目录已经存在，安装失败闭合，
不接管、不覆盖也不删除该状态。

`${desktop_user_sid}` 是本文的运行时替换记号，不是 NSIS compile-time define。首装取得 canonical SID
字符串后构造 `expanded_sddl`，先用 Windows security descriptor API 解析并核对 3 个 ACE，再传给
Everything；清单只保存 canonical SID 与完整 `expanded_sddl`。

## 总体架构

```text
NSIS per-machine installer (elevated)
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

`installer.nsi` 固定派生自 Tauri CLI 2.11.4 官方模板，只允许两个受控行为区域发生差异：

1. NSIS reinstall page 不允许“先卸载旧版再安装”的替换路径。升级、允许的降级以及同版本 repair 都
   直接进入原位覆盖，绝不调用旧 uninstaller；Tauri 自动更新的 `/UPDATE` 路径同样原位覆盖。只有同
   版本维护页明确选择“卸载应用”时才调用旧 uninstaller，调用时不带 `/UPDATE`，成功后立即退出当前
   Setup，不进入目录页或 Install Section。
2. Uninstall Section 先执行 Tauri 的 `CheckIfAppIsRunning`，确认 UiPilot 已退出且用户没有取消，再执行
   `NSIS_HOOK_PREUNINSTALL`。原模板中两者的顺序反转，不能用于有外部 Service 状态的卸载事务。

Source gate 对模板来源版本、这两个行为区域做快照校验：替换安装不得执行旧 uninstaller，明确卸载
必须在旧 uninstaller 成功后退出，app-running gate 必须位于 `PREUNINSTALL` 前。任何其他模板差异都
使检查失败。升级 Tauri CLI 时必须显式重基模板。

安装流程：

1. `PREINSTALL` 读取并验证已有清单、Service name、SCM ImagePath 和旧版本 executable；只准备回滚
   信息，不停止或删除旧 Service。覆盖升级必须沿用清单中的 `desktop_user_sid`，不得改授本次安装
   session 或 elevated token。全新安装从当前 session 的 Explorer shell 获取 `desktop_user_sid`：
   `GetShellWindow` -> `GetWindowThreadProcessId` -> `OpenProcessToken(TOKEN_QUERY)` ->
   `GetTokenInformation(TokenUser)` -> `ConvertSidToStringSidW`，并要求 shell PID 与 installer PID 的
   `ProcessIdToSessionId` 相同。没有同 session shell、SID 无效或仅能读到 elevated token 时中止安装；
   不从环境变量、用户名或活动控制台猜测 SID。
2. Tauri 模板复制 UiPilot 主程序和 bundle resources。
3. `POSTINSTALL` 校验 bundle 中 Everything 的固定版本、SHA-256 和 Authenticode publisher，再将
   Service executable 复制到版本化 ProgramData 目录，并设置普通用户不可写的 ACL。
4. 使用固定命令安装 Service：
   `Everything.exe -instance "UiPilot" -install-service`
   `-install-service-pipe-name "\\.\PIPE\UiPilotEverythingService"`
   `-install-service-security-descriptor`
   `expanded_sddl`。
   覆盖升级在执行该命令前，先使用已验证的旧 executable 停止并卸载同名旧 Service；全新安装没有
   该步骤。
5. 查询 SCM，验证 Service name 精确为 `Everything (UiPilot)`；ImagePath 的 canonical executable 必须
   等于当前版本化路径，参数必须在语义上等于 `-svc`、instance、完整 pipe 和清单中的
   `expanded_sddl`。
6. 验证 Service 为 Running 后，原子替换受保护清单，再清理不再引用的旧版本目录。

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
ProgramData 保持原状。新 `POSTINSTALL` 暂存新版本后才停止并切换 Service。切换失败时删除部分创建
的新 Service，使用旧清单和旧 executable 重新安装并启动旧 Service，保留旧清单并删除新暂存版本，
然后让 Setup 失败。NSIS 此时可能已经写入新 UiPilot 文件，因此这里只承诺 Service runtime 回滚，
不承诺整个应用目录事务回滚；旧或新 uninstaller 入口始终保留，用户可以重新运行同一可信 Setup 或
执行真正卸载。

若 uninstaller 被外部工具显式传入 `/UPDATE`，`PREUNINSTALL` 不操作 Service、清单或 ProgramData；
本设计的手动替换和 Tauri 自动更新都不依赖该兼容分支调用旧 uninstaller。

真正卸载时 `$UpdateMode != 1`。自定义模板先完成 app-running gate；用户拒绝关闭 UiPilot 或取消时，
`PREUNINSTALL` 尚未执行，应用与 Service 均保持原状。通过 gate 后，`PREUNINSTALL` 验证清单、Service
name、SCM ImagePath 和受管根目录，再停止并卸载 UiPilot private Service；失败则在 Tauri 删除应用文件
前中止。`POSTUNINSTALL` 只在 Service 已确认不存在时删除受管清单和版本目录。用户自己的 Everything
Service、进程、配置和数据库不在匹配范围内。

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
  卸载 app-running gate 位于 `PREUNINSTALL` 之前、禁用网络与更新配置全部存在。
- Installer unit/source tests：路径规范化、清单匹配、SCM ImagePath 参数、Service ownership、自动更新、
  手动替换安装、Service-only 回滚、真正卸载和拒绝非受管路径。
- Runtime unit tests：异步启动、session mutex、只启动一次、owned HWND/PID、空索引 Ready、重启预算、
  `PROC_THREAD_ATTRIBUTE_JOB_LIST` 创建时所有权、未知进程拒绝和 shutdown ordering。
- Backend tests：生产只使用 Supervisor 提供的 owned client，runtime 未 Ready 时返回稳定 unavailable；
  client 重启后旧结果在下一次查询 admission 前仍通过现有路径认证执行；`begin_query` 立即退役旧集合，
  后端失败时保持为空，成功时发布新 ResultSet。
- 现有 `/find` Query2、ResultRegistry、文件执行和前端测试继续通过。

Windows 人机/VM 矩阵至少覆盖：

1. 干净 Windows 管理员账户首装，UAC 后 `/find` 可查询。
2. 标准用户启动安装器、输入另一管理员凭据，安装完成后原标准用户可查询。
3. 用户默认 Everything 同时运行，UiPilot 安装、搜索、退出和卸载均不影响它。
4. Tauri 自动更新和手动运行新版 Setup 两条覆盖升级路径均不调用旧 uninstaller；在进入 Install
   Section 前取消或注入失败时，旧应用、卸载入口、Service、清单和 ProgramData 均保持不变。进入
   Install Section 后 UiPilot 和私有客户端正在运行时不出现 executable write error，旧 Service 在
   commit 前保持可回滚。
5. Service/客户端被终止后的有界恢复与明确 unavailable 状态。
6. 卸载后 private Service、ProgramData runtime 和受管清单删除，用户 Everything 保留。
7. 首装 `desktop_user_sid` 的 console/RDP session 可并发运行且各自客户端、INI 和 DB 不冲突；第二个
   本地非管理员用户和第二管理员的 UAC filtered token 不能连接 Service，也不能仅通过运行 Setup 覆写
   清单中的授权 SID；第二管理员的 elevated token 按明确的 administrator trust boundary 可以连接。
8. 在停止旧 Service 后、安装新 Service 后、启动新 Service 前后分别注入失败，验证旧 Service 回滚；
   回滚自身失败时保留旧清单和所有已验证 binary，并输出可诊断失败，不进行破坏性清理。
9. UiPilot 在 Job 创建、attribute list 初始化、`CreateProcessW` 返回、resume、ready probe、注销和快速
   关机各阶段退出时，不留当前会话客户端进程；注入 CreateProcess 失败时不存在 child，成功返回时
   child 已在 Job 中。
10. UiPilot 正在运行时启动真正卸载并在 app-running gate 取消，验证主程序、Service、清单和
    ProgramData runtime 均未改变；确认关闭后再次卸载才执行 Service 清理。
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
    明确选择“卸载应用”时，旧 uninstaller 成功后当前 Setup 立即退出，不进入安装流程。

## 实施拆分

1. **Installer Service task**：受控 NSIS template、hooks、版本化 Service staging、受保护清单、INI 模板、
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
