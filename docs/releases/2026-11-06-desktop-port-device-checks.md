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

### M1 中文名称搜索回归

在候选 `a79f612` 的安装副本中，测试者输入“计算器”没有结果。用 M1 / macOS 15.7.4 上的实际系统应用复现：扫描得到 152 个应用，`Calculator` 命中，`计算器` 零命中。系统计算器在 `InfoPlist.loctable` 中提供“计算器”和“計算機”，旧搜索只匹配当前显示名。原测试项分为两步：选词后应出现计算器但不启动；完成选词后再次按 Enter 才执行。

Mac 修复候选 `6ec54e8`（版本仍为 0.19.0）读取应用自身的本地化名称并按同一应用匹配。M1 实物测试中 `计算器` 和 `Calculator` 均首位命中系统计算器；Image Playground 仍使用原生显示名。Rust 库测试 108 项通过，两项系统应用检查通过，前端 IME 定向测试 14 项通过。面向测试者的 DMG 为 `UiPilot-0.19.0-arm64-6ec54e8.dmg`，SHA-256 为 `8b7e35425eb594532ea7e8630bf843e8417d0e92fb42c637376acf200e8f2c74`，主程序为 `3f33699a918dc59180f95627441021b950016b607a10eea231496584f23a0cc5`。该包已经过只读挂载、签名、最低系统、架构及资源核验。此前的安装和交互记录对应原包；测试者已在 M1 上回报「中文可以搜索到了」，修复包的中文名称匹配通过。随后测试者反馈「可以运行」，确认可以从中文搜索结果启动计算器。未单独记录选词瞬间的窗口状态。

### 开发候选 207d590（人工项延后）

`UiPilot-0.19.0-arm64-207d590.dmg` SHA-256 为 `5165334091533e968f54a981bd4a8878d376a4ffa0ca9437c9af08d0bb1ae807`。已只读挂载并校验内部签名、arm64、macOS 15.7.4、共享资源及可执行文件。macOS 全仓验证通过；提交后的 Rust 库复验 130 项通过、3 项环境相关测试默认跳过。公开系统字体目录的 Spotlight 探针另行通过。

Mac 新增显式录制同侧 ⌘ 的双击手势，首次选用才可能发起「输入监控」授权；普通热键不使用监听。保存失败保留原键、切回普通热键后释放监听、超时/混按/点击抑制等已自动验证。权限对话框、系统撤权、跨应用真实唤起和睡眠恢复留待后续统一实测。较早的 M1 睡眠唤醒结果对应普通组合键。Spotlight 内部查询和身份检查可用于开发，当前 `files=false`。

### 2026-11-07 · Find 预览候选 1f5cdf5

产品默认范围已选择系统已索引、当前宿主获准访问的本地文件。此次内部 DMG 使用 `macos-find-preview`，仅搜索通过本地挂载和读取校验的公共字体目录及 `/Users/Shared`。输入 `/find 文件名` 可转发到 Find；文件默认请求 Finder 定位，目录请求 Finder 打开，支持的普通文档另有打开动作。未改变公共插件文件权限。

| 交付物 | 核对值 |
| --- | --- |
| 产物 | `UiPilot-Find-preview-0.19.0-arm64-1f5cdf5.dmg`（内部 ad-hoc 签名） |
| DMG SHA-256 | `36c5b7df74a76e66c90686c868fd55ee92fc39cc8b2f466305acaba3e1369e88` |
| arm64 主程序 SHA-256 | `e16e8828a4609a9480c530e54ee21a3d1452422ca0514fc9e97a6b23661a6a6e` |

独立 Find 窗口、调用者/来源校验、主窗口收藏、结果取消和迟到回调、文件与父目录替换、同路径异 inode、权限撤销、超时未确认后不重复提交均有自动验收。受限预览 Rust 库测试 162 项通过，隔离系统 QA 配置 166 项通过。另行在指定系统字体目录命中并授权返回了 `Andale Mono.ttf`，未读取个人目录或查询共享目录现有内容。Windows 共用分类的 10 个 Everything 查询谓词已逐个与改动前核对。

稍后在隔离账号集中验收真实 Find 窗口焦点/固定/关闭、Finder 选中指定文件和实际打开文档。系统全域索引的隔离构建与普通预览分开；须按[专项设计](../superpowers/specs/2026-11-06-macos-file-search-design.md)验证已允许、未授权和撤权的真实签名身份。此次没有运行全域查询，未将该范围标为发布通过。

已观察到的范围差异：用户在受限预览中搜索位于「文稿」嵌套目录的 PNG，窗口显示 0 条。该位置不属于预览范围；此反馈未验证文件的索引或授权状态。后续隔离验收用合成 PNG 和包含中文、emoji 的目录复现：先确认 Spotlight 已收录且测试应用已获访问权限，用完整文件名和中文片段均应命中同一文件，Finder 定位应选中它；撤权后重搜不应显示该文件，旧结果也不能执行。

### 2026-11-07 · Find 文稿预览候选 ff13ec2

候选包为 `UiPilot-Find-Documents-0.19.0-arm64-ff13ec2.dmg`，已只读挂载核验内部签名、arm64、最低 macOS 15.7.4 和资源。DMG SHA-256 为 `5f7b218d5326d1f50bc7511abb2394594a3b9827b4527d70860ccedf5184a655`，主程序 SHA-256 为 `b9eba14d308ae03b5d20d3e996e45f384b323fb09188d23d050532d7cd90d801`。

Find 默认搜索公共根。点击「允许本次搜索文稿」后，用户可在系统面板选择当前账户的本地文稿目录，已有关键词会自动重搜。点击「停止搜索文稿」或退出程序后，本次授权结束。普通构建及系统 QA 配置使用各自的范围策略。

自动验证覆盖原生选择取消、关闭时迟到的授权、查询和结果随授权失效，以及撤销与 Finder 提交的两种先后顺序。受限预览与系统 QA 的 Rust 库测试各 175 项通过。常规全仓验证通过。真实 Spotlight 查询仅在测试专用目录运行，用完整中文文件名和片段都命中带 emoji 的嵌套合成图片，撤销后旧结果无法提交。样例已清理。

M1 实机截图确认：原生文稿选择框可用，授权后按完整文件名搜索，Find 在文稿的多层目录中显示了预期的一条 PNG 结果。Finder 实际定位、停止搜索及重启后的行为分别验收。

### 2026-11-07 · Find 记住文稿授权候选 9a1c17d

本地交付物 `UiPilot-Find-Remember-Documents-0.19.0-arm64-9a1c17d.dmg` 的 SHA-256 为 `561be1ab118771248466ed8bce3c35785c9d45c0a88267035eb8e069b2d3b9d2`，主程序 SHA-256 为 `52164da1be2e789249836cf7dc90c8fdf40cc7a5919b799cc0213b4654a9321b`。只读挂载核验了内部签名、arm64、最低 macOS 15.7.4 和共享资源。镜像中的主程序只有应用专属书签权限。

使用实际私有存储和文稿授权代码，在同一签名测试程序的独立进程中验证了保存、恢复、忘记、再次启动后无授权，以及同路径父目录被替换时拒绝恢复。另一个签名身份的对照探针无法恢复原书签。所有样例均为临时合成目录。公开预览的 Rust 库测试 185 项通过；跨版本更新、真实系统撤权与全域索引另行验收。

2026-11-07，M1 测试者按此候选的复测步骤反馈「符合预期」：选择记住文稿，完全退出 UiPilot，再启动后直接搜索同一关键词，仍能找到目标文件，期间无需再次选择目录。此项记为实机通过。

「停止并忘记文稿」后重启已通过独立进程测试，M1 界面操作待测。下一项全机索引验收使用隔离账户和合成文件：先确认 Spotlight 已收录样例，再验证允许访问时首次搜索能命中，系统拒绝或撤权后不显示该文件，先前的结果也不能再执行。

隔离验收的准备、操作及证据口径见[全机 Spotlight 隔离验收](macos-spotlight-qa.md)。开发机可以运行 `sh scripts/build-macos-find-qa.sh /绝对路径/新文件.zip` 生成专用测试包；普通文稿预览包和 Windows 安装包均不含该独立探针。

### 2026-11-07 · 全机 Spotlight 测试包 6614baf（实机延后）

- `UiPilot-Spotlight-QA-arm64-6614baf.zip` 的 SHA-256 为 `42e6635716a71e0180349b2ce23fd2cc0340e3f557d21dbf50f9bfcdeace4723`，解压出的程序 SHA-256 为 `28c26575d04756c71eb56d4dbeeecb1f6d579d758937ff6548eae78471679b32`。已核对内部签名、arm64、最低 macOS 15.7.4、可执行权限与随包说明。
- QA 工具配置的 Rust 库测试 184 项通过，现有文稿预览配置 186 项通过。全仓本地验证通过。解压包的日常账户拒绝、缺少显式启用标志及无参数启动均按预期退出。
- 按测试者安排延后实机运行。两份样例的索引命中、系统范围的正向结果及同进程撤回旧目标仍待专用账户回传；UiPilot 的真实系统隐私授权另行验收。

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

## 2026-11-07 剪贴板共用基础自动验证

按[专项规格](../superpowers/specs/2026-11-07-macos-clipboard-foundation-design.md)，Mac 现已编译共享模型、格式选择与预览逻辑。路径指纹回归先复现了大小写不同路径被误合并，修复后 8 项共享测试通过；Windows 旧指纹的固定向量也在本机验证。独立审核未发现严重问题。该单元未接入原生历史服务、持久化、复制或插件权限，不改变现有安装包的实机验收结论。

环境沿用 M1 Pro / macOS 15.7.4、Node 22.22.1、Rust 1.96.1 及隔离 Cargo 缓存；仅使用合成内容，未访问真实剪贴板。

| 检查 | 实际结果 |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib clipboard_history::` | 8 通过；包含 Mac 路径字节、Windows 固定指纹、格式优先级、摘要 DTO、文字和图片边界 |
| `npm run verify:local` | 退出 0；前端 774 通过/18 既有跳过，普通 Mac Rust 160 通过/3 既有忽略，CLI 186 通过，公开插件测试 323 通过，SDK/schema/包校验通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0；普通 Mac 所有目标编译通过 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 194 通过/6 既有忽略，退出 0 |
| Windows 原生回归、Mac 原生采集/授权/退出和复制 | 待后续实现与实机验证，未标记通过 |

本机日志为 `/private/tmp/uipilot-clipboard-red.log`、`/private/tmp/uipilot-clipboard-shared-tests.log`、`/private/tmp/uipilot-clipboard-foundation-{verify,check,preview}.log`。原计划 Task 10.1 仍未完成，下一单元为原生 reader/observer 及停止/重启时的代际隔离。人工测试按用户安排延后。

## 2026-11-07 原生文本观察器自动验证

依[观察器规格](../superpowers/specs/2026-11-07-macos-clipboard-observer-design.md)完成原生单项纯文本 reader、worker、停止准入和会话隔离；独立审核通过。初始红阶段为新 API 尚未实现的编译失败，不记作已复现既有系统竞态。完成后，fake 读取和并发测试 20 项、既有共享测试 8 项通过；覆盖阻塞读取、旧事件、新会话、消费者在途、并发停止、回调/消费者/worker 析构重入、初始化失败和 panic 清理。

原生用例 `clipboard_history::macos_observer::tests::native_unique_board_text_and_mixed_format_classification` 默认 ignored。首次在沙箱内执行因 `pasteboardWithUniqueName` 返回 NULL 而失败，未取得测试 board。随后仅对该用例在获准的非隔离环境精确执行 `--ignored --exact`，1 项通过：唯一命名 board 的空文本、中文/emoji、NUL 精确保留，混合格式拒绝，worker 建立基线后收到合成写入，停止后旧事件拒绝。测试只释放自身创建的 board，未调用 general pasteboard，不等同于跨应用、用户授权或历史存储验收。

环境继续为 M1 Pro / macOS 15.7.4、Node 22.22.1、Rust 1.96.1。验证结果：

| 检查 | 实际结果 |
| --- | --- |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib clipboard_history::` | 28 通过、1 默认忽略；该原生用例已另行通过 |
| `npm run verify:local` | 退出 0；前端 774 通过/18 既有跳过，普通 Mac Rust 180 通过/4 默认忽略，CLI 186 通过、公开插件测试 323 通过，schema/SDK/参考包校验通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 214 通过/7 默认忽略，退出 0 |

普通库与 Find 预览各新增的 1 个默认忽略均为上述原生用例，其余分别是已有 3/6 个忽略，未将它们计为通过。日志：`/private/tmp/uipilot-observer-{red,tests,native,native-unsandboxed,verify,check,preview}.log`。

生产 Mac 宿主没有启动该观察器，SDK 权限和能力开关不变。本单元未修改 Windows 原生 observer/service/store/paste，也未补做 Windows 原生回归。私有存储、插件授权撤回、图片/文件格式、复制及通用剪贴板实机测试仍未完成；人工测试按用户安排延后。

## 2026-11-07 私有文本历史与授权自动验证

依[存储与授权规格](../superpowers/specs/2026-11-07-macos-clipboard-storage-design.md)实现共享纯变换、Mac 私有文本介质和未装配的授权协调器。最多 50 条，Mac 序列化预算 8 MiB；删除/清空不复用 ID，重复文本保留首次入库时间。所有测试仅使用合成事件与新建的临时目录；ACL/文件权限操作只作用于这些夹具，不修改系统隐私设置或个人历史。

独立审核修正并验证了撤权后残留 store 锁、根目录替换、无授权批次和错误分类。实际测试还复现了仅依赖 close 解锁时，fork/dup 暂时保留文件描述符会使立即重开失败；显式解锁并新增保留 dup FD 的回归后通过。正常遗留 staging 在排他锁和严格身份检查后丢弃，不恢复为历史；异常 staging、损坏 current、链接/硬链、宽松权限或扩展 ACL 均拒绝。提交前失败保留旧快照和原始文件，提交后不确定则拒绝继续读写，关闭重开再校验。

| 检查 | 实际结果 |
| --- | --- |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib clipboard_history::` | 51 通过、1 原生命名剪贴板用例默认忽略；本轮未重跑该原生用例 |
| `npm run verify:local` | 退出 0；前端 774 通过/18 既有跳过，普通 Mac 库 203 通过/4 既有默认忽略，CLI 186 通过，公开插件测试 323 通过，schema/SDK/包校验通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 237 通过/7 既有默认忽略，退出 0 |

环境仍为 M1 Pro / macOS 15.7.4、Node 22.22.1、Rust 1.96.1。日志：`/private/tmp/uipilot-history-{red,lock-red,tests,verify,check,preview}.log`。`red` 为新 API 缺失的编译红阶段；`lock-red` 为独立复现的文件锁回归，二者分开记录。

Windows store 仅委派提取后的纯历史变换，原格式校验与文件 IO 保留；共享测试覆盖文本去重/容量/计数，以及图片文件名与预算淘汰名单，但不替代 Windows 原生回归。本轮仍未开放 Mac 宿主服务或 SDK。读取前批次钩子、原生采集时间传递、真实授权与退出、图片/文件、复制和插件双端验收均待后续；当前协调器记录的是首次消费入库时间。整体 Task 10.1 未完成，人工测试继续延后。

## 2026-11-07 读取前授权与内部采集自动验证

依[采集接线规格](../superpowers/specs/2026-11-07-macos-clipboard-collection-design.md)将 Mac observer、读取前授权与私有历史接成内部 collector。没有授权时正文读取为零；授权集合改变只重建 count 基线，避免后来授予权限时补采停用期间的内容。生产历史入口只消费密封的事件，绑定原批次、observer 会话及正文读取前的 UTC 观测时间，不再接受任意 event 与新 batch 配对。

原生读取使用独立许可，读取期间不持 history state 或 store 锁；许可在回调前释放。撤权和 shutdown 先关闭准入，再在状态锁外等待已开始的读取及历史操作结束。独立审核补查了启动基线排序、幂等授权、测试的真实撤权信号，以及 shutdown 的阻塞读取路径；复审无严重问题。

13 项新增测试实际使用 fake source、合成文本和专用临时目录。包括无授权后晚授权、读取中新增授权不接收旧批次、撤权/重授权/关闭排空、时钟与回调错误、panic 清理、跨实例与旧会话事件拒绝、固定观测时间，以及 collector → index → snapshot → 关闭重开。初始红阶段仅为新 API 缺失的编译失败，未将其称为真实系统竞态复现。

| 检查 | 实际结果 |
| --- | --- |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib clipboard_history::` | 64 通过、1 原生命名剪贴板用例默认忽略；本轮未重跑该原生用例 |
| `npm run verify:local` | 退出 0；前端 774 通过/18 既有跳过，普通 Mac 库 216 通过/4 默认忽略，CLI 186 通过，公开插件 323 通过，schema/SDK/参考包校验通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 250 通过/7 默认忽略，退出 0 |

环境沿用 M1 Pro / macOS 15.7.4、Node 22.22.1、Rust 1.96.1。日志为 `/private/tmp/uipilot-collector-{red,tests,verify,check,preview}.log`。本轮无 Windows 原生代码变更，未重新执行 Windows 原生验证；未读取 general pasteboard 或个人历史，未变更系统权限，未开放生产宿主或 SDK。Task 10.1 仍需复制与外部写入保护等后续工作。

用户本轮指定完整目标达到约 70% 时统一人工测试。当前仍估计约 55%；到达该节点再冻结版本、交付带 SHA-256 的测试包及清单并通知测试者。本轮没有新的用户安装包，既有实机证据不自动转移为新功能验收结果。

## 2026-11-07 显式历史文本复制自动验证

按[复制规格](../superpowers/specs/2026-11-07-macos-clipboard-copy-design.md)实现独立复制授权、单次请求、最终记录复验及原生一次 declareTypes/setData。读取授权不自动扩为复制；撤回读取、撤回复制和 shutdown 分别排空已接纳提交。独立终审通过。

`clipboard_history::macos_copy::` 聚焦 15 项通过、1 项原生用例默认忽略；剪贴板整体 79 项通过、2 项默认忽略。新增 fake 用例明确展示 count 检查到 claim 之间不是 CAS，并验证观察到变化时零 claim、claim 后所有权丢失/异常没有第二次写入、之后的外部 B 不被恢复动作覆盖，以及准备期间删除/撤权/重授权的拒绝。原生夹具使用系统唯一命名 board，仅定义、未运行；通用剪贴板未访问。红阶段仅为缺失新 API 的编译失败。

日志：`/private/tmp/uipilot-copy-{red,tests,clipboard}.log`。本单元未装配宿主/SDK、未修改 Windows 行为；自动临时恢复仍为 Unsupported，未虚构原子条件写保证。人工测试继续汇总到整体约 70% 的统一节点。

## 2026-11-07 Keychain 隔离基础自动验证

[Keychain 基础规格](../superpowers/specs/2026-11-07-macos-keychain-foundation-design.md)对应的内部 Vault 和现代 Keychain 后端通过独立审核。宿主/插件 namespace、实例与 generation 授权、身份封装、有界秘密、撤回/关闭排空已实现；生产凭据服务与 SDK 尚未装配。

首个 classic 方案因无法从实现证实无交互参数而被审核否决，未调用真实 Keychain。改为每次 CRUD 强制 Data Protection、禁止同步、仅本机解锁访问及非交互认证，不带 classic search list/use-keychain，不在签名失败时回退。新增依赖仅为 lock 中已有的 security-framework-sys 2.17.0，无版本升级。

`cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib macos_keychain::` 9 项通过：包含真实 CFDictionary 的精确参数检查和注入 SecItem 控制流，update 失败不 add、duplicate 不重试、错误固定且正文限长。默认测试全部使用合成值，raw SecItem 未调用。日志 `/private/tmp/uipilot-keychain-modern-tests.log`；过程中修正测试的 CFDictionary 长度类型，不记为系统行为回归。

签名/entitlement、原生 CRUD、锁定及跨版本访问仍待独立实测，缺这些证据不开放生产凭据。Rust 缓冲 Drop 清零不等同于系统 CFData 可主动清零。本单元不修改 Windows DPAPI 行为，不计入同包双端插件实测通过数。

## 2026-11-07 Mac 启动器快速计算自动验证

依[快速计算规格](../superpowers/specs/2026-11-07-macos-quick-evaluation-design.md)复用四类本地计算、注册私有复制结果及查询绑定的单次执行。审核补齐了跨 registry 票据身份、旧查询复制失败不污染新结果，以及真实 Mac 能力快照中的前端显示门禁；最终复审通过。汇率仅显示未启用提示，不发送网络请求。

真实回归红绿证据包括 registry 数字相同仍不能交换票据、复制不确定结果退休、晚到复制错误不影响新查询，以及 Mac 能力过滤前四类计算全部消失的复现与修正。默认 Windows 预览测试不足以证明 Mac 主界面可见，新增测试用 builtinFeatures/plugins=false 的 Mac 快照执行 calculator Enter，确认 currency 不执行、其他内置与插件继续关闭。

| 检查 | 实际结果 |
| --- | --- |
| `npm run verify:local` | 退出 0；前端 777 通过/18 跳过，Mac 库 320 通过/5 默认忽略，CLI 186 通过、公开插件 323 通过，schema/SDK/参考包通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 354 通过/8 默认忽略 |

日志为 `/private/tmp/uipilot-quick-final-verify.log`、`/private/tmp/uipilot-quick-{check,preview,cap-red,cap-green,collision-red,collision-green,ui-red,ui-stale-red}.log`。完整验证一次因命令环境未包含 Cargo 提前结束，修复 PATH 后完整重跑退出 0；不计为源码失败。所有 native 复制测试使用 fake，没有读取或改写通用剪贴板。用户可见复制/焦点仍在 70% 候选包人工验收，Windows 原生回归未重跑。

## 2026-11-07 Mac Web 查询动作自动验证

[Web 查询规格](../superpowers/specs/2026-11-07-macos-web-search-design.md)已实现：普通查询在应用/Find 后追加无收藏搜索行；固定 provider 与单 query 编码，用户执行才交给 NSWorkspace。已有停用设置被尊重，最终准入持 settings 短锁原子消费精确 Web 动作，原生调用时已释放 settings/registry 锁。不会将正文当 URL 模板，不会在输入阶段联网。空/slash/快速计算不产生 fallback；4,096 字节查询及 16 KiB URL 上限明确拒绝。

复审补上规范化前拒绝空 userinfo 与反斜杠的 helper 边界，query 中 `@` 仍可作为搜索文字。真实 Mac 能力配置测试覆盖结果可见、Enter 只发 ID、独立引擎设置与保存失败回退；新增错误沿用单次消费与晚到查询保护。默认测试只调用 fake opener；没有启动浏览器。

| 检查 | 实际结果 |
| --- | --- |
| `npm run verify:local` | 退出 0；前端 780 通过/18 跳过，Mac 库 337 通过/5 默认忽略，CLI 186 通过，公开插件 323 通过，schema/SDK/参考包通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview` | 371 通过/8 默认忽略 |
| `node --test scripts/macos-host-acl.test.mjs` | 4 通过 |

行为红阶段复现普通查询没有 fallback 及前端 Mac 过滤；后端新增 14 项测试，原有应用与复制回归通过。日志 `/private/tmp/uipilot-web-{red,ui-red,ui-green,verify,check,preview,browser,apps,registry,copy,tests,acl}.log`。Windows URL 构造与执行保持既有路径，Windows 原生回归和 Mac 默认浏览器实际跳转留统一节点。本单元不提供可复用 `/web-search` 入口，不能收藏一次查询值。

## 2026-11-07 Mac 登录项预览自动验证

[登录项规格](../superpowers/specs/2026-11-07-macos-login-item-design.md)对应的原生适配、独立 IPC 和设置控件已通过终审。系统四态加 unavailable 是唯一事实；旧 settings.autostart 保留而不对账。默认构建 `loginItems=false`，三条 IPC 在可信 main 检查后、主线程派发前检查非默认 `macos-login-item-preview`；70% 内部候选显式开启，实测前不开放生产默认能力。

7 项原生 fake 测试覆盖 54 种变更后结果/状态组合及所有零写分支。真实红阶段 3 通过/4 失败，修复为 7 通过；另有严格 IPC 输入与 gate 用例。false/异常后仍复读，RequiresApproval 不重复注册且可停止，无自动补偿或重试。绑定已编译/link，但未调用真实 SMAppService，没有改变登录项或打开系统设置。

前端单例 pending 在组件卸载后保留，新界面的 read 等待旧 set settle；组件请求代际丢弃旧响应。9 项新增前端用例包含真实 client+control 跨重建 deferred、实际 main adapter 的预览能力、系统状态覆盖旧 bool、独立停止注册，以及 Windows 路径保持。初始前端红是缺少新模块；中途修正测试字面量类型与 Ant Design 双字按钮空格匹配，不把这些称为原生行为回归。

| 检查 | 实际结果 |
| --- | --- |
| `npm run verify:local` | 退出 0；前端 789 通过/18 跳过，普通 Mac 库 345 通过/5 默认忽略，CLI 186 通过，公开插件 323 通过；schema/SDK/参考包通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview` | 379 通过/8 默认忽略 |
| `node --test scripts/macos-host-acl.test.mjs` | 4 通过 |

日志 `/private/tmp/uipilot-login-{red,tests,host,client-red,client-green,ui-red,ui-green,focused,verify,check,preview,acl}.log`。签名、系统通知、外部撤销及真实注销/登录行为在 70% 统一人工节点验收，Task 10.5 不提前勾选完成。

## 2026-11-07 Mac 插件 HTTPS 共用核心自动验证

按[共用基础规格](../superpowers/specs/2026-11-07-macos-plugin-network-foundation-design.md)编译真实 broker/registry/address policy/native transport 与共享音频 authority。生产插件、权限 gateway、runtime、secret store 未装配。SensitiveBytes 提取并保留 Windows re-export；DPAPI 成功路径通过等价 into_vec 转移，错误清理行为未变。独立源码终审通过。

首次未接线时为零测试，随后缺模块编译失败；这两次仅作为装配红阶段。首次实际套件 44 通过/9 失败，9 项均因沙箱禁止 localhost bind（EPERM），非 TLS 逻辑失败。经工具自动审批在允许本地监听的环境重跑，53 项通过（41 网络、9 共用 guard authority、2 内存容器、1 manifest 类型同一性），代理环境子进程另明确 1 项通过。动态 rcgen 证书与内存 root 仅用于 127.0.0.1 临时 TLS 服务，未访问公网或改系统信任/Keychain。

| 检查 | 实际结果 |
| --- | --- |
| `npm run verify:local` | 退出 0；前端 789 通过/18 跳过，Mac 库 398 通过/5 默认忽略（代理子进程另 1 通过），CLI 186、公开插件 323 通过，schema/SDK/参考包通过 |
| `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` | 退出 0 |
| `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview` | 432 通过/8 默认忽略 |

日志 `/private/tmp/uipilot-network-{unwired-red,module-red,green,native-green,verify,check,preview}.log`。真实 native TLS 的成功/未信任/错 hostname/过期证书、不得降级、限额/取消和 proxy 环境子进程均执行，未跳过 Mac 失败或改 production trust policy。Windows 原生验证未执行；本单元不增加同包双端插件通过计数。

## 2026-11-07 WKWebView 独立媒体探针构建验证

[媒体探针规格](../superpowers/specs/2026-11-07-macos-media-probe-design.md)与 `spikes/macos-media-probe/` 已完成实现和独立复审。仅固定内存 WAV、oscillator 和默认关闭的可选语音，不运行插件。默认启动不创建媒体 WebView；人工点击才进入案例。C 生命周期核心用于实际探针准入，默认自动验证不启动 AppKit/WKWebView。

纯 C stub 实际红阶段断言退出 134，补齐状态机后三组绿色；Objective-C 使用 arm64/min15.7.4 与 `-Wall -Wextra -Werror` 编译/link，签名 strict、Info.plist、Mach-O/load commands 与 JS 语法检查通过；已有输出拒绝重建退出 2。复审发现截止采样异步完成必被退役丢弃，改为加载准入后 1 秒单次采样，2 秒截止独立 retire/stop，不等采样延长播放。迟到回调与正常 completion 走同一准入方法，逐通道记录分开；终审无严重问题。

日志 `/private/tmp/uipilot-media-lifecycle-red.log` 及构建输出中的 `lifecycle-tests.log`、`signature-check.log`、`macho-check.log`、`load-commands.log` 和源文件 SHA-256。实际 GUI/音频/语音、系统调度延迟与 WebContent 进程恢复仍未验证；本轮没有运行媒体。原生活动状态不是静音开关，探针不颁发 production GuardProof；UiPilot 插件运行仍关闭。

## 2026-11-07 约 70% 候选冻结

主程序 `42a094b` 与独立探针 `b279271` 已构建并核对；固定哈希、完整证据、平台边界和待执行清单见[70% 人工验收交接](2026-11-07-macos-70-percent-checkpoint.md)。DMG 只读挂载核对通过，M1 Pro / macOS 15.7.4 的只读预检通过；没有安装运行新主程序、调用真实登录项或启动媒体探针。Windows 同源码 ZIP 已逐文件核对，新的 Windows 原生构建待执行。旧设备通过记录不自动延伸到本候选。

## 2026-11-08 70% 候选 M1 对话式人工验收（进行中）

测试者在 M1 Pro / macOS 15.7.4 对候选 `42a094b` 执行人工步骤。预检截图确认 DMG SHA-256 为 `7a6b979e098b9d809a39940fa28ba103dc6704e73925ab02066e224981f68599`。以下依据用户逐项反馈及截图，不代表其余清单通过。

已确认通过：

- 替换安装到“应用程序”后正常启动，无其他启动弹窗；Option + 空格唤起并获得输入焦点，中文“计算器”搜索与打开、再次唤起、Esc 隐藏正常。
- `2+2` 显示 4，用户确认回车复制到剪贴板；单位换算 `1000 mm to in`、日期 `today + 1 day`、时区 `Mumbai time` 均符合预期。
- `中文测试 & A B` 展示网页结果而不自行打开浏览器；执行后 Chrome 打开 Bing，完整查询保留；切换其他搜索引擎并搜索正常。搜索引擎和默认浏览器不同不是故障。
- 文稿中文 PNG 搜索成功，中文/emoji 路径正常；双击/回车实际打开 Finder 中正确文件夹并选中目标文件，定位行为通过，直接打开图片未通过验收（见下）。
- “停止并忘记文稿”后结果消失，退出重启后仍不返回；重新选择 Documents 并“授权并记住文稿”后搜索恢复，再次完整退出重启后无需授权即可命中。撤销与记住授权的持久化均通过。
- 首次输入监控授权后双击 Command 录制无效，完整退出重启后用户确认可以录制并全局唤起；再次退出重启后双击 Command 仍可唤起且输入焦点正常。
- 关闭输入监控权限后按系统提示“退出并重新打开”，双击 Command 不再唤起，也未反复弹权限提示。菜单栏截图明确显示“显示 UiPilot（双击 ⌘ 不可用，暂用 Option + 空格）”；备用快捷键实际唤起尚待验证。

待后续修改或核对（用户要求先记录，继续逐项测试）：

1. 中文输入法输入 `today` 后，用回车确认输入会立即执行下方 Find 结果。预期确认输入的回车不执行结果；根因尚未定位。
2. 登录项显示“当前应用的登录项不可用”，勾选框禁用，点击刷新后不变。真实启用/停用、系统状态同步及重新登录均未能验收；根因尚未定位。
3. 文件结果双击/回车执行 Finder 定位而不是直接打开图片。需核对现有动作约定、默认动作与验收文案后处理，不预先断言底层打开 API 错误。
4. 用户报告主窗口不能拖拽，具体可拖拽区域与复现步骤尚待核对。
5. 权限撤销后设置页仍只显示“双击 Command”及静态权限说明，没有当前不可用状态或备用快捷键提示。菜单栏已有准确降级提示，问题范围是设置页状态反馈不完整，不能记成所有界面都没有提示。
6. 首次输入监控授权后须重启才能录制的引导是否充分，待核对；授权后重启再录制与唤起已通过。

截图依据：预检 `codex-clipboard-45d8b1a0-9783-4aa0-acbb-67cb62545f04.png`；登录项 `codex-clipboard-e794875c-71bd-448a-89ff-89b591f67321.png`；搜索 `codex-clipboard-48c69f29-1d98-4b80-92e6-ea6a2238f07d.png`；撤权系统提示 `codex-clipboard-d7bc49fd-fe53-44c8-b639-1c2737373aff.png`；菜单栏与设置状态对照 `codex-clipboard-79bdb235-56a5-4ab9-98d2-d4c5c3b6046c.png`、`codex-clipboard-c7b93207-e40d-403f-9aeb-b0f451b61683.png`。原图由用户在本对话提供，不包含于 Git 归档。

当前测试状态：输入监控权限关闭，保存的快捷键仍为双击 Command；菜单声明回退 Option + 空格。尚未完成备用快捷键实测、原状态恢复、独立媒体探针或新 Windows 候选验收。生产插件仍关闭，整体不因部分人工通过宣称完成。

### 同日后续：备用快捷键与 WebAudio 实机失败证据

用户确认权限撤销后的备用 Option + 空格可唤起且输入焦点正常。随后通过录制将保存的快捷键恢复为测试前的 Option + 空格，无单独保存按钮；切换 Finder 后再次验证正常唤起和输入焦点。输入监控权限保持关闭。此前“备用快捷键待验证、保存双击 Command”的状态由此更新。

独立探针 `b279271` 解压后经系统打开确认运行，启动时展示说明、语音默认未勾选。用户点击短音基线，听到约两秒短音并自动停止。用户提交的 generation=1 日志显示 HTML 时间从 0 推进至 1.400，WebAudio 至 1.695；原生两次观察 Playing。HTMLMedia 与 WebAudio 基线均有效，语音未验证。

**先暂停再加载：实机隔离失败，阻塞此路径作为插件媒体隔离保证。** 用户反馈“还是有声音”，并提交 generation=4 完整日志。关键顺序：

```text
暂停 completion 到达（不是 mute proof）；即将准入固定 fixture。
准入 固定新导航，generation=4，阶段=0。
原生 暂停后、加载前：None（活动状态，非开关读回）
g=4 fixtureStarted：HTML paused=1 t=0.000，WebAudio unavailable t=0.000
g=4 htmlRejected：HTML paused=1 t=0.000，WebAudio suspended t=0.000
g=4 audioResumed：HTML paused=1 t=0.000，WebAudio running t=0.000
原生 固定页面加载后：Suspended（活动状态，非开关读回）
g=4 sample：HTML paused=1 t=0.000，WebAudio running t=0.398
原生 加载准入后 1 秒：Suspended（活动状态，非开关读回）
g=4 sample：HTML paused=1 t=0.000，WebAudio running t=0.998
g=4 sample：HTML paused=1 t=0.000，WebAudio running t=1.599
g=4 finished：HTML paused=1 t=0.000，WebAudio running t=1.698
可疑播放推进
逐通道：HTML 时间推进=未观察到；WebAudio 时间推进=有。
原生截止：请求暂停并销毁。WebContent 进程更换未强制触发，未验证。
```

以上是用户原始日志的关键摘录，完整日志见对话。HTML 本次未推进且 play 被拒绝；WebAudio 在显式 resume 后推进，结合听到声音的实机反馈，不能将暂停 completion 或 Suspended 状态视为 WebAudio 静音保证。该结论限于当前候选/设备/案例，尚未归因到具体系统实现，也不代表所有音频路径结论。

暂停后续播放测试；reload/新导航、迟到回调/替换、取消、语音和 WebContent 进程更换的实机行为仍未验证。生产插件保持关闭，不以脚本覆写或忽略此失败开放运行。下一步排查原生能力与方案边界，再决定修复或替代方案。Windows 新候选仍未验收。

用户随后确认独立媒体探针已退出、窗口关闭且无声音。本轮主程序反馈的修复与下一次复验边界见[实机反馈修复记录](2026-11-08-macos-manual-fixes.md)；不能将自动修复通过替代新包人工结果。

## 2026-11-08 ae687c0 修复候选复验与跨屏问题

用户确认已替换“应用程序”内副本并启动 ae687c0：中文输入法输入 `today` 后确认回车不再误开 Find，下一次正常 Enter 仍执行结果；主窗口可以拖动；设置显示“当前生效：Option + 空格”。这些结果为人工反馈通过，非自动测试推断。

登录项：用户先在 macOS 系统设置中手动添加应用，随后应用内正确显示启用；应用内取消勾选后“未启用”且系统记录移除；重新在应用中勾选后系统记录增加；最终取消勾选恢复未启用。由此仅确认**经系统手动初始化之后**的启停同步通过，不代表首次 NotFound 的“尝试启用”或重新登录自动启动通过。用户明确表示登录启动功能先不用再测，本轮后续不安排登录项测试。

跨屏问题的最终澄清：内置屏幕上能记住隐藏前的位置，不能记成所有位置保存均失败。用户将主窗口拖到另一块屏幕右侧，点击该屏桌面失焦隐藏，鼠标保留该屏；再次唤起回到原来的屏幕。当前用户已经改用 **Shift + 空格**，不是先前的 Option + 空格。外接屏型号/分辨率/缩放及排列尚未提供，不据此断言具体拓扑。

修复按锁定依赖源码调查：tao 0.35.3 的 Mac cursor_position 把 NSEvent 全局逻辑坐标乘主屏 scale，而 monitor_from_point 将入参直接与 CGDisplayBounds 逻辑坐标比较；旧宿主直接传递，使混缩放跨屏命中可能失败并回退主屏。旧位置与工作区也分别按不同屏的 scale 表示，不能直接当同一物理空间比较。新候选应统一逻辑坐标并保留有效窗口位置，只在窗口离开当前可用工作区时回退。该调查阶段外屏回归尚未通过；后续结果见下方 51eb1fa 实测记录。

## 2026-11-08 51eb1fa 跨屏位置人工复验通过

用户确认替换安装并启动跨屏修复候选 `51eb1fa`，随后反馈“多次测试，现在可以按上一次消失的位置再次激活显示”。结合正在执行的外接屏拖动/隐藏/再唤起步骤，跨屏位置保持记为通过。固定候选哈希及边界见[跨屏修复记录](2026-11-08-macos-cross-display-fix.md#2026-11-08-人工复验通过)。这是用户实测证据，不外推拔屏、其它排列/缩放或完整退出后的持久化。当前 Shift + 空格；登录启动人工测试仍暂停，媒体隔离失败仍未解决，生产插件关闭。

## 2026-11-08 Mac 共享数据调用准入验证

Mac foundation 同源接入 Windows 已有的 `data_call_gate.rs`，生产实现不改动。原筛选实际命中 0 项；接线后原有 4 项及新增 3 项共 7 项实际通过。新增覆盖关闭后同身份 activate 不复活、旧 generation/activation/admission epoch 的 close/remove 不影响替代身份，以及新旧实例多个 lease 的独立排空。独立源码复审通过。

`cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` 退出 0；`cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview` 为 450 通过、8 默认忽略，网络 proxy 子进程另 1 通过。日志 `/private/tmp/uipilot-data-gate-{before,after,check,preview}.log`。全库回归中的网络夹具仅绑定本机临时端口，不改系统信任。没有 GUI、登录项、声音或权限变更。

本次没有装配 Mac plugin manager、数据 gateway 或 Runtime，不构建新的人工安装包；Windows 原生验证未执行，生产插件仍关闭，同包双端插件通过数仍为 0。

## 2026-11-08 无窗口 JSC 子进程探针

依据[独立探针规格](../superpowers/specs/2026-11-08-macos-jsc-probe-design.md)，新增 `spikes/macos-jsc-runtime-probe/`，只运行固定合成脚本。公开 JavaScriptCore + Foundation 原生 helper，经严格 clang、arm64/min15.7.4 构建及 ad-hoc 签名检查，最终自动测试 **10 项全部通过**。没有运行用户插件、GUI、音频、个人文件访问、网络或系统权限操作。

实际证据包括：同步与 Promise 返回；列出的 16 个 DOM/媒体/网络/系统全局入口为 undefined；异常不输出合成秘密正文；唯一响应和 generation/requestId 校验；畸形/重复键/额外字段/重复 wire 完成拒绝。初审后补充 stdout/stderr 各 5000 字节、合计超过 8 KiB 的真实拒绝，以及第二次 JS 完成回调从原来接受第一次结果改为固定 scriptFailed（有红→绿证据）。

同步死循环、永不 settle 的 Promise、先回调再死循环均由父进程截止时间触发 kill + wait。测试逐次要求 `os.waitpid` 报 ChildProcessError 确认子进程已回收，再以新 PID 成功运行正常 case；父读取异常也走真实子进程回收。不能把这些结果解释成生产沙箱或内存硬配额已经实现。

最终日志与源码 SHA-256 位于 `/private/tmp/uipilot-jsc-probe.QDglUD/`；重复 JS 完成红阶段为 `/private/tmp/uipilot-jsc-review.V3M2F0/red/tests.log`。独立源码终审通过。构建目录记录的是构建时基线 HEAD，未提交探针文件另有源码哈希，不误称当时已有最终源码提交。

ES Module `export onCommand` 被当前 evaluateScript 路径拒绝，这是明确的不兼容负例。下一步仍需确定真实包模块加载、API bridge 与生产进程隔离方案；Panel/Window 的 WK 音频问题独立存在，Mac 生产插件继续关闭，同包双端通过数仍为 0。本轮不需要安装新 App 或安排人工音频复验。

## 2026-11-08 既有插件 ESM 转换实验

在 `spikes/macos-jsc-esm-probe/` 完成[独立模块转换实验](../superpowers/specs/2026-11-08-macos-jsc-esm-probe-design.md)。Node 仅解析和打包受控包内快照，不执行模块；生成代码嵌入固定的独立 JSC helper，在父进程 deadline 下执行。使用现有 Node v22.22.1 及已锁定的 esbuild 0.28.2、rolldown 1.1.5，没有新增依赖或修改实际插件包/SDK/生产宿主。

| 验证 | 实际结果 |
| --- | --- |
| 构建测试 | 6 通过，覆盖 AST 拒绝动态模块、包边界/符号链接/资源限额、内存快照、保留 PURE 注解顶层调用以及不执行模块 |
| 原生测试 | 8 通过；11 个固定 helper 均以 arm64/min15.7.4 编译、ad-hoc 签名及验证 |
| 真实 `demo-return` | 原始入口未改；中文/emoji、内部空格、固定日期、requestId、完整复制动作数据与预期一致；没有操作剪贴板 |
| 模块语义 | 相对导入、重导出、live binding、异步返回、冻结输入、严格模式的所列场景通过；未宣称任意 ESM 等价 |
| 真实 `translate` | 三模块图转换成功；load-only 观测为 completed=false，加载前 TextEncoder 未定义；没有调用 onCommand 或权限 API，不认定插件可用或推断完整异常根因 |
| 归属与回收 | 私有完成通道、包篡改全局内建、错误 requestId/超大响应/异常拒绝；顶层循环与 pending Promise 真实 kill/wait 后新 PID 成功 |

开发中实际发现 esbuild IIFE 默认产物未保留所需 strict 语义：原生红例中 unbound this、未声明赋值拒绝、全局未污染、冻结输入写入拒绝四项均为 false；独立 bundle 加 strict banner 后通过。runner 的 strict 无法替代包本身的 strict。红日志 `/private/tmp/uipilot-jsc-esm.A42F5u/native-tests.log`，缺 builder 初始红例 `/private/tmp/uipilot-esm-red.log`，占位 helper 红例 `/private/tmp/uipilot-jsc-esm.i3BqJC/native-tests.log`。

作者冻结证据 `/private/tmp/uipilot-jsc-esm.ImzPLT/`，协调者独立重建 `/private/tmp/uipilot-jsc-esm.oBL347/` 均为 6+8 全绿。源码清单含复用 supervisor，模块证据含每个输入/依赖、工具版本、锁文件与 bundle SHA-256，生成物清单含 native helper/header/bundle。独立审核核对 21 条源码哈希、34 条产物哈希和 11 个模块图一致，终审通过。真实 `demo-return` 入口 SHA-256 为 `37b16110f51c3be4fd39dbe3f3cd8e7494cb7a6d013c08db55ab1f8384be1812`，这是入口文件哈希，不是插件归档哈希。

本次只新增实验代码与记录，没有主程序变更，未重复整仓 UI/Rust 回归或构建人工安装包。生产模块 loader、权限 bridge、沙箱及资源配额尚未实现；Panel/Window 媒体隔离仍阻塞。当前生产同包双端通过数仍为 0，登录启动人工测试保持暂停，Mac 分支不并入 main。

## 2026-11-08 原样 ESM 与引擎资源限制实验

按[QuickJS 独立实验规格](../superpowers/specs/2026-11-08-macos-quickjs-runtime-probe-design.md)，新增 `spikes/macos-quickjs-runtime-probe/` 独立 Cargo workspace。固定 rquickjs 0.14.0/内置 QuickJS-NG 0.16.2，仅启用 std+loader；core 的 relative-path 为必要依赖 feature，sys 无额外 feature。首次 fetch 取得源码及独立锁文件，审阅 Mac 构建路径后均以 locked/offline 编译。主程序依赖和锁文件没有改动。

实际自动验证：**4 项 Rust 单测、11 项原生进程测试、6 项协议测试全部通过**。真实 demo-return 原样嵌入，不经转译，中文/emoji、内部空格、日期及完整动作数据匹配；未执行复制。合成图覆盖相对导入、重导出、循环/live binding、动态 import、Promise 和顶层 await；先等待模块 Promise 完成才调用命令。静动态越界、系统模块、外部字节码路径和 import attributes（包括已缓存模块）均拒绝；只有固定内存图，不读取插件外文件。

| 实际对照 | 结果 |
| --- | --- |
| 同一 4 MiB Uint8Array 分配脚本，堆上限 2/8 MiB | 低档 command 阶段失败，高档完成并保留分配；不是初始化失败，interrupt 未触发 |
| 同一 100 层非尾递归，栈上限 64 KiB/1 MiB | 低档 command 阶段失败，高档完成，interrupt 未触发 |
| 固定循环启用引擎 interrupt | 标志实际触发，返回 interrupted 诊断，进程正常退出并回收 |
| 关闭 interrupt 的循环、命令 pending、模块顶层 pending | 父 deadline 强制 kill/wait，逐次核对 PID 已回收，后续新 helper 正常运行 |
| 协议故障 | 双流合计限额、重复/畸形/非有限数/旧身份拒绝、完成后仍卡死不得接受、父读取异常回收通过 |

资源诊断外层 `status=ok` 仅指记录有效，测试另核 phase/outcome/限额/interrupt。`resources.json` 的 memory_usage 是 Context 仍存活时的执行后值，不是峰值或进程 RSS。最初 400 层递归在两档均失败，未算通过；改为同一 100 层脚本后才得到低高对照。本次为 debug/arm64 构建证据，不推广为所有构建配置或架构的固定递归阈值。

作者冻结产物 `/private/tmp/uipilot-quickjs-probe.I7Fv4o/`，协调者独立重建 `/private/tmp/uipilot-quickjs-probe.HkqWgl/` 均全绿；后者总日志 `/private/tmp/uipilot-quickjs-coordinator.log`。日志、resources、feature tree、Mac 限定 metadata、源码/锁文件/引擎源码与可执行文件哈希随产物保留。独立审核复核 29 条源码哈希及依赖/helper 哈希匹配，终审通过。helper 为 arm64，ad-hoc 签名验证通过，动态链接仅列 libSystem；这些检查不是 OS 沙箱证明。

本轮没有修改或构建产品宿主、SDK、Windows，未运行 GUI、音频、个人数据或权限服务操作。原样模块与引擎预算的实验已通过，但生产包加载/安装事务、Host bridge、撤权调度、OS 沙箱、进程整体资源预算与 Windows 原生验收仍待实现；独立 job 异常精确分类也保留为后续任务。Panel/Window 的 WK 媒体隔离仍独立阻塞，Mac 生产插件关闭，同包双端生产通过数仍为 0。无需人工安装新包，登录启动人工测试仍暂停。

## 2026-11-08 共享请求生命周期与双向子进程通道

依据[请求通道规格](../superpowers/specs/2026-11-08-macos-runtime-call-session-design.md)，新增产品目录共享 `call_session.rs`，复用真实 `data_call_gate`；最小提取插件 ID 校验器和五个请求/调用 DTO，Windows 原导出路径与 serde 合同保持不变。Mac 编译同源核心，尚未装配生产 Runtime 或 SDK 服务。独立 `spikes/macos-runtime-bridge-probe/` 以真实 QuickJS 子进程和明确命名的 `testMemory` 夹具验证双向通信，不把内存夹具当作 settings/storage 服务。

宿主不可序列化的 Arc 身份绑定请求、activation/admission 与截止时间；对端自报 ID 不能创建权限。调用、回复和最终提交分别验证当前身份与 gate。close 先拒新准入，管道适配器取消并回收子进程，再在锁外排空；已准入操作仍可完成，旧 drain 结束才确认它已退出。UI 最终 owner 检查与实际 SDK 授权接线仍待完成。

协调者主程序验证：all-targets check 退出 0，共享核心聚焦 **9 项通过**；双 preview 全库 **461 项通过、8 项默认忽略**，网络 proxy 子进程另 **1 项通过**。日志 `/private/tmp/uipilot-bridge-main-{check,focused,preview}.log`。本轮没有 UI 改动；未重复人工安装或登录启动测试，Windows 原生运行未执行。

独立审查发现并稳定复现完成时的 stderr 竞态：夹具先发 completion 并关闭 stdout，30 ms 后输出 9000 字节 stderr，正常退出，旧适配器错误接受。红证据 `/private/tmp/uipilot-bridge-stderr-red.log`；修复后必须消费两个输出流的结束事件、拒绝任何错误/额外消息，并确认正常退出，才准许最终提交。严格 typed frame 直接反序列化，重复字段与未知字段拒绝；父写入和子输出分别有累计预算，读写受父 deadline 监督。

最终独立实验 **19 项单测 + 5 项进程测试全部通过**：单测为新 session 9、既有 gate 7、DTO 2、严格帧 1；进程测试实际执行 **22 次子进程案例**，包含往返、阻塞撤销、错误身份、重复 call/completion、提前退出、迟到完成、部分 header/body、超大帧、尾字节、stderr 尾超限及失败后恢复，逐次记录 PID 和已回收状态。作者产物 `/private/tmp/uipilot-bridge-probe.wiXzQG`，协调者独立执行构建测试入口的产物 `/private/tmp/uipilot-bridge-probe.ZmE4Ww`，总日志 `/private/tmp/uipilot-bridge-coordinator.log`。实际运行签名后的 helper，SHA-256 为 `333de20eecf62d92da6f007ec44c1046f2bfedb0003e2b9c86b7a496ac309eb3`；原 Cargo target 文件哈希单列，源码清单包含同源核心及产品集成文件。主程序 Cargo 依赖/锁文件未变。证据中的 source-base-commit 是构建时已有 HEAD，新文件由源码哈希识别，不冒称当时已提交。

OS 隔离采用独立的[XPC 后续接入边界](../superpowers/specs/2026-11-08-macos-runtime-xpc-boundary.md)。本轮只核对公开指南和本机 SDK，不启动 XPC 服务；管道回收和 ad-hoc 签名不计为沙箱证明。生产 Mac 插件继续关闭，同包双端生产通过数仍为 0，Mac 分支不并入 main。

## 2026-11-08 XPC 连接前边界与固定服务打包

按[专项规格](../superpowers/specs/2026-11-08-macos-xpc-boundary-probe-design.md)新增 `spikes/macos-xpc-boundary-probe/`，仅固定 ping/pong App 与内嵌 service。无 QuickJS、插件源码、SDK port 或共享 CallSession 装配。产品主程序和 Cargo 依赖未改。本机签名查询为 **0 个有效身份**；用户确认暂时没有开发者账号，真实身份和沙箱验收延期，继续其他开发。

实际自动验证均通过：**3 个注入策略场景、8 个本地 XPC 协议案例、7 个 owner 断言、7 个严格 entitlement 断言、7 项打包测试**。策略测试只证明正常配置/owned 失败/borrowed 失败的顺序，不调用实际 connection。协议测试只构造本地 XPC 值。打包测试复制并重新签名临时副本，拒绝额外权限、integer 1 代替 boolean true、错误签名/Info 标识、链接和额外 service，并保留正常包正例。

native 预检先核正在运行的代码身份，再查磁盘 host/service，validity 成功后才信任 team/entitlements，比较 running 与对应磁盘代码的 team 和哈希，显式禁止网络校验。首次实现对 NSDictionary 的相等比较把 NSNumber 1 当作 true，红例 `/private/tmp/uipilot-xpc-entitlements-red.log` 复现；改为精确 CFBoolean 后通过。策略 setter 失败仍返回成功的红例为 `/private/tmp/uipilot-xpc-policy-red.log`。

实际运行了不带沙箱 entitlement 的 host `--preflight` 和无参数入口，两者均输出 `blockedSigningIdentity`、`connectionsCreated:0` 并退出 3。该数字是已审阅无连接分支的固定诊断，并非 OS 追踪计数；该分支不进入连接函数。service 即使自检也未执行，只有编译、签名和静态校验；没有启动 XPC、创建服务容器、GUI、音频、个人数据或权限操作。

作者冻结产物 `/private/tmp/uipilot-xpc-probe.5818W6`，协调者独立构建 `/private/tmp/uipilot-xpc-probe.S7G9Jn`，总日志 `/private/tmp/uipilot-xpc-coordinator.log`。19 条源码和 20 条产物哈希已逐项核对；实际 host SHA-256 `4087ca98105eabc2f6b0e9b604c91a33ae8b663331b625387a63e3862e555152`，仅静态验证的 service SHA-256 `1d51354dbf652f2f39b119c252f058457dfcff5326ebe2900cd0f5449680c3a9`。两者为 arm64、minOS 15.7.4、ad-hoc 签名；host 无 entitlement，service 最终签名中只有 app-sandbox=true。静态声明不代表沙箱效果通过。

真实签名下的 peer 认证、launchd 启动/重启、连接错误与服务终止、受控文件/网络拒绝，以及跨插件进程隔离仍未验证。新 connection 或取消回调不计为独占进程/进程退出。无需人工安装此探针；登录启动人工测试仍暂停，生产 Mac 插件保持关闭，同包双端生产通过数仍为 0。本轮未重复未修改产品的整库回归，Mac 分支不并入 main。

## 2026-11-08 插件私有数据基础

按[私有存储与设置规则规格](../superpowers/specs/2026-11-08-macos-plugin-data-foundation-design.md)提取同源 `data_scope.rs`、`storage_document.rs` 和 `settings_values.rs`，保留 Windows 原导出及 state/storage 调用行为。普通设置的定义检查、更新校验和升级 reconcile 已由真实 Windows caller 委派共享实现；Mac 编译同一套规则，未伪造持久配置、安装或权限服务。

Mac 新增 FD 相对的私有文件介质和实际 `PluginStorageStore`。根目录须已存在且私有，本单元以稳定锁对整个 root 排他；每次 owner IO 前核 scope 与真实 data gate identity，lease 跨完整磁盘操作。根/owner/current 的替换、符号链接、硬链接、非普通文件或不合权限均拒绝；首次 missing/存在状态绑定，只有本实例成功 rename 才更新 current 身份。rename 前失败保旧，rename 后同步结果不确定则禁止继续使用该 owner 的已打开存储，不能报告回滚成功。

共享 values 配额仍为 5 MiB、schema 1、原错误枚举；Windows 原 IO、backup 和 quarantine 不变。Mac 原始读取另限 5 MiB + 256，拒绝重复 JSON 键（包括嵌套）并保留递归限制；损坏不假装空数据库。已测试最长合法 pluginId 的文档额外编码在余量内。Mac 这些文件输入限制不扩大成 Windows parser 已修改或 SDK 已接通的结论。

验证结果：**14 项 Mac 存储测试、2 项共享文档测试、4 项共享设置测试通过**。覆盖真实 Unicode/JSON 读写、不同 owner 隔离、remove/重开、准确配额边界、损坏/超限/重复键、root/owner/current/lock 替换、current 的 symlink/hardlink/FIFO/mode、stage 安全回收、root 排他、提交前后故障和阻塞写时撤权。关闭 gate 后新写拒绝，实际 drain 观察仍有在途 lease，释放已准入写后才完成排空，新 identity 可读取其结果。

设置红→绿日志为 `/private/tmp/uipilot-settings-values-{red,green}.log`，两个失败分别为缺失升级保留和默认值合并；最终使用原 reconcile 规则通过。重复原始键真实红例 `/private/tmp/uipilot-plugin-storage-duplicate-red.log` 已修复。撤权测试另做**故障注入敏感性检查**：临时提前 drop lease 会失败于 `!drain.is_drained()`，日志 `/private/tmp/uipilot-plugin-storage-lease-injection-red.log`；最终源码已恢复持有 lease，不能将这项注入称为自然发现的生产缺陷。

协调者独立编译检查退出 0，storage 聚焦 **16/16**；双 preview 全库 **481 通过、8 默认忽略**，网络 proxy 子进程另 **1 通过**。证据归档 `/private/tmp/uipilot-plugin-data-evidence.1fai11uu`，包含 11 个变更 Rust 文件的哈希、锁文件哈希与上述红绿/编译/回归日志。baseCommit 记录归档时已有 HEAD，新实现用源码哈希识别。独立审核通过；Windows 原生回归未执行，不用 Mac 测试冒充。

本轮没有接入 Runtime/SDK gateway、持久插件设置事务、UI、卸载清理或同包双端生产验收；未运行 GUI、XPC 服务、个人数据或系统权限操作。无需人工安装新包，生产 Mac 插件仍关闭，同包双端生产通过数仍为 0。证书相关验收和登录启动人工测试继续延期，Mac 分支不并入 main。

## 2026-11-08 设置持久化与真实 Runtime 数据接线

按[专项规格](../superpowers/specs/2026-11-08-macos-settings-runtime-data-design.md)实现普通设置独立文档与宿主事务。复用私有 FD 介质，以固定 Settings 枚举选取 `settings.json/settings.pending`，storage 原布局不变；共享定义校验、reconcile 与 quota，保留设置 key 与 storage key 的区别。定义升级与批量更新合成一次持久提交，成功后才发布新 revision/句柄；非法更新不部分保存，同值 save 不增加 revision。旧句柄、跨 store/owner/identity、关闭 gate、损坏和提交不确定状态拒绝继续使用。

`RuntimeDataHost` 按关闭 gate/session → 取消传输 → 排空 → 新 epoch 配置事务 → 新请求的顺序切换；排空后已过期不开始保存。保存成功但随后请求发布过期时保留已保存配置，不能将无 endpoint 当作回滚。Runtime 只能调用普通 `settings.get` 和私有 `storage.get/set/remove`，使用宿主捕获 binding 与 CallPermit 的真实身份，不能从 wire 选择 owner、路径或定义；回复持有真实 ReplyPermit 到写入结束。此控制器显式创建新请求，不代替未来设置 UI 的同值保存路径。

设置层 **14 项**、Runtime 控制器 **7 项**通过，原 storage/document **16 项**回归通过。协调者 all-targets 编译检查成功；双 preview 主库 **502 通过、8 默认忽略**，网络 proxy 子进程另 **1 通过**。Windows 只扩大 setting validator 可见性、提取相同 quota helper 和增加只读 CallPermit identity getter，未改变 Windows IO、SDK 或生产启动行为；Windows 原生回归未执行。

冻结 bridge 探针 `/private/tmp/uipilot-bridge-probe.BB9HK9`：**80 项单测 + 5 项数据进程测试 + 5 项既有进程测试全部通过**，实际 **38 次 helper 进程均记录 PID 并回收**。原样 `demo-settings/package/runtime.js` 在真实 QuickJS 内四次读取落盘设置，宿主和 helper 重建后保持自定义值；另覆盖真实 JSON storage 重开、未知设置、伪造 owner、Runtime 设置写、重复 call、错误/缺失结果 ID 和已提交写后撤销。父进程独立检查原结果 ID，正确外层 envelope 不能修复错误内部归属。已提交写在取消后仍读得到；没有将撤销称为回滚。

实际运行的签名 helper SHA-256 为 `0605400920d67ab5d820838fd8de70987daa2839cd183a55ac9bc1ccc587c815`，32 条源码哈希已逐项核对。探针仍用 rquickjs 0.14.0/std+loader，仅独立 Cargo 加入同源数据模块依赖，产品 Cargo.lock 未改。首次完整探针因原探针 Cargo 版本 0.1 与真实 manifest 的 current Host 版本推导不符、schema 包装类型不一致失败；调整探针版本与产品 0.19 一致及同产品 `to_value` 包装后全绿，未跳过 manifest 测试。

红绿证据：`uipilot-settings-revision-red.log` 为缺失持久 revision 的真实红例；`uipilot-settings-combined-red.log` 复现原 configure→save 两步在非法更新时留下部分配置，修为单事务；`uipilot-data-result-owner-red.log` 为移除内层 requestId guard 的故障注入，错误结果原本被接纳、回归确实失败，最终恢复检查。主库、红绿和产物元数据归档 `/private/tmp/uipilot-settings-runtime-evidence.4vyq82g8`。探针 4 KiB 帧/8 KiB 累计预算是诊断限制，不代表完整 SDK 的传输容量。

此轮接通的是内部宿主数据服务与真实引擎，仍未装配完整安装/权限生命周期、UI/结果校验与发布、秘密服务或签名 XPC 沙箱。生产 Mac 插件保持关闭，同包双端生产通过数仍为 0；无需人工安装，证书及登录启动验收继续延期，分支不并入 main。

## 2026-11-08 主结果合同与内部注册表发布

按[主结果专项规格](../superpowers/specs/2026-11-08-macos-main-result-publication-design.md)继续无需开发证书的工作。`main_result_contract.rs` 原样承接 Windows 的 PublicMainResult、wire/limits/parser，`runtime_error.rs` 原样承接完整错误枚举和文本；activation/runtime 保留旧 re-export，Panel/Window 仍使用同一 64 KiB 限额。共享 **6 项合同测试**通过，逐字核对提取前后实现；缺共享 API 的红日志只代表编译接线尚未实现，不称原 Windows parser 存在行为缺陷。

新增 `runtime_result_macos.rs` 将 raw 唯一键检查、同源完整 parser 与真实 prepare/submit completion 票据接通。信息行可以进入内部路径；带 copyText 的响应因尚无真实插件动作授权/执行链而整体拒绝，不默默删动作。新 `main_result_macos.rs` 使用实际 ResultRegistry、现有 ResultItem/SearchResponse 和宿主 opaque IDs，行不携带可执行动作、复制载荷或插件自报 row ID。

publisher 的 opaque owner 绑定实例、query 和 Runtime binding；一次 pending→prepared→terminal 完成，准备失败也消费 owner 并条件清理。迟到错误不能清除成功结果；可信生命周期 retire 单列且仍须匹配原 query。新查询、其他 domain、hide/show、同数字的另一 registry 均不受旧 owner 清理影响。最终同步 publish_if_latest 在检查 query 的同一 registry 锁内写入；runtime completion permit 已获准也不能跳过此检查。

主库 **12 项 runtime result/实际 registry 测试**通过，包括真实提交闭包已准入后暂停、新 plugin query 发布、再释放旧闭包，旧提交返回无结果而新 action 哨兵仍可解析。故障注入临时移除最终 query sequence 检查后，该回归稳定失败（旧提交错误返回 Some），日志 `/private/tmp/uipilot-main-publication-owner-red.log`；检查已恢复，`uipilot-main-publication-green.log` 全绿。这是敏感性证明，不称 Windows 原实现新发现缺陷。

all-targets 检查成功，双 preview 全库 **520 通过、8 默认忽略**，网络 proxy 子进程另 **1 通过**。冻结 probe `/private/tmp/uipilot-bridge-probe.oSGhCn` 为 **91 项单测 + 6 项数据进程测试 + 5 项既有进程测试**全绿，**44 次实际 helper 均记录 PID 并回收**。原样 demo-settings 结果经完整 parser 与 Runtime 完成票据后从 typed rows 生成诊断输出；未知字段、重复 row ID、21 行、不支持 copyText 拒绝，空结果合法。

证据明确分层：真实 QuickJS → parser/完成票据是独立探针；实际 ResultRegistry/显示 DTO 是产品主库测试，未将两者描述为已连通的 GUI 端到端。测试的签名 helper SHA-256 为 `01178990dedeef29dd6f941ea4d2961f826de9964158be602f23cd81f1eee2c9`；42 条源码/夹具哈希已核对。证据生成器补纳外部测试文件后重新生成清单，未改测试代码或产物。日志与元数据归档 `/private/tmp/uipilot-main-result-evidence.gsh9ix5g`，baseCommit 为当时已有 HEAD，新增实现由源码哈希识别。

未装配生产插件安装/升级/权限、主窗口 IPC/真实显示、动作执行、Panel/Window 或签名 XPC；本轮无 GUI、剪贴板、个人数据、系统权限操作。Windows 原生回归未执行，生产 Mac 插件仍关闭，同包双端生产通过数仍为 0。无需人工安装，继续保留用户延期的证书/登录启动验收，分支不合并 main。
