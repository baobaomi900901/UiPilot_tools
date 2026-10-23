# 本地插件基座 M1 打包实施计划

目标：提供可复现、安全且不覆盖现有文件的通用本地打包命令。架构：现有目录快照与验证器 -> 确定性 ZIP writer -> 同目录无覆盖发布 -> CLI。规范以 [打包设计](../specs/2026-10-23-local-plugin-pack-design.md) 的精确接口、数据流和验收为准。

技术：Node 20+ 内置 API、TypeScript、Vitest、现有 CRC32/包验证模块；不增加依赖、不修改宿主版本、插件 API 或 schema。

## 全局规则

M0 全仓门禁完成后执行本计划。Task 1 -> Task 2；每项聚焦 TDD、独立提交、规格与质量评审后进入下一项。所有代码位于隔离 worktree；不改原目录。每项报告保存红/绿结果。无网络发布、无真实安装、无真实凭据。

### Task 1: 确定性归档与无覆盖输出

**Files:** 新建 `packages/plugin-cli/src/archive-writer.ts`、`packages/plugin-cli/src/pack.ts`、`packages/plugin-cli/tests/archive-writer.test.ts`、`packages/plugin-cli/tests/pack.test.ts`；扩展 `packages/plugin-cli/src/contracts.ts` 的 pack 类型。

**Dependencies:** M0；设计 `数据流`、`ZIP 格式` 和 `目标与接口`。

- [ ] 实现纯 snapshot -> ZIP writer，复用 crc32 与包上限。
- [ ] 同一快照校验、构造、归档复验，不重新读取源内容。
- [ ] 输出普通父目录校验、源目录外约束、独占临时文件及原子无覆盖发布。
- [ ] 结构化错误与确定性 SHA-256，未知错误留给 CLI_INTERNAL。

**Distinct test coverage:** 字节/mtime/源路径无关、目录与 WAV/icon 资源归档往返、归档含头超限；坏包无写入、现有目的路径不覆盖、目的路径被抢占、源内输出拒绝、链接拒绝、写入失败只清理自身临时文件。文件系统测试用临时目录，外部 Windows 句柄可注入最小发布故障而不改生产策略。

**Verify:** `npm test --workspace=@uipilot/plugin-cli -- archive-writer.test.ts pack.test.ts`；`npm run check --workspace=@uipilot/plugin-cli`。

### Task 2: CLI 接线、分发 smoke 与本地教程

**Files:** `packages/plugin-cli/src/cli.ts`、`packages/plugin-cli/src/bin.ts`、`packages/plugin-cli/tests/cli.test.ts`、`packages/plugin-cli/scripts/smoke.mjs`、`packages/plugin-cli/README.md`、根 `README.md`、`docs/plugin-sdk/public-plugin-developer-guide.md`。

**Dependencies:** Task 1；设计 `目标与接口`、`模块与兼容`、`验收`。

- [ ] pack 参数、help、JSON/普通输出与退出码接线；validate 原调用兼容。
- [ ] 独立 npm consumer smoke 实际打包并验证，使用临时示例，保持完全离线。
- [ ] 文档给出通用本地打包/校验/安装路径，注明无覆盖与输出目录要求；不宣传在线发布。

**Distinct test coverage:** 输出参数缺失/重复、未知参数、不同平台默认、JSON成功/预期失败/未知异常、validate 回归；打包后的 CLI 无仓库依赖完成 pack 与 validate。

**Verify:** `npm run verify --workspace=@uipilot/plugin-cli`；`npm run verify:local`。

## 总验收

- [ ] 两任务独立评审通过。
- [ ] 全仓验证通过；真实 Host 本地安装人工检查单独列出，不伪报自动完成。
- [ ] 原工作区未提交改动保持原样。
