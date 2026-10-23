# 本地插件基座 M0 实施计划

目标：建立可复现的全仓本地验证闭环，防止宿主、SDK、CLI 和公开示例继续漂移。总路线与 M0 合同见 [设计](../specs/2026-10-23-local-plugin-foundation-design.md)，本计划仅执行 M0；后续阶段分别形成可验收的计划，不把路线图标记为已实现。

技术：现有 Node ESM、Vitest 4、TypeScript 7、Rust 1.96、Tauri 2；不增加运行依赖。平台 Windows x64。本地安装和本地提交，不发布。

## 全局执行规则

- 从独立 worktree 基线 81144f4 开发，原目录所有未提交改动保持原样。
- 每任务使用聚焦失败证据，然后最小实现与验证；每任务独立提交，报告记录红/绿证据。已有失败测试可作为任务 1 的红证据。
- Task 1 -> Task 2。规格与质量评审通过后才能进入依赖任务；全计划最终再进行一次整体评审。
- Node 命令以工作区为 cwd；Rust 使用 `--manifest-path src-tauri/Cargo.toml`；不运行服务安装、GUI 输入或真实网络凭据测试。
- 使用绝对任务 brief/report/diff 路径给 agent，不复制整份计划历史。

### Task 1: 恢复公开参考包测试基线

**Files:** `examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js`、`examples/public-plugins/com.uipilot.translate/tests/runtime.test.js`。

**Dependencies:** 设计 `M0 设计合同 / 参考包断言`。

- [x] 重现两个现存失败，仅同步已提交 manifest 和包根的精确断言。
- [x] 保留 strict 包文件集合及权限/输出模式断言，不更改 Runtime 或 manifest。

**Distinct test coverage:** 当前 demo-panel 固定版本/最低宿主/5 个 Host Keys 全部正确；translate 严格包含 icon.png 且无意外资源。

**Verify:** `node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js examples/public-plugins/com.uipilot.translate/tests/runtime.test.js`。

### Task 2: 统一本地验证与开发入口

**Files:** 新增 `scripts/verify-local-foundation.mjs`、`scripts/verify-local-foundation.test.mjs`、`README.md`；修改根 `package.json`；如确认 Windows checkout 的 CRLF 导致严格 schema 检查误报，在 `.gitattributes` 为生成 schema 固定 LF（不改变 schema 语义或放宽比较）；同步 `examples/public-plugins/com.uipilot.notes/tests/sdk-contract.ts` 中已过期的 Host Key 联合断言，不改 production SDK。必要的 Windows npm runner 保持在新脚本内或同责任的小模块。

**Dependencies:** Task 1；设计 `M0 设计合同 / 验证协调器` 和 `文档与验证`。

- [x] 实现可测试的发现/阶段构造/顺序执行；导入模块不能触发检查。
- [x] 新增 verify/verify:local 入口与只读 --list；保留 npm test 原语义。
- [x] 串联完整九类门禁，失败准确传播，不补下载、不自动修生成物。
- [x] 根 README 提供真实命令、源码导航、本地插件流程和人工验收边界。

**Distinct test coverage:** 稳定排序；带空格路径以独立 argv 传递；入口 cwd 独立；缺目录、空包/测试/合同集合、符号链接均 fail-closed；首个阶段失败阻止后续；spawn error 清晰退出；--list 无进程启动；Schema 检查失败不自动更新文件。

**Verify:** `node --test scripts/verify-local-foundation.test.mjs`；`npm run verify:local -- --list`；`npm run verify:local`。

## 完成验收

- [x] 两个任务规格与质量评审通过。
- [x] verify:local 全流程真实通过，报告各阶段结果与保留 warning。
- [x] Git 仅含本计划任务文件；原工作区 dirty 文件未动。
- [x] 本计划只声明 M0 交付，不将后续阶段的路线图当作已实现能力；后续状态见各阶段计划。
