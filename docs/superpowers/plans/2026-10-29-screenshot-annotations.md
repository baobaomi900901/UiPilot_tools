# 截图标注与撤销重做实施计划

目标：在现有截图编辑阶段提供马赛克、文字、实心矩形、撤销与重做，并让保存、复制和贴图使用相同的合成结果。

架构：独立的标注/历史模型记录物理坐标中的编辑；共享像素合成生成预览和输出。原生适配负责工具栏、属性行、笔刷光标、中文文字输入和消息路由。

技术：Rust、windows 0.61.3、现有 Win32 原生截图覆盖层与 Lucide 资源。技术合同以 [已确认方案](../specs/2026-10-29-screenshot-annotations-design.md) 为准。

## 全局约束与执行规则

- rust_uia 只使用原版独立编译物，源码不修改、不复制。
- 保留 80% 黑遮罩、黑白线状图标、无消息推送和主动终止隐藏至托盘。
- 当前功能分支就地实现，保留既有用户未提交改动；仅提交本任务文件/增量。
- 标注和选区使用桌面物理坐标，预览与三种输出共享合成，像素分配遵守 PixelBudget。
- 每个实现任务使用有意义的先失败后通过测试，完成后提交并进行规格/质量审查；Task 1 → Task 2 → Task 3。以 subagent-driven-development 执行，不重复请求用户确认。
- Cargo 检查使用测试资源覆盖 `TAURI_CONFIG={"bundle":{"resources":[]}}`，日志存入 `.superpowers/sdd/annotations/`。避免多任务同时构建，保护用户运行中的 dev 会话。

### Task 1: 标注数据、像素合成与编辑历史

**Files:** 新增 `src-tauri/src/screenshot/annotations.rs`，修改 `src-tauri/src/screenshot/mod.rs`；需要时将合成拆入相邻 `annotation_render.rs`。

- [x] 实现有序标注：实心矩形、连续圆形马赛克笔画和具有文本元数据的预算受控文字栅格；记录创建时选区裁切范围。
- [x] 实现编辑历史，覆盖标注新增/文字替换/选区调整、撤销/重做分支、无效操作忽略与资源失败保持状态。
- [x] 合成接收冻结 monitor frames、当前选区与 PixelBudget，支持负坐标及跨屏；马赛克从该笔之前的合成结果取固定网格颜色，不重复破坏原图。
- [x] 向下游提供清晰接口：当前选区/标注快照、追加与替换、撤销/重做、最终像素合成；记录具体接口到任务报告。

**Distinct test coverage:** 快速笔画连续性、选区外不涂抹及断笔、图层顺序、马赛克覆盖此前标注、矩形完全不透明、文字透明栅格、选区缩放后恢复、撤销分支、预算释放和失败原子性。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib screenshot::annotations:: -- --test-threads=1`；可先用独立 rustc harness 缩短纯模型迭代。

### Task 2: 原生工具栏、鼠标标注与中文文字输入

**Dependencies:** Task 1。

**Files:** `windows/editor.rs`、`windows/overlay.rs`、`windows/toolbar.rs`；新增 `windows/annotation_ui.rs`、`windows/text_input.rs`（可按实际职责细分）；`resources/screenshot-icons/`；若需 Win32 IME feature，仅对 Cargo.toml 做对应增量。

- [x] 扩展为九个工具栏按钮，属性行提供笔刷大小、文字字号和红黄蓝黑白颜色，支持小屏幕分行与动态可用状态。
- [x] 实现工具互斥、连续笔画、矩形防抖、圆形笔刷提示和选区限制；保留无工具模式下的八方向调整。
- [x] 接入原生多行中文输入，输入法组合期间不抢键；支持提交/取消、点击已存在文字再次编辑及输出前提交，预览和输出共用文字栅格。
- [x] 将选区调整纳入历史，每次拖动和方向键序列各为一步；实现快捷键和禁用状态。
- [x] 将当前编辑合成接入预览与 start_output，输出失败保留编辑状态，正在拖动时阻止输出。
- [x] 定位现有工具栏焦点线原生测试失败，修正真实原因并覆盖多 DPI。

**Distinct test coverage:** 工具切换/状态、键盘与中文输入法路由、空文字不记录、编辑文字撤销、拖动取消、点击抖动、焦点与尺寸在多个 DPI 下正确、原生三种输出消费同一标注像素。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib screenshot:: -- --test-threads=1`；原生受控测试生成深浅主题图并进行视觉检查。

### Task 3: 整体验证与人工验收交付

**Dependencies:** Task 2。

**Files:** `docs/screenshot-development.md`、`docs/testing/builtin-screenshot-acceptance.md`、本计划与规格状态；按审查发现修正 Task 1/2 涉及文件。

- [x] 审查输入生命周期、历史内存预算、跨屏坐标、输出失败和取消竞争；修复实质问题。
- [x] 完成格式、diff、截图相关测试及深浅主题渲染检查；不重复与本次变更无关的 Provider 重编。
- [x] 更新当前行为文档，记录真实通过/未覆盖项目，并提供人工验证马赛克、中文、矩形、撤销重做及三种输出的步骤。

**Verify:** `rustfmt --check`（本次 Rust 文件）、`git diff --check`、截图测试结果与实际原生渲染图片。

实施结果：2026-10-29 完成。最终截图测试161通过/4忽略；原生专项64通过/1忽略；模型与集成审查通过。真实输入法、跨屏交互及复制/贴图人工验收见 docs/testing/builtin-screenshot-acceptance.md。
