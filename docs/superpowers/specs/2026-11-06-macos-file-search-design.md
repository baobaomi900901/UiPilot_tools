# macOS Find 与 Spotlight 文件搜索设计

**状态：2026-11-07 已选定方案 A，Task 8 正在实施。** 基线 `37d1bca`，适用 M1 系列及后续 Apple Silicon。普通构建保持 `HostCapabilities.files=false`。显式预览构建已接通主窗口 `/find`、Find 专用 IPC、Spotlight 查询、对象身份复验和 Finder 动作。首版 Task 7 的 Windows 原生交付进展见设备验收单；按用户决定跳过后续芯片/较新系统实机行。公开分发的公证与 Gatekeeper 验收仍单列。按本轮决定先做不依赖范围选择的查询、取消和身份校验，非阻塞人工测试后补。默认范围已选定；正式开放产品入口仍需验证范围授权边界。参见[首版支持矩阵及 Task 8](../plans/2026-11-06-macos-port.md)和[逐步验收 §8](../plans/2026-11-06-macos-port-steps.md)。

## 用户可见范围与默认行为

用户在主启动器输入 `/find 会议`，按 Enter 打开一个独立的 Find 窗口。Find 保留现有搜索框、分类和列表；主窗口只在所属提交成功时清空输入。**默认检索范围为方案 A：系统已索引、UiPilot 可访问的本地文件**。Spotlight 同时支持系统定义的范围和显式目录 URL；每次启动手选一个目录不是 API 限制。本机显式目录查询已正向命中。系统级 scope 加严格公开路径谓词的实验完成但返回 0 行；它没有验证 A 的真实覆盖，不构成选择 B 的依据。

| 范围策略 | 用户体验 | 权限、失效与验证成本 |
| --- | --- | --- |
| A（已选）：搜索系统已许可的 Spotlight 本地范围 | 输入文件名即可检索，更贴近启动器。应明确提示只覆盖 Spotlight 已收录且当前进程可访问的本地对象 | 采用系统 indexed local scope 或等价本地卷范围。SDK 对 IndexedLocalComputerScope 的定义还包含可能位于远端的 home，须校验结果实际挂载类型并排除网络。不能把结果可见当作文件授权，也不能把整个卷都视为已授权。需在普通账户、正式签名身份上实测 TCC 拒绝/撤销是否无意弹窗或泄漏名称；不能证明边界时继续关闭该策略 |
| B（未选）：缩小到用户明确选取的目录，允许记住上次选择 | 首次选一次，此后直接检索旧范围，适合只搜项目/工作资料。可替换或清除范围；取消选择保留原设置 | 仅保存用户选取的本地根及校验身份。持久 bookmark 只用于定位，不能替代重新鉴权；需要 security-scoped access 时须验证签名/沙箱/entitlement 条件，并平衡 start/stop。重启重新解析，检查卷、根身份、是否移动/替换、权限。stale、不可达、身份变化或撤权则显示「重新选择目录」，旧票据失效；不得自动切到更广范围 |
| B 的可选隐私模式：仅本次会话 | 用户主动选择「仅本次」时不记住路径，退出释放。每次启动重新选择的交互成本仅由该模式承担 | 隐藏/重开保留进程内选择；退出释放。适用与持久目录相同的身份/本地卷检查 |

已选定 A 作为产品默认范围。内部预览先限定到已验证的本地公开目录，不要求用户选择目录；这只验证入口与系统操作，不代表用户目录的覆盖或正式发布验收。B 仅留作备选。额外授权系统「文稿」只为核验特定受保护位置的访问权限：原有公共范围继续有效，不提供任意目录作为搜索根，不持久保存路径或书签。

两种策略都排除网络卷、跨挂载逃逸、别名/符号链接与包内部条目；B 不接受整个卷或包目录作根。Spotlight 元数据中的路径只提供候选，不授予文件访问权限。不读取 iCloud 占位文件内容、不触发下载，不申请完全磁盘访问。受拒项目不得展示其路径；系统能证明拒绝时提示范围限制。Windows 保持原有搜索范围与 Everything/SystemIndex 行为。

| 操作 | Mac 拟定结果 |
| --- | --- |
| 空输入或只输入 `/find` | 显示/聚焦已有 Find，清空查询；零查询调用 |
| 在文件行按 Enter | 在 Finder 定位并选中该对象，沿用 Windows 的文件默认意图 |
| 在目录行按 Enter | 在 Finder 打开该目录 |
| 文件行的「打开文件」 | 仅对允许的本地普通文档，经身份复验后交给默认文档应用 |
| 固定、Esc、关闭 | 固定时失焦/执行不隐藏，Esc 无动作；关闭按钮始终隐藏；隐藏保留窗口 |
| 文件预览 | 可展示已获授权的名称、类型、大小、修改时间、路径；Mac 此阶段不生成缩略图，不向关闭的 thumbnail IPC 发请求 |

「打开文件」仅用于已支持的文档/媒体扩展类别且普通文件没有执行位；应用包、脚本、可执行文件、链接/别名、URL 快捷方式不提供该动作。文件默认定位与显式打开分开，避免一次 Enter 启动可执行内容。界面遵循[UI 规范](../../ui-guidelines.md)，Light/Dark、键盘焦点圈与中文 IME 确认均按现有 Find 交互验证。

## 能力、窗口与 IPC

- 启动阶段沿用共享 React/IPC 和 `FindWindowController`。Mac 有能力时才创建隐藏的单个 `find`，允许生产 `tauri://localhost` 或已配置的开发回环来源。现有 `host_capabilities::require_main`、Mac 的 local-navigation 插件均仅允许 main；应新增显式的 find 身份检查、分别限定导航和 ACL，并分别验证 main/find WebView label、窗口与来源。拒绝任意其他窗口、外部页和 WebView 后才读取设置、范围、registry 或原生操作状态。
- main 可展示/收藏和转发 Find。单独开放 `files`，保持 `plugins`、`builtinFeatures` 等其他能力关闭。目前 `src/host-capabilities.ts` 会在 `builtinFeatures=false` 时过滤含 iconKind 或内置收藏的结果；要为 **Find 自身** 放行 existing Find 图标、`ResultFavorite::builtin(BuiltinFeature::Find,…)` 和 `favoriteMatchesResult`，并回归上下文菜单。宿主的收藏操作也只能修改已启用的 Find。Mac `search_apps` 需实现 `/find` 补全和 `OpenFind`，而非打开所有内置命令。
- main 的 `open_find_window` 校验当前 main invocation/query，成功转发使用既有前端提交所有权。find 的 `prepare_find_initialization`、`commit_find_ready`、状态读取、pin/hide、`search_files` 及 Find 执行只接受当前合法 Find。作用域由宿主选择。Mac `execute_result` 今天只允许 main 应用结果，将按受信 label 路由到 main 应用或独立 find 文件 registry；同一 ID 不跨域。额外的 `open_find_document(requestId,resultId)` 仅允许 find，宿主持有动作与目标，客户端不得提供路径、根、命令、程序或 URL。
- Find 显示宿主报告的范围。文稿预览允许读取状态、请求授权及停止搜索，三条命令只接受 Find 的 invocation ID，由宿主选择系统文稿目录。授权有独立 revision，变更时旧票据失效。查询和动作分别复核当前允许的本地挂载、根身份及授权。
- Find 私有目录/结果权限与公开插件的 `files.index.readAll`、`files.openText` 相互独立。本次不扩大 SDK 权限。独立 ACL 仅在实际后端、原生窗口、负面测试都通过后才启用。

## 文稿访问（内部预览）

默认无需选择目录。Find 中的「允许本次搜索文稿」只接受当前可见 Find 发出的请求。宿主自行定位本地系统文稿目录，用户在原生目录面板确认后才把它作为临时授权根；WebView 不传文件路径。选择其他位置、取消、窗口在选择期间关闭或转发到新查询时，迟到的选择不能改变范围。QA 的系统范围证据与这个入口分开。

授权变更使旧查询和结果票据失效，同一搜索词应重新查询。停止搜索、系统撤权、文稿根被替换或进程退出后，不再发布该根下的项目或执行旧结果。内部 ad-hoc 包只承诺当前进程内的授权。正式版若扩展到跨启动恢复，需要独立验证签名、entitlements、安全作用域书签与撤权生命周期。

## 范围验收与发布条件

- 普通构建不开启 Find。内部 `macos-find-preview` 构建将本地公共字体目录和 `/Users/Shared` 的目录 URL 传给 Spotlight，当前进程获准访问本地文稿后再加入文稿根。每项结果都复验。本构建用于验收查询、收藏、窗口转发、文稿授权和 Finder 动作。
- `macos-find-system-qa` 仅为受控签名身份和隔离测试账户准备；它使用系统 `NSMetadataQueryIndexedLocalComputerScope`。只有 QA 构建、宿主环境显式设置 `UIPILOT_FIND_ISOLATED_QA=1` 且 `UIPILOT_FIND_QA_VERIFIED_ROOTS` 有值时才启用入口。后者是本次已独立核验权限的目录，以冒号分隔；只接受小范围规范本地根，不接受整个用户主目录、上级、重叠根或跨卷逃逸。该证据限 QA 流程，不是最终用户的搜索偏好。该模式不分发给普通用户。
- 系统 QA 验收需分别覆盖已允许、未授权、明确拒绝、撤权与对象替换，确认没有意外隐私弹窗、受拒行或错误系统动作，才能扩大公开支持范围。Apple 说明在缺少明确或隐含授权时首次访问桌面可能触发系统询问。[Apple 桌面目录访问说明](https://developer.apple.com/documentation/bundleresources/information-property-list/nsdesktopfolderusagedescription)

## 生命周期与并发顺序

以[单实例 Find 规格的 Opening And Forwarding、Native Main-To-Find Focus Transfer、Window-Scoped Result Authorization](2026-08-11-find-single-window-design.md)为既有合同。Windows HWND、置顶和 foreground 快照按平台分开；Mac 用 AppKit 应用激活、key window 与 Tauri focus/visibility 联合确认，不把陈旧的 Focused 事件当作新转移成功。

1. Find 先完成监听注册和 readiness prepare，再 commit 当前 token。prepare 不消费排队的转发；晚到旧 token、超时与退出均不能把控制器标为 ready。复用原 5 秒准备/排队上限与 2 秒焦点转移上限。A 在 prepare 期间排队，B 替换 A；A 只得到 superseded，commit 当前 token 后只交付 B。
2. 主窗口转移时记录 main/find 可见性、key window、当前活动 invocation；仅抑制本次预期 main blur。提交 Find 所有权前失败/超时则还原原生状态；提交后 emit 失败则清空新 find scope 并隐藏 Find，绝不复活旧 ticket。成功后 main 保持可用。获取顺序为 controller → find registry；系统 API、用户选择、等待通知或主线程调度均在锁外。
3. Find 可见且 invocation 匹配才允许查询。每次有效提交用 `registry.begin_query(QueryDomain::File,…)` 令前一结果集失效。新查询 B、清空框、清除/替换范围、隐藏、进程退出分别撤销其旧 query owner；空输入须在 JS 清屏的同时通过 find-only `cancel_find_query(invocationId,querySequence)` 取消原生查询并退役旧结果；按单调序列验收，迟到的取消不得影响更新查询。异步 completion 回到宿主后重查 invocation、范围 revision、query token 和可见状态才能 `publish_if_latest`；迟到结果返回 null，不发事件、不改行、不附旧错误。
4. 选中当前文件后取得私有 ExecutionTicket；等待时 pin 发生变化，以完成时的 pin、controller、ticket 决定隐藏。已开始 native hide 的旧执行遇到新 forward，保持单个最新等待者，hide 结束后显示新查询。已经开始 B 的情况下 A 的完成不能隐藏 B。关闭/进程退出立即使待选目录、待执行和查询所有者失效，只释放所属实例。

## Spotlight 后端与结果

采用 Foundation `NSMetadataQuery`。一个专属、有运转 run loop 的串行后台执行环境拥有 query、观察者和取消操作；不在主线程等待 gathering。由宿主根据已批准模式设置系统本地索引 scope 或已选目录 NSURL；WebView 不能传入范围列表。固定 `%K CONTAINS[cd] %@`，键是 `NSMetadataItemFSNameKey`，用户输入作为独立参数。查询只匹配文件名；引号、星号、斜线和 SQL/Spotlight 样式字串均按用户文本，不拼接 predicate/命令，不用 `mdfind` 子进程承载产品查询。

| 合同 | 拟定实现门槛 |
| --- | --- |
| 输入 | 沿用 `prepare_file_query`：1–255 个字符、最多 1,024 字节、无 NUL，非空 invocation、递增且非零 querySequence；空框走取消。排序只接受现行 `modifiedDesc`，保留 Windows 现有行为 |
| 分类 | 复用 `all/folder/excel/word/ppt/pdf/image/video/audio/archive` 与 `everything::category_predicate` 的扩展名清单，提取共享语义，不把 Everything 表达式送给 Spotlight。类型/kind 在身份认证后复核 |
| 数量与排序 | 用修改日期降序收集首批候选，最多核查 400 个，最多发布 200 行。在同一批内按已核实修改时间降序、显示路径稳定打破平局；`total` 等于返回行数。超过候选预算或存在不可核实项，状态为 `partial` 并提示范围/截断，不宣称已穷举。OS 可能在内部收集更多；产品限制针对宿主复制/鉴权的数量 |
| 截止与清理 | 查询从启动起最多 3 秒；在 finish、取消或超时的同一串行 owner 上停止 query、移除 observers 并释放对象。完成只取一次快照；迟来的 finish/update 不可发布。超时有已验证结果则 partial，无可用结果报可重试的搜索服务错误 |
| 返回值 | 沿用 `FileSearchResponse`、`FileResultItem`；`requestId/resultId` 为 opaque；`indexRevision` 是已发布快照本地单调编号，不是 Spotlight 索引版本。fullPath 只供展示；大小、时间来自已认证对象，目录大小为 null，modifiedUtc 为 UTC RFC3339 |
| 空、排除与访问错误 | 成功完成但 0 行只说明「当前范围内的 Spotlight 可见结果无匹配」，可提示索引覆盖限制。只有明确 OS 证据才标注「未索引/排除」；权限拒绝、根卸载/替换、服务启动失败、超时分别报告，绝不静默遍历磁盘或请求全盘权限 |

开始前订阅 finish，令 request owner 与该 NSMetadataQuery 实例一一对应。取消必须同时终止宿主发布资格与原生工作；只调用 `stopQuery` 不足以收回已经排队的通知。未来 Rust 实现可使用仓库锁定的 objc2-foundation 绑定并只开启所需 feature；本次 Objective-C 探针验证 API，不选择命令行回退。

## 文件身份、授权与系统动作

A 中每个允许访问的本地卷须绑定挂载身份，按当前 OS 权限逐对象校验；本地卷身份不是对卷内文件的访问授权。B 中根身份由已打开目录描述符、卷/设备、inode 及 scope revision 标识。基于已验证的范围入口逐组件打开候选相对路径，拒绝绝对路径、`..`、跨根、跨挂载、符号链接/别名、包内及特殊文件。只接受范围内、当前用户和宿主仍有权读取的普通文件或目录。宿主保管 file-reference URL、根内相对路径、认证时的对象类型、设备/inode 及用于检测替换的时间戳；禁止只凭客户端 fullPath 或 Spotlight `kMDItemPath` 作系统调用。切换模式、权限撤销、卷卸载或根替换即退役旧结果，相同字符串的新目录不能继承原授权。

执行顺序为 **验证调用者 → Find 可见性 → 解析 current ticket → 从原根重新鉴别路径及对象 → 确认 file-reference URL 与原对象一致 → 最终 ticket/范围核验 → 系统调用**。用户重命名、删除、同路径替换、替换父目录或撤权都返回固定错误，系统调用次数为零。对同一 ticket 的双击、Enter 与菜单选择只允许一次系统提交；新 query 或退出在最终 admission 之前先到达则拒绝。日志只记计数、状态和错误类别，不写查询、文件路径或元数据内容。

Finder 定位使用 `NSWorkspace.activateFileViewerSelectingURLs`，它没有成功回调，返回值只表示已发出请求。打开文件使用 `openURL:configuration:completionHandler:`；打开目录请求 Finder 对应本地目录。继续使用现有 `fileRevealRequested/folderOpenRequested`；显式文档打开新增 `fileOpenRequested`，同步前端协议与状态处理。已向系统提交但结果无法确认则消费该 ticket、保持窗口并提示用户检查，不自动重试。尚未提交的错误允许重新搜索/再次尝试。Finder 实际选中什么、默认文档应用是否打开及焦点切换，均需经授权的实机验收。

## 私有后端实现进展

`207d590` 的查询桥在后台 run loop 运行 NSMetadataQuery，保留独立观察者和取消标记；按同一截止时间限制收集及复制。文件名作为字面参数，最多复制 400 条内部候选。已通过 8 个确定性查询测试，并在指定的系统字体目录中命中 `Andale Mono.ttf`。文件身份模块从已验证的根目录描述符逐组件拒绝符号链接和跨挂载，在重新鉴别时检查父目录、叶子和身份是否变化；6 项测试通过。

以上是 `207d590` 的基线。随后已在显式预览构建中接入窗口和命令白名单、结果发布、挂载/包/别名排除、有效读取检查、原生文件引用及执行前复验。默认构建不注册 Find IPC。真实 Finder 选择与焦点、正式签名身份的受保护路径拒绝或撤权仍待单独验收。

## 本次只读证据

2026-11-06，Apple Silicon 开发机 macOS 15.7.4 (24G517)、arm64、Command Line Tools SDK 15.2。仅查询系统公开的应用和字体样例。未启动 Finder、打开文档、修改索引或授权。TextEdit.app 用于验证查询机制；Andale Mono.ttf 给出真正普通文件的正向命中，产品将排除应用包。

| 实际检查 | 观察结果 |
| --- | --- |
| 受限终端中的 `mdutil -s /`、`mdls`、`mdfind` | 分别显示服务关闭、找不到 TextEdit.app 和 0 行。只读本机检查中卷报告 Indexing enabled，`stat` 确认 Directory，`mdls` 返回 `TextEdit.app`、`com.apple.application-bundle`；同一 `mdfind -onlyin /System/Applications` 返回 1 行。运行环境影响可见性 |
| NSMetadataQuery，文件名包含 `TextEdit.app` | start=1，finish=1，1 行，完整样例路径匹配 1 次，stop 后 stopped=1，61 ms |
| 文件名包含 `Text*Edit.app` | start=1，finish=1，0 行，stop 后 stopped=1，24 ms；本例中星号没有变成通配匹配 |
| 文件名包含 `uipilot-absent-172dae.ac` | start=1，finish=1，0 行，stop 后 stopped=1，25 ms |
| start 后立即 stop，再运行 run loop 250 ms | start=1，finish=0，0 行，stopped=1，总时长 275 ms；此一次观察不保证所有取消都没有晚到回调 |
| NSMetadataQuery，限定 `/System/Library/Fonts/Supplemental`，查询 `Andale Mono.ttf` | start=1，finish=1，1 行，完整公开字体路径匹配 1 次，stopped=1，26 ms。`mdls` 的类型为 public.truetype-ttf-font |
| NSMetadataQuery，`NSMetadataQueryIndexedLocalComputerScope` 加固定公开字体完整路径与文件名相等谓词 | start=1，finish=1，0 行，stopped=1，24 ms。与指定字体目录的实验同时改变了 scope 和谓词，不能据此断定系统 scope 失败或字体未索引。没有读取个人文件名；正式 A 仍需正向测试、受限目录负例和局部卷过滤 |
| 对系统样例内 `Contents/Info.plist` 创建 file-reference URL 并解析回本地路径 | 原对象与回解析对象均为同一设备/inode 的普通文件；未执行任何打开或定位 |

耗时只记录这一轮，不能作为生产 SLA。未验证 A 的系统范围边界、B 的持久 bookmark 恢复、Spotlight 隐私排除、TCC 拒绝/撤销、未索引目录、不同磁盘、真实 Finder 焦点、WKWebView 来源，以及后续 Apple Silicon 设备；这些行为保留在下列门槛中。

## 实施映射和验收

| 工作 | 通过标准及待收集证据 |
| --- | --- |
| 基础装配 | 在 `src-tauri/src/{macos_host.rs,macos_apps.rs,find_window.rs,result_registry.rs,file_search/mod.rs}` 与平台配置中接 Find；目前 Mac 只创建 main，`ResultRegistry::for_main` 需替换为共享 allocator 的 `ResultRegistries`，保留 Task 4 应用查询与 Task 5 归属 hide。main Tauri ACL 不能调用 find 的查询/选择范围/文件执行；外部页、错 label 触发时受保护状态读数和 OS 调用数均为 0 |
| 查询后端 | 新增 `src-tauri/src/file_search/macos.rs` 与可控 provider fixture。`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib file_search::` 命中正向、边界和拒绝用例。测试 201 条、截断、时间平局、字面符号、未知分类、查询超时、未授权路径、无匹配与服务不可用；核对响应状态、数量、候选上限、停止/注销计数 |
| 会话/授权 | `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib find_window::`，覆盖 prepare/commit 替换、A 查询/B 发布/A 晚到、A 隐藏时晚回调、范围替换、执行等待期间 pin、hide 期间再次 forward、退出取消；每序列检查唯一终态、registry ticket 和监听释放。另用可注入执行端口证明 stale/逃逸/符号链接/同路径替换不产生系统调用、已提交超时不重复发送 |
| 前端/入口 | `npm test -- src/find-core.test.ts src/find-view.test.tsx src/launcher.test.tsx` 与 `npm run build`；覆盖 `/find` 输入/补全/收藏、如批准 A，验证无需选择即搜索；如批准 B，验证首次选择、取消保留原范围、记住并重启、stale 后要求重新选择。两者均覆盖范围撤销及替换、中文 IME、无匹配/partial/拒绝、两主题、键盘 Enter/显式打开与缩略图关闭。确认其他内置入口/插件/凭据无调用 |
| 手工、仅在批准实施后 | 使用隔离用户和已索引与**既有**排除/不可用样例目录；按所选模式验证 A 的本地权限/远端排除或 B 的目录身份/重启恢复；放同名中文/空格/特殊字符样例，核对 2 类输出、Finder 实际选中/打开对象与真实回焦、普通文档应用打开、pin/隐藏重开、目录替换后失效。涉及选目录、打开、外部焦点与改变权限的场景按范围单独确认；不为了取证修改用户索引。Windows 原生运行原 Find 查询、分类、默认定位/打开、pin、缩略图、插件/应用 registry 隔离。每行保存设备 OS、提交、包哈希、脱敏操作序列、实际终态 |

上述命令是实施后的验收入口；需要先用 `-- --list` 确认新增 Rust 用例确实被收集。只有 Task 7、规格批准、两平台适用的自动/实机验收以及对应发布证据全部齐备，才可修改支持矩阵。

## 只读探针复现

以下命令只在临时目录编译探针，运行环境需要能连接本机 Spotlight。输出只含计数与耗时。对照 SDK 中 Foundation `NSMetadata.h` 的 gathering/stop 接口，以及 AppKit `NSWorkspace.h` 的 file-viewer/openURL 接口；下列探针不调用 AppKit。

```sh
probe_dir=$(mktemp -d)
cat > "$probe_dir/probe.m" <<'OBJC'
#import <Foundation/Foundation.h>
#import <sys/stat.h>

static void run(NSString *tag, NSString *text, NSString *scopePath, NSString *expected, BOOL cancel) {
    NSMetadataQuery *q = [NSMetadataQuery new];
    if (scopePath) {
        q.searchScopes = @[[NSURL fileURLWithPath:scopePath isDirectory:YES]];
        q.predicate = [NSPredicate predicateWithFormat:@"%K CONTAINS[cd] %@", NSMetadataItemFSNameKey, text];
    } else {
        q.searchScopes = @[NSMetadataQueryIndexedLocalComputerScope];
        q.predicate = [NSPredicate predicateWithFormat:@"%K == %@ AND %K == %@",
            NSMetadataItemPathKey, expected, NSMetadataItemFSNameKey, text];
    }
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    __block NSUInteger finishes = 0;
    id obs = [center addObserverForName:NSMetadataQueryDidFinishGatheringNotification object:q queue:nil usingBlock:^(NSNotification *n){ finishes++; }];
    CFAbsoluteTime start = CFAbsoluteTimeGetCurrent();
    BOOL started = [q startQuery];
    if (cancel) { [q stopQuery]; }
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:cancel ? 0.25 : 4.0];
    while (!finishes && deadline.timeIntervalSinceNow > 0) {
        [NSRunLoop.currentRunLoop runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
    }
    NSUInteger count = q.resultCount;
    NSUInteger exact = 0;
    for (NSUInteger i = 0; i < MIN(count, 32); i++) {
        NSMetadataItem *item = [q resultAtIndex:i];
        NSString *path = [item valueForAttribute:NSMetadataItemPathKey];
        if ([path isEqualToString:expected]) { exact++; }
    }
    [q stopQuery];
    printf("%s started=%d finish=%lu count=%lu exact=%lu stopped=%d elapsed_ms=%.0f\n",tag.UTF8String,started,(unsigned long)finishes,(unsigned long)count,(unsigned long)exact,q.stopped,(CFAbsoluteTimeGetCurrent()-start)*1000);
    [center removeObserver:obs];
}
int main(void) { @autoreleasepool {
    NSString *apps = @"/System/Applications";
    NSString *app = @"/System/Applications/TextEdit.app";
    run(@"positive", @"TextEdit.app", apps, app, NO);
    run(@"literal", @"Text*Edit.app", apps, app, NO);
    run(@"negative", @"uipilot-absent-172dae.ac", apps, app, NO);
    run(@"cancel", @"TextEdit.app", apps, app, YES);
    run(@"regular_file", @"Andale Mono.ttf", @"/System/Library/Fonts/Supplemental",
        @"/System/Library/Fonts/Supplemental/Andale Mono.ttf", NO);
    run(@"system_indexed_scope", @"Andale Mono.ttf", nil,
        @"/System/Library/Fonts/Supplemental/Andale Mono.ttf", NO);
    NSURL *file = [NSURL fileURLWithPath:@"/System/Applications/TextEdit.app/Contents/Info.plist"];
    NSURL *ref = file.fileReferenceURL;
    struct stat a,b;
    BOOL ok = ref.isFileReferenceURL && lstat(file.fileSystemRepresentation,&a)==0 && lstat(ref.filePathURL.fileSystemRepresentation,&b)==0 && a.st_dev==b.st_dev && a.st_ino==b.st_ino && S_ISREG(b.st_mode);
    printf("public_regular_file_reference_dev_ino_equal=%d\n",ok);
    return ok ? 0 : 2;
} }
OBJC
xcrun clang -fobjc-arc -fblocks -framework Foundation "$probe_dir/probe.m" -o "$probe_dir/probe" && "$probe_dir/probe"
```
