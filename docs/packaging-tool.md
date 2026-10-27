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
- 执行期间显示实时日志；Ctrl+C 取消。结束后显示产物路径和 SHA256，按任意键关闭。

固定产物位置（相对于项目根目录）：

| 类型 | 位置 |
| --- | --- |
| 主程序 | `artifacts/packages/setup/UiPilot_tools_<版本>_x64-setup.exe` |
| 插件 | `artifacts/packages/plugins/<pluginId>-<版本>.uipilot-plugin` |
| 每次运行日志 | `artifacts/packages/.runs/<本次任务>/run.log` |

同名文件自动覆盖，无需确认。先在内部任务目录生成并校验新包，再替换固定位置的旧包；构建或校验失败时保留旧包。不同版本文件名不同，不自动删除旧版本。内部任务目录保留构建产物和诊断文件。

## 环境与流程

需要 Windows PowerShell 5.1、Node.js/npm，以及在项目运行过 `npm ci`。Setup 还需要 Windows x64、仓库指定的 Rust/MSVC 工具链、C++ 构建工具及 Everything 资源。首次 Setup 构建可能下载工具链依赖。

Setup 复用 `npm run tauri -- build --bundles nsis` 和既有生产 Everything Helper 钩子；只生成安装包，不运行安装器。插件依次执行当前 CLI 构建、源码校验、pack 和归档校验，不执行插件 Runtime。worker 仅清除自身的构建环境覆盖，不修改全局环境。取消或关闭终端会通过 Job Object 清理本次 worker 及后代，不按进程名结束其他任务。

直接运行脚本（无结尾按键暂停）：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-console.ps1
```

聚焦检查：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-package-tool.ps1
```

人工验收：选一个插件打包两次，确认固定目录中的同名文件更新；选择 `1` 确认生成 Setup。真实 Setup 生成与安装验收分开执行。

此交互以用户 2026-10-27 的终端菜单及同名覆盖要求为准，替代此前桌面 GUI 和每次独立对外交付目录的方案。旧设计文档仅保留作为历史记录。
