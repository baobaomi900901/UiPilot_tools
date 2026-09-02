# Raycast Kill Process 与 UiPilot 功能差异研究

- 研究日期：2026-09-02
- Raycast 源码基线：[`b2293b093846d374e0b703c832787dd2015800b3`](https://github.com/raycast/extensions/tree/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/)
- UiPilot 基线：当前工作区 `0.3.4`（[`package.json` 第 3 行](/D:/code/UiPilot_tools/package.json:3)、[`PublicPluginHost::current` 第 1165–1177 行](/D:/code/UiPilot_tools/src-tauri/src/public_plugins.rs:1165)）

## 结论先行

Raycast 的 Kill Process 已经是一套完整的跨平台进程管理扩展：它能枚举进程，展示 CPU/内存，搜索、排序和聚合应用进程，并提供终止、强制终止、终止同名进程、终止进程树、重启、复制路径、自动刷新和 AI 工具入口。它能做到这些的关键不只是 `@raycast/api`，而是 Raycast 允许扩展直接使用 Node.js 的 `child_process`、文件系统和操作系统命令。源码分别在 macOS 调用 `ps`、`kill`、`killall`、`pgrep`、`open`，在 Windows 调用 PowerShell、WMI/CIM 和 `taskkill`。[进程执行层](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process.ts#L1-L53)；[平台命令层](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L32-L165)

UiPilot `0.3.4` 的公开插件 API 目前不能实现真实的 Kill Process。公开 Runtime 明确不能访问 Shell、任意文件、原生二进制、Tauri 或原生输入；现有权限中也没有进程枚举或进程控制权限。能做的是插件私有存储、设置读取、受控 HTTPS、受控通知/计时器，以及 `mainResult`、单例窗口或 Panel UI。[公开插件 Runtime 边界](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:63)；[已实现权限](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:200)；[v1 明确不支持的能力](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:215)

因此：

- 用 UiPilot 公开插件可以复刻一部分视觉和交互壳，例如在 Panel 中画进程列表、搜索框、排序控件和确认对话框。
- 但公开插件无法取得本机进程、CPU/内存，也无法终止或重启进程；这不是缺少前端组件，而是缺少宿主权限与原生 API。
- 若要实现同等功能，当前只能做 UiPilot 宿主内置功能，或先新增版本化、宿主管理的进程读取/控制 API；单纯移植 Raycast TypeScript 源码不可行。

## 研究范围与一手资料

### Raycast 固定提交中已阅读的功能源码

已逐文件阅读该目录的全部运行逻辑、AI 配置和测试：

- Manifest、命令、AI tools、偏好与依赖：[`package.json`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/package.json)、[`ai.yaml`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/ai.yaml)、[`CHANGELOG.md`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/CHANGELOG.md)
- 主 UI：[`src/index.tsx`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx)
- 进程和平台实现：[`src/utils/process.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process.ts)、[`src/utils/platform.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts)、[`src/utils/process-grouping.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process-grouping.ts)、[`src/utils/refresh.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/refresh.ts)、[`src/types.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/types.ts)
- 刷新 Hook：[`src/hooks/use-interval.tsx`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/hooks/use-interval.tsx)
- AI tools：[`list-processes.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/list-processes.ts)、[`kill-process.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/kill-process.ts)、[`killall-process.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/killall-process.ts)
- 测试：[`platform.test.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/test/platform.test.ts)、[`process-grouping.test.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/test/process-grouping.test.ts)、[`refresh.test.ts`](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/test/refresh.test.ts)

图标、截图和 lock/config 文件也做了目录清点，但它们不承载运行功能。结论没有使用博客、第三方教程或非官方 API 说明。

### Raycast 官方文档

核对了 [Manifest](https://developers.raycast.com/information/manifest)、[List](https://developers.raycast.com/api-reference/user-interface/list)、[ActionPanel](https://developers.raycast.com/api-reference/user-interface/action-panel)、[Actions](https://developers.raycast.com/api-reference/user-interface/actions)、[Alert](https://developers.raycast.com/api-reference/feedback/alert)、[Toast](https://developers.raycast.com/api-reference/feedback/toast)、[Preferences](https://developers.raycast.com/api-reference/preferences)、[LocalStorage](https://developers.raycast.com/api-reference/storage)、[Environment](https://developers.raycast.com/api-reference/environment)、[窗口与搜索栏](https://developers.raycast.com/api-reference/window-and-search-bar)、[系统工具](https://developers.raycast.com/api-reference/utilities)、[图标与图片](https://developers.raycast.com/api-reference/user-interface/icons-and-images)、[键盘](https://developers.raycast.com/api-reference/keyboard)、[AI Extension 核心概念](https://developers.raycast.com/ai/learn-core-concepts-of-ai-extensions)、[AI 可用性](https://developers.raycast.com/ai/getting-started) 和 [安全/运行时模型](https://developers.raycast.com/information/security)。

### UiPilot 本地资料

以当前源码和合同为准，主要核对：

- [`Public Plugin API v1`](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:1)
- [`第三方插件开发教程`](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-developer-guide.md:1)
- [`TypeScript API v1`](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-api-v1.d.ts:1)
- [`Manifest Schema`](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-v1.schema.json:1)
- [`公开插件 UI 规范`](/D:/code/UiPilot_tools/docs/ui-guidelines.md:1)
- [`PublicPermission` 实现](/D:/code/UiPilot_tools/src-tauri/src/public_plugins/manifest.rs:20)
- [`公开 WebView CSP`](/D:/code/UiPilot_tools/src-tauri/src/public_plugins.rs:87)

另用仓库 CodeGraph 和全文检索检查了 `src/`、`src-tauri/`、`packages/`、`examples/` 与 `docs/`。当前没有面向用户的进程管理/终止功能；匹配到的 `TerminateProcess`、`Stop-Process` 或 `child_process` 用途属于测试进程清理、开发脚本或插件 CLI 构建适配，不是启动器功能，也没有公开给插件。

## Raycast 插件实际做了什么

### 入口与配置

Manifest 同时注册一个 `view` 命令和三个 AI tools：`list-processes`、`kill-process`、`killall-process`，并声明 macOS、Windows 两个平台。[Manifest 第 20–49 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/package.json#L20-L49)

它有十项偏好：自动刷新间隔、是否搜索路径/PID、筛选时是否优先应用、是否显示 PID/路径、终止后关闭窗口/清空搜索/回到根搜索，以及是否跳过确认。[Manifest 第 697–788 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/package.json#L697-L788)

### 进程读取与展示

- macOS 直接执行 `ps -eo pid,ppid,pcpu,rss,comm`；Windows 先用 `Get-Process` 取 PID、名称、Working Set 和路径，再用 `Win32_PerfFormattedData_PerfProc_Process` 补 CPU，并按逻辑核心数归一化。[平台读取命令](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L32-L113)
- UI 初始加载进程；Windows 的 CPU 查询在后台完成并缓存，后续刷新先复用缓存，避免列表先全部显示为 0。[主 UI 第 106–162 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L106-L162)
- 默认按 CPU 降序，可切换按内存降序；排序下拉使用 `storeValue` 持久化。[主 UI 第 167–186、385–407 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L167-L186)
- 搜索始终匹配进程名，可按偏好追加路径、PID 和聚合应用名；每行显示 CPU 百分比与内存，并可选显示 PID/路径。[主 UI 第 335–372、415–466 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L335-L372)
- macOS `.app` bundle 内的主进程与 helper 会聚合为一项，CPU/内存求和并记录子 PID；Windows 路径不匹配 `.app`，所以该聚合逻辑实际只对 macOS 生效。[聚合实现](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process-grouping.ts#L3-L62)
- 自动刷新默认 3000 ms，可设为 0；代码强制有效间隔至少 1000 ms，并在 `background` launch type 下完全禁用刷新。[刷新 Hook](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/hooks/use-interval.tsx#L5-L27)；[后台刷新判定](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/refresh.ts#L1-L3)

### 用户动作

每个进程行提供：

1. Kill
2. Force Kill
3. Restart（存在可启动路径时）
4. Force Restart（存在可启动路径时）
5. Kill All
6. Force Kill All
7. Copy Path（路径存在时）
8. Reload
9. 启用/禁用应用聚合

[ActionPanel 源码第 467–518 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L467-L518)

Kill/Kill All/Restart 默认先弹确认框，并启用 `rememberUserChoice`；用户也可通过偏好完全跳过确认。成功后用 Toast 反馈，立即从本地列表移除结果或重新读取，并按偏好关闭窗口、回根搜索或清空搜索栏。[单进程终止](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L220-L260)；[同名终止与重启](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L262-L333)

### 平台命令差异

| 操作 | macOS | Windows |
| --- | --- | --- |
| 读取进程 | `ps -eo pid,ppid,pcpu,rss,comm` | PowerShell `Get-Process` |
| CPU | `ps` 的 `%CPU` | WMI/CIM 格式化性能计数器，除以逻辑核心数 |
| 内存 | `rss`，KiB | `WorkingSet64 / 1KB`，KiB |
| Kill | `kill -9 PID` | `taskkill /PID PID` |
| Force Kill | `sudo kill -9 PID` | `taskkill /F /PID PID` |
| 终止树 | 递归 `pgrep -P`，普通为 `TERM`，强制为 `sudo ... -9` | `taskkill /T`，强制再加 `/F` |
| Kill All | `killall 'name'`，强制时通过 `sudo` | 编码 PowerShell：`Get-Process -Name ... \| Stop-Process [-Force]` |
| 重启 | 等待退出，`.app/.prefPane` 用 `open`，binary 用 `nohup` | 等待退出后 PowerShell `Start-Process` |

命令构造来源：[终止与 Kill All 第 115–165 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L115-L165)；[重启命令第 168–250 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L168-L250)；[重启流程第 146–188 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process.ts#L146-L188)

两个容易误读的细节：

- macOS 的普通 `Kill` 本身已经发送 `SIGKILL (-9)`；`Force Kill` 的主要额外能力是加 `sudo`，而不是把温和信号升级为 `SIGKILL`。只有“进程树”普通终止使用 `TERM`。[命令源码](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L118-L145)
- 运行代码把 `mem` 当 KiB：Windows 除以 `1KB`，macOS 使用 RSS，UI 再乘 1024 交给 `pretty-bytes`；但 `ai.yaml` 第 35 行写的是 bytes，AI 说明与实际数据单位不一致。[平台解析](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L37-L47)；[UI 转换](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L452-L465)；[AI 说明](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/ai.yaml#L20-L35)

### AI tools

- `list-processes` 支持多个搜索词、任意 `Process` 字段排序和升/降序，找不到结果时抛错。[源码](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/list-processes.ts#L4-L70)
- `kill-process` 按 PID 终止；`killall-process` 按精确进程名终止全部实例。两者都导出 `Tool.Confirmation`，由 Raycast 在执行危险工具前生成人在回路的确认 UI。[单进程 tool](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/kill-process.ts#L30-L77)；[同名 tool](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/tools/killall-process.ts#L17-L53)；[Raycast 官方 Tool/Confirmation 文档](https://developers.raycast.com/ai/learn-core-concepts-of-ai-extensions)
- `ai.yaml` 指示 AI 先列进程、精确匹配名称、默认不杀 helper、按 PID 请求时直接调用 kill tool，并把确认责任交给 tool。[AI 指令](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/ai.yaml#L1-L52)
- 没有 AI 重启 tool；Restart/Force Restart 只存在于可视 UI。

Raycast 官方当前说明 AI Extensions 暂不支持 Windows，因此在 Windows 上可使用这个扩展的可视命令，但不能假定三个 AI tools 同样可用。[AI Getting Started](https://developers.raycast.com/ai/getting-started)

## 插件实际使用的 Raycast/运行时 API

| 类别 | 实际使用 | 用途 | 官方依据 |
| --- | --- | --- | --- |
| 原生列表 UI | `List`、`List.Section`、`List.Item`、`List.Dropdown`、`List.EmptyView` | 搜索、排序、加载/错误/空状态、进程行与 CPU/内存 accessory | [List](https://developers.raycast.com/api-reference/user-interface/list) |
| 动作 UI | `ActionPanel`、`Action`、`Action.CopyToClipboard` | 每行多动作、自定义回调、复制路径、快捷键 | [ActionPanel](https://developers.raycast.com/api-reference/user-interface/action-panel)、[Actions](https://developers.raycast.com/api-reference/user-interface/actions) |
| 确认与反馈 | `confirmAlert`、`showToast`、`Toast.Style` | 破坏性操作确认、成功/失败/刷新反馈 | [Alert](https://developers.raycast.com/api-reference/feedback/alert)、[Toast](https://developers.raycast.com/api-reference/feedback/toast) |
| 偏好与状态 | `getPreferenceValues`、`LocalStorage.getItem/setItem` | 十项用户配置；保存应用聚合开关 | [Preferences](https://developers.raycast.com/api-reference/preferences)、[Storage](https://developers.raycast.com/api-reference/storage) |
| 环境与生命周期 | `environment.launchType`、`clearSearchBar`、`closeMainWindow`、`popToRoot` | 后台启动时停刷；操作后收尾 | [Environment](https://developers.raycast.com/api-reference/environment)、[Window & Search Bar](https://developers.raycast.com/api-reference/window-and-search-bar) |
| 图标与主题 | `Icon`、`Image.ImageLike`、`Color.PrimaryText` | 系统/应用文件图标、动作图标、主题自适应 accessory | [Icons & Images](https://developers.raycast.com/api-reference/user-interface/icons-and-images)、[Colors](https://developers.raycast.com/api-reference/user-interface/colors) |
| 键盘 | `Keyboard.Shortcut.Common` 和手写 shortcut | Copy Path、Reload、Restart、Kill All 快捷键 | [Keyboard](https://developers.raycast.com/api-reference/keyboard) |
| 打开外部目标 | `open` | 打开 macOS sudo/Touch ID 帮助页 | [System Utilities](https://developers.raycast.com/api-reference/utilities) |
| AI 工具 | `Tool.Confirmation`、Manifest `tools`、JSDoc 输入说明 | 暴露自然语言列进程/终止工具并执行前确认 | [AI Extension Core Concepts](https://developers.raycast.com/ai/learn-core-concepts-of-ai-extensions) |
| Node.js，不属于 Raycast UI API | `child_process.exec/execFile`、`fs/promises.access`、`Buffer`、定时器 | 执行 OS 命令、检查重启路径、PowerShell 编码、轮询退出 | [Raycast Security / Runtime Model](https://developers.raycast.com/information/security) |

`@raycast/api` 的 import 清单可在 [`index.tsx` 第 1–18 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/index.tsx#L1-L18) 和 [`platform.ts` 第 1 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L1) 核对；Node import 在 [`process.ts` 第 1–3 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/process.ts#L1-L3) 和两个 kill tool 的第 1 行。

## Raycast 的权限与安全行为

这个扩展的 Manifest 没有声明 Shell、文件或“进程控制”权限。Raycast 官方安全文档解释了原因：扩展运行在 Raycast 管理的 Node.js 进程和独立 V8 isolate 中，但对 Node 文件 I/O、网络等能力没有进一步的策略沙箱。也就是说，`child_process` 是扩展运行时本身的能力，而不是通过 `@raycast/api` 按权限授权的能力。[Raycast Security](https://developers.raycast.com/information/security)

真正的执行边界由操作系统决定：

- macOS Force 操作在命令前加 `sudo`，失败时提示开启 sudo 的 Touch ID/密码交互。
- Windows Force 操作使用 `/F` 或 `Stop-Process -Force`，失败时提示可能需要管理员权限。
- 普通操作也可能因为目标进程已退出、属于其他用户或系统保护而失败。

[平台错误帮助第 375–410 行](https://github.com/raycast/extensions/blob/b2293b093846d374e0b703c832787dd2015800b3/extensions/kill-process/src/utils/platform.ts#L375-L410)

## UiPilot 当前公开插件能力

### 入口和 UI

每个公开插件只能注册一个 slash command、一个隐藏 Runtime，并可选择一种固定输出：

- `mainResult`：0–20 个纯文本结果。
- `window`：一个宿主管理的单例内容窗口。
- `panel`：一个挂载在启动器内的插件 Panel。

[合同总览与 Manifest 约束](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:5)；[响应合同](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:142)

`mainResult` 每项只有 `id/title/subtitle/detail`，可选的唯一动作是宿主持有的 `copyText`；不支持 `actions[]`、自定义标签、回调、链接、命令、HTML/Markdown 或多个动作。[类型定义](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-api-v1.d.ts:74)；[合同限制](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:144)

Panel/Window 能运行插件自己的 HTML/CSS/JS，因此可以画出接近 Raycast 列表的界面。Panel 有 `onUpdate`、有限 Host Key、`focusHostInput()`、`requestHide()`、私有存储和可选剪贴板历史桥；Window 有 `onUpdate`、私有存储、关闭和受控计时器。[Window 类型](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-api-v1.d.ts:113)；[Panel 类型](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-api-v1.d.ts:175)

不过这些内容页不能调用 Tauri、网络或 Shell；Panel 的按键路由也只允许 Manifest 声明的少数键。[开发教程第 571–614 行](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-developer-guide.md:571)

### Runtime 与权限

Runtime API 只暴露：

- 可选的 Host 托管 HTTPS `network.request()`；
- 插件隔离 JSON storage；
- 非 secret 设置读取和 secret 是否已配置；
- Windows 请求绑定的即时/延迟纯文本通知。

[Runtime TypeScript API](/D:/code/UiPilot_tools/docs/plugin-sdk/uipilot-plugin-api-v1.d.ts:15)；[Runtime 合同](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:63)

当前实际可用权限为 `ui.window`、`ui.panel`、`clipboard.write`，以及 Windows 上的剪贴板历史、HTTPS、通知和受控计时器。源码还解析了若干保留名字，但 `is_available` 对它们返回 false，安装会失败；其中同样没有 `process.read`、`process.terminate` 或 `shell`。[权限实现](/D:/code/UiPilot_tools/src-tauri/src/public_plugins/manifest.rs:20)；[公开合同](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:200)

公开 WebView 的 CSP 只允许自身脚本/样式、宿主 IPC 和 `data:` 图片，媒体、对象、frame、form action 均关闭；合同进一步明确不提供浏览器网络、原始 socket、任意文件、原生二进制和 Shell。[CSP 实现](/D:/code/UiPilot_tools/src-tauri/src/public_plugins.rs:87)；[Unsupported in v1](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-v1.md:215)

## 逐项功能差异

| 能力 | Raycast Kill Process | UiPilot 0.3.4 公开插件 | 结论 |
| --- | --- | --- | --- |
| 命令入口 | 一个原生 view command | 一个 slash command | 可映射 |
| AI 入口 | 3 个 tools；列进程、杀单个、杀同名全部 | 无 AI tool 类型或 AI 编排入口 | 缺失 |
| 本机进程枚举 | Node `execFile` 调 `ps`/PowerShell | 无 Shell、文件、原生或进程 API | 阻断 |
| CPU/内存 | macOS `ps`；Windows Get-Process + WMI/CIM | 无进程性能 API | 阻断 |
| 进程分类与应用图标 | 路径判定 app/binary/prefPane；文件图标 | Panel 可自画图标，但拿不到进程路径/系统文件图标桥 | 数据与宿主能力缺失 |
| 搜索/排序 | 名称、可选路径/PID；CPU/内存排序 | Panel 可本地实现；`mainResult` 也可返回静态结果 | UI 可做，数据源缺失 |
| `.app` helper 聚合 | macOS bundle 聚合并汇总 CPU/内存 | Panel JS 可实现算法 | 算法可移植，数据源缺失 |
| 自动刷新 | view 生命周期内 interval，最小 1 秒 | Runtime 无长期后台；Panel 可刷新本地 UI，但不能重新读取 OS 进程 | 无法完成真实刷新 |
| 多行大列表 | 原生 `List` 动态渲染进程 | `mainResult` 最多 20 项；Panel 可自建任意 DOM 列表 | 只能选 Panel 承载 UI |
| 每行多动作 | 9 类上下文动作和自定义回调 | `mainResult` 最多一个 `copyText`；Panel 可自画按钮但不能调用进程控制 | 主结果不匹配；Panel 仍无权限 |
| 终止/强制终止 | OS 命令，权限由 OS 决定 | 无进程控制权限/API | 阻断 |
| 终止树/同名全部 | `pgrep` 递归、`taskkill /T`、`killall`/Stop-Process | 无进程树/按名终止 API | 阻断 |
| 重启 | 终止树、等待最多 5 秒、验证路径、重新启动 | 无终止与进程启动 API | 阻断 |
| 确认 | 原生 `confirmAlert`；AI `Tool.Confirmation` | Panel 可实现 DOM 确认；无宿主通用破坏性动作确认 API | 外观可做，可信执行链缺失 |
| 反馈 | Toast、错误帮助、列表立即更新 | Runtime 有受控消息；Panel 可显示状态；没有等价通用 Toast 回调合同 | 部分可映射 |
| 复制路径 | `Action.CopyToClipboard` | `mainResult.copyText` 在授权后可复制 | 可映射，但拿不到路径 |
| 偏好 | 10 个 Manifest preferences + `getPreferenceValues` | 支持 text/secret/number/boolean/select 设置读取和私有 storage | 大部分可映射 |
| 平台 | 可视命令 macOS + Windows；AI tools 当前官方说明不支持 Windows | 合同含 windows/macos；但 Panel、HTTPS、通知、计时器等多项当前为 Windows-only | 平台组合不同 |
| 安全模型 | Node 能力未做进一步策略沙箱；直接起子进程 | Host allowlist、CSP、精确权限和窄桥 | UiPilot 更严格，不能源码直搬 |
| 依赖/打包 | npm 依赖、React、Node built-ins | 静态包；不支持依赖安装、原生二进制 | 需 bundle 纯 JS，且仍无 OS 权限 |

## 对 UiPilot 的能力判断

### 能直接复用的部分

- `Process` 数据结构、排序、搜索、macOS bundle 聚合等纯算法。
- Panel 的列表布局、CPU/内存 accessory、空状态、加载状态、主题适配和键盘选择模型。
- boolean/number/select 类型偏好与插件私有状态。

UiPilot 的 Panel 设计规范本身支持紧凑、键盘优先的列表工作流，并要求方向键、Enter、Escape、Tab 和 Light/Dark 对等；视觉层不是主要障碍。[Panel 布局规范](/D:/code/UiPilot_tools/docs/ui-guidelines.md:128)；[键盘规范](/D:/code/UiPilot_tools/docs/ui-guidelines.md:172)；[公开插件规则](/D:/code/UiPilot_tools/docs/ui-guidelines.md:191)

### 当前不能由公开插件完成的部分

以下四项都是硬阻断，缺一不可：

1. 枚举本机进程及父子关系。
2. 读取 CPU、Working Set/RSS 和可执行路径。
3. 以 PID、名称或树为范围终止进程，并区分普通/强制权限。
4. 验证并重新启动可执行文件或应用 bundle。

公开插件文档明确要求遇到未覆盖需求时不要绕过 WebView/Tauri 边界，而应等待宿主公开版本化 API。[开发教程第 887–898 行](/D:/code/UiPilot_tools/docs/plugin-sdk/public-plugin-developer-guide.md:887)

### 最小产品方向（仅作为差异结论，不是实现方案）

- 若功能只服务 UiPilot 本体：做宿主内置功能最直接，由 Rust/平台层持有进程权限，前端只接收经过校验的 DTO 和动作结果。
- 若要让第三方插件实现：需要新的、版本化的宿主管理能力，至少分离只读的进程枚举/性能权限和高风险的终止/重启权限；确认、PID 复用、权限提升、系统进程保护和审计都必须由 Host 定义，不能开放通用 Shell。
- 在 API 出现前，公开插件最多只能做演示数据或远程主机进程面板，不能声称管理本机进程。

本报告只做源码与能力对比，没有修改产品代码，也没有尝试绕过当前插件安全边界。
