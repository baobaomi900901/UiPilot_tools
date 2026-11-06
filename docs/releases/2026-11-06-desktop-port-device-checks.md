# UiPilot 0.19.0 双设备验收单

较新 Apple Silicon Mac 的测试者验证同一份 DMG，Windows x64 的测试者从同一产品提交构建 NSIS 安装包。请在测试账户或可恢复的干净环境中运行。安装、切换前台、注册快捷键、睡眠和卸载由设备使用者亲自确认和操作。

| 核对项 | 固定值 |
| --- | --- |
| 产品提交 | `a79f612f99c31b4bb081fbdb04eb71ac92b7c761` |
| 两台 Mac 使用的文件 | `UiPilot_tools_0.19.0_aarch64.dmg` |
| DMG SHA-256 | `3c640e2b8da704d438f59afcb437cc06509273b1bcff05e792e44e04cede77a3` |
| 包内主程序 SHA-256 | `5656161a61cb60c9480023ddd4f94b28e3711785ad8e5aeaff7e5a608df02b6d` |
| Windows 可选源码档案 | `UiPilot-source-a79f612.zip`，SHA-256 `47970c7579ee4c381afc90092d620122f84d67760eaead827ebe19b7d75ff673` |

此 DMG 为内部 ad-hoc 签名候选。M1 Pro / macOS 15.7.4（24G517）已从单独的候选 .app 验过唤起、搜索、启动、激活、隐藏和退出。最低系统设备的 DMG 安装、升级等未测项目请单独补记。[已有记录](../superpowers/plans/2026-11-06-macos-port.md)

2026-11-06，测试者在 M1 Pro（arm64，macOS 15.7.4，24G517）上对手动传输的候选 DMG 运行预检。SHA-256 与上表一致，脚本报告 `Preflight passed`。本次覆盖最低系统设备的包校验，从“应用程序”启动的结果待回传。

## 较新 Mac

较新 Mac 实机行不需要安装 Node、Rust 或 Xcode。

1. 在 **M1 系列之后的 Apple Silicon** 和高于 15.7.4 的实际系统上，准备上表中的 DMG 及随交接工具提供的 `preflight-macos-device.sh`。运行 `sh preflight-macos-device.sh "/实际目录/UiPilot_tools_0.19.0_aarch64.dmg"`。保留芯片、OS/build 和哈希结果。若哈希不符，停止该行；另行构建的相同版本 .app 不能用于这行。
2. 从 Finder 打开已核验的 DMG，按正常流程复制到“应用程序”，从安装位置启动。记录系统安全提示和能否打开。若 Gatekeeper 阻止运行，按实际结果记录；保持系统安全设置，不移除下载隔离属性。
3. 依次用菜单栏和普通快捷键（初始候选为 Option+Space）打开启动器；直接输入中英文应用名，Enter 启动计算器，再次唤起并激活同一已运行应用。测试 Esc、切换到其他应用后自动隐藏、再次输入、中文输入法确认候选，以及从菜单退出后重新启动。
4. 在设置中换主题和测试组合键，退出再启动后验证设置和真实快捷键。测试一次与已占用组合键冲突，原键应仍能唤起；初始键不可用时应能从菜单进入设置。使用者自行操作睡眠/唤醒；有第二台显示器时再测试跨屏和拔屏。记下未覆盖的设备条件，恢复测试用快捷键。
5. **覆盖升级单列。** 需要同一产品标识的、可核验的不同旧版本 .app。先记录旧包版本/哈希，设置非默认主题或组合键并退出，换入固定 DMG 内的候选，再检查数据和快捷键。若只有这一版，记录“缺旧版，升级待验”。重装同一文件仅记为同版本替换。最后退出并卸载测试副本，核对常驻进程退出、菜单图标消失、组合键释放。

## Windows x64

2026-11-06，测试者在原生 Windows x64 的 VS x64 开发者环境中复检通过。候选源码、Everything 及打包输入通过，MSVC 编译器、链接器和 Windows SDK 工具均已找到。独立 Provider 为 `0bb397e0f9ea0caf1a05d25193f44a364c8de5dc`。NSIS 构建、安装实测和 WebView2 启动验证待进行。

同日，固定源码包 `a79f612` 已完成 npm 安装和 Cargo 依赖下载。首次打包在 `tauri.ps1` 接收 `build` 参数时报告 `PositionalParameterNotFound`。使用该源码包时，从源码根目录运行 `python scripts/provider_build.py build-session -- build -- --locked` 执行下方第 3 步的打包命令；此入口会建立同一受管构建会话并执行原有钩子。

仅做安装实测时，直接使用 Windows 构建者交付的 NSIS 与哈希，从第 4 步开始；测试机无需安装 Rust、Node 或独立 Provider 源码。

**交接工具与候选源码分开存放**。从工具目录取 `scripts/preflight_windows_port.py`，该文件可在仓库外单独运行。维护者提供独立 Provider 源码；如有批准的 Provider commit，可加 `--provider-commit <完整 SHA>`。

1. 在 VS 的 **x64 Native Tools / Developer PowerShell** 中准备 Windows PowerShell 5.1、原生 Python 3.10+、Git、x64 Node 22.13+（22.x）或 24+、Rust 1.96.1 MSVC x64、C++ 工具和 Windows SDK。将 `$env:UIPILOT_RUST_UIA_ROOT` 设为本机独立 Provider checkout。预检沿用产品的路径优先级，不打印路径或环境变量。使用独立测试机/账户，不用个人凭据测试插件。
2. 有 Git checkout 时，在干净的候选源码目录运行下方第一条。无仓库账户时，先用 `Get-FileHash -Algorithm SHA256 .\UiPilot-source-a79f612.zip` 核对上表 ZIP 哈希，用支持长路径的解压器完整解压，进入其 `UiPilot_tools` 子目录，**在安装依赖前**运行第二条（任选一种）。
   ```powershell
   python -B C:\交接工具\preflight_windows_port.py --repo .
   python -B C:\交接工具\preflight_windows_port.py --repo . --source-zip C:\交接工具\UiPilot-source-a79f612.zip
   ```
   预检只读取源码、版本、外部 Provider revision 和必要资源；`BLOCKED` 或退出码 2 表示需补齐对应项。归档校验会逐文件比对原 ZIP，允许单独的 `.uipilot-local.json` 配置。通过后记录产品与 Provider 的完整 revision。
3. **构建 / CI 行。** 从干净输出运行以下命令，任一步非零即停止。先由受管构建生成 Provider 和 Helper，再跑 Rust/前端门禁；保留构建日志、NSIS 路径及哈希。首次运行需要能取得锁定依赖和 Tauri 的 NSIS 工具。也可由具备相同源码的自托管 runner 执行手动工作流。工作流没有安装或更新测试者设备。[工作流](../../.github/workflows/desktop-port.yml)
   ```powershell
   npm ci
   if ($LASTEXITCODE) { throw "npm ci failed" }
   cargo fetch --locked --manifest-path src-tauri/Cargo.toml
   if ($LASTEXITCODE) { throw "cargo fetch failed" }
   npm run tauri -- build -- --locked
   if ($LASTEXITCODE) { throw "Windows bundle failed" }
   npm run verify:local
   if ($LASTEXITCODE) { throw "Windows verification failed" }
   Get-FileHash -Algorithm SHA256 '.\src-tauri\target\release\bundle\nsis\UiPilot_tools_0.19.0_x64-setup.exe'
   ```
   若设置了独立 target 目录，用构建实际报告的本次 NSIS 路径计算哈希；不要取历史安装器。核对受管 Helper、Provider 清单/哈希、Everything 和 NSIS hooks 的构建记录。
4. **Windows 实机行。** 在干净、获准安装的 x64 环境核验同一 NSIS 的 SHA-256 再安装，记录 WebView2 是否可用、是否要求管理员权限和安装结果。验证托盘/快捷键唤起、应用与文件搜索并打开、插件授权的允许及拒绝（只用测试插件）、设置重启保留、退出释放快捷键。升级需从明确的旧版本安装包覆盖，记录两包版本和哈希；卸载后复核常驻进程及受管资源清理。仅构建成功时只填写构建行。

## 回传结果

每个操作填 **通过 / 失败 / 待测**；失败附复现动作、实际结果和脱敏日志位置。仅记录芯片、系统版本/build、产品与 Provider revision、对应包哈希、签名状态、测试人和日期，不记录设备序列号、用户名、密钥或个人应用数据。

| 设备或阶段 | 分别回传的证据 |
| --- | --- |
| 较新 Mac | DMG 哈希、干净安装、核心交互、持久化、睡眠/多屏、旧版覆盖升级、卸载 |
| Windows 构建 / CI | 产品与 Provider revision、各门禁退出码、NSIS 哈希、资源核验、日志位置 |
| Windows 实机 | 安装包哈希、安装与权限、应用/文件搜索、插件授权、重启、退出、升级、卸载 |
| M1 Pro 最低系统 | 同一 DMG 哈希和新增的安装/升级/卸载等记录；保留原有交互结果 |

缺少必需设备行时，双平台发布验收维持待完成。公开分发另需经授权的 Developer ID 签名、公证、staple 和带隔离属性的安装证据。
