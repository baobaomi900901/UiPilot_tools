# 文本模板（离线 Panel Runtime 示例）

插件 `com.uipilot.templates` **1.2.0**，要求 Windows UiPilot **0.6.0+**。默认启动键 `/templates`，只声明 `ui.panel`。使用 demo-panel 包结构和 Notes 的紧凑侧栏布局，无网络、远程资源或依赖安装。

在设置 → 插件点击 **选择开发目录**，选择本例的 `package` 目录；或选择交付的 `.uipilot-plugin` 归档。授权后启用插件。开发目录不是父级示例目录。安装会复制资源，修改源码后需提高版本并重新安装更新。

## 使用

1. 输入 `/templates` 后按 Enter。初始不选中模板；点击 **新建模板**（或 Ctrl+N）。
2. 填写名称与正文，例如 `你好，{{name}}！`，点击 **预览**。Runtime 发现变量并报告缺失样例。
3. 填写 name 样例后再次预览，点击 **保存**。关闭重开后从左侧选择保存项；退出重启后也可读取。
4. 在 Host 主输入框输入模板名称关键词并按 Enter 筛选。上下方向键把焦点转入列表；列表内上下移动，Enter/Space 或点击才载入。方向移动不读写数据，不自动选中首项。筛选保留当前草稿，Ctrl+F 返回主输入框。Tab/Shift+Tab 按视觉顺序移动；Ctrl+S 保存。

预览是普通可选择的只读文本，可用系统复制快捷键。它不执行 HTML，也没有专用剪贴板接口。Escape、失焦和关闭仍由 Host 管理，未保存草稿不承诺跨关闭保留；切换模板或主动新建时，确认弹窗的 **取消**、右上角 **关闭** 和 Escape 都保留当前草稿；只有 **放弃并继续** 才执行切换或新建。1.2.0 增加就地字段错误和模板动作面板，保留原有草稿与会话保护。

详情工具栏的 **动作**（插件内容内 Ctrl+K）提供预览、保存、新建模板，以及保存待确认时的重新载入。它们使用与直接按钮相同的状态和请求保护。动作面板的 **取消**、右上角 **关闭** 和 Escape 不执行操作；上下/Home/End 移动焦点，Enter/Space 执行动作，Tab/Shift+Tab 由原生模态管理。动作面板先关闭，再打开可能需要的草稿确认。

Ctrl+K 仅在插件内容文档有效，Host 主输入框没有新增 Ctrl+K 声明；主输入 Enter 仍提交筛选。模态打开时，背景 Ctrl+N/S/F 和 Host 按键不会触发草稿操作。

保存前名称不能为空白且原文最多 80 个 Unicode 码点；预览不要求名称。正文最多 8192 UTF-8 字节，已提供样例最多 256 个码点；空正文和缺少样例仍可保存。字段下方显示错误，编辑清除该字段旧错误，不修改或截断用户值。Runtime 继续负责语法、变量数量和存储校验；较早回复不会给新编辑标记字段错误或移动焦点。

## 语法与数据

仅支持 `{{name}}`，名称匹配 `[A-Za-z][A-Za-z0-9_]{0,31}`，可有外围空白。变量按首次出现排序，重复引用只生成一个输入框。`constructor`、`prototype` 是普通合法名称。没有表达式、循环、宏、递归替换或 HTML 渲染。

未填写的输入没有样例条目，预览会报告缺值；主动填写后再清空会保留明确的空字符串。保存允许缺少样例，但不允许错误语法。已不被正文使用的合法样例会被丢弃。非法或重复样例拒绝。

Runtime 的 `onCommand` 只读取目录并返回列表元数据；按钮通过可选 `panel.request` 调用 `onPanelAction`：

- `load`: `{id}`。
- `preview`: `{body, values:[{name,value}]}`。
- `save`: `{id, expectedRevision, title, body, values}`；新建的 id/revision 均为 null，更新要求确切已存版本。

面板不直接读写 storage。Runtime 使用 `templates.catalog-v1`，一个完整目录快照只调用一次 `storage.set`。每个模板有稳定 ID 和单调版本；冲突不会覆盖。保存中继续输入时，成功回复只合并同一草稿的 ID/版本，之后的新编辑仍未保存，不会再次新建同一个模板。

最多 20 个模板，名称最多 80 个 Unicode 字符，正文最多 8 KiB UTF-8，最多 16 个不同变量，每个样例最多 256 个 Unicode 字符。预览最多 32 KiB UTF-8；目录紧凑 JSON 最多 512 KiB。完整回复另受 Host 64 KiB JSON 限制，超出返回业务错误，不截断文本。计数器耗尽拒绝，损坏目录显示错误，不重置数据。

## 保存结果待确认时

超时、取消、会话失效或存储错误可能发生在写入之后，**不能据此判断未保存**。不会自动重试，保存按钮保持阻止状态。请先复制需要保留的草稿文字，再明确检查：

1. 已有模板：点击 **重新载入已保存**，确认放弃当前编辑后读取该 ID 的实际内容。检查后再修改、保存。
2. 新草稿尚未获得 ID：点击 **重新载入已保存**，在 Host 主输入框**清空筛选并按 Enter**刷新列表。当前草稿保留。检查列表后主动选择已保存项并确认放弃草稿；如果确认没有保存，才主动点击 **新建模板** 并确认放弃旧草稿。普通主题/列表更新不会解除保存阻止状态；刷新列表本身也不会自动重试保存。
3. 面板已关闭：重新进入 `/templates`，清空筛选后检查保存项。需要继续时选择实际保存的模板；不要直接重建相同内容。

## 验证与人工验收

```powershell
node --test packages/plugin-ui/tests/*.test.mjs examples/public-plugins/com.uipilot.templates/tests/*.test.js
node packages/plugin-ui/scripts/sync-templates.mjs --check
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.templates/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.templates/package --platform windows
```

自动检查覆盖解析、字面替换、缺值/空值、重复样例、冲突、失败保存、损坏目录、容量与回复上限，不替代真实 Host 操作。人工仍需检查安装入口、首次预览/保存、重启持久化、错误恢复、快速编辑/关闭、键盘/双主题，以及旧 Notes/多命令插件。人工结果在交付说明中单独登记，未收到用户反馈前均为待测试。


## 可复用 UI 子集

保留 1.1.1 已有的 [packages/plugin-ui](../../../packages/plugin-ui/README.md) 的 List、Detail 状态与 ActionBar；
1.2.0 加入 Form 和 ActionPanel。源码为 packages/plugin-ui/src/ 下的 ui.js、ui.css、form.js、action-panel.js，四文件通过固定同步脚本复制到包内 dist/ui。
修改组件后运行同步，再执行只读 --check；不要直接编辑副本。插件归档自带相对 ES module
和 CSS，不需要宿主安装组件库。此交付是原生表单绑定和基础动作选择面板，不是完整动态表单渲染器，也不代表 M5 全部完成。

Host Enter 保持提交/筛选与保存不确定时的刷新；Manifest 按键声明不变，仍用默认 content 焦点策略。
模板数据、Runtime、权限与最低 Host 版本不变。本轮实际环境是 Host 0.13.0。
自动测试增加实例隔离、稳定节点/焦点、忙碌表单、重复点击、草稿取消、保存期间编辑、
新会话旧回复及排队旧确认事件，以及 Unicode/UTF-8 边界、样例缺值/空值、过期字段错误、动作最新状态与两模态关闭顺序。真实原生按键、双主题和窄窗口仍待用户人工验收。