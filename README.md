# UiPilot

Windows x64 启动器与本地公开插件宿主（当前版本 0.19.0）。宿主为 Tauri 2、React 和 Rust；公开插件使用受控的 v1 Runtime / Panel / Window API。

## Apple Silicon 开发

Apple Silicon 内测包已在 M1 Pro / macOS 15.7.4 上验证从 DMG 安装、快捷键唤起与输入；核心启动器已验证应用搜索、启动和激活。按本轮安排跳过更高系统和更新芯片的实机测试。Windows 保留公开插件宿主，Mac 内测包目前只开放核心启动器。详见[设备验收记录](docs/releases/2026-11-06-desktop-port-device-checks.md)和[插件兼容性说明](docs/plugin-sdk/public-plugin-developer-guide.md#windows-与-macos-兼容性)。

在 arm64 的 Node 22 环境运行 `npm ci`，再运行 `npm run dev` 预览前端，或用 `npm run build` 检查 TypeScript 并构建页面。Mac 原生入口为 `npm run tauri -- dev`，要求 aarch64-apple-darwin Rust 1.96.1 和 macOS SDK。Mac 核心宿主与前端已可一起打包。内部测试包使用本机临时签名：

```sh
npm run tauri -- build --target aarch64-apple-darwin --bundles app,dmg --config src-tauri/tauri.macos-internal.conf.json -- --locked
node scripts/verify-macos-bundle.mjs
```

校验脚本只读挂载本次 DMG，检查签名、arm64 架构、最低系统版本及资源，并输出 SHA-256。内部包用于本机测试；公开发布另需 Developer ID 签名与公证。前端预览不覆盖原生快捷键、窗口焦点或 IPC。

### 文件搜索预览

内部 Apple Silicon 文件搜索预览包可在主窗口输入 `/find 文件名` 并回车。默认搜索公共字体目录和本机共享目录 `/Users/Shared` 中已被 Spotlight 索引且允许读取的项目。要搜索本地「文稿」，点击「授权并记住文稿」，在系统窗口选中当前账户的「文稿」文件夹并确认。Find 会用原关键词重新搜索，包括子目录。显示「停止并忘记文稿」即表示授权已保存；退出并重新打开同一个安装包，首次搜索会自动恢复。点击该按钮可移除保存的授权。若只需本次使用，在 ⌘K 操作菜单中选择「仅本次搜索文稿」。

选中文件行后按回车可在 Finder 中定位文件，选中文件夹则会打开它。支持的普通文档另有「打开文件」操作。正式目标是免配置搜索系统已索引、UiPilot 可访问的本地文件。全系统范围仍待签名身份与系统隐私授权验收，常规构建继续关闭 Find。

使用以下命令构建并验证支持记住文稿的内部包。只有主程序携带应用专属书签权限。更换签名或安装新版本后可能需要重新授权。发布状态与测试边界见[文件搜索设计](docs/superpowers/specs/2026-11-06-macos-file-search-design.md)。

```sh
npm run tauri -- build --features macos-find-preview --target aarch64-apple-darwin --bundles app,dmg --config src-tauri/tauri.macos-internal.conf.json --config src-tauri/tauri.macos-find-documents.conf.json -- --locked --no-default-features
node scripts/verify-macos-bundle.mjs --documents
```

## 工具链与开发

安装 Node.js 22 LTS（CLI 最低要求 Node 20）、npm、Rust stable / Cargo、Windows MSVC C++ 构建工具与 WebView2。首次准备依赖运行 `npm ci`；Rust 离线验证需要本地已有 Cargo 依赖缓存。验证不会下载依赖或修复生成文件。

```powershell
npm run dev:vite   # 仅浏览器前端预览
npm run dev        # Windows 完整开发入口（检查 Everything 运行条件并启动 Tauri）
npm run build      # TypeScript 检查与 Vite 构建
npm run tauri -- build # 原生应用构建
```

完整开发入口需要已准备的 Everything.exe 及可用服务；请参阅 `scripts/dev-with-everything.ps1`。验证不安装或修改服务。安装器构建入口为 `npm run build:managed-everything`，其系统与 VM 验收独立进行。

## 终端交互打包

Windows 下安装 Python 3.10+ 后，在终端运行 **.\Pack.cmd**（也可双击），或直接执行 `python -X utf8 scripts/package.py`：输入 `1` 打包主程序 Setup，输入 `2` 后从列表选择一个插件打包。打包器仅使用 Python 标准库。产物固定在 `artifacts/packages/setup` 和 `artifacts/packages/plugins`，同名文件在新包成功后自动覆盖。准备条件和使用方法见[打包工具说明](docs/packaging-tool.md)。

## 原生 CI 验证

[Desktop port verification](.github/workflows/desktop-port.yml) 由维护者手动触发。Mac runner 需要 `self-hosted, macOS, ARM64, uipilot-build` 标签、原生 Node 22、仓库指定的 Rust 和 macOS SDK。Windows runner 需要 `self-hosted, Windows, X64, uipilot-provider` 标签、Node 22、MSVC/Rust、NSIS 与现有 Windows 构建依赖，另需备妥独立 Provider 源码（通过 `UIPILOT_RUST_UIA_ROOT` 指定，或使用既有位置）。两个 runner 均需能取得锁定依赖。工作流构建本地测试包并检查，不发布。本轮实机结果按 M1 最低系统与 Windows x64 分列记录。

## 验证

```powershell
npm run verify:local -- --list # 只列出阶段与实际发现数量
npm run verify:local           # 全仓本地门禁
npm run verify                 # 同义入口
```

九个阶段依次为：协调器测试、前端测试、前端构建、Rust 库离线测试、manifest Schema 只读检查、插件 CLI 完整 verify、全部公开插件 Node 测试、全部 SDK contract 类型检查、全部参考 package 静态校验。任一失败立即退出非零。公开示例递归发现并稳定排序，缺失、空集合或符号链接会失败；不自动更新 Schema，也不执行真实插件 Runtime 或请求真实 API。

聚焦检查（从仓库根目录运行）：

```powershell
node --test scripts/verify-local-foundation.test.mjs
npm test                         # 原有 Vitest 语义
npm test -- src/protocol.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema -- --check
npm run verify --workspace @uipilot/plugin-cli
node --test examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.note/tests/sdk-contract.ts
```

部分 React 测试存在已知 `act(...)` 与 Ant Design 警告，应保留并结合断言结果检查。浏览器预览和单元测试不能替代真实 Windows 宿主的焦点、键盘、双主题、剪贴板粘贴、安装/升级/卸载以及 Everything 服务/安装器 VM 验收。

## 源码导航

| 位置 | 职责 |
| --- | --- |
| `src/` | React 启动器、设置及前端状态 |
| `src-tauri/src/` | Rust 宿主、系统集成与公开插件运行时 |
| `packages/plugin-cli/` | 独立本地插件打包与静态校验 CLI |
| packages/plugin-ui/ | 随插件离线分发的 List、Detail、Form、ActionPanel 组件 |
| `examples/public-plugins/` | 可测试的本地插件参考包 |
| `docs/plugin-sdk/` | 公开插件教程、API、类型和 Schema |
| `docs/ui-guidelines.md` | 宿主及插件 UI 规范 |
| `scripts/` | 验证、开发和安装器辅助入口 |

## 本地插件流程

先读[公开插件教程](docs/plugin-sdk/public-plugin-developer-guide.md)和 [UI 规范](docs/ui-guidelines.md)，选择最接近的参考示例，保持 `tests/` 与实际安装的 `package/` 分离。API 合同见 [v1 规范](docs/plugin-sdk/public-plugin-v1.md)、[类型定义](docs/plugin-sdk/uipilot-plugin-api-v1.d.ts)和 [Schema](docs/plugin-sdk/uipilot-plugin-v1.schema.json)。

```powershell
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-panel/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.demo-panel/package --output ./demo-panel.uipilot-plugin --platform windows
node packages/plugin-cli/dist/cli.mjs validate ./demo-panel.uipilot-plugin --platform windows
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-return
```

在 UiPilot 插件设置中选择 `package` 目录或生成的 `.uipilot-plugin` 产物进行本地安装。`pack` 必须显式指定源目录外的输出路径，后缀严格为 `.uipilot-plugin`，父目录须已存在且祖先无链接或 reparse point；输出文件系统须支持硬链接（Windows 通常为 NTFS）。已有目标不会覆盖，没有 `--force`，再次打包请换文件名。第三方开发者可安装提供的 npm tarball，使用同样的 `uipilot-plugin pack` / `validate` 命令，详见 [CLI README](packages/plugin-cli/README.md)。运行示例自身 Node 测试与 SDK contract 检查后，在真实宿主确认权限提示、打开/关闭/重载和数据持久化。

本仓库路线以本地包为范围；在线商店、在线发布、自动下载和云同步不属于当前基座。真实凭据请求、系统安装和服务变更需要单独的人工验收环境。

Windows Host `0.4.0+` 支持将已授权的 secret 作为精确 HTTPS host 的 bearer 引用消费，Runtime 仅提交 credential ID。参见 [credential demo](examples/public-plugins/com.uipilot.demo-credentials/README.md) 与 [SDK 合同](docs/plugin-sdk/public-plugin-v1.md#host-managed-https)。示例测试仅使用合成值和离线 fake provider；服务端回显、配置保存失败的非回滚与后续核验恢复边界必须按合同处理。CLI npm 包版本独立保持 `0.1.0`，校验目标 Host 为 `0.14.0`。


## 0.14.0 本地插件基座交付

Host 与 CLI 校验目标为0.14.0，CLI npm包版本独立保持0.1.0。已交付本地打包安装、多命令与统一结果动作、逐命令全局快捷键、普通设置及凭据消费、Panel/Window到Runtime调用、标准插件UI、浏览器链接、选定文本导入、授权粘贴、升级卸载及恢复诊断。各切片核心人工验收和最终代表组合测试已通过，详见[交付清单与已知限制](docs/local-plugin-foundation-delivery.md)。

开发者从[公开插件教程](docs/plugin-sdk/public-plugin-developer-guide.md)开始，选择最接近的参考插件：

| 场景 | 示例 |
| --- | --- |
| 多命令、结果动作、共享存储 | [文本片段](examples/public-plugins/com.uipilot.text-snippet/README.md) |
| 面板表单、预览、标准UI | [文本模板](examples/public-plugins/com.uipilot.templates/README.md) |
| 独立窗口与用户选定文本导入 | [JSON工作台](examples/public-plugins/com.uipilot.json-workbench/README.md) |
| 实际Host认证HTTPS | [认证链路验收](examples/public-plugins/com.uipilot.auth-check/README.md) |
| SDK组件复用 | [Plugin UI](packages/plugin-ui/README.md) |

公开插件保持受控v1合同；能力最低Host版本、权限及平台要求以SDK和manifest为准。旧单命令包保持兼容。数据schema升级后的目录不保证可由更旧Host读取，不提供任意插件数据的自动迁移或旧schema回写。

临时测试副本与中间产物不纳入Git；测试夹具和插件package内的分发代码属于源码，必须保留。已验收程序、插件包及原始构建记录保存在本机忽略目录 .superpowers/deliveries/，远端源码不包含这些交付二进制。可审计的版本、文件大小和SHA256清单见[交付文件清单](docs/releases/local-plugin-foundation-0.14.0-artifacts.json)。

普通Enter次数问题按用户要求暂缓；安装器/干净VM发行验收独立安排。在线商店、在线安装发布和云同步不属于本轮范围。
