# UiPilot 设置页插件管理与验证功能移除设计

## 目标

本次变更包含两个同时交付的范围：

1. 完整移除 Research ID、手动应用重扫、验证数据导出/清除 UI 及其专用代码。
2. 在设置页增加插件清单，展示包内 Markdown 说明，并支持单插件事务式重新加载和可靠卸载。

该功能面向内部开发者 MVP。插件仍由开发者直接放入
`%APPDATA%\com.uipilot.launcher\plugins`，本次不增加安装器、插件市场或自动更新。

## 已确认决策

- 删除插件以原子移动为提交点，保证原插件根下的包路径消失，并在当前进程立即移除触发词和运行时；移入宿主隔离目录的内容允许延迟回收。
- 插件说明固定来自插件根目录 `README.md`，不在 `plugin.json` 增加路径字段。
- 说明使用安全 Markdown 渲染，支持标题、段落、列表、强调和代码；禁用 HTML、链接和图片。
- 单插件重新加载在当前进程完成，任何失败都保留旧插件继续工作。
- Research ID 删除时一并删除本地验证计数、会话标记、生命周期埋点和导出服务。
- 已存在的验证数据文件停止读写，但不主动删除。

## 非目标

- 不增加插件安装、导入、启用/禁用、排序或自动更新。
- 不允许在设置页编辑插件说明。
- 不支持通过重新加载改变插件 ID；改变 ID 必须删除旧包后按新插件重新启动宿主加载。
- 不恢复设置页的应用别名或应用清单。
- 不增加全局“重新扫描插件”按钮。

## 删除范围

### 设置和前端

- 从设置视图、更新请求和前端模型删除 `researchId`。
- 删除“重新扫描”“导出验证数据”“清除验证数据”及清除确认状态。
- 删除 `rescanApps`、`exportValidationData`、`clearValidationData` 客户端方法、操作状态和错误文案。
- 保留“保存”和“重新加载设置”。重新加载设置仍只恢复当前后端设置快照。

### Rust 后端

- 从 `Settings`、`SettingsUpdate`、设置 DTO 和校验中删除 `research_id`。
- 旧 `settings.json` 中的 `researchId` 作为未知字段被忽略，并在下一次设置写入时自然消失。
- 删除手动 `rescan_apps` command；应用启动时已有的初始扫描和缓存刷新机制保留。
- 删除 `validation_data.rs`、`validation_export.rs`、`session_marker.rs` 及仅由它们使用的原子文件写入代码。
- 删除应用唤起、应用执行和退出路径上的验证事件记录与 `validationFailed` notice。
- 退出流程继续保证文件索引清理和现有系统会话处理，不再等待或写入验证 marker。
- 从 command 注册、能力文件、build allowlist 和测试契约中删除对应命令。

## 插件包说明约定

插件包结构固定为：

```text
internal.math/
├─ plugin.json
├─ README.md
├─ runtime.html
└─ runtime.js
```

`README.md` 规则：

- 文件名和位置固定，不由 manifest 配置。
- 必须是插件根目录下的普通文件，不接受符号链接、junction 或其他重解析点。
- 内容必须是 UTF-8，最大 16 KiB。
- 缺失、超限、非法 UTF-8 或非普通文件统一映射为“未提供说明”，不影响插件功能加载。
- 内容不写入日志和错误信息。

`examples/plugins/internal.math/README.md` 说明 `/math` 的用途、输入格式、实时结果以及回车复制到剪贴板的行为。

## 后端接口

新增三个仅允许主窗口调用的命令：

```text
list_plugins() -> PluginView[]
reload_plugin(plugin_id) -> PluginView
delete_plugin(plugin_id) -> ()
```

前端可见 DTO：

```text
PluginView {
  id: string,
  version: string,
  trigger: string,
  description: string | null
}
```

接口不返回插件根目录、权限、运行时入口、WebView label 或运行时数据目录。

删除和重载只接收插件 ID。后端必须从当前活动目录反查根目录，不能接受前端提供的文件路径。

固定错误类别：

- `pluginListFailed`
- `pluginReloadFailed`
- `pluginDeleteFailed`

错误消息不包含插件 ID、路径、README 内容或 manifest 原文。

## 插件管理器状态

当前一次性 `OnceLock<PluginCatalog>` 改为受锁保护的活动目录。目录读取仍提供短生命周期快照，查询路径不持有写锁等待插件运行时响应。

每个活动 entry 记录宿主生成且单调递增的 `generation`、运行时 label，以及该活动包在加载或成功重载时取得的 Windows volume/file identity。generation 溢出时该插件的后续重载 fail closed，旧 generation 保持活动。

插件变更由一个管理器内的 mutation lock 串行化。MVP 同一时刻只执行一次重载或删除，避免两个操作同时修改目录、admission、ready 状态和 WebView。reload 从 staged 创建到提交或回滚期间拥有 mutation lock，但固定 readiness deadline 保证它不会无限占用该锁。

管理器同时维护仅在重载期间存在的 staged asset 映射和 staged ownership 映射。两张映射使用同一个 runtime identity（`plugin_id + label + generation`）并在同一个 manager 临界区创建、晋升或移除。每个 identity 唯一拥有自己的 generation data directory，该目录随 ownership 一起从 staged 晋升为 active。自定义协议可以为活动 runtime 和 staged runtime 提供资产，但查询路由只指向活动目录。

管理器另有一个 plugin admission gate。查询发布、插件剪贴板副作用和 ready callback 取得 read admission；重载切换、回滚、删除提交及 process-failed/意外 close callback 取得 write admission。mutation lock 只负责串行化管理命令，不能代替 admission gate。

staged runtime readiness 复用现有宿主等待语义，定义单一固定常量 `PLUGIN_RUNTIME_READY_TIMEOUT = 500ms`。该常量不暴露配置项，也不按插件覆盖。

## 事务式重新加载

重新加载流程：

1. 通过活动插件 ID 定位原目录；校验该目录仍是插件根下的普通直接子目录。
2. 从磁盘重新读取 manifest、运行时入口和 README。
3. 要求新 manifest ID 与原 ID 一致，并验证版本、host 版本、权限、触发词及与其他活动插件的冲突。
4. 为候选插件分配新的 generation 和临时 label，以同一个 runtime identity 注册 staged asset/ownership 映射并创建不可见、不可聚焦的临时 WebView。
5. 最多等待 `PLUGIN_RUNTIME_READY_TIMEOUT`，由该 runtime identity 的 ready callback 把 staged attempt 标记为 `ready=true`。等待期间旧插件继续处理查询；ready 只表示可以尝试提交，不提前改变 ownership。timed wait 期间不得持有 admission、active/staged catalog、ready/disabled/timeout/pending 状态锁或 ResultRegistry lock。
6. 候选 ready 后取得 write admission；在 admission 和同一个 manager 临界区内最后重验该 identity 仍唯一拥有 staged asset/ownership slot、`ready=true`、`failed=false`，且对应 WebView 仍存在。任一条件不满足都不能晋升。
7. 最终健康复核通过后，在同一个 manager 临界区把候选 asset entry 和 ownership 从 staged 两张映射原子移出并写入 active entry，同时移除旧 active ownership、取消旧 generation pending query并清理其 timeout/disabled/ready 状态。候选 identity 在任何时刻只属于 staged 或 active 之一，不能双重归属。
8. 在仍持有 write admission、但不持有 manager 状态锁时调用 `ResultRegistry::invalidate_domain(QueryDomain::Plugin)`，使所有旧 Plugin token 失效。active entry 切换和 domain epoch 推进均完成后才释放 admission，对外发布事务提交。
9. 提交成功后只关闭旧 active runtime；关闭完成后 best-effort 清理旧 generation data。promoted generation data 属于当前活动 WebView，必须保留到其后续被替换或删除。
10. 候选完成切换前任一步失败或 readiness deadline 到期时，进入同一 rollback 路径：取得 write admission 并在同一个 manager 临界区同时移除该 identity 的 staged asset/ownership 映射；释放 admission 后先关闭 staged runtime，再 best-effort 清理 staged generation data，返回 `pluginReloadFailed`。活动目录和旧 active runtime 保持不变，并在 rollback 完成后释放 mutation lock，使下一次管理操作可以立即开始。

旧 generation 或回滚 staged generation 的数据清理失败，不改变已经确定的提交或回滚结果，也不暴露路径。任何路径都不得在 promoted runtime 仍活动时清理其 generation data。

## 删除提交与隔离回收

删除成功的精确契约是：提交后 `%APPDATA%\com.uipilot.launcher\plugins\<原插件目录>` 不存在，当前进程不再路由或授权该插件，宿主重启也不会重新加载它。删除成功不承诺插件全部字节在提交时已经从磁盘擦除；原子移入宿主隔离目录的内容可以暂存到本次操作或后续宿主启动的 best-effort 清理。

删除流程：

1. 前端显示明确的二次确认。
2. 宿主在 app data 下预先维护与插件根同卷、位于插件根之外的隔离目录；隔离目录不是插件扫描入口。
3. 后端取得 mutation lock 和 write admission，通过 `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS` 打开当前插件目录 entry，不跟随重解析点；校验它仍是普通目录、其 volume/file identity 与活动 entry 一致，并且目标是插件根的直接子 entry。
4. 使用目录 handle 的 Windows rename primitive，把该 entry 原子移动到隔离目录中的宿主生成唯一名称，不允许覆盖。移动是删除提交点。
5. 移动失败时返回 `pluginDeleteFailed`；活动 entry、路由、授权、runtime 和原目录均保持不变，不能发生部分递归删除。
6. 移动成功后，在仍持有 write admission 时取消该 generation pending query并移除活动 entry/ownership/路由/授权；释放 manager 状态锁后调用 `invalidate_domain(QueryDomain::Plugin)`，最后释放 admission。
7. 提交后关闭被删除的 active runtime；关闭完成后 best-effort 清理该 generation data，并对隔离目录中的内容做 best-effort 递归清理。任一清理失败仍算删除成功；遗留隔离内容由后续宿主启动继续 best-effort 清理。因为原包路径已消失且内容位于插件根之外，当前进程触发词立即失效且重启不会恢复。

删除不使用“先检查路径再 `remove_dir_all`”作为提交协议。文件 identity 与 handle-based rename 把路径替换检查和移动绑定到同一个已打开对象，关闭校验后替换的 TOCTOU 窗口。

## 设置页交互

基础设置下方增加不嵌套卡片的“插件”区。每个插件行展示：

- 插件 ID
- 版本
- 触发词
- README Markdown 说明
- `重新加载`按钮
- `删除`按钮

交互规则：

- 每次进入设置页都启动独立的插件清单请求；普通设置可以先显示，插件区单独显示 loading。
- 插件清单状态固定为 `idle | loading | ready | error`。只有 `ready` 且 entries 为空时显示“未安装插件”，`error` 不能降级为空态。
- `pluginListFailed` 时显示“插件清单加载失败”和“重试”按钮；重试只重载插件清单，不重载或覆盖普通设置草稿。
- 每次清单请求绑定设置 view epoch 和前端 operation token。离开设置页立即废弃 owner；重新进入后旧响应、旧错误和旧逐行操作结果均不能直接覆盖当前插件状态。
- 逐行 reload/delete 完成时先判断其启动 epoch 和 operation token 是否仍拥有当前行。当前 owner 的成功可直接应用返回值，当前 owner 的失败可显示行错误；过期操作的 `PluginView`、删除结果和错误一律不能直接写入当前视图。
- 过期逐行操作无论成功或失败，只要完成时当前 view 仍是 settings，就必须立即签发一个绑定当前 view epoch 和全新 operation token 的 `list_plugins` reconciliation 请求，以后端活动目录为准。操作完成前已经发起的清单请求不能覆盖这次 reconciliation，也不能被复用为其结果。
- reconciliation 的新 token 立即取代当前 list owner；更早的清单成功或失败响应继续由 owner 规则丢弃。多个过期操作完成可以各自签发新 token，较新的 token 自然合并最终状态，不增加后端 API。
- 过期逐行操作完成时若当前不在 settings，不启动后台清单请求；下一次进入设置页已有的强制 `list_plugins` 足以完成 reconciliation。
- 缺少有效 README 时显示“未提供说明”。
- 重新加载只锁定当前插件行，并显示该行 loading 状态。
- 重载成功后使用返回的 `PluginView` 更新版本、触发词和说明。
- 重载失败保留旧行内容和旧插件功能，并在该行显示固定错误文案。
- 删除弹出确认框；成功后移除该行，失败时保留该行并显示固定错误文案。
- 插件操作不保存或覆盖快捷键、开机启动等尚未提交的设置草稿。

Markdown 使用 `react-markdown`，配置明确的 allowed elements；不启用 raw HTML 插件，不渲染 `a` 和 `img`。

插件清单状态、普通设置状态和逐行 mutation 状态是三个独立状态域。清单加载失败不把普通设置设为只读；某一行重载/删除也不占用普通设置 save/load operation。reconciliation 只替换插件 list owner，不修改普通设置草稿或普通设置 operation。

## Generation 与副作用线性化

- `PluginRoute`、`PendingPluginQuery` 和内部 `ResultAction::CopyText` 都绑定 `plugin_id + generation`；generation 不进入前端 DTO。
- 插件查询建立 route、签发 `QueryDomain::Plugin` token 和登记 pending request 时取得 read admission，随后释放；不能跨 admission 长时间等待 runtime。
- `publish_plugin_results` 取得 read admission，验证 callback label、pending generation 和当前活动 generation 全部一致，并在仍持有 admission 时调用 `publish_if_latest`。因此重载/删除提交后的旧查询即使晚到，也无法发布。
- 每次重载或删除提交都在 write admission 内调用 `invalidate_domain(QueryDomain::Plugin)`。这既清除当前 Plugin result set，也推进 Plugin domain epoch，使提交前已经签发的 token 永久失效；`invalidate_plugin` 不能替代该步骤。
- `resolve` 返回的 `CopyText` action 保留签发时的 generation。执行路径先完成 `resolve` 并释放 registry lock，再取得 read admission，重新验证 plugin ID、generation 和当前剪贴板权限，并在同一个 read admission 内完成 clipboard write。write admission 无法穿透 generation 校验与剪贴板副作用之间的区间。
- 如果剪贴板路径先取得 read admission，副作用线性化在 mutation 之前；如果 mutation 先取得 write admission，旧 generation 校验失败且不触碰剪贴板。
- staged runtime 不能接收用户查询，也不能发布没有 pending request 的结果。

## Runtime callback ownership

- 每个 ready、process-failed 和 close callback 只捕获不可变 runtime identity：`plugin_id + label + generation`，不捕获 `slot_kind`。
- callback 取得 admission 后由 manager 动态解析该 identity 当前属于 staged、active 还是无 ownership；所有状态改变前必须在对应临界区再次确认 ownership。
- ready callback 取得 read admission。identity 当前属于 staged 时只设置该 staged attempt 的 `ready=true` 并唤醒 waiter；属于 active 时 ready 是幂等 no-op；无 ownership 时忽略。
- process-failed 和意外 close callback 取得 write admission。identity 当前属于 staged 时只设置该 staged attempt 的 `failed=true` 并唤醒 reload waiter；不能 disable 活动 generation，也不能 invalidate Plugin domain。
- 同一 failure/close callback 在 promotion 之后取得 write admission时，manager 会把该 identity 动态解析为 active，并执行当前 generation 的 active failure 转换：disable、取消 pending、释放 manager 状态锁后推进 Plugin domain epoch并使当前结果失效。
- 旧 active runtime 被替换后或 staged rollback 后延迟到达的 callback 动态解析为无 ownership，只能结束自己的 callback；不能重新插入 slot、disable、取消 pending 或 invalidate 新 generation。
- 正常关闭旧 active、回滚 staged 或删除 active 前，manager 已在 write admission 内移除对应 ownership；因此由宿主主动关闭产生的 close callback 解析为无 ownership，不会被误判为活动 runtime 崩溃。

## 锁顺序与提交边界

所有需要同时取得多个同步原语的路径遵守固定顺序：

```text
mutation lock（仅管理命令）
  -> plugin admission gate
    -> active/staged catalog
      -> ready/disabled/timeout/pending 的单个状态锁
        -> ResultRegistry
```

- 不同时持有两个 ready/disabled/timeout/pending 状态锁；取得 ResultRegistry 前释放这些细粒度状态锁，但 admission 继续持有。
- `ResultRegistry::resolve` 在取得 admission 前完成并释放 registry lock，避免 registry -> admission 的反向嵌套。
- 不在任何 admission、catalog、状态或 ResultRegistry lock 下等待 runtime ready；等待只持有本次 reload 的 mutation ownership，并由固定 500ms deadline 结束。rollback 完成后必须释放 mutation lock。
- 不在任何 catalog、状态或 ResultRegistry lock 下执行文件 I/O、调用 clipboard 或关闭 WebView；clipboard 只在 admission read guard 下执行。
- 重载提交点是 write admission 内完成最终 staged 健康复核、将 staged asset/ownership 原子移动为 active，并推进 Plugin domain epoch；此前失败同时移除 staged 两张映射并回滚，此后旧窗口/旧 generation data 清理失败不回滚。
- 删除提交点是 write admission 内成功完成的 handle-based 原子移动；移动前失败零状态和文件副作用，移动后内存移除不得失败，窗口/隔离目录清理失败不回滚。

Plugin domain epoch 或 plugin generation 溢出时 fail closed：旧 token 不能重新变为有效，不执行剪贴板副作用；重载不切换到无法表示的新 generation。

## 安全边界

- manifest 和 runtime 沿用普通文件及 reparse-point 拒绝规则；README 校验失败只把说明降级为空，不改变插件是否可运行。
- 删除路径只来自后端已加载 entry，并通过 no-follow handle、持久化 file identity 和原子 rename 重新认证；前端路径和仅基于字符串的 canonicalize 结果都不是删除授权。
- 隔离目录位于插件根之外且与其同卷，名称由宿主生成；插件扫描永远不读取隔离目录。
- staged label 由宿主生成，不接受插件输入。
- Markdown 不执行 HTML、脚本、链接导航或外部资源加载。
- 所有前端命令沿用 main-window caller guard；插件运行时 capability 不获得管理命令权限。

## 自动测试

Rust：

- 读取有效、缺失、超限、非法 UTF-8 和 reparse README。
- 插件清单只暴露批准字段。
- 非主窗口调用在任何状态和文件副作用前失败。
- ID 改变、重复触发词、非法 manifest 和运行时未 ready 均回滚并保留旧路由。
- staged runtime 在固定 500ms deadline 内未 ready 时进入统一 rollback，返回 `pluginReloadFailed`；staged 两张映射和 staged data 按既定顺序清理。
- readiness timeout rollback 完成后 mutation lock 已释放，紧接着发起的另一次重载或删除可以立即取得 mutation ownership，不发生永久阻塞。
- staged runtime 上报 ready 后、reload 取得 write admission 前发生 process-failed 时，最终健康复核必须拒绝晋升，同时移除 staged asset/ownership 映射并保留旧 active generation。
- 成功重载只在候选 ready 后切换，并推进 Plugin domain epoch。
- promotion 临界区内 staged asset/ownership 同时移出并成为唯一 active ownership；任何观察点都不能看到同一 identity 同时属于 staged 和 active。
- promotion 后新 active runtime 的 process-failed 或意外 close callback 必须被动态解析为 active，disable 当前 generation、取消其 pending 并推进 Plugin domain epoch。
- 提交前签发的旧 Plugin token 在提交后晚发布时被拒绝且不能重建 result mapping。
- 旧 `CopyText` action 在 resolve 后发生重载或删除时 generation 校验失败；mutation 与 clipboard read admission 的两种线性化均被覆盖。
- 删除原子移动失败时文件、目录项、路由和授权零变化；移动成功后路由和授权消失。
- 隔离目录递归清理失败仍返回删除成功，原插件根路径保持不存在且下一次启动不会加载该插件；遗留隔离内容可由后续宿主清理。
- 活动目录被 junction、symlink 或不同 file identity 的普通目录替换时移动被拒绝；伪造插件 ID 不能越过插件根。
- staged rollback 后和 active 切换后的延迟 ready/process-failed/close callback 不能影响新活动 generation。
- reload 成功只在旧 runtime 关闭后尝试清理旧 generation data，promoted generation data 保持存在且可继续服务资产。
- reload 回滚先移除 staged 映射并关闭 staged runtime，再尝试清理 staged generation data；清理失败不改变回滚结果。
- delete 成功先移除 active ownership并关闭被删 runtime，再尝试清理被删 generation data；清理失败不改变删除结果。
- Research ID、验证模块和三个旧 command 不再出现在生产 wiring。

前端：

- 设置页不再渲染 Research ID、重新扫描、导出和清除按钮。
- 设置保存 payload 只包含仍存在的设置字段。
- 插件空态、README Markdown 和缺失说明渲染正确。
- HTML、链接、图片不进入插件说明 DOM。
- 进入设置页时插件清单独立 loading；加载失败显示错误和重试，不能显示“未安装插件”。
- reload 成功覆盖“epoch A 启动操作、离开并进入 epoch B、epoch B 首次 list 先返回旧快照、epoch A 操作后完成”：旧行 `PluginView` 不能直接应用，但必须自动发起 epoch B 的新 list，最终清单等于后端活动目录。
- delete 成功覆盖同一时序：epoch B 的旧快照不能永久保留已删除项，过期删除完成后自动 reconciliation 并最终移除该项；旧删除响应本身不直接修改当前行。
- 过期 reload/delete 失败不能把 epoch A 的错误写入 epoch B；如果完成时仍在 settings，仍发起同一 reconciliation 刷新以确认后端活动目录。
- 当前 list owner 继续拒绝更早清单响应；reconciliation token 发出后，epoch B 首次 list 的迟到成功或失败均不能覆盖最终清单。
- 单行重载/删除 pending、成功和失败状态互不影响其他插件或设置草稿。
- 删除必须经过确认。
- adapter 只调用新的三个插件 command，旧 command 完全移除。

## 人工验收

1. 安装 `internal.math` 后打开设置，看到 ID、版本、`/math`、README 说明和两个操作按钮。
2. 修改 `/math` 计算逻辑或 README，点击重新加载；不重启宿主即可看到新功能和新说明。
3. 制造非法 manifest 或无法 ready 的 runtime，重新加载失败；旧 `/math` 仍可计算并复制结果。
4. 点击删除并取消，插件保持不变。
5. 确认删除后，清单立即移除插件，`/math 1+1` 不再路由。
6. 检查原路径 `%APPDATA%\com.uipilot.launcher\plugins\internal.math` 不存在；重启宿主后 `/math` 仍不存在。不要求隔离目录中的全部字节在删除提交时已经擦除。
7. 设置页只保留普通设置操作和插件清单，不再出现 Research ID、重新扫描、导出或清除验证数据。
