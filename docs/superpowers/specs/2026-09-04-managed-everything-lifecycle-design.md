# UiPilot 托管 Everything 生命周期设计

## 状态

- 日期：2026-09-04
- 状态：用户已批准架构，等待书面规格复核
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

本设计只补齐安装与生产生命周期。现有 Query2 编解码、文件结果协议、ResultRegistry 和路径认证
边界保持不变。

## 已确认决策

1. 使用 UiPilot 专用 Service、named instance、pipe、配置和数据库。
2. 安装器为 per-machine，并在安装时请求一次 UAC。
3. 标准用户运行 UiPilot 时必须可用，即使安装时输入的是另一个管理员账户的凭据。
4. Service 随 Windows 启动，UiPilot 退出时不停止 Service。
5. UiPilot 启动隐藏客户端；退出时只终止自己拥有的客户端进程。
6. 不探测、复用、修改、关闭或卸载用户自己的默认 Everything。
7. Everything 启动和建库不得阻塞 UiPilot 主界面显示。

此处第 3 项取代
`2026-07-23-find-everything-managed-migration-design.md` 中仅授权首装 credential-owner SID 的
限制。其余既有安全边界继续有效。

## 固定身份与路径

- Everything instance name：`UiPilot`
- Everything service pipe name：`UiPilotEverythingService`
- 安装资源：`<UiPilot install>/resources/everything/Everything.exe`
- 受保护 Service runtime：
  `%ProgramData%/UiPilot_tools/everything/<version>/Everything.exe`
- 受保护安装清单：`%ProgramData%/UiPilot_tools/everything/managed-runtime.json`
- 每用户配置目录：`%LOCALAPPDATA%/UiPilot_tools/everything/`
- 每用户配置：`Everything-UiPilot.ini`
- 每用户数据库：`Everything-UiPilot.db`

受保护清单固定版本、架构、SHA-256、Authenticode publisher、instance、pipe 和 Service binary
路径。升级、修复和卸载只能操作与清单及固定根目录同时匹配的对象。

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
  └─ EverythingClient::connect("UiPilot")
       └─ private client -> UiPilotEverythingService pipe -> Service
```

## 子系统 1：NSIS Service 生命周期

Tauri 配置继续使用 `installMode: "perMachine"`，并新增
`windows.nsis.installerHooks`，指向 `src-tauri/windows/nsis-hooks.nsh`。

安装流程：

1. 校验 bundle 中 Everything 的固定版本、SHA-256 和 Authenticode publisher。
2. 将 Service executable 复制到版本化 ProgramData 目录，并设置普通用户不可写的 ACL。
3. 用固定 instance、pipe 和 pipe security descriptor 安装 Service；安装失败则安装整体失败。
4. 启动 Service，验证 Service 状态和实际 binary path。
5. 原子写入只读受保护清单。

Service pipe DACL 只允许：

- LocalSystem；
- Built-in Administrators；
- Windows `INTERACTIVE` well-known SID。

不授权 Everyone、Authenticated Users、Network、Batch 或任意安装凭据账户 SID。这样通过另一管理员
凭据完成 UAC 的标准桌面用户仍可使用 `/find`，非交互式本地账户和网络身份不能连接。接受的产品
权衡是：同一台电脑上任何交互式登录用户都能通过该 Service 枚举 NTFS 文件名，但仍不能借此读取
文件内容。

升级采用版本化 Service binary：先校验并暂存新版本，再停止并切换 Service。切换失败时恢复上一版本
Service；确认新 Service running 后才清理旧版本。这样不会尝试覆盖正在运行的 Service executable。

卸载前验证清单、Service 名称、binary path 和受管根目录；只停止并卸载 UiPilot private Service，随后
删除受管清单和版本目录。用户自己的 Everything Service、进程、配置和数据库不在匹配范围内。

## 子系统 2：运行时客户端 Supervisor

新增 `EverythingRuntimeSupervisor`，在 Tauri production setup 中异步启动。主窗口和托盘初始化不等待
Everything；`/find` 在 runtime Ready 前显示稳定的“文件搜索正在准备”或“文件搜索服务不可用”状态。

Supervisor 启动步骤：

1. 通过 Tauri Resource 目录解析 packaged `Everything.exe`，不搜索 `PATH` 或注册表。
2. 验证资源 lock、SHA-256 和 Authenticode publisher。
3. 创建当前用户专属配置和数据库目录，并在每次启动前从只读模板原子生成受管 INI；数据库保留，
   但会影响 instance、pipe、网络入口、更新或 UI 行为的配置不得沿用用户修改值。
4. 确认没有未知 executable 占用 `UiPilot` named instance。只可清理路径和签名都匹配受管资源的遗留
   私有实例；身份不匹配时失败闭合。
5. 使用固定参数启动后台客户端：private instance、private Service pipe、INI 和 DB 路径、`-startup`。
6. 把子进程加入启用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的 Windows Job Object。
7. 在有界启动期限内，通过 Query2 probe 验证 private instance 可连接且索引非空，然后发布 Ready。

固定 INI 禁用托盘图标、更新提示、自更新、HTTP、ETP、FTP、全局快捷键和非 UiPilot 所需的网络入口。
客户端以当前用户普通权限运行，不从应用内触发 UAC。

运行期间若客户端意外退出，Supervisor 最多执行 3 次有界退避重启；超过预算后进入 Unavailable，
不无限拉起。Service 缺失、停止、pipe 不匹配、资源校验失败或未知进程占用均直接进入 Unavailable，
并提示用户重新运行可信安装包，不尝试应用内修复 Service。

UiPilot 正常进程退出时显式终止并等待自己持有的客户端；隐藏主窗口或关闭 `/find` 窗口不触发终止。
崩溃或强制退出时由 Job Object 自动清理。Service 保持运行。启动过但未被当前 Supervisor 拥有的
进程不能仅按进程名终止。

生产 `/find` 改为连接 `EverythingClient::connect("UiPilot", ...)`，不再连接默认实例。Runtime 未 Ready
时不得缓存无效 IPC client；Runtime generation 变化时清除旧 client。

## 状态与错误行为

Runtime 对外状态固定为：

- `Starting`：客户端启动、索引 probe、有界退避或重启尚未完成；不可执行搜索。
- `Ready`：private IPC probe 成功；允许 `/find`。
- `Unavailable(reason)`：启动校验失败或重启预算耗尽；除重新安装可修复的缺失项外，本次进程不再
  自动重试。
- `Stopping`：UiPilot 正在退出；拒绝新查询。

任何 Everything 故障不得阻止 UiPilot 启动，也不得影响计算、插件、快捷链接或设置。错误信息不暴露
本地路径、pipe 名、SID 或原始系统错误；诊断日志可记录稳定 reason code，但不得记录搜索内容和结果
路径。

## 子系统 3：验证与发布门禁

自动化检查包括：

- Source gate：per-machine、installer hooks、资源、固定 instance/pipe、禁用网络与更新配置全部存在。
- Installer unit/source tests：路径规范化、清单匹配、Service ownership、升级回滚和卸载拒绝非受管路径。
- Runtime unit tests：异步启动、只启动一次、Ready probe、重启预算、Job Object 清理、未知进程拒绝和
  shutdown ordering。
- Backend tests：生产连接使用 `UiPilot` instance，runtime 未 Ready 时返回稳定 unavailable。
- 现有 `/find` Query2、ResultRegistry、文件执行和前端测试继续通过。

Windows 人机/VM 矩阵至少覆盖：

1. 干净 Windows 管理员账户首装，UAC 后 `/find` 可查询。
2. 标准用户启动安装器、输入另一管理员凭据，安装完成后原标准用户可查询。
3. 用户默认 Everything 同时运行，UiPilot 安装、搜索、退出和卸载均不影响它。
4. 覆盖升级时 UiPilot 和私有客户端正在运行，不出现 executable write error。
5. Service/客户端被终止后的有界恢复与明确 unavailable 状态。
6. 卸载后 private Service、ProgramData runtime 和受管清单删除，用户 Everything 保留。

## 实施拆分

1. **Installer Service task**：NSIS hooks、版本化 Service staging、受保护清单、INI 模板及 Source gate。
2. **Runtime Supervisor task**：Rust process abstraction、Job Object、health/restart/shutdown、private instance
   connection 和状态映射。
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
- `docs/superpowers/specs/2026-07-23-find-everything-managed-migration-design.md`
