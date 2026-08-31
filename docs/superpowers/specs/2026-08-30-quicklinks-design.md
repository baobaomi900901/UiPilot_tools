# Quicklinks 内置快速链接设计

## 文档信息

- 日期：2026-08-30
- 状态：设计待用户确认（已按独立审核意见补齐）
- 范围：UiPilot 宿主侧内置功能

## 用户目标

用户希望在 UiPilot 中维护一组自定义快速链接。例如配置启动键 `jd` 和链接模板
`https://search.jd.com/Search?keyword={Query}` 后，在主界面输入 `/jd 手机` 并回车，
UiPilot 使用默认浏览器打开替换后的链接。

## 范围拆分

Quicklinks 分两部分交付：

1. `/quicklinks` 管理面板：新增、编辑、删除 quicklink。
2. 主界面运行 quicklink：输入 `/jd query` 后打开模板 URL。

这两部分共享同一个宿主数据源和命令命名空间，但前端状态与 public plugin panel 分离。

## 全局命令命名空间

Quicklinks 与内置命令、public plugin 命令、legacy plugin 命令共用 launcher slash command 命名空间。

启动键语法沿用现有 launcher command 规则：

```text
^[a-z][a-z0-9-]{0,31}$
```

也就是首字符必须是小写英文字母，后续可以是小写英文字母、数字或短横线，总长度不超过 32。

保留内置命令：

- `find`
- `quicklinks`
- `web-search`

冲突策略：

- quicklink 保存时必须检查内置命令、已有 quicklink、已安装 public plugin effective name、legacy plugin route。
- public plugin 安装或改名时也必须检查已有 quicklink，不能创建冲突命令。
- legacy plugin 如果仍允许注册命令，也必须走相同冲突检查。
- 不允许 shadowing；同名命令必须失败并显示明确错误。

主界面解析优先级：

1. 保留内置命令精确匹配：`/find`、`/quicklinks`、`/web-search`。
2. Quicklink 精确匹配：`/{command}` 或 `/{command} {argument}`。
3. Public plugin discovery / route。
4. Legacy plugin route。
5. 普通应用、文件和 web fallback。

`/` catalog 中显示顺序：

1. `/find`
2. `/quicklinks`
3. `/web-search`
4. public plugin 建议

当用户输入普通文本 `str` 时，常用插件仍按既有规则提前；quicklinks 只响应 slash command，不抢普通文本搜索。

## 数据模型

新增宿主模块 `quicklinks`。

Quicklink 记录字段：

- `id`：宿主生成的稳定 ID，使用单调递增十进制字符串。
- `name`：目录名称，非空，展示给用户。
- `command`：启动键，不含前导 `/`。
- `template`：链接模板。
- `icon_asset`：宿主管理的图标资产引用，不暴露原始本地路径给前端。
- `created_at`：创建时间。
- `updated_at`：更新时间。

持久化：

- 配置文件路径：`app_data_dir/quicklinks/quicklinks.json`。
- 图标目录：`app_data_dir/quicklinks/icons/<id>.png`。
- 图标候选目录：`app_data_dir/quicklinks/icon-candidates/<token>.png`。
- 配置文件包含 `schemaVersion: 1`、`nextId` 和 `items`。
- 保存采用临时文件 + rename 的原子写入策略。
- 加载时将合法配置放入内存 cache；保存成功后同步更新 cache，主界面无需重启即可搜索到新 quicklink。
- 配置文件损坏时，复制为 `quicklinks.corrupt.<timestamp>.json`，返回空列表，并向 UI 返回固定错误码。

图标暴露：

- 前端不直接读取 `icon_asset` 本地路径。
- 后端向 launcher result 和 quicklinks panel 返回 `data:image/png;base64,...`。
- 删除 quicklink 时删除对应图标。
- 更新图标时写入 `<id>.png`，旧图标被覆盖或清理。
- 未保存的图标候选以 token 引用；保存成功后移动到 `icons/<id>.png`，取消或过期候选由宿主清理。

## 链接模板规则

模板必须满足：

- scheme 为 `http` 或 `https`；
- 至少包含一个 literal `{Query}`；
- 不包含 NUL 或控制字符；
- 替换所有 literal `{Query}`。

`{Query}` 编码规则：

- 按 UTF-8 percent-encode 一个 URL component；
- 空格编码为 `%20`，不使用 `+`；
- `&` 编码为 `%26`，`?` 编码为 `%3F`，`#` 编码为 `%23`。

示例：

```text
template = https://search.jd.com/Search?keyword={Query}
query    = 手机 A&B?
url      = https://search.jd.com/Search?keyword=%E6%89%8B%E6%9C%BA%20A%26B%3F
```

空参数不打开链接，主界面提示用户输入参数。

## 图标规则

- 只能选择 PNG 文件。
- 宿主必须实际解码 PNG，不只检查扩展名。
- 图片必须正好是 `128x128`。
- 伪 PNG、损坏 PNG、`129x128`、`128x129` 都必须拒绝。
- 保存成功后复制到 `app_data_dir/quicklinks/icons/<id>.png`。

## 后端接口设计

新增或扩展的 Tauri 命令：

- `list_quicklinks()`：返回 quicklink 列表和固定错误码。
- `save_quicklink(input)`：新增或更新 quicklink；只在 draft 满足必填项和字段校验后调用。
- `delete_quicklink(id)`：删除记录和图标。
- `choose_quicklink_icon()`：打开本地文件选择器，只允许 PNG；校验通过后复制到 `icon-candidates/<token>.png`，返回 `{ token, dataUrl }`。

前端内部 action：

- `completeQuicklinkCommand(command)`：从内置 Quicklinks Panel 返回主界面并补全 `/{command} `；这是前端 core action，不是 Tauri command。

固定错误码：

- `quicklinkLoadFailed`
- `quicklinkSaveFailed`
- `quicklinkDeleteFailed`
- `quicklinkCommandConflict`
- `quicklinkInvalidCommand`
- `quicklinkInvalidTemplate`
- `quicklinkInvalidIcon`
- `quicklinkOpenFailed`

## 默认浏览器打开 URL

现有 `web_search::open_search(engine, query)` 只能打开固定搜索引擎，不作为 quicklinks 的直接复用接口。

新增通用 browser opener seam：

- `browser_open::open_url(url::Url) -> Result<(), ()>`，Windows 下内部使用 `ShellExecuteW`。
- `web_search::open_search` 改为生成搜索 URL 后调用 `browser_open::open_url`。
- quicklinks 执行时先生成并验证目标 `Url`，再调用 `browser_open::open_url`。

这样 `/web-search` 和 quicklinks 共用默认浏览器打开能力，但 URL 生成逻辑仍各自独立。

## 主界面搜索与执行

### `/quicklinks`

- `/quicklinks` 返回一个内置结果，activation 为 `OpenQuicklinks`。
- 用户直接输入 `/quicklinks` 并回车时，前端自动打开 Quicklinks 内置 Panel。
- 在 `/` catalog 中也显示 `/quicklinks`，选中回车同样打开内置 Panel。

### `/jd`

- 如果 `jd` 是已保存 quicklink：
  - 输入 `/jd` 返回一个 list-item，title 为 `/jd`，subtitle 为 quicklink 名称，状态提示“请输入参数”。
  - 该结果 `hasDefaultAction=false`，避免空参数回车打开。

### `/jd 手机`

- 输入 `/jd 手机` 返回一个 list-item：
  - `activation: ExecuteResult`
  - `hasDefaultAction: true`
  - registry action 为 `ResultAction::OpenQuicklink { id, url }`
  - title 为 `/jd`
  - subtitle 展示目标名称或链接预览。
- 用户按 Enter 后，前端通过 `execute_result` 执行该 registry action。
- 打开成功后隐藏主窗口，行为与 `/web-search` 一致。

### 自动执行事件流

后端 `SearchResponse` 增加通用字段：

```text
autoExecuteResultId?: string
```

规则：

- 只有 `submit=true`、当前搜索仍属于最新 query sequence、且 `autoExecuteResultId` 指向当前 response 中 `hasDefaultAction=true` 的结果时，前端才自动执行。
- `/web-search 手机` 迁移到同一字段，移除前端硬编码 `/web-search\s+\S` 特例。
- `/jd 手机` 回车时，后端设置 `autoExecuteResultId` 为 quicklink 结果 ID。
- `/jd` 无参数时不设置该字段。

## 前端状态设计

Quicklinks 是内置 Panel，不复用 public plugin runtime panel。

前端新增 launcher mode：

```text
launcherMode: 'applications' | 'files' | 'panel' | 'quicklinks'
```

其中：

- `panel` 继续只表示 public plugin panel，保留 `pluginId/sessionEpoch/hostKeys`。
- `quicklinks` 表示内置 Quicklinks 管理面板，不需要 `pluginId`、`sessionEpoch`、`hostKeys`，不调用 `openPluginPanel` / `submitPluginPanel`。

Quicklinks 前端状态包含：

- `items`
- `selectedId`
- `draft`
- `loadStatus`
- `saveStatus`
- `deleteStatus`
- `fieldErrors`

Quicklinks Panel 视图挂在主界面内部，不创建新的 WebView。它复用主窗口焦点和已有 `panel-host-region` 视觉空间，但不接入 public plugin panel lifecycle。

## Quicklinks Panel UI

布局参考 `examples/public-plugins/com.uipilot.notes` 的视觉结构，但实现为宿主前端组件：

```text
┌────────────────────────────────────────────────────────────┐
│ /quicklinks                                                │
├──────────────────────┬─────────────────────────────────────┤
│ 快速链接             │  目录名称  [京东搜索              ] │
│ + 新建               │  启动键    [/ jd                 ] │
│                      │  图标      [预览] [选择 PNG]       │
│  🛒 京东搜索 /jd     │  链接      [https://...{Query}    ] │
│  🔎 GitHub /gh       │                                     │
│                      │  [删除]                保存状态提示 │
└──────────────────────┴─────────────────────────────────────┘
```

交互：

- 左侧列表支持方向键切换。
- 新建按钮创建空 draft。
- 表单字段失焦时，如果 draft 已满足全部必填项，则调用保存；如果必填项缺失或字段非法，只更新本地字段错误，不写入 store。
- 图标通过按钮选择本地 PNG。
- 删除需要确认。
- Esc 退出 Quicklinks Panel，返回主界面。

## 从 Quicklinks Panel 补全命令

当焦点位于 Quicklinks Panel 左侧 list-item，例如 `/jd`，用户按 Enter：

1. 前端调用 `completeQuicklinkCommand('jd')`。
2. Quicklinks mode 退出，回到 `launcherMode='applications'`。
3. `query` 和 `queryControlValue` 设置为 `/jd `。
4. 递增 application query sequence，清空旧 results。
5. 主输入框获取焦点。
6. 触发一次搜索，显示 `/jd` 参数提示。

该流程不调用 public plugin 的 `closePluginPanel`，也不生成 public plugin session。

## 执行动作设计

`ResultAction` 增加：

```rust
OpenQuicklink {
    id: String,
    url: String,
}
```

执行要求：

- `execute_result` 解析到 `OpenQuicklink` 后重新解析 `url` 为 `url::Url`，只允许 http/https。
- 调用 `browser_open::open_url`。
- 成功后隐藏主窗口。
- 失败返回 `quicklinkOpenFailed`。

`id` 用于日志和未来统计；执行时不依赖前端传回模板或 query。

## 错误处理

- 启动键冲突：表单显示“启动键已被占用”。
- 启动键非法：表单显示“启动键只能以小写字母开头，包含小写字母、数字和短横线”。
- 图标不合法：提示“请选择 128x128 PNG 图片”。
- 链接模板不合法：提示“链接必须是 http/https，且包含 {Query}”。
- 打开浏览器失败：主界面状态提示“打开链接失败”。
- 配置文件损坏：提示“快速链接配置加载失败，已重置为空列表”，并保留损坏备份。

## 自动化验收

- 启动键 `jd` 通过校验，`1jd`、`-jd`、`Jd`、`jd_1` 被拒绝。
- `find`、`quicklinks`、`web-search` 作为 quicklink 启动键被拒绝。
- quicklink 与 public plugin install/rename 冲突时，两边都会拒绝冲突命令。
- URL 模板必须是 http/https 且包含 `{Query}`。
- `手机 A&B?` 被编码为 `%E6%89%8B%E6%9C%BA%20A%26B%3F`。
- 伪 PNG、损坏 PNG、`129x128`、`128x129` 被拒绝，`128x128` PNG 被接受。
- 配置文件损坏时生成备份，UI 得到固定错误码，列表为空。
- 保存 quicklink 后无需重启，`/jd` 立即出现在主界面结果中。
- `/jd` 无参数不自动执行，并显示“请输入参数”。
- `/jd 手机` 在 `submit=true` 时通过 `autoExecuteResultId` 自动执行。
- `/web-search 手机` 也通过 `autoExecuteResultId` 自动执行，避免 quicklinks 引入第二套前端硬编码。
- Quicklinks Panel 选中 `/jd` 后按 Enter，回到主界面并补全 `/jd `。
- Esc 退出 Quicklinks Panel，返回主界面。

## 人工验收

- 输入 `/quicklinks` 回车进入管理 Panel。
- 新增 `jd` quicklink，保存 128x128 PNG 图标和京东链接模板。
- 输入 `/jd` 时出现 `/jd` list-item，并提示输入参数。
- 输入 `/jd 手机` 回车，默认浏览器打开京东搜索链接。
- 在 Quicklinks Panel 中选中 `/jd` 并回车，返回主界面并补全 `/jd `。
- 修改 quicklink 名称、启动键、图标、链接模板后，主界面立即反映最新值。
- 删除 quicklink 后，`/jd` 不再匹配。

## 暂不包含

- 多级目录。
- 云同步。
- 批量导入/导出。
- 静态无参数链接。
- public plugin API 扩展。
- quicklink 使用次数排序。
