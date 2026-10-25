# 普通插件设置编辑实施计划

目标：交付Host0.8.0插件详情普通设置表单及可验证的离线设置示例。设计来源：[普通设置设计](../specs/2026-10-25-plugin-settings-editor-design.md)。使用现有React19/Ant Design6、Tauri2及API v1，新增独立组件与聚焦回归，不改后端保存语义。

## 全局约束

- 遵循设计的“数据与校验”“交互与异步所有权”“可运行示例与版本”，以及docs/ui-guidelines.md。
- 单一源码写入agent；主agent只编辑本轮文档/忽略的报告及交付目录。W为C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools，原D:/code源码只读；共享Rust target可用于构建。无需新工作树。
- 不运行原生Host、安装真实插件或操作用户数据/剪贴板/网络。用户执行人工功能测试；自动化限类型、构建和必要的边界/异步/持久化检查。
- 针对实际失效风险先写聚焦失败检查再实现，不为简单可逆样式写机械测试。完成提交后独立规格/质量评审，再由主agent交付。保留branch不合并/推送。

### Task 1: 普通设置编辑与离线示例完整交付

**Files:** src/public-plugin-settings.tsx（新建，按需拆纯值辅助模块）；src/public-plugin-panel.tsx；src/styles.css；src/public-plugin-settings.test.tsx（新建）；src/launcher-browser-preview.tsx模拟普通设置patch保存；examples/public-plugins/com.uipilot.demo-settings/（plugin-owned manifest/runtime/tests/README/package脚本）；package.json/package-lock.json、src-tauri/Cargo.toml/Cargo.lock/tauri.conf.json、packages/plugin-cli/src/contracts.ts及相关版本断言；SDK developer-guide添加操作入口说明。

**Dependencies:** 已接受设计。普通设置复用savePublicPluginSettings，不改wire API；所有非secret四类全覆盖。公共插件代码前先读public-plugin-developer-guide及schema/API，参考demo-return/multi。

- [ ] 提取字段编辑组件，按设计实现dirty patch/校验/取消/未设置和secret隔离。
- [ ] 接入详情的互斥busy、已接受inventory和异步session所有权；实现失败不确定提示与重新读取恢复。
- [ ] 创建无副作用的settings-demo及1.0.1兼容升级生成脚本，明确人可读输出；不动recovery数据或旧fixtures。
- [ ] 同步Host0.8.0及CLI target，更新相关当前版本断言和最小使用说明。
- [ ] 运行聚焦检查与构建，自审提交；将完整结果写入.superpowers/sdd/local-foundation/settings-editor-report.md。

**Distinct coverage:** 仅变化普通字段进入IPC/secret不发送；unset不被隐式初始化；空数字/范围错误不提交；原型特殊key正确；失败保留草稿且不盲重试；成功+刷新失败明确区分；pending重复提交只调用一次；关闭重开/换插件后旧完成不刷新新详情；定义变化不能静默覆盖；保存与开发重载互斥。Runtime测试当前设置读取及无存储写入。

**Verify:** npm test -- src/public-plugin-settings.test.tsx src/public-plugin-recovery.test.tsx；npm run build；node --test examples/public-plugins/com.uipilot.demo-settings/tests/runtime.test.js；npm run build --workspace @uipilot/plugin-cli，然后按本地CLI help验证/打包两个fixture。仅版本配置变动需cargo check --manifest-path src-tauri/Cargo.toml --offline --locked，不重跑全Rust。新增合理的相关聚焦检查可补充并报告。

## 主agent交付与人工检查点

独立评审Task1整个diff，解决阻塞项；冻结源码后构建acceptance独立exe与5资源，校验稳定输入/新exe/两包及旧交付不变，写中文用例。用户先测默认值→四类保存生效→重启保留，再测取消、校验、升级保留和键盘双主题。源码实现、自动验证、人工验收分别记录；本轮不计完整基座完成。