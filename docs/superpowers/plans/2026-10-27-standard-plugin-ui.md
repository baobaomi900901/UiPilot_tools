# 标准插件 UI 与文本模板集成实施计划

目标：交付无依赖可复用 List/Detail/ActionBar 模块，并用于文本模板1.1.0。设计合同：[标准插件 UI](../specs/2026-10-27-standard-plugin-ui-design.md)。执行使用 subagent-driven-development，单个写代码 agent；核心模块和唯一消费者属于同一可人工验收单元。

## 技术与全局约束

原生 ES modules、CSS、Node test、已有 JSDOM。Host0.13.0保持，插件minimumHostVersion0.6.0，ui.panel权限与业务存储不变。src/、src-tauri/、packages/plugin-cli/ 和 SDK 合同只读；不安装依赖、不操作真实程序、不重建宿主。仅packages/plugin-ui、templates包/测试/文档以及本交付文档在范围内。全局状态/身份和接口精确定义见设计的组件合同、文本模板集成、分发与验证。

## 执行规则

先写针对行为的失败检查，再最小实现、聚焦验证和本地原子提交。独立检查 Spec 与 Quality 后才打包交付；发现问题集中修复并复查。测试、编译和目录/归档校验只运行与本次变化有关的检查，真实交互由用户完成。设计、源码、交付与人工状态分别记录。

### Task 1: 可复用 UI 模块及文本模板1.1.0

**Files:** 新建 packages/plugin-ui/{README.md,src/ui.js,src/ui.css,scripts/sync-templates.mjs,tests/ui.test.mjs}；修改 examples/public-plugins/com.uipilot.templates/{README.md,package/plugin.json,package/dist/panel.html,package/dist/panel.css,package/dist/panel.js}；新建该插件 package/dist/ui/{ui.js,ui.css} 与 tests/panel.test.js。runtime.js只读，现有runtime测试保留。

**Dependencies:** 设计“组件合同”“文本模板集成”“分发与验证”。

- [x] 实现三个无权限的DOM组件与限定CSS；独立实例互不影响，稳定ID复用节点、范围内键盘、禁用与dispose正确。
- [x] 接入模板既有界面与request保护，保留字段ID和业务数据/交互；新增Host方向键，模态打开时不处理背景快捷键。
- [x] 加入固定副本同步/只读校验，测试验证副本一致；更新版本、插件开发与使用说明，明确这是组件首个子集。

**Distinct test coverage:** 焦点项重排保留节点，移除只在列表内恢复邻近项，空列表与disabled无动作；输入文字按文本展示；多个实例/销毁后无旧事件；busy详情保留表单内容；重复click与异步request只有一个调用；列表方向键不加载、Enter调用正常选择；未保存切换取消不改草稿和预览；输入期间保存回复仍标记新编辑未保存；新session先到、旧request后完成不会覆盖；模态中Host Key不影响背景、旧待选动作失效；源/副本一致性失败有明确非零结果。

**Verify:** `node --test packages/plugin-ui/tests/ui.test.mjs examples/public-plugins/com.uipilot.templates/tests/*.test.js`；`node packages/plugin-ui/scripts/sync-templates.mjs --check`；`node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.templates/tests/sdk-contract.ts`；既有CLI `validate examples/public-plugins/com.uipilot.templates/package --platform windows`。

## 交付检查

独立合并审查本切片完整diff与测试报告。打包至独立的 .superpowers/deliveries/templates-1.1.0-acceptance/plugins 目录并验证归档；记录源码提交、插件hash、Host0.13.0不变的hash、人工待测。无需宿主/前端构建。第一组人工用例按设计下发，不标记M5整体完成，不重新运行既有通过矩阵。


实现与聚焦验证、独立审查及本地打包已完成，最终源码3350d0d。人工验收待用户执行，详见交付目录manual-cases.zh-CN.md；该状态不代表M5全部完成。
