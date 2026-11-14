# macOS 插件管理界面

目标：将已验证的安装与权限生命周期接到设置页，同时维持插件执行关闭。技术与状态边界以[管理界面设计](../specs/2026-11-08-macos-plugin-management-ui-design.md)为准；沿用 React/Ant Design、Tauri main-origin ACL 和既有 Rust 私有文件事务。

统一验证：围绕真实失败路径增加测试、观察红/绿；完成聚焦与完整前后端回归、浅深主题渲染和独立审查后，以一个原子提交推送当前 macOS 分支，不合并 main。只使用临时测试目录及独立无头浏览器，不打开 UiPilot 原生 GUI，不修改用户系统权限或个人安装库。

### Task 1: 设置页、可信管理命令和恢复接线

**Files:** 新增 `src/macos-plugin-management*`、Mac 管理 Rust 门面和 IPC；修改 launcher/client/capabilities、Mac app 注册和精确 ACL；增加对应测试、浏览器 preview 与验收记录。

- [x] 独立管理能力、真实列表与普通设置/启停/权限配置；保留执行门禁。
- [x] 原生选包、单次准备 token、revision 确认、升级与明确的移除语义。
- [x] 草稿、键盘、异步回包所有权、关闭清理与不确定提交后的明确核对。
- [x] 自动测试、编译、双主题渲染与独立源码审查。

**Distinct test coverage:** 新 Mac 管理页不得调用旧执行/插件接口；确认取消保留原选择和草稿；旧客户端迟到响应不得复活；存储事务已准入时 reset 立即退休 owner、不等待 IO、不谎称回滚；AfterRename 失败不自动重写，下次明确读核对实际提交且旧 token 失效；移除后重装重新授权、不复活普通设置。

**Verify:** `npm test`、`npm run build`、`node --test scripts/macos-host-acl.test.mjs`；`cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview`；同特性的 `cargo check --all-targets`。

原生选包/授权与完整应用手工验收另列未测，不以浏览器渲染替代。

### Task 2: 列表直接启停与配置恢复验收

**Files:** `src/macos-plugin-management.{tsx,css}`、组件测试、`src-tauri/src/public_plugins/management_macos_tests.rs`、设备验收记录。

**Dependencies:** Task 1；设计的“列表直接启停整个插件”和“界面、键盘与异步状态”。

- [x] 列表开关与详情共用启停入口，独立可访问控件，反馈统一底栏。
- [x] 连点仅提交一次；迟到/失败不乐观改变已确认开关，明确刷新后才能继续。
- [x] 真实磁盘重开验证多命令配置、普通设置和撤回权限在停用/启用后保留。
- [ ] 新候选在 M1 上验收列表启停、主目录同步、重启保留和权限配置独立性。

**Distinct test coverage:** 开关操作不误进详情、不改权限；异步停用确认前保持原状态，重复点击不重复提交；stale 拒绝后刷新读回新清单，不自动重放旧操作；多命令包停用并重开后目录为空，重新启用及重开后恢复逐命令名称/收藏，拒绝的剪贴板权限仍保持拒绝。

**Verify:** `npm test -- src/macos-plugin-management.test.tsx src/macos-plugin-management-client.test.ts`；Rust `public_plugins::management_macos::tests` 过滤器；`npm run build`。发布 App/DMG 与手工验收分开记录。按用户要求不使用 GUI 自动化，新布局的实机和主题外观留给人工节点。

2026-11-14 自动证据：组件/客户端 26 通过；真实管理器 15 通过；Launcher 的 Mac 聚焦集成 2 通过（423 个无关用例未运行）；main ACL 4 通过；TypeScript/Vite 构建通过。新增组件回归先确认缺失开关导致失败，再实现通过。Rust 回归只用私有临时目录，不改动个人插件数据。新包的 GUI/浅深主题仍待用户实测。
