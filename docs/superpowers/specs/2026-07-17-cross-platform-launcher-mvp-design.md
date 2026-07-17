# Windows 桌面启动器 MVP-A 需求与验证设计

## 1. 文档信息

- 文档状态：评审修订版
- 产品阶段：MVP-A
- 正式支持：Windows 11 x64
- 兼容性验证：Windows 10 22H2 x64；ESU 设备仍只有扩展安全更新，非 ESU 设备不提供安全维护承诺
- 技术路线：Tauri 2 + TypeScript + Rust
- 目的：验证键盘启动器的核心价值，不承诺完整跨平台插件产品

## 2. 决策摘要

MVP-A 只包含以下能力：

1. 全局快捷键唤起桌面启动器。
2. 搜索、启动应用，并尽力激活已运行应用的窗口。
3. 通过 Windows Search 按文件名查找文件，并在 Explorer 中定位。
4. 基础设置、开机启动和签名安装包。

文件搜索采用 `ISearchFolderItemFactory` 的前提是第 6.3 节 SystemIndex Spike 证明不会发生文件系统遍历；Spike 未通过时停止该功能排期并重新评审查询方案。

以下能力不属于 MVP-A：

- 翻译。
- 第三方插件、插件 SDK、插件窗口和权限沙箱。
- macOS。
- 文件内容搜索、自建文件索引和云服务。
- 主程序自动更新和离线安装包。
- 主 WebView 崩溃或卡死的自动检测与恢复。

内置功能只共享宿主维护的 `ResultItem` 数据结构，不以动态插件形式运行。

## 3. 产品假设与验证

### 3.1 待验证假设

- H1：开发者和高频电脑用户愿意使用全局快捷键替代鼠标或开始菜单启动应用。
- H2：应用启动和文件定位两个流程已经足以形成每周持续使用。
- H3：在 Windows 的系统限制下，“尽力激活窗口”仍能明显减少用户操作步骤。
- H4：依赖 Windows Search 的文件名搜索覆盖率可以被目标用户接受。

### 3.2 目标样本

- 招募 12 名不参与项目开发的 Windows 11 x64 用户。
- 用户画像为开发者或每天使用电脑不少于 6 小时的高频用户。
- 其中至少 8 人当前经常使用开始菜单搜索、PowerToys Run、Listary、Everything 或同类工具。
- 试用周期为连续 4 周。

### 3.3 数据采集

MVP-A 不接入远程遥测。程序只在本地记录以下聚合计数：

- 每日唤起次数。
- 应用启动请求次数。
- 应用激活 API 报告成功的次数。
- 应用激活 API 明确拒绝的次数。
- 已提交非空查询的文件搜索会话次数。
- 文件定位请求次数。
- 文件搜索结果为“找到”“未找到”“取消”的次数。
- 宿主异常退出次数。

一次文件搜索会话从防抖结束后向 SystemIndex 提交首个非空查询开始，到执行结果、清空查询或隐藏窗口时结束；会话内后续按键不重复计数。研究构建为每个会话记录且只记录一个结果：成功定位文件记为“找到”，用户点击“未找到预期文件”记为“未找到”，其他结束方式记为“取消”。任何结果都不保存查询词或路径。

本地计数不记录查询词、应用名称或文件路径。试用者每周主动导出本地聚合数据，并参加一次不超过 20 分钟的访谈。导出文件包含研究参与者 ID，因此属于可关联的假名化数据，不称为匿名数据。

“成功动作”仅指宿主获得明确成功返回的应用启动请求、窗口激活 API 或 Explorer 文件定位请求。它是使用频率代理指标，不代表目标应用或 Explorer 最终完成了用户任务。

任务成功率通过主持式测试记录，不依赖本地计数推断。每名完成测试的用户对以下每类任务执行 3 次：

1. 启动一个未运行的应用。
2. 请求激活一个已运行的普通桌面应用。
3. 查找一个已被 SystemIndex 索引的文件并在 Explorer 中定位。

H3 采用配对对照：用户分别使用 Windows 原生流程和 MVP-A 完成相同的 3 个激活任务，执行顺序在用户间交替。每个任务的原生流程在测试前固定为开始菜单搜索、任务栏或 Alt+Tab 之一，不允许使用第三方启动器。计步规则固定为：一次快捷键组合、一次连续文本输入、一次鼠标点击或一次结果确认各计 1 步。耗时从主持人发出任务开始，到目标窗口进入前台结束；30 秒内未完成按失败并记为 30 秒。

H4 使用两个指标：

- 文件覆盖率代理 = `找到次数 / (找到次数 + 未找到次数)`；“取消”不进入分子或分母。
- 用户每周以 1 至 5 分评价“文件搜索是否覆盖了我预期能找到的文件”。

“找到次数 + 未找到次数”为 0 时，H4 视为未获得验证，不能满足 Go 条件。

### 3.4 Windows 阶段退出标准

满足以下全部条件时，MVP-A 判定为 Go：

- 12 名用户中至少 10 名完成 4 周试用。
- 第 4 周至少 8 名用户在 3 个不同日期使用产品。
- 对每名第 4 周留存用户，先计算其各活跃日成功动作的中位数；再计算这些用户中位数的群体中位数，结果不少于 5 次。
- 应用启动成功率不低于 90%，公式为成功次数除以该类已执行测试次数。
- 应用激活成功率不低于 80%，公式为成功次数除以该类已执行测试次数。
- 文件定位成功率不低于 90%，公式为成功次数除以该类已执行测试次数。
- H3 的配对结果中，MVP-A 的步骤数中位数比原有方法至少减少 30%，且完成耗时中位数不高于原有方法。
- H4 的 4 周累计文件覆盖率代理不低于 85%，且第 4 周满意度中位数不低于 4 分。
- 至少 8 名用户明确表示愿意继续使用。
- 试用期间没有数据丢失、安全事故或 P0 缺陷。
- 试用期间未处理的宿主崩溃不超过 1 次，并且该崩溃已完成根因修复和复测。

出现以下任一情况时，MVP-A 判定为 No-Go：

- 应用启动或文件定位成功率低于 75%。
- 应用激活成功率低于 60%。
- H3 的步骤数中位数没有减少。
- H4 的累计文件覆盖率代理低于 70%。
- 第 4 周活跃用户少于 6 名。
- 出现未解决的数据丢失或安全事故。

介于 Go 和 No-Go 之间时，只允许进行一次不超过 2 周的针对性迭代，不进入 macOS 或插件平台开发。

达到 Go 后，根据访谈中的明确需求数量和技术风险，在“macOS”“翻译”“插件技术验证”中只选择一个下一阶段，不默认承诺 macOS。

## 4. 用户流程

### 4.1 程序生命周期

- 程序保持单实例并常驻后台。
- 默认快捷键为 `Alt+Space`。
- 快捷键可在设置中修改。
- 系统托盘菜单提供“打开设置”和“退出”。
- 开机启动可配置，默认关闭。

### 4.2 主窗口

- 快捷键唤起后，主窗口置顶且输入框自动获得焦点。
- 窗口由输入框和自适应高度的滚动结果列表组成。
- 空输入不展示推荐内容。
- 主窗口失去焦点或用户按 `Esc` 时隐藏。
- `Up`、`Down` 移动选择，`Enter` 执行当前项。
- 执行动作后隐藏主窗口；动作失败时保留窗口并显示错误。
- 界面跟随系统明暗主题，不提供自定义主题。

### 4.3 查询路由

- 不以 `/` 开头的输入进入应用搜索。
- `/find <文件名>` 进入文件搜索。
- 未知斜杠命令显示“未知命令”，不执行应用搜索。
- 不实现插件关键词、动态命令注册或推荐排序。

## 5. 应用搜索、启动与激活

### 5.1 应用发现

- 扫描当前用户和所有用户的开始菜单应用入口。
- 保存应用名称、图标、启动入口、可解析的进程标识和用户别名。
- 启动时建立轻量本地缓存。
- 用户可以在设置页手动重新扫描。
- MVP-A 不承诺发现没有开始菜单入口的便携应用。

### 5.2 搜索

- 搜索字段包括应用名称和用户自定义别名。
- 匹配忽略英文字母大小写。
- 支持完全匹配、前缀匹配、包含匹配和字符顺序匹配。
- “企业”或“微信”均可匹配“企业微信”。
- MVP-A 不支持拼音、错别字纠正和语义搜索。
- 结果最多返回 20 项。
- 排序依次参考匹配类型和本地最近使用次数。

### 5.3 启动

- 目标应用没有可识别的已运行窗口时，调用其开始菜单启动入口。
- 启动调用成功只表示请求已交给 Windows，不等于目标应用完成启动。
- 启动入口返回成功时只显示“已发送启动请求”，不等待目标窗口出现。
- 启动入口失效时保留主窗口并提示用户重新扫描。

### 5.4 尽力激活

激活不是绝对保证。宿主按以下顺序尽力处理：

1. 根据启动入口、可执行文件或应用标识查找匹配进程。
2. 进程映射不唯一或没有可见顶层窗口时，不猜测目标窗口，直接调用正常启动入口。
3. 进程映射唯一时，在该进程中选择 Z-order 最靠前的可见顶层非工具窗口。
4. 请求 Windows 将该窗口置于前台。
5. 激活 API 明确返回失败时，回退到正常启动入口，并提示“Windows 拒绝了前台切换，已发送启动请求”。
6. 激活 API 返回成功时不再轮询窗口状态。

以下场景允许无法激活或产生不同结果：

- 目标窗口以高于宿主的权限运行。
- Microsoft Store/UWP 应用不能稳定映射到窗口。
- 应用由启动器进程拉起实际工作进程。
- 多进程应用没有稳定的主窗口标识。
- 应用只存在托盘窗口、隐藏窗口或自定义窗口。
- Windows 前台锁定策略拒绝切换。

验收只要求宿主在映射唯一时正确执行激活尝试，并在 API 明确失败时走回退流程；映射不唯一不尝试激活，不要求所有第三方应用一定进入前台。

## 6. 文件搜索

### 6.1 查询范围

- 只按文件名搜索，不搜索文件内容。
- 只返回文件，不返回文件夹。
- 使用 Windows Search 的 `SystemIndex`。
- 查询作用域只包含 Windows Search Crawl Scope Manager 当前报告为已纳入索引的 `file:` 根目录。
- 不自行建立索引，不执行阻塞式全盘遍历。
- UI 明确说明结果可能不包含未索引目录。

### 6.2 输入边界与查询构造

- 去除 `/find` 命令和外围空白后，查询必须包含 1 至 256 个 Unicode 标量值。
- 拒绝 U+0000 至 U+001F 控制字符；其他合法 Unicode 按原值传递，不进行损失性转码。
- 引号、百分号、下划线、方括号、星号和问号均按普通文本处理，不赋予 SQL、AQS 或通配符语义。
- 使用 `IConditionFactory::MakeLeaf` 构造 `System.FileName`、`COP_VALUE_CONTAINS` 和字符串 `PROPVARIANT` 组成的叶条件。
- 通过 `ISearchFolderItemFactory::SetCondition` 将 `ICondition` 交给 Windows Search。
- 通过 `ISearchFolderItemFactory::SetScope` 显式设置当前已索引的 `file:` 根目录，不传入磁盘根目录、未索引目录或网络位置。
- 禁止将原始输入拼接进 WSSQL，也不直接把用户输入交给 `GenerateSQLFromUserQuery` 解释为 AQS/NQS。
- Windows Search 服务、SystemIndex 或结构化查询 API 不可用时，不创建、不获取也不枚举 Search Folder；只显示索引错误。
- 任何错误都不回退到字符串拼接、未索引目录枚举或磁盘遍历。

### 6.3 SystemIndex Spike

文件搜索进入正式估算和实现前，必须完成一次技术 Spike：

1. 通过 Crawl Scope Manager 枚举并记录当前纳入 SystemIndex 的 `file:` 根目录和排除规则。
2. 验证 Search Folder 的 `SetScope` 只接收上述已索引根目录。
3. 关闭 Windows Search 服务后执行查询，验证宿主在创建或枚举 Search Folder 前返回“索引不可用”。
4. 在未索引目录创建唯一文件并查询该文件名，结果必须为空。
5. 使用 ProcMon、ETW 或等价文件 I/O 跟踪验证第 3、4 项没有由宿主触发的目录枚举或文件内容读取。
6. 记录 Windows build、索引服务状态、实际 scope 和 I/O 跟踪证据。

只有上述测试全部通过，才能采用 `ISearchFolderItemFactory` 路线。若出现文件系统枚举，或无法证明仅使用索引，则该路线判定为 No-Go；替代查询方案必须另行评审，不能静默降级。

### 6.4 查询行为

- 输入变化后防抖 150ms。
- 新查询开始时取消上一条查询；无法取消的旧结果必须按请求 ID 丢弃。
- 查询采用异步流式返回，不等待全部结果完成。
- 结果最多返回 100 项。
- 排序依次参考文件名完全匹配、前缀匹配、包含匹配和最近修改时间。
- 每项显示文件图标、文件名和完整路径。
- 结果为空或缺少预期文件时提供“未找到预期文件”操作，只记录计数。
- 查询 5 秒仍没有结果时继续保持 UI 可操作，并提供检查 Windows 索引状态的入口。

### 6.5 执行动作

- 按 `Enter` 后调用 Explorer 打开文件所在目录并选中文件。
- Explorer 调用失败时保留主窗口并显示错误。
- SystemIndex 不可用时显示处理指引，不降级为磁盘遍历。

## 7. 宿主数据结构

MVP-A 只定义宿主内部的列表数据，不把它发布为第三方 SDK：

```ts
type SearchResponse = {
  requestId: string
  items: ResultItem[]
}

type ResultItem = {
  resultId: string
  kind: 'application' | 'file' | 'status'
  title: string
  subtitle?: string
  icon?: string
}
```

- Rust 返回完整的 `{ requestId, items }` 响应，并在内存中维护 `requestId + resultId -> 真实动作与目标` 映射。
- `requestId` 对每次查询唯一；`resultId` 只需在所属响应内唯一，二者均为不承载业务含义的字符串。
- 主 WebView 执行结果时只能回传 `requestId` 和 `resultId`。
- 新查询开始或主窗口隐藏时，上一份结果映射立即失效。
- Rust 拒绝未知、过期或与当前结果集不匹配的 ID，不接受前端传入路径、启动入口或 Shell 参数。
- 第三方插件阶段不得直接复用这个结构作为公开合同，必须重新做版本化设计。

## 8. 系统架构与安全边界

### 8.1 组件

#### Rust 核心进程

- 单实例和应用生命周期。
- 全局快捷键、窗口和托盘管理。
- 设置持久化。
- 应用发现、启动和尽力激活。
- Windows Search 查询和 Explorer 定位。
- IPC 参数验证和错误分类。

#### TypeScript 主 WebView

- 输入框、结果列表和键盘导航。
- 设置页和状态展示。
- 只展示 Rust 返回的数据，不执行任意系统命令。

MVP-A 没有第三方脚本、动态 HTML、远程页面或额外插件 WebView。

### 8.2 Tauri 命令权限

通过 `invoke_handler` 注册的自定义 Tauri 命令默认可被所有窗口和 WebView 调用。MVP-A 必须执行以下约束：

- 使用 `AppManifest::commands` 将所有自定义命令纳入 Tauri 权限系统。
- 只为固定标签的主窗口配置所需 Capability。
- 不使用匹配所有窗口的 `windows: ["*"]`。
- 每个命令使用窄参数结构，不提供通用命令转发、Shell 或任意路径执行。
- 自动化测试创建非主窗口标签并验证自定义命令被拒绝。

### 8.3 CSP 与内容来源

主 WebView 必须显式配置 CSP，最低要求为：

- `default-src 'self'`。
- `script-src 'self'`，不允许 `'unsafe-eval'` 或 `'wasm-unsafe-eval'`。
- `connect-src` 只允许 Tauri IPC 所需来源。
- `object-src 'none'`、`frame-src 'none'`。
- 不加载 CDN、远程脚本、远程页面或用户提供的 HTML。
- 导航处理器拒绝离开应用本地来源的顶层导航。

CSP 是纵深防御，不能替代 Rust 侧命令权限和参数校验。

## 9. 设置、本地数据与隐私

### 9.1 设置

- 全局快捷键。
- 开机启动。
- 应用别名。
- 重新扫描应用。
- 打开 Windows 索引设置。
- 导出本地聚合验证数据。
- 清除本地验证数据。

### 9.2 存储

- 普通设置、应用缓存、最近使用次数和验证计数保存在系统应用数据目录。
- MVP-A 使用结构化本地文件，不引入数据库。
- 写入采用临时文件加原子替换。
- 配置损坏时回退到上一份有效配置。

### 9.3 隐私

- 程序不发送远程遥测。
- 日志和验证计数不得记录查询词、应用名称、剪贴板内容或文件路径。
- 只有用户主动导出后，汇总文件才离开本机。
- 试用开始前取得知情同意，说明采集字段、用途、关联方式和删除期限。
- 产品负责人单独保管参与者身份与研究 ID 的映射。
- 用户退出试用并要求删除时，产品负责人在 7 个自然日内删除已提交数据。
- 最终试用结论形成 30 天后，产品负责人删除导出文件、身份映射和可关联的访谈记录，并记录删除日期。
- MVP-A 不包含任何业务网络请求。

### 9.4 可访问性

- 输入框、结果列表和设置控件必须显示清晰的键盘焦点。
- 输入框使用组合框语义并关联结果列表；结果列表使用 `listbox`，结果项使用 `option` 和 `aria-selected`。
- 当前选择变化和错误消息通过无障碍状态区域通知辅助技术。
- 错误状态包含文字或图标，不只依赖颜色。
- 图标按钮提供可访问名称和悬停提示。

## 10. 性能验收

### 10.1 参考环境

性能验收使用以下基线：

- Windows 11 24H2 x64；Windows 10 22H2 x64 另行执行兼容性基准，不混合统计。
- 至少 4 个物理 CPU 核心、16GB 内存和 SSD。
- Release 构建，接通电源，系统启动至少 5 分钟。
- 当前稳定版 Evergreen WebView2 Runtime。
- 应用缓存包含 500 项。
- SystemIndex 处于健康且空闲状态，包含至少 100,000 个本地文件。

Windows 11 正式支持基准与 Windows 10 兼容性基准必须分别运行。每份报告记录精确的 Windows 版本与 build、CPU 型号、内存、存储类型、WebView2 Runtime 版本和 SystemIndex 文件数量。只有 Windows 11 结果用于正式性能门槛，Windows 10 结果单独报告。

### 10.2 埋点定义

埋点只在测试构建中写入本地性能日志。事件后缀标识时钟域：`_ui` 使用 WebView 的 `performance.now()`，`_rust` 使用 Rust 的 `Instant`，`_external` 使用外部测试驱动的单调时钟。快捷键事件用 `invocationId`、文件查询事件用 `requestId` 关联；不同后缀的时间戳禁止直接相减。

外部测试驱动事件：

- `shortcut_sent_external`：测试驱动发送全局快捷键。
- `input_focus_observed_external`：测试驱动通过 Windows UI Automation 观察到输入框获得焦点。

WebView 事件：

- `show_event_received_ui`：主 WebView 收到显示事件。
- `input_interactive_ui`：输入框聚焦后的首个 `requestAnimationFrame`。
- `query_input_ui`：主 WebView 收到输入事件。
- `app_results_committed_ui`：应用结果提交到 DOM。
- `file_debounce_elapsed_ui`：150ms 防抖完成，主 WebView 准备发送文件查询。
- `first_file_result_received_ui`：主 WebView 收到首个文件结果事件。
- `first_file_result_committed_ui`：首个文件结果提交到 DOM。

Rust 事件：

- `shortcut_received_rust`：Rust 收到全局快捷键事件。
- `show_event_emitted_rust`：Rust 向主 WebView 发出显示事件。
- `file_ipc_received_rust`：Rust 收到文件查询 IPC。
- `file_query_submitted_rust`：Rust 将查询提交给 SystemIndex。
- `first_file_result_received_rust`：Rust 从 SystemIndex 收到首个结果。
- `first_file_result_emitted_rust`：Rust 向主 WebView 发出首个结果事件。

### 10.3 指标

- 热启动唤起端到端：`shortcut_sent_external` 到 `input_focus_observed_external`，100 次样本，前 5 次预热不计，P95 不超过 1 秒。
- Rust 快捷键处理：`shortcut_received_rust` 到 `show_event_emitted_rust`，同一批样本单独报告。
- WebView 显示处理：`show_event_received_ui` 到 `input_interactive_ui`，同一批样本单独报告。
- 应用搜索：`query_input_ui` 到 `app_results_committed_ui`，1,000 条固定查询，P95 不超过 100ms。
- 文件搜索防抖：`query_input_ui` 到 `file_debounce_elapsed_ui` 不早于 150ms，200 条固定查询的 P95 不超过 200ms。
- Rust 文件查询准备：`file_ipc_received_rust` 到 `file_query_submitted_rust`，200 条固定查询，P95 不超过 100ms。
- SystemIndex 等待：`file_query_submitted_rust` 到 `first_file_result_received_rust`，200 条保证命中的固定查询，单独报告 P50、P95 和最大值。
- Rust 首结果处理：`first_file_result_received_rust` 到 `first_file_result_emitted_rust`，同一批查询单独报告。
- WebView 结果渲染：`first_file_result_received_ui` 到 `first_file_result_committed_ui`，同一批查询，P95 不超过 100ms。
- 文件首结果端到端基准：`query_input_ui` 到 `first_file_result_committed_ui`，200 条保证命中的固定查询，P95 目标为 1 秒。

文件首结果指标只在上述参考环境中作为基准。SystemIndex 的外部等待时间必须单独报告，不能对所有用户环境承诺 1 秒。现场环境超过 1 秒时，UI 必须保持响应并显示搜索状态。

## 11. 错误处理

| 场景 | 预期行为 |
| --- | --- |
| 全局快捷键冲突 | 程序继续运行，提示用户修改快捷键 |
| 应用缓存入口失效 | 保留主窗口，提示重新扫描 |
| 前台激活被 Windows 拒绝 | 回退到正常启动并提示系统限制 |
| SystemIndex 不可用 | 显示 Windows 索引设置入口，不遍历磁盘 |
| 文件查询超过 5 秒 | 保持 UI 响应，允许取消并显示索引帮助 |
| Explorer 定位失败 | 保留主窗口并显示错误 |
| 本地配置损坏 | 使用上一份有效配置并提示用户 |

## 12. 发布前置条件

MVP-A 对外试用前必须满足：

| 责任角色 | 前置条件 |
| --- | --- |
| 产品负责人 | 批准 Windows 签名和测试资源预算，指定试用名单 |
| 产品负责人 | 管理试用知情同意、研究 ID 映射、数据删除请求和到期删除记录 |
| 发布负责人 | 取得可用的 Windows 代码签名身份，保护 CI 签名凭据 |
| 发布负责人 | 选择并记录 SmartScreen 策略；不承诺新证书立即具有信誉 |
| 安装包负责人 | 检测 WebView2 Runtime，缺失时使用 Evergreen Bootstrapper 安装 |
| QA 负责人 | 在干净的 Windows 11 x64 环境验证安装、卸载和升级覆盖 |
| QA 负责人 | 在 Windows 10 22H2 x64 上单独执行兼容性验证，并记录是否启用 ESU |
| QA 负责人 | 覆盖标准用户、管理员用户和目标应用提权运行场景 |

在签名预算、签名身份或负责人未落实前，只允许内部未签名构建，不对外承诺发布日期。

## 13. MVP-A 验收标准

### 13.1 主程序

- 只能运行一个主实例。
- 默认快捷键能显示主窗口并聚焦输入框。
- `Up`、`Down`、`Enter` 和 `Esc` 行为符合第 4 节。
- 快捷键冲突不会导致程序退出。

### 13.2 应用功能

- 输入“企业”或“微信”可以匹配已发现的“企业微信”。
- 目标未运行时能通过已缓存启动入口发起启动。
- 普通、同权限桌面应用的激活成功路径通过测试。
- 进程映射不唯一时跳过激活并直接调用启动入口。
- Windows 拒绝激活时执行正常启动回退并展示提示。
- 提权应用、Store/UWP 和多进程应用记录为允许差异，不作为绝对激活失败。

### 13.3 文件功能

- `/find <文件名>` 只返回 SystemIndex 中的文件。
- 快速修改查询时不会出现旧结果覆盖新结果。
- 按 `Enter` 后 Explorer 打开所在目录并选中文件。
- SystemIndex 不可用时不会开始磁盘遍历。
- 空查询、257 个 Unicode 标量值及 U+0000 至 U+001F 控制字符被明确拒绝。
- 单双引号、百分号、下划线、方括号、星号、问号、中日韩文字、emoji 和组合字符按字面进入结构化条件，不改变查询结构。
- 关闭 Windows Search 服务后发起查询，宿主在创建或枚举 Search Folder 前返回错误；I/O 跟踪中没有宿主目录枚举。
- 查询未索引目录中的唯一文件名时结果为空；I/O 跟踪中没有宿主访问该目录。
- SystemIndex Spike 未通过时，文件搜索不得进入正式实现或排期。

### 13.4 安全

- 非主窗口标签不能调用自定义 Tauri 命令。
- CSP 阻止远程页面、远程脚本、frame、eval 和 WebAssembly 执行。
- 未知、过期或篡改的 `requestId/resultId` 不能触发动作。
- 文件查询只通过 `IConditionFactory::MakeLeaf` 和 `ISearchFolderItemFactory::SetCondition` 构造；代码与测试中不存在原始用户输入拼接 WSSQL 的路径。
- 日志和导出汇总不包含查询词、应用名称或文件路径。

### 13.5 可访问性

- 只使用键盘可以完成唤起、输入、选择、执行、取消和打开设置。
- 所有可交互控件都有可见焦点。
- 结果列表向 Windows Narrator 正确报告控件角色、当前选中项、标题和副标题。
- 错误消息能由 Narrator 读出，并且不只依赖颜色表达。
- Windows Narrator 冒烟测试覆盖应用搜索和文件搜索各一条完整流程。

### 13.6 安装与发布

- Windows 10 22H2 兼容性环境缺少 WebView2 Runtime 时，安装程序能引导安装 Evergreen Runtime。
- Windows 11 x64 完成签名安装、卸载和覆盖安装测试。
- Windows 10 22H2 x64 的兼容性结果单独记录，不作为正式支持承诺。
- 签名证书和 CI 凭据不进入仓库或安装包。

## 14. 后续候选阶段

后续阶段都不属于 MVP-A，也没有工期承诺。

### 14.1 翻译候选

进入条件：

- 至少 6 名目标用户在 MVP-A 访谈中明确提出翻译需求。
- 单独验证用户自行申请有道开发者凭据的激活流失。

若进入实现：

- 只在用户按 `Enter` 后显式发送翻译请求，不使用 300ms 自动请求。
- 源语言优先使用服务商自动检测结果，不使用“包含中文字符”的本地启发式规则。
- 凭据保存在 Windows Credential Manager。
- 先验证凭据配置完成率和单次任务成功率，再决定是否进入正式版本。

### 14.2 macOS 候选

macOS 不是“只替换系统适配”。进入排期前必须分别验证：

- LaunchServices/Applications 应用发现差异。
- AppKit 应用激活请求及系统拒绝场景。
- Spotlight 查询语义和权限。
- 全局快捷键、菜单栏、开机启动和 Keychain。
- WKWebView 行为、CSP 和进程失败恢复。
- Apple Developer 账号、签名、公证和发布凭据。
- Apple Silicon 与 Intel 构建、测试机和 CI 构建机。

macOS 允许在系统限制下采用不同交互和错误提示，不承诺与 Windows 完全相同行为。

### 14.3 第三方插件技术 Spike

插件平台是独立高风险项目。在排期或公开 SDK 前，必须完成一个不进入正式产品的技术 Spike。

Spike 必须验证：

1. 动态插件内容使用独立来源和插件 ID 绑定，不能继承主 WebView 权限。
2. 所有自定义命令通过 `AppManifest::commands` 和 Capability 进入权限系统。
3. 插件 CSP 默认拒绝全部内容，只按需允许本地脚本、样式和图片。
4. `eval`、动态代码生成和 WebAssembly 默认不可用。
5. 插件不能直接发起网络请求；网络只能通过 Rust 代理。
6. Rust 代理按每次请求校验 HTTPS 域名，重定向后重新校验，超时 10 秒，响应体上限 2 MiB。
7. 远程导航被拦截；只允许通过宿主在默认浏览器打开经过校验的 HTTPS URL，不允许打开任意文件。
8. 每个插件使用独立存储命名空间，默认配额 10 MiB，卸载时清除。
9. Windows 监听 WebView2 `ProcessFailed` 并尝试重建插件窗口。
10. 独立 WebView 不能被视为确定的独立进程；必须实测共享进程崩溃和卡死对宿主 UI 的影响。
11. 如果 Tauri/Wry 无法暴露可靠的进程失败或卡死恢复能力，未受信任的 window 插件判定为 No-Go。

Spike 通过后，SDK 规格必须先定义以下合同，之后才能估算开发：

- `ResultItem` 的动作类型、序列化和版本规则。
- 查询请求、取消、过期响应和超时协议。
- RPC 请求、响应、错误码和最大消息尺寸。
- 权限授予范围、有效期、撤销和升级行为。
- 宿主与插件版本兼容策略。
- UI 导航、外部链接和存储生命周期。
- Windows WebView2 与 macOS WKWebView 的允许差异。

按文件扩展名拒绝 Python 或原生二进制只能作为包格式约束，不能作为安全边界。真正的安全边界必须由不可绕过的命令权限、CSP、来源隔离、RPC 参数校验和运行时恢复共同构成。

## 15. 交付顺序

1. Windows 宿主、全局快捷键和主窗口。
2. 应用发现、搜索、启动和尽力激活。
3. 完成并评审 SystemIndex Spike；No-Go 时停止文件搜索排期。
4. Spike 通过后实现 Windows Search 文件查询和 Explorer 定位。
5. 设置、隐私汇总、CSP 和 Tauri 命令权限。
6. WebView2 Runtime 部署、签名安装包和 Windows QA。
7. 12 人、4 周 MVP-A 试用。
8. 根据第 3.4 节作出 Go、No-Go 或一次性迭代决定。
9. 达到 Go 后只选择一个后续候选阶段。

## 16. 已确认决策

- MVP-A 正式支持 Windows 11 x64；Windows 10 22H2 x64 只做兼容性验证。
- MVP-A 只包含启动器、应用功能、文件搜索、设置和签名安装包。
- 应用窗口激活采用尽力而为和可见回退，不作绝对保证。
- 文件搜索依赖 Windows Search，不自建索引。
- 翻译、macOS 和第三方插件平台移出 MVP-A。
- 不使用远程遥测，以本地聚合数据和主持式任务测试验证产品。
- 第三方插件平台在技术 Spike 通过前不估算、不排期。

## 17. 参考资料

- [Tauri Capabilities](https://v2.tauri.app/security/capabilities/)
- [Tauri Content Security Policy](https://v2.tauri.app/security/csp/)
- [Tauri Process Model](https://v2.tauri.app/concept/process-model/)
- [WebView2 Process Model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)
- [WebView2 Runtime Distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)
- [Windows SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
- [Windows 10 生命周期](https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro)
- [Windows 10 ESU](https://learn.microsoft.com/en-us/windows/whats-new/extended-security-updates)
- [IConditionFactory::MakeLeaf](https://learn.microsoft.com/en-us/windows/win32/api/structuredquery/nf-structuredquery-iconditionfactory-makeleaf)
- [ISearchFolderItemFactory::SetCondition](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-isearchfolderitemfactory-setcondition)
- [ISearchFolderItemFactory::SetScope](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-isearchfolderitemfactory-setscope)
- [Windows Property System Overview](https://learn.microsoft.com/en-us/windows/win32/properties/property-system-overview)
- [Windows Search Crawl Scope Manager](https://learn.microsoft.com/en-us/windows/win32/search/-search-3x-wds-extidx-csm)
