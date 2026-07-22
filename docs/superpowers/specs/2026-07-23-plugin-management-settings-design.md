# UiPilot 设置页插件管理与验证功能移除设计

## 目标

本次变更包含两个同时交付的范围：

1. 完整移除 Research ID、手动应用重扫、验证数据导出/清除 UI 及其专用代码。
2. 在设置页增加插件清单，展示包内 Markdown 说明，并支持单插件事务式重新加载和物理删除。

该功能面向内部开发者 MVP。插件仍由开发者直接放入
`%APPDATA%\com.uipilot.launcher\plugins`，本次不增加安装器、插件市场或自动更新。

## 已确认决策

- 删除插件会物理删除对应插件包目录，并在当前进程立即移除触发词和运行时。
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

插件变更由一个管理器内的 mutation lock 串行化。MVP 同一时刻只执行一次重载或删除，避免两个操作同时修改目录、ready 状态和 WebView。

管理器同时维护仅在重载期间存在的 staged runtime 映射。自定义协议可以为活动运行时和 staged runtime 提供资产，但查询路由只指向活动目录。

## 事务式重新加载

重新加载流程：

1. 通过活动插件 ID 定位原目录；校验该目录仍是插件根下的普通直接子目录。
2. 从磁盘重新读取 manifest、运行时入口和 README。
3. 要求新 manifest ID 与原 ID 一致，并验证版本、host 版本、权限、触发词及与其他活动插件的冲突。
4. 为候选插件分配临时 generation label，注册 staged asset 映射并创建不可见、不可聚焦的临时 WebView。
5. 等待候选运行时通过现有 title 协议上报 ready。此期间旧插件继续处理查询。
6. 候选成功后，在写锁内把活动目录切换到新 entry；取消旧插件 pending query、使旧结果失效，并清理 timeout/disabled/ready 状态。
7. 活动目录切换是事务提交点；提交后关闭旧 WebView 并移除 staged 标记，新 generation 成为活动运行时。
8. 候选完成切换前任一步失败时，关闭临时 WebView 并移除 staged 映射，活动目录和旧 WebView 保持不变。

旧窗口关闭和临时运行时数据目录删除属于提交后的 best-effort 清理。清理失败不改变已经成功的切换结果，也不暴露路径；旧 generation 已无路由和 pending request，不能继续发布结果。

## 物理删除

删除流程：

1. 前端显示明确的二次确认。
2. 后端在 mutation lock 和活动目录写锁保护下重新验证插件根目录边界。
3. 先尝试删除插件包目录；删除失败时目录项和运行时保持不变。
4. 删除成功后移除活动目录项、取消 pending query、使旧结果失效并关闭运行时。
5. 插件触发词立即无法路由；重启后因为包目录不存在，也不会重新出现。

宿主生成的 runtime data 目录做 best-effort 清理，不影响“插件包目录已物理删除”的成功语义。

## 设置页交互

基础设置下方增加不嵌套卡片的“插件”区。每个插件行展示：

- 插件 ID
- 版本
- 触发词
- README Markdown 说明
- `重新加载`按钮
- `删除`按钮

交互规则：

- 没有活动插件时显示“未安装插件”。
- 缺少有效 README 时显示“未提供说明”。
- 重新加载只锁定当前插件行，并显示该行 loading 状态。
- 重载成功后使用返回的 `PluginView` 更新版本、触发词和说明。
- 重载失败保留旧行内容和旧插件功能，并在该行显示固定错误文案。
- 删除弹出确认框；成功后移除该行，失败时保留该行并显示固定错误文案。
- 插件操作不保存或覆盖快捷键、开机启动等尚未提交的设置草稿。

Markdown 使用 `react-markdown`，配置明确的 allowed elements；不启用 raw HTML 插件，不渲染 `a` 和 `img`。

## 并发与结果一致性

- 插件 mutation 全局串行，普通查询只读取活动 entry 快照。
- staged runtime 不能接收用户查询，也不能发布没有 pending request 的结果。
- 切换或删除时调用现有 `ResultRegistry::invalidate_plugin`，旧结果不能在新 generation 或删除后执行。
- pending query 在切换和删除时以 `RuntimeDisabled` 结束，不能跨 generation 发布。
- 剪贴板授权始终基于当前活动 entry 的权限；候选权限在切换前不生效。

## 安全边界

- manifest 和 runtime 沿用普通文件及 reparse-point 拒绝规则；README 校验失败只把说明降级为空，不改变插件是否可运行。
- 删除路径只来自后端已加载 entry，并在删除前再次验证其位于插件根直接下一层。
- staged label 由宿主生成，不接受插件输入。
- Markdown 不执行 HTML、脚本、链接导航或外部资源加载。
- 所有前端命令沿用 main-window caller guard；插件运行时 capability 不获得管理命令权限。

## 自动测试

Rust：

- 读取有效、缺失、超限、非法 UTF-8 和 reparse README。
- 插件清单只暴露批准字段。
- 非主窗口调用在任何状态和文件副作用前失败。
- ID 改变、重复触发词、非法 manifest 和运行时未 ready 均回滚并保留旧路由。
- 成功重载只在候选 ready 后切换，旧 pending/result 被清理。
- 删除失败保留目录项；删除成功移除物理目录、路由和授权。
- 路径替换、符号链接和伪造插件 ID 不能越过插件根。
- Research ID、验证模块和三个旧 command 不再出现在生产 wiring。

前端：

- 设置页不再渲染 Research ID、重新扫描、导出和清除按钮。
- 设置保存 payload 只包含仍存在的设置字段。
- 插件空态、README Markdown 和缺失说明渲染正确。
- HTML、链接、图片不进入插件说明 DOM。
- 单行重载/删除 pending、成功和失败状态互不影响其他插件或设置草稿。
- 删除必须经过确认。
- adapter 只调用新的三个插件 command，旧 command 完全移除。

## 人工验收

1. 安装 `internal.math` 后打开设置，看到 ID、版本、`/math`、README 说明和两个操作按钮。
2. 修改 `/math` 计算逻辑或 README，点击重新加载；不重启宿主即可看到新功能和新说明。
3. 制造非法 manifest 或无法 ready 的 runtime，重新加载失败；旧 `/math` 仍可计算并复制结果。
4. 点击删除并取消，插件保持不变。
5. 确认删除后，清单立即移除插件，`/math 1+1` 不再路由。
6. 重启宿主后 `/math` 仍不存在，插件包目录已删除。
7. 设置页只保留普通设置操作和插件清单，不再出现 Research ID、重新扫描、导出或清除验证数据。
