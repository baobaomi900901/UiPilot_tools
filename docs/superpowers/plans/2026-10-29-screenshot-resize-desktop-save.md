# 截图选区调整与桌面直存实施计划

采用 writing-plans 与 subagent-driven-development 流程，在当前已授权的功能分支执行；不再询问执行方式。技术合同以[已批准设计](../specs/2026-10-29-screenshot-resize-desktop-save-design.md)为准。

目标：选中后保留遮罩，支持八方向调整和桌面直存。Rust纯逻辑模块管理调整几何；原生遮罩负责呈现、输入与异步任务；独立输出模块负责无覆盖发布。现有原版 Provider 管道客户端保持不变。

## 全局约束与验证

- 不编辑 rust_uia、不读取其源码来重写识别、不新增UIA识别实现；只使用现有EXE适配器。
- 保留用户未提交的其他文件。继续在 codex/builtin-screenshot 工作。
- 每单元先补聚焦失败测试，再实现、验证及复审；提交只包含本任务文件。
- 测试编译可使用进程内 `TAURI_CONFIG={"bundle":{"resources":[]}}` 避免复制运行中的资源，不修改正式配置。最终验证完整截图模块和原版EXE受控集成。
- 原生线程不得等待PNG编码或写盘；冻结帧只取一次，输出不含编辑控件。退出与发布须有明确线性化点，不覆盖已有文件。
- Task 1、2可独立推进；Task 3依赖其接口；Task 4统一验证。

### Task 1: 纯选区调整模型

**Files:** 新增 src-tauri/src/screenshot/editing.rs；模块声明由协调者接入。
- [x] 实现八个方向、视觉点与命中、拖过对边、1像素下限、桌面边界和键盘调整。
- [x] 实现保存按钮在右下/右上及可见屏内的布局，处理多屏空隙、小选区及不同DPI。
**Distinct test coverage:** 所有方向、对边翻转、负坐标、最小矩形、边界裁限、角点优先和屏幕边缘按钮可达。
**Verify:** `screenshot::editing::tests`。

### Task 2: 有界后台桌面输出

**Files:** 新增 src-tauri/src/screenshot/desktop_output.rs；必要时仅复用 output.rs 的编码接口。
- [x] 使用注入的真实桌面目录，自动命名PNG并原子无覆盖发布，碰撞追加序号。
- [x] 输出取消/提交门控、编码预算、错误分类及临时文件清理；保留Ctrl+C复制能力。
**Interfaces:** `OutputKind::{SaveDesktop,Copy}`、`OutputSuccess::{Saved(PathBuf),Copied}`、`OutputGate`；后台 `perform(kind,image,budget,desktop,gate)` 返回成功或错误。模块自行提供明确接口供Task3使用。
**Distinct test coverage:** 同名文件不被覆盖、并发重名写入、不可写/缺失目录、不完整写入、取消先于提交及提交先于取消、重复提交隔离。文件测试只写临时目录。
**Verify:** `screenshot::desktop_output::tests`。

### Task 3: 原生遮罩编辑与主程序会话接入

**Files:** screenshot/windows/overlay.rs、windows/mod.rs；可新增windows/editor.rs拆分绘制/输入；screenshot/window.rs、service.rs、mod.rs。
**Dependencies:** Tasks 1、2。
- [x] 初选松开进入编辑，暂停Provider；保留防抖与冻结帧，拒绝晚到探测修改选区。
- [x] 绘制八个调整点、尺寸和原生保存按钮，正确处理捕获、焦点、Tab、方向键、Enter、Ctrl+C及Esc。
- [x] 启动后台输出，输出期间禁止重复提交；失败保留编辑会话，成功关闭遮罩并提供结果位置反馈。
- [x] 主程序传入真实桌面路径，输出成功结束正确会话；重拍取消保留旧图，旧响应不影响新会话。
**Distinct test coverage:** 单击与拖框均不立即提交；调整输入不穿透；控件不入图；失败可重试；过期结果/取消/提交竞争；输出成功后不弹预览或抢焦点。
**Verify:** `screenshot::windows`、`screenshot::service`、`screenshot::window`。

### Task 4: 统一回归和交付

**Files:** 开发与验收文档。
- [x] 复审源码，完整截图回归与原版EXE真实管道集成；核对rust_uia工作区仍与初始源码一致。
- [x] 验证dev编译、记录实际测试及人工复测步骤；明确本轮是否重建Setup。
**Verify:** `cargo test --lib screenshot::`、显式忽略集成测试、Rust格式和diff检查。

## 执行记录

- Task1：76a4348；纯几何RED→GREEN、独立复审通过。
- Task2：60ae101；11项输出测试及实际依赖回归通过、独立复审通过。
- Task3：3e9ca9a；原生遮罩、后台输出及主程序会话接入。
- Task4：最终截图回归116通过、原版EXE集成2通过、两主题原生渲染查看通过；详情见截图验收记录。用户桌面的鼠标操作和混合DPI仍按验收步骤复测，本轮不重建Setup。
