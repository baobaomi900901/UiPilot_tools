# `/find` 托管 Everything 迁移设计

## 状态

- 日期：2026-07-23
- 设计方向：用户已确认
- 实现状态：No-Go；必须先完成 IPC 与安装器 Spike
- 目标平台：Windows x64
- 基线候选：Everything 1.4.1.1032 x64 Stable；Iteration 0 校验哈希后冻结，不跟随 1.5 Beta

## 执行摘要

该迁移可行，并且相较继续完善 UiPilot 自研 Rust 文件索引，能显著缩小索引构建、USN 监听、SQLite
一致性、恢复和生命周期协调的自有维护面。

推荐方案是：UiPilot 安装器以 per-machine 模式请求一次 UAC，在安装阶段部署并维护一个 UiPilot
专用 Everything Service；UiPilot 运行时启动一个当前用户会话内的隐藏 Everything 客户端实例，并由
Rust 通过 Everything Query2 IPC 查询该实例。`/find` 的 React 交互、Tauri command、ResultRegistry
令牌和文件执行安全边界继续保留。

该方案不是“只把 SQLite 查询换成 SDK 调用”。它引入了一个新的受管运行时，包含安装器、Windows
Service、用户态客户端、IPC 消息线程、版本锁定、升级回滚和健康检查。因此必须用分阶段 Spike 验证，
再删除现有自研索引。

## 当前实现评估

### 可保留资产

- `/find` 模式进入、退出和 query sequence 的前端所有权。
- 文件类别、修改时间升降序、最多展示 200 项和元数据预览。
- `search_files` Tauri command 及 `FileSearchResponse` 的前后端协议形状。
- ResultRegistry 生成的不透明 `resultId`，以及 WebView 不提交任意路径的执行边界。
- 文件夹打开、文件定位、过期请求拒绝和 launcher 生命周期规则。
- 现有 React/Vitest 的键盘、鼠标、可访问性、刷新和 stale response 覆盖。

### 应替换资产

`src-tauri/src/file_index/mod.rs` 已超过 8,000 行，并同时承担：

- 固定卷发现和身份校验。
- 首次目录扫描与流式入库。
- USN/文件系统变更监听及扫描到监听的无损交接。
- SQLite schema、事务、恢复、完整性检查和 clean-close 标记。
- 搜索 admission、runtime epoch、暂停、退出和故障状态机。
- 路径执行前的索引记录重新解析。

这些能力中绝大部分与 `/find` 的用户价值无关，却构成当前主要开发风险。迁移后不应保留第二套后台
索引作为长期 fallback，否则安装、测试和故障组合会加倍。

## 已确认产品决策

1. UiPilot 随安装包捆绑 Everything，而不是要求用户预装。
2. 安装器请求管理员权限，并安装 UiPilot 专用 Everything Service。
3. Service 可以随 Windows 启动，UiPilot 退出时不停止 Service。
4. UiPilot 只管理自己的 Everything 实例、Service、pipe、配置和数据目录。
5. 不探测、复用、修改、停止或卸载用户自己安装的 Everything。
6. per-machine 安装器的 UAC 被拒绝时，整次安装取消，不能承诺主程序继续安装；已有安装保持不变。
7. 首版继续只搜索名称和元数据，不提供内容搜索，也不直接暴露 Everything 高级查询语法。

## 方案比较

### 方案 A：Everything SDK DLL FFI

Rust 动态加载 `Everything64.dll`，调用官方查询和结果读取函数。

优点：官方 API 简单，Spike 快，排序和 metadata request flags 已封装，适合做协议对照。

缺点：SDK API 使用进程级共享查询状态，需要强制单线程串行化；同时增加 DLL 位数、导出符号、动态
加载失败和再分发检查，异步超时与实例隔离控制也弱于直接 IPC。

结论：适合作为 Spike 对照，不作为生产首选。

### 方案 B：Rust 原生 Everything Query2 IPC

Rust 创建专用消息线程和隐藏 HWND，通过 `WM_COPYDATA` 发送 Query2 请求，接收 result-list reply，
解析名称、完整路径、大小、修改时间和 attributes。

优点：不需要 SDK DLL；query id、超时、stale reply、进程重启和实例选择均由 UiPilot 控制；与现有
query sequence、ResultRegistry 和生命周期模型更容易明确绑定。

缺点：需要正确实现 C ABI 数据结构、UTF-16、对齐、消息循环和 HWND 生命周期，并建立真实进程集成测试。

结论：推荐的生产方案。

### 方案 C：Everything CLI/ES.exe 子进程

每次查询启动 CLI 并解析文本或 CSV 输出。实现最简单，但进程启动、转义、编码、取消、总数、metadata
和错误分类均较差。

结论：拒绝，不进入生产 Spike。

## 推荐架构

```text
NSIS per-machine Installer (elevated once)
  ├─ Program Files/UiPilot/runtime/everything/Everything.exe
  ├─ install/upgrade/remove UiPilot Everything Service
  └─ protected manifest + version + checksum

UiPilot.exe (current user)
  ├─ EverythingRuntimeSupervisor
  ├─ EverythingIpcWorker (dedicated Win32 message thread)
  ├─ FileSearchRevisionTracker (response fingerprint)
  ├─ FileSearchService
  └─ ResultRegistry

Everything private client (current user, hidden)
  └─ UiPilot private service pipe
       └─ Everything Service (LocalSystem)
            └─ filesystem metadata / NTFS USN index
```

### `EverythingRuntimeSupervisor`

- 只寻找安装清单声明的受管二进制，不搜索 `PATH` 或注册表中的其他 Everything。
- 使用固定 instance name、service pipe name、INI 路径、配置目录和数据库目录；所有路径都来自受保护的
  manifest，不使用 Everything 默认 AppData 或用户已有配置。
- UiPilot 启动后异步预热客户端；不得阻塞 launcher 主窗口出现。
- 启动参数必须落实 private instance、private pipe、后台无托盘运行、受管 INI 和固定 DB 路径。
- 进入 `Ready` 前执行 capability probe：DB loaded、所需 request flags、实际 `sort_type`、date-modified
  fast sort、网络服务关闭和版本均必须满足固定契约。
- 监控客户端进程和 IPC window；意外退出时按有上限的退避策略重启。
- UiPilot 正常退出时终止自己的用户态客户端，不停止 Windows Service。
- 连续失败后进入 `Unavailable`，不无限拉起或弹 UAC。

### `EverythingIpcWorker`

- 独占一个 OS 线程、隐藏窗口和 Win32 message loop。
- 外部只暴露 typed Rust request/response，不暴露 HWND、裸指针或 Everything query syntax。
- 首版串行执行搜索；队列只保留最新未发送请求。
- query spec 本身不携带 HWND。window 层在编码 Query2 时注入 `reply_hwnd` 和唯一
  `reply_copydata_message`；后者作为 pending request id。
- `WM_COPYDATA` envelope 层先验证 source HWND 等于当前受管 Everything IPC HWND，再从
  `COPYDATASTRUCT.dwData` 关联 pending request；LIST2 payload parser 不生成或猜测 query id。
- LIST2 parser 返回并验证实际 `request_flags` 与 `sort_type`，然后解析长度、字段 offset 和 UTF-16 边界。
- 超时或客户端重启后旧 query id 全部失效；迟到 reply 只丢弃。

### `FileSearchRevisionTracker`

- Everything 1.4 Query2 没有供 UiPilot 复用的全局 DB-change 通知，不能只在客户端重启时更新 revision。
- `launcher-core.ts` 将现有 building poll 扩展为：只要 launcher 处于 `/find`，每 1 秒执行一次有 deadline
  的 `refresh_files`。该 command 使用 preserve-current refresh transaction，不能调用会立即清空 current 的
  普通 `begin_query`；隐藏 launcher 或退出 `/find` 时现有 timer ownership 立即停止轮询。
- backend 生成版本化 canonical fingerprint。preimage 固定包含 query/category/sort、status、total，以及
  每个有序可见项的 canonical path、逐组件 identity chain、leaf volume serial/file id、kind、size 和
  modified FILETIME，全部使用长度前缀 little-endian 编码。明确排除 query sequence、requestId、resultId、
  Query2 reply id 和内存地址。
- 同路径、同展示 metadata 但 file id 或任一路径组件 identity 改变必须提交新 action 并递增 revision。
- `content_revision` checked overflow 时 file domain 永久 fail-closed、失效当前集合并进入 unavailable；不回绕。
- query、category 或 sort 变化时建立新的 fingerprint key。生产切换后删除 `file-index://changed` listener、
  `FileIndexChanged` 协议和旧 index event；不新增 monitor lifecycle command 或 capability。

### `FileSearchService`

- 保持现有 `search_files` command 的输入和响应结构。
- 将用户输入视为普通文本，而不是 Everything 表达式。
- 对查询语法字符做确定性转义，强制关闭 regex、match path、match case 和 whole word。
- 类别只能由枚举生成固定过滤表达式，前端不能提交扩展名表达式。
- Query2 offset 页不是快照。分页使用双遍一致性读取：page size 256、overlap 64；每页验证 total、request
  flags 和 sort 不变，用 handle 获得的 volume serial + file id 去重，并要求 overlap sentinel identity 与
  顺序完全匹配。
- 第 200 项与页尾同时间戳时读取完整 cutoff tie group。第一遍完成后从 offset 0 重读第二遍，要求直到 tie
  boundary 的有序稳定 identity 序列、total、flags 和 sort 完全相同，才允许排序和提交。
- 漂移、非 overlap 重复、sentinel 缺失或两遍不同，整次读取最多重试 2 次；耗尽返回
  `ConcurrentMutation`。上限固定为每遍 64 页、16,384 个唯一项、32 MiB 和 3 秒总 deadline；超限返回
  `TieGroupTooLarge`。两者都是查询级错误，不计入 runtime/protocol crash-loop。
- 排序只允许修改时间升序或降序。

### `ResultRegistry`

- IPC 返回的路径只能在 Rust 内注册为不透明 result action。
- WebView 继续只提交 `resultId`、invocation id 和 query sequence。
- 文件 action 改为 backend-neutral 枚举。迁移期包含 `Indexed(OpenIndexedPath)` 和
  `Everything(AuthenticatedPathAction)`；旧引擎删除后同时删除 `Indexed` variant 和 `OpenIndexedPath`。
- `AuthenticatedPathAction` 保存 publication generation、最终 canonical path/kind，以及从 volume root 到
  leaf 的每个组件 identity：resolved prefix、volume serial、file id、attributes 和 reparse tag/policy。
- 泛化并复用现有 `pin_indexed_path_components`：执行时逐组件打开不共享 `FILE_SHARE_DELETE` 的 handle，
  拒绝未批准 reparse shape，比较全部组件 identity 和最终 resolved volume/path；所有 handles 保持到
  path-based Shell API 返回。父目录 rename、junction/reparse substitution、跨卷和 leaf 替换均失败闭合。
- 不把 Everything result index、裸路径或 query id 当作长期执行凭证。
- 普通用户新查询继续使用 `begin_query`。轮询使用 `begin_preserving_refresh`，只捕获 generation、domain
  epoch、当前 requestId 和 fingerprint，不修改 latest query、current 或任何 ID。
- backend 在 registry lock 外完成查询、双遍分页、逐组件 action 认证和 fingerprint 构建；失败或超时直接
  丢弃 token，当前 ResultSet 完整保留。
- `commit_refresh_if_current` 在一个 registry lock 临界区重新验证 token。fingerprint 不变时返回
  `Unchanged`，requestId/resultId/actions/revision 完全不变；变化时才分配新 IDs、checked increment revision
  并一次原子替换 ResultSet。旧 IDs 在提交点之前有效，提交点之后才 stale。

## 安装、升级与卸载

### 安装

1. Windows 发布目标收敛为支持自定义 hooks 的 NSIS per-machine 安装器。
2. 安装器启动时一次性请求 UAC。
3. 将固定版本 Everything 放入受 ACL 保护的 `Program Files` 子目录。
4. 写入 UiPilot 自己的 manifest 与受管 INI，包括版本、预先审核的 SHA-256、instance、pipe、INI/DB 路径
   和配置 schema；禁用更新、ETP、HTTP、FTP、托盘及非必要 UI，启用 date-modified fast sort。
5. 首装 `service_owner_sid` 固定为 elevated NSIS token 的用户 SID，即实际提供 UAC 管理员凭据的账户。
   标准用户通过 OTS 输入另一管理员凭据时，owner 是该管理员，不是桌面交互标准用户；文档和 UI 不得
   把它称为“发起安装用户”。使用 `-install-service-security-descriptor` 只授权 SYSTEM、Administrators
   和 owner SID 访问 pipe。受管 INI/DB/config 目录 ACL 同样绑定 owner SID。
6. 验证 Service 名称、binary path 和 pipe 均属于当前安装目录。
7. 失败时回滚本次创建的 Service 和文件；不得影响同机其他 Everything。

Everything 客户端是用户会话进程，不能只依赖 Service。客户端由 UiPilot 首次启动时以普通用户权限启动。

### 升级

1. 安装器通知或终止 UiPilot 自己的客户端实例。
2. 停止 UiPilot 专用 Service。
3. 先验证既有 ownership manifest；覆盖升级、降级尝试或另一管理员重跑安装包都必须保留原
   `service_owner_sid`，不得从本次 elevated token 静默重新授权。manifest 缺失或 owner 不一致时失败闭合。
4. 原子替换受管二进制和 manifest。
5. 运行配置迁移；不直接覆盖未知 schema，并重新验证 INI/DB/config ACL 仍与 owner 对齐。
6. 启动 Service 并执行健康检查。
7. 健康检查失败时恢复上一版本二进制和配置。

### 卸载

1. 仅停止指定 instance/pipe 对应的 UiPilot 客户端。
2. 停止并删除 UiPilot 专用 Service。
3. 删除安装目录中的受管二进制。
4. 用户数据清理遵循卸载器选项；默认可保留设置，但不保留失去所有者的 Service。
5. Service identity、binary path 或 manifest 不匹配时失败闭合并记录非敏感诊断，不执行模糊删除。

## 状态模型

| 状态 | Everything 语义 | UI 行为 |
|---|---|---|
| `building` | 客户端已启动，但 DB 未加载完成或正在首次建立索引 | 显示“正在准备文件索引”，允许继续输入 |
| `ready` | IPC 可用且 DB loaded | 正常展示结果 |
| `partial` | 非所有预期固定卷均在线，或部分 scope 不可用 | 展示当前结果和降级提示 |
| `rebuilding` | DB 正在重建或受管客户端刚恢复 | 保留旧结果或显示等待状态，不执行旧结果 |
| `unavailable` | 组件未安装、Service/客户端无法启动、协议不兼容或连续崩溃 | 空结果、重新安装提示，不回退自研索引 |

`indexRevision` 改为 UiPilot 的 `content_revision`。客户端重启、DB ready、IPC 重连、状态改变以及查询
fingerprint 改变时 checked increment。普通新增、删除或重命名由 `/find` 活跃期间的 1 秒有界轮询发现；revision
不能只表示 runtime 生命周期，也不映射 Everything 内部数据库 revision。

## 查询与结果语义

### 必须保持

- 空 query 可以进入 `/find`，返回按修改时间排序的前 200 项。
- 非空 query 只匹配文件或文件夹名称，不匹配父目录路径。
- 分类扩展名集合继续由 UiPilot 固定定义。
- 文件夹只出现在“全部”和“文件夹”。
- `total` 可以大于 200，但 `items` 不超过 200。
- 修改时间缺失或非法的记录不发布；大小未知可为 `null`。
- 同分记录增加确定性 tie-breaker，例如完整路径 ordinal compare，避免选择项抖动。
- 超过 200 个相同修改时间的候选必须分页读取完整 cutoff tie group 后再截断，不能只在首个 200 项上排序。

### 需要 Spike 冻结

Everything 查询字符串有自己的操作符。Spike 必须用真实进程验证普通文本翻译器至少覆盖：

- 中文、英文和组合字符。
- 多个空格、前后空格、双引号和反斜杠。
- `!`、`|`、`<`、`>`、`(`、`)`、`*`、`?`、冒号和分号。
- 文件名包含 Everything 关键字或函数样式文本。
- 大小写、Unicode normalization 和 Windows ordinal 行为差异。

Rust 可以对候选结果再次执行现有 `fold_name(name).contains(folded_query)`，但只能作为防御性一致性
断言。有限 over-fetch 无法保证精确 `total`，因此不能作为语义 fallback。若普通文本无法安全、等价地
翻译，Iteration 1 必须 No-Go，或另行批准“采用 Everything 原生查询语义”的产品变更；不得静默发布
结果集和 total 不一致的实现，也不得直接开放高级 Everything 语法来规避问题。

## 并发、超时与恢复

- launcher 现有 query sequence 继续作为最终发布门禁。
- 迁移旧 adapter 时暂时保留 `file-index://changed`；生产切换时改为 `/find` 内 1 秒查询轮询并删除该事件链。
- UI debounce 后的新查询覆盖尚未发送的旧查询。
- 已发送 Query2 不依赖协议取消；结果到达后通过 query id 和 sequence 丢弃。
- 单次 warm query deadline 初始设为 1 秒；超时只失败当前查询，并触发一次健康检查。
- 客户端启动/DB loaded 使用独立的较长 deadline，不与搜索 deadline 混用。
- 连续三次协议或进程故障后进入 `Unavailable`，仅在应用重启或退避窗口到期时重试。
- 不允许搜索请求触发 Service 安装、UAC 或安装器操作。
- 退出时停止接受新请求，失效 query ids，关闭 IPC window，再终止受管客户端。

## 安全与隐私

- 固定 Everything 版本和人工审核的 SHA-256；普通 fetch 任务只能验证 lock，禁止根据刚下载的 artifact
  生成或重写信任根。更新 lock 必须是独立的人工评审变更，并同时验证 Authenticode publisher。
- 正式包必须对 UiPilot 安装器和受管二进制供应链做签名与校验。
- Everything 二进制、manifest 和 Service binary path 位于普通用户不可写目录。
- 配置和数据库目录禁止 WebView 写入，不提供任意路径配置 command。
- 搜索日志不记录 query、文件名、完整路径、用户名或原始 IPC buffer。
- 验证导出只允许状态、版本、延迟分桶、错误码和计数。
- 首版禁用 ETP/HTTP/FTP 等网络服务，并验证无监听端口。
- 禁用 Everything 更新提示和自更新；升级只跟随 UiPilot 发布。
- Service 和客户端使用唯一、版本稳定的名称，防止与用户实例混淆。
- 升级和卸载必须先验证所有权 manifest，禁止按进程名或模糊 Service 名批量清理。
- 默认 Everything Service 会扩大本机文件名可见面。首版用 `service_owner_sid` 限制 pipe，并用 OTS 桌面
  标准用户、第二本地用户和普通未授权账户执行负向枚举测试；无法证明隔离则 No-Go。
- 首版不提供 WebView repair 按钮或 repair command。故障 UI 只提示从可信发布渠道重新运行已签名安装包，
  避免在应用内保存、发现或启动 installer maintenance helper。

## 兼容与发布策略

- 首版只发布 x64，且文件搜索只支持执行安装的 Windows 用户；其他本地用户看到稳定 unavailable。
- 基线候选为 Everything 1.4.1.1032 x64 Stable；1.5 Beta 不进入首版依赖。
- 不静默使用机器上的其他版本，即使受管组件损坏。
- 新旧后端只在开发 feature flag 下并存；生产包只启用 Everything 后端。
- 迁移期间保留现有前端协议，避免 UI、索引和安装器同时大改。

## 迭代方案

### Iteration 0：依赖与许可冻结

交付物：

- 固定 Everything 1.4 Stable 的精确版本、官方下载来源、独立人工审核的 SHA-256、Authenticode publisher
  和 MIT License；自动 fetch 只验证，不生成或更新 lock。
- 确认发布包可再分发 Everything 主程序，而不只是假设 SDK DLL 可再分发。
- 冻结 instance、Service、pipe、安装目录、配置目录和数据库目录命名。
- 记录 Tauri NSIS per-machine、自定义 install/uninstall hooks 和升级回滚方式。

Go 条件：法务/许可、供应链、签名、安装器和静默升级路径均无阻塞项。

### Iteration 1：独立 IPC Spike

在 `spikes/everything-ipc/` 建立不接入 Tauri 的 Rust 可执行 Spike。

交付物：

- 启动私有 Everything 客户端并找到正确 IPC window。
- Query2 查询名称、完整路径、attributes、size 和 date modified。
- 分类、升降序、空 query、总数和 200 项限制。
- Query2 编码注入 reply HWND/message；`WM_COPYDATA` envelope 验证 source HWND 和 `dwData` 后关联 request；
  LIST2 parser 只解析 payload，并返回实际 request flags 与 sort type。
- query id、超时、迟到 reply、客户端退出和重启。
- 普通文本转义 fixture 和真实文件树对照测试。
- 超过 200 个相同修改时间结果的 cutoff tie-group 双遍分页测试；覆盖分页期间在 cutoff 前插入、删除、
  重命名、重复页、漏页、持续变化，以及远大于 320 项直至资源上限的同时间戳组。
- 与 SDK DLL 对照结果，确认直接 IPC 没有遗漏必要能力。

Go 条件：真实 Windows 环境连续运行 1,000 次查询无崩溃、串线、越界解析或结果语义偏差。

### Iteration 2：安装器与托管运行时 Spike

交付物：

- NSIS 安装时一次 UAC 安装 UiPilot 专用 Service。
- `service_owner_sid` 对应账户启动 UiPilot 后自动启动隐藏客户端并连接 private pipe；OTS 桌面标准用户
  启动时得到稳定 unavailable，不尝试重写 owner 或放宽 ACL。
- 提交受管 INI 模板和固定 INI/DB 路径；验证更新及 ETP/HTTP/FTP 关闭、无托盘、DB loaded、必需
  request flags、实际 sort type 和 date-modified fast sort。
- Service pipe SDDL 与 INI/DB/config ACL 绑定首装 `service_owner_sid`。标准用户 + 不同管理员 OTS 时 owner
  是管理员凭据账户，桌面标准用户不能枚举；另一管理员覆盖升级不得改变 owner。
- 与用户已安装的 Everything 同时运行，配置、DB、进程和 Service 均不碰撞。
- 覆盖全新安装、标准用户 + 不同管理员 OTS、另一管理员覆盖升级/卸载、降级拒绝、安装中断、重新运行
  签名安装包和卸载；升级必须保留首装 owner SID。
- 用户拒绝 UAC 时安装整体取消；已有版本、已有 Service 和用户自己的 Everything 均保持不变。

Go 条件：干净 VM 和“已安装个人 Everything”的 VM 均通过安装/升级/卸载矩阵。

### Iteration 3：后端适配层接入

交付物：

- 新建 `file_search` 抽象，现有 command 不再直接依赖 `FileIndex`。
- 新建 backend-neutral `FileResultAction`。迁移期显式区分 Indexed 和 Everything action，禁止新 backend
  构造 `OpenIndexedPath`。
- ResultRegistry 新增 preserve-current refresh transaction；不变 refresh 不清空、不换 ID，变化 refresh 只在
  新 actions 全部认证后原子 swap，失败/超时保留当前 ResultSet。
- `AuthenticatedPathAction` 泛化逐组件 pin/auth，父目录、junction/reparse 和 leaf identity 都进入 action。
- `EverythingFileSearch` 接入 Query2 worker 和 runtime supervisor。
- 保留 `FileSearchResponse`、query sequence、ResultRegistry 和 execute_result 契约。
- 用 fake backend 迁移 command 和生命周期单元测试。
- 开发构建可在旧后端与 Everything 后端间显式切换，生产默认仍不切换。

Go 条件：现有前端 `/find` 测试不需要改变产品行为即可通过。

### Iteration 4：生产行为与故障体验

交付物：

- `building/ready/partial/rebuilding/unavailable` 的新状态映射。
- 安装缺失、Service 停止、客户端崩溃、DB rebuilding、IPC timeout 和协议不匹配提示。
- `launcher-core.ts` 每 1 秒调用 `refresh_files`；未变化 in-flight/完成期间旧 ID 始终可执行且完全不变；变化
  in-flight 期间旧 ID 可执行，原子提交后才 stale；失败/超时保留当前集合。
- canonical fingerprint 包含完整执行身份。同路径、同 metadata 但 file ID 或父组件 identity 变化必须原子
  替换并递增 revision；checked overflow 失败闭合。
- 首版不提供 repair command；故障 UI 只提示重新运行可信渠道的已签名安装包。
- warm、cold、首次 DB 构建和进程恢复性能证据。
- 无查询、路径或原始 buffer 的诊断导出。

Go 条件：功能、性能、可访问性、安全和故障注入验收通过。

### Iteration 5：切换与删除自研索引

交付物：

- 生产构建切换到 Everything backend。
- 删除扫描、USN watcher、SQLite index、integrity worker 和相关生命周期协调代码。
- 删除 `rusqlite` 及只服务于自研索引的 Windows features。
- 将仍有价值的查询、ResultRegistry 和执行测试迁移到新模块。
- 删除旧索引数据库时只清理 UiPilot 已知路径，并保留一次版本化迁移记录。

Go 条件：发布候选包不包含可达的旧索引代码路径，安装包和运行时验证全部通过。

### Iteration 6：受控发布

交付物：

- 内部、灰度、正式三个渠道分别记录安装成功率、Service 健康率和查询错误率。
- 只采集匿名计数和错误码，不采集搜索词或路径。
- 为受管 Everything 组件准备独立 kill switch：禁用 `/find`，不影响 launcher 主功能。
- 明确回滚到上一 UiPilot + Everything 固定版本的发布步骤。

Go 条件：灰度期没有安装器冲突、Service 残留、用户 Everything 被影响或持续崩溃。

## 验收门禁

### 功能

- 文件、文件夹、空 query、九类过滤和修改时间排序与当前产品契约一致。
- 结果最多 200 项，total 正确，旧响应不能覆盖新响应。
- 未变化 refresh 不重分配 requestId/resultId；变化 refresh 在提交点前保留旧执行凭证，提交点原子替换。
- 分页漂移不会提交部分结果；持续变化、资源超限或 timeout 保留当前 ResultSet。
- 文件定位和目录打开只通过 ResultRegistry action。

### 性能

- UiPilot 主窗口显示不等待 Everything 客户端或 DB ready。
- 已预热时 `/find` 首批结果达到当前体验目标，并记录 p50/p95/p99。
- 客户端冷启动和首次 DB 构建有独立指标，不用 warm query 指标掩盖。
- 记录 UiPilot、Everything 客户端和 Service 的组合 working set。

### 隔离

- 用户自己的 Everything 可以运行、升级、退出和卸载，不影响 UiPilot 实例。
- UiPilot 卸载后只删除自己的 Service、pipe、配置和受管文件。
- `service_owner_sid` 对应账户可连接；OTS 桌面标准用户、未授权本地账户和第二用户会话无法连接 pipe 或
  枚举文件名，并得到稳定 unavailable。另一管理员可执行已授权卸载，但不能通过升级把 owner 改成自己。

### 安全

- 普通用户不能替换 Service binary、manifest 或受管 Everything binary。
- WebView 无法提交 Everything syntax、路径、实例名、pipe 名或命令行参数。
- 畸形 IPC reply、超大长度、错误 offset、无终止 UTF-16 和未知 flags 均失败闭合。
- envelope source HWND、`COPYDATASTRUCT.dwData`、LIST2 request flags 和 sort type 全部经过验证。
- 同路径替换、父目录 rename、junction/reparse substitution 和跨卷替换的竞态均被逐组件 pin/auth 拒绝。
- 无网络监听、无自更新、无敏感诊断导出。

### 生命周期

- Service 停止、客户端崩溃、Windows sleep/resume、Explorer restart、UiPilot 快速退出和系统注销均可恢复。
- 安装器升级时不会留下锁定二进制、重复 Service 或孤儿客户端。
- 拒绝 UAC 会安全取消安装；磁盘满、杀毒软件隔离和配置损坏均有可解释状态。

## 主要风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| Service 与 SDK/IPC client 被误认为同一进程 | 安装后仍无法查询 | 明确双进程拓扑，安装器只管 Service，UiPilot supervisor 管客户端 |
| Everything 查询语法改变普通文本语义 | 错误结果或语法注入 | 固定 translator、危险字符 fixture、必要时 Rust 二次过滤 |
| 用户已有 Everything 被误操作 | 严重信任与数据风险 | 私有 instance/pipe/config/DB，所有清理基于 manifest 精确所有权 |
| SDK/IPC 协议解析错误 | 崩溃或越界读取 | 专用线程、长度/offset 校验、fixture、真实进程和模糊测试 |
| 普通文件变化不触发刷新 | `/find` 长期陈旧 | `launcher-core` 活跃轮询，backend 只在可见结果或状态变化时递增 revision |
| 轮询提前清空 ResultSet | 可见结果 Enter 间歇 stale、ID 每秒抖动 | preserve-current token，fingerprint 不变不分配 ID，变化时单锁原子 swap |
| fingerprint 漏掉执行身份 | 同路径替换后旧 action 永久滞留 | canonical preimage 包含逐组件 identity；identity 变化强制 revision/atomic replace |
| Service 暴露全机文件名 | 其他本地用户越权枚举 | pipe/目录 ACL 绑定首装 credential-owner SID，覆盖 OTS 与另一管理员负向测试 |
| Query2 offset 分页漂移 | tie group 漏项、重复或顺序错误 | overlap sentinel + stable identity + 双遍一致性 + 有界重试，失败不提交 |
| 仅 pin 叶子对象 | 父目录替换使 path-based Shell 操作错误对象 | 泛化逐组件 pinning，全部 handles 持有至 Shell API 返回 |
| 安装器失败留下 Service | 升级与卸载故障 | 事务式 hooks、安装清单、重新运行签名安装包、全新/中断/回滚矩阵 |
| Everything 版本升级改变行为 | 搜索或安装回归 | 固定 1.4 Stable，升级作为独立依赖评审 |
| 双后端长期并存 | 维护成本不降反升 | 只在开发迁移期并存，Iteration 5 删除旧引擎 |
| 第三方组件不可用 | `/find` 中断 | 独立 kill switch 和明确的重新安装提示，不影响 UiPilot 主功能 |

## 最终建议

迁移建议为 Go，但只授权 Iteration 0 到 Iteration 2。完成许可冻结、Query2 IPC Spike 和安装器/隔离
Spike 后，再进行一次正式 Go/No-Go 评审。若三项均通过，后续接入应以“保持前端和安全协议、替换后端
所有权”为原则推进，并在生产切换后删除自研索引，而不是把 Everything 作为第二个 fallback。

## 官方参考

- Everything downloads and version channels: <https://www.voidtools.com/downloads/>
- Everything MIT License: <https://www.voidtools.com/License.txt>
- Everything SDK: <https://www.voidtools.com/support/everything/sdk/>
- Everything IPC: <https://www.voidtools.com/support/everything/ipc/>
- Everything command-line options: <https://www.voidtools.com/support/everything/command_line_options/>
- Everything multiple instances: <https://www.voidtools.com/support/everything/multiple_instances/>
- Tauri Windows installer: <https://v2.tauri.app/distribute/windows-installer/>
- Tauri NSIS configuration: <https://v2.tauri.app/reference/config/#nsisconfig>
