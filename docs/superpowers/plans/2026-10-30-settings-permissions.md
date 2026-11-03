# 设置权限分类管理实施计划

目标：实施 [已确认设计](../specs/2026-10-30-settings-permissions-design.md)，在设置中集中按权限分类管理公开插件授权，并保证撤销在宿主生效。

架构：Rust 插件管理器负责持久化、能力授权与资源退役；React 权限页读取严格清单，复用现有网络确认和 UI token。使用现有 React、Ant Design、Lucide、Tauri，不增加依赖。

## 全局约束与执行

- 当前目录/功能分支已获授权，保留所有既有未提交修改；不修改 rust_uia、不发消息推送。交叉改动不混合提交。
- 使用 subagent-driven-development 分工与独立审查；先定向失败测试，再实现并验证。宿主和页面可按下列固定契约并行，集成依赖二者。
- 主界面管理接口：`set_public_plugin_permission_access(pluginId, permission, granted, expectedInventoryRevision)`；前端 `setPublicPluginPermissionAccess({pluginId:string,permission:PublicPermission,granted:boolean,expectedInventoryRevision:U64Decimal}):Promise<void>`。平铺 IPC 参数，清单使用既有 PublicPluginInventory。
- 新增主窗口事件 `public-plugin-permissions-changed`，前端可选监听 `listenPublicPluginPermissionsChanged` 收到后权威回读，不使用事件 payload 作为权限数据。
- 所有权限变更使用当前清单版本；网络分派至已有网络范围校验/撤销事务，保留既有网络管理接口供详情页调用。
- 仅主 WebView 可管理，校验先于状态访问；禁止插件自我授权、未申报与未支持授权。
- 按设计“开关行为”“宿主与数据边界”保证撤销、重启持久化、重载/更新保留撤销；不隐式启用插件、不清理用户插件数据。
- 测试用临时插件/内存客户端；不自动修改用户安装插件授权或模拟实际外部输入。

### Task 1: 宿主安装后权限管理与撤销

**Files:** `src-tauri/src/public_plugins/activation.rs`, `state.rs`, `state_tests.rs`, `src-tauri/src/public_plugins.rs`, `commands.rs`, `lib.rs`, `build.rs`, `capabilities/main.json` 及新管理命令权限文件；按实际生命周期需要最小修改 plugin_panel/plugin_window/timer 等宿主模块。

- [x] 实现普通权限单项授权/撤销，复用现有管理事务、版本校验与网络专用路径。
- [x] 核对全部受支持权限的调用与长期资源边界，退役旧结果/请求，撤销界面关闭受影响会话，通知/计时器清理有关活动资源，维持其他权限。
- [x] 部分授权可持久化及恢复；重新加载/升级保留仍申报权限的撤销，新权限沿用确认，拒绝未申报授予。
- [x] 注册主窗口专用管理 IPC、事件同步；更改失败回滚或按现有失败关闭合同收敛。

**Distinct test coverage:** 非主调用先拒绝；未申报/未支持/过期版本/停用插件管理；各能力撤销后拒绝、旧请求和界面/计时器退役；授权和撤销不更改无关权限；持久化失败、发布失败；重启/重载/更新保留撤销与新增权限确认。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib permission_access` 及实际涉及模块定向测试；`cargo check --manifest-path src-tauri/Cargo.toml --lib`。

### Task 2: 权限分类页面

**Files:** 新建 `src/plugin-permissions-panel.tsx`, `src/plugin-permissions-panel.test.tsx`, `src/plugin-permissions-panel.css`。必要共享展示 helpers 可新建文件；不改 launcher-view/protocol/main（协调者集成）。

**Dependencies:** 固定客户端契约；设计“页面”“开关行为”。

- [x] 导出 `PluginPermissionsPanel({client,active}:{client:LauncherClient;active:boolean})`，组件自己读取清单、订阅 public-plugin-permissions-changed 事件，防旧请求覆盖，隐藏后停止页面级快捷键/监听。
- [x] 分类行、插件/启动命令/权限搜索、网络范围展开、状态和无权限/无结果/错误展示；复用现有图标与主题。
- [x] 同插件操作串行，权威回读后解除 pending；网络确认版本绑定，取消无写入，错误只重读不自动重复写入；详情页通过同一事件状态同步。
- [x] Ctrl+F、Tab、模态焦点返回；深浅主题及窄窗口不溢出；未支持禁用、已停用插件仍可管理。

**Distinct test coverage:** 分类/搜索/细分权限；未支持与停用项；确认取消和范围更新；同插件并发、写入失败后读取失败、迟到读取和卸载；Ctrl+F 仅 active 生效、键盘焦点。

**Verify:** `npm test -- --run src/plugin-permissions-panel.test.tsx`；视觉验收由协调者集成后执行。

### Task 3: 设置导航、客户端与集成验收

**Files:** `src/protocol.ts`, `main.ts`, `launcher-view.tsx`, `launcher-core.ts`, `launcher-browser-preview.tsx`，相关测试 fixtures；文档。

**Dependencies:** Tasks 1–2 接口；设计“验证”。

- [x] 新增 permissions tab 类型与设置导航、挂载页面，保留设置键盘导航和已有 tab 行为。
- [x] 接通客户端 IPC，预览及测试客户端补齐契约，提供多权限预览数据。
- [x] 验证页面与详情同步、导航、网络授权；前端构建和相关回归、Rust 定向检查、git diff 检查。
- [x] 独立审查宿主撤销和异步 UI 行为，修复实际问题；记录全量测试已知失败与人工验收点。

**Verify:** `npm run build`; `npm test -- --run src/plugin-permissions-panel.test.tsx src/protocol.test.ts src/launcher.test.tsx`; `git diff --check`。


## 最终验证（2026-10-30）

- 前端最终 `npm run build` 通过；权限页/详情同步/协议 45 项通过，导航/滚动布局 2 项通过。此前相关四套完整运行 412 通过、18 跳过，唯一旧滚动容器数量断言已更新并复测通过。
- Rust 权限专项 17 项通过；状态 36、剪贴板历史 18 及延迟通知/计时器/结果动作/网络/凭据/管理授权相关过滤均通过，共 130 次测试执行（含重叠）。未重复运行已有无关失败的全量套件。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib --release` 通过。默认 debug 检查因运行中 dev 的 Everything.exe 文件占用失败，未终止用户进程。
- 独立审查四项问题均修复并复核：原生关闭错误、失败关闭时剪贴板记录授权、记录写入与撤销的同步、已出队延迟通知在撤销重授权后的失效。
- 浏览器以合成插件完成 Dark 720px / Light 520px、搜索与 Ctrl+F、普通开关、网络确认及 Escape、详情双向状态同步验收；未更改真实插件授权。
- SDK 文档补充安装后权限变化、重载保留撤销及旧任务失效语义；无 rust_uia 改动，无消息推送，保留当前分支未提交修改。
- 详细证据：`.superpowers/permissions-host-report.md`、`permissions-ui-report.md`、`permissions-review.md` 及相关验证日志。
