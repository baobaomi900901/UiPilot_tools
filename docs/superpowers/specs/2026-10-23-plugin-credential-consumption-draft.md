# M4 凭据消费设计

状态：独立规格评审通过，网络开关调度与attempt预算意见已纳入；作为M4实现合同，不代表实现完成。日期：2026-10-23。
M0/M1已完成；当前执行M4，实施计划见 ../plans/2026-10-23-local-plugin-foundation-m4.md。文件保留原draft路径以维持已有引用。
基线：docs/superpowers/specs/2026-10-23-local-plugin-foundation-design.md。
本文路径均相对仓库根目录；实现不得读取真实凭据或发送真实认证请求。

## 1. 目标与选择

Windows Host 0.4.0 在现有 API v1 中添加 bearer 凭据引用；普通 v1 网络插件无需修改。
Runtime 只能提交 credential ID；Host 在已授权 HTTPS 请求中注入 Authorization: Bearer <token>。
复用 PluginSecretStore、PluginNetworkAuthorityGate、scheduler 和网络 registry；不建立另一套凭据会话、epoch 或任务调度器。
不包含 raw Authorization、任意 header 模板、签名、OAuth、刷新 token、有道签名、UI 直连网络或明文 getter。
M4 可独立于 M2/M3：先用 submit/mainResult Runtime 和假服务完成纵向验收。

## 2. Manifest 与静态校验

```json
{
  "apiVersion": 1,
  "minimumHostVersion": "0.4.0",
  "permissions": ["network.https"],
  "network": {
    "httpsHosts": ["api.example.com"],
    "credentials": [
      {"id": "service-token", "settingKey": "api-token", "host": "api.example.com", "type": "bearer"}
    ]
  },
  "settings": [{"type": "secret", "key": "api-token", "label": "API token"}]
}
```

以上是相关字段片段，其他现有必填字段仍必需。
network.credentials 可省略；出现时必须是 1–8 项数组，不接受 null 或空数组。
每项只允许 id、settingKey、host、type 四个必填字段；未知字段、重复 JSON key 均拒绝。
id 和 settingKey 均匹配 ^[a-z][a-z0-9.-]{0,63}$，id 在本插件中唯一；id 大小写敏感且不自动修剪。
settingKey 必须精确引用当前 manifest 的 secret setting；普通 setting、缺失 key 均拒绝。
host 必须通过现有 valid_https_host，且是 network.httpsHosts 的精确成员；没有通配、端口、URL 或后缀匹配。
type 只能为 bearer。不同 ID 可以使用相同 settingKey 或 host；同一 secret 绑定两个 host 必须列两个声明。
credentials 出现时 minimumHostVersion >= 0.4.0；保留 network.https 当前 Windows 可用性判定。
Rust、schema、CLI 使用同一合同；canonical 化按 id 排序，仅改变数组顺序不产生新增授权。
授权比较单位是完整四元组 (id, settingKey, host, type)，不是 ID 单独比较。
schema 无法表达的跨字段引用、host 成员关系和版本门槛由 Rust/CLI 二次校验。

## 3. Runtime 请求 DTO 与错误

```ts
interface PluginNetworkCredential {
  readonly id: string
  readonly settingKey: string
  readonly host: string
  readonly type: 'bearer'
}
interface PluginNetworkRequest {
  readonly url: string
  readonly method: 'GET' | 'POST'
  readonly headers?: Readonly<Record<string, string>>
  readonly body?: PluginNetworkRequestBody
  readonly credential?: string
}
```

PluginNetworkCredential 用作文档/manifest 元数据类型，不提供获取 token 的方法。
Runtime 调用示例：api.network.request({url: 'https://api.example.com/v1/items', method: 'GET', credential: 'service-token'})。
credential 缺失保持旧行为；出现时必须为合法非空 ID 字符串，null/对象/空字符串/多参数均拒绝。
bootstrap snapshotNetworkRequest 复制 own data properties，拒绝 getter、symbol、prototype key、未知字段，规则与当前请求一致。
Rust serde 仍 deny_unknown_fields；credential 用 present-only 反序列化，避免 null 被 Option 默默当缺失。
凭据 ID 必须在当前已授权快照中存在；URL 解析后的规范 host 必须等于该凭据 host。
大小写不敏感地拒绝 credential 与任何用户 Authorization header 同时出现，包括相同值；不允许合并或覆盖。
无 credential 时现有普通 Authorization header 继续可用，不破坏旧插件。
secret存储保留通用格式；credential admission和authority重开核验token为 ASCII bearer b64token：^[A-Za-z0-9._~+/-]+=*$，至少一字节，最大 8192 字节。
不 trim、不 Base64 转换、不接受空白、CR/LF、Unicode 或重复 Bearer 前缀；最终 header 仍计入现有 16 KiB/32 fields 总限额。
现有 secret 存储 64 KiB 上限不变，旧配置可保存但不一定满足 bearer 消费格式。
错误仍通过 PluginNetworkCommandError {code} 和固定 JS Error.name 映射，禁止携带值、路径或 provider 错误正文：
- InvalidNetworkRequestError：credential 形状错误、Authorization 冲突、注入后 header 超限。
- PermissionDeniedError：未知/未授权凭据 ID、网络权限关闭或声明授权不匹配。
- NetworkTargetDeniedError：请求目标不符合凭据绑定或现有 HTTPS/DNS/IP 策略。
- 新增 CredentialUnavailableError：secret 不存在、解密/读取失败、token 格式不适用；故意不区分其内部原因。
- ExpiredRequestError：请求上下文已失效、轮换/删除取消、旧 activation/generation。
错误先按当前请求/authority判定：authority因坏记录关闭时返回PermissionDenied；已admit或缺失文件的credential消费失败返回CredentialUnavailable。其他网络错误和response DTO保留，完成后的api facade仍不可使用。

## 4. 持久化授权与安装事务

PluginStateDocument 和 EffectivePluginConfig 增加 networkCredentialsGrant: PluginNetworkCredential[]。
磁盘 schema 继续为 1；字段缺失读为空，空值写出时省略，非空值严格校验且按 id 排序。
旧 state 的 networkHttpsHostsGrant 绝不自动升级为 credential 授权；旧文件迁移只在后续正常持久化提交中发生。
新增字段不包含 plaintext/ciphertext；恢复仍使用现有 current/backup、prepare/persist/publish 事务。
network_grant_snapshot 必须同时验证声明 host 集合与 host grant 相等、声明 credential 集合与 credential grant 相等。
区分两种损坏，不声称任何损坏都能保留整份 state：
- 可反序列化且基础 state 合法，但 grant ID重复、数量/格式非法、host/key/type与当前 manifest语义失配：清除 network.https、host grant和credential grant，保留其他本地状态；不回退 backup、不取交集授权。
- JSON/基础 state或强类型 grant的结构损坏（如数组变字符串、缺必填字段、未知enum）：沿 load_owner 整份 quarantine/fallback，可能丢失 current 中 backup没有的普通设置；不可称“只关闭网络”。
任何由 backup恢复的 state（包括 current缺失），一律先清除 network.https及两份 network grant，再持久写回 current，才可发布 config/bundle；旧 backup 的授权绝不能直接恢复。
若恢复后的撤权 state无法可靠持久化，该插件本轮不激活并报固定 storage诊断；原数据保留供修复，不以旧 backup授权继续运行。
每次从 backup读取都重复此降权规则，故不依赖额外 durable标志；重启不能绕过降权。StoredState.raw必须对应已规范化落盘字节，不能保留含旧授权的backup raw。
current结构正常且 grant字段缺失是旧格式：以空credential grant加载；当已装manifest要求credentials时语义核对失败，撤权而非自动补齐。
语义降权同样须在恢复/激活前通过现有 state事务持久化；持久化失败只保留诊断和文件，不发布网络authority。
禁用网络清除 host grant 与 credential grant；重新授权同时写入当前完整声明。
卸载保留数据仍沿现有授权清理规则清空 grants，重新安装重新同意；secret 是否保留沿 retainData。
移除 credential 声明的升级只保留当前完整候选集合，之后重新加入已移除声明要再次同意。
宿主降级到旧版本不承诺读取含 credential grant 的新 state；普通无凭据插件仍写兼容旧形状。

PublicPluginPrepareNetwork 扩展为：
```ts
{
  httpsHosts: readonly string[]
  addedHttpsHosts: readonly string[]
  credentials: readonly PluginNetworkCredential[]
  addedCredentialIds: readonly string[]
  requiresNetworkConsent: boolean
}
```
credentials 始终输出数组（普通包为 []）；addedCredentialIds 包含相对上次已安装 manifest 新增或四元组变更的候选 ID。
新装时两个 added 列表均为完整候选集合；更新只比较当前安装版本，不能比较曾经删除的历史授权。
requiresNetworkConsent = 存在 network 且（新装，或 addedHttpsHosts 非空，或 addedCredentialIds 非空）。
未改变声明的升级保留先前撤销状态，不借“确认升级”恢复网络；新增权限的确认同时授权整份展示的候选 network。
prepare token 绑定候选包摘要、previous_config、旧 host/credential 声明；commit 复核全部，防止准备后替换或并发配置变化。
保留 commit {token, permissionGrants} wire：确认 token 即确认其展示的完整 network 声明；不能在 commit 接收客户端任意 credential tuples。
prepare/cancel/commit 失败继续遵守现有安装事务，拒绝升级不破坏旧版。

PublicPluginInventoryNetwork 增加 credentials: PluginNetworkCredential[]（始终为数组）。
set_public_plugin_network_access 增加 optional expectedInventoryRevision: U64Decimal。
对当前 manifest 含 credentials 的 granted:true 请求，该 revision 必填且必须等于当前全局 inventory revision；缺失/陈旧返回管理错误。
granted:false 和普通无 credentials 的旧调用仍接受旧参数。
宿主 UI 读取 inventory 展示完整绑定后才提交 revision；并发升级/编辑造成失败时重载，不自动再次授权。
state.prepare_set_network_access、prepare_activation 和所有授权清理路径必须同步处理两个 grant 集合。

## 5. 安装、详情与设置 UI

扩展 src/public-plugin-panel.tsx 的安装网络区域：逐项显示 ID、secret settingKey、目标 host 和“Bearer”；详情页从当前inventory.settings显示label与key。prepare摘要不新增仅为label服务的冗余字段，也不借用旧安装版本的label展示候选升级。
新/改绑定标“新增或变更”，固定说明“此凭据会发送给该服务；服务响应可能包含其回显。”
不展示凭据内容或掩码前后缀。空凭据是可安装状态，配置后才能调用；授权UI不读取secret，提交后的Host activation按6.2核验已存在记录。
详情页展示相同绑定与网络授权状态；增加网络开关，开启需先展示这些绑定并显式确认，关闭直接撤销。
0.4.0 UI 为所有 network-access 开启调用附带所展示 inventory.revision，更新 protocol/main/browser mocks。
secret 输入沿现有 password 写入/清除通路；不要从 D:/code/UiPilot_tools 搬运未提交设置页面。
工作区缺少可用 secret 编辑入口时，在 PublicPluginDetail 增加仅声明 secret 的 password 输入、保存/清除和 isConfigured 状态。
仅编辑的 key 进入 secrets map，空白输入不代表删除；清除显式发送 null，保存成功立即清空受控输入。
save_plugin_settings 现有多 key 保存不具全批原子性；UI 单 key 提交，错误后刷新状态，不宣称其他 key 回滚。
遵守 docs/ui-guidelines.md：双主题、紧凑布局、键盘焦点、busy/错误 aria 状态，不引入新 WebView 权限。

## 6. 权限边界与锁序

扩展现有 PluginNetworkAuthoritySnapshot 保存已授权完整凭据元数据；不保存 token，不增加 revision/凭据状态机。
Host 构造 current snapshot 时核对 manifest、durable grants、generation、activation_id、admission_epoch；重开时的具体规则见6.2，当前代码并没有自动secret核验。
正常网络路径锁序：authority gate -> scheduler（短暂检查后释放）-> registry attempt reservation（短暂后释放）-> secret bounded read/DPAPI -> registry registration。
不得从持有 scheduler/data_gate/state 锁的回调反向获取 authority gate；继续沿既有 lock order。
manager 管理路径：mutation -> authority gate -> scheduler（短暂）-> registry；持锁状态只做有界同步磁盘/DPAPI操作，不 await。
凭据读取/解密必须在 gate admission 临界区内，来源是 Host plugin identity + 当前声明 settingKey，不是客户端 pluginId/key。
先校验调用上下文和授权，保留broker现有每context attempt reservation在请求校验/secret I/O之前的顺序（其registry锁立即释放）；再校验目标和header冲突、读取/解密，最后注册prepared call。缺失/无效secret及请求校验失败同样消耗既有attempt预算；预算耗尽时不得读盘或DPAPI。attempt reservation不等于call注册，不在secret I/O期间持registry锁。
DPAPI读取采用上限读取并核验普通文件/目录与记录格式；不得由 manifest 或请求提供磁盘路径。
网络 DNS/TLS/send/read 全部在释放 gate 后执行，仍用既有取消 token、10 秒 deadline、同 host redirect 和返回前 deliver 复验。
注入值属于内部 prepared call 的 sensitive headers，不写回 PluginNetworkRequest、IPC response、state 或缓存。
每次 redirect 使用同一 admitted token 快照，仍必须通过 gate current 检查；不在 redirect 中读取更新后的 token。

save_secret 的最小取消策略采用“整个插件网络上下文失效”，不新增 per-key epoch：
1. 持 mutation，验证 plugin/声明 secret key；写入时先完成限额、DPAPI保护和候选密文准备，不持 authority。
2. 获取 authority transition；取得当前 authority 元数据，使 running.current=false、移除 waiting，再 set_plugin_authority(None) 取消该插件全部网络 calls；保留 running 槽直到完成或既有 deadline。
3. 保持 transition，提交密文原子 replace 或删除；此阶段阻挡新的 admission、redirect 检查和 deliver。
4. 成功后按6.2统一核验入口重新构造 authority，释放 transition/mutation；旧 requestId依然失效，只有后续新请求可用新值。
5. 若准备阶段失败，旧请求/旧文件不变；进入步骤2后提交失败，旧请求仍取消。
6. 提交失败或提交结果不确定时本次调用不重开authority，返回固定管理错误；此关闭不是永久锁存，未来能否恢复必须走6.2，不能把None当作失败原因标记。
7. 失败不等于成功撤销：落盘最终可能仍是旧值，也可能是新值；未来恢复若核验有效可以使用实际落盘值，但永远不能恢复旧request。此限制必须写入错误后的UI说明与SDK。

对已进入步骤2的 mutation，取消点是持 gate 完成 scheduler 失效和 registry 撤销的时刻；成功保存先于重新开放 admission。
复用 scheduler 现有 running/current/waiting/deadline，增加 retire_plugin_requests 操作：返回受影响 context/token 供 PublicPluginService 结算 pending submissions；不得直接 invalidate_plugin 抹掉仍在执行的 running 槽。
后续新命令进入现有 waiting；旧 handler 完成后接受其清理但 accepted=false，不发布业务结果；若不结束，沿既有 timeout/replacement 流程重建 Runtime。
修正 bootstrap 完成顺序：handler settle 后先使旧 facade current=false、释放其 busy，再 invoke complete_plugin_command；只报告一次完成，旧 finally 不得覆盖新 handler 的 busy。Host 仍只在完成处理后派发 next。
调用返回成功后，旧请求不得继续 redirect 或向 Runtime 投递；已完成投递的结果不追回。
取消不能撤回服务端已收到的字节；刚好与取消竞争的已 admitted transport 也可能已发送。合同不提供远端事务回滚。
保存错误不得包含 plaintext、secret path 或 DPAPI细节。失败恢复必须测试，不新增单独“credential recovery”状态机。
save_secret 不能在持 authority guard 时调用再次锁同一 gate 的 close_network_authority；失败处理走当前 transition 或先释放再进入既有关闭路径。

### 6.1 整个设置保存入口

现状 commands.rs::save_plugin_settings 逐项 save_secret 后无条件调用 save_settings；后者当前 invalidate_plugin 会删除running，故仅修save_secret不成立。
command改为调用 PublicPluginService 的一个保存协调入口；service统一收集manager产生的retired context/submission token，并在成功和部分失败出口都结算它们。
协调入口先验证整个DTO与当前manifest；仍不承诺跨secret/普通settings文件的批次原子性。每次内部manager mutation继续使用已有锁/事务，不嵌套重复获取mutation锁。
secret-only请求（settings={}）绝不进入普通 save_settings；两个map都空直接返回当前 PublicPluginMutation，无持久化、revision递增、失效或window teardown。
非空普通settings先校验类型，再比较当前有效值；所有值均未改变是no-op，同样不调用持久化和invalidate。
真正普通settings更新仍走prepare/persist/publish，但把同一Runtime内的invalidate_plugin替换为retire_plugin_requests；这适用于secret混合保存以及仅普通settings保存，不限于UI单key路径。
真实secret写/删除和真实普通settings更新的retired effects必须携带到service，即使后续key保存失败也不得因?提前退出而丢失；已提交改动不回滚。
不改变Runtime generation的保存不得移除running槽；换代/卸载等原有destroy/rebuild路径仍可使用invalidate_plugin。
窗口teardown仅在已产生真实改动/取消效果时按现有策略执行，partial failure同样处理；全no-op不关闭窗口。保存结果/错误仍保留现有外部DTO。
补command集成测试：secret-only、settings空、普通值相同、普通值改变、混合、第二key失败；断言running保留、tokens结算、无新命令被busy丢弃。
同一Runtime内的网络授权开关也遵守以上retire与service结算合同：set_public_plugin_network_access经service协调，manager不能继续invalidate_plugin删除running。真实授权变化在transition内retire并关闭旧calls，成功或部分失败均将effects交service结算；无变化请求保持no-op（开启凭据网络仍先校验expectedInventoryRevision）。旧handler未结束时后续命令排队，完成后accepted=false并正常派发next。网络开关仅变更网络权限，不扩大为关闭插件窗口；窗口处理仍沿既有网络开关行为。

### 6.2 全部网络authority发布与恢复

将 network_authority_snapshot 替换/包装为单个内部 checked_network_authority_snapshot，传入当前已确认配置、runtime身份与PluginSecretStore；此处是要实现的改动，不是现有能力。
所有生产Some(authority)发布必须经此入口，禁止直接使用旧bundle的快照重新授予；调用点清单（activation.rs）：
- load_with_timer_and_network_dependencies（启动加载，原约701行）；
- commit_with_readiness（安装/升级，原1352行）、set_enabled_with_readiness（启用，原1534行）；
- set_network_access（原1746行）与本设计secret/普通settings保存后的重新发布；
- finish_runtime_recovery（原2573行，覆盖卸载中止恢复）；
- finish_runtime_replacement（原3322行，覆盖timeout generation替换；当前仅从bundle发布，必须改）。
abort_uninstall_before_commit自身仅发布None；它随后进入finish_runtime_recovery。关闭/禁用/卸载/退出路径只能发布None，不调用核验来意外重开。
发布时在mutation -> authority顺序内复核bundle与已发布state身份/授权一致；finish_runtime_replacement须新增mutation保护。启动尚无并发时可省mutation，核验仍不可省。
必须区分 state持久提交不确定与secret写入不确定：前者沿现有bundle reservation/fail-closed限制，不能拿旧bundle跳过durable状态校验；新增helper不绕过该限制。
对每个声明绑定的settingKey去重后做bounded普通文件/目录检查、record校验与DPAPI解密，临时明文及时清零；不缓存解密结果。
缺失文件是“未配置”的有效事实：允许发布网络authority，带credential请求仍返回CredentialUnavailable；使首次安装和成功清除不阻断普通无认证请求。
已存在但读取失败、损坏、不可解密或非bearer格式的记录令此次authority为None，保留本地Runtime可用；不得自动退用旧secret或其他key。
每次credential admission仍重新读取/核验，重开检查不是长期授权token或明文缓存。成功保存之后也必须重新核验实际最终文件。
保存失败后未来启动/启用/授权/恢复可使用核验通过的实际旧值或新值；没有永久失败锁存承诺，也不新增第三种authority状态。用户要撤销必须收到清除或关闭网络成功。
父级已选择上述非锁存最小方案：失败本次fail closed，后续既有重建或显式管理操作经统一核验可恢复；不增加sticky持久阻断。独立规格评审已完成；实现阶段仍需逐任务及全批代码评审。

## 7. 内存、错误和服务端回显

CryptUnprotectData 由 test-only 提升为 Host-only helper，公开 API 仍只有 settings.isSecretConfigured。
DPAPI明文缓冲在复制到敏感容器后清零再 LocalFree，所有异常路径同样处理；Rust token 容器 Drop 清零，不派生裸 Debug。
NativeHttpsRequest/内部 prepared request 改为脱敏 Debug；普通 header 中的 Authorization 也不输出值。
创建 hyper HeaderValue 后 set_sensitive(true)；敏感字节仅在 bounded 请求生命周期内，不留重试/诊断缓存。
HTTP/TLS库内部复制不能保证彻底清零，不在产品文档声称内存永远不存在明文。
log_terminal 继续仅记录 plugin/method/host/result/status/time；错误仍是固定 enum，不连接 provider 异常文字。
服务端会收到 token，可以在 response header/body 原样或编码回显；普通 response 返回合同保持不变。
不靠字符串替换声称防止回显泄漏，不把合法 provider response 静默改写；授权 UI/SDK 明示这是目的服务信任边界。
测试确认 Host 自身不会自动回填请求 header，并用 fake echo server 验证回显的明确边界。

## 8. 实现路径与版本同步

- Rust合同/持久化：src-tauri/src/public_plugins/{manifest,state,activation,activation_bundle,secrets}.rs。
- 执行：src-tauri/src/public_plugins/{runtime,scheduler}.rs、src-tauri/src/public_plugins.rs、network/{authority_gate,broker,registry,transport}.rs；registry 优先复用现有撤销，无需新状态字段。
- 宿主接线：src-tauri/src/commands.rs、src/protocol.ts、src/main.ts、src/public-plugin-panel.tsx、src/launcher-browser-preview.tsx；相关 UI tests。
- SDK：docs/plugin-sdk/{public-plugin-v1.md,public-plugin-developer-guide.md,uipilot-plugin-api-v1.d.ts,uipilot-plugin-v1.schema.json}。
- CLI：packages/plugin-cli/src/{manifest,contracts}.ts；按现有生成器重建 src/generated/manifest-validator.mjs/.d.mts，禁止手改生成 validator。
- 新的隔离参考包：examples/public-plugins/com.uipilot.demo-credentials/；仅假 token fixture，README 写人工 Host 步骤，测试无公网依赖。
- 复用 state_tests.rs、secrets_tests.rs、各网络模块 tests、activation.rs 生命周期 fixtures；无需为 credentials 增加 Tauri command/capability。
- 版本源全部同步至 0.4.0：根 package.json、package-lock.json 根/root-package 两处，src-tauri/Cargo.toml、Cargo.lock 的 uipilot package 条目、src-tauri/tauri.conf.json。
- Host目标：src-tauri/src/public_plugins.rs::PublicPluginHost::current；CLI目标 packages/plugin-cli/src/contracts.ts::PLUGIN_CLI_HOST_VERSION。
- network/broker.rs::USER_AGENT 更新 UiPilot-Plugin/0.4.0，或由 package version 同源生成；对应断言同步。
- 根 CLI package 的独立版本 0.1.0 不属于 Host版本；Cargo.lock 其他依赖的 0.3.4 不改；历史功能最低门槛与旧示例 minimumHostVersion 不普遍抬升。
- 更新当前 Host目标测试而保留 0.3.2/0.3.4 历史边界 tests；新增 0.3.4 拒绝/0.4.0 接受 credential manifest 测试。

## 9. 验收矩阵与交付切片

A 合同/授权：字段和限额正反例；Rust/schema/CLI一致；旧 state 无隐式授权；新装、同声明升级、绑定改 key/host/type/id、移除再加、撤销后升级、prepare后变更、重启。
B 消费纵向：临时 DPAPI fake token -> bounded broker -> fake TLS/transport ->正确 bearer header；Runtime/Panel均无法读明文；其他插件同 key 拒绝；缺失/坏密文/不适用 token 固定错误。
C 并发：屏障精确卡在 admission、DPAPI后注册前、transport开始、redirect前、response完成deliver前，分别执行 secret替换/删除、网络撤销、禁用、更新、卸载、超时/重建和关闭。
C2 调度：旧 handler 被取消但尚未结束时新命令只排队；等待 token 被结算；旧 complete 不接受结果；取消后不合作 handler 到期重建；next dispatch 不能被 bootstrap busy 静默丢弃。
D 失败恢复：DPAPI保护失败不取消；提交错误本次保持closed；有效旧值/新值、缺失、坏record分别经过6.2每个发布入口；保存失败不暗示回滚或永久撤销；仅新request可恢复。current坏shape/截断/缺失+旧授权backup必须先撤权落盘，写失败不可激活；语义坏grant不能fallback复活授权。
E 泄漏：Debug/错误/log 无假 token；HeaderValue sensitive；注入字段不出现在 IPC/state；服务端 echo按合同返回；不为回显误写强隔离断言。
F UI：安装新增绑定提示、取消保留旧版、陈旧revision不授权、secret单key保存/清除、busy防双击、键盘和双主题；按钮不触发真实请求。
先交 A+B，再 C+D+E 完成生产边界，最后 F 和 SDK/CLI/demo 全链验收；各切片不得把尚未完成的功能标为可生产。
自动验证用现有 verify:local 加聚焦单测/临时TLS；真实 Windows Host 安装与交互独立记录，未执行不能用 mock通过代替。
