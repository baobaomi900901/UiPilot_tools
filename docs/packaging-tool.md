# 终端交互打包

在项目根目录运行 `.\Pack.cmd`，也可以双击该文件。仅使用终端菜单，不启动桌面打包窗口。

```text
UiPilot 打包工具
1. 主程序 Setup 包
2. 插件
0. 退出
请输入编号:
```

- 输入 `1`：直接打包生产 Release / Windows x64 / NSIS Setup。
- 输入 `2`：显示 `examples/public-plugins/*/package` 下插件的名称、ID、版本，再输入编号，只打包选中的插件；输入 `0` 取消。
- 默认显示当前阶段、阶段耗时及去重后的简短警告；长步骤每 15 秒提示仍在执行。Rust 长步骤还显示最近的编译目标和无新输出时长；等待 Cargo 目录锁时会单独提示。Ctrl+C 取消。结束后显示总耗时、产物路径、大小和 SHA256，通过 `Pack.cmd` 启动时按任意键关闭。
- 原始构建输出始终完整保存在 `run.log`。失败时额外展示最后 18 行输出；需要实时查看全部细节时加 `--verbose`，例如 `uv run scripts/package.py --verbose` 或 `.\Pack.cmd --verbose`。

固定产物位置（相对于项目根目录）：

| 类型 | 位置 |
| --- | --- |
| 主程序 | `artifacts/packages/setup/UiPilot_tools_<版本>_x64-setup.exe` |
| 插件 | `artifacts/packages/plugins/<pluginId>-<版本>.uipilot-plugin` |
| 每次运行日志 | `artifacts/packages/.runs/<本次任务>/run.log` |

同名文件自动覆盖，无需确认。先在内部任务目录生成并校验新包，再替换固定位置的旧包；构建或校验失败时保留旧包。不同版本文件名不同，不自动删除旧版本。内部任务目录保留构建产物和诊断文件。

## 环境与流程

打包器需要 Python 3.10+，仅使用标准库，无需安装 Python 第三方包。`Pack.cmd` 优先使用 `py -3`，否则使用 PATH 中的 `python`。还需要 Node.js/npm，以及在项目运行过 `npm ci`。Setup 还需要 Windows x64、Windows PowerShell 5.1（现有构建钩子使用）、仓库指定的 Rust/MSVC 工具链、C++ 构建工具及 Everything 资源。首次 Setup 构建可能下载工具链依赖。

Setup 复用 `npm run tauri -- build --bundles nsis` 和既有生产 Everything Helper 钩子；只生成安装包，不运行安装器。插件依次执行当前 CLI 构建、源码校验、pack 和归档校验，不执行插件 Runtime。worker 仅清除自身的构建环境覆盖，不修改全局环境。取消或关闭终端会通过 Job Object 清理本次 worker 及后代，不按进程名结束其他任务。

直接运行脚本（无结尾按键暂停）：

```powershell
uv run scripts/package.py
# 或使用已安装的 Python
python -X utf8 scripts/package.py
```

聚焦检查：

```powershell
python -X utf8 -m unittest discover -s scripts -p 'test_package*.py' -v
node --test scripts/test-preserve-build-output.mjs
```

维护入口：`scripts/package.py` 包含菜单、插件发现、构建与产物发布；`scripts/package_job.py` 仅封装 Windows Job Object 进程生命周期；`scripts/package_progress.py` 处理终端摘要。`Pack.cmd` 只负责启动 Python 和结束后的按键暂停。原有项目构建钩子继续复用。

重复打包加速：Vite 仍执行类型检查和完整前端构建、正常清理旧资源。`scripts/preserve-build-output.mjs` 作为 Vite 插件，在构建前后比较文件内容，仅为同路径且字节完全相同的文件保留旧修改时间，避免 Cargo 因重写相同资源而重复编译嵌入前端的 Rust 主程序。内容变化、新增文件、Rust 源码或工具链变更仍由正常构建流程处理；首次构建不保证加速。Everything 助手身份检查、生产与测试产物隔离、NSIS 构建和最终哈希校验均保留。

2026-10-27 本机重复 Setup 打包对照：未改业务源码，优化前约 175.6 秒，优化后约 89.5 秒（减少约 49%）；主程序 Cargo 阶段由 2 分 04 秒降到 0.91 秒。两次均生成并发布完整 Setup，未运行安装器。这是单机单次对照，工具启动、磁盘、杀毒扫描和 NSIS 压缩开销会使总耗时波动。原始记录位于本机忽略目录 `.runs/setup-20261027-231450-0fd07a40ebd54f19be2eeaaa9c93e698` 和 `.runs/setup-20261027-232132-b4878ebe39674f85b6c533731c77df10`（完整前缀为 `artifacts/packages/`）。

人工验收：选一个插件打包两次，确认固定目录中的同名文件更新；选择 `1` 确认生成 Setup。真实 Setup 生成与安装验收分开执行。

此交互以用户 2026-10-27 的终端菜单及同名覆盖要求为准，替代此前桌面 GUI 和每次独立对外交付目录的方案。旧设计文档仅保留作为历史记录。

## Everything 助手构建边界

`installer-helper-build` 仅用于显式构建独立安装助手。该模式不编译宿主库的业务模块，避免为助手重复优化截图、插件运行时及启动器代码。普通主程序与 Setup 的主程序阶段不启用该特性。首次构建助手仍可能需要编译 Cargo 包所声明的依赖；随后构建复用独立目标目录的依赖缓存。

## Windows 插件校验

插件 CLI 在一次目录读取、归档读取或输出目录检查中复用同一个 Windows 属性检查进程，结束或失败时关闭。每次读取前后的属性检查仍然实时执行，不缓存属性结果；重解析点拒绝、文件身份检查、协议大小限制和单次请求超时保持有效。插件打包不会构建主程序前端。

## Windows 插件校验

插件 CLI 在一次目录读取、归档读取或输出目录检查中复用同一个 Windows 属性检查进程，结束或失败时关闭。每次读取前后的属性检查仍然实时执行，不缓存属性结果；重解析点拒绝、文件身份检查、协议大小限制和单次请求超时保持有效。插件打包不会构建主程序前端。
