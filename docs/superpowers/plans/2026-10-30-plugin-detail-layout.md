# 插件详情页与启动配置实施计划

目标：实施 [已确认设计](../specs/2026-10-30-plugin-detail-layout-design.md)，统一公开与内置详情 UI，并保持重名命令、快捷键、常用状态按稳定标识归属。

架构：Rust 宿主负责配置、管理授权、命令匹配和原生快捷键生命周期；React 详情页消费严格 DTO、复用录制与授权流程。使用现有 React、Ant Design、Lucide 和 CSS tokens，不引入另一套 UI 框架。应用 tailwind-design-system skill 的语义 token、组件变体、双主题与可访问性原则，具体数值遵循 `docs/ui-guidelines.md`。

## 全局约束与执行

- 用户授权在当前功能分支和目录工作，保留已有改动；不修改 rust_uia。已有改动与任务交叉，暂不提交混合文件。
- 新行为先以定向失败测试验证，实施后运行定向测试；无行为变化的样式不添加实现镜像测试。
- 每个任务完成自审及任务审查；最后统一构建、回归与视觉验收。使用 subagent-driven-development 分工，协调者处理前端协议、集成和验收。
- 后端任务与详情 UI 可按以下固定 DTO 并行推进；集成依赖二者。
- `BuiltinFeatureView` 新增 `defaultCommand: string`, `favorite: boolean`, `hotkey: string | null`, `hotkeyStatus: 'unset' | 'active' | 'suspended' | 'unavailable'`, `directory: string | null`；`command` 为有效完整斜杠命令。
- 新客户端方法 `setBuiltinFeatureName({feature, nameOverride: string | null}): Promise<void>`、`setBuiltinFeatureHotkey({feature, hotkey: string | null}): Promise<void>`，IPC 分别为 `set_builtin_feature_name`、`set_builtin_feature_hotkey`，参数平铺。公开插件清单新增 `installationDirectory: string | null`。
- 后端管理接口只授权主 WebView。保留网络、数据删除及录制授权边界，不自动操作用户插件或全局热键作测试。

### Task 1: 宿主启动配置与命令消歧

**Files:** `src-tauri/src/settings.rs`, `commands.rs`, `command_hotkeys.rs`, `lifecycle.rs`, `lib.rs`, `public_plugins/state.rs`, `public_plugins/activation.rs` 及相应路由/权限文件和测试。

- [x] 按设计「启动指令与匹配」实现重复别名、稳定标识分派、携带参数的歧义列表，以及三种内置别名持久化。
- [x] 按设计「内置快捷键」实现绑定、暂停、录制、重启恢复及冲突/失败回滚，兼容既有截图快捷键。
- [x] 补齐清单 DTO 与实际安装目录、新 IPC 注册及授权。

**Distinct test coverage:** 重名保存、默认重置、禁用与参数消歧；旧设置加载；快捷键保存失败恢复旧注册、注册失败不落盘、录制期间不触发；非 main 调用在访问状态之前拒绝。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib`，优先按实现模块筛选定向测试，再集成构建。

### Task 2: 详情页面及统一 UI

**Files:** `src/public-plugin-panel.tsx`, `src/builtin-feature-panel.tsx`, `src/launcher-view.tsx`, `src/hotkey-recorder-input.tsx`, `src/styles.css`，相关组件测试。

**Dependencies:** 上述固定 DTO；设计「页面与交互」。

- [x] 页头居中、星标与当前命令绑定，多命令选择后展示对应启动配置。
- [x] 失焦自动保存、初始化、录制与未设置占位符；保留草稿、异步所有权和错误反馈。
- [x] 整理只读内容、按需网络区域、真实目录，保留插件设置和凭据入口。
- [x] 删除确认及 retainData、诊断只读文本域，内置隐藏不适用操作。
- [x] 使用 UI skill 的组件一致性和可访问性原则，统一表单轨道、控件尺寸、主题和焦点。

**Distinct test coverage:** 失焦与初始化不双写、换命令不串状态、头部常用操作；删除取消和两种 retainData；诊断按需展开；内置配置入口与不可删除。

**Verify:** `npm test -- --run src/launcher.test.tsx src/builtin-feature-panel.test.tsx` 及新增定向测试文件。

### Task 3: 协议、预览与集成验收

**Files:** `src/protocol.ts`, `src/tauri-client.ts`, `src/launcher-browser-preview.tsx`, `src/settings-browser-preview.ts` 和客户端/解析测试（按现有实际文件定位）。

**Dependencies:** Tasks 1–2；设计「数据与权限边界」「验收」。

- [x] 严格解析新 DTO 并接通 IPC；更新浏览器模拟客户端和测试清单。
- [x] 运行 TypeScript/Vite 构建、前端相关回归、Rust 定向验证；修复集成问题。
- [x] 渲染深浅主题，检查窄窗口、字段顺序、长路径、键盘焦点和弹窗。
- [x] 独立代码审查后处理实际问题，记录需用户实机确认的系统热键部分。

**Verify:** `npm run build`; `npm test -- --run src/launcher.test.tsx src/builtin-feature-panel.test.tsx src/protocol.test.ts`; `git diff --check`。

## 验证记录（2026-10-30）

- 页面与交互实现完成，已通过深浅主题及 520px 窄窗口浏览器检查。
- `npm run build` 通过；5 个相关组件测试文件 404 通过、18 跳过；协议及命令身份测试 27 通过。
- SDK 安装恢复后，Rust 相关定向测试 16 项通过；`cargo check --manifest-path src-tauri/Cargo.toml --lib` 在沙箱外通过。沙箱内 Tauri 构建脚本曾因访问拒绝失败。
- 独立审查指出的别名冲突、稳定身份路由、IME 异步所有权、删除后数据清理反馈均已修复。
- Rust 全量测试尚未全绿。此前完整运行 1793 通过、21 失败、11 忽略；已修复本任务的陈旧 DTO 断言，其余失败复测和原因记录见 `.superpowers/detail-host-report.md`，不把未证明的失败称为基线。
- 仍需用户在原生程序中检查全局快捷键录制、冲突、禁用暂停与重启恢复。未自动修改用户真实快捷键。
- 保留当前分支及已有未提交修改，不提交混合文件、不修改 rust_uia、不发送消息推送。
