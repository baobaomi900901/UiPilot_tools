# 划词翻译实施计划

2026-11-01：首版已完成；后续复制兼容、条件恢复及两套历史处理按 [新计划](2026-11-01-selection-copy-compatibility.md) 执行。下方 UIA-only 为首版验收记录。

目标：用户在外部程序选择文字后按 Alt+A，UiPilot 自动打开翻译、填入选区并提交一次。依据 [已确认方案](../specs/2026-10-31-selection-translation-host-capability.md)。采用 Windows 原生取词、宿主快捷键及既有翻译调用，保持插件仅处理业务输入。

沿用当前已授权功能分支和所有既有改动，不提交混合改动、不修改 rust_uia，不读取真实密钥、不自动调用真实有道服务。使用 writing-plans、subagent-driven-development 和专项测试；自动测试注入原生驱动，人工验收由用户触发。三项实现按明确合同分别进行，整体验收前统一复核。

## 共用合同

- 新权限 `selection.read`，Windows，默认关闭；翻译插件申报它。独立取词快捷键仅绑定 `com.uipilot.translate` 的旧式单命令；原命令快捷键不变。
- 主程序设置持久化 `translationSelectionHotkey: string | null`，缺省 Alt+A；清除为 null。权限授权前不捕获选区。冲突不得覆盖已有注册。
- 内部 main IPC：`get_translation_selection_settings` → `{hotkey: string|null, status: 'active'|'unset'|'suspended'|'unavailable'}`；`set_translation_selection_hotkey({hotkey:string|null})` → 同一 DTO。详情页显示录制/清除行。权限仍经现有权限变更 API。
- 主窗口 shown 事件新增可选 `selectionTranslation: {id:string, text?:string, error?:string}`。成功 text 与失败 error 互斥；id 防重复投递。宿主解析稳定翻译身份后提供现有 commandEntry，读取后才显示；前端按该身份激活、填入文本并自动提交一次。已过期显示或用户编辑不重放。
- 新模块 `src-tauri/src/selection_capture.rs` 提供 Windows 只读 UIA 取词，返回 `Result<String, SelectionCaptureError>`，纯校验与注入驱动可测试；不显示窗口。原前台在快捷键回调时绑定，捕获和最终显示前均复核目标。检查修饰键释放、空白/NUL/8192 字节限制。审查发现模拟复制归属不可可靠证明，首版禁用该路径，系统剪贴板不变。

### Task 1: 原生取词模块

**Files:** 新 `src-tauri/src/selection_capture.rs`（可拆子模块）、必要 Windows Cargo feature。

- [x] 覆盖空选区、Unicode/长度、目标变更与不支持选区的失败行为，实现只读 UIA 取词。
- [x] 保持后台等待有界、超时不借用新目标，不对用户应用自动运行原生测试。

**Distinct test coverage:** 原前台改变时不读取新应用；没有选区不借用剪贴板；UIA 不支持时返回失败，无复制或恢复副作用。

**Verify:** `cargo test --lib selection_capture` 与 Windows 编译检查。

### Task 2: 宿主权限、快捷键和投递

**Files:** 新宿主选择翻译模块；`settings.rs`、`lifecycle.rs`、`commands.rs`、`lib.rs`、权限 Manifest/Schema/SDK 合同、翻译包 manifest。

**Dependencies:** Task 1 原生模块接口。

- [x] 注册独立 Alt+A；持久化、清除、冲突回滚及禁用卸载管理。
- [x] 权限默认关闭、独立可撤销，捕获前后核验插件和授权。
- [x] 取词完成后唤醒、稳定身份投递，重复按键和旧操作不能覆盖新操作。

**Distinct test coverage:** 未授权无取词；禁用/撤权期间捕获完成不得投递；快捷键冲突不修改原注册；原快捷键正常；关闭窗口或更新操作时迟到结果不能唤醒。

**Verify:** 原生模块及 selection translation 专项测试、权限/协议校验和 `cargo check --lib --release`。

### Task 3: 设置界面及自动提交

**Files:** `src/protocol.ts`、`src/main.ts`、`src/launcher-core.ts`、插件详情与权限组件、测试与预览。

**Dependencies:** Task 2 DTO。

- [x] 详情添加划词快捷键行，权限页增加“读取选中文字 / Selected Text”分类。
- [x] shown 事件填入参数并只提交一次；失败显示具体提示，不发送请求。
- [x] 用户编辑、关闭及重复事件不能覆盖输入或多次调用，遵循双主题与现有表单布局。

**Distinct test coverage:** 成功填入/自动提交；错误/空输入不提交；重复 id 不重放；延迟解析期间编辑不覆盖；配置改动失败后刷新真实状态。

**Verify:** 前端专项 Vitest、`npm run build`、主题预览。

### Task 4: 集成与交付

**Dependencies:** Task 1–3。

- [x] 独立评审并修复跨模块边界，更新 SDK 与插件包、打包校验。
- [x] 完成专项测试、构建和 UI 检查，提供安装与人工取词验收步骤。

**Verify:** 前端构建、Rust 发布检查、最新插件 CLI validate/pack。真实跨应用选区及剪贴板格式保护由人工测试。
## 验证记录（2026-10-31）

- 前端专项 6 文件、70 项通过；`npm run build` 通过（已有 bundle size 提示）。插件业务离线测试 9/9，CLI manifest/validate/cli 88/88。
- Rust 最终 `selection` 过滤 42 项、`command_hotkey` 过滤 16 项通过；父代理另用最新测试二进制复跑 selection_capture 6 项、selection_translation 8 项通过。`cargo check --lib --release` 通过。测试使用进程内空 bundle resources，避免复制正在使用的开发资源，未修改正式打包配置。
- 浏览器检查深浅主题、520px 窄窗口、详情取词独立分组、模拟授权后快捷键状态刷新及权限页同步。截图存于 `.superpowers/selection-translation-detail.png` 和 `.superpowers/selection-translation-permissions.png`。仅修改模拟预览状态，未开启用户真实授权。
- 独立评审发现并修复：快捷键物理回调与队列显示两处目标漂移、稳定命令路由 trim 选区空白、启动时已有快捷键优先权。无法证明归属的复制恢复路径已删除，首版仅读取 UIA TextPattern，无剪贴板或输入模拟副作用。
- 插件 1.2.0 经原生 Windows CLI 校验打包：`artifacts/plugins/com.uipilot.translate-1.2.0.uipilot-plugin`；SHA-256 `0c2f2c1b89251d09d02ec044f095302b4f46f141e8ee7373be550c3fb3039c31`。
- 未运行完整 setup 构建，未读取真实选区或密钥，未调用真实供应商 API。人工验收需重启新版主程序、升级翻译包、授权读取选中文字/有道/网络，再在提供 UIA 选区的应用中选中文字按 Alt+A。应用未提供可访问性选区时提示手动输入。
