# 标准插件 UI 第一项交付：文本模板的列表、详情与动作栏

## 目标与授权

沿用本地插件完整基座路线图 M5 及用户自主开发授权。用户在整体进度中获知下一项是标准列表／详情与动作组件后明确要求继续。本轮做一个可复用模块并在已验收的文本模板中实际使用；人工验收由用户执行。原生 Host 保持 0.13.0，不改权限合同，不开新系统能力。

## 方案与边界

采用无依赖 ES module + CSS，源码放在 packages/plugin-ui，明确同步到插件包的 dist/ui。相对 import 在已有 CSP 与离线归档中运行，不在运行时引用仓库或下载资源。相比仅改模板 CSS，此方案有可复用行为接口和同步校验；相比完整 React 组件库，此方案无需新工具链和宿主 API，反馈更早。

这是 List、Detail 状态与 ActionBar 的首个可用子集，不宣称完整 Form/ActionPanel 或 M5 完成。保留 templates 的业务字段、Runtime、存储格式、命令身份和权限。其他插件不迁移。没有安装依赖、发布、网络访问或宿主重建。

## 组件合同

模块不接触 window.uipilotPlugin*、storage、网络或系统接口。消费者持有业务状态，模块只处理 DOM、可访问语义、键盘和呈现。文本用 textContent，调用者提供的文案不会当 HTML 执行。每个实例只监听自己拥有的元素，dispose 后不再处理事件，不抢宿主主输入焦点。

- createList(container, { onActivate })：update({ items, selectedId, disabled, emptyText })，items 为稳定唯一字符串 id 与 title；move(delta) 在可用项之间移动焦点而不执行动作。列表使用原生 button 和 aria-current，鼠标与 Enter/Space 进入同一个 onActivate(id)；列表内无修饰上下键移动，IME/修饰键不拦截。无默认业务选择；初次向下从首项、向上从末项开始，端点不循环。当前焦点项继续存在时更新保持同一 DOM 节点与焦点；移除时只在原本焦点确实在列表内才把焦点放到邻近可用项。空列表不制造可执行条目，不把焦点跳回已移除的节点。disabled 时按钮不可激活、move 不移动；重新可用时保持逻辑位置，绝不自动执行。
- createDetail({ empty, content, label })：update({ hasContent, busy, text }) 只控制空态/内容显隐、状态文字和 aria-busy；忙碌时已有内容保持可见，不重建输入、不覆盖草稿、不移动焦点。没有内容时保留现有领域空态。
- createActions(entries)：entries 为 id -> { button, onInvoke }；update({ [id]: { disabled } }) 同步原生按钮状态。实际 click 每次检查当前禁用状态，调用对应回调一次；业务异步、重复提交和取消仍由插件已有 request/owner 逻辑负责。不新增全局快捷键或任意命令分发。
- CSS 使用 uipilot-ui- 前缀类和公开 --uipilot-color-* / --uipilot-font-family；遵守 docs/ui-guidelines.md 的 220px 侧栏、48px 工具栏、滚动条留白、紧凑字号、主题与焦点规则。保留兼容回退、强制颜色与长文本截断。布局填满 Panel；低宽度允许合理压缩侧栏而不遮盖详情按钮。

API 可在实现时增加必要的私有辅助函数，不能扩展为业务框架或另加公开能力。至少一个测试在两个独立实例中使用同一模块并验证状态/事件互不干扰。

## 文本模板集成

版本 1.1.0，minimumHostVersion 仍 0.6.0；Host0.13.0 作为本轮实际验收环境。保留 pluginId、/templates、ui.panel、templates.catalog-v1、runtimeActions 与所有数据限制。panel.hostKeys 增加 ArrowDown/ArrowUp，仍使用默认 content 焦点路由；不声明 Enter，主输入 Enter 继续筛选。已有 Tab/Shift+Tab/Primary+N 保留。

主输入空白占位符改为领域文案“搜索模板名称”，操作方法在帮助/说明中描述。打开时没有默认选中项。主输入上下键转入列表，内容列表上下移动、Enter/Space 载入；方向移动不触发 Runtime 读写。未保存状态下实际载入另一项继续走已有放弃确认，取消保留草稿、选中身份与预览。筛选隐藏当前项不丢草稿；列表选中以 draft.id 为准，不把焦点当成已载入。

所有编辑、预览、保存、保存结果不确定、恢复、session/draft/editRevision 归属保持。列表和动作 disabled 反映 activeRequest，不因新组件绕过保护。主题更新、业务处理前后的重新呈现不重建当前编辑控件或造成无理由焦点跳动。会话替换时新组件只反映新状态；旧回调不能写回新会话。模态打开时 Host Key 不得把焦点移到背景或触发新动作；取消不得执行已废弃的待选动作。仅修正接入组件所触及的这类状态衔接，不扩展确认框功能。

## 分发与验证

packages/plugin-ui/src/ui.js 与 ui.css 是唯一源码，scripts/sync-templates.mjs 只同步这两个明确文件到 templates/package/dist/ui；--check 只读比较，差异非零，不自动修复。目标固定在仓库内，不接受外部路径。仓库普通验证发现的 templates tests 中包含副本一致性检查；组件测试显式运行。开发文档说明其他插件把这两个文件随自己的包分发，宿主无需安装 UI 库。

重点验证：实例隔离、注入字面显示、选择/焦点保持、列表键盘/禁用、动作不重复、空态/忙碌状态不丢输入；模板集成的首屏、Host Key、草稿取消、异步编辑/保存、新会话旧回复、数据回归和副本一致性。复用 Node test 与已安装 JSDOM；真实布局、原生按键和主题由人工验收，不用 JSDOM 通过推断。保留已通过项目，不重复全仓门禁。

验收交付只打包 templates1.1.0，现有 CLI 目录/归档校验；核对 Host0.13.0 hash 不变。第一批人工用例：升级保留旧模板；新建/预览/保存；列表方向键/Enter 与未保存取消。其后再做双主题、筛选无结果、长名称/窄窗口和重启保存。新的 native 交互均待用户反馈。
