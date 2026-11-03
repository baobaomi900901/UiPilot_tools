# Ctrl+K 动态操作面板实施计划

按 [已确认设计](../specs/2026-11-01-context-actions-design.md) 实现共享宿主菜单、Panel 桥接及 Notes 操作。React/TypeScript、原生 JavaScript bootstrap、Rust/Tauri；沿用当前分支，保留已有改动，不自动提交。

### Task 1: 共享菜单与主启动器入口

**Files:** `src/context-actions-ui.js`、类型与测试；`src/launcher-view.tsx`、样式、测试。
- [x] 共享视图按设计“表现与交互”实现分组和焦点恢复。
- [x] 固定右下入口；主结果默认动作、动作列表、设置/隐藏入口统一接入。
**Verify:** 菜单 Vitest、launcher 定向回归与前端构建。

### Task 2: Panel 会话桥接

**Dependencies:** Task 1；设计“Panel 合同”。
**Files:** `src-tauri/src/plugin_panel.rs`、commands/注册/权限；`src/main.ts`、protocol；SDK 文档与 bootstrap 测试。
- [x] 注入共享菜单，注册同步提供器，处理内容 Ctrl+K。
- [x] main 打开菜单和有票据的提交请求；更新会话使旧菜单作废。
**Distinct coverage:** main/content 权限隔离；旧会话/旧 ticket/输入变化均不提交；关闭恢复焦点，重提交不执行旧回调。
**Verify:** bootstrap Vitest、Rust panel 测试、SDK 类型检查。

### Task 3: Notes 集成与验收

**Dependencies:** Tasks 1–2；设计“Notes”。
**Files:** Notes 包、测试、预览及文档。
- [x] 提供主输入、目录、正文的当前操作和通用操作；捕获并校验笔记对象。
- [x] 更新版本，预览验证，无真实用户数据变更。
**Verify:** Notes runtime、CLI validate、浏览器视觉与键盘验证；记录未覆盖的实机验证。


## 验证结果（2026-11-01）

- 相关前端完整回归 410 项通过；跳过已有的 18 项及本轮前已存在的 `/find` 空查询失败项 `opens the catalog find row directly with an empty query`。未修改该既有问题。
- 审查修正后的定向回归 44 项通过，覆盖相同动作 ID 在新结果中的旧菜单失效、随机单次提交票据、设置 Ctrl+T 隔离和窗口失焦关闭。
- Rust Panel 相关 118 项通过；最终焦点转移定向测试 1 项通过。现有截图错误枚举 dead_code 警告仍在。
- Notes 28 项测试、SDK 类型检查、CLI validate（com.uipilot.notes@1.2.8）通过；前端 TypeScript 与生产构建通过（保留既有大包体提示）。
- 独立只读审查未发现阻断问题；提出的 Ctrl+T 捕获阶段冲突已修正并加入回归。复制辅助输入框在 finally 中恢复原焦点和选区。
- 浏览器使用合成数据验证深/浅主题、主输入/列表/正文动态内容、Esc 恢复、点击底栏保持正文上下文。截图：artifacts/context-actions-notes-dark.png。不含“点击执行、↑↓选择”说明。
- 未自动操控用户真实笔记或安装插件；原生 WebView 实机验收仍需用户更新 dev 后，重新选择 Notes package 开发目录更新至 1.2.8（保留数据），分别在主输入框、列表和正文按 Ctrl+K，并验证 Esc 与原有快捷键。


## 2026-11-02：入口右对齐及虚拟滚动条

- 按用户调整：状态左对齐，操作入口放在底栏最右侧；设置版本号位于入口左侧，弹层从右侧向上展开。
- 共享菜单隐藏原生滚动条，使用细轨道与圆角虚拟滑块，支持滚轮、拖拽、轨道点击；无溢出隐藏，关闭时销毁监听和 ResizeObserver。最大高度根据底部位置限制，避免小窗口越界。
- 47 项菜单/启动器定向测试、14 项 Panel bootstrap 测试、Rust bootstrap 合同测试和生产构建通过。
- 浏览器合成长列表验证滑块拖拽、轨道操作和 End 键末项可见性；实际主界面预览确认右对齐。效果截图：artifacts/context-actions-right-aligned.png、artifacts/context-actions-scroll-verified.png。
- 此轮修改在宿主共享组件中，现有 Notes 1.2.8 无需再次更新；重启 dev 以重新编译注入原生 Panel 的菜单代码。
