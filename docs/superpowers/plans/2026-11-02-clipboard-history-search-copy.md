# 剪贴板历史查询与复制实施计划

目标：按焦点分离输入框查询与记录复制，Ctrl+K 同步上下文。
架构：宿主按原始历史数据查询并受权限保护地写剪贴板；公开桥提供 list({query}) / copy({id})；插件负责焦点、分类与动作。
技术：Rust/Tauri、JavaScript Panel、TypeScript SDK、Node test/Vitest。
规范：[已授权设计](../specs/2026-11-02-clipboard-history-search-copy-host-capability.md)。版本下限使用新 Host 0.17.0，插件 1.2.0；不改变旧 paste 行为。工作在当前目录保留既有修改，不自动提交。
公共验证：新增行为先加失败测试；分别验证 Rust 数据/权限边界、桥与参数契约、Panel 查询与焦点顺序；最后编译主程序、构建 CLI 并打包插件。真实剪贴板与窗口焦点验收使用明确的测试数据，不读取无关用户历史。

### Task 1: 宿主查询与仅复制
文件：src-tauri/src/clipboard_history/、commands.rs、public_plugins/activation.rs、lib.rs、build.rs、capabilities。
依赖：设计“建议宿主能力”。
- [x] list 支持 <=512 Unicode 字符查询，匹配全文及任一文件名，返回摘要且排序不变。
- [x] copy command 在当前 Panel 和 read/copy 授权下读取完整记录并写入剪贴板，不隐藏或触发 Ctrl+V；写入与撤权/会话切换的顺序受现有门控保护。
- [x] 测试长文本末尾、第二文件名、空查询、图片、越界输入和授权失败；保留旧接口兼容。
验证：cargo test --lib clipboard_history；cargo check --lib。

### Task 2: 公开桥、权限和版本
文件：plugin_panel.rs、public_plugins/manifest.rs、src/protocol.ts、权限 UI、SDK docs/schema、packages/plugin-cli。
依赖：Task 1 命令接口；设计“建议宿主能力”“开发边界”。
- [x] 新增 clipboard.history.copy 权限及 0.17.0 版本下限、依赖 read、仅 Windows Panel，更新权限显示和安装校验。
- [x] list 查询不能污染全量推送缓存；copy 使用当前会话，返回真实完成结果，冻结并校验输入/输出。
- [x] 测试过期会话、未知字段、查询结果 revision 相同、未授权及 SDK/Schema 同步。
验证：npm run test -- src/plugin-panel-bootstrap.test.ts；npm run check --workspace @uipilot/plugin-cli；对应 manifest 测试。

### Task 3: 插件查询与焦点
文件：examples/public-plugins/com.uipilot.clipboard-history/。
依赖：Task 2 公开 API；设计“插件接入与焦点交互”“验收”。
- [x] 移除 Enter Host Key，content 焦点路由用于从输入框进入列表；本地上下键移动及 Enter 复制。
- [x] 查询更新、分类和历史推送共用请求代数，避免旧快照回填；异步状态不抢新焦点。
- [x] Ctrl+K 按 source 区分查询/记录动作，复用宿主搜索和返回输入框；保留分类及删除。
- [x] 更新预览、SDK 合同、版本和 README；测试菜单互斥、空列表、IME、重复 Enter、过期复制与查询。
验证：node --test examples/public-plugins/com.uipilot.clipboard-history/tests/*.test.js；真实 CLI validate/pack；人工 Host 验收焦点与三类记录完整复制。

## 验证记录

- Rust clipboard_history 相关 39 项测试通过（含查询、权限、会话、旧粘贴兼容）。
- 桥/动作菜单/权限页 43 项测试通过；插件 42 项测试通过，SDK 类型和 CLI 校验通过。
- 主程序前端生产构建及最终 `cargo check --lib` 通过。最终插件包已重新生成并校验，且归档内容与最新插件源码一致，SHA256 `601f1fef876c4cf661182f98ceb3764295cfc82b2c7a07fc31b88daa69ab9e84`。
- 浏览器合成数据在深/浅主题验证了 Enter 查询、Down 入列表、Enter 复制反馈、Ctrl+K 两种焦点菜单与 Ctrl+F 返回输入框；未将预览的模拟复制当作系统剪贴板验收。
- 人工验收待用户使用新版 Host 与插件并授权复制后，检查跨原生 WebView 焦点以及文本/图片/文件的真实粘贴完整性。
