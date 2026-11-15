# Mac 内置功能启用管理

让用户在设置中启用/停用已可用的网页搜索与快速链接；保留配置和收藏。采用现有 Rust/Tauri/React/Ant Design 及 SettingsStore，遵循[设计](../specs/2026-11-15-macos-builtin-management-design.md)的 IPC、最终准入和 UI 生命周期边界。

单个逻辑单元：聚焦失败断言 → 实现 → 全量回归 → 合同/代码自查 → 提交 → CLI 打包与交付。使用合成临时数据和 fake browser；真实 GUI 只由用户操作。无新依赖，不合并 main，不开放生产插件 Runtime。

### Task 1: 宿主状态与列表接线

**Files:** macos_builtin_management、macos_host/platform、build.rs/ACL、macos_apps 测试、Mac plugin management client/UI/tests。

- [x] 两种 feature 的 main-only IPC 与持久化。
- [x] 列表分组、筛选、底栏提示、保留焦点的开关和跨页面队列。
- [x] 回归与内部 App 构建。
- [ ] DMG 校验与人工交付。

**Distinct test coverage:** 停用发生在浏览器最终准入前则旧结果零 dispatch，原链接保留；请求在途时第二次切换被阻止且原焦点不丢；写入失败时状态不乐观更新、刷新前禁止再写；新挂载读取排在旧挂载写入之后；未知 feature/额外字段与非 main caller 拒绝。

**Verify:** Rust 库测试（preview features）；默认和 preview all-targets；Mac management/client 与完整 launcher Vitest；`node --test scripts/macos-host-acl.test.mjs`；`npm run build`；App 构建与 `verify-macos-bundle.mjs --documents`。

人工节点：替换重启后查看内置分组，停用网页搜索/快速链接并核对搜索入口，重启保持、重新启用后配置收藏保留；一次一个动作。

## 自动验证与自查

Rust 库最终全量 705 passed / 22 ignored；Mac management/client、launcher 与文本导航前端 450 passed / 18 skipped；ACL 5 passed。默认和 preview all-targets、TypeScript/Vite、arm64 App 构建通过。三项 backend 定向测试覆盖状态持久化、严格 DTO 和写入失败回滚。

首次全量的既有 DNS 取消测试出现一次 Busy/Timeout 时序失败；未修改网络代码，单项复跑和后续两次全量通过。保留原始与复跑日志，不将首次失败隐藏。

合同/代码自查：仅 main 获得新 IPC，来源检查早于状态访问；后台写入复用原子 SettingsStore；未增加文件路径输入或开放其他功能。前端复用跨挂载串行队列及 generation，失败不发布乐观状态，不自动重试；开关保存期间使用 aria-disabled 保持焦点，底栏左侧统一反馈。新增分组复用主题 token，真实视觉和键盘操作由用户验收；浅色实机按用户要求跳过。未使用 GUI 自动化，未开放 Mac 生产插件运行，也未合并 main。
