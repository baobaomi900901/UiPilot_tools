# 标准表单与动作面板实施计划

目标：实现可复用Form校验绑定和ActionPanel，并交付文本模板1.2.0。架构及精确状态/接口以[设计](../specs/2026-10-27-plugin-form-action-panel-design.md)为准。用户已授权自主安排agent开发；使用subagent-driven-development单写入者完成这一项纵向交付。

## 技术与约束

原生ES modules/CSS、Node test、已有JSDOM。Host0.13不变，templates1.2.0/minimumHostVersion0.6.0/ui.panel和存储格式不变。模板runtime.js、Host、CLI、SDK只读。无依赖安装、真实UI、联网或新系统能力。四个明确UI资源随包同步；控件值和请求由插件持有，组件无权限。

## 执行规则

先以针对性失败检查明确行为，再实现最小合同并验证、原子提交。独立Spec/Quality审查本切片完整diff，修复后仅复查差异。测试/类型/目录及归档检查只覆盖本次变更，不机械全仓构建。真实按键、焦点和主题由用户人工验收，未反馈前保持待测。

### Task 1: Form、ActionPanel与文本模板1.2集成

**Files:** 新增packages/plugin-ui/src/{form.js,action-panel.js}、tests/{form.test.mjs,action-panel.test.mjs}；修改src/ui.js、src/ui.css、scripts/sync-templates.mjs、README.md。修改examples/public-plugins/com.uipilot.templates/{README.md,package/plugin.json,package/dist/panel.html,package/dist/panel.css,package/dist/panel.js,tests/panel.test.js}及显式跟踪package/dist/ui四个副本；必要时增加tests/form-actions.test.js以隔离本轮实际模块场景。Runtime只读。

**Dependencies:** 设计“Form合同”“模板校验与异步归属”“ActionPanel合同”“模板动作面板集成”。

- [ ] 实现Form原生控件绑定、字段错误/描述与子集校验、增删与dispose；消费者只增加合同中轻量本地规则，并映射当前回复的字段错误。
- [ ] 实现原生dialog ActionPanel和键盘/最新状态/单次调用/关闭顺序，接入模板统一状态及函数；旧业务、已有模态和会话保护保持。
- [ ] 四文件同步与漂移检查、版本/文档/使用示例，保留已验收List/Detail/ActionBar。

**Distinct test coverage:** 两实例不相互影响；checkbox/select/text/textarea绑定不改原值；aria-describedby保留原token并移除自身token；局部validate不错误要求标题、空正文/缺少样例可保存，显式空样例和未填写不同；Unicode80/256、UTF8 8192边界；旧错误reply在新编辑后不覆盖字段错误或焦点，保存reply仍正确合并identity；动作hidden/disabled最新状态及重复click、cancel/close/Esc不调用、close先于业务确认、旧close晚到不影响新modal；新session关闭旧面板不派发/夺焦点；busy/unknown-save不能通过动作入口绕过；Host Enter保持，内容快捷键不能穿透模态。

**Verify:** `node --test packages/plugin-ui/tests/*.test.mjs examples/public-plugins/com.uipilot.templates/tests/*.test.js`；`node packages/plugin-ui/scripts/sync-templates.mjs --check`；`node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.templates/tests/sdk-contract.ts`；`node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.templates/package --platform windows`。

## 最终交付

独立审查和必要修复完成后，打包至.superpowers/deliveries/templates-1.2.0-acceptance/plugins，验证归档及Host/旧包hash不变，保存证据和精简人工用例。无Host/前端重建；不按测试数量或单次交付宣称整个M5完成。
