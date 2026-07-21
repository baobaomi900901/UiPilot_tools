# UiPilot 启动与 DoubleTap 修复设计

## 1. 文档信息

- 日期：2026-07-21
- 状态：待产品负责人书面确认
- 产品基线：`996a9bfb361df76d922de73c283adc7470012d39`
- 原 Cursor WIP 证据：`codex/cursor-wip-archive` / `1dd6463`
- 目标平台：Windows 11 x64

## 2. 决策摘要

不继续修补归档 WIP。启动延迟只通过缩小初始前端模块图解决，不改变已经验收的八命令、
`launcher://shown`、`load_settings` readiness 或 capability 边界。设置界面保留 Ant Design，但作为
Vite 可静态分析的懒加载模块；启动器输入框使用原生 HTML。Windows 主题继续由系统和
`prefers-color-scheme` 驱动，不读取注册表、不固定窗口主题。

DoubleCtrl/DoubleAlt 使用一个专用 Windows 消息循环线程。`HHOOK` 只在该线程创建、使用和释放；
`LifecycleCoordinator` 只持有可安全跨线程的停止/等待所有权，不通过整数转换绕过 `Send/Sync`。

## 3. 目标

1. 冷启动时初始 JavaScript 不包含 Ant Design 设置界面。
2. 保持现有 listener -> native input binding -> `load_settings` readiness 顺序。
3. 托盘、全局快捷键和单实例请求仍只通过既有 `request_show` 流程。
4. 设置页首次打开时按需加载，并保留当前功能、IME ownership 和可访问性。
5. 亮色/暗色在应用运行期间继续跟随 Windows 系统变化。
6. 启动加载和运行时保存产生的 DoubleTap hook 都有可工作的消息循环和确定性卸载。

## 4. 非目标

- 不新增 Tauri command、event、capability、permission、依赖或窗口。
- 不新增 `early-ready.ts`、`boot.ts`、全局 window bridge、deferred payload 或 drain API。
- 不更改 Rust 生命周期状态机、notice、ResultRegistry 或 `load_settings` 语义。
- 不读取 `AppsUseLightTheme`，不调用 `window.set_theme(Some(...))`。
- 不保留临时时序日志、raw error console logging 或 lint suppression。
- 不处理 `/find`、插件系统、安装、发布或运行时安全门禁。

## 5. 前端设计

### 5.1 初始入口

`index.html` 仍加载 `src/main.ts`。HTML 可以包含与启动器尺寸和语义一致的静态 shell，但不得包含
内联脚本。React 首次提交替换 shell；真实窗口仍由现有 readiness 在可交互后显示。

### 5.2 模块边界

- `launcher-view.tsx`：React、原生查询 input、结果列表、状态区和 settings lazy boundary。
- `settings-panel.tsx`：现有 Ant Design 设置表单、快捷键录制器和设置 input binding。
- lazy import 必须是 `import('./settings-panel')`，不得使用 `@vite-ignore`。
- `vite.config.ts` 最多只排除 `antd` 的开发期预构建；不得增加第二套入口或 runtime loader。

生产构建必须生成一个初始 JavaScript 和至少一个设置相关动态 chunk；初始 chunk 不得包含 Ant Design
设置组件文本，产物不得保留无法解析的 `import('./settings-panel')`。

### 5.3 主题

原生 launcher 使用 CSS `prefers-color-scheme` 和 `color-scheme`。`LauncherView` 现有
`matchMedia('(prefers-color-scheme: dark)')` 仅把布尔值传给懒加载的 Ant Design `ConfigProvider`。
不创建第二个系统主题来源。

## 6. DoubleTap hook 所有权

`HotkeyHook::install` 创建一个命名线程，并等待线程完成以下步骤后才返回成功：

1. 建立线程消息队列。
2. 在该线程调用 `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`。
3. 把安装成功状态和停止句柄返回给调用方。
4. 运行阻塞 Windows message loop。

卸载时，owner 请求该线程退出并等待 join。hook 线程在退出前调用并检查
`UnhookWindowsHookEx`；原始 `HHOOK` 不存入 `LifecycleCoordinator`，也不跨线程移动。
安装失败必须清理 `HOOK_STATE` 并结束线程。重复卸载不得留下 callback state 或后台线程。

现有 settings transaction 的回滚、注册上限和持久化顺序保持不变。

## 7. 错误处理

- settings chunk 加载失败继续进入现有固定本地初始化失败界面，不显示异常文本。
- hook 安装、停止或 join 失败映射为现有 settings failure，不提交新设置。
- 不记录 thrown value、路径、PID、handle 或用户输入。

## 8. 测试与验收

按 RED -> GREEN 顺序执行：

1. 前端 source/build RED：拒绝 `@vite-ignore`，要求设置动态 chunk，拒绝新 Tauri importer/command。
2. 真实 `boot` 不存在；adapter 测试继续证明 binding 失败时零 listen、零 invoke。
3. settings lazy mount、首次 focus、unbind -> retire 顺序和主题 change/remove listener 回归通过。
4. hook owner RED：证明 install 与 uninstall 发生在同一消息循环线程，失败清理一次，owner drop 无残留。
5. focused tests 后运行 scoped frontend tests、production Vite build、default/all-features Rust tests、
   default/all-features Clippy `-D warnings`、fmt 和两项安全配置脚本。
6. 最终 command/capability/permission 数量仍精确为八，`Cargo.lock` 和安全输入不变。

## 9. 提交边界

实现从独立 `codex/cursor-startup-fixes` worktree 开始。允许修改的候选文件仅为：

- `index.html`
- `vite.config.ts`
- `src/launcher-view.tsx`
- `src/settings-panel.tsx`
- `src/styles.css`
- `src/launcher.test.tsx`
- `src-tauri/src/hotkey_hook.rs`
- 必要时与 hook 失败传播直接相关的既有 lifecycle 测试/实现

若需要 command、capability、permission、Cargo feature、生命周期 readiness 或其他文件，立即停止并重新审查设计。
