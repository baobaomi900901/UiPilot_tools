# 截图工具栏与贴图实施计划

**Goal:** 四个图标动作、原生置顶贴图和可配置的截图保存目录。

**Architecture:** 扩展现有原生编辑遮罩，提取工具栏与贴图为独立模块；目录偏好使用现有 SettingsStore、命令授权和通用设置状态模型。按已确认的 [设计](../specs/2026-10-29-screenshot-toolbar-pin-design.md) 实施。

**Tech Stack:** Windows Rust/windows 0.61、Tauri 2、React/TypeScript、Lucide。

## 全局约束与执行

- 元素探测继续调用 rust_uia 独立 EXE；不修改、复制或重新实现 rust_uia 源码。
- 保留现有悬停、防抖自由框选、冻结画面、50% 遮罩和八方向调整。
- 当前 codex/builtin-screenshot 分支实施，保留用户既有未提交改动；提交只包含本任务增量。
- 执行 subagent-driven-development：每项先增加有效失败测试、实现、针对验证、独立审查，通过后继续。每项独立提交。
- 原生测试使用自有受控窗口和临时目录，不模拟输入到用户应用、不替换剪贴板内容做无关测试。
- Rust 验证在 src-tauri 中执行；必要时仅对测试设置 TAURI_CONFIG='{"bundle":{"resources":[]}}' 避开正在使用的资源复制，不改打包配置。
- 顺序 Task 1 → Task 2 → 最终回归与文档。Setup 与 dev 验证结果分别记录，不把测试构建冒充安装包构建。

### Task 1: 默认保存目录设置与实际输出接通

**Files:** `src-tauri/src/settings.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`（仅命令注册增量）, `src-tauri/src/screenshot/window.rs`, `src-tauri/src/screenshot/desktop_output.rs`, `src-tauri/src/screenshot/windows/editor.rs`, `src/protocol.ts`, `src/client.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx` 和相关测试/预览 fixture。

**Requirements:** 设计“默认保存位置”全部条款及“工具栏”中的保存失败保留选区。读取 `docs/ui-guidelines.md` 后修改通用设置。

- [x] 增加可空持久化目录字段及独立偏好更新，目录选择器取消不修改；路径必须为存在的绝对目录。主窗口授权先于任何目录选择/修改。
- [x] 通用设置显示解析后的完整路径，提供选择文件夹、恢复桌面，匹配现有读写忙碌/失败模型。
- [x] 每次开始截图固定已提交的目录；输出错误改为通用保存目录文案，默认行为继续解析真实桌面；无覆盖原子写入不变。
- [x] 在现有脏文件中保留用户改动；先保存任务前文件快照，提交时仅本任务相对快照的增量。

**Distinct test coverage:** 缺失字段迁移为默认桌面；自定义目录与恢复默认重启生效；其他设置保存不丢失目录；非法/相对/非目录拒绝；非主窗口在打开对话框前被拒绝；文件夹选择取消保持原状态；目录设置写入失败前端保留旧值；离开设置后旧异步响应不覆盖新状态；开始截图后偏好改变仍使用会话快照；保存目录删除/不可写时选区保留。

**Verify:** `npm test -- src/launcher.test.tsx src/client.test.ts src/protocol.test.ts`; `npx tsc --noEmit`; `cargo test --lib screenshot:: -- --test-threads=1`; `cargo test --lib settings::tests -- --test-threads=1`，另运行新增命令授权测试的明确名称。

### Task 2: 图标工具栏、原生贴图与完成生命周期

**Files:** 新增 `src-tauri/src/screenshot/windows/toolbar.rs`、`src-tauri/src/screenshot/windows/pin.rs`（可按职责调整命名）；修改 `windows/editor.rs`, `windows/overlay.rs`, `windows/mod.rs`, `screenshot/desktop_output.rs`, `screenshot/window.rs`, `screenshot/editing.rs`；必要的图标资源与 Cargo feature。

**Dependencies:** Task 1；设计“工具栏”“贴图窗口”及“验证与交付”。

- [x] 四个原生图标按钮 X/Pin/Save/Copy，使用 Lucide 已有图标资源与授权，中文名称/tooltip、主题和焦点；整体定位替换单个保存按钮。
- [x] 扩展纯编辑状态与键盘路由，Tab 顺序和 Enter/Space 按当前焦点触发；调整点 Enter 保持保存；输出防重复、失败保留。
- [x] 原生贴图窗口持有有预算的冻结裁剪像素，图像内容初始位置与物理大小等于选区；独立可拖动、置顶、Esc/右键关闭、多张共存，阴影外扩且不拦截底层点击。
- [x] 顶部光源投影，跨 DPI 拖动不缩放内容。创建成功再关闭遮罩，创建失败回到编辑。退出宿主释放贴图，正常新截图包含可见贴图。
- [x] 输出单次提交与取消有明确线性化点，提交成功不得被晚到取消错误归类；迟到消息不能关闭新截图会话。图像/窗口资源在所有失败和关闭路径释放。

**Distinct test coverage:** 四动作顺序/键盘、按钮焦点与调整点焦点分别 Enter、拖动期间禁止输出、重复点击只一次；工具栏边缘/跨屏布局、原生按钮名称和实绘图标；真实裁剪不含工具栏/阴影；贴图创建失败保留选区；取消先发生时不出现贴图，创建提交先发生时真实成功且独立存活；多贴图关闭其一仅释放其配额；图像物理尺寸/位置及阴影方向，阴影点击透明，混合 DPI 移动保持大小。受控窗口测试避免对用户桌面发送输入。

**Verify:** `cargo test --lib screenshot:: -- --test-threads=1`；实际原版 EXE 用既有 `screenshot::probe::tests::real_provider_` ignored 集成测试；生成并检查 light/dark 原生工具栏与贴图测试图。

## 最终检查

- [x] 两任务审查通过；运行必要前端/Rust回归和构建，核对 rust_uia worktree 未变。
- [x] 更新 `docs/screenshot-development.md`、`docs/testing/builtin-screenshot-acceptance.md`，区分自动结果和用户人工待验项。
- [x] 提供 dev 重启/人工测试步骤，明确是否重建 Setup；仅提交本任务增量。

完成记录：Task 1 `a2965a5`，Task 2 `7ce941f`。两项任务审查及完整扩展的生命周期集成审查均通过，无 Critical/Important 问题。截图129通过/4忽略，原版Provider集成2通过，设置37通过，授权/ACL/协议各1通过；前端399通过/18跳过，TypeScript、Vite生产构建和Rust格式检查通过。双主题设置/工具栏/贴图图像已查看。未重建Setup；真实混合DPI拖动及系统文件夹选择器等待人工验收，详见验收记录。当前功能分支保留，不合并或推送。
