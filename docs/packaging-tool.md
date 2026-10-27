# Windows 可视化打包工具

在仓库根目录双击 **Pack.cmd**，打开 Windows PowerShell 5.1 / WinForms 窗口。无需额外 GUI 依赖。窗口显示固定仓库路径及当前应用版本，提供 Setup 安装包和插件包两个页签。可修改公共输出根目录，默认 `artifacts/packages`。

## 使用

1. Setup 页固定生成生产 Release / Windows x64 / NSIS 安装包。插件页可选择 `examples/public-plugins/*/package`，也可浏览外部含 `plugin.json` 的 package 目录；ID 和版本会显示在窗口中。
2. 选择输出目录，点击“开始打包”。运行时可以滚动、选择和复制日志，查看当前阶段与耗时；不会显示估算百分比。
3. 成功后点击“打开输出目录”。每次运行有唯一子目录，保留 `request.json`、`run.log`、`status.json`、`result.json` 和产物。成功结果记录产物的 SHA256。失败和取消也保留诊断目录，不覆盖或清理之前的产物。

同一窗口只运行一个任务。点击“取消”会终止该窗口拥有的 worker 及子进程；其他 node/cargo 进程不受影响。运行中关闭窗口会先取消，确认本次进程树全部退出后再关闭。取消状态不会显示成功；可在退出后重新打开重试。强制结束 GUI 进程时，Windows Job Object 也会终止其拥有的构建进程。

输出根必须在插件源目录之外，且输出路径及祖先必须是普通目录，不能是 junction、符号链接或其他 reparse point。支持中文、空格、`&` 等合法路径字符。产物每次使用新目录，已有文件不会覆盖。

## 工具链与固定流程

预先安装 Node.js 22 LTS（插件 CLI 最低 Node 20）及 npm，并在仓库执行 `npm ci`。工具不会自动安装或修复依赖。npm 必须能在 `npm.cmd` 相邻的 `node_modules/npm/bin/npm-cli.js` 找到本地入口（标准 Node.js 安装布局）。

Setup 还需要 Windows x64、64 位 PowerShell、仓库 `rust-toolchain.toml` 指定的 Rust/MSVC 工具链、Windows C++ 构建工具及仓库 Everything 资源。首次构建可能由既有工具链下载 Rust/NSIS 依赖，需要网络和时间。工具只构建，不运行 Setup、不安装服务、不请求提权；签名、干净 VM 安装/升级/卸载不属于此工具的验证保证。

Setup 调用现有入口：

```powershell
npm run tauri -- build --bundles nsis
```

正常执行 `beforeBuildCommand` 前端构建和生产 Everything Helper 构建，以及 `beforeBundleCommand` 生产暂存钩子。worker 仅在自身进程清除继承的 `CARGO_TARGET_DIR`、`CARGO_BUILD_TARGET`、`TAURI_CONFIG` 等构建覆盖，不改全局环境。仅接受 `src-tauri/target/release/bundle/nsis` 下的 `UiPilot_tools_<当前版本>_x64-setup.exe`，并核对构建前后证据及生成时间；旧版本、未更新的同版本文件和另行命名的 VM 产物不会被复制为本次成功结果。

插件依次执行以下流程，任一步退出非零立即停止；不执行插件 Runtime：

```powershell
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate <package> --platform windows --json
node packages/plugin-cli/dist/cli.mjs pack <package> --output <本次目录/pluginId-version.uipilot-plugin> --platform windows --json
node packages/plugin-cli/dist/cli.mjs validate <归档> --platform windows --json
```

最终成功同时要求 worker 退出码为 0、本次运行 ID 对应的成功结果、预期产物存在且 SHA256 一致；插件还必须通过归档校验。用户路径通过 JSON 和原生参数传递，不拼成可执行脚本。

如果窗口无法启动，入口会提示错误，诊断文件位于 `%TEMP%\uipilot-package-startup.log`。其他错误请查看本次输出目录的 `run.log` 与 `result.json`；修复缺失依赖或目录后重试。

## 验证与人工验收

自动化聚焦检查（不启动 GUI、不构建或安装真实 Setup）：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-package-tool.ps1
```

覆盖 fake command 流程与失败短路、路径/安全文件名、独立输出、旧 Setup、SHA256、取消状态及退出码核对；真实无害子进程验证原生参数和 Job Object 后代取消，另有 PowerShell 5.1 解析与 UI wiring 静态检查。这些检查不能代替窗口交互和真实 Setup 生成。

人工验收三项：

1. 双击 `Pack.cmd`，调整窗口大小，用 Tab 和 Alt 快捷键操作两个页签与目录选择，核对版本/插件信息、可复制日志及缺依赖提示；运行中取消或关闭，确认进程退出后才允许继续。
2. 选择已知参考插件，使用含中文、空格、`&` 的源/输出路径，打包成功后核对 `result.json` 的 SHA256；在 UiPilot 插件设置选择生成的归档进行本地安装与功能确认。再打一次应进入不同目录并保留前次归档。
3. 在准备完整的 Windows 构建环境点击 Setup，确认日志执行现有生产 hooks、输出当前版本安装包及 SHA256。仅生成并检查产物，不自动运行安装程序。安装/服务/VM 验收另行安排。
