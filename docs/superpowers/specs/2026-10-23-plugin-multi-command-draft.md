# M2-A 多 mainResult 命令设计草案

状态：待独立规格评审；不是已批准实施计划，不是 M0/M1 的完成项。
日期：2026-10-23。基线：本地插件基座隔离工作区现有公开插件实现。
本草案只定义 M2 的第一条纵向切片，不授权扩大当前交付范围。

## 1. 交付场景与边界

一个本地插件包注册两条主结果命令，共用同一 Runtime、设置、存储和权限。
每条命令可分别搜索、输入参数、执行、改启动名称及收藏；重启、升级后身份稳定。
旧 `command` 包保持原有 mainResult/panel/window 行为，无需重打包。
本切片不增加多命令 panel/window、次级结果动作、onAction、插件间调用或每命令权限。
这些能力分别进入下一切片，不能由本草案中的命令身份字段推导为已支持。
API 继续 `apiVersion: 1`、`schemaVersion: 1`，通过新最低宿主版本区分能力。
暂建议功能门槛 `0.4.0`；该数字须在正式设计冻结前与宿主发布版本统一协调。
下文用 `MULTI_COMMAND_MIN_HOST` 代表最终确定的唯一版本，Rust/CLI/文档共用它。

## 2. 现状证据

- `src-tauri/src/public_plugins/manifest.rs`：PublicManifestV1 必需单个 command；PublicCommandV1 无 ID。
- `state.rs`：PluginStateDocument 的 default_name、name_override、favorite 目前均为插件级。
- `activation.rs`：route、suggestions、inventory 读取 manifest.command；panel_route 只接收 pluginId/input。
- `scheduler.rs`：PluginRequestCandidate 无 commandId；by_plugin 管理插件级队列。
- `runtime.rs`：PluginInvocation 无 commandId；启动代码只要求导出 onCommand。
- `public_plugins.rs`：pending submission 保存 route，以其 output_mode 解析响应。
- `commands.rs`：主结果发布将 copy_text 注册为私有 ResultAction::CopyText。
- `result_registry.rs`：不透明结果句柄已有请求、窗口作用域与结果集失效机制。
- `model.rs`、`src/protocol.ts`：收藏目标 PublicPlugin 目前只有 pluginId。
- `src/launcher-core.ts`：safeResultFavorite/favoriteMatchesResult 有严格形状与激活匹配检查。
- `src/public-plugin-panel.tsx`：插件管理页目前每个插件只有一个名称编辑入口。
- `packages/plugin-cli/src/manifest.ts`、`validate.ts`：语义验证与报告直接访问 manifest.command。

## 3. Manifest wire

旧形态原样接受；旧 `command` 不增加可选 id，避免出现两种默认身份解释。
新形态用 `commands` 替代 `command`；两个字段必须且只能出现一个，null 不视为缺失。
新增字段示例（其余包级字段沿现有 v1 合同）：

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "minimumHostVersion": "0.4.0",
  "commands": [
    {
      "id": "default",
      "defaultName": "text-upper",
      "summary": "转换为大写",
      "activationMode": "live",
      "outputMode": "mainResult",
      "inputRequired": true,
      "inputPlaceholder": "输入文本"
    },
    {
      "id": "lower",
      "defaultName": "text-lower",
      "summary": "转换为小写",
      "activationMode": "submit",
      "outputMode": "mainResult",
      "inputRequired": true,
      "inputPlaceholder": "输入文本"
    }
  ]
}
```

提议 commands 数量上限 16、下限 1；正式评审冻结此上限后同步 schema/语义验证。
id 使用 `^[a-z][a-z0-9-]{0,31}$`，包内唯一、大小写敏感且只能小写；不得用展示名生成。
defaultName 沿现有启动名规则；包内重复 defaultName 即拒绝，不能依赖用户改名修复。
所有 commands 项只接受 mainResult，activationMode 继续允许 live/submit。
summary/inputRequired/inputPlaceholder 的现有语义、长度和非法字符规则逐项适用。
commands 形态禁止 window/panel 字段及 ui.window/ui.panel、timer.control、clipboard.history.* 权限。
这防止声明目前只能经窗口/面板消费的能力却没有合法命令入口；其余权限仍走既有规则。
旧 command 形态的 panel/window、权限和最低版本校验保持原样。
commands 即使只有一项，也必须达到 MULTI_COMMAND_MIN_HOST，不降级为旧形态。
Rust 与 CLI 都先完成结构、互斥、逐项语义及版本校验，再暴露规范化命令集合。
JSON schema 与独立 CLI validator 必须表达字段互斥、数组上下限、必填 id 和输出模式限制。
重复 ID/名称等跨项规则由共享 fixture 校验 Rust/CLI 一致，不能只靠 schema。

## 4. 稳定命令身份与规范化

内部身份定义为 `(pluginId, commandId)`；generation、activation/admission、requestId 继续描述运行代次。
旧 command 永远规范化为 commandId `default`，与包版本、启动名、数组顺序无关。
新 commands 使用显式 id；`default` 可用于承接旧命令，也允许全新包不含 default。
id 改变视为删除旧命令并新增命令，不自动转移收藏、名称覆盖或待执行操作。
manifest wire 保留旧/新形态信息，避免规范化后无法决定旧 Runtime invocation 的兼容输出。
建立局部命令读取接口供 route/state/validator 使用，不复制整个插件 manager。
插件级 runtime.entry、网络授权、settings/storage、生命周期与故障状态继续共享。

## 5. 注册、搜索与管理展示

安装候选阶段为每条命令计算 effectiveName，并对整个命令集合一次性检查全局占用。
占用集合包括当前已安装插件命令、内置命令与 quicklinks；禁用插件沿现有保留名称策略。
替换同插件时仅排除该插件旧集合；新集合内部仍须检查彼此冲突。
一次安装/升级只发布完整命令表；任何冲突失败保留旧包、旧表与旧状态。
route 按 effectiveName 查到 commandId，之后依靠 ID 定位定义，不再用名称当身份。
输入处理、大小写、trim、inputRequired 与 live/submit 触发语义沿现有合同。
每条命令产生独立 suggestion；排序使用现有收藏优先及名称排序，末尾追加 commandId 稳定破同值。
首版使用包名加 `/effectiveName` 区分同包命令，summary 为命令级；不新增 command title 字段。
管理 inventory 显示一个插件及其 commands 列表，包级开关/设置保持单份。
每行分别改启动名；不把同一包展开成多个可独立卸载的“插件”。
宿主内部 inventory/protocol 同步更新，不要求旧前端连接新后端；旧插件包兼容另行保证。
命令 result activation 与 mainResultCommandContext 都携带 commandId，前端严格校验并保留它。
旧 panel/window 激活继续走原路径，在宿主内部只映射 default，禁止选取任意第一项。

## 6. Runtime、调度与响应归属

新 commands 包的 invocation 增加 `commandId: string`；其余 apiVersion/requestId/input/context 不变。
SDK PluginInvocation 使用可选 commandId，文档说明 commands 包的调用中该字段必有。
旧 command 包仍省略 commandId，避免破坏对 invocation 做严格键检查的旧 Runtime。
同一个 onCommand 通过 commandId 分派，宿主不新增每命令 JS 文件或 handler 导出约定。
PluginRequestCandidate、PublicPluginRoute 与 pending submission 保存规范化 commandId。
调度队列仍按 pluginId 串行/取消；不借此切片改变并发度、超时与已有替换规则。
排队、替换、延迟 dispatch 时，候选的 commandId 与 input 必须作为整体保存，不能从当前搜索框重建。
PluginRequestContext 的公开桥接形状可保持 pluginId/generation/requestId；宿主以 requestId 绑定候选。
Runtime 响应仍只回 requestId；插件不能自报另一个 commandId 来改变响应接收者。
完成时从绑定的 pending route 确定命令与输出类型，并校验原有生命周期有效性。
切换同包命令时延续 launcher query/intent 失效机制，旧响应不得替换当前命令结果。
改任一命令启动名可继续使整个插件请求队列失效，沿用现有保守策略，不新增并发优化。
收藏变更不改变命令身份、不重启 Runtime；沿用现有事务与结果更新机制。

## 7. 结果句柄与执行边界

本切片只保留现有可选 defaultAction.copyText；没有 actions 数组或自定义动作回调。
插件 result.id 只需在一次响应内唯一，两个命令可以返回同名结果。
宿主结果 ID 继续是不透明 registry handle，不拼接 `/命令名` 或信任插件提供的 ID 执行。
私有公开插件复制动作记录 commandId，执行时确认该命令仍存在于有效激活配置。
沿用 request/result handle、generation、结果集与主窗口作用域失效校验及 clipboard.write 授权检查。
补齐公开插件命令改名/升级/禁用/卸载的结果域失效路径；不能仅使 scheduler 失效而留旧复制句柄。
名称不是执行授权字段；新命令占用旧名称也不能接管先前结果句柄。
前端仍只提交 opaque request/result handle，不提交复制文本或任意 commandId 要求重解释旧动作。

## 8. 收藏、名称覆盖与旧 IPC

收藏目标增加 commandId；safeResultFavorite、favoriteMatchesResult、目标相等比较同步检查二元身份。
旧目标缺少 commandId 时只规范化为 default；宿主确认 default 存在后才能接受变更。
set_plugin_favorite 与 set_plugin_effective_name 增加可选 commandId 参数。
参数省略只针对 default；显式 ID 不存在则失败，不回退第一项，不批量修改同包命令。
新前端总是发送 commandId，旧参数兼容仅为保留默认命令调用路径。
mainResult 命令入口可收藏；执行结果本身不因此变成可收藏入口。
改名按 ID 修改单个 override，并以完整已安装命令表检查冲突；失败没有半提交。
恢复默认名先验证默认名可用，不能删除 override 后才发现冲突。
重复设置同一 favorite/name 值沿现有幂等行为处理。

## 9. 持久化迁移与升级删除规则

建议 PluginStateDocument 增加内部 schema 2，使用 commands 映射保存每个 ID 的 defaultName/nameOverride/favorite。
插件 enabled/fault/permissions/settings/generation/digest/revision 仍在包级；不复制秘密值。
schema 1 使用显式旧 DTO 读取并严格校验，内存规范化到 commands.default；禁止宽松忽略未知字段。
旧 default_name/name_override/favorite 无损映射到 default；不存在的 favorite 沿现有 false 默认值。
首次读取只做内存迁移；首次成功状态事务写 schema 2，复用当前 prepared commit/durable publish 流程。
持久化失败沿既有恢复语义处理，不提前发布迁移后 inventory，也不覆盖原有效 state。
同 ID 升级保留 override/favorite，用新 manifest 更新 defaultName，再整体检查名称冲突。
新增 ID 默认无 override、favorite=false；删除 ID 在成功提交时移除状态，不留下幽灵收藏或占名。
旧单命令升级为 commands 只有显式 id=default 能继承旧状态；遗漏 default 则按删除旧命令处理。
该删除语义在开发文档明确提示，不能按相同 defaultName 或数组第一项“智能迁移”。
同包版本回退到旧 command 只保留 default 的状态，其余命令按删除规则处理。
宿主二进制降级到不识别 state schema 2 的版本不在兼容保证内；正式发布说明必须明确此边界。
不新增静默降级写回 schema 1 的旁路；若产品要求宿主降级，应另行设计可逆存储迁移。

## 10. 快捷键证据与本切片约束

现有 `src-tauri/src/settings.rs` 的 hotkey 是宿主全局唤起键，默认 Shift+Space。
`src-tauri/src/lib.rs` 的 global shortcut handler 请求 ShowTarget::Launcher。
`src-tauri/src/commands.rs` 的 save_hotkey 修改这一宿主设置；没有已验证的插件命令快捷键映射。
因此本切片不创建命令快捷键 UI、持久化字段或系统注册，不迁移不存在的数据。
既有全局唤起键保持原样；启动器 Enter/Tab/Escape 操作随 commandId 完整路由并回归测试。
后续若增加每命令快捷键，目标必须是 `(pluginId, commandId)`，另行设计冲突、注册失败与失效合同。

## 11. 拒绝行为与版本兼容

command/commands 同时出现、都缺失、null、空数组、超限、重复 ID/名称、非法 ID：拒绝整个包。
commands 中出现 panel/window 或禁止字段/权限：拒绝，不忽略、不只注册可支持的项。
commands 低于功能门槛或高于当前宿主版本：按既有不兼容/非法包错误分类，Rust/CLI fixture 锁定结果。
旧宿主严格解析会拒绝新 commands 包；只承诺“新宿主运行旧包”，不承诺反向兼容。
旧 schema/API 版本号不变不代表旧宿主能加载新字段；教程必须明确最低宿主要求。
非法命令目标或过期句柄返回既有管理/执行失败类型，不将未知 ID 当作默认命令。
全部兼容策略基于显式 ID；没有在线迁移服务、自动下载或远程发布环节。

## 12. 实施文件地图（待正式计划拆分）

- Rust 合同/状态：public_plugins/manifest.rs、state.rs、state_tests.rs、activation_bundle.rs。
- 路由/运行：public_plugins/activation.rs、scheduler.rs、runtime.rs、src-tauri/src/public_plugins.rs。
- 宿主协议/执行：src-tauri/src/commands.rs、model.rs、result_registry.rs。
- 旧 UI 模式回归：plugin_panel.rs、plugin_window.rs；仅在类型传播必要时改动生产代码。
- 前端：src/protocol.ts、launcher-core.ts、launcher-view.tsx、public-plugin-panel.tsx 及对应测试。
- CLI：packages/plugin-cli/src/manifest.ts、validate.ts、contracts.ts 及 manifest/validate fixtures。
- SDK：docs/plugin-sdk/public-plugin-v1.md、uipilot-plugin-api-v1.d.ts、public-plugin-developer-guide.md。
- 生成链：generate_public_plugin_schema.rs、SDK schema、CLI schema 与 generated validator；通过现有脚本再生。
- 验收示例：新增一个双 mainResult 包及其 runtime/SDK contract 测试；旧 demo 不为兼容性修改 manifest。

## 13. 关键验收与评审退出条件

1. 同一包 live 与 submit 两命令可分别发现、输入、执行，Runtime 收到正确稳定 ID，仍只有一个 runtime。
2. 两条命令可用相同插件 result.id；交错响应、切换输入及待执行旧句柄不会串命令。
3. 独立收藏/改名、重启、调整 commands 顺序、修改默认名、升级/删除/恢复命令符合上述映射规则。
4. 同包、他包、内置、quicklinks 冲突与恢复默认名失败均保持旧配置；无半安装或部分命令可见。
5. schema 1（含缺失 favorite）迁移、schema 2 重读、持久化失败、损坏/未知 schema 均有真实状态 fixture。
6. 旧 demo-return/demo-panel/demo-win/pomodoro 原包通过；旧 invocation 键集合不改变。
7. Rust/CLI/生成 schema 对新旧合法包与全部拒绝矩阵一致；CLI 报告支持每命令诊断且不破坏旧 validate 合同。
8. 权限撤销、禁用、更新、卸载、改名后复制与响应失效；收藏操作不误取消有效业务调用。
9. 主界面命令建议/收藏右键/名称编辑覆盖键盘与双主题；自动测试与真实 Host 人工验收分别记录。
10. 正式设计冻结实际最低宿主版本、命令上限与持久化降级边界后，才编写实施计划并进入实现。
