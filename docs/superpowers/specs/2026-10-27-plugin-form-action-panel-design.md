# 标准插件 Form 与 ActionPanel：文本模板1.2.0

## 目标、选择与授权

用户在上一轮标准List/Detail/ActionBar人工验收完成及下一步表单/动作面板说明后要求继续，沿用自主规划/agent开发授权。本轮新增纯DOM表单错误呈现和可键盘操作的动作面板，实际用于文本模板。复用现有模块与真实业务，比新增演示插件更容易验证旧数据；比引入React/JSON表单框架更小且不改变Host合同。唯一消费者版本1.2.0，最低Host仍0.6.0，实际验收Host0.13.0不变。

这是可复用表单绑定/校验及基础动作选择面板，非完整动态表单渲染器。无网络/发布/安装依赖/新权限。src/、src-tauri/、packages/plugin-cli/、SDK合同和模板runtime.js只读。旧List/Detail/ActionBar行为保持。

## 文件与模块边界

新增packages/plugin-ui/src/form.js、action-panel.js，由ui.js重新导出，ui.css增加前缀样式。固定同步脚本同步ui.js、ui.css、form.js、action-panel.js四个文件到templates/package/dist/ui；运行时仅使用包内相对ES imports。README给出明确API及调用示例，--check保持只读与差异非零。模板tests中的一致性/临时漂移夹具同步为四文件。

所有组件不持有Host、Runtime、网络或存储权限，不读写业务值或自动提交；消费者仍拥有状态、校验规则与请求。文本使用textContent，实例隔离，dispose清除监听/过期状态。只使用公开主题变量与既有DOM能力，不注册全局跨插件快捷键。

## Form 合同

导出createForm({fields})，fields为有序数组{id, control, error, validate?}。id稳定唯一；control是调用者提供的原生input/select/textarea，error是有唯一id的错误文本元素，validate(control)返回错误字符串或null。控件类型及值属于调用者，不强制修改value/checked，不做类型转换或截断。

返回setFields(fields)、validate(ids?)、setErrors(errors)、clear(ids?)、dispose()。validate只校验指定字段或全部字段，逐字段清除/写入本次结果，返回是否全部有效；显式用户动作触发时可在当前有焦点的文档内聚焦第一个错误控件，不能跨WebView夺焦点。setErrors以Map<id,message>仅更新给出的字段，不自动聚焦；clear只清除组件拥有的错误。未知id不创建控件。

错误显示为字段下方短文本，设置aria-invalid，错误id加入aria-describedby且保留原有提示tokens；清除/移除/dispose时清除自己的关联，不删除调用者原有描述。input/change编辑清除该字段旧错误，不触发请求或自动校验。setFields保留相同id/控件的状态，移除旧字段监听/错误关联；动态变量移除后不得留下重复listener或悬空aria引用。dispose后方法/事件不再改变状态。必须验证独立两实例，以及文本/textarea/checkbox/select原生控件不被组件改值。

## 模板校验与异步归属

表单覆盖模板名称、正文和动态样例输入，保留所有既有id/编辑模型。保存前检查名称trim后非空且原字符串最多80个Unicode码点；预览不要求名称。预览/保存均检查正文最多8192 UTF-8字节，以及每个已提供样例最多256个Unicode码点。空正文可保存。不要用HTML maxlength替代码点检查，不trim/规范化用户值。

缺少样例与明确空字符串继续由Map是否有该键区分；绑定表单不能把未填写样例写入Map。保存仍允许缺少样例，预览通过Runtime发现变量/缺失。Runtime仍负责完整语法、变量数量、存储冲突与容量校验，前端不复制解析器。当前仍显示的旧变量样例也按传入payload校验；找不到字段的样例错误用样例区域/状态栏兜底。

Runtime返回invalid-title对应名称，body-limit/invalid-syntax/variable-limit对应正文；missing-variable在成功更新动态变量DOM后标记对应样例。其他错误继续现有状态栏；invalid-values可在样例区域说明。字段错误仅能应用在current(owner,true)（同session/draft/editRevision）时；较早回复不能给新输入标错或夺焦点。成功保存合并id/revision和处理保存不确定仍保留现有较弱identity归属，不得因增加字段校验破坏保存中继续编辑的流程。

新建/载入/新session清理旧表单错误，主题或筛选更新不得清理用户正在处理的错误/草稿。预览与保存、主按钮与Ctrl+S和动作面板必须走同一函数与单次请求保护。不能在校验失败时发出请求，不能绕过recovery禁止保存；已有恢复流程保持。

## ActionPanel 合同

导出createActionPanel({dialog, trigger, list, closeButton, cancelButton, actions})。原生dialog由调用者提供标题和aria-labelledby，actions是有序{id,label,onInvoke}数组。组件生成普通type=button动作行，label按文本呈现。返回open()、close({restoreFocus=true}={})、update(state)、dispose()；state按id提供disabled/hidden，使用Map或安全自身属性，不继承原型键。

open在未销毁、trigger可用、没有其他打开dialog时显示模态；重复open无副作用。初始聚焦首个可用动作，无可用动作则聚焦取消。按钮与上下/Home/End导航使用同一可用集合，边界不循环；原生Enter/Space激活当前按钮，Tab/Shift+Tab由native dialog保持模态焦点。IME/修饰方向键不拦截。更新时保持节点/焦点；被隐藏/禁用的焦点项只能在本模态内移至可用项或取消，不能逃到背景。

取消、右上角×及native cancel(Escape)均关闭且无业务调用，取消后在同一仍有焦点文档且无其他模态时恢复调用前的有效元素，必要时回到trigger。选择动作时检查最新可用状态，先同步close({restoreFocus:false})再调用一次onInvoke；这样“新建”/“重新载入”能随后打开既有草稿确认，不同时出现两个模态。不从排队的close事件分发动作或恢复旧焦点。关闭后重复旧click、晚到close事件不得再次执行或关闭后来的模态。dispose/新session关闭不得执行动作或显式恢复旧session焦点。

## 模板动作面板集成

保留既有直接按钮；详情工具栏新增“动作”按钮，标题为“模板动作”，包含预览、保存、新建模板、重新载入已保存；重新载入仅recovery存在时出现。它们与原按钮共享同一次计算的可用状态，调用同一preview/save/newDraft/reload。无草稿预览/保存禁用；busy/不可用时禁用触发器及所有动作；保存不确定时保存禁用，其他状态遵守原业务保护。ActionPanel是可选操作入口，不替换原输入或列表。

Ctrl+K只在插件内容文档内处理（不带Alt/Shift、非IME、非repeat且没有其他模态）；不增加Host key声明，不宣称主输入Ctrl+K能打开插件动作。Host输入Enter继续筛选。ActionPanel打开时阻止现有Host key和document的N/S/F等背景业务快捷键；方向键只作用于模态动作。新session先关闭动作面板且放弃旧表单状态，再应用新状态；旧请求仍按现有owner丢弃。

取消/×/Esc必须显式可见且可访问；ActionPanel与旧确认框均遵守已验收的对话框模式，取消为无副作用。CSS遵守UI指南，前缀类、语义变量、两种主题、窄视口滚动与长文案、focus-visible/disabled/错误可区分，无装饰性阴影。

## 验证与人工节点

单项纵向交付：聚焦Form/ActionPanel独立实例、控制值保持/描述tokens/子集校验、动作最新状态/重复点击/关闭顺序/模态焦点/销毁；模板实际模块验证名称/字节/样例边界、空正文与缺失/空样例差异、延迟错误遇到新编辑、保存中编辑、unknown-save恢复、两模态与新session顺序。复用既有Node/JSDOM和Runtime回归，不操作真实程序；JSDOM只能证明模拟行为。

独立Spec/Quality通过后，用既有CLI打包templates1.2.0与校验归档；记录源码/包hash、输入稳定、旧包和Host0.13未变。不重建宿主。第一组人工：升级旧数据保留；名称为空保存产生就地错误且修正后保存；变量预览缺值就地提示，填写后从动作面板预览/保存。之后单独检查取消/X/Esc、ActionPanel新建触发草稿确认、内容Ctrl+K/上下/Enter与双主题，保存重启和已有列表短回归。
