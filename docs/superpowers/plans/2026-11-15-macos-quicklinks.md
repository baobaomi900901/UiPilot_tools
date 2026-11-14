# macOS Quicklinks 接线

完成不依赖证书的快速链接管理、命令搜索和浏览器执行。采用现有 Rust/Tauri/React、共享 Quicklinks 格式和原生 AppKit；遵循 [设计](../specs/2026-11-15-macos-quicklinks-design.md) 的命名空间、执行所有权、IPC 与选择器边界。禁止 GUI 自动化，不合并 main。

统一先加失败断言，再实现与回归；临时数据和 fake opener 验证，不调用真实浏览器。自查代码和合同后提交。任务顺序 1 → 2。

### Task 1: 存储、命名空间与执行接线

**Files:** quicklinks、management_macos、macos_apps、macos_host、result_registry、平台模块及 ACL。

- [x] 共享存储 Mac 原子适配、双向名称冲突与原生图标选择。
- [x] 命令发布、收藏、浏览器一次性执行和完成归属。

**Distinct test coverage:** 预览安装后 quicklink 占名，最终提交拒绝；编辑/删除发生在浏览器最终准入前后产生不同结果；错误 caller 在状态/文件访问前拒绝；选择器旧 owner 不覆盖新 owner。

**Verify:** Mac 库 quicklinks / management / macos_apps 聚焦测试、all-targets 检查、main ACL 测试。

### Task 2: 管理界面与人工候选

**Files:** host-capabilities、launcher-core/view 测试、发布记录及内部构建产物。

**Dependencies:** Task 1；设计「查询与执行所有权」和「验证边界」。

- [x] 窄能力启用现有管理面板、收藏路径和键盘交互。
- [x] 自动回归、arm64 App/DMG 校验与测试包交付。
- [ ] 用户逐步人工验收。

**Distinct test coverage:** Mac 能力不开放无关内置功能；创建/编辑/删除后查询更新；空命令只补全；收藏、设置及重启保留。

**Verify:** launcher.test.tsx、npm run build、verify-macos-bundle.mjs；实机操作由用户执行。

## 自动验证记录

Mac 库全量 626 通过、22 项既有 ignored；启动器 411 通过、18 项既有 skipped；ACL 4 通过。带 preview 与默认配置的 all-targets 检查、TypeScript/Vite 构建通过。新增磁盘提交失败时保留已保存缓存测试。自查命名空间双向互斥、最终执行准入、调用者身份及选择器生命周期；未运行 GUI，安装后交互待用户验收。

后端提交 `0052602`，界面提交 `aa3a72d`。arm64 App 构建及 CLI 生成 DMG 的只读挂载、签名、资源、最低系统验证通过；ZIP 全部条目 CRC 和 manifest 文件哈希通过。交付 `~/Documents/UiPilot-Mac-quicklinks-aa3a72d.zip`，详见设备验收单最新候选。人工验收待开始。
