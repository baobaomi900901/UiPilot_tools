# 本地插件基座 M4 凭据消费实施计划

目标：完成 Windows Host 对本地插件 bearer 凭据引用的授权、消费、更新取消与设置闭环。技术合同以 [凭据消费设计](../specs/2026-10-23-plugin-credential-consumption-draft.md) 为准。复用现有 DPAPI secret store、网络 authority gate、scheduler、registry 和安装事务；不增加凭据会话或第二套调度器。

技术：现有 Rust/Tauri 2、Windows DPAPI、Hyper/Tokio、React 19/TypeScript、Vitest 与 Node CLI；不增加依赖。独立规格评审已通过，开始依序实现。

## 全局约束与执行

- 仅 bearer、精确 HTTPS host、Windows；无明文 getter、OAuth、签名、在线发布或安装渠道。Runtime 仅提交 credential ID。
- 锁序为 mutation -> authority gate -> scheduler（短暂）-> registry；网络 admission 在 authority gate 内验证上下文、目标、读取 secret 并注册，DNS/TLS/传输不持 gate。
- 保存取消保留仍在执行的 running 槽，旧 request 永不复活。失败本次关闭网络，后续统一核验实际磁盘后可恢复；无永久关闭锁存承诺。
- backup 恢复及语义 grant 损坏必须先撤销网络并可靠持久化，禁止恢复旧授权。所有生产 Some(authority) 发布走统一核验入口。
- 服务端会收到凭据且可能回显；UI 与 SDK 明示此边界。错误、Debug、日志不输出 secret。
- 在既有隔离 worktree 工作，不修改原工作区。只使用合成 token 与本地 fake transport；不调用真实服务、不读取真实凭据、不安装真实插件。
- Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6。每项聚焦 TDD，记录红/绿证据，至少一个原子提交，经规格和质量评审后再进入依赖任务。文件列表允许必要的编译接线与 fixture 兼容修改，不借机重构无关代码。
- Host 与 CLI 当前目标保持 0.3.4，直到 Task 6 完成闭环后统一升至 0.4.0；中间提交的新合同通过显式 0.4.0 测试 host 验证，生产不提前开放半成品凭据插件。
- Rust 聚焦命令统一为 `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline <filter>`；复用现有 target 缓存仅作为本地进程环境，不写入仓库。前端用 `npm test -- <file>`；CLI 用 workspace 脚本。不新增依赖、不重跑无关全套测试。

### Task 1: Manifest 与工具链凭据合同

**Files:** `src-tauri/src/public_plugins/manifest.rs`、`tests.rs`；`docs/plugin-sdk/uipilot-plugin-v1.schema.json`、`uipilot-plugin-api-v1.d.ts`；`packages/plugin-cli/src/manifest.ts`、`contracts.ts`、生成的 schema/validator；相关 CLI manifest/schema tests 与 SDK 类型 fixture。

**Dependencies:** 正式设计评审通过；设计 §2、§3 的类型定义、§8 版本边界。

- [x] 添加严格凭据声明、canonical 元数据类型及跨字段校验；Rust、JSON schema、CLI 对齐。
- [x] 用现有生成脚本同步 CLI schema/validator；只增加类型声明，不开放 Runtime 请求消费。
- [x] 保持旧插件兼容；0.3.4 拒绝新合同，显式 0.4.0 host 接受。

**Distinct test coverage:** 缺失允许，null/空数组/超8项/未知和重复JSON key拒绝；ID语法和唯一性、secret引用、host成员、bearer类型、minimumHostVersion；相同集合重排 canonical 相同。SDK credential 元数据和请求字段类型通过正反例。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins::tests`；`npm test --workspace=@uipilot/plugin-cli -- manifest.test.ts`；`npm run check --workspace=@uipilot/plugin-cli`；`node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict docs/plugin-sdk/tests/network-api-contract.ts`。

### Task 2: 安装授权与持久化恢复

**Files:** `src-tauri/src/public_plugins/state.rs`、`state_tests.rs`、`activation.rs`、`activation_bundle.rs`、`public_plugins.rs`；`src-tauri/src/commands.rs`；`src/protocol.ts` 及 DTO fixtures。

**Dependencies:** Task 1；设计 §4（完整合同）、§6.2 中 durable state 边界。

- [x] 持久化完整 credential grant，所有授权、升级、禁用、卸载保留数据路径同步处理；不从旧 host grant 推导授权。
- [x] prepare/inventory 返回凭据列表与变更 ID；token绑定旧配置和旧声明；开启凭据网络要求所展示的 inventory revision。
- [x] backup 恢复一律撤权后持久化；可解析的语义坏 grant保留普通状态并撤权，不回退旧授权；强类型结构损坏沿原 quarantine/fallback。
- [x] 为后续 authority 注入传递完整已授权元数据；当前运行目标仍拒绝生产凭据 manifest。

**Distinct test coverage:** 新装同意/取消、四元组变更/重排/删除再添加、撤权后同声明升级不复活、prepare后配置变化拒绝、陈旧/缺失revision开启拒绝而关闭兼容；current缺失/截断/坏结构+旧backup先撤权落盘，写失败不激活；语义损坏不回退backup，其他设置保留；序列化空grant省略、raw与落盘一致、重启不能恢复旧grant。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins::state`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins::activation`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline commands::tests::plugin_network_management`；`npm run build`。

### Task 3: Host 内部凭据读取与网络注入

**Files:** `src-tauri/src/public_plugins/secrets.rs`、`secrets_tests.rs`、`network/{authority_gate,broker,transport,mod}.rs`、`runtime.rs`、`public_plugins.rs`；`src/plugin-network-bootstrap.test.ts`。

**Dependencies:** Tasks 1–2；设计 §3、§6 正常请求路径、§7。

- [x] 实现 bounded普通文件/目录记录读取与 Host-only DPAPI 解密；敏感容器和DPAPI输出清零、Debug脱敏。
- [x] Runtime安全快照和Rust严格请求DTO支持 credential；保留每context attempt reservation先于请求校验/secret I/O且立即释放registry锁；验证当前授权/目标/header冲突后在gate内读取并注入内部prepared headers，随后登记call再释放gate。
- [x] 复用当前deadline、取消、redirect与deliver；redirect保持同一token快照。固定CredentialUnavailable错误与JS映射，敏感HeaderValue标记。
- [x] 保留无credential的现有Authorization行为；最终header总预算含注入值。

**Distinct test coverage:** fake token DPAPI roundtrip到fake TLS/transport正确header；不同插件同key隔离，未知ID、未授权、host不匹配在读secret前拒绝；缺失/坏record/DPAPI失败/非法bearer固定错误；失败消费attempt预算、预算耗尽无secret I/O；8192边界和最终header数/大小；DTO null/getter/symbol/未知键拒绝；Debug/log/错误无token；fake服务回显按正常响应返回，Host自身不回填header。屏障卡在gate读取后、注册前并并发撤权：先完成admission再取消，旧响应不能deliver，无死锁。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins::secrets`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins::network`；`npm test -- src/plugin-network-bootstrap.test.ts`。

### Task 4: 设置保存取消、调度结算与全部恢复入口

**Files:** `src-tauri/src/public_plugins/{activation,activation_bundle,scheduler,secrets,runtime}.rs`、`src-tauri/src/public_plugins.rs`、`src-tauri/src/commands.rs`；相关 Rust 与 Runtime bootstrap tests。

**Dependencies:** Task 3；设计 §6、§6.1、§6.2 的全部时序；§9 C/C2/D。

- [x] secret准备与提交拆分；准备失败不取消，进入transition后取消旧请求并保持running槽直到settle/deadline，成功提交后统一核验再开authority。
- [x] service统一协调settings保存与同一Runtime网络开关；网络开关保留原窗口行为、真实变化retire并结算、no-op不取消。settings DTO验证、普通settings no-op、secret多key和部分失败，收集并结算全部retired contexts/submission tokens；真实变更/取消才teardown窗口。
- [x] 修正bootstrap busy释放和complete顺序，旧finally不覆盖下一handler；不合作旧handler沿既有timeout重建。
- [x] 所有Some(authority)生产发布走checked helper；核对当前durable bundle身份/grants与实际secret记录，timeout replacement增加mutation保护；confirmed bundle查询在book锁内重查terminal并拒绝任意未清除reservation，成功state/bundle发布后才checked重开，恢复不得绕过state reservation。

**Distinct test coverage:** secret-only、全空、普通值相同、普通值变化、混合、第二key失败；旧running保留、新命令只排队，waiting token结算，旧complete accepted=false后新handler正常派发；不合作handler超时重建。网络开关在旧handler执行中改变授权后旧请求失效而running保留、waiting结算、后续命令正常派发；无变化开关不取消。准备失败旧请求继续；提交失败旧请求取消且本次关闭，实际旧/新有效record、缺失、坏record分别经启动/安装启用/网络授权/卸载恢复/timeout恢复入口核验，只有新请求恢复。屏障停在Unknown关闭gate后、reservation Drop前，并发尝试重开仍拒绝；RESERVED/COMMITTING/DURABLE各阶段都不可使用旧bundle。屏障卡于transport开始、redirect前、response deliver前，分别替换/清除secret及既有禁用/升级/卸载/关闭，旧请求不再redirect/deliver；不声称撤回已发送字节。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline public_plugins`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline commands::tests::save_plugin`；`npm test -- src/plugin-network-bootstrap.test.ts`（若新增独立调度fixture，也运行该文件）。

### Task 5: 凭据授权与设置界面

**Files:** `src/public-plugin-panel.tsx`、`launcher-view.tsx`、现有styles、`protocol.ts`、`main.ts`、`launcher-browser-preview.tsx`、相关launcher/plugin UI tests。

**Dependencies:** Task 4；设计 §5，`docs/ui-guidelines.md`；复用已有secret设置通路，缺失时在详情页补齐。

- [x] 安装列出ID、候选settingKey、host、Bearer；详情补充当前secret label/key，标新/变更绑定与服务端回显说明。
- [x] 开启网络先明确确认完整绑定，所有开启调用带展示revision；陈旧失败重载不自动重授权，关闭直接执行。
- [x] 声明secret的password单key保存/显式清除，状态与busy/错误可访问，成功清空输入；错误刷新状态并解释部分提交边界。
- [x] Light/Dark、紧凑布局与键盘操作验证；不新增Tauri capability，不复制原目录未提交UI。

**Distinct test coverage:** 安装取消保持旧版、变更标记、无值/掩码泄漏、revision随确认传递、陈旧拒绝不重试；保存仅编辑key、空白不删除、clear=null、busy防双击、成功清输入、错误刷新；键盘焦点/aria与双主题渲染。

**Verify:** `npm test -- src/launcher.test.tsx`（或新增聚焦组件test）；`npm run build`；使用本地preview验证Light/Dark，真实Host人工交互另外标注。

### Task 6: SDK 示例、版本切换与全链验收

**Files:** `docs/plugin-sdk/{public-plugin-v1,public-plugin-developer-guide}.md`、SDK类型及合同fixtures；新增 `examples/public-plugins/com.uipilot.demo-credentials/` 与本地测试；根README；设计 §8 列出的全部Host版本源与对应当前版本断言。

**Dependencies:** Tasks 1–5；设计 §7–§9；示例遵守public-plugin-developer-guide及ui-guidelines，参考demo-http。

- [ ] 完整SDK消费/错误/授权/取消/失败非回滚/回显合同；合成凭据示例和离线fakeprovider测试、人工Host步骤。
- [ ] 完成闭环后同步Host及CLI目标0.4.0；CLI包版本维持0.1.0，历史minimumHostVersion和其他依赖版本不变。
- [ ] 全仓门禁及独立全批评审，修正发现；更新路线图/进度与未执行的人工检查，不把mock通过称为真实Host验收。

**Distinct test coverage:** 0.3.4拒绝/0.4.0接受新示例；isolated CLI pack/validate新示例，Host staging兼容；demo无真实token或公网依赖；SDK声明与Runtime实际字段/错误一致，版本源一致。

**Verify:** `npm run verify:local`；独立全批规格/代码评审，覆盖M4增量及M0/M1接口集成。实际Windows Host安装、键盘/焦点、双主题和真实服务配置未执行则明确保留人工验收状态。
