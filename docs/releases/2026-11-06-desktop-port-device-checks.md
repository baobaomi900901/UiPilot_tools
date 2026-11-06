# UiPilot 0.19.0 双设备验收单

本轮设备为 M1 Pro 和 Windows x64。2026-11-06，因无较新 Apple Silicon 和更高版本 macOS 设备，按用户决定跳过该设备行；该组合未实测，本轮验收无需等待。请在测试账户或可恢复的干净环境中运行。安装、切换前台、注册快捷键、睡眠和卸载由设备使用者亲自确认和操作。

| 核对项 | 固定值 |
| --- | --- |
| 产品提交 | `a79f612f99c31b4bb081fbdb04eb71ac92b7c761` |
| 两台 Mac 使用的文件 | `UiPilot_tools_0.19.0_aarch64.dmg` |
| DMG SHA-256 | `3c640e2b8da704d438f59afcb437cc06509273b1bcff05e792e44e04cede77a3` |
| 包内主程序 SHA-256 | `5656161a61cb60c9480023ddd4f94b28e3711785ad8e5aeaff7e5a608df02b6d` |
| Windows 可选源码档案 | `UiPilot-source-a79f612.zip`，SHA-256 `47970c7579ee4c381afc90092d620122f84d67760eaead827ebe19b7d75ff673` |

此 DMG 为内部 ad-hoc 签名候选。M1 Pro / macOS 15.7.4（24G517）已从单独的候选 .app 验过唤起、搜索、启动、激活、隐藏和退出。[已有记录](../superpowers/plans/2026-11-06-macos-port.md)

2026-11-06，测试者在 M1 Pro（arm64，macOS 15.7.4，24G517）上对手动传输的候选 DMG 运行预检。SHA-256 与上表一致，脚本报告 `Preflight passed`。测试者随后确认「M1 从应用程序启动成功，快捷键能唤起并输入」。最低系统设备的 DMG 安装启动、Option+空格唤起及直接输入通过。

### M1 设置持久化与睡眠恢复

在原生 M1 Pro 上，自动化用临时设置文件复验了冲突后旧配置不变、重绑成功后重新加载新键和主题。Rust 库测试 104 项通过，启动器主题与快捷键相关前端测试 5 项通过。只读挂载先前实测的同一 DMG，确认包内共享声音与源码文件逐字节哈希匹配，DMG 哈希仍与上表相同。

2026-11-06，测试者对两项实机操作反馈「符合预期」。在已安装的候选中更改主题和快捷键，从菜单退出再启动，主题保留，新快捷键能唤起并输入；系统睡眠唤醒后，用新键搜索并打开计算器。两项通过。

剩余交互包括中文输入法选词时避免误启动，以及快捷键冲突回退。使用测试账户验证占键冲突时，确认原键仍能唤起；默认键首次注册失败时才用菜单进入设置。记录可用的显示器和旧版本条件；同版本替换单列，跨版本升级需有真实旧包。卸载与退出清理留到最后，先保留当前安装用于继续验收。

## 较新 Mac（本轮已跳过，以下步骤仅供以后复测）

较新 Mac 实机行不需要安装 Node、Rust 或 Xcode。

1. 在 **M1 系列之后的 Apple Silicon** 和高于 15.7.4 的实际系统上，准备上表中的 DMG 及随交接工具提供的 `preflight-macos-device.sh`。运行 `sh preflight-macos-device.sh "/实际目录/UiPilot_tools_0.19.0_aarch64.dmg"`。保留芯片、OS/build 和哈希结果。若哈希不符，停止该行；另行构建的相同版本 .app 不能用于这行。
2. 从 Finder 打开已核验的 DMG，按正常流程复制到“应用程序”，从安装位置启动。记录系统安全提示和能否打开。若 Gatekeeper 阻止运行，按实际结果记录；保持系统安全设置，不移除下载隔离属性。
3. 依次用菜单栏和普通快捷键（初始候选为 Option+Space）打开启动器；直接输入中英文应用名，Enter 启动计算器，再次唤起并激活同一已运行应用。测试 Esc、切换到其他应用后自动隐藏、再次输入、中文输入法确认候选，以及从菜单退出后重新启动。
4. 在设置中换主题和测试组合键，退出再启动后验证设置和真实快捷键。测试一次与已占用组合键冲突，原键应仍能唤起；初始键不可用时应能从菜单进入设置。使用者自行操作睡眠/唤醒；有第二台显示器时再测试跨屏和拔屏。记下未覆盖的设备条件，恢复测试用快捷键。
5. **覆盖升级单列。** 需要同一产品标识的、可核验的不同旧版本 .app。先记录旧包版本/哈希，设置非默认主题或组合键并退出，换入固定 DMG 内的候选，再检查数据和快捷键。若只有这一版，记录“缺旧版，升级待验”。重装同一文件仅记为同版本替换。最后退出并卸载测试副本，核对常驻进程退出、菜单图标消失、组合键释放。

## Windows x64

2026-11-06，测试者在原生 Windows x64 的 VS x64 开发者环境中复检通过。候选源码、Everything 及打包输入通过，MSVC 编译器、链接器和 Windows SDK 工具均已找到。独立 Provider 为 `0bb397e0f9ea0caf1a05d25193f44a364c8de5dc`。

**Windows x64 构建通过。** 恢复下述两份许可证的原始换行后，从固定源码归档运行受管 Python 构建入口。Rust release 构建完成，日志包含 `UIPILOT_PROVIDER_VALIDATED`、`MANAGED_EVERYTHING_BUNDLE_INPUT_PASS` 和 `Finished 2 bundles`。产物为 `UiPilot_tools_0.19.0_x64-setup.exe`（NSIS）及 `UiPilot_tools_0.19.0_x64_en-US.msi`。测试者用 certutil 核对 NSIS 的 SHA-256 为 `7744753a04167393a9f48a33ee082b496f078821385e52c261581c7826cf1fc5`，从该文件安装后反馈「可以正常使用」。Windows 安装启动已通过。升级、卸载和插件授权等专项用例仍待单独验证。

同日，固定源码包 `a79f612` 已完成 npm 安装和 Cargo 依赖下载。首次打包在 `tauri.ps1` 接收 `build` 参数时报告 `PositionalParameterNotFound`。使用该源码包时，从源码根目录运行 `python scripts/provider_build.py build-session -- build -- --locked` 执行下方第 3 步的打包命令；此入口会建立同一受管构建会话并执行原有钩子。

Windows 本地回归首次运行到公开插件测试时，322/323 通过。设置示例的升级测试把编码后的中文目录名交给 Node，导致子进程找不到脚本。修复测试文件后，Windows 在原中文路径复验，323/323 通过，零失败、零跳过。SDK 合约检查已完成，参考插件验证已输出至 `com.uipilot.translate@1.4.3`。测试者在同一窗口回传 `npm run verify:local` 退出码 `0`，Windows 全量本地回归通过。

仅做安装实测时，直接使用 Windows 构建者交付的 NSIS 与哈希，从第 4 步开始；测试机无需安装 Rust、Node 或独立 Provider 源码。

第二次实机构建已完成 Provider 准备和前端构建，随后因 `Resource License does not match the reviewed lock` 停止。旧交接包预检仅比对规范化后的文本，误报通过。固定源码 ZIP 中两份 Everything 许可证均为 LF（2,648 字节）；原始 CRLF 文件为 2,699 字节。使用此归档时，在源码目录运行以下命令，仅在两份转换结果都匹配审核锁时恢复它们。保留原 ZIP 和锁文件，验收时记录这次换行修复。随后使用前述受管构建命令重试。

```cmd
python -I -c "from pathlib import Path; import hashlib; files=[Path('third-party/everything/LICENSE.txt'),Path('src-tauri/resources/everything/LICENSE.txt')]; data=[p.read_bytes().replace(b'\r\n',b'\n').replace(b'\n',b'\r\n') for p in files]; assert all(hashlib.sha256(b).hexdigest()=='c13d19adcbfd5d07e9512de9df99956a3423399ed1fadc5fd33186697ad8df2f' for b in data), 'License content mismatch'; [p.write_bytes(b) for p,b in zip(files,data)]; print('License SHA-256 OK')"
```

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

## 插件归档的双目标静态基线

下列参考包均由同一份源码各打包一次，再用 CLI 对同一归档分别指定 `--platform windows` 和 `--platform macos` 校验，8 项通过。当前候选 Mac 的公开插件宿主关闭，运行通过数仍为 0。Windows-only 的 `demo-panel` 原包目录在 macOS 目标得到预期的 `PLATFORM_INCOMPATIBLE`。

| 参考包 | 同包 SHA-256 |
| --- | --- |
| demo-settings 1.0.0 | `99792034c4d314150c0f3f53a9d981018aba12bd90b11f0e0c5acb0b0b2fb212` |
| demo-return 1.0.2 | `e9caba619b365f6e0764a039daa4603e4e3d953e64dfdb2967aa7e7e19e685a0` |
| demo-multi 1.0.0 | `a2e1c59a62b120555db9d7529fb8137ea0b5e03dfbdd1dc32d6211cf323332e9` |
| note 1.1.5 | `0c24a3115a90ddf49c36c7cef447d86ea4401796cd4b47694a63d69aef2895ae` |

## 回传结果

每个操作填 **通过 / 失败 / 待测**；失败附复现动作、实际结果和脱敏日志位置。仅记录芯片、系统版本/build、产品与 Provider revision、对应包哈希、签名状态、测试人和日期，不记录设备序列号、用户名、密钥或个人应用数据。

| 设备或阶段 | 分别回传的证据 |
| --- | --- |
| 较新 Mac | 本轮按用户决定跳过，未实测，无需回传。 |
| Windows 构建 / CI | 产品与 Provider revision、各门禁退出码、NSIS 哈希、资源核验、日志位置 |
| Windows 实机 | 安装包哈希、安装与权限、应用/文件搜索、插件授权、重启、退出、升级、卸载 |
| M1 Pro 最低系统 | 同一 DMG 哈希和新增的安装/升级/卸载等记录；保留原有交互结果 |

缺少必需设备行时，双平台发布验收维持待完成。公开分发另需经授权的 Developer ID 签名、公证、staple 和带隔离属性的安装证据。
