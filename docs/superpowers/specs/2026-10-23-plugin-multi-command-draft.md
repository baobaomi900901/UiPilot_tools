# M2-A 多 mainResult 命令设计

状态：已通过独立设计评审，主 agent 依据现有自主规划与开发授权正式纳入；实施以配套计划及逐任务评审为门禁。
日期：2026-10-23。范围：本地插件基座隔离 worktree。
基线：M4 最终源码 `761a9e4` 已独立复审 PASS / Approved，开放问题为零；完成记录 `430be00`。真实 Windows Host 人工验收仍 pending。
本文采用 0.5.0、16 项上限、一个替换式启动名覆盖、严格 state schema 2 和增量 CLI 报告，规范性继承 [M4 设计](2026-10-23-plugin-credential-consumption-draft.md)第 6–6.2 节。实现顺序见 [M2-A 实施计划](../plans/2026-10-23-local-plugin-foundation-m2a.md)。

## 1. 交付场景、阶段边界与版本

一个本地插件包注册两条 mainResult 主结果命令，共用一个 Runtime、设置、存储、权限、网络授权和包级生命周期。每条命令可独立搜索、输入参数、执行、改启动名称和收藏；重启、升级及调整数组顺序后身份稳定。旧 `command` 包保持原有 mainResult/panel/window 行为，无需重打包。

M2-A 交付稳定 `(pluginId, commandId)`、多 mainResult 注册、独立收藏及每命令一个启动名覆盖。路线图中的“别名身份”在本切片仅指该既有覆盖：覆盖替换 effectiveName，释放旧路由，不使原名成为同时有效的第二别名。独立别名集合与每命令快捷键另行设计。

本切片不增加多命令 panel/window、次级结果 actions 数组、统一动作合同、`onAction`、插件间调用或每命令权限。完整 M2 仍需已评审并实现的统一结果动作切片，以及对快捷键范围的明确处置。M2-A 完成不代表完整 M2 完成，不满足 M3 对完整 M2 的依赖，也不冻结 M3 所需动作合同。

Manifest 继续 `schemaVersion: 1`、`apiVersion: 1`；M2-A 首次发布宿主版本为 **0.5.0**，唯一能力常量 `MULTI_COMMAND_MIN_HOST = 0.5.0`，Rust、CLI、schema 语义说明及 SDK 文档一致。所有 `commands` 包，包括单项数组，都必须声明 `minimumHostVersion >= 0.5.0`；声明高于运行目标版本仍不兼容。数组固定为 **1..16** 项。

M4 凭据的既有 0.4.0 门槛及其他历史功能门槛不变；不能把旧示例最低版本普遍提高。发布时同步根 package/package-lock、Cargo package/lock、tauri 配置、Host current、CLI Host target 与相关版本断言；CLI 包自身版本不等于宿主目标版本。本文不执行任何版本修改。Manifest/API schema 1、内部 state schema 2、CLI report schema 1 是三个独立版本，不能互相替代。

## 2. 现有接口与预期改动范围

当前 manifest/CLI 读取单个 `command`，命令没有 ID；状态中的 defaultName/nameOverride/favorite 为插件级。路由、建议、inventory、scheduler 候选、Runtime invocation 与 pending submission 需传播规范化命令身份。收藏协议、前端严格形状检查和管理名称编辑同样需更新。已有 opaque result registry、请求/窗口/结果集失效、prepared state commit 和 M4 请求失效/authority 机制继续复用；不复制整个插件 manager，不另建调度器或凭据状态机。

实施使用上述已评审 M4 基线。最终两项 UI/文档修正已核对，不改变本设计的源代码假设；原工作区未提交改动不纳入。

## 3. Manifest wire 与静态验证

旧形态原样接受；旧 `command` 不增加可选 `id`。新形态用 `commands` 替代 `command`；二者必须且只能出现一个，显式 null 不视为缺失。以下仅展示相关字段，其余包级 v1 必填字段仍必需：

```json
{
  "schemaVersion": 1,
  "apiVersion": 1,
  "minimumHostVersion": "0.5.0",
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

`commands` 长度 1..16。`id` 必填，匹配 `^[a-z][a-z0-9-]{0,31}$`；包内唯一、大小写敏感，仅小写，不自动 trim，不从展示名或位置生成。`defaultName` 使用现有启动名校验；包内重复 defaultName 拒绝，不能依赖用户改名修复。

每项只接受 `outputMode: "mainResult"`；`activationMode` 允许 live/submit。summary/inputRequired/inputPlaceholder 的现有语义、长度、非法字符及未知字段规则逐项适用。`commands` 形态禁止 window/panel 字段以及 `ui.window`、`ui.panel`、`timer.control`、`clipboard.history.*` 权限，避免声明本切片没有合法入口的能力；其余权限沿现有规则。旧 `command` 的 panel/window、权限及最低版本校验保持既有合同。

Rust 与 CLI 在暴露规范化集合之前完成结构、互斥、逐项语义及版本校验。生成 JSON schema 与独立 CLI validator 表达互斥、数组上下限、必填 ID、输出模式和禁止字段；跨项重复 ID/名称、引用及版本等 schema 无法完整表达的规则由 Rust/CLI 共享 fixture 锁定。不能只靠 schema 或只在安装注册时发现。

同时出现 command/commands、都缺失、任一显式 null、空数组、17 项及以上、重复/非法 ID、重复名称或混入不支持模式/字段/权限，均拒绝整个包，不忽略、不注册部分项。低于能力门槛或高于当前目标的最低版本按既有非法包/不兼容分类处理，并用 fixture 固定 Rust/CLI 一致性。

## 4. 稳定身份与规范化

内部命令身份为 `(pluginId, commandId)`；generation、activation/admission、requestId 继续表示运行代次与请求生命周期，不用 commandId 替代它们。旧 `command` 永远规范化到 `default`；包版本、启动名或数组顺序不影响身份。新 `commands` 使用显式 ID，可以用 `default` 承接旧命令，也允许全新包完全不含 default。

`constructor` 符合 ID 语法，是合法命令 ID；不得因 JavaScript 原型属性同名而拒绝、误路由或继承其他值，也不新增保留 ID 规则。`commands` 映射与所有消费者只按实际命令条目解释身份。

改变 ID 等同删除旧命令并新增命令；不按相同 defaultName、effectiveName 或数组第一项转移收藏、名称覆盖或待执行操作。Manifest 保留旧/新 wire 形态信息，以决定 Runtime invocation 是否输出 commandId。局部命令访问接口供 route/state/validator 使用，runtime.entry、settings/storage、权限、故障及生命周期仍为包级共享。

## 5. 原子注册、搜索与管理展示

安装/升级候选为每条命令计算 effectiveName，并一次性对完整命令集检查全局占用。占用包括已安装插件命令、内置命令及 quicklinks；禁用插件沿现有名称保留规则。替换同插件只排除该插件旧集合，新集合内部仍检查彼此冲突。

验证或冲突拒绝不改旧包、旧命令表或旧状态；成功事务只发布完整新命令表。对确定未提交且旧字节复核成功的持久化失败同样不发布候选。持久结果不确定或旧字节复核失败不能承诺旧可用性，按第 10–11 节 fail-closed 合同处理，不能用“失败必定完整回滚”覆盖安全恢复规则。

route 先按 effectiveName 找到 commandId，此后依 ID 定位定义；名称不是身份。输入大小写、trim、inputRequired、live/submit 触发语义沿现有合同。每条命令独立生成 suggestion；使用现有收藏优先和名称排序，末尾以 commandId 稳定破同值。展示用包名加 `/effectiveName` 区分同包命令，summary 为命令级，不新增 command title。

管理 inventory 仍是一份插件及其 commands 列表，包级开关/设置一份；每行独立改名，不变成可独立卸载的多个插件。主结果 activation 与 mainResultCommandContext 必须携带经验证 commandId，前端严格校验并保留。宿主 inventory/protocol 同步更新，不要求旧前端连接新后端；旧包兼容另行保证。旧 panel/window 激活走既有路径，在宿主内部只映射 default，禁止任意取第一项。

所有 launcher、名称编辑、收藏菜单及插件管理表面遵守 `docs/ui-guidelines.md` 的面板、主题、状态、焦点、键盘和无障碍规则；自动检查与真实 Host 验收分别记录。

## 6. Runtime、请求归属及同一 Runtime 的请求失效

新 `commands` 包 invocation 包含 `commandId: string`，其余 apiVersion/requestId/input/context 保持原合同。SDK `PluginInvocation.commandId` 可选，文档明确新形态调用必有；旧 `command` invocation 完全省略该 key，保持旧 Runtime 严格键集合兼容。同一个 onCommand 按 ID 分派，不新增每命令 JS 文件、独立 handler 导出或 Runtime。

PluginRequestCandidate、PublicPluginRoute 与 pending submission 保存规范化 commandId。队列仍按 pluginId 串行/取消，沿现有并发度、超时及替换规则。等待、替换或延迟 dispatch 必须整体保存 commandId/input，不能从当前查询框重建。公开 PluginRequestContext 桥接仍可保持 pluginId/generation/requestId，宿主以 requestId 绑定候选。

Runtime 响应仍只返回 requestId，不能自报 commandId 改变接收者。完成时由绑定 pending route 决定命令及输出类型，并验证原生命周期。切换同包命令沿 launcher query/intent 失效，旧响应不得替换当前命令结果。

真正成功的 nameOverride 状态变化（包括 effectiveName 不变的覆盖设置或清除）统一沿用保守的整插件请求失效范围；若保留同一 Runtime/代次，必须：

1. 把 running 标为 non-current，同时保留 running 占用槽直到 handler 完成或既有 deadline/rebuild 路径；移除 waiting，并将受影响的精确 request context/submission token 交由 service 结算。
2. 使旧请求上下文及其网络调用失效；后续任一兄弟命令只能排队，不能进入仍忙的 Runtime。
3. 旧 handler 的迟到完成不接受业务结果，但仍执行正常清理与队列推进。只有真正 destroy/rebuild Runtime 的路径可丢弃 running 槽。
4. 成功改名同时失效第 7 节要求的结果句柄域；不能只使 scheduler 中的请求失效 而保留可复制旧句柄。

仅当请求的目标 nameOverride 与该命令当前存储的 nameOverride 完全相同（明确区分 null 与字符串）时，改名才是 no-op：不使有效请求失效、不持久化、不仅为迁移写 schema 2。effectiveName 相同本身不能判定 no-op。清除已有覆盖，或将无覆盖改为显式覆盖，即使当前 effectiveName 不变，也必须按真实状态变更完成名称冲突验证及 prepared commit/durable publish；不得丢弃用户选择的覆盖状态。准备失败，或 `NotCommitted` 且旧字节复核成功，不发布新名称或候选状态。`Unknown`/旧字节复核失败沿既有 fail-closed 路径处理；若真实的请求失效效果已经发生，即使管理调用返回错误，service 仍须结算全部精确 effects，不能因提前返回丢失 token。该合同复用 M4 请求失效/service-effects 机制，不引入每命令队列或并发优化。

收藏变化不改变身份、不重启 Runtime，不应误取消有效业务请求；沿现有事务与结果更新机制，同时遵守所有 authority 重开规则。

## 7. 结果句柄、复制与执行边界

本切片仅保留现有可选 defaultAction.copyText，没有 actions 数组或自定义动作回调。插件 result.id 只需在同一次响应内唯一；不同命令可以返回相同 result.id。宿主结果 ID 始终是 opaque registry handle，不拼接启动名或信任插件 ID 执行。

私有公开插件复制动作保存 commandId，执行时确认该命令仍属于当前有效 activation 配置。继续验证 request/result handle、generation、结果集与主窗口作用域，以及 clipboard.write 授权。补齐改名、升级、禁用、卸载及相应权限撤销的结果域失效，不能只改 scheduler。

名称不是授权字段：新命令占用旧名也不能接管旧句柄。前端只提交 opaque request/result handle，不提交复制文本或要求用任意 commandId 重解释旧动作。过期/非法目标返回既有执行失败类型；相同插件结果 ID、重排序及新名称占用都不绕过 registry 生命周期。

## 8. 收藏、单一名称覆盖与严格旧 IPC

收藏目标加入 commandId；safeResultFavorite、favoriteMatchesResult、目标相等比较同步验证 `(pluginId, commandId)` 及有效 activation 匹配。mainResult 命令入口可收藏；执行结果本身不因此成为可收藏入口。

`set_plugin_favorite` 与 `set_plugin_effective_name` 增加可选 commandId 参数，新前端总发送它。只有文档列明的旧收藏/管理路径上**字段缺失**才规范化到 default，并须确认 default 存在。显式 null、空字符串、格式非法、未知 ID 一律失败；缺失 ID 而 default 不存在也失败。不把 serde Option 的 null 当缺失，不回退第一项，不批量修改兄弟命令。新 activation/context payload 要求 validated commandId，不开放旧参数缺失兼容。旧 Runtime invocation 仍按第 6 节省略新 key，二者不可混淆。

按 ID 仅修改目标命令的一个 nameOverride；override 替换默认启动名，不保留附加路由。改 A 不改 B，改名保留 A 的 ID/favorite，并在正常全局冲突规则下释放旧名。改名以完整已安装命令表检查冲突；恢复默认名先确认其可用，不能先删覆盖再失败。重复 favorite 值或完全相同的目标 nameOverride 才是幂等 no-op，不仅为格式转换增加 revision/generation 或写盘。真正改名遵守第 6 节的请求失效处理及第 7 节句柄失效。

## 9. 精确 state schema 2

持久所有者文件仍为 `state.json`；`inventory.json` 保持独立 schema 1。使用显式 schema-1/schema-2 DTO，两者均拒绝未知字段，不能宽松吞掉无法理解的数据。以下是实际序列化 wire keys，不是新增公开 SDK 类型：

```ts
type CommandStateV2 = {
  defaultName: string
  nameOverride: string | null
  favorite: boolean
}

type PluginStateDocumentV2 = {
  schema: 2
  pluginId: string
  version: string
  commands: Record<string, CommandStateV2>
  installed: boolean
  enabled: boolean
  fault?: 'runtimeUnavailable' | 'consecutiveFailures' | null
  permissionGrants: PublicPermission[]
  networkHttpsHostsGrant?: string[]
  networkCredentialsGrant?: PluginNetworkCredential[]
  inventoryRevision: number // Rust u64，沿现有表示
  activeGeneration?: number // Rust u64，缺失默认 0
  packageDigest?: string | null
  settings: Record<string, JsonValue>
}
```

`commands` 必填，1..16 个条目，map key 使用 manifest 同一 ID 语法。每条目仅含上述三个必填 key，不在 value 重复 ID；`nameOverride: null` 表示无覆盖，schema 2 缺失/null favorite 均非法，缺失 nameOverride 也非法。拒绝未知条目字段及重复 JSON command key，禁止 last-value-wins；采用 `BTreeMap` 确定性序列化。defaultName 与非 null nameOverride 使用既有名称校验，注册前验证完整 effectiveName 集合。

根默认值保留既有合同：缺失 fault -> None；缺失 networkHttpsHostsGrant -> 空；缺失 networkCredentialsGrant -> 空且空时写出省略；缺失 activeGeneration -> 0；缺失 packageDigest -> None。其他列出的根字段必填。根 Option 字段既有可选/null 语义不变；数组字段的缺失默认不意味着 null 合法。写端输出当前规范完整根表示，仅继续省略空 credentials。package digest/generation、settings、权限语义及网络 grant 修复规则不变。仅 defaultName/nameOverride/favorite 从根移入 commands；schema 2 拒绝这三个旧根字段。

enabled、fault、permissions、settings、generation、digest、revision 与 host/credential grants 都保持包级；命令状态不复制秘密、明文或密文。retainData 卸载保留 commands 条目并清空全部 favorite，沿用保留数据后的改名行为，不创建空 commands tombstone。授权及秘密的卸载清理沿 M4/既有 retainData 合同，不把保留命令状态等同保留网络授权。

## 10. schema 1 首读、事务与失败恢复

有效 primary schema-1 文件以严格旧 DTO 读取，包括缺失 favorite -> false，然后仅在内存无损规范化为 commands.default。该纯表示转换可供 inventory 与启动路由读取，无迁移专用写入、revision 或 generation 递增；StoredState.raw 保留原始 schema-1 精确字节。首次真正成功的状态变更事务通过现有 prepared commit/durable publish 写 schema-2 候选后发布。no-op 不强迫迁移；正常启动激活本来需要的合法事务仍可发生，不能把其写入误称迁移专用写。

纯表示转换与安全修复严格区分。backup 恢复或权限/网络语义修复必须先可靠持久化修复状态才能发布，修复可以同时写 schema 2。backup 恢复先移除 network.https、host grant 与 credential grant；迁移不能恢复已被恢复流程丢弃的授权。语义坏 grant 的降权与结构损坏的 quarantine/fallback 沿 M4 合同，不把结构损坏伪装为只修一个字段。无法可靠持久化修复时不发布网络 authority/相应激活，保留固定 storage 诊断及数据供修复。

首次变更失败为 `NotCommitted` 且 previous bytes 复核成功时，先前可读的规范化状态、revision 与 inventory 保持，原 schema-1 字节仍是权威；重试成功才写 schema 2。`Unknown` 或旧字节复核失败沿既有 fail-closed 恢复，不承诺可用性不变、事务回滚或恢复凭据授权。任何失败都不把候选变更当已提交状态发布。真实产生的请求失效/取消 effects 仍依第 6 节在错误出口结算。

未知 schema、缺失/未知字段、命令重复 key、非法名称/ID、无效 grants 等依既有恢复/quarantine 分类处理；不能忽略未知版本或静默放宽 schema 2。

## 11. M4 authority、确认发布与终止继承

M2-A **规范性继承 M4 设计第 6–6.2 节**，包括锁序、request-context 失效、网络 admission/deliver、秘密分离、attempt 预算及 checked publication。本文不另建 authority 状态机。M4 实现、自动检查及独立评审已完成；真实 Windows Host 人工验收单独记录。

任何网络 authority 的发布或重新开放都必须经统一 checked 路径，以当前已确认 bundle、durable state、Runtime identity 和 secret 验证为依据；不能从旧 bundle 快照复活授权。未清除的 RESERVED/COMMITTING/DURABLE reservation 都阻止重开。成功路径先发布已提交 state，再发布 reservation bundle 并清除 reservation，之后才 checked 重开；确认须遵守 M4 的 mutation -> authority 保护及 book/terminal 复核，不以 state==bundle 单独替代确认。

启动/启用、安装升级、网络授权、settings/secret 保存、运行恢复、timeout replacement 及本设计改名、收藏、迁移、包回退中任何实际重开都受同一约束。backup 恢复必须先持久撤销网络权限与两份 grants；格式迁移不恢复授权。未知持久结果或 prior-byte 复核失败不能通过旧 bundle 绕过 durable 校验。credential 缺失、坏记录、解密失败及 bearer 格式行为继续依 M4，不缓存秘密或建立新恢复状态。

宿主 shutdown 对网络 admission/delivery 是终态：延迟迁移、恢复、改名、收藏或包回退完成均不能重开。包回退重新进行普通权限/凭据确认与验证，不复活旧请求上下文。关闭/禁用/卸载/退出路径只能关闭，不能因通用 helper 意外重开。未来合法恢复只允许新的 request，不恢复已失效 context。

## 12. 升级、删除、包回退与二进制降级

同 ID 升级保留 override/favorite，以新 manifest 替换 defaultName，再对完整候选集检查名称冲突。新增 ID 默认为 nameOverride=null、favorite=false；删除 ID 只在成功提交时移除，不能留下幽灵收藏或占名。删除、重启、再加入同 ID 是全新状态。

旧单命令升级为 commands，仅显式 default 继承旧值；未包含 default 即删除旧命令，不按名称/第一项迁移。新宿主内包版本回退到旧 command 只保留当时存活 default 的状态，其余 ID 按删除规则处理。若 default 已被删除，再回退旧包则创建 fresh default；不从历史或 backup 搜索已删除身份。

包回退在新宿主中受支持，遵循普通原子安装、冲突、授权及生命周期校验。宿主二进制回退则不同：一旦正常事务或必要恢复写出 state schema 2，只懂 schema 1 的旧宿主不保证能使用该数据目录。发布说明必须明确这一边界；不静默写回 schema 1，不把 schema-1 backup 当降级功能，不推断恢复旧授权。若以后要求二进制可逆降级，需另行设计可逆迁移。

旧宿主严格解析会拒绝新 commands 包；只保证新宿主运行旧包，不保证反向兼容。最低版本及 schema/API 1 的含义必须在开发教程中明确。没有在线迁移服务、自动下载或远程发布环节。

## 13. CLI 报告、定位与人类输出

保留 report `schemaVersion: 1`、现有 exit codes、pack 嵌入 validate report 和错误分类。增量类型为：

```ts
type ReportPlugin = {
  pluginId: string
  version: string
  outputMode: PluginOutputMode
  commands?: Array<{
    commandId: string
    defaultName: string
    activationMode: 'live' | 'submit'
    outputMode: 'mainResult'
  }>
}

type PluginValidationIssue = {
  code: string // 沿现有 code 联合/合同
  path?: string
  location?: ExistingIssueLocation
  message: string
  commandId?: string
}
// 内部 ManifestValidationIssue 同样增加 commandId?: string；其余原有字段/类型不变。
```

旧 command 的 JSON plugin keys、diagnostic 字段与人类格式原样保留，省略 commands 及 issue.commandId。宿主 target 随正式发布同步，不提高 report schema。保证旧严格消费者处理旧包时字段不变，不承诺它们已理解新数组形态。

commands 包 `plugin.outputMode` 固定为同质聚合 mainResult，不从第一项选择；plugin.commands 按 manifest 数组顺序输出，单项数组也保留新形态。报告描述声明，不代表成功注册。将来允许混合输出模式时必须重新决定聚合/报告版本，不能静默选首项。

metadata 可用性沿现有阶段：schema-invalid manifest 不输出 plugin；schema-valid 即使后续 semantic/API/platform 检查失败也可带 plugin 声明元数据。调整数组顺序改变 report 顺序和 pointers，不改变命令身份。

命令局部 issue 使用 `path: "plugin.json"`，以及 `location: {kind:"jsonPointer",value:"/commands/<index>/<field>"}`；整项错误可指 item。仅当该项 ID 语法有效且在解析数组中唯一时附 commandId。缺失、非法或重复 ID 都省略 commandId，以精确 pointer 定位；不以 defaultName 或 index 伪造身份。重复 ID 的 diagnostics 指各 offending item pointers；重复 defaultName 可携带各自有效唯一 ID。

集合/包级错误用 /commands、/minimumHostVersion 或相关包 pointer，不带 commandId。共享 Runtime 资源缺失不能归到任意命令；非法 JSON 保留既有 byte-offset 定位。保留现有 codes、collector 100 项上限、truncation、phase 顺序和确定性去重。不同命令的 semantic findings 必须有不同 pointer/rule rank，使 collector 不误合并两条命令的错误，不用展示名作去重身份。

新形态人类输出展示命令上下文，例如：

```text
plugin.json: MANIFEST_SEMANTIC_INVALID [command lower at /commands/1/inputPlaceholder]: ...
```

ID 不可用时仅显示 exact pointer；不能只给 JSON 增字段而让人类输出无法区分命令。legacy 人类格式保持不变。目录、archive 与 pack 嵌入报告遵守同一规则。

## 14. 快捷键与未来阶段

现有 settings hotkey 是宿主全局唤起键，默认 Shift+Space；global shortcut handler 请求 Launcher，save_hotkey 修改此宿主设置。目前没有已验证的插件命令快捷键映射，因此本切片不新增命令快捷键 UI、持久化字段、系统注册或迁移不存在的数据。既有全局唤起键保持；Enter/Tab/Escape 仍沿 commandId 完整路由并回归。后续每命令快捷键必须以二元身份为目标，另行定义冲突、注册失败及失效合同。

统一 actions/onAction、多命令 panel/window、声明式业务调用及 M3 依赖另立规格。不能因本设计预留命令身份就宣称这些能力已交付。

## 15. 后续实施文件地图

本节是配套实施计划的文件地图；任务边界、依赖和验证以已批准计划为准。

- Rust 合同/状态：public_plugins/manifest.rs、state.rs、state_tests.rs、activation_bundle.rs。
- 路由/运行/请求失效：public_plugins/activation.rs、scheduler.rs、runtime.rs、src-tauri/src/public_plugins.rs。
- 宿主协议/执行：src-tauri/src/commands.rs、model.rs、result_registry.rs；沿 M4 service-effects 和 checked authority 发布接口。
- 旧 UI 模式回归：plugin_panel.rs、plugin_window.rs，仅在类型传播必要时改变生产代码。
- 前端：src/protocol.ts、launcher-core.ts、launcher-view.tsx、public-plugin-panel.tsx 及相关测试。
- CLI：packages/plugin-cli/src/manifest.ts、validate.ts、contracts.ts、report.ts、cli.ts 及对应 fixtures。
- SDK：public-plugin-v1.md、uipilot-plugin-api-v1.d.ts、public-plugin-developer-guide.md；按既有生成器再生宿主/CLI schema 与 generated validator，禁止手改生成 validator。
- 示例：新增双 mainResult 包及 runtime/SDK contract 测试；旧 demo 不为兼容性改 manifest。

配套计划遵守仓库 implementation-plan-guidelines；本规格负责技术合同，不在每个任务重复完整技术设计。

## 16. 独立验收矩阵

下列均为后续实现必须提供的验收；本文未执行代码测试，也未把自动测试等同真实 Host 人工验收。

1. **版本与上下界**：0.4.0 minimum 拒绝 commands；目标 0.5.0 接受合法 0.5.0 包，未来 minimum 拒绝；0/17 拒绝、1/16 接受。legacy M4 凭据仍按自身 0.4.0 门槛，CLI 包版本与 Host target 不混淆。
2. **结构矩阵**：command/commands 双有/双无/null、空数组、未知字段、非法 ID、重复 ID/名称、混合 panel/window、禁止权限逐项拒绝整包；Rust/schema/CLI 合法集与拒绝分类一致。
3. **纵向与队列**：同包 live A、submit B 独立发现、输入、执行，只有一个 Runtime；新 invocation 必有正确 ID，legacy invocation key 集合不变。等待/替换/延迟 dispatch 保留 B 的精确 ID/input，不从当前 query 重建。
4. **严格 ID**：旧管理/收藏字段缺失且 default 存在才映射 default；显式 null、空、非法、未知均拒绝；缺失且无 default 拒绝。新 activation/context 缺失/null/空/未知拒绝；前端匹配错误兄弟 ID 拒绝，不回退首项。legacy Runtime 仍省略 key。所有语法合法 ID（明确包含 `constructor`）均能正确 route/store/report/favorite，不受 inherited-property 混淆；不通过新增禁用字规避。
5. **响应归属与句柄**：A/B 可返回相同插件 result.id；交错响应、切换 input/intent、重排序、占用旧名均不串命令。旧 request 的结果、复制 handle、其他窗口/结果集句柄不接受；前端不能提供文本或 commandId 重解释句柄。
6. **收藏与改名隔离**：独立 favorite/override 跨重启保留；改 A 不改 B，旧名释放且不保留第二路由，ID/favorite 不变。恢复默认名冲突、同包/他包/内置/quicklinks 冲突均无半提交。收藏不取消有效业务调用。补充覆盖：default=old、override=custom → 同 ID 升级 default=custom → 恢复默认成功持久化 override=null（虽 effectiveName 未变）→ 重启及再次升级 default=new 后 effectiveName=new；相反 null→显式同名 override 也保存该选择，只有重复相同 override 才 no-op。
7. **同 Runtime 改名占用**：A 的 non-cooperative handler 未完成时改 A 或兄弟 B，再提交 B/input。running 变 non-current 但槽保留，B 不提前 dispatch；已失效的 running/waiting 的精确 context/token 均在 service 结算，旧网络 context、completion 业务结果及旧 handle 拒绝。旧完成正常推进 B，或旧 handler 不结束时沿 timeout/rebuild 派发保存的 B ID/input。仅完全相同的目标 nameOverride（区分 null 与字符串）才 no-op，不取消、不写盘、不迁移。管理错误若已产生真实 effects，也必须结算。
8. **schema-1 纯首读**：有效 primary、含/缺 favorite 分别精确映射 default；raw bytes、revision 不变，无迁移专用写，可读 inventory/route；与正常 startup activation 自身事务分开观察。
9. **首个事务失败**：favorite/rename 的确定 NotCommitted+复核成功保留旧 bytes/visible 值；重试只成功写 schema 2 一次。Unknown 与 failed revalidation 单独验证 fail-closed，不断言旧可用性不变、不发布 candidate。
10. **安全恢复**：语义坏 grant primary、结构坏 primary、current 缺失或 backup 恢复分别覆盖；安全撤权须 durable 后才发布，写失败不 expose authority。成功修复可写/重读 schema 2；raw 对应实际修复字节，迁移不恢复 backup 旧授权。
11. **严格 schema 2**：完整 round-trip；缺 commands/条目 key、null favorite、未知条目/根 key、重复 command JSON key、非法 key/名称、0/17 条目、旧根 defaultName/nameOverride/favorite、未知 schema、坏 grants 均验证恢复/拒绝分类。根默认值与空 credentials 省略按本文精确覆盖，inventory 仍 schema 1。retainData 卸载保留条目/覆盖但清空所有收藏及按 M4 清理授权。
12. **升级和删除**：同 ID 保留 override/favorite、采用新 defaultName；新增无覆盖/false，删除原子移除。删除→重启→重加 fresh；legacy→无 default 的 commands→旧包回退创建 fresh default。按名或数组位置迁移必须失败。新宿主包回退与不保证旧二进制读 schema 2 分别记录。
13. **authority 与终止**：M4 每个 authority 生产发布入口及任何新增入口使用 confirmed bundle/durable state/runtime/secret 检查；RESERVED/COMMITTING/DURABLE 阻止重开，成功 state/bundle 发布后才重开。Unknown/failed revalidation 不从旧 bundle 复活；backup先 durable 撤权。shutdown 后延迟迁移/rename/favorite/恢复/包回退均不恢复 admission/deliver，旧请求永不复活。
14. **CLI legacy golden**：legacy 合法/非法目录、archive JSON 及人类格式不增加新字段；pack 嵌入相同报告，report schema/exit codes/collector 限额不变。
15. **CLI 新形态**：双命令与单项数组报告有有序 commands、聚合 mainResult；schema-invalid 无 plugin，schema-valid semantic/API/platform 失败可带声明元数据。两命令同时错误去重后都保留；missing/invalid/duplicate ID 只用 pointer，duplicate-name 保留有效唯一 IDs。重排序改变 pointers 不改身份；共享资源错误无 commandId；人类输出提供命令或 exact pointer。验证 100 项上限、截断及 phase 稳定性。
16. **兼容及 UI**：demo-return/demo-panel/demo-win/pomodoro 原包通过，panel/window 内部仅 default；权限撤销、禁用、升级、卸载、改名后的响应/复制失效。建议、收藏右键、名称编辑、busy/错误状态覆盖键盘、焦点、双主题及无障碍；真实 Windows Host 安装/交互验收单独记录。
17. **阶段退出**：M4 实现与自动门禁及最终独立评审已完成，本规格和配套计划已正式纳入；每个依赖任务须等待前项 Spec PASS 与 Quality Approved。真实 Windows Host 人工验收独立登记，不由自动检查替代。M2-A 验收通过后仍不把完整 M2、命令热键/别名集合、统一 actions/onAction 或 M3 标为已交付。

## 17. 当前交接状态

设计与八任务计划均已通过独立评审，由主 agent 按既有自主开发授权正式纳入。实现从 Task 1 开始，依序经过测试及独立规格/质量评审；当前不宣称任何 M2-A 功能已实现。本文没有剩余产品选择交给实现者临场决定。
