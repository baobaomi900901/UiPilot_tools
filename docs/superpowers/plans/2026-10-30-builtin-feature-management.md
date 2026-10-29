# 内置功能管理实施计划

目标：在“设置 → 插件”展示截图、快捷链接、网页搜索的内置条目，支持详情和可持久化的启停，并在所有入口执行状态检查。

架构：宿主维护专用的三个功能身份和持久化停用集合；宿主接口提供权威清单与启停操作。前端把内置条目与现有插件列表合并展示，详情复用现有设置页外壳，但保持两类身份和操作分离。设计合同见 [已确认方案](../specs/2026-10-29-builtin-feature-management-design.md) 的“列表和详情”“关闭的具体含义”“实现边界”。

技术：Tauri/Rust + React/TypeScript。仅修改宿主与前端；`D:\code\rust_uia` 只读。保留当前工作区其他未提交改动。禁用入口不会删除配置、链接或既有贴图。

执行规则：每任务先写能失败的重点回归，再做最小实现并验证；每任务独立提交且审查，不混入用户现有改动。依赖顺序 Task 1 → Task 2 → Task 3 → Task 4。Rust 专项用 `TAURI_CONFIG={"bundle":{"resources":[]}}` 运行测试，最终做完整截图/设置相关回归、前端测试、格式和 diff 检查。UI 按 `docs/ui-guidelines.md`。

### Task 1: 持久化和内置清单

**Files:** `src-tauri/src/settings.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/protocol.ts`, `src/main.ts`。

- [ ] 专用类型仅允许 screenshot/quicklinks/webSearch；旧配置默认全部启用，停用集合保存到现有设置文件。
- [ ] 宿主只向主窗口暴露内置清单和启停接口；清单含功能标识、名称、命令、说明、示例、宿主版本、启用状态。不复用公共插件 ID 或卸载接口。
- [ ] 写入失败保持当前启用状态，接口响应从权威设置读取。

**Distinct test coverage:** 旧配置缺字段与重启往返；非法标识及非主窗口调用在触碰设置前拒绝；保存失败不翻转可见状态；详情元数据顺序固定。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib settings::tests -- --test-threads=1`；`npm test -- protocol.test.ts`。

### Task 2: 发现、执行和热键门禁

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/lifecycle.rs`, `src-tauri/src/screenshot/hotkey.rs`, `src-tauri/src/settings.rs`，必要时现有结果注册模块。

**Dependencies:** Task 1；设计“关闭的具体含义”“实现边界”。

- [ ] 搜索和命令结果仅展示启用功能，已有结果在最终执行时再次检查；快捷链接管理和用户命令也按同一状态拒绝新动作，网页搜索普通结果及兜底入口一致。
- [ ] 截图命令与全局热键在关闭后不能创建会话；禁用注销注册但保留保存的快捷键，启用时按现有冲突/回滚流程恢复。停用期间编辑快捷键不重注册。
- [ ] 状态变更使当前搜索结果刷新，保留输入和其他功能状态；已进入截图编辑和贴图继续存在。

**Distinct test coverage:** 旧搜索结果在停用后执行被拒绝；快捷链接子命令停用及数据恢复；网页搜索普通结果/显式命令一致；热键注销、冲突恢复保持关闭与原配置；状态更新与正在进入的截图的先后顺序有明确单一结果。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::tests -- --test-threads=1`；截图和生命周期专项测试。

### Task 3: 设置列表和详情

**Files:** `src/public-plugin-panel.tsx`, `src/launcher-view.tsx`, `src/protocol.ts`, `src/main.ts`, `src/styles.css`, `src/launcher.test.tsx`。

**Dependencies:** Task 1 API，Task 2 启停语义；设计“列表和详情”。

- [ ] 同一列表顶部固定三个带“内置”标识的条目，与公共插件共用搜索框和行样式。只显示启用开关及详情入口，无删除/卸载/重新加载。
- [ ] 详情沿用当前设置页头部、返回键、Esc、焦点和滚动框架，显示宿主元数据及启用状态；旧设置入口位置不变。
- [ ] 启停期间禁重复提交；失败刷新权威清单并显示就地错误。停用后仍可在列表/详情重新启用。

**Distinct test coverage:** 三项固定排序和搜索过滤；不存在内置删除按钮或插件卸载调用；详情返回焦点与暗/亮主题；启停失败后 UI 读回、快速连点单次提交；原公共插件详情/卸载不退化。

**Verify:** `npm test -- launcher.test.tsx`；`npm run build`。

### Task 4: 集成验收与说明

**Files:** `docs/testing/builtin-feature-management-acceptance.md`，按测试结果修正 Task 1–3 范围内文件。

**Dependencies:** Task 2、Task 3；设计“验证”。

- [ ] 从设置关闭/启用三个功能，检查入口、旧结果、持久化及用户数据；明确哪些真实桌面行为仍需人工验收。
- [ ] 检查 `rust_uia` 工作区干净、原有脏文件未吸收，运行格式及 diff 检查；记录新 dev 测试操作。

**Distinct test coverage:** 端到端前端桥接与宿主设置快照一致；重新启用后原快捷链接、搜索引擎、截图快捷键及贴图状态；现有插件列表和删除路径保持可用。

**Verify:** `npm test`；`cargo test --manifest-path src-tauri/Cargo.toml --lib screenshot:: -- --test-threads=1`；相关后端完整专项。
