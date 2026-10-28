# UI 与 UX 统一实施

目标：让主启动器、设置、文件搜索及公开插件的 mainResult / panel / window 三种输出遵循同一套可验证的视觉与交互规范。基线为 [UI 规范](../../ui-guidelines.md)，保留已验收的功能、数据、快捷键录制与插件权限边界。

沿用宿主 CSS token + Ant Design 主题，以及插件包内 CSS 和现有 `packages/plugin-ui`。不引入新的 UI 框架或公开 Runtime API。宿主与插件只共享视觉合同，不重写插件业务状态机。

## 约束与验证

- React 19 / Ant Design 6 / Tauri 2；Windows WebView2；Light、Dark 同等支持。
- 默认字体栈统一为 Inter、Microsoft YaHei UI、system-ui。没有内置 Inter 字体资源时使用系统回退，不联网加载字体。
- 用户偏好人工功能验收。CSS 调整使用真实浏览器双主题、窄窗口检查；主题映射和键盘行为使用针对性自动回归；不为纯样式逐项重复 TDD。
- Task 1 -> Tasks 2、3 -> Task 4。并行实施分别限定宿主和插件文件，完成后统一审查。
- 每个实现单元验证后独立提交；不包含临时预览产物，不推送远端。

### Task 1: 建立尺寸与交互合同

**Files:** `docs/ui-guidelines.md`、本计划。

- [x] 补齐同类控件尺寸、三种插件的布局边界、文案和快捷键矩阵。
- [x] 明确文件搜索分类 Tab 特例、图钉 Ctrl+T、刷新拦截和未保存确认语义。

**Distinct test coverage:** 规范覆盖所有六类界面，特殊业务读数与窗口标题栏尺寸有明确例外。

**Verify:** 按 `docs/ui-ux-acceptance.md` 的覆盖矩阵核对。

### Task 2: 宿主主题和控件一致性

**Files:** `src/ui-theme*`、`src/styles.css`、`src/find-view*`、`src/plugin-window-view*`、`src-tauri/src/plugin_window.rs`、`src-tauri/src/plugin_panel.rs`。

**Dependencies:** Task 1；规范「主题与颜色」「宿主界面布局」「键盘与焦点」。

- [x] 统一字体、选择态、标题与说明字重字号；清理设置导航叠加缩进。
- [x] 文件搜索采用 60px 搜索区 / 44px 输入 / 36px 底栏，保留分类、左右预览与动画。
- [x] 独立插件窗口颜色与面板/宿主一致，外壳 Ant 主题随已注入主题更新。
- [x] 分类焦点下 Enter/Escape 可用，搜索输入具备可访问名称；保留录制优先、IME 与图钉行为。

**Distinct test coverage:** CSS / Ant / 原生注入颜色与字体一致；主题变更监听清理；分类焦点执行/退出且不重复触发。

**Verify:** `npm.cmd test -- src/ui-theme.test.ts src/find-view.test.tsx src/plugin-window-view.test.tsx src/webview-reload-guard.test.ts`

### Task 3: 插件包内视觉统一

**Files:** `packages/plugin-ui/`、`examples/public-plugins/` 中可视插件 CSS / HTML。

**Dependencies:** Task 1；规范「组件与状态」「公开插件附加规则」。

- [x] 统一 notes、clipboard-history、templates、note、json-workbench、pomodoro、demo-panel、demo-win 的同类控件。
- [x] 修正暗色主按钮对比、旧式投影/字体、过小元数据和无回退的派生色。
- [x] 为现有笔记对话框补齐可访问命名与右上角关闭入口，复用取消路径保留草稿状态。
- [x] 同步 templates 引用的共享 UI 文件。

**Distinct test coverage:** 插件原有编辑/存储/确认行为不变；双主题按钮清晰；离线包包含完整资源。

**Verify:** `node --test packages/plugin-ui/tests/*.test.mjs`，以及受影响插件现有 Node 测试；`node packages/plugin-ui/scripts/sync-templates.mjs --check`。

### Task 4: 视觉复核和人工验收交付

**Files:** `dev/*preview*`、`src/*browser-preview*`、`docs/ui-ux-acceptance.md`。

**Dependencies:** Tasks 2、3。

- [x] 真实组件预览覆盖主界面、设置、文件搜索、mainResult、panel、window。
- [x] 在 Light/Dark、常规/窄窗口验证级联后的尺寸、截断、滚动、主题和对话框。
- [x] 编译、复核差异，列出用户下一轮人工验收用例及插件更新方式。

**Distinct test coverage:** 不能用源字符串测试替代真实浏览器布局；不能用浏览器预览替代原生 WebView 的焦点和快捷键验收。

**Verify:** `npm.cmd run build`；浏览器视觉与 computed style 检查；人工用例单独标记待验收。

## 完成记录

代码、浏览器双主题复核、构建与相关回归已完成。独立复核发现的浅色错误文字对比问题已修正并增加对比度测试。8 个本地插件包已重新生成，版本与数据格式不变。原生人工验收仍待用户按 [验收清单](../../ui-ux-acceptance.md) 确认。
