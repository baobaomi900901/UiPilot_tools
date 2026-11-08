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
