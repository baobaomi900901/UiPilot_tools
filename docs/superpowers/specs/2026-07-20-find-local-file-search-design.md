# `/find` 本地文件搜索设计

## 状态

- 日期：2026-07-20
- 状态：用户已确认交互与技术方向；第五轮书面复审已通过；仍待用户书面 Go
- 产品阶段：MVP-A 之后的独立候选功能
- 实现状态：No-Go；本设计不授权实施、集成或发布

## 背景与决策边界

MVP-A 明确不包含文件搜索，也不定义通用斜杠命令系统，见
[文件搜索移出 MVP-A 决策](2026-07-17-remove-file-search-from-mvp-a-design.md)。2026-07-17 的
SystemIndex Spike 因宿主 I/O 证据违反当时冻结的门槛而判定为 No-Go，见
[SystemIndex Spike 结果](../../spikes/2026-07-17-systemindex-results.md)；该路线不能直接进入生产。

本设计根据新的明确需求重新评审 `/find`。它选择应用自建的本地元数据索引，并明确修改 I/O
边界：允许目录枚举和文件元数据读取，继续禁止文件内容读取。该选择不推翻旧 Spike 的事实，
也不把 SystemIndex、Explorer 搜索或外部索引服务作为回退。

`/find` 是 UiPilot 内置功能，不是插件，不建立第三方插件 API，也不纳入当前 MVP-A 的完成度或
发布判定。进入实现前仍需单独的实施计划、依赖复审、安全复审和代码复审。

## 用户目标

用户输入 `/find UiPilot` 并按 Enter 后，UiPilot 打开键盘优先的本地文件搜索面板。用户可按类型
筛选、按修改时间排序、预览文件元数据，并通过键盘或鼠标在 Windows 资源管理器中定位结果。

成功标准：

1. 已有索引时，面板和首批结果快速出现。
2. 首次使用时可边扫描边搜索，不阻塞输入和选择。
3. 搜索只匹配文件或文件夹名称，结果确定、可解释。
4. WebView 不能提交任意路径或 Shell 参数。
5. 索引不读取文件内容、不上传、不进入验证导出。

## 非目标

首版不实现：

- 文件内容搜索、内容预览或缩略图。
- 完整路径匹配、模糊匹配、拼音匹配或语义搜索。
- 网络盘、映射网络盘、可移动磁盘或云盘专用适配。
- SystemIndex、Everything、USN Journal 或 Explorer 搜索委托。
- 用户自定义分类、排除目录或索引位置。
- 设置图标的实际设置页。
- 插件协议、第三方 SDK、云同步、遥测或索引导出。
- 修复或放宽 `SEC-RUNTIME-PROBE-001`。

## 调用与模式切换

主输入只增加一个内置分支，不建立通用命令注册框架：

- 精确的 ASCII 小写 token `/find` 后接输入结束或一个空格时，Enter 打开文件搜索面板。
- `/find <query>` 用 `<query>` 初始化文件搜索词并立即查询。
- `/find` 单独使用时打开空查询面板；在索引 listener 注册成功后仍精确调用一次 `search_files`，以空
  query 完成 ResultRegistry token、FileIndex 懒加载和空结果 publish。不得因 Task 7 现有“空应用查询
  不调用 Rust”优化跳过这条文件模式初始化调用，也不新增独立 init 命令。
- `/finder`、`/findx` 等继续作为普通应用搜索文本处理。
- 每次进入 `/find` 时分类重置为“全部”，排序重置为修改时间降序；预览开关读取持久偏好。
- 面板内 Escape 沿用启动器生命周期，调用既有隐藏路径，不在前端直接操作窗口。

应用搜索和文件搜索共用 Task 7 core 的一个 checked `querySequence`。同一 launcher invocation 内进入
或离开 `/find` 都不能重置它：进入文件模式（包括空 query）先 checked increment，再把新值发送给
`search_files`。只有新的合法 `launcher://shown` 才把 sequence 重置为 0；该 invocation 的第一条
应用或文件查询使用 1。溢出时固定失败并隐藏，不回绕或另建文件序号。

该分支是固定产品行为。未来若需要第二个斜杠命令，必须另行评审是否值得抽取命令框架。

## 界面设计

界面复用 Task 7 的 React 和 Ant Design 基础，不新增前端框架、路由或状态库。页面为无卡片的三栏
工作区，并保留顶部输入区和底部状态栏。

### 左栏：文件类型

100% 三栏布局下左栏使用固定宽度，按以下顺序显示：

1. 全部
2. 文件夹
3. Excel
4. Word
5. PPT
6. PDF
7. 图片
8. 视频
9. 音频
10. 压缩文件

分类区遵循单一 Tab stop：当前分类 `tabIndex=0`，其他分类 `tabIndex=-1`。Tab 从查询框进入当前分类，
再按 Tab 离开分类区到排序按钮；Shift+Tab 反向移动，分类区不得捕获 Tab 形成内部循环。分类区获得
焦点后，垂直布局用上/下、横向响应式布局用左/右方向键循环切换分类，Home/End 切换到首项/末项；
切换立即生效，Enter/Space 不执行结果。切换分类保留查询文本，把选择重置到当前结果第一项。鼠标
单击分类执行相同行为。

固定扩展名集合如下；比较不区分大小写：

| 分类 | 扩展名 |
|---|---|
| Excel | `.xls`, `.xlsx`, `.xlsm`, `.xlsb`, `.csv` |
| Word | `.doc`, `.docx`, `.docm`, `.rtf` |
| PPT | `.ppt`, `.pptx`, `.pptm` |
| PDF | `.pdf` |
| 图片 | `.bmp`, `.gif`, `.heic`, `.jpeg`, `.jpg`, `.png`, `.svg`, `.tif`, `.tiff`, `.webp` |
| 视频 | `.avi`, `.m4v`, `.mkv`, `.mov`, `.mp4`, `.webm`, `.wmv` |
| 音频 | `.aac`, `.flac`, `.m4a`, `.mp3`, `.ogg`, `.wav`, `.wma` |
| 压缩文件 | `.7z`, `.bz2`, `.gz`, `.rar`, `.tar`, `.tgz`, `.zip` |

未知扩展名只出现在“全部”。文件夹只出现在“全部”和“文件夹”。首版分类表不可配置。

### 中栏：结果

中栏占用剩余宽度。每行显示通用文件或文件夹图标、完整名称和后缀。查询有结果时默认选择
第一项；查询输入保持 DOM 焦点时，上、下方向键循环选择最多 200 个已展示结果。

- Enter 执行当前选择。
- 鼠标单击选择后把焦点恢复到查询输入，双击执行；结果行自身永不取得 DOM 焦点。
- 名称过长时使用受约束的多行或省略展示，不改变行高以外的布局轨道。
- 完整名称通过 tooltip 和右侧预览可见。
- 总匹配数可大于 200；列表不提供分页、无限滚动或“加载更多”。

### 右栏：元数据预览

100% 三栏布局下右栏使用固定宽度，仅显示：

- 文件或文件夹名称。
- 大小；文件夹显示 `--`。
- 修改时间。
- 所在磁盘的完整路径。

首版不读取文件内容，不生成图片、文档或视频缩略图。关闭“文件预览”后右栏完全折叠，中栏
扩展到剩余宽度。预览开关保存在当前用户设置中，跨软件重启记忆。

### 底部状态栏

从左到右固定为：

1. `共 N 条结果`；索引运行时追加稳定的扫描状态和已发现数量。
2. 修改时间排序按钮；默认降序，点击或按 Alt+S 在升序、降序间切换。
3. “开启文件预览”开关；点击或按 Alt+P 切换。
4. 设置图标按钮；首版禁用，tooltip 为“设置暂不可用”。

分类、排序和预览都是明确的控件状态。设置图标不得保持可点击但无响应。

### 可访问性与视觉约束

- 使用 Ant Design 的 Input、Button、Switch、Tooltip、Spin 等已有控件；不增加卡片容器。
- 查询 Input 是唯一持有 `aria-activedescendant` 的 combobox；结果使用不可聚焦的 listbox/option，
  `aria-activedescendant` 只引用当前 listbox 中的 option。分类、结果和预览不得建立第二个焦点 owner。
- 选择变化时保证当前项滚动到可见区域。
- 状态变化通过单一礼貌 live region 宣告，不朗读完整路径列表。
- 焦点顺序在桌面和窄屏都固定为查询输入、分类单一 Tab stop、排序按钮、预览开关；禁用的设置按钮
  跳过。结果和纯预览内容不在 Tab 顺序中。查询焦点下，左/右保持原生光标移动，上/下选择结果，
  Enter 执行，Escape 走统一隐藏；分类焦点下只使用对应布局的方向键/Home/End 切换分类，Escape 同样
  隐藏。排序和预览控件保留原生键盘行为，Escape 统一隐藏。
- Task 7 基线窗口固定为不可调整大小的 720 x 420；`/find` 不修改窗口大小或 resizable 配置。
- 100% 且有效 CSS 宽度至少 600 px 时使用左侧分类、中间结果、右侧预览三栏。
- 150%/200% 或有效宽度小于 600 px 时，分类变为结果上方的局部横向可滚动单行 tablist，预览移动到
  结果之后；页面按查询、分类、结果、预览、状态栏的 DOM 顺序纵向滚动，逻辑 Tab 顺序仍遵循上一条，
  不产生页面级横向滚动。
- 响应式切换只改变 CSS grid area，不卸载当前分类、结果或预览控件，不丢失焦点和选择。
- 支持 100%、150%、200% 缩放、forced colors 和长名称/长路径；路径使用 `overflow-wrap:anywhere`，
  不允许文本重叠。
- 图标按钮必须有可访问名称；颜色不作为唯一选择标识。

## 查询合同

查询只匹配最后一个路径组件，即文件或文件夹名称。索引和查询使用同一个固定折叠函数
`uipilot-unicode-15.1-full-fold-nfc-v1`：先按 Unicode 15.1.0 做 NFC，应用 Default Case Folding 的
full、non-Turkic 映射，再做 NFC。实现依赖必须在 Dependency Go 中证明使用 Unicode 15.1.0 数据；
数据库 metadata 同时保存该算法 ID，不匹配时索引整体重建，不混用不同折叠版本。

固定样例为：`UiPilot -> uipilot`、`Straße -> strasse`、`CAFE\u{301} -> café`、`Σ/σ/ς -> σ`、
`İ -> i\u{307}`。兼容字符不做 NFKC：`Ｕｉ -> ｕｉ`，不等于 ASCII `ui`。匹配是在折叠键上的连续
包含匹配，不解释通配符、引号、布尔运算符或 FTS 语法。

查询顺序固定为：

1. 过滤当前固定磁盘和当前分类。
2. 匹配名称。
3. 按修改时间升序或降序。
4. 时间相同时按原始名称的 Windows `CompareStringOrdinal(..., TRUE)` 顺序。
5. 名称仍相同时按规范化绝对展示路径的 `CompareStringOrdinal(..., FALSE)` 顺序，保证结果稳定。

后端取得 FileIndex publication gate 后开启 SQLite read transaction，并先执行一次 metadata read 以
固定 read snapshot；在同一临界区捕获 `index_revision` 和 aggregate status 后释放 gate，再在该事务中
读取准确总数和排序后的前 200 项，最后统一提交读事务。scanner/watcher writer 和 status-only transition
也必须取得同一个 gate，因此响应中的 status、count、items 和 revision 对应同一个线性化点。索引仍在
进行时，总数代表该快照已发现的集合，状态必须明确表示结果仍在增长。

SQLite FTS5 trigram 对已折叠文本使用 `case_sensitive 1`，不再执行 SQLite 自身的大小写转换。
查询字符串由后端构造为参数化的字面量，不得把 WebView 文本直接拼入 FTS 表达式。1 或 2 个
Unicode scalar 的查询使用参数化 `instr(folded_name, folded_query)`；空查询返回零项，不隐式展示
最近文件。

caller guard 通过后、读取 ResultRegistry 或 FileIndex 前，后端必须验证：原始查询最多 1024 个
UTF-8 bytes、最多 255 个 Unicode scalar、不含 NUL；category 只能是十个固定枚举值；sort 只能是
`modifiedDesc` 或 `modifiedAsc`；`invocationId` 非空；`querySequence` 是大于零的 `u64`。任一失败
返回固定 `invalidFileQuery`，并证明 ResultRegistry、SQLite、扫描器和 watcher 访问次数均为零。折叠
发生在上述验证后、`begin_query` 前；折叠结果再限制为最多 4096 UTF-8 bytes 和 1024 Unicode scalar，
超限同样零 registry/数据库访问。

## 后端架构

后端新增一个受管的 `FileIndex`，沿用项目现有“一个受管状态、明确命令包装、blocking worker”模式，
不增加通用存储接口或插件抽象。

`FileIndex` 内部只有三个职责单一的部分：

1. SQLite 元数据存储与查询。
2. 一个后台 coordinator；它拥有 scanner/watcher join handles，串行启动扫描和执行 corruption recovery，
   自身不取得 DB-work reservation，也不属于恢复时等待的 worker 集合。
3. 每个固定磁盘的 Windows 变更通知。

文件搜索不得建立第二个结果注册表。它扩展并复用唯一受管 `ResultRegistry`：真实窗口 show 仍只调用
`on_show(invocationId)`，应用搜索和文件搜索都用同一
`begin_query(domain, invocationId, querySequence)` 和 `publish_if_latest`，其中 domain 只能是
`Application | File`。隐藏和所有 show/focus/执行成功路径都只消费统一 `clear_and_hide`，后者先
`hide_and_clear` 再隐藏窗口。`ResultAction` 只新增一个内部文件动作 variant；generation、active
invocation、latest query、request/result ID 和当前 action mapping 继续只有一份。

同一 registry mutex 内另加窄的 `invalidate_domain(File)`：它 checked increment 内部 query epoch，使
所有在途 File token 失效；只有 current mapping 属于 File 时才清空该 mapping，绝不改变 active
invocation，也不清除 Application mapping。该方法只服务数据库恢复和 lifecycle pause/terminal，不建立
第二套 generation 或结果表。

WebView 不连接数据库，也不持有可执行路径。扫描和数据库工作不得占用 Tauri 主线程。

### 元数据记录

每条记录至少包含：

- 稳定数据库 row ID。
- `volume_guid_path`、volume serial、filesystem name 组成的稳定卷身份。
- 相对卷根路径；绝对展示路径由当前已认证 mount point 组合，不作为记录身份。
- 原始名称、折叠算法 ID 和用于匹配的折叠名称。
- `file` 或 `directory` 类型。
- 固定分类。
- 文件大小；目录为 null。
- 修改时间。
- 所属 `committed_generation` 或 `candidate_generation`。

数据库位于当前 Windows 用户的应用数据目录，文件名固定为 `file-index.sqlite3`、
`file-index.sqlite3-wal`、`file-index.sqlite3-shm`，依赖现有目录权限，不额外使用 DPAPI，不上传。
数据库和 WAL/SHM 文件都属于敏感本地数据，不进入日志、诊断包或验证导出。

预览偏好复用现有 `SettingsStore`，但不经过会触发 hotkey/autostart 编排的 `save_settings`。持久设置
新增带 `#[serde(default = "default_file_preview_enabled")]` 的 `file_preview_enabled`，该函数固定返回
`true`；`SettingsView` 新增
`filePreviewEnabled`。切换调用一个 main-guarded `set_file_preview_preference`；
`FilePreviewPreferenceUpdate` DTO 精确为 `{ enabled: boolean }`，Tauri 外层参数名固定为 `preference`。
store 只原子更新该字段并保留其他设置。UI 一次只允许一个偏好保存；失败时
回滚到最后一次持久值并显示固定错误，不保留仅会话生效的不同状态。该命令在写入前取得现有
LifecycleCoordinator critical reservation，并把 reservation 移入 blocking worker，worker 从
AppHandle 重新取得唯一 SettingsStore；Cleaning/SystemEnding 时零 dispatch、零设置访问。它不注册
快捷键、不改变 autostart，也不调用通用 `save_settings`。既有 `UserSettingsUpdate` 不新增该字段；
既有设置保存从当前 Settings clone candidate，因此必须原样保留 `file_preview_enabled`。

### 命令和 DTO

命令和 DTO 冻结为：

- `search_files` 输入精确为 `{ query, category, sort, invocationId, querySequence }`。
- `FileSearchResponse` 精确包含 `{ requestId, indexRevision, total, status, items }`。
- `indexRevision` 和 `total` 都是无前导零的规范非负十进制字符串。前端把 revision 严格解析为范围在
  `0..=u64::MAX` 的 BigInt 后比较；空串、负号、前导零、非十进制或越界 payload 都使整个响应/事件
  零状态变化。revision 和 total 都不得转成 JavaScript number。
- 每个 item 精确包含 `{ resultId, name, kind, sizeBytes, modifiedUtc, fullPath }`；`sizeBytes` 是规范十进制
  字符串，目录为 null，避免把 `u64` 文件大小压入 JavaScript number。
- 既有 `execute_result` 继续作为唯一结果执行命令，输入仍只含 `{ requestId, resultId }`；其 outcome
  增加 `fileRevealRequested` 和 `folderOpenRequested`，不新增文件专用 execute command。
- 索引提交只发一个 `file-index://changed` 事件，payload 精确为 `{ revision, status }`；response 和
  event 共用同一个 status 枚举：`building | ready | partial | rebuilding | unavailable`。revision 同样是
  规范十进制字符串，不暴露内部错误和被拒绝目录列表。
- `set_file_preview_preference` 输入精确为 `{ preference: { enabled } }`，其中 enabled 是严格 boolean。

`category` 只能是 `all | folder | excel | word | ppt | pdf | image | video | audio | archive`；`sort`
只能是 `modifiedDesc | modifiedAsc`；`kind` 只能是 `file | folder`。`modifiedUtc` 是 UTC RFC 3339 字符串。
为保持 caller guard 是函数体第一步，`search_files` wire wrapper 将 category/sort 接收为 String，在 guard
通过后才解析为不可扩展的私有 Rust enum；不得把 serde enum 的 pre-body 拒绝冒充 guard 证据。以上名称
在实施计划中不得漂移。

所有新增 Tauri 命令都注入 `WebviewWindow`，并将既有精确 main-label caller guard 作为函数体第一步。
非 main caller 在读取 `FileIndex`、数据库、注册表或窗口状态前失败。

文件查询在输入验证后立即调用唯一 ResultRegistry 的 `begin_query(File, ...)`；无 token 时零数据库访问
并返回 stale/null。blocking 查询完成后，`publish_if_latest` 在同一 registry mutex 临界区分配新
request/result ID 并原子替换 action mapping；若 token 已被新 query、new show、hide 或
`invalidate_domain(File)` 淘汰，则丢弃整个响应。

取得 token 后，查询才取得 publication gate 检查 FileIndex availability 并尝试取得 DB-work reservation；
Normal 时在 gate 内建立 read snapshot，Rebuilding/Unavailable 时不取得连接并走固定空响应。查询从不
同时持有 ResultRegistry mutex 和 publication gate；该锁顺序必须由并发测试认证。

Pausing 查询保持同一顺序：输入验证后先取得 File token，再在 publication gate 内识别 pause，零
DB/path/new-worker；释放 FileIndex gate 后才用该 token 调用同一个 `publish_if_latest`。token 仍 latest 时
发布固定空 `status=unavailable,total="0",items=[]` 且不存在文件 action mapping；若 Cleaning invalidation、
新 query、hide 或 new show 已淘汰 token，则按通用合同丢弃整个响应并返回 stale/null。Cleaning 开始时的
`invalidate_domain(File)` 必须使 pause 前 token/mapping 失效；任何路径都不同时持有 registry mutex 与
FileIndex gate。

## 索引生命周期

### 启动

普通软件启动不打开数据库、不枚举磁盘、不创建 watcher。第一次有效 `/find` 调用才懒加载
`FileIndex`。

FileIndex lifecycle mode 初值精确为 Uninitialized。第一个合法文件查询在 publication gate 内取得唯一
lazy-init ownership；并发查询不得并行打开数据库或启动 worker，未取得 ownership 的查询只返回 building
空结果。非 fatal 的 ReturnRunning 回到 Uninitialized 后复用完全相同的单 owner 路径，不增加热重启分支。

- 若数据库不存在：创建 schema，立即显示空面板并开始首次扫描。
- 若数据库存在：立即查询旧索引，同时开始本次进程的后台校准。
- 一个进程内重复进入 `/find` 复用同一索引和 worker，不重复启动全盘扫描。

### 扫描范围

通过 Windows drive type API 只选择 `DRIVE_FIXED` mount point。每个 mount point 必须再由
`GetVolumeNameForVolumeMountPointW` 解析为 Volume GUID path，并由 `GetVolumeInformationW` 取得
volume serial 和 filesystem name。GUID path 规范化为大写 GUID 加结尾反斜杠，serial 存为 `u32`，
filesystem name 规范化为大写；三者组成 `VolumeIdentity` 并按规范化值精确比较。盘符只是可变化的
展示 mount point，不能作为数据库身份。网络、remote、removable、CD-ROM 和 RAM disk 不进入范围。
扫描使用当前用户权限，不提权。

同一 Volume GUID/serial 的多个盘符只扫描一次，选择 ordinal 最小的当前盘符作为展示 mount point。
每次 FileIndex 激活、每次查询、每次扫描开始及每次结果执行都重新认证当前 mount point 的 drive type
和 `VolumeIdentity`。数据库查询只 join 当前认证身份；盘符被不同固定卷复用时，旧卷记录立即停止展示
且不能执行。旧数据可保留为 detached，但只有原 `VolumeIdentity` 恢复后才重新进入查询。

任一条目或任一父组件具有 Hidden、System 或 ReparsePoint 属性时跳过该条目或整个子树。额外跳过
Windows 系统目录以及系统和当前用户的临时目录。实现必须从 Windows API 解析这些目录，不依赖
硬编码用户名或盘符。

实现还必须从 Tauri path API 取得并认证 UiPilot app-data 根，将其转换为当前 `VolumeIdentity` 下的
相对路径，并无条件排除整个子树。SQLite 数据库、`-wal`、`-shm` 三个精确文件名也必须单独进入
排除集合。扫描器和 watcher 在任何 metadata/DB 操作前应用同一排除函数，因数据库写入产生的事件
不得回流为索引更新或展示内部路径。app-data 根无法认证时 FileIndex 不启动。

扫描只读取目录项、类型、大小、修改时间和属性；不得打开目标文件读取内容。目录枚举和元数据读取
是本设计明确允许的新边界。

### generation、watcher 和无丢失交接

每个 `VolumeIdentity` 的 metadata 精确区分：

- `committed_generation`：唯一可长期查询的完整代次。
- `candidate_generation`：当前扫描的暂存代次；不存在时为 null。
- `next_generation`：持久化的下一个 checked `u64`，不得复用崩溃前 candidate 编号。
- `scan_state`：`idle | scanning | dirty | partial`。

候选记录存于独立 staging table，不通过改写 committed rows 来表示扫描中状态。进程启动时发现遗留
`candidate_generation/scanning`，说明上次在原子切换前退出：删除全部 candidate rows、把
`candidate_generation` 清为 null、保留原 committed generation、checked increment `next_generation`
后从 watcher 安装步骤重新扫描。若从未存在 committed generation，当前进程正在写入的唯一 candidate
可作为 status=building 的首次索引结果；它不会在失败或崩溃后冒充已提交索引，也不使用 partial 名称。
创建 candidate 时 `scan_state=scanning`；原子切换成功后清空 `candidate_generation`，无跳过子树则
`scan_state=idle/status=ready`，存在认证过的拒绝访问前缀则 `scan_state=partial/status=partial`；任何
不确定事件或交接失败设置 `scan_state=dirty`。这些名称在 schema、状态机和测试中必须一致。

每轮全量扫描与 watcher 的顺序固定如下：

1. 认证 `VolumeIdentity`、mount point、排除根和现有 committed generation。
2. 先创建最多 65,536 条结构化事件的有界缓冲、checked 单调 `eventSequence` 和只写入该缓冲的回调
   sink；sink 完全可调用后才打开 watcher handle，并 arm/确认第一条 `ReadDirectoryChangesW`（或等价
   native watch）已挂起。arm 期间同步到达的事件也必须进入缓冲；达到上限等同 watcher overflow。
   watcher 只使用一块固定、DWORD-aligned user buffer，不建 buffer pool。completion 必须先把 returned
   byte range 完整解析并恰好一次写入结构化缓冲，完成前不得复用该 user buffer；随后才用同一 buffer
   re-arm。两次调用之间的变更依赖该目录 handle 已关联的 Windows system buffer，并由下一次调用返回。
   返回 bytes=0、`ERROR_NOTIFY_ENUM_DIR`、解析失败或 re-arm 失败都视为 overflow，立即 dirty 并安排
   全卷校准，不猜测丢失事件。该语义以
   [Microsoft ReadDirectoryChangesW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)
   为准。watcher 首次 arm 未成功不得创建 candidate 或开始枚举。
3. watcher 已确认 armed 后创建新的 candidate generation，开始枚举并分批写入 staging table。
4. watcher 事件先取得 sequence 并进入缓冲；有 committed generation 时，同一事件批次也应用到
   committed rows 并递增 index revision，使活跃查询不必等待全量扫描。首次扫描时事件应用到当前
   candidate，并仍保留在缓冲供最终重放。
5. 枚举完成后取得交接 mutex，冻结一个 buffer cutoff；把 cutoff 以内事件按 sequence 重放到 candidate。
6. 在仍持有交接 mutex 时，用一个 SQLite write transaction 复制拒绝访问子树的旧 committed rows、
   应用最后事件、把 candidate 标记为新的 committed generation、删除旧 generation、递增 revision，
   再把 watcher 切换为 live mode。事务提交后才释放 mutex。
7. mutex 等待期间到达的事件随后在 live mode 应用到新 committed generation。由此 watcher 安装前、
   扫描期间、切换期间均没有未覆盖窗口。

任一 watcher arm/运行失败、溢出、event sequence 溢出、无法配对 rename 或 candidate transaction
失败，都禁止 candidate commit：停止该 watcher，清空事件缓冲，删除全部 candidate rows，将
`candidate_generation` 清为 null，保留旧 committed generation 并将卷标记 dirty。若清空了当前可见
的首次 candidate，或该 transition 改变 aggregate status，则按统一 publication 规则 checked increment
revision 并发事件；仍显示同一 committed 结果且 status 仍为 building 时不虚增。没有 committed
generation 时同样先把 provisional 结果清为空；不得保留失败 candidate、不得创建隐含第三代。
`partial` 只表示成功提交但含拒绝访问子树的 committed generation，不表示失败或仍在写入的 candidate。

每卷只允许一个 `calibration=idle | pending(deadline, runtime_epoch) | running(runtime_epoch)`。失败时 saturating increment
`consecutive_failures`，由同一个 FileIndex coordinator 用 monotonic clock 安排
`min(60s, 1s * 2^min(consecutive_failures-1, 6))` 的确定退避，即 1/2/4/8/16/32/60 秒；pending/running
期间的重复失败或事件只合并为 dirty，不新增 timer 或并行扫描。只有 watcher 成功 arm 且 candidate
成功原子 commit 后才把计数和 deadline 重置。应用关闭或卷 detach 会取消 pending；同一
`VolumeIdentity` 重新激活时若无 running 只合并为一个 pending：计数为 0 时立即运行，否则按当前退避
重新计算 deadline，反复 detach/attach 不得绕过延迟。新的 launcher `/find` invocation 也不绕过退避。
owner 精确等于当前 FileIndex `runtime_epoch`；进程初值为 0，每次 corruption recovery 或 Cleaning
ownership teardown 开始都在 publication gate 内 checked increment，溢出按统一规则设置 fatal latch 并
进入 unavailable。该 coordinator
只维护每卷一个 deadline 和一个 Condvar，不抽取通用 scheduler。

目录 rename/delete 不逐条猜测子项：可靠 rename 删除旧相对路径前缀并对新前缀做有界子树重扫；
delete 删除整个旧前缀；无法配对则 dirty 全卷校准。目录 Hidden/System/ReparsePoint 属性变为禁止时
删除整个前缀，变为允许时重扫该子树。文件属性变化只重读该文件元数据。相同规则用于 live 应用和
candidate replay，并必须覆盖父目录变化影响整棵子树的测试。

初次和校准扫描一次只处理一个磁盘，并使用低优先级 blocking worker。应用退出时请求停止，不为等待
扫描完成而延迟生命周期退出。首版不使用 USN Journal。

### 活跃查询刷新

Task 7 client 在本进程第一次进入 `/find` 时先成功注册唯一 `file-index://changed` listener，再发第一条
`search_files`；注册失败时不触发 FileIndex 懒加载，显示固定错误。listener 在 core destroy 时精确
unlisten 一次。事件 payload 必须通过精确 key/type/status 解析；revision 只接受规范 `u64` 十进制并
在 core 内解析为 BigInt。解析失败使整个 payload 零状态变化；只有严格大于 `latestSeenRevision` 的
事件可更新 owner并安排刷新，相等或更旧 event 同样零状态变化。response 使用相同的严格解析，但
比较规则不同且固定：当前 query owner 的 response revision 小于其捕获的 required/latest seen revision
时 stale；等于或大于时可提交并更新 max。由 event 触发的查询因此可以接受同 revision 的 response。

`FileIndex` 在进程内持有一个全局 checked `u64 index_revision_high_water`，从有效数据库 metadata 的
revision 初始化；首次新库从 0 开始。每个会改变对外可见结果或 aggregate status 的 transaction/transition
都必须取得 publication gate，在同一线性化点 checked increment 高水位、持久化新值（数据库可用时）
并计算 status；提交后才发送一个不含路径的 `{ revision, status }` 事件。只改变内部字段、但结果和
aggregate status 都未变化的 transition 不递增、不发事件。任何 increment overflow 在同一 gate 内设置
fatal latch 并进入永久 `unavailable`，不得回绕、删除数据库或继续 publish。

所有卷映射为唯一 wire status，response 和 event 共用同一计算函数及以下优先级：FileIndex fatal 为
`unavailable`；corruption recovery 为 `rebuilding`；否则任一卷 `scanning`、`dirty` 或尚无 committed
generation 为 `building`；否则任一成功 committed generation 的 `scan_state=partial` 为 `partial`；
其余为 `ready`。watcher overflow/dirty 因而显示统一“索引正在建立或校准”，不增加 wire enum。
扫描批次最多每秒提交一次；前端对事件做 250 ms trailing debounce，并设置一秒 maximum wait，避免
持续扫描永远推迟刷新。

事件处理只在当前 view 仍为 `/find` 且捕获的 view epoch、`invocationId`、查询文本、分类、排序都未变
时生效。刷新由 Task 7 core 递增同一个 `querySequence` 并重新调用文件查询，不使用后台私有 sequence。
新编辑、新 invocation、hide 或更新的 revision 使旧 timer/response 零效。相等或更旧 revision 不重复
查询；每个合法响应用 BigInt 比较并把 `latestSeenRevision` 更新为当前值与响应值的较大者。响应
revision 小于当前 event owner 时，其显示结果也视为 stale 并立即由已排队刷新取代。刷新沿用当前
launcher invocation 的 sequence：checked increment 当前 Task 7 `querySequence` 后调用 `search_files`，
不得从 0 或 1 重新建立文件专用 sequence；只有新的合法 `launcher://shown` 才重置为 0。

最新响应通过唯一 ResultRegistry 的 `publish_if_latest` 原子替换 request/result action mapping 后才返回
前端。前端按完整展示路径保留当前选择；该路径仍在新 200 项内则保持，否则选择第一项，无结果则
清空选择。刷新与 Enter 竞态按 registry 线性化：若旧 request 先 resolve，后端只执行其重新认证后的
原动作；若新 publish 先替换 mapping，旧 Enter 返回 stale 且零 Shell 调用。任何顺序都不能把旧
resultId 映射到新文件。

### SQLite 恢复

使用事务和 WAL。懒加载关键路径只认证 app-data/文件类型，打开数据库，并检查精确
`PRAGMA application_id=1430868038`（ASCII `UIPF` / `0x55495046`）、`PRAGMA user_version=1` 和必需
schema objects；它不运行 `integrity_check`，因此已有索引的首次结果 P95 不依赖全库扫描。
schema/version 不匹配时索引是可再生缓存，首版直接重建，不实现迁移。

数据库 metadata 保存 `clean_close` 和 `last_integrity_check_utc`。FileIndex 每次首次打开或 ReturnRunning 后
lazy reopen 时，必须在同一个受管 transaction 内先读取并在进程内缓存 `prior_clean_close` 与
`prior_last_integrity_check_utc`，再写 `clean_close=false` 并提交；不得先覆盖后回读。首次 `/find` 的第一份
响应返回后，完整检查条件只能使用该 prior snapshot 加本次 `created_schema`：prior 不是 clean close、prior
完整检查超过七天或本次创建过 schema 时，才在低优先级独立连接执行一次完整
`PRAGMA integrity_check`。同一进程最多一次且不能阻塞查询或扫描 writer；ReturnRunning lazy reopen 不得
触发第二次。

`clean_close=true` 只是 FileIndex 自身的 fail-closed 优化证据。既有 pause coordinator 已 signal cancel、
全部 DB-work/reservation 归零并关闭/释放全部普通 worker/read/write/integrity connection 后，它取得
publication gate、Acquire-load phase 非 Terminal 并保留唯一 coordinator-owned 专用 connection；成功检查
是该 clean-close attempt 相对 FileIndex handler/admission 的线性化点。释放 gate 后执行短 transaction，
Terminal 可并发且绝不等待：事务在进程退出前原子提交则 durable true 合法，否则由 SQLite rollback/未提交
保持 false，两者数据安全等价。随后关闭最后一个 connection，不把 handle 析构成功当作第二个 marker，
也不把 FileIndex 纳入新的通用 cleanup 框架。phase 在 attempt 准入前已为 Terminal、五秒收口 deadline
到达或前置步骤失败时不启动 transaction，磁盘保持 false。
任一 SQLite 操作返回 `CORRUPT`/`NOTADB`，或后台
完整检查结果不精确为单行 `ok` 时，进入相同重建流程。只有结果精确为单行 `ok` 后，才在受管 SQLite
write transaction 中把 `last_integrity_check_utc` 更新为本次检查完成的 UTC 并提交；transaction 失败
不得更新时间。只有该失败本身是 `CORRUPT`/`NOTADB` 才请求 corruption recovery；BUSY/FULL/IOERR 等
走既有固定 DB 错误并由下一进程重新尝试检查，不删除索引。成功提交不改变结果/status/revision。

每个 query/scanner/watcher/writer/integrity-check 数据库任务在取得连接前必须持有 FileIndex 内部 RAII
DB-work reservation；文件执行从 registry resolve 后也属于该集合。FileIndex 用 mutex/Condvar 跟踪计数
和是否接受新任务。任何任务发现损坏时只调用窄的 `request_recovery`，不得自己 wait/join/delete：

FileIndex 的 lifecycle mode 精确为 `Active | Pausing(attempt_epoch, resume_requested) | Uninitialized |
Terminal`。只有 SystemEnding 或已决定退出的 Clean/`CleanDecision::Exit` 能不可逆切到 Terminal；Cleaning
只是 checked `attempt_epoch` 拥有的可撤销 pause，不能设置永久 shutdown。`attempt_epoch` 由既有
LifecycleCoordinator 的同一个 ExitGate 在创建新 tray clean attempt 时 checked increment，并随
Cleaning/ReturnRunning handler 传递；它是严格单调的 `u64`，只做普通大小比较而不使用回绕序号算法，
用于匹配或向前接管同一次可撤销 pause。若该 increment 溢出，
LifecycleCoordinator 仍按既有逻辑处理退出，但 FileIndex 不创建新 Pausing owner：handler 在 publication
gate 内关闭 admission、设置 `fatal_unavailable=true`、失效 File domain 并 signal 当前 worker/recovery
停止；ReturnRunning 不得 reopen，同一进程后续 `/find` 固定 unavailable。唯一 checked
`runtime_epoch` 绑定 query/action/calibration ownership；corruption recovery 和每个新的 Cleaning teardown
各递增一次。recovery cancel 绑定启动它的 runtime epoch，Cleaning 通过递增 runtime epoch 使旧 recovery
失效，不增加第三个 action counter。

`fatal_unavailable` 是与 lifecycle mode 正交的 process-lifetime latch，初值 false。统一规则是：任何
FileIndex 内部失败或 checked overflow 实际把内部 availability 转为 Unavailable 的路径，都必须在同一个
publication gate 临界区同时设置 `fatal_unavailable=true`，包括 recovery 收口超时、create/schema/seed、
reopen 清理/卷认证/revision 持久化失败以及 revision/runtime/attempt epoch overflow；Cleaning/pause/
ReturnRunning 绝不能清除。Pausing 查询只临时 publish wire `status=unavailable`，不改变内部 availability
或该 latch。latch=true 时 admission 永久关闭、wire status 仍使用既有 unavailable，只有新进程可以重试，
不新增 wire enum。

Cleaning start handler 在释放 lifecycle mutex 后取得 FileIndex publication gate，并在任何 mutation 前
Acquire-load atomic phase/attempt mirrors；只有 phase=Cleaning 且 mirror attempt 精确等于参数
`attempt_epoch` 时，才把 Active 原子切到 `Pausing(attempt_epoch, false)`、checked increment runtime epoch、
关闭 admission、把所有 pending 置 idle、调用 `invalidate_domain(File)`，并 signal 全部 running 与旧
runtime epoch 的 recovery cancel。该 handler 可从 Active（无论 availability 为 Normal/Rebuilding/
Unavailable）或 Uninitialized 进入。若 start(B) 在 phase=Cleaning、mirror attempt=B 时看到现有
`Pausing(A, *)`：B==A 只观察；B>A 时只把同一个 pause 原子 retag 为 `Pausing(B, false)`，复用正在进行或
已完成的 teardown、cancel、runtime epoch 和唯一 coordinator，不再次 increment/invalidate/close，也不
启动 worker；B<A 是旧 handler 并 no-op。若 phase 已回 Running，迟到的 start handler 是 no-op；若
phase=Terminal，绝不覆盖 Terminal；若 mirror attempt 与参数不匹配，同样 no-op。唯一 FileIndex coordinator
随后 quiesce/join worker、
关闭 watcher/handles/buffers 和 SQLite connections；这不是数据库损坏，不删除数据库，也不改变
`fatal_unavailable`。

LifecycleCoordinator 因 marker failed 或 timeout 返回 Running 后，matching ReturnRunning handler 先释放
lifecycle mutex，再取得同一个 publication gate。ReturnRunning(B) 只有在 Acquire-load phase=Running 且
mirror attempt=B 时可处理现有 pause：若当前为 `Pausing(A, *)` 且 B>=A，则原子 retag/保持 B 并设置
`resume_requested=true`，从而覆盖 B start 完全错过的调度；B<A 是旧 handler 并 no-op。不存在 Pausing 时
同样 no-op，Terminal 始终先胜。若 pause 已完成 quiesce/close，则立即切到 Uninitialized，否则由
coordinator 完成 pause 后切换。只有 `fatal_unavailable=false` 才进入 Uninitialized；latch=true 时回到
`mode=Active, availability=Unavailable` 且 admission closed，零 lazy init。两条路径都保持零 pending、
零自动 worker；非 fatal 时下一次
合法 `/find` 才通过既有 lazy-init 路径重新打开/认证数据库并精确启动一次校准。

pause coordinator 只等待 cancel 后的 worker 收口，不等待全盘扫描自然完成：signal watcher/scanner/writer
cancel，并以固定五秒 monotonic deadline 等待旧 handles/connections/reservations 全部归零。ReturnRunning
可以先返回应用；只要旧 ownership 尚未归零，mode 就保持 Pausing，新的 `/find` 返回固定
empty `status=unavailable,total="0",items=[]` 且零 DB/new-worker，绝不能并行打开新数据库。deadline 到达仍未归零时也
不创建第二套 ownership；唯一 coordinator 继续通过现有 Condvar 被动观察，实际归零且 matching
`resume_requested=true` 后才按同一 fatal latch 规则原子切到 Uninitialized 或 admission-closed Unavailable。
该等待不阻塞 lifecycle 退出或主线程。健康数据库只有在上述 ownership 全部归零且 phase 尚非 Terminal
时才可尝试 clean-close 流程；若流程在 ReturnRunning 后完成，仍先关闭并进入 Uninitialized，下一次合法
`/find` lazy open 必须立即重新提交 `clean_close=false`。

SystemEnding 或 Clean/Exit handler 在释放 lifecycle mutex 后取得 publication gate，原子切到 Terminal、
关闭 admission、取消 pending 并 signal running/recovery/pause cancel。Terminal 不可清；任何旧
ReturnRunning、recovery completion 或 lazy init 都不能 reopen。锁序固定为先完成并释放 lifecycle mutex，
再取得 FileIndex gate；任何路径都不得同时持有两把锁。terminal 退出只 signal cancel，不等待扫描自然
结束；系统/应用退出不因 FileIndex teardown 延迟。

为使该锁序可实现，LifecycleCoordinator 只新增两个供 FileIndex 读取的窄原子 mirror：精确编码
`Running | Cleaning | Terminal` 的 `AtomicU8 lifecycle_phase`，以及 `AtomicU64 lifecycle_attempt_epoch`，
不建立通用观察框架。初值分别为 Running 和 0；创建新 Cleaning 时仍在既有 ExitGate mutex 内先把 checked
attempt 写入 attempt mirror，再用 Release store 发布 Cleaning；其他 state transition 在释放 mutex 前用
Release store 同步 phase：ExitState::Running 映射 Running，ExitState::Clean/SystemEnding 映射 Terminal。
FileIndex 在 publication gate 内先 Acquire-load phase，再 Acquire-load attempt，绝不调用 `observe_exit` 或
任何会锁 ExitGate 的方法。Cleaning phase 的 Acquire 同步保证随后读取到对应 attempt。
因此 lifecycle state 已变化但后续 FileIndex handler 尚未取得 gate 的窗口中，所有 FileIndex admission
都会因 phase 非 Running 而失败闭合；ReturnRunning 虽先 mirror=Running，FileIndex mode 仍为 Pausing，
必须等 matching handler 完成且旧 ownership 归零后才能 lazy init。

所有 admission 统一经过一个 crate-private `admit_locked(kind, expected_runtime_epoch)` eligibility predicate，
且只在已持有 publication gate 时调用；调用方只能在它返回成功后的同一临界区创建对应 owner/reservation。
kind 只允许 `LazyInit | DbWork | Execution`。helper 第一项必须 Acquire-load `lifecycle_phase` 并要求
Running，随后检查 lifecycle mode、fatal latch、admission 和 expected runtime epoch：LazyInit 只接受非
fatal Uninitialized；DbWork 只接受 Active/open 且 task epoch 等于当前 runtime epoch；Execution 还要求
action runtime epoch 等于当前值。query snapshot、scanner、watcher、writer、integrity 使用 DbWork；
OpenIndexedPath 使用 Execution；首次打开使用 LazyInit。coordinator 启动任务也必须先走该 helper；持有
既有 DbWork reservation 的 corruption 报告者在 recovery transition 前以同一 helper 和自身 task epoch
重新验证，但不得为该 transition 重复增加 reservation。任一拒绝都必须零新 reservation/owner、DB/path/
Shell，调用方不得复制或绕过这些判断。

1. 只有在 publication gate 内 `admit_locked(DbWork, reporter_runtime_epoch)` 成功时，第一个报告者才把
   FileIndex 从 Normal 原子切到 Rebuilding、关闭新
   query/execution/scanner/watcher/writer/integrity reservation admission、checked increment revision、
   checked increment `runtime_epoch`、把当前值保存为 active recovery owner，并设置该 runtime epoch 的
   cancel 来停止旧 worker，再形成 `rebuilding` 事件。重复报告只观察 Rebuilding/Unavailable 后立即
   返回；Pausing/Uninitialized/Terminal 不启动 recovery。runtime epoch overflow 设置 fatal latch。
2. 第一个报告者仍持有 gate 时调用唯一 ResultRegistry 的 `invalidate_domain(File)`，使此前所有在途
   file query token 失效，并仅在 current mapping 属于 File 时清空它；Application token/mapping 和
   launcher invocation 不受影响。随后 signal 唯一 FileIndex coordinator，释放 gate、发送事件并从
   当前 DB task 返回，使自身 RAII reservation drop；报告者绝不等待或 join 自己。
3. 不属于 DB-work/被等待集合的 coordinator 被唤醒后发送停止信号，等待 scanner、watcher、writer、
   reader、file execution 和 integrity worker 全部退出且 reservation=0，使用固定五秒 monotonic deadline。
   deadline 到达时必须先在 publication gate 内确认 active recovery owner/runtime epoch 和 mode=Active，
   并 Acquire-load `lifecycle_phase=Running`，才可把它归类为纯 corruption recovery 超时并按统一规则设置
   fatal latch、转为 `unavailable`；保持 admission 关闭、不关闭连接、不删文件。若 phase 已为 Cleaning/
   Terminal，或 matching pause/Terminal 已使 owner/mode 不匹配，则立即停止 recovery、交还对应 lifecycle
   teardown，零 fatal 误报。
4. 只有全部 worker join 且 reservation=0 后，coordinator 才能继续；在关闭连接、删除文件、创建新库
   每个破坏性边界前都重新取得 publication gate，要求 active recovery owner 精确等于当前 runtime epoch、
   lifecycle mode=Active，且 Acquire-load `lifecycle_phase=Running`。每次成功检查是紧随其后的单个操作的
   线性化点；close、每个 delete、create、schema 和 seed 不得共用一次旧检查。当前 recovery cancel 必然
   仍为 true，因为它负责停止本轮旧 worker，不能把该 expected cancel 当作 reopen veto。任一检查变为
   runtime epoch mismatch、Pausing、Terminal 或 phase 非 Running 时立即停止 recovery，把 ownership 交给
   lifecycle teardown，且从该点起零后续 close/delete/create、零 fatal 误报。条件保持时才关闭 SQLite
   connections，认证 app-data 根和三个精确文件名后逐个删除损坏数据库及现存 WAL/SHM；索引是可再生
   缓存，不保留敏感路径损坏副本。
5. coordinator 在 admission 仍关闭时创建空数据库/schema，并把 metadata revision 初始化为进程内当前
   高水位，不得从 0 重新开始；create、schema、seed 各自使用步骤 4 的 owner/mode/Acquire-phase 边界检查。
   任一步失败都保持 admission 关闭，并在同一 gate 内设置 fatal latch 后转为 `unavailable`；只有后续进程
   的全新 FileIndex 初始化可以重新尝试恢复。
6. 新库成功且旧 worker 已全部 join 后，coordinator 重新认证当前 VolumeIdentity 集合并取得 publication
   gate。它必须同时确认 active recovery owner 精确等于当前 runtime epoch、没有更高优先级 pause/Terminal
   ownership、lifecycle mode=Active，且 `lifecycle_phase` Acquire-load 仍为 Running。条件成立时才清除当前
   runtime epoch 的 expected recovery cancel；把全部旧 watcher handles、
   user/structured buffers 和 candidate runtime ownership 置空；丢弃已 detach 卷的 runtime state；把
   每个当前卷的 calibration 重置为 idle，并为新库首次校准把 `consecutive_failures=0`、deadline=null。
   随后才原子切回 Normal、开放全部 DB-work admission，以 aggregate `building` checked increment
   revision 并持久化，并在释放同一个 gate 前把每个当前卷精确设置为
   `pending(now, runtime_epoch)`。任一清理、卷认证、revision 或持久化失败都不得部分切换：
   本次 recovery cancel 保持、admission 保持关闭、零新任务，并在同一 gate 内设置 fatal latch 后转为
   `unavailable`。
   若 Cleaning 已把 mode 切到 Pausing 或使 active recovery owner/runtime epoch 不匹配，则不属于数据库
   失败：保持 admission 关闭、零 building 事件、零新 worker，coordinator 关闭任何健康新库并把剩余
   ownership 交给 matching pause teardown；ReturnRunning 后按 pause 合同进入非 fatal Uninitialized 或
   fatal Unavailable。若 mode=Terminal，则同样关闭并退出，Terminal 胜出。两者都不得误报 unavailable。
7. 释放 gate 并发送 building 事件后，coordinator 只消费已经存在的 pending，不再 enqueue。每次启动前
   重新取得 publication gate，并调用 `admit_locked(DbWork, pending_runtime_epoch)`；成功后还要求
   availability=Normal，随后才原子改为 running 并创建 reservation；任一不匹配都取消该
   pending。正常路径再建立 buffer/sink、arm watcher、创建 candidate 并扫描。每卷仍最多一个
   pending/running，且全局仍一次只扫描一个卷；第一个新库 response/event 必须严格大于恢复前前端
   可见的最高 revision，至此 corruption recovery 完成。watcher/scan/candidate failure 只进入既有
   per-volume backoff，不把已健康创建的新库再次判为 unavailable。

Rebuilding/Unavailable 期间，合法 `search_files` 仍完成 caller guard、输入验证和
`begin_query(File, ...)`，但零 SQLite/旧索引访问，并 publish 精确的空响应：`total="0"`、`items=[]`、
status 为当前 `rebuilding` 或 `unavailable`。这会原子替换文件 action mapping，使 Enter 零 Shell 调用。
查询输入、分类、排序和应用搜索保持响应；不得声称文件查询或旧索引仍可用。

删除数据库不会重置进程内 revision 高水位。若任一所需 checked increment 在 `u64::MAX` 溢出，立即
关闭 DB admission、`invalidate_domain(File)` 并使当前文件面板走统一 `clear_and_hide`；不发相等/回绕
revision，不删库或继续 publish。不得在无法认证 app-data 根时删除或移动任意文件。

## 结果执行

唯一 `execute_result` 保持既有 main caller guard 和 ResultRegistry resolve，然后只按私有 `ResultAction`
做穷举 match，不抽取通用 action pipeline：

- `LaunchApplication` 分支继续走 Task 5 现有 `apps::execute_application`、立即 `clear_and_hide`、
  `ValidationStore::record`、`SettingsStore::increment_use_count` 及
  `validationFailed > settingsFailed > windowFailed` 错误优先级；现有 app 行为和测试不得因文件功能改变。
- `OpenIndexedPath` 分支只走下述 FileExecutionReservation、DB/path/native Shell 流程。Shell 成功后调用
  同一个 `clear_and_hide` 精确一次；Shell/认证失败保留窗口。该分支对 ValidationStore、SettingsStore、
  AppCache 必须零访问，不创建应用 validation/use-count，也不得把 full path 写入 ValidationEvent 或其他事件。

每次成功 publish 由唯一 ResultRegistry 生成新的 request ID，并在其现有 current action mapping 中保存
最多 200 个 `OpenIndexedPath { runtime_epoch, volume_identity, row_id, relative_path, kind }`。新查询、任何新 launcher
invocation、隐藏窗口或统一 `clear_and_hide` 都通过既有 generation 使旧请求失效；FileIndex 不保存
第二份可执行 mapping。action 的 kind 是后端私有 `File | Directory`；wire DTO 只把 Directory 映射为
既定的 `folder`，前端不能回传 kind。

真实 `ResultRegistry::resolve` 会 clone action 后释放 registry mutex，因此 mapping invalidation 不能撤销
已经 resolve 的 action。文件执行固定按以下锁顺序线性化：

1. resolve request/result IDs 并释放 ResultRegistry mutex。
2. 取得 FileIndex publication gate 并调用 `admit_locked(Execution, action.runtime_epoch)`；只有 helper 成功且
   availability=Normal 时，才在同一 gate 临界区取得一个计入恢复等待集合的 RAII
   `FileExecutionReservation`，随后释放 gate。phase 非 Running、epoch 或其他 admission 条件不匹配都固定
   stale/unavailable 且零 DB/path/Shell。
3. reservation 存续期间按 row_id 从 SQLite 重读记录。lookup 缺失，或记录的 `volume_identity`、
   `relative_path`、`kind` 任一与 cloned `OpenIndexedPath` 不精确相等，都返回 stale 且零 path/Shell；
   只有四字段完全绑定才继续。由此 row_id 删除后复用不能把旧 action 指向新路径，无需 AUTOINCREMENT
   或持久 file ID。只有 `CORRUPT`/`NOTADB` 调用 `request_recovery`；其他 DB error 返回固定错误，均为
   零文件系统/Shell。
4. 完成下述逐组件 pin 和 Shell dispatch；Shell 返回后才释放 handles/reservation。最后再调用统一
   `clear_and_hide`，整个过程中不同时持有 registry mutex、publication gate、SQLite connection 或
   window lock。

若执行先取得 reservation，它线性化在 recovery 之前并允许在恢复删库前完成；recovery 关闭 admission
后等待该 reservation。若 recovery 先关闭 admission，即使 action 已被 resolve，执行也返回固定
`searchUnavailable`，且 DB/path/Shell 访问计数均为零。`invalidate_domain(File)` 只处理未 resolve mapping
和 query token，不能替代这条 execution admission。

执行时后端重新从 SQLite 和文件系统解析结果，并验证：

- 请求和结果仍有效。
- 路径仍存在且类型未变化。
- 当前 mount point 仍为 `DRIVE_FIXED`，重新解析出的 Volume GUID/serial/filesystem name 与 action
  中的 `volume_identity` 精确一致。
- 从根到最终组件没有 reparse point。

native adapter 从已认证 volume root 开始按相对路径逐组件调用 `CreateFileW(OPEN_EXISTING)`。每个 handle
固定 `dwDesiredAccess=0`，share mode 精确为 `FILE_SHARE_READ | FILE_SHARE_WRITE` 且不含
`FILE_SHARE_DELETE`；全部组件使用 `FILE_FLAG_OPEN_REPARSE_POINT`，目录另加
`FILE_FLAG_BACKUP_SEMANTICS`。每次 open 后立即通过
`FileAttributeTagInfo`、`GetFinalPathNameByHandleW` 和 volume API 认证 reparse/type、当前
VolumeIdentity 以及规范化 Volume GUID + 相对前缀。任何组件无法 open/pin、sharing conflict、身份或
路径不一致都失败闭合。share/access 语义以
[Microsoft CreateFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea)
为准。

所有祖先和最终组件 handles 必须一直存活到 Shell dispatch 返回；缺少 `FILE_SHARE_DELETE` 只阻止其间
rename/delete 路径替换，不阻止正常内容写入或 writable mapping。路径语义只要求执行 pin 时该路径指向
同卷、同类型、非 reparse 的对象：某组件 pin 前发生的
同卷同类型普通对象替换可作为当前路径接受；reparse/type/volume/prefix 变化必须在后续认证中拒绝且
零 Shell。组件 pin 后的替换必须被 Windows sharing 拒绝。无法证明 pin 生效的 filesystem/volume
不执行结果，不引入持久 file-ID 身份模型。

文件使用 `SHOpenFolderAndSelectItems` 在 Explorer 中定位；文件夹使用 Windows Shell open 打开。
两条路径都只消费后端注册表解析的可信路径，不接受工作目录、参数、verb 或任意前端 payload。

结果已删除、类型变化、磁盘不可用或 Shell 调用失败时，返回固定错误，移除或刷新该索引项，不执行
替代命令。文件/文件夹 Shell 动作成功后立即调用同一个 `clear_and_hide` 精确一次；Shell 动作失败时
不清空当前结果、不隐藏窗口，便于显示固定错误。

## 错误与进度

错误分为固定用户状态：

- 索引正在建立或校准。
- 部分位置无法访问。
- 索引正在重建。
- 搜索暂不可用。
- 查询无效。
- 无法保存文件预览设置。
- 文件已不存在。
- 无法在资源管理器中打开。

单个目录拒绝访问不会停止其他磁盘或目录。数据库不可用会停止查询但不影响应用启动搜索。错误消息
不得包含内部 SQL、原始 OS 错误、完整失败目录列表或任意文件内容。

## 安全与隐私边界

- 完整路径是明确新增的敏感 DTO，只能在 main WebView 本地显示。
- 完整路径不得进入日志、验证事件、验证导出、崩溃 marker、遥测或网络。
- WebView 不能提交路径、PID、HWND、Shell verb、参数或工作目录。
- 所有查询输入在 caller guard 后、registry/FileIndex 前执行长度和枚举验证；拒绝路径不得触达数据库。
- Volume GUID/serial/filesystem name 和相对路径只存在于数据库及私有 `ResultAction`，不进入前端 DTO。
- app-data 根和 DB/WAL/SHM 必须同时由 scanner 与 watcher 排除，不能依赖目录恰好带 Hidden/System 属性。
- 数据库路径、SQLite 依赖、Windows API features、新命令、handler、capability 和前端 DTO 都属于新的
  trust inputs，必须逐项列入实施计划 allowlist。
- security probe、probe config、现有安全脚本和 ReleaseSecurityBlocked 状态不得被 `/find` 顺带修改。
- 所有路径结构解析、UTF-8/UTF-16 转换、reparse 检查和 native 调用都失败闭合。

## 性能门禁

使用一百万条合成元数据和固定查询集验证：

- `/find` Enter 到面板可见的 P95 不超过 100 ms。
- 已有索引时，首次结果 P95 不超过 250 ms。
- 后续名称查询 P95 不超过 100 ms。
- 固定查询集必须单列高命中的 1-scalar 和 2-scalar 查询，每条至少匹配语料的 30%；分别运行 30 次
  新进程/新连接 cold 查询和同连接 100 次 warm 查询。cold P95 必须不超过 250 ms，warm P95 必须
  不超过 100 ms，且每次验证同 snapshot count/前 200 项。不得只测 trigram 路径来规避短查询。
- 正常文件通知链路从 watcher 收到事件到最新活跃查询 publish 完成必须在五秒内。
- 正常扫描/校准期间，输入、分类和 committed/provisional candidate 查询保持可用。Rebuilding/Unavailable
  期间只要求输入、分类、排序、状态显示和应用搜索响应；文件查询必须按恢复合同返回空状态且零旧库访问。

首次全盘扫描不设置与磁盘无关的虚假总时限。门禁记录磁盘类型、文件数、耗时、数据库大小和峰值
工作集。任一 1/2-scalar 门禁失败即设计 No-Go；不得在实现阶段临时提高最小查询长度。用实测决定
未来是否需要 USN Journal；首版不为该可能性预建抽象。

## 测试与证据

### 自动化

- 纯分类和匹配测试覆盖扩展名、全部固定 Unicode 15.1 folding 样例、算法 ID 漂移重建、特殊字符、
  排序 tie-break 和 200 项上限。
- SQLite 测试覆盖 schema、publication gate 下同一线性化 read snapshot 的 status/revision/count/items、
  批量事务、committed/candidate 两代、首次 candidate 失败清空、候选崩溃清理、partial 仅来自成功提交、
  损坏和后台 integrity-check 触发条件；精确单行 ok 成功提交完成时间，non-ok 请求一次 recovery，
  timestamp transaction 的普通失败不更新时间也不删库，只有 CORRUPT/NOTADB 进入 recovery。
- 扫描测试使用受控临时树，覆盖 Hidden/System/ReparsePoint、app-data/DB 排除、拒绝访问和停止请求。
- watcher 测试覆盖 sink/buffer-before-arm、arm 成功瞬间到达的事件进入缓冲、completed user buffer 在
  完整解析/入队前不 re-arm、between-call 事件由下一调用恰好返回一次、bytes=0/ERROR_NOTIFY_ENUM_DIR、
  扫描中缓冲、cutoff 重放、原子切换、切换期间事件、新增/删除、目录 rename、属性切换、整棵子树、
  overflow 和 dirty 重扫。
- calibration 测试用 fake monotonic clock 持续注入 arm/overflow/transaction failure：0..63 秒内每卷
  最多启动 7 次（0/1/3/7/15/31/63），任意时刻 running 不超过 1，重复事件只合并一个 pending；关闭
  取消 deadline，卷重新激活只触发一次，恢复后 watcher arm + candidate commit 会重置下一次退避为 1 秒。
- 卷测试覆盖相同盘符换成不同 GUID/serial、同卷多 mount point、detached/恢复和执行前身份变化。
- 命令测试覆盖 main caller guard、查询 byte/scalar/folded 上限、非法枚举在零 registry/DB 访问时拒绝、
  同一 invocation 先做应用查询再以非零连续 sequence 进入空 `/find`、只有 new shown 重置 sequence、
  invocation/querySequence 淘汰、伪造/过期 ID、刷新/Enter 线性化和统一 clear-and-hide。`execute_result`
  分支测试分别认证：LaunchApplication 保持既有 app/validation/use-count/error-priority 调用次数；
  OpenIndexedPath 成功只调用一次 Shell 和一次 clear-and-hide，失败不 hide，两者都对
  ValidationStore/SettingsStore/AppCache 零访问且事件中零 full path。
- revision/status 测试覆盖规范 BigInt 解析、malformed/越界、event 仅 newer 生效、当前 response
  equal/newer 可提交而 older stale、结果和 status-only transition 的单调递增、多卷优先级、dirty 到
  building 映射，以及高 revision 下删库重建后首个 response/event 仍严格变大；checked overflow 必须
  清空文件 domain 并失败闭合。
- 恢复测试分别由 query、writer、integrity worker 报告 corruption，并覆盖重复报告：只有一个恢复
  winner/coordinator，报告 worker 返回并 drop 自身 reservation、无 self-join；先关闭 admission 和
  `invalidate_domain(File)`，旧 file publish 零效、Application mapping 保留，全部 reservation/join 后才
  关闭/删除，五秒超时不删库，Rebuilding/Unavailable 查询零 SQLite 访问及 empty response；reopen gate
  前 query/scanner 零 DB，成功 create/schema/seed 后首个 query 可读空新库，正常 calibration 可取得
  reservation。旧 worker 必须先观察本次 recovery cancel 并 join；reopen 原子清当前 runtime epoch cancel、旧
  handles/buffers/ownership 和 running/pending，并在 gate 内为每卷恰好建立一个带当前 runtime epoch 的
  pending(now)；新 worker 观察 cancel=false，全局无并行扫描。create/schema/seed 或 reopen 失败保持
  cancel=true、同一 gate 内 fatal latch=true 且零新任务。lifecycle 测试覆盖：初始 Uninitialized 只有在
  phase=Running 时允许唯一 lazy owner；Cleaning success/Clean 与 SystemEnding 都进入 Terminal、零 restart；
  Cleaning failed 和 timed out 均由 matching attempt 回到 Running，若旧 teardown 已完成且非 fatal 则立即
  Uninitialized，否则 `/find` 在 handles/connections/reservations 归零前只 publish 固定 empty unavailable、
  归零后下一次 `/find` 恰好 lazy-init 一次。availability 已因内部失败变为 Unavailable 的实例经过
  Cleaning -> ReturnRunning 后仍 fatal/closed 且零 lazy init；Pausing query 自身不设置 latch。Cleaning 后
  SystemEnding 与 ReturnRunning 竞态必须由 Terminal 胜出。recovery 与 Cleaning 同时发生时 pause 通过
  runtime epoch mismatch 取消 recovery、不得误设 fatal；matching ReturnRunning 只在非 fatal 时回
  Uninitialized。attempt epoch overflow 不创建 pause owner，关闭 admission、失效 File domain并设置 fatal，
  但不阻止既有 LifecycleCoordinator 完成退出流程。另固定调度 Cleaning start handler 迟到于同 attempt
  ReturnRunning：ReturnRunning 先到 gate 时无 matching pause 且 no-op，迟到 start 观察 phase=Running 后也
  no-op，最终保持原可用状态且无悬挂 Pausing；旧 start handler 撞上新 Cleaning 时因 attempt mirror 不匹配
  同样 no-op。跨 attempt 固定调度 A ReturnRunning handler 延迟、B start 取得 gate：B 只把 Pausing(A)
  retag 为 Pausing(B,false)，late A no-op；B failed/timed out 后非 fatal 恰好回 Uninitialized，B success 则
  Terminal。全过程 runtime epoch 只在首次 ownership teardown checked increment 一次，只有一个 cancel、
  teardown 和 coordinator。另一调度让 B start 也迟到于 B ReturnRunning：ReturnRunning(B) 以 B>A 前向
  接管 Pausing(A) 并设置 resume，迟到 start(B) 因 phase=Running no-op，teardown 归零后非 fatal 恰好恢复；
  B<A 旧 handler 永远 no-op，attempt overflow 继续走既有 fail-closed。
- clean-close 测试覆盖 terminal exit 先于 pause coordinator 收口时 true-write 计数为零且 metadata 保持
  false；cancel 已观察、全部 DB-work/reservation 归零、普通 connections 全部关闭且 gate 内 phase 非
  Terminal 时才准入唯一专用 connection 的短 transaction。准入后 Terminal 与 transaction 竞态不互等：
  原子 commit 完成则 true，退出打断而未提交则 rollback/false，且不以最后 handle 析构结果建立第二阶段。
  完整 teardown 后若 LifecycleCoordinator ReturnRunning，下一次唯一 lazy reopen 必须在任何查询/worker 前
  重新提交 false。另以旧 marker=true/false 两行证明同一 open transaction 总是先缓存 prior 值再写 false，
  integrity 条件只读 prior snapshot，且同一进程不会运行第二次完整检查。
- lifecycle phase/admission 测试固定注入 ExitGate 已 transition 并 Release-store Cleaning/Terminal、FileIndex
  handler 尚未取得 publication gate 的窗口：pause 前 query token/mapping 被后续 handler 失效；窗口内新
  query 仍可按统一顺序取得 File token，token 仍 latest 时只能 publish empty unavailable；若 invalidation
  在 pause 判定与最终 publish 之间先赢，则响应固定 stale/null。query snapshot、已 resolve
  OpenIndexedPath execution、scanner、watcher、writer、integrity、recovery transition、lazy init 和
  coordinator start 全部必须经 `admit_locked` 拒绝，且零新 reservation/owner、DB/path/Shell。ReturnRunning
  后旧 resolved action 即使等到 lazy init 完成，也因 runtime epoch mismatch 保持 stale。测试还证明
  phase 已为 Cleaning 而 handler 尚阻塞在 gate 时，recovery timeout 不设置 fatal，且 close/delete/create/
  schema/seed 计数全零。lifecycle mutex/publication gate 从不同时持有且各顺序无死锁；后续 watcher/scan
  failure 只走 per-volume backoff。
- 执行/恢复并发测试覆盖两个固定顺序：执行先取得 reservation 时 recovery 必须等待 Shell 返回；recovery
  先关闭 admission 时，即使 registry 已 clone action，也必须零 DB/path/Shell。测试同时认证规定锁顺序
  无死锁，恢复不会只依赖清空 mapping；recovery 完成并 reopen 后，旧 runtime epoch 的已 resolve action 仍必须
  stale 且零 DB/path/Shell。另覆盖 row lookup 缺失，以及同一 row_id 被复用为不同
  VolumeIdentity、relative path 或 kind；每项都 stale 且零 path/Shell，完全匹配才进入 pin。
- native adapter 测试不真正打开 Explorer；受控 race 在祖先/最终项 pin 边界注入 reparse/type/volume
  变化：pin 前成功变化必须零 Shell，同卷同类型普通对象在 pin 前替换按当前路径接受；pin 后的
  rename/delete 路径替换必须被 sharing 拒绝。已有 FILE_SHARE_WRITE 写 handle 和 writable mapping 时
  仍可 pin 并调用 Shell seam。
  Windows 手工 smoke 使用专用临时目录验证正在编辑文件、普通文件定位和目录打开。
- React/Vitest 覆盖三栏、查询 Input 唯一 active-descendant owner、结果永不聚焦、分类单 Tab stop/方向键、
  桌面与窄屏固定焦点顺序、选择保留/回退、刷新 debounce/max-wait、鼠标、预览持久化成功及失败回滚、
  listener-before-empty-search/非零连续 sequence/非法事件/unlisten、排序、进度、Rebuilding/Unavailable
  empty state、错误以及 720 x 420 下 100/150/200% 响应式布局。

### I/O 证据

使用受控样本和 ProcMon 证明扫描进程只进行目录枚举、属性/元数据访问和自身 SQLite I/O，不读取目标
文件内容。原始证据保存在 gitignored artifacts；仓库只提交去标识汇总。

该证据使用本设计的新允许集，不能复用旧 SystemIndex Spike 的 Go/No-Go 公式。

### 依赖与供应链

SQLite Rust crate、resolved SQLite 实现、feature graph、license、source、integrity、build script 和 lockfile
变化都必须在实施计划前冻结并单独取得 Dependency Go。不得因为前端已有 Ant Design 而自动授权新的
Rust 或 native 依赖。

## 实施与集成顺序

1. 本设计书面 Go。
2. 编写并书面复审独立实施计划。
3. 冻结届时本地 main 的精确实现和 trust baseline。
4. 在新的 `codex/` 分支和 worktree 中按 TDD 实现。
5. 分别完成 dependency、security、code 和 UI review。
6. 通过本设计的性能、I/O、Windows smoke 和 trust gates。
7. 仅在明确本地集成授权后集成；不得由 `/find` 的 TaskCodeGo 推断 release Go。

现有 Task 6、Task 7 和失败 security-probe 证据 worktree 不承担 `/find` 开发，也不得为本功能清理或
改写。

## 书面验收标准

设计只有在以下条件全部满足时才可进入实施计划：

- 用户确认三栏交互、分类、打开语义、预览、排序和索引时机。
- 应用自建 SQLite 路线得到明确批准。
- 旧 SystemIndex No-Go 保留且新 I/O 允许集无歧义。
- 文件查询复用唯一 ResultRegistry 的 invocation/query/hide 生命周期，无第二套 action mapping。
- watcher buffer/sink-before-arm、between-call system buffer、单一 backoff calibration、candidate replay、
  原子 generation 切换和活跃查询刷新顺序均可测试。
- 失败 candidate 只使用 committed/candidate 两代；损坏恢复会 quiesce 全部 DB work、定向失效 File mapping，
  由非 worker coordinator 执行并保持进程内 revision 严格单调；Cleaning 只可撤销 pause，非 fatal 的
  ReturnRunning 才回到 lazy Uninitialized，SystemEnding/Clean 才是不可逆 Terminal；所有实际内部
  Unavailable 转换都设置 process-lifetime fatal latch。
- LifecycleCoordinator 的 Release/Acquire phase mirror 与唯一 `admit_locked` 覆盖 query、文件执行、全部
  DB worker、recovery、lazy init 和 coordinator start，生命周期 handler 尚未取得 FileIndex gate 的窗口
  也不能开始新 DB/path/Shell 工作。
- 已 resolve 文件 action 仍受 execution admission 约束；逐组件 pinned handles 在允许正常写入时阻止
  Shell dispatch 前的 rename/delete 路径替换；action runtime epoch 及 row_id lookup 的 volume/path/kind
  必须完全绑定。
- 唯一 execute_result 显式穷举 app/file 两个私有分支；文件动作不触达应用 validation、use count 或 cache。
- Volume GUID/serial 身份、app-data 排除、输入上限和同 snapshot count/items 均失败闭合。
- 720 x 420 窗口在 100/150/200% 下有可达且不重叠的响应式焦点顺序。
- 文件内容读取、任意前端路径执行和发布解锁明确不在范围内。
- 新 dependency/security review 前置得到保留。
