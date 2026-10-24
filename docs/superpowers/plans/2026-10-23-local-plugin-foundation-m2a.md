# M2-A 多 mainResult 命令实施计划

状态：八项实施任务已完成；最终源码 `b84bda3` 已通过默认配置的全仓验证及 Task 8 / 整批 M2-A 独立规格与质量评审（PASS / Approved，无新增发现）。此前各任务的修复与验证证据保存在进度账本和验收索引中。0.5.0 可运行验收包已构建；真实 Windows Host 人工验收待用户执行，不能据此把完整 M2 或整个本地基座标为完成。

> **For agentic workers:** 使用 `subagent-driven-development`；单写者顺序实施，每任务通过独立 Spec 与 Quality 双门禁后才可开始下一任务。

**Goal:** 一个包以稳定命令身份交付两条可分别搜索、执行、改名和收藏的 mainResult 命令，共用现有 Runtime 与包级能力，旧包无需改动。

**Architecture:** 延伸现有 manifest 规范化、prepared state commit、activation bundle、scheduler、opaque registry 及 M4 service-effects/checked authority 机制。命令配置变为集合，执行身份贯穿路由与异步拥有者；不复制 manager、Runtime、权限或网络状态机。前后端协议同批更新，旧包兼容与旧 IPC 参数兼容分别处理。

**Tech stack:** 当前锁定的 Rust/Tauri 2、serde/schemars、TypeScript/React 19/Vitest、Node ESM 与独立 plugin-cli；不新增依赖或联网构建步骤。

## Governing references and readiness gate

- 唯一技术合同：[M2-A 设计](../specs/2026-10-23-plugin-multi-command-draft.md)。设计与本八任务计划已通过独立评审；以下任务引用其精确节号，不重新定义 DTO、事件顺序或安全边界。
- 阶段边界：[路线图](../specs/2026-10-23-local-plugin-foundation-design.md)。M2-A 不等于完整 M2，也不单独提供 M3 的业务动作合同；统一 actions/onAction、每命令快捷键、独立别名集合、多命令 panel/window 均不在本计划。2026-10-24 方向审核后，M3 依赖稳定身份与另行评审的有限业务动作合同，不再以 M2-C 原生快捷键完成为硬前置。
- [计划指南](../../agents/implementation-plan-guidelines.md) 优先于泛用模板；[UI 规范](../../ui-guidelines.md) 用于所有宿主表面；SDK/示例任务先读 [开发指南](../../plugin-sdk/public-plugin-developer-guide.md)。本任务是明确的宿主开发，允许改 host/CLI/SDK。
- **执行前置门禁已满足：** M4 Task 6 与整批最终评审通过，最终两文件 UI/文档差异已核对且不改变本计划假设；M2-A 设计与计划经独立复审后由 controller 正式纳入。真实 Windows Host 人工验收仍 pending，不作为已执行证据。Task 1 起按以下顺序执行。

## Shared contract and global constraints

共享 wire/state/report 合同只在此索引一次：设计 §3–4 为 manifest/规范化身份，§5 为原子注册/搜索/管理，§6–7 为运行归属/请求失效/opaque copy，§8 为收藏与缺失字段专属 IPC 兼容，§9–10 为精确 state schema 2/迁移/恢复，§11 规范性继承正式 M4 设计 §6–6.2，§12 为升级/删除/包回退与二进制降级，§13 为 CLI 增量报告，§14 为后续能力边界，§16 为验收矩阵。任务只引用这些定义，不建立第二份类型规格。

- 工作区仅 `C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools`；原 `D:/code/UiPilot_tools` 的源码、未提交改动及真实凭据不读取、不复制、不覆盖。此 worktree 无 `.codegraph/`，不运行索引。
- Manifest/API 仍为 1，state 为 2，inventory 仍为 1，CLI report 仍为 1。`MULTI_COMMAND_MIN_HOST = 0.5.0`；commands 为 1..16。ID 包含合法的 `constructor`，不能加保留名或以对象原型/数组位置查找身份。
- Host current、根/Cargo/Tauri 版本和 CLI Host target 在 Tasks 1–7 保持 0.4.0；新能力只在显式 0.5.0 测试 Host 或验证器已有 hostVersion 注入点下验证。Task 8 才统一改 0.5.0。旧凭据门槛保留 0.4.0，CLI 自身 0.1.0 不随 Host target 自动升级。
- 中间提交必须编译，旧包回归必须通过。生产 0.4.0 必须继续拒绝所有 commands 包；不得为方便测试放宽门槛、合成首项单命令、增设任意 fallback、保留双写旧根状态，或把不完整路径伪装为兼容支持。
- manifest/config/DTO 字段变动必须同任务更新全部构造点、调用者和 fixture。文件清单包含必要类型传播；可增补实际直接调用者，但不能借机改功能合同。旧 panel/window 只能显式使用 legacy/default 路径并验证模式；完全不含 default 的新包不能被误当 legacy。
- 沿用 M4 锁序、reservation 和 checked publication；网络只用假 transport/假 secret，禁止真实服务和真实凭据。shutdown 是 admission/delivery 终态。

## Global execution and review rules

- 依赖顺序固定 `Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6 -> Task 7 -> Task 8`；只运行一个源码写者。前任务独立 Spec PASS 与 Quality Approved 后才派发后任务。每任务的 reviewer 必须独立于实现者；Critical/Important 修复后重评审，Minor 留在 ledger 供最终评审处置，不能静默丢弃。
- 每任务按 TDD 增加聚焦测试，确认有意义的预期失败，完成最小实现并跑聚焦验证；不为已有正确行为制造失败。至少一个原子提交，不吸收其他任务或用户改动。controller 记录 base/head、brief、报告、完整 diff 包和双 verdict；用已记录 base 而非 HEAD~1 生成 review range。
- 任务中的命令以 worktree 根执行；Rust 一律 `cargo ... --manifest-path src-tauri/Cargo.toml ... --offline`；Node/npm 沿现有本地依赖，不安装/升级。每任务除所列 focused Verify 外，若改 Rust 共享类型则跑 `cargo check --manifest-path src-tauri/Cargo.toml --lib --offline`；若改 TS 共享类型则跑 `npm run build`，改 CLI 共享类型则跑 `npm run check --workspace @uipilot/plugin-cli`。这是编译边界，不代替任务行为测试。
- 首次 dispatch 必须附上该任务引用的设计节、全局约束与前任务实际接口；不给实现者整段历史。计划不预先发明私有 helper 签名；已存在符号可重构，但公开 wire 字段仅依设计。新增私有结构命名由实现者依现有模块风格决定，并在报告交接。
- 后续任务不得假设前任务已经提供它尚未拥有的行为。准备任务只提供可验证的模型/校验和安全类型传播；最终 0.5.0 开放前由全链路和完整验收保证所有行为闭环。
- 自动检查、浏览器预览与真实 Windows Host 人工验收分别记录。隔离临时目录中的静态 pack/staging 可自动执行；实际 Host UI 安装、系统焦点/剪贴板交互与安装器操作不由测试授权推断，需 controller 核对既有授权，必要时取得明确批准。没有自动远程 push、发布、真实凭据请求或安装到用户数据目录。

### Task 1: Manifest wire、规范化集合与 Rust/CLI/schema 一致性

**Files:** `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/tests.rs`, `src-tauri/src/public_plugins/package.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins.rs` 中直接 manifest 消费点；`packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/src/validate.ts`, `packages/plugin-cli/src/contracts.ts`, `packages/plugin-cli/src/source-validation.ts` 的直接消费者；`docs/plugin-sdk/uipilot-plugin-v1.schema.json`, `packages/plugin-cli/schema/uipilot-plugin-v1.schema.json`, `packages/plugin-cli/src/generated/manifest-validator.mjs` 及 `.d.mts`；对应 `packages/plugin-cli/tests/manifest.test.ts`, `validate.test.ts`, `source-validation.test.ts`；创建共享 `docs/plugin-sdk/tests/multi-command-manifest-fixtures.json`。

**Dependencies:** readiness gate；设计 §3–4、§13 的声明元数据规则及 §16.1–2。此任务不改变 Host current/CLI target。

- [x] 实现互斥 wire 形态、逐项严格解析和完整规范化命令视图，保留 legacy/new wire 来源；仅校验通过后暴露集合。保留现有 legacy 结构和模式语义，禁止给旧 command 增 id。新形态没有可被旧消费者误用的合成标量 command。
- [x] 同步更新所有直接字段读取和构造 fixture；现有 legacy 路径显式取 default/验证 legacy 模式，集合语义路径按真实条目遍历。当前生产版本拒绝新形态，因此不得在此任务为了让单命令 state/route 工作而临时投影任一新命令；尚待 Task 2–3 的集合调用者保留明确的模式边界。
- [x] Rust/CLI 应用相同能力常量及完整语义校验。CLI 公共 manifest 类型变动时同步 `validate.ts` 的聚合 metadata 构造，commands 报告固定 mainResult 并按声明顺序保留 commands，不从首项读取；legacy metadata 的 key 集合不变。详细 issue 定位/人类渲染由 Task 6 完成。
- [x] 用 Rust generator、CLI schema sync 与 standalone validator generator 再生制品，不手改生成代码；共享 fixtures 由 Rust 和独立 CLI 读取，明确 schema 与语义校验分别负责的拒绝分类。

**Distinct test coverage:** 两 wire 互斥/缺失/null、1/16 接受与 0/17 拒绝；所有非法/重复 ID、重复 defaultName、逐项 summary/input 规则、禁用模式/字段/权限；constructor 有效；0.4.0 minimum 拒绝、显式 0.5.0 Host 接受、未来 minimum 不兼容；legacy panel/window 与凭据门槛不变。新形态 schema-valid 的失败可有声明 metadata，schema-invalid 无 metadata；单项数组仍输出 commands，重排仅改变报告顺序。生产 current 0.4.0 不接纳任何 commands 包。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests --offline`; `npm test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts tests/validate.test.ts tests/source-validation.test.ts`; `cargo run --manifest-path src-tauri/Cargo.toml --bin generate_public_plugin_schema --offline -- --check`; `node packages/plugin-cli/scripts/sync-plugin-schema.mjs --check`; `node packages/plugin-cli/scripts/generate-schema-validator.mjs --check`。

### Task 2: 严格 state schema 2、首读迁移与 durable 恢复

**Files:** `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`, `src-tauri/src/public_plugins/activation_bundle.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins.rs`；`src-tauri/src/commands.rs`, `src-tauri/src/plugin_panel.rs`, `src-tauri/src/plugin_window.rs` 中 EffectivePluginConfig/状态调用者；必要直接 fixture；可按职责创建 `src-tauri/src/public_plugins/state_codec.rs` 并在 state 模块内接入，不能另建持久化所有者。

**Dependencies:** Task 1；设计 §9–12、§16.8–13。读取最终 M4 的 state prepared/durable/revalidation 和 recovery 实际接口后修改，不重建事务系统。

- [x] 引入显式严格 schema-1/schema-2 DTO 与确定性命令 map，读取按 schema 分流；检测重复 JSON 命令 key，必填 nullable nameOverride 不可被 Option 缺省吞掉。存储 raw 始终保留实际权威字节，规范化 view 不触发写盘。
- [x] 同任务更新 EffectivePluginConfig 和所有直接构造/消费者，命令配置只由经验证 ID 查询；网络 grant/settings/revision/generation 仍包级。现有 legacy 外层入口只用已验证 default，不能选择集合首项。Task 3 才切换主结果公共协议，不在此任务新造公开 SDK config 类型。
- [x] 把 activation/升级 reconciliation、rename/favorite prepared state、冲突检查及 retainData 状态操作改为命令集合，所有写端写 schema 2。准确比较存储 override，favorite/override 真 no-op 不生成迁移事务。完整集合验证后才 prepared publish。
- [x] 保持 backup/语义修复 durable-first 以及 M4 grant 修复分类；Unknown、NotCommitted 和 failed revalidation 分别处理。将状态发布及任何 authority 重开留在既有统一 checked 通道，不能用内存规范化成功作 durable 证据。

**Distinct test coverage:** 精确 schema-2 roundtrip/根默认值/空 credentials 省略，inventory schema 不变；缺 commands、0/17、非法 key/name、重复 JSON key、未知根/条目、缺/null favorite、缺 nameOverride、旧根三字段、未知 schema 全部拒绝或按既有分类恢复。有效 schema-1 primary 含/缺 favorite 首读后 inventory/route 可读，raw/revision 无变化、没有迁移专用写；与正常 startup 合法 activation 写入分开观测。第一次真实 favorite/rename commit 的 NotCommitted+旧字节复核成功不发布，重试只写一次 schema 2；Unknown/复核失败 fail-closed，不断言旧可用性。语义坏 grant、结构坏 primary、current 缺失及 backup 恢复分别测试撤权 durable 后发布，修复写失败无 authority。retainData 保留命令条目/覆盖、清空全部 favorite，按 M4 清授权。仅存活同 ID 继承；删除→重启→重加 fresh；legacy→无 default→回退旧包产生 fresh default，绝不按名/位置恢复。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::state::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests --offline`。

### Task 3: 稳定 ID 的完整路由、队列、Runtime 与 opaque 执行链

**Files:** `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/model.rs`, `src-tauri/src/result_registry.rs`, 必要 `src-tauri/src/plugin_panel.rs`/`src-tauri/src/plugin_window.rs` 类型传播；`src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/public-plugin-panel.tsx`, `src/launcher-browser-preview.tsx`, `src/main.ts`；`src/protocol.test.ts`, `src/launcher.test.tsx`, `src/plugin-window-core.test.ts`, `src/plugin-window-view.test.tsx` 及相关 Rust tests/fixtures。

**Dependencies:** Task 2；设计 §4–7、§8 的 activation/context 严格形状、§16.3–5/16。管理旧 IPC 缺失兼容和完整名称编辑体验由 Task 5 收口；此任务的新 activation/context 从第一天严格要求 commandId。

- [x] 以完整有效名称表原子注册并按命令 ID 定位；每条命令有独立 suggestion、summary 与包名/启动名显示，排序末尾 ID 稳定破同值。完成新 inventory commands 列表与所有前后端 parser/mock/preview/组件的同批类型切换，包级开关/设置不复制。
- [x] commandId 从 route、request candidate、pending submission、dispatch 到绑定 completion 全程保存；waiting/replacement/recovery 使用保存的 ID/input。invocation 仅新 wire 带 ID，legacy 严格省略 key。仍一个 Runtime、pluginId 队列和现有超时，不把 ID 当 generation/admission。
- [x] 主结果 activation 与 command context、snapshot 及其所有构造/消费者同任务更新；前端严格保留 validated ID，错误兄弟 ID 不匹配。收藏 DTO 新字段的构造/消费者必要类型传播同批完成，但 Task 5 拥有完整收藏菜单/管理入口验收。
- [x] 私有 CopyText action 加命令归属，执行时验证命令属于当前 activation 配置，继续以 opaque handle 和现有授权/窗口/结果域执行；所有复制动作构造与 match 分支一起改，不能留下默认 ID 或前端文本执行入口。
- [x] 旧 panel/window 仅显式 legacy/default 路径，原包 manifest 不改。新包完全不含 default、合法 constructor ID 均可经同一完整链运行。

**Distinct test coverage:** 显式 0.5.0 Host 上 live A 与 submit B 一个 Runtime；A running→B waiting→query 改变→迟到 A completion，仅绑定 route 决定归属，B 保留原 ID/input；waiting 被替换和 timeout/rebuild 的两个事件序列分别验证。A/B 返回相同插件 result.id 不串结果；重排、改名后别人占旧名不接管旧句柄；错误 request/result/window/result-set、前端自报文本/commandId 均不能重解释动作。新 activation/context 缺失/null/空/非法/未知 ID 被拒；constructor 从 manifest 到 route/store/invocation/context/opaque action 正确。旧 invocation 精确键集仍相同；demo-return/demo-panel/demo-win/pomodoro 的旧路径可用。完整集合原子性另测：已安装旧双命令集合及其 override/favorite；安装或升级候选含一个合法变更及另一条与同包、其他插件、内置或 quicklinks 冲突的命令，或完整候选 prepared 后 commit 为 NotCommitted 且旧字节复核成功；整个候选拒绝，旧包/全部路由/状态字节与命令覆盖/收藏均保持，无半注册。成功升级一次性发布完整集合；Unknown/复核失败沿 Tasks 2/4 fail-closed 合同，不断言旧可用性。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::scheduler::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::runtime::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib result_registry::tests --offline`; `npm test -- src/protocol.test.ts src/launcher.test.tsx src/plugin-window-core.test.ts src/plugin-window-view.test.tsx`。

### Task 4: 改名的 running 槽、精确 effects 与 M4 authority 闭环

**Files:** `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/scheduler.rs`, `src-tauri/src/public_plugins/state.rs`, `src-tauri/src/public_plugins/state_tests.rs`, `src-tauri/src/public_plugins/activation_bundle.rs`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/result_registry.rs`；现有 service/commands 测试所在模块；必要 `src-tauri/src/plugin_window.rs` teardown 调用。

**Dependencies:** Task 3；设计 §6–8、§10–11、§16.6–7/9/13。首先对照 M4 最终 `retire_plugin_requests`, `ManagementEffects`, `settle_management_effects`, checked authority publication 的实际实现；复用而非创建第二状态机。

- [x] 将 manager rename 从会丢 running 槽的 invalidate 路径切到同 Runtime 请求失效；所有 running/waiting 精确 context/token 进入 effects。通过 service 执行并在成功/错误出口都 settle 后才返回；commands 不再直接调用 manager 然后提前 ? 丢失 effects。
- [x] 真实 override 变化按包保守失效请求、网络 context 和结果句柄；完全相同的存储 override 才 no-op。不因 effectiveName 相同省略 durable commit；favorite 不误取消有效请求。仅 destroy/rebuild 才能释放 running 占用。
- [x] 审计本切片接触的 enable/install/upgrade/network/settings/secret/recovery/timeout/rename/favorite/迁移/rollback 的实际 authority 发布调用；全部复用 confirmed bundle/durable state/runtime/secret 的 checked 通道和 reservation/terminal 检查，close-only 路径不调用 reopen helper。

**Distinct test coverage:** A non-cooperative running、旧 waiting→改 A 或兄弟 B→提交 B/input：旧 running non-current 仍占槽、旧 waiting 精确结算、B 未提前 dispatch；旧 completion 丢业务结果但推进已保存 B，另一序列旧 handler 永不完成则既有 deadline/rebuild 推进 B。改名前旧网络 context 与复制 handle 均失效；service 在 post-durable 错误/Unknown 关闭已发生 effects 后仍结算全部 token，不误结算新 submission。default old/override custom→升级 default custom→clear 存储 null→重启/再升级 default new 跟随 new；反向 null→显式同名覆盖真实持久化并失效，重复目标 no-op 不写盘/迁移/取消。RESERVED/COMMITTING/DURABLE 各 barrier 均阻止 checked reopen；成功先 state、后 bundle/清 reservation、再 reopen；Unknown/复核失败不从旧快照复活。shutdown 先发生，随后延迟 rename/favorite/迁移/修复/包回退完成仍无 admission/delivery；只允许未来新请求，旧 context 永不复活。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::scheduler::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::activation::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests --offline`。

### Task 5: 独立收藏、严格旧 IPC 与管理/启动器 UI

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/model.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/public_plugins/state_tests.rs`；`src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/public-plugin-panel.tsx`, `src/main.ts`, `src/launcher-browser-preview.tsx`, `src/styles.css` 中必要现有样式；`src/protocol.test.ts`, `src/launcher.test.tsx`, `src/ui-theme.test.ts`。可新增聚焦 UI 测试文件，但不为复用抽象重构其他设置页。

**Dependencies:** Task 4；设计 §5、§8、§16.4/6/16；UI 规范 §5–9。Task 3 的基础 ID/DTO 类型已存在，此任务完成管理参数边界和可操作表面。

- [x] `set_plugin_favorite`/`set_plugin_effective_name` 在真实 Tauri 参数提取层保留字段 presence，显式 null 不得被 Option 自动等同缺失；仅设计指定旧路径的字段缺失且 default 实存时规范化。新前端所有调用总发 commandId，保留 require_main_window 授权检查先于受保护状态操作。
- [x] 完成 ResultFavorite 构造、safeResultFavorite、favoriteMatchesResult、目标相等和异步 favorite owner 的 pair 身份；可收藏主命令入口，业务执行结果不能伪造入口收藏。不要让 sibling 相同名称/结果 ID 或 prototype 属性匹配成功。
- [x] 一份包详情列出命令行，每行独立编辑/恢复名称和收藏状态；loading/busy/error 属于当前命令及当前 revision。重载/关闭/更换详情时，旧异步完成不得覆盖新目标或把焦点移回旧 DOM。主题 token、键盘、可访问名称与实际焦点归属遵循既有 UI 规范。
- [x] 保持包级 enable/settings/network 一份；不添加命令 title、hotkey、第二别名或命令卸载控件。所有现有 preview/mock/IPC 参数断言同步，旧包收藏通过 default 保持。

**Distinct test coverage:** 对两个管理 IPC 分别通过实际提取路径验证：缺字段且有 default 成功；缺字段无 default、显式 null、空、非法、未知 ID 拒绝且未改状态，绝不落第一项；非 main caller 先拒绝。favorite A 不改 B、跨重启保留、重复值 no-op、pending A 后切 B 仅更新 A owner；错误 sibling favorite payload 拒绝，constructor route/favorite/管理表正常。改 A/恢复默认的同包、他包、内置、quicklinks 冲突均无半提交并保留编辑值/可恢复焦点；旧名成功释放。双主题下 Enter/Tab/Escape、右键菜单、busy 禁用、错误提示、保存后恢复焦点、焦点触发元素消失时的合法 fallback 均有自动测试，并记录浏览器 Light/Dark 小窗口实检，不能仅看 activeElement mock。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests --offline`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::state::tests --offline`; `npm test -- src/protocol.test.ts src/launcher.test.tsx src/ui-theme.test.ts`。

### Task 6: CLI 每命令定位、去重及 legacy golden 兼容

**Files:** `packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/src/validate.ts`, `packages/plugin-cli/src/contracts.ts`, `packages/plugin-cli/src/report.ts`, `packages/plugin-cli/src/cli.ts`；`packages/plugin-cli/tests/manifest.test.ts`, `packages/plugin-cli/tests/validate.test.ts`, `packages/plugin-cli/tests/report.test.ts`, `packages/plugin-cli/tests/cli.test.ts`, `packages/plugin-cli/tests/pack.test.ts`；创建 `packages/plugin-cli/tests/fixtures/multi-command/` 与 `packages/plugin-cli/tests/fixtures/legacy-reports/`，fixture 内不含实际凭据。

**Dependencies:** Task 5；设计 §13、§16.14–15。Task 1 已提供新形态声明 metadata；本任务收口诊断与人类输出，Host target 仍 0.4.0。

- [x] 将逐项语义规则拆为可保留多个 findings 的定位规则，携带精确 pointer，并仅为语法有效且唯一 ID 的条目赋 commandId；包级/共享资源 issue 不带命令身份。
- [x] 贯通内部 ManifestValidationIssue、report issue、collector 存储/比较/去重和 human renderer。不同项各有稳定 pointer/rule rank；保持已有 phase、code、exit code、100 条上限及截断，不能以名称或数组序号伪造 ID。
- [x] 目录、archive、pack 嵌入 validate report 复用同一结果；legacy JSON key 集合、issue 字段和人类文本锁定 golden。新增诊断字段只在新形态出现，不因 nullable 字段输出 null。

**Distinct test coverage:** 双命令同时语义失败均保留；missing/invalid/duplicate ID 的每个 offending pointer 无 commandId；重复名称的有效唯一 IDs 保留；重排变 pointer/顺序、不变身份；runtime 资源缺失无 commandId；schema-invalid 无 plugin，semantic/API/platform 失败保留声明 metadata；非法 JSON byte offset 不变。人类输出在 ID 可用时显示 ID+pointer，ID 不可用时仅 exact pointer；legacy valid/invalid 目录与 archive JSON/human golden 不加字段，pack 嵌入同报告。超过 100 findings 的 phase、顺序、truncated 稳定。此时 .5 包的目标版本不兼容 issue 是预期；成功包/pack 的生产 CLI 验收在 Task 8 版本切换后执行，不新增未设计的 CLI --host-version 旗标。

**Verify:** `npm test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts tests/validate.test.ts tests/report.test.ts tests/cli.test.ts tests/pack.test.ts`; `npm run verify --workspace @uipilot/plugin-cli`。

### Task 7: SDK 文档与双命令离线示例

**Files:** `docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`, `docs/plugin-sdk/public-plugin-developer-guide.md`；新增 `examples/public-plugins/com.uipilot.demo-multi/README.md`, `examples/public-plugins/com.uipilot.demo-multi/package.json`, `examples/public-plugins/com.uipilot.demo-multi/tests/fixtures/multi-package/plugin.json`, `examples/public-plugins/com.uipilot.demo-multi/tests/fixtures/multi-package/dist/runtime.js`, `examples/public-plugins/com.uipilot.demo-multi/tests/runtime.test.js`, `examples/public-plugins/com.uipilot.demo-multi/tests/sdk-contract.ts`；`src-tauri/src/public_plugins/tests.rs` 的真实包 staging fixtures；必要现有 SDK contract 测试与 `scripts/verify-local-foundation.test.mjs` 发现数量断言。

**Dependencies:** Task 6；设计 §1、§3–8、§12–14、§16.3/12/16。选最接近的 `com.uipilot.demo-return`，运行时/测试沿其 mock Host 模式，不改旧示例 manifest 最低版本。

- [x] SDK invocation 增加可选 commandId，并准确解释新 wire 必有/旧 wire 完全省略、一个 onCommand 内按 ID 分派；文档区分 Manifest/API/state/report 版本和 Host/CLI 包版本，明确 replacing nameOverride、ID 删除/重加、包回退与旧二进制读 schema 2 不受保证。
- [x] 双命令示例使用一个 runtime.js，live upper 与 submit lower 两入口，共享包级状态/设置，只有必要权限；拒绝未知 ID，不能 default 分支随便执行第一命令。测试可明确验证 constructor 变体，但不需要为此增加第三产品命令。
- [x] README 给出离线测试、pack、Host 0.5.0 安装和预期交互步骤，明确此示例在 Task 8 发布前不为 current 0.4.0 接受。本任务将完整示例包放在 `tests/fixtures/multi-package/`（目录名不是 `package`），Node/runtime 和 Rust staging 测试直接使用它；协调器仍发现测试/SDK，但不把这个尚未发布的 fixture 当正式参考 package。Task 8 将该目录原子移动为示例根 `package/` 并更新导入，不增加发现白名单或重复生产 runtime。
- [x] 为真实目录/归档 Host staging 添加显式 0.5.0 Host 测试，验证完整新包及旧示例原包；SDK 正反类型用例覆盖 optional 字段，runtime keys 的 legacy golden 不改。纯静态 staging 不启动实际插件服务。

**Distinct test coverage:** 两 ID 输出正确及未知 ID 明确失败，旧 command invocation 缺 key、新形态必带 ID；shared runtime/settings/storage 由一个包管理；跨版本/reorder 的 identity 不按启动名分派；新示例正确要求 .5，0.4 current 拒绝，显式 .5 Host 真实 stage 成功，旧 return/panel/window/pomodoro 仍 stage。SDK contract 无新增 actions/onAction、hotkeys 或多命令 panel API。所有测试只用临时数据/假 Host，不读取真实凭据。

**Verify:** `node --test examples/public-plugins/com.uipilot.demo-multi/tests/runtime.test.js`; `node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.demo-multi/tests/sdk-contract.ts`; `cargo test --manifest-path src-tauri/Cargo.toml --lib public_plugins::tests --offline`。测试此时从 `tests/fixtures/multi-package/` 读取，Task 8 移动后更新相对路径并断言内容/hash 不变。

### Task 8: 原子 0.5.0 发布准备、隔离 pack/staging 与最终验收

**Files:** `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src-tauri/src/public_plugins.rs`, `src-tauri/src/public_plugins/network/broker.rs`, `packages/plugin-cli/src/contracts.ts`；新增 `tests/fixtures/public-plugin-pack/multi-command.uipilot-plugin`；`packages/plugin-cli/scripts/smoke.mjs`, `packages/plugin-cli/tests/validate.test.ts`, `src-tauri/src/public_plugins/tests.rs` 及实际版本断言；Task 7 fixture 移动到 `examples/public-plugins/com.uipilot.demo-multi/package/plugin.json`, `examples/public-plugins/com.uipilot.demo-multi/package/dist/runtime.js` 并更新其 Node/Rust fixture 路径；`README.md`, SDK/CLI README 的版本与发布说明；对应正式规格/路线图状态；scratch 验证日志/最终评审包。

**Dependencies:** Task 7；设计 §1、§12、§16 全矩阵；所有任务双门禁关闭。此任务才允许 Host current/CLI target/应用版本从 .4 一起到 .5。

- [x] 统一发布版本与 lockfile package 条目，保持 CLI 自身包版本和旧功能最低版本不变；区分精确 current 断言与历史 .4 fixture，不能全局替换 0.4.0。Host network User-Agent 及其精确字节断言同一提交改为 `UiPilot-Plugin/0.5.0`。原子加入正式 demo-multi package，所有参考包发现/测试/SDK 集合继续非空且全覆盖。
- [x] 为生产 CLI 成功验证和 pack 增加双命令/单项数组 fixture。延续现有 isolated CLI smoke 的 Node permission、network trap 和 bundle 审计：validate 保留临时目录只读边界及既有 Windows 系统路径例外；pack 为祖先元数据检查保留现有较宽读取许可，写入严格限于唯一临时输出目录，不宣称 pack 被禁止读取仓库或父目录。目录/归档报告等价、重复 pack 字节/hash 一致、已有输出不覆盖、失败不产包，绝不运行插件 Runtime。
- [x] 沿 M1 已跟踪归档接缝，用刚构建的生产 0.5 CLI 从最终 demo-multi package 生成并提交 `tests/fixtures/public-plugin-pack/multi-command.uipilot-plugin`。Rust 测试 `cli_multi_command_pack_fixture_stages_on_0_5_0` 以 include_bytes! 将完全相同的字节交给全新 TestRoot staging，验证完整命令集合及资源字节。isolated consumer smoke 把 demo-multi 加入 copy/pack 集合，使用本次 verify 重新构建的已安装 CLI 产包，在清理临时目录前逐字节及 SHA-256 比较该 tracked fixture；不使用旧 dist、不在测试中重写 fixture、不跳过缺失前提。root 保留 Rust-before-CLI 顺序，最终完整通过证明两端消费字节相同。历史 .4 Host 拒绝新包、.4 凭据旧包继续兼容。升级/删除/包回退/重启的完整矩阵引用前任务行为测试证据，发现跨任务缺口才新增聚焦回归。
- [x] 运行完整 verify 协调器并记录日志及真实 exit code；随后按各任务 base..head 与 M4 最终批准 baseline..HEAD 生成最终 diff 包，派发独立最终 Spec+Quality review，统一修复全部最终 findings 并重跑覆盖修复的检查。记录真实 Windows Host/UI/焦点/剪贴板验收的已执行证据或明确 pending，不把浏览器/单测替代它。
- [x] 发布说明明确 schema-2 后旧宿主二进制降级不保证、仅新 Host 运行旧包及在新 Host 内包回退受支持；路线图只更新 M2-A，完整 M2 actions/快捷键处置及 M3 继续未完成。没有远程发布/push/安装器部署步骤。

**Distinct test coverage:** current/CLI target/根 Cargo/Tauri/lock 及 broker User-Agent 同为 0.5.0，旧功能门槛和 CLI 0.1.0 独立；生产 CLI 新包 valid、确定性 pack 和隔离 consumer 均通过；Rust 实际 stage 的 tracked fixture 与本次 fresh isolated CLI pack 的字节及 SHA-256 完全相同，任何漂移令门禁失败；Host 完整命令集合与 CLI 报告一致，新旧包都在自动发现集合；root verify 无 skip 规避。最终 reviewer 逐条核对设计 §16.1–17、reservation/shutdown 与保存 override 语义的跨任务证据；人工 Host 未跑时结果明确记为 pending，不宣称真实系统总验收完成。

**Verify:** 顺序执行 `npm run verify --workspace @uipilot/plugin-cli`; `cargo test --manifest-path src-tauri/Cargo.toml --lib cli_multi_command_pack_fixture_stages_on_0_5_0 --offline`; `npm run verify:local`；`git diff --check`。root verify 已包含前端测试/build、Rust lib offline、schema --check、CLI verify/isolated smoke、全部公开 Node 测试、SDK 类型和全部参考包验证，不无理由重复整套。真实 Host 人工步骤依 Task 7 README 和 UI 规范单独登记。

## Exit and coverage ledger

| 设计 §16 验收 | Owner |
| --- | --- |
| 1–2 版本/manifest | Task 1；发布 current 与 pack 在 Task 8 |
| 3–5 路由/队列/严格 activation/opaque | Task 3；管理缺失兼容在 Task 5 |
| 6–7 收藏/覆盖/同 Runtime effects | Task 2 存储，Task 4 请求/authority，Task 5 UI/IPC |
| 8–12 schema/迁移/失败/恢复/删除/回退 | Task 2；Task 4 错误 effects，Task 8 发布说明 |
| 13 authority/终止 | Task 4，Task 8 最终跨任务审查 |
| 14–15 CLI 报告 | Task 6；Task 8 current .5 成功 pack |
| 16 旧包/UI/真实 Host | Tasks 3/5/7；Task 8 分开记录人工验收 |
| 17 阶段退出 | readiness gate 与 Task 8 |

自检：任务标题可解析，依赖顺序无环；每任务有 checkbox/files/dependencies/distinct tests/Verify；共享合同仅索引一次；没有完整 TDD/commit 模板反复复制。最宽 Task 3 是同一身份字段的跨 Rust/TS 编译边界，不拆为“先发新后端、后修旧前端”的不兼容提交。真实技术选择来自已批准设计；本计划没有新增产品待决项。
