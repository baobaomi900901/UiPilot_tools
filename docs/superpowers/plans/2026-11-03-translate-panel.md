# 翻译插件 Panel 开发计划

日期：2026-11-03。状态：用户已批准，实施中；用户已明确授权宿主、SDK 与插件整套迁移，并选择隔离工作树。宿主精确合同见 [宿主扩展实施合同](../specs/2026-11-03-translate-panel-host-design.md)。

**目标：** 将翻译插件迁移为启动器内嵌 Panel，提供原文编辑、双向语言选择、自动检测、译文展示、原文/译文复制和朗读，以及可扩展的翻译服务选择；首版只接入有道。

**架构：** 插件内容页维护编辑和显示状态，既有 Panel request 桥接调用 Runtime，Runtime 通过宿主管理的有道服务翻译。系统复制、朗读与外部选区属于宿主能力。先交付 [宿主能力请求](../specs/2026-11-03-translate-panel-host-capability.md) 的 H1–H4，再完成插件真实接入；不得在插件开发任务中修改宿主或 SDK。

**技术：** 沿用包内原生 HTML/CSS/ES modules、Node.js 20+ 的 node:test、仓库已有 jsdom、TypeScript SDK 类型检查和本地 plugin-cli。采用 `demo-panel` 的基础桥接与 `templates` 的 Panel Runtime/测试模式，视觉遵守 [UI 规范](../../ui-guidelines.md)。

## 已确认需求

- 插件形态改为 panel。
- 上方可编辑原文框；中间三个控件依次为源语言、交换方向、目标语言；下方译文框。
- 两个文本区域都有朗读和复制按钮。
- 源语言支持自动检测；点击自动检测打开语言选择面板，可选自动检测或指定语言；显示实际识别语言。
- 目标语言可选择；译文区顶部提供服务选择入口，目前只有有道，后续可接入其他翻译 API。
- 用户参考图表达布局和交互，主题、字体、尺寸仍使用 UiPilot 规范。

## 建议默认交互（实施基线，用户可调整）

1. `/translate` 无参数也可进入空 Panel；有参数或划词输入时填入原文并发起一次翻译。宿主顶部输入保留，提交时替换原文；内容原文框是多行业务编辑器，不重复制作搜索栏或底栏。
2. 原文编辑结束后防抖 500 ms 自动翻译；IME 组词期间不请求；Ctrl+Enter 可立即提交，普通 Enter 在原文框内换行。语言或服务改变后触发新翻译；失败只在用户重新提交或更改输入后重试。
3. 默认源语言为自动检测、目标语言为简体中文、服务为有道；仅持久化这三项偏好，不自动保存原文、译文或历史。首版至少支持英语和简体中文，菜单按宿主实际支持列表生成，不假定所有有道语言已开放。
4. 源语言选择面板包含自动检测与明确语言；目标语言面板仅含明确语言。支持搜索、方向键、Enter 选中、Escape 关闭及焦点恢复。点击识别语言提示同样打开源语言选择面板。检测失败显示“未识别”，不伪造识别结果。
5. 交换使用明确源语言；自动检测时使用当前文本已识别语言。交换两侧语言，并以本次有效译文作为新原文，发起反向翻译；没有有效译文或无法确定源语言时禁用并解释原因。空文本不请求；指定同语种直接展示原文。
6. 译文只读、可选中，保留换行。编辑原文或改变方向后清除旧译文及旧识别结果，禁用旧译文的复制、朗读与交换，避免把旧结果当新结果。
7. 服务选择器现在只有“有道翻译”一个可用项；不放可点击的假服务，不把百度 OCR 等非翻译功能当翻译提供商。选择器与 Runtime 通过稳定 provider ID 分派。
8. 点击朗读开始，再次点击停止；原文与译文互斥播放。编辑相关文本、变更语言/服务或离开会话时停止。无对应声音时保留按钮并解释不可用原因。复制保留 Panel 与焦点，结果通过宿主 setStatus 反馈。

## 约束与能力前置

- 当前插件为 Windows `1.2.1`、最低 Host `0.16.0`；当前仓库 Host 为 `0.17.0`，不据此假定已支持新增能力。新插件版本需高于 1.2.1，最低 Host 使用主程序实际发布版本。
- 保留 `com.uipilot.translate`、`translate` 默认启动键与现有用户绑定；改为 legacy command、submit、panel，启用 panel.runtimeActions；不启用 runtime.resultActions。
- 继续由宿主保管有道密钥；内容与 Runtime 不使用浏览器直连、凭据读取、任意 Tauri invoke、speechSynthesis 或剪贴板历史绕过。
- 最低可用能力：H1（Panel 翻译、检测、语言清单）、H2（任意文本复制）、H3（朗读）、H4（划词兼容与 SDK 交付）。未完成时不得把翻译功能切到不可用的 Panel 版本。
- 插件实施代理只修改 `examples/public-plugins/com.uipilot.translate/`、插件自身测试/文档及计划记录；宿主实施代理按用户本轮明确授权可修改 `src/`、`src-tauri/`、`packages/plugin-cli/` 与 SDK 合同，范围限 H1–H4。
- 公开 CSS token、深浅主题、紧凑布局、语义化标签、可见焦点、强制颜色模式必须覆盖；UI 不复制宿主顶部和底部边框，不另绘 Ctrl+K 动作菜单。

## 插件内部职责与异步规则

建议新增文件位于 `examples/public-plugins/com.uipilot.translate/package/dist/`：

| 文件 | 职责 |
| --- | --- |
| panel.html / panel.css | 两个文本区、语言栏、服务入口和选择面板；公开主题变量 |
| panel.js | DOM 绑定、Host onUpdate/onActions、键盘、状态反馈与卸载 |
| panel-model.js | 纯状态、输入修订号、翻译调度、迟到结果处理 |
| languages.js | 语言标签、菜单过滤与方向合法性；只映射已发布能力 |
| providers.js | provider ID 注册和 Runtime 有道适配，不包含密钥或 HTTP 签名 |
| panel-capabilities.js | 仅封装正式 SDK 的复制/语音/能力可用性，便于注入测试替身 |
| runtime.js（修改） | onCommand 初始化、onPanelAction 翻译分派及安全错误映射 |

内部翻译动作统一为 `request({ action: 'translate', payload: { providerId: 'youdao', text, sourceLanguage, targetLanguage } })`。sourceLanguage 使用插件内部 `auto` 或正式 SDK 语言 ID，Runtime 在边界适配；`auto` 不表示现有 Host 已接受该参数。

业务响应使用 `{ ok: true, text, detectedSourceLanguage, targetLanguage }` 或 `{ ok: false, code, message }`，外层仍是 SDK `{ requestId, data }`。检测语言未知为 null；业务错误码由插件统一映射，不能透传任意异常字符串。实际宿主签名以 H1–H4 正式合同为准。

状态包含原文、源/目标语言、provider ID、识别语言、译文、idle/loading/success/error、朗读状态，以及 sessionEpoch + revision。每次编辑、切换语言/服务或 Host 更新都使旧 revision 失效。一个 Panel 同时最多一个未完成 request；请求中仅保存最新待翻译快照，旧请求结束后发送最新且未过防抖限制的快照。不并发调用、不假定取消可回滚或节省已发生的服务费用。

onUpdate/onHostKey 内不等待 request；回调返回、当前会话就绪后再调度。关闭面板后丢弃所有定时器、待处理快照及旧回调；隐藏导致会话销毁，不承诺保留正文。

## 全局执行规则

- 每项实施任务统一遵循 TDD：先写针对性失败测试并核实原因，再实现、运行专项验证；每项一个原子提交，不夹带已有修改。本次实施包含用户明确授权的宿主前置能力；宿主与插件按任务分开提交。
- 顺序：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6。宿主前置能力未交付时，可在独立草稿中验证模型和 UI，但不把模拟结果当集成通过。
- 测试从仓库根目录执行；node:test 使用 `node --test`，SDK 专项使用本地 TypeScript CLI，包校验使用本地 CLI 构建产物。每项结束核对需求与代码质量，再进入依赖任务。
- 自动化阶段使用假服务、假语音与假剪贴板，不安装插件、不打开权限、不使用真实 API 额度。真实系统交互由用户主动验收或在获得该轮明确授权后执行。

### Task 1: 交接宿主能力并锁定正式合同

**Files:** `docs/superpowers/specs/2026-11-03-translate-panel-host-capability.md`；本计划的能力前置及内部适配说明。

- [ ] 将 H1–H4 交给单独承担主程序开发的代理，记录正式设计、SDK 版本、Host 最低版本与验收证据。
- [ ] 对照能力请求逐项确认检测语言、复制限额、语音状态/停止、会话撤销和划词输入均有合同，更新本计划中的版本依赖；不在插件任务补丁中修改宿主。

**Distinct test coverage:** 由宿主任务提交能力请求“验收检查”的授权、事件顺序、兼容与失败证据；缺任一必需能力则保持本任务未完成。

**Verify:** `git diff --check`；该命令只验证文档格式，功能交付必须另查主程序验收报告，不能以命令通过替代。

### Task 2: 建立翻译状态与语言/服务模型

**Files:** 新建 `package/dist/panel-model.js`、`languages.js`、`tests/panel-model.test.js`、`tests/languages.test.js`（均在翻译插件目录下）。

**Dependencies:** Task 1；本计划“建议默认交互”“插件内部职责与异步规则”。

- [ ] 实现源语言自动/指定、目标语言、识别语言、方向交换和 provider ID 状态；菜单数据来自已确认清单。
- [ ] 实现防抖、单请求调度、仅保留最新待处理输入、失效响应丢弃、空文本与同语种处理。
- [ ] 对保存的偏好做白名单校验；未知 provider/language 回到明确默认值，正文不入存储。

**Distinct test coverage:** 自动检测不是汉字正则猜测；交换自动源时使用当前检测结果，检测未知/译文过期禁用；A 请求中连续编辑 B、C，A 返回不显示且只执行 C；会话替换后旧请求不能改变新状态；IME 不请求；8192 字节边界按 UTF-8 而非字符数验证。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/panel-model.test.js examples/public-plugins/com.uipilot.translate/tests/languages.test.js`

### Task 3: 接入 Panel Runtime 翻译适配

**Files:** 修改 `package/dist/runtime.js`、`tests/runtime.test.js`、`tests/sdk-contract.ts`；新建 `package/dist/providers.js`。

**Dependencies:** Task 2；H1 正式 SDK；本计划内部翻译动作与响应。

- [ ] onCommand 仅返回初始化输入与合法偏好；onPanelAction 严格验证 action/payload，通过 provider ID 调用有道服务。
- [ ] 保留已有授权、密钥未配置/停用、额度、超时等安全错误映射；增加正式合同要求的检测/语言错误。
- [ ] 只注册有道；拒绝未知 provider/action、非法语言和额外字段，不暴露通用 URL 或网络执行入口。

**Distinct test coverage:** 带参数/无参数初始化不提前联网；合法自动/指定方向使用正确 SDK 参数；未知服务不调用任何 API；缺能力、权限关闭、配置关闭、额度耗尽、响应异常得到固定文案；保留前后空白与换行；错误名 constructor/__proto__ 不误取继承字段。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/runtime.test.js`；`node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict --target ES2021 --module ESNext --moduleResolution Bundler examples/public-plugins/com.uipilot.translate/tests/sdk-contract.ts`

### Task 4: 实现两框布局与语言选择面板

**Files:** 新建 `package/dist/panel.html`、`panel.css`、`panel.js`、`tests/panel.test.js`。

**Dependencies:** Task 3；本计划“已确认需求”“建议默认交互”；`docs/ui-guidelines.md` 第 3、5.3、5.4、6、7、9 节及 Panel 外层分割线归属。

- [ ] 实现可编辑原文、语言三控件、服务选择、可选择只读译文和两组复制/朗读按钮；空/loading/error 与长文本保持稳定布局。
- [ ] 语言选择面板支持自动检测入口、搜索、选择、高亮和焦点恢复；目标列表不含自动检测。图标有可访问名称，使用既有 Lucide 方案。
- [ ] Host update 填入本次输入并更新主题；Ctrl+K 用 onActions 注册当前可用操作，选择弹层拥有焦点时返回 null。
- [ ] 普通 Enter 换行、Ctrl+Enter 翻译、IME 保护、Escape 先关弹层、Tab 顺序明确；onUpdate 不等待翻译请求。

**Distinct test coverage:** 点击自动检测/识别提示进入同一个源语言面板；手动语言选中后关闭并归还焦点；Escape 只关本层；参数更新替换原文并失效旧结果；只有有道一个服务项仍可打开选择器；XSS 字符串作为纯文本；长单词/多行不溢出。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/panel.test.js`；用浏览器离线假桥接渲染 Light/Dark、窄宽布局并保存截图，不把截图当原生 Host 验收。

### Task 5: 接入原文与译文复制、朗读

**Files:** 新建 `package/dist/panel-capabilities.js`、`tests/panel-capabilities.test.js`；修改 `panel.js`、`tests/panel.test.js`、`tests/sdk-contract.ts`。

**Dependencies:** Task 4；H2/H3 正式 SDK。

- [ ] 两处复制使用正式 Host 纯文本复制，完成后 setStatus 显示“已复制”；失败保留文本、焦点并说明原因。
- [ ] 两处朗读使用对应文本和明确/检测语言，实现播放/停止、互斥播放、缺少声音提示和生命周期清理。
- [ ] 复制/朗读按钮与 Ctrl+K 操作共用回调，核对当前会话及 revision；空内容、过期译文和不可用能力不得执行。

**Distinct test coverage:** 复制包含完整多行译文，失败不显示成功；复制 A 未完成时更新到 B，A 状态不覆盖 B；朗读原文后切译文只播放后者，旧完成事件无效；编辑/隐藏/失效时停止；权限拒绝与缺少声音有明确反馈；不调用 navigator.clipboard、speechSynthesis 或原生私有 invoke。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/panel-capabilities.test.js examples/public-plugins/com.uipilot.translate/tests/panel.test.js`；复用 Task 3 的 SDK 类型检查命令。

### Task 6: 切换插件包并完成集成验收

**Files:** 修改 `package/plugin.json`、`README.md`；新建 `tests/package.test.js`、`tests/panel-host-contract.test.js`；必要时修改插件现有 tests，不改 SDK/CLI/宿主源码。

**Dependencies:** Task 5；H4 宿主交付与真实集成环境。

- [ ] Manifest 切换 submit + panel、inputRequired: false，增加 panel entry/runtimeActions 与 ui.panel；版本高于 1.2.1，最低 Host 使用 Task 1 记录值。
- [ ] 根据正式 SDK 申报必要复制/朗读权限，保留既有网络与服务授权边界；不自动开启权限。已有非有道服务权限不代表实现了其他服务，避免顺手变更无关权限策略。
- [ ] 更新 README 的操作、语言范围、错误恢复、能力版本与升级说明，删除“Panel 不能调用有道”等对新版本已失效的插件说明，但保留旧 Host 限制说明。
- [ ] 包测试覆盖所有入口、许可文件类型、语言/服务一致性；假 Host 集成从 onCommand → onUpdate → request → onPanelAction → 显示完成闭环。
- [ ] 使用正式 Host 验证划词填入 Panel、命令改名/收藏/绑定保持、复制和朗读、隐藏再进入及升级取消。未实测项目明确记为未验收。

**Distinct test coverage:** 安装包缺文件被发现；旧 Host 拒绝不兼容版本；升级取消保留旧插件；有道或网络撤权时不误报成功；划词失败不沿用旧正文；新划词替换面板期间的旧响应/音频无法回写；深浅主题、强制颜色、键盘与长文本通过。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/*.test.js`；Task 3 SDK 命令；`npm run build --workspace @uipilot/plugin-cli`；`node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.translate/package --platform windows`；`git diff --check`。

交付安装包时先建立插件目录下的 `artifacts/`，用 CLI `pack` 写入其中一个尚不存在的版本化 `.uipilot-plugin` 路径，再 validate 该产物；不覆盖既有包、不自动安装。CLI 构建与校验只是使用工具，不修改工具源码。

## 完成标准

- [ ] 用户确认的双文本框、三语言控件、检测选择面板、服务入口、双复制/双朗读全部可用。
- [ ] H1–H4 有正式合同及 Host 验收证据，插件只依赖公开能力。
- [ ] 离线测试、SDK 类型、包验证、双主题视觉检查均有实际结果；真实供应商与系统交互验收单独记录。
- [ ] 保留旧插件身份和划词入口，不泄露密钥、不写正文历史，过期响应与音频不会污染新会话。
- [ ] 交付变更说明、支持语言、最低版本及可安装包；不能仅凭模拟 UI 或改好 Manifest 宣称迁移完成。

## 实施记录

- 2026-11-03：用户批准计划，并明确授权宿主/SDK开发、选择隔离工作树。
- 工作区：C:/Users/moby/.codex/worktrees/translate-panel/UiPilot_tools；分支 codex/translate-panel-implementation；基线 dc14697。
- 现有翻译插件9项测试通过；环境的 node --test 派生子进程受限，直接 node tests/runtime.test.js 可运行相同 node:test 用例。
- Task1按服务/划词 H1/H4、复制/朗读/版本 H2/H3 两个可评审提交组实施；正式接口见宿主扩展实施合同。

