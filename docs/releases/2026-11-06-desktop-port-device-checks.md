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

## 2026-11-08 内部插件安装、升级与权限生命周期

**范围：** 在 `codex/macos-port-plan` 接通内部 Host 的真实目录/标准 ZIP 包快照、原子 inventory、升级/普通设置/启停/权限管理与 Runtime。生产公共插件入口仍关闭，未接安装 UI、启动扫描或签名 XPC。相同插件包双平台生产宿主验收通过数仍为 **0**。设计和执行记录见 `docs/superpowers/specs/2026-11-08-macos-plugin-install-lifecycle-design.md`、`docs/superpowers/plans/2026-11-08-macos-plugin-install-lifecycle.md`。

已安装代码来自不可变已验证快照；来源目录后续改写或删除不影响已安装版本。单份固定 owner inventory 原子保存代码快照、设置、enabled、grants 和 generation/revision，管理事务检查批准/冲突/容量/溢出后才关闭旧运行时。升级保留停用状态与旧撤权，移除已删除的权限；过时 consent token 不可复用。普通设置按真实 manifest 规范化，Runtime 直接读取权威快照，不由旧派生设置文件覆盖。

Retirement 绑定本次真实 request，显式关闭、升级/停用/撤权/设置变更和库存损坏会先关准入，再退休结果并取消传输，排空在途调用。Drop 关准入并触发清理，但不当成 drain 成功。rename 前失败保留旧文档但不复活旧 binding；rename 后不确定状态关闭直到重新打开。实际 ResultRegistry 测试证明已发布结果被清理且不误删更新 query。ClipboardWrite 仅内部授权状态与可信测试 effect；未执行系统剪贴板效果，copyText 仍拒绝。网络/凭据等范围权限没有开放。

**验证：** 包/共享规则 **13 项**、生命周期 **14 项**、真实注册表生命周期 **2 项**、Runtime 数据 **10 项**通过。主库双 preview 完整回归 **553 通过、8 默认忽略**，网络代理隔离子进程另 **1 通过**；all-targets 检查通过。升级撤权敏感性测试临时恢复所有声明权限后确实失败，恢复正确实现后全库通过；ZIP 库原始同名条目被去重的风险也有独立红/绿证据，Mac 预检在解压前拒绝重复。Windows 原 ZIP IO 本轮未改，不把 Mac 修复算作 Windows 修复。

冻结探针 `/private/tmp/uipilot-bridge-probe.50VYyo`：**122 项同源单测、6 项数据进程测试、2 项安装进程测试、5 项既有协议进程测试**全绿。**51 个不同 PID 全部回收**，协调者与独立 reviewer 已核对来源/执行证据，最终协调者复核 **53 条来源哈希**一致。实际执行的 ad-hoc 签名 helper SHA-256 为 `67c4593be97b77a0fe6dde6636297753ba443107ed11d3aba0a9bb68c286a7cd`；probe Cargo.lock 为 `630e4b5af5c50add131558401716343635163029faad2e2cf5ed3ed8a2b15043`。主库日志与行为敏感性证据保存在 `/private/tmp/uipilot-install-lifecycle-evidence.agapqp_s`。

新增真实进程场景执行原样 demo-settings：安装并保存设置 → 删除临时来源 → 从安装快照执行 → 同一测试进程 drop/load 控制器并启动新 helper → 设置与代码恢复 → 升级临时包版本/build 标记并执行新输出；另外在 helper 等待首个设置 reply 时停用/撤权，捕获的清理回调取消并回收它，旧 binding 不再可用。此处“恢复”是控制器重开与新 helper PID，**不是 Host 主进程重启的实机验收**。

**保留限制：** 内部单包编码与整库各限 5 MiB；标准单盘非 Zip64 归档；执行探针仅单入口 2 KiB ESM，不装模块图。生产 launcher 保留命令/用户覆盖配置、卸载/数据清理、管理 UI、真实权限服务、签名 XPC/沙箱、Windows 原生回归与双端同包实测仍待完成。开发者证书缺失不妨碍本轮内部事务测试，但这些结果不能替代公开发布或生产加载验收。未操作 GUI、系统权限、个人文件、剪贴板或凭据。

## 2026-11-08 macOS 插件管理设置页

开发分支新增独立 `pluginManagement` 能力与「设置 → 插件」管理页，`plugins` 执行能力保持 false。可查看/筛选安装清单，使用原生选择器准备归档或开发目录，确认安装/升级，保存普通设置、启用配置和 clipboard.write 配置，以及移除安装。界面明确当前插件不能运行；不接旧插件调用、启动命令改名/快捷键、secret 或真实 OS 权限服务。移除确认明确删除安装和普通设置、保留插件私有存储数据；重新安装需要重新授权。

main-origin ACL 精确增加八个管理命令，前端不获通用路径/dialog 权限。所有变更校验十进制 u64 revision，准备 token 短期单次且绑定当前选择会话；窗口隐藏/退出不等待 IO 锁即可退休旧 owner。读取包时持有原生 NSURL 与成功取得的 scope；不虚报系统授权。主程序启动时不加载管理库，首次访问才在固定私有子根初始化。

Storage/CommitUncertain 错误先撤销旧选择和 token、释放控制器与锁，下次明确请求重新核对磁盘；UI 先要求刷新，不自动重试写入。真实 AfterRename 故障测试验证实际提交被读回且文件字节不被重写，旧 token 拒绝。已准入事务可以完成，关闭只让旧响应变 stale，不声称回滚。

**自动验证：** 前端完整 **811 通过、18 跳过**；Rust 双 preview 完整 **565 通过、8 默认忽略**，网络代理隔离子进程另 **1 通过**。管理/IPC 聚焦 **12 项**、精确 ACL **4 项**通过。TypeScript/Vite 生产构建和 Rust all-targets 检查通过。生产 bundle 不含浏览器预览 fixture 标记。独立审查 PASS。执行能力错误地随管理能力开启、以及失败后继续保留 poisoned 控制器，两条行为变异均留下真实红/绿证据。

前端验收覆盖真实 Launcher 下 Mac 管理 tab 不调用旧 legacy/public/builtin 插件服务；普通设置的合法 constructor 键、多行文本、false/空值；草稿切换/返回/刷新/移除确认；安装和放弃确认的 Escape；同一时刻重复提交；旧客户端迟到回包；跨挂载客户端串行读写。列表、详情、主按钮、输入框和滚动区域以真实 Launcher 组件的独立浏览器 fixture 检查了浅色/深色，另检查 720 像素窗口。预览不连接真实安装库，也不打包到生产入口。

日志与记录：`/private/tmp/uipilot-management-evidence.wlt8x19o`；浏览器截图：`/private/tmp/uipilot-management-visual.lc6qg7f2`。设计与计划分别为 `docs/superpowers/specs/2026-11-08-macos-plugin-management-ui-design.md` 和 `docs/superpowers/plans/2026-11-08-macos-plugin-management-ui.md`。

**尚未实机验证：** Tauri 原生选包/TCC、真实应用窗口焦点、管理包安装后交互。没有启动 UiPilot GUI、修改用户授权或个人安装库。开发者签名/XPC 沙箱、插件生产执行、完整模块图、真实能力服务及双端同包生产验收仍未完成，同包双端生产宿主通过数仍为 **0**。这些待验收项未计入本轮自动通过。

### 管理页键盘与安装确认补充

插件列表补齐上下键及 Home/End 焦点导航，只访问当前筛选后可用行；边界停留，输入法组词（含 keyCode 229）及修饰键不触发导航。进入详情再返回会恢复原行焦点。替换已安装插件时同时显示当前版本与目标版本，按钮明确为「确认替换」；不把同版本重装或较低版本误称为升级，不改变 Host 的安装准入规则。新安装仍为「确认安装」。

管理 UI/client 与实际 Launcher 回归 **416 通过、18 跳过**，生产构建通过。键盘导航和版本展示已确认旧实现测试失败，再修复通过。浅深主题在独立浏览器模拟安装库中检查；仍未操作真实应用或系统授权。本轮仅前端行为和文案，未重复计算上一轮 Rust 测试为新增通过。

## 2026-11-09 主程序与插件运行协调接线

主程序的 `search_apps` 在可信 main 校验后接入运行协调器；与管理 IPC 共用同一 ManagementState/MacPluginLifecycle 和主 ResultRegistry。默认工厂无执行适配器，在解析 appdata、读取主题和派发 worker 前就跳过插件路径，`plugins=false` 保持，未新增 IPC/ACL 或环境开关。窗口唤起、隐藏和退出接入请求失效；原应用、计算、Find 查询继续走原路径。

内部适配器可用时，明确命令才按需恢复并重验安装库，无需先打开管理页。Live 随匹配查询调用，Submit 只复用现有 `submit=true` 才调用，必填空输入、NUL、超过 64 KiB 的输入不进入传输；内置命令保留优先级。Host 选择不可变安装快照，构造稳定 commandId、requestId、Mac 平台、主题和 RFC3339 时间，传输在管理锁外使用真实设置/存储绑定。正常终结后才进入原 MainResultPublisher，生成真实注册表的 opaque ID 和无动作信息结果。

新查询、停用、撤权、升级、设置更新与隐藏立即关闭旧 gate/session 准入，后台按捕获的 owner 等待在途调用。修复并验证：旧查询等待管理锁期间新请求已运行，旧查询恢复不能 close 新请求；旧 reset 通知晚到不能取消新 epoch 请求；即时 revoke 后保留原许可供后续 close 真正等待。失败、超时、畸形响应及 copyText 动作不会被发布。退出只承诺关闭准入并请求取消，不把后台清理等同于退出前已完成进程回收。

**自动验证：** 协调器、管理、数据运行时与会话聚焦 **41/41** 通过；主库双 preview 完整 **579 通过、8 默认忽略**，网络代理隔离子进程另 **1 通过**。Rust all-targets 检查及精确 ACL **4 项**通过，独立源码审查 PASS。三项行为变异分别去掉锁内准入、旧 epoch 匹配和等待 drain，测试均真实失败，恢复后全绿。此前受限沙箱中的广筛出现 9 项旧网络测试 bind EPERM，授权本机端口环境的完整重跑已通过，不把失败那次记录为通过。前端没有变更，未重复跑前端全套。

**证据与边界：** `/private/tmp/uipilot-launcher-runtime-evidence.vrfhz1zx`。本轮使用受控模拟传输，配合真实临时安装库、设置/存储、会话许可和结果注册表，验证 Host 编排；没有启动真实 JS helper、XPC、GUI、用户安装库或系统授权。原有真实 QuickJS 探针的证据不冒充本轮端到端证据。生产隔离适配器、完整模块图、命令补全/稳定身份路由、正式界面显示、Panel/Window 与同包双端生产验收仍未完成。同包双端生产通过数仍为 **0**，分支不合并 main。


## 2026-11-13 Windows 虚拟机同包人工验收（进行中）

设备为 M1 上的 Parallels Windows ARM64，运行 x64 应用；不等同于原生 x64 Windows 实机覆盖。候选主程序源码 `41ff5f7`，打包路径修复 `63d4df7`；已安装 setup SHA-256 为 `698cd62532209ea82427954af4183da07a02874f477cbb7dc7f92c61c9a20121`。共享 `build-result.json`、哈希文件和实际安装包已核对一致，Windows 归档管理测试通过。

用户逐步确认：安装、启动、`/find` 可用；demo-settings 保存 `Windows test` 后 Runtime 显示新值，退出重启保留；升级至 1.0.1 后文字、数量及其余默认设置保留。demo-return 正常生成并复制结果，经记事本粘贴核对一致；恢复写入权限后再次生成并复制成功。demo-multi 的 live 大写与 submit 小写命令均正常；两条命令共用 `[shared] ` 前缀；包级停用隐藏两条命令且不影响 demo-return，重新启用后恢复；退出重启后两条命令均保留前缀。

未通过/待查：demo-return 撤销 clipboard.write 后只显示“操作不可用，请重试”，多次请求后用户观察到插件关闭，恢复权限后用户观察到自动开启。权限拒绝可能被计入 Runtime 连续失败，但完整原因尚未确认。最初“撤权后仍应显示带复制动作的结果”的人工指引有误；合同在发布结果阶段即检查授权，不能将该预期作为产品验收依据。此缺陷尚未修复。

用户另报告主结果列表不能滚轮滚动、滚动条不能拖动，右键命令出现 Windows 系统窗口菜单（含移动、最小化、关闭），而非应用内“设为常用”。已缩小 CSS 原生拖拽范围：移除整个 launcher-surface/launcher-view 的拖拽矩形，仅在搜索栏及原有底栏区域保留，显式排除完整结果区域；macOS 专用顶部拖拽条保留。聚焦的拖拽边界、收藏和上下文菜单前端检查 29 项通过。原生 WebView2 命中测试不在 jsdom 覆盖内，需新 Windows 包回归后才能关闭这三项缺陷。收藏与命令独立改名尚未完成验收。


## 2026-11-13 Windows 鼠标交互回归与命令状态验收补充

用户安装拖拽范围修复包后确认：主列表滚轮可滚动、滚动条可拖动，窗口位置均不变；右键显示应用内“设为常用”，不再显示 Windows 系统菜单。共享目录实际修复包 SHA-256 `3a88940b652ccb8362c69533069775d7e421d065e22b335dc174857ea6090f23` 与 `build-result-ui-fix.json` 一致，这三项人工回归通过。

用户收藏 upper 后，切换插件详情到 lower 时星标不亮，切回 upper 点亮，确认收藏按命令独立。upper 改名为 `/win-upper` 后新入口正常返回 `[shared] HELLO WIN`，旧 `/demo-upper` 不再可搜索，lower 名称不变；新入口星标保留，退出并重启后用户确认收藏仍生效。

撤权问题的源码原因已定位：Windows legacy mainResult 响应的复制权限检查把当前授权缺失返回为 InvalidOperation，并计入 Runtime 连续失败。候选修复区分声明缺失与当前授权缺失；后者返回 PermissionDenied，不计入故障次数，在启动器映射为现有 pluginPermissionDenied 提示；前者仍按协议故障拒绝。没有放宽未授权结果的发布/复制。新增服务级回归覆盖连续六次拒绝、配置保持启用且无故障、恢复权限后新的响应成功，以及未声明复制动作三次仍停用；命令错误映射另有用例。已完成 Rust 语法解析与 diff 检查，Windows 原生编译和这些回归尚待共享脚本执行，不标记为已通过。


## 2026-11-13 Windows 撤权修复验收通过

候选修复提交 `73ab26a1d8a1fcd8f74597efb330cbaa0866b457` 的 Windows 原生三项回归及插件归档管理测试全部通过。实际 setup SHA-256 为 `8fdfe9a75230cf03de05d6a8f7ea23114aee6574fdfa02d0c83e25f8ab37408a`，与共享构建记录 `build-result-permission-fix.json` 和哈希文件核对一致。

用户重新安装并重启后确认：关闭 demo-return 写入权限时插件仍启用；首次请求显示明确的“插件所需权限不可用，请在『设置 → 权限』中检查。”；随后再连续请求五次，每次均为权限提示，插件没有自动关闭；恢复权限后结果输出和复制均正常。此次回归关闭“权限提示不明确”和“撤权后的连续请求导致自动停用”两项已复现缺陷。

这些结果仅覆盖本次 Windows ARM64 虚拟机中的 x64 应用及上述插件路径，不代表所有插件能力、macOS 生产运行时或双平台总目标验收完成。


## 2026-11-13 macOS 撤权语义与同包诊断回归

Mac 主结果发布区分“已声明、但当前被撤销”的复制权限与未声明动作：前者保留 `pluginPermissionDenied` 至启动器提示，后者仍拒绝为不支持的动作。连续六次拒绝不改变启用配置；重新授权后只接受新请求的单次复制。显式停用后重新授权仍保持停用，重新加载安装库也一致。生产执行开关保持关闭。

自动验证：Mac 双 preview 主库 **583 通过、18 默认忽略**；真实 QuickJS helper 自身 **123 单元、6 数据进程、2 安装进程、5 进程测试通过**。使用 Windows 人工验收的原始共享归档（ZIP SHA-256 `f5c0c465fa7e4ba2f98f887decc7a375ed826c4c3761e7edb227c007fe6484dc`），设置 seed 与重开/升级、multi、return 共 **4 项通过**；复制准入进程专项另 **1 项通过**。return 归档覆盖连续撤权拒绝、保持启用及重新授权后新的结果可复制。

证据：`/private/tmp/uipilot-macos-permission-full.log`、`/private/tmp/uipilot-permission-probe-build.log`、`/private/tmp/uipilot-permission-archive-seed.log`、`/private/tmp/uipilot-permission-archive-tests.log`、`/private/tmp/uipilot-permission-process.log`。所有安装库、Runtime 数据和复制接收器为隔离测试对象，没有操作用户剪贴板或个人安装库。诊断 helper 是临时 ad-hoc 签名的 QuickJS 进程；不代表签名 XPC 沙箱、Mac 生产插件运行或双端生产同包验收通过。


## 2026-11-13 macOS 按命令收藏与改名

管理清单和设置页已接通稳定 commandId 下的独立收藏、启动名称覆盖及恢复默认。升级只保留仍存在的命令配置，卸载删除配置；旧清单不含 commands 字段也能加载。Host 拒绝无效名称、内置命令和其他已安装命令冲突，失败不改变清单；变更沿用 revision 与生命周期退休机制。内部启动器目录按有效名称补全并带收藏身份，改名不改变 Runtime commandId。

验证：Mac 主库双 preview **588 通过、18 忽略**；相关前端与真实 Launcher **419 通过、18 跳过**；TypeScript/Vite 构建、Rust all-targets、精确 Mac ACL 四项通过。新增行为覆盖旧清单重开、独立命令、升级默认名变化、移除命令与卸载重装、名称冲突无副作用、旧 Runtime 权限失效、按新名称路由及收藏元数据。浅色/深色真实管理组件在独立浏览器 fixture 中检查了 720 像素宽度；未读取用户安装库。

证据：`/private/tmp/uipilot-command-full.log`、`/private/tmp/uipilot-command-ui-all.log`、`/private/tmp/uipilot-command-check.log`、`/private/tmp/uipilot-command-acl.log`。生产 Runtime 仍关闭；管理 UI 的原生安装、收藏改名及重启恢复需下一节点人工验收。

同一份原始 demo-multi 归档的真实 QuickJS 诊断还验证了改名后新入口仍调用 upper、旧入口不再路由、lower 不变，以及销毁并重建 Host 后收藏与名称保留。专项 1 项通过，日志 `/private/tmp/uipilot-command-real-archive.log`；仍不替代生产 GUI 运行验收。


### macOS 插件管理人工验收候选已生成

候选源码 `1bf5994` 已推送到 `codex/macos-port-plan`，未合并 main。安装包通过 arm64、最低 macOS 15.7.4、共享资源、签名与 Documents entitlement 校验；本机 preflight 通过。release 构建保留符号表（`CARGO_PROFILE_RELEASE_STRIP=none`），解决临时工具链缺少 rust-objcopy 的打包问题；不改变应用权限或运行开关。

- DMG SHA-256：`0302be8bbdfa554e1bac7fa6f9c2b63e98e214bd1a8bc29651e570c372d715ca`。
- 主程序 SHA-256：`800abedc99d8d2b53458afd012418137d7eafe5b7303071aa1295e7ecf88c570`。
- 交付 ZIP：`/Users/mobytang/Documents/UiPilot-Mac-plugin-management-1bf5994.zip`，SHA-256 `6fd549d6bbb17f390e5054c0832e3df34df9fdc4d43106af07bf6c32e4754398`。

ZIP 包含 DMG、固定哈希 preflight、Windows 已测的四份原始插件归档、校验日志、候选 manifest 和操作说明。ZIP CRC 与 manifest 中 15 个文件哈希逐项验证通过。只完成命令行构建和静态包校验，没有安装或启动候选应用；原生导入、设置持久化、升级、独立命令收藏/改名和重启恢复均待用户人工验收。生产 Runtime 仍关闭，本节点不要求运行 Mac 插件。用户明确要求之后不使用 computer use，全部 GUI 操作通知用户并由用户执行。


## 2026-11-13 macOS 插件目录人工反馈与修复

用户在 `1bf5994` 候选上确认：demo-settings 安装成功，问候文本保存为 `Mac test` 后完整退出再启动仍保留；升级到 1.0.1 后版本和问候文本正确。demo-multi 安装及 upper/lower 详情可见，但分别收藏后，主界面空查询、`/demo` 和带参数的完整命令都没有相应条目。保存普通设置也没有在当前可见区域提示成功。这是主界面接线缺陷，不能将收藏验收记为通过。

修复将已安装命令元数据展示与 Runtime 执行分开：启用的插件命令按已保存名称参与空查询、前缀、完整命令及带参数查询；每个 commandId 的收藏独立显示并可从主界面取消或设置常用，收藏项优先。停用或移除不再展示，改名后旧名称不再匹配。当前 Mac 版本不能运行的条目明确标注状态，回车显示原因；这些条目无原生执行目标、无插件动作或资源加载，不启动 Runtime。普通设置成功提示移到保存按钮旁的状态区。

自动验证：Mac 双 preview 主库 **590 通过、18 默认忽略**；Launcher 与 Mac 管理前端 **421 通过、18 跳过**；TypeScript/Vite 和 Rust all-targets 检查通过。覆盖磁盘重开、独立命令收藏/改名、停用与移除、过期查询和隐藏窗口不发布、无法执行的结果无注册目标、前端严格身份校验与收藏刷新。前端首轮曾因超时连带 mock 清理失败及两处新增测试断言问题失败；修正测试等待与无效 ID 样本后完整重跑通过。

证据：`/private/tmp/uipilot-catalog-lib.log`、`/private/tmp/uipilot-catalog-ui-rerun.log`、`/private/tmp/uipilot-catalog-check.log`、`/private/tmp/uipilot-catalog-build.log`。本次未操作 GUI，未安装或启动候选应用；主界面原生展示、收藏联动与保存提示仍需用户在新包上继续验收。Mac 生产 Runtime 仍未开放，不计为插件执行通过。

### 插件目录修复候选 a26eacd

源码 `a26eacd` 已推送至 `codex/macos-port-plan`，未合并 main。修复包通过 arm64、最低系统版本、共享资源、ad-hoc 签名及 Documents entitlement 校验，本机只读 preflight 通过。原生交互等待用户复测，没有使用 computer use 或自动操作 GUI。

- 交付 ZIP：`/Users/mobytang/Documents/UiPilot-Mac-plugin-catalog-a26eacd.zip`。
- ZIP SHA-256：`36247f35a2b5d00ec0976987be2f9a636e2a93e5cbce38afacea009bbe5a5782`。
- DMG SHA-256：`f5308ea8e4ddb78d0b71247705d8f24924f74c2c24d91ecc486b8533efbd8197`。
- 主程序 SHA-256：`29623069f9fe53087b7294287ab7e32689f904a9a62e93cfee9f0993f5d72b36`。

ZIP CRC 及 manifest 的 16 个文件哈希全部验证通过，包含新 DMG、固定哈希 preflight、原始四份共享插件归档、说明及测试日志。无需重新导入已有插件；下一步由用户完整退出旧应用，再按单步指引替换并继续验收。


## 2026-11-14 插件目录修复人工验收与保存状态位置调整

用户替换并重新打开 `a26eacd` 后逐项确认：

- 空查询和 `/demo` 前缀都能显示 upper/lower，星标正确，并有当前 Mac 暂不可运行说明。
- 主界面取消 upper 常用后 lower 不变，详情 upper 星标同步熄灭；详情重新收藏后主界面同步点亮。
- `/demo-upper hello` 回车显示明确不可运行提示，窗口保持打开，没有执行插件。
- upper 改为 `mac-upper` 后可按新名称搜索，旧名称不再命中；lower 名称不变，upper 收藏保留。
- 完全退出后重新启动，`/mac-upper` 和收藏仍保留。
- settings-demo 之前保存的 `Mac test` 仍在。保存设置成功提示已经显示，但用户要求统一移到底栏左侧。

本次将普通设置保存成功提示接入宿主底栏左侧，沿用成功色和可访问状态播报；移除按钮旁重复提示。继续编辑、离开详情或组件卸载时清除这条提示，其他设置页面不显示它。右侧版本号与操作入口沿用原布局。以上目录、收藏和名称人工验收通过；新提示位置待新包人工确认，插件生产执行仍未开放。

验证：相关前端 **417 通过、18 跳过**；TypeScript/Vite 构建通过。沿用已有底栏双主题成功色、单行截断和版本号/操作入口布局。没有运行 GUI 自动化，实际位置待用户确认。日志：`/private/tmp/uipilot-footer-ui.log`、`/private/tmp/uipilot-footer-build.log`。


保存提示修复候选 `1271793` 已构建并通过签名、架构、最低系统与 Documents entitlement 检查，只读 preflight 通过。App 使用 `--bundles app`，DMG 使用 `hdiutil create -format UDZO`，没有运行 Finder 外观自动化或启动应用。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-status-footer-1271793.zip`，SHA-256 `ed0226f9157116f344765f22130fd083b03dae3b60f0c36e46d8981b7c4b36fe`。
- DMG SHA-256：`4c035afecb33e085390196e9d6634e9772cdaf1835e9e207d69fa9a8a6e4e4da`。
- 主程序 SHA-256：`ed55ebc9f30bcb433ed3b6eba15239d50345db10aa06d95ded3aa184b08bb704`。

ZIP CRC 与 manifest 中 8 个文件哈希逐项通过。只需用户在新包确认底栏左侧保存提示，已有插件无需重装。

2026-11-14 用户替换并重新打开 `1271793` 后确认：保存普通设置时，“设置已保存。”显示在窗口底部左侧，按钮旁不再出现提示。保存反馈位置人工验收通过；重新编辑后的提示清除尚待确认。


## 2026-11-14 继续人工验收及名称冲突输入回退

用户确认：重新编辑普通设置时底栏“设置已保存。”清除，保存按钮恢复可用；取消修改恢复上次保存值，保存按钮禁用。upper 恢复默认名称并保存后 `/demo-upper` 可搜索且保留星标，旧 `/mac-upper` 不再匹配。

将 upper 名称改为 `find` 时 Host 正确拒绝冲突。截图仍显示已保存命令 `upper · /demo-upper`，输入框保留 `find` 草稿；用户点击取消名称修改后恢复 `demo-upper`。这是未保存草稿残留，不是已保存名称被覆盖。用户要求冲突时直接丢弃本次输入。

已调整启动名称保存的冲突处理：仅明确收到 `pluginManagementConflict` 时丢弃名称草稿，恢复当前已保存名称，显示“启动名称与内置命令或已安装插件冲突，已恢复原名称。”，解除待核对锁定，可以直接输入其他名称，不自动重试。再次编辑会清除旧冲突提示。其他保存错误仍沿用原有核对流程，不擅自丢弃输入。原名称、收藏、其他命令及权限配置不变。

相关管理组件与客户端测试 **19 项通过**，覆盖字符串及对象形式的冲突、自动回退、可以再次编辑、无自动重试和独立收藏。新包原生行为待用户验收。


名称冲突修复候选 `aef913a` 完成 TypeScript/Vite 与 release App 构建；通过签名、架构、最低系统和 Documents entitlement 校验，只读 preflight 通过。App 使用无界面构建，DMG 由 hdiutil 生成；没有安装或启动 GUI。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-command-conflict-aef913a.zip`，SHA-256 `7300a99789ed51f44aef9b91f7221160963aff53928d982e2552deb7b4d1ebb7`。
- DMG SHA-256：`3dde2020acc713c03c350f9233006131cbfcbe25e772929b3d7cd78ffcdf685a`。
- 主程序 SHA-256：`56078f631677ea23201b5ca5bd3a646d0e2a23ce73192fa2d748498904e66573`。

ZIP CRC 与 manifest 中 7 个文件哈希验证通过。新包只需人工确认冲突自动回退，已有插件及此前验收结果保留。


## 2026-11-14 Mac 插件管理统一底栏提示

用户替换 `aef913a` 后看到“启动名称与内置命令或已安装插件冲突，已恢复原名称。”，要求所有操作提示统一放在底部左侧。此前只迁移普通设置保存成功提示不够完整。

本次统一管理页的处理进度、刷新、安装、启用/权限/名称/收藏保存、冲突、读取/写入错误、缺少管理服务和输入校验至宿主底栏左侧，使用结构化消息与语义颜色。删除内容区和按钮旁的重复提示及预留空白；字段仍有 aria-invalid，并关联底栏可访问状态。静态功能说明、空列表和确认对话框保留各自用途。切换到其他设置页面不显示旧管理提示。已将统一位置写入 UI 规范，防止后续只迁移单条消息。

验证：相关前端 **423 通过、18 跳过**，TypeScript/Vite 构建通过；浅色/深色的 Launcher DOM 回归实际保存与冲突事件，确认消息只出现在底栏、颜色正确、冲突恢复原名且切换通用页后不残留。没有运行 GUI 自动化；实际位置仍由用户确认。日志 `/private/tmp/uipilot-unified-status-ui.log`、`/private/tmp/uipilot-unified-status-build.log`。


统一状态候选 `a9a3dcf` 完成无 GUI 的 App/DMG 构建，通过签名、架构、最低系统和 Documents entitlement 检查，只读 preflight 通过。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-unified-status-a9a3dcf.zip`，SHA-256 `96c094e74481e73c92e7d1841af472b5379718ce2d29cdcbf6b313235d1abdd1`。
- DMG SHA-256：`784fe27ba5f3a7200cb9f6235219b87306a04de4ff8512189015fe9b35f76524`。
- 主程序 SHA-256：`da1fe1a9773e9eed944bd8fadec8fab1ffb41d3e38a910a86eb5bf6990b8ec22`。

ZIP CRC 与 manifest 中 8 个文件哈希验证通过。新包待用户确认冲突及其他操作提示均位于底栏左侧；既有插件无需重新导入。


## 2026-11-14 Mac 设置窗口图钉

用户在 `a9a3dcf` 新包确认：保存 `find` 时自动恢复 `demo-upper`，冲突提示只显示在底栏左侧；输入 `Two Words` 时底栏显示格式提示，保存按钮禁用，已保存名称不变。统一提示和格式校验人工验收通过。用户随后要求设置界面与 Windows 一样，有图钉固定窗口，切到其他应用时不自动隐藏。

Mac 设置页复用 Windows 的右上图钉、Ctrl+T、忙碌态、失败重试与成功后状态更新。新增受本地主窗口 ACL 限定的 Mac `set_settings_pinned` 命令，在主线程更新窗口置顶层级，并将失焦保留状态绑定到当前 invocation。固定后失焦不隐藏；取消固定或退出设置页恢复普通行为；显式关闭、退出和新窗口会话清除旧固定状态。旧会话延迟的固定/取消请求拒绝，不影响新会话。快捷键录制和操作菜单继续优先处理自己的按键。

没有运行 GUI 自动化；真实跨应用失焦、置顶层级和取消固定需新包人工确认。此改动不开放插件执行或其他未完成能力。

验证：Mac 双 preview 库 **592 通过、18 默认忽略**；Rust all-targets、TypeScript/Vite 和 4 项 Mac ACL 检查通过。Windows/Mac 图钉行为与 Mac 真实适配器专项 **3 项通过**；涵盖点击、Ctrl+T、重复按键、失败重试、退出设置释放和会话参数传递。原生模型测试覆盖失焦保留、取消、离开设置、新会话及过期请求。


设置图钉候选 `5557de0` 完成无 GUI 的 App/DMG 构建，通过签名、架构、最低系统和 Documents entitlement 检查，只读 preflight 通过。最后一轮完整 Launcher 前端回归 **405 通过、18 跳过**。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-settings-pin-5557de0.zip`，SHA-256 `b522d2de299c218340d885f8855c966e88fae69fcb9c27719841b5a41e61f0c7`。
- DMG SHA-256：`e4db27c61019aff5458507e2ebd221859afbcc7b9fd2bdb7a160ad62d7518a26`。
- 主程序 SHA-256：`f6ac59d6d4efd6cb0a4d9b4077a4e98147d5dcc8ec9265cc7908b704af2d4db5`。

ZIP CRC 与 manifest 中 12 个文件哈希验证通过。真实固定后跨应用保留与取消固定仍待用户手动验收；没有使用 computer use。

### 设置图钉人工验收通过

用户替换并重新打开候选 `5557de0` 后，逐项手动确认：

- 点击设置页图钉后点亮，悬停显示“取消置顶”，无报错。
- 点击其他应用，设置窗口仍保持显示并置顶，没有自动隐藏或最小化。
- 取消置顶后图钉熄灭，无报错；再次点击其他应用恢复正常自动隐藏。
- 在设置页按 Ctrl+T 后图钉点亮，窗口进入固定状态。
- 固定状态下点击返回箭头，正常返回主界面，无报错；再点击其他应用，主界面正常隐藏，设置页的固定状态没有影响启动器。

以上设置窗口图钉交互与退出设置后的状态释放人工验收通过。全部 GUI 操作由用户完成；此结果不代表 Mac 插件 Runtime 执行验收通过。


## 2026-11-14 命令设置直达与键盘返回

继续不依赖证书的宿主界面接线：Mac 已安装命令在主结果右键及 Ctrl+K 当前对象菜单提供“插件设置”，以稳定 pluginId/commandId 打开对应命令详情。改名不影响目标，同包 upper 不再需要手动选择以避开默认 lower。入口检查当前查询/窗口所有权，实际详情以重新读取的安装清单为准；已移除的命令显示底栏错误而不选择错误对象。一次性目的地离页失效，迟到列表不能重新打开旧页。

详情页 Ctrl/Command+F 经未保存确认后返回列表并聚焦筛选框；列表原有聚焦保持，安装确认或处理中不抢焦点。普通齿轮仍打开通用设置；Windows 行为不变，Mac 生产插件运行仍关闭。

验证：新增入口和焦点/过期目标测试先失败后通过；完整相关 Launcher、Mac 管理及客户端回归 **431 通过、18 跳过**，TypeScript/Vite 构建通过。覆盖右键及 Ctrl+K 指向改名后的 upper、收藏对应、页签重开不重跳、移除包/命令、离页迟到响应、草稿取消/放弃及安装对话框焦点。首次实现验证发现视图没有暴露私有 activation 字段，已改为使用核心校验后的 favorite 稳定身份；测试补齐 jsdom scrollIntoView stub，最终完整回归通过。没有调用 GUI、运行插件或操作用户安装库。

日志：`/private/tmp/uipilot-command-settings-{red,green,ui,build}.log`。原生菜单与键盘交互待新候选人工确认。完整目标当前工程估计约 80%，不因本轮界面接线单独上调；同包双端生产运行通过数仍为 0 / 17。


命令设置候选 `b77d3e4` 已完成 App/DMG 构建及实际 DMG 只读校验：arm64、最低 macOS 15.7.4、应用身份、资源、ad-hoc 签名和 Documents entitlement 通过。没有启动应用或操作 GUI。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-command-settings-b77d3e4.zip`，SHA-256 `3050e07d4b1ec8ccb048099f6478cdceb4c7380b91b13650cbc0bfd5a4484c92`。
- DMG SHA-256：`6d526735c8e0bff262ae8a2d936e0cd717e59ff24f57ad15e5dd2a656062a2ac`。
- 主程序 SHA-256：`263206a2f3b8164494a2f5b69aaa0b33b500ffca0f2cdf5449b843b7979b99e8`。

只读 preflight、ZIP CRC 和 manifest 中 9 个文件哈希逐项通过。候选交给用户按单动作指引验证右键/Ctrl+K 定位命令及详情 Ctrl+F；原有插件无需重装。原生交互保持待验收，分支推送但不合并 main。


## 2026-11-14 Mac 主修饰键与操作菜单键盘选择

用户在 `b77d3e4` 确认：upper 右键“插件设置”直接进入对应详情、名称与收藏正确；Ctrl+F 返回插件列表并聚焦筛选框。随后要求 Mac 内置主操作使用 Command，并报告主界面 Ctrl+K 打开操作面板后不能直接上下选择、回车确认。本轮停止沿用旧 Ctrl 人工步骤。

Mac 宿主操作统一使用 ⌘+K（操作菜单）、⌘+T（图钉）、⌘+F（筛选），主界面、设置与 Find 的提示和按键对应；共享快速链接处理也按宿主平台选择主修饰键。Windows 保持 Ctrl；用户录制的全局唤起键不改写。UI 规范已明确平台主修饰键约定。

共享菜单增加显式可聚焦/轮换 tabindex 与当前选项状态，打开选择首个可用项，上下移动跳过禁用项，Enter 不依赖 document.activeElement 才能确认，长按不重复执行；焦点无法落到按钮时容器接收按键并保留高亮选项。选中项滚入视野，关闭恢复原焦点和选区。主输入重新绑定也不再抢占已打开的操作菜单。仅在本次指定平台上使用对应关闭快捷键，默认 Windows 注入行为保留。

新增测试先失败后通过，覆盖原生按钮焦点拒绝、宿主把焦点带回输入框、双平台修饰键、上下选择/单次回车、菜单关闭恢复、Mac 命令详情键盘直达及组合输入拒绝。实际 WKWebView 行为仍需用户用新包确认；没有使用 computer use。


最终自动验证：Launcher、Find、共享菜单、平台快捷键及插件窗口相关回归 **464 通过、18 跳过**；插件面板与注入脚本回归 **40 通过**；TypeScript/Vite 构建通过。日志 `/private/tmp/uipilot-mac-command-keys-{red,green,ui,panel,build}.log`。没有把 DOM 或模拟焦点检查计为原生 Mac 人工通过。


Command 快捷键候选 `c42c381` 已完成无 GUI App/DMG 构建，实际 DMG 的 arm64、最低 macOS 15.7.4、签名、资源与 Documents entitlement 检查通过。只读 preflight、ZIP CRC 与 manifest 中 10 个文件哈希通过。

- ZIP：`/Users/mobytang/Documents/UiPilot-Mac-command-keys-c42c381.zip`，SHA-256 `70c8f41627cde8bdae595308a17c13cd3b4848b5427e027be68ad7c06ebf049b`。
- DMG SHA-256：`295e98c97fb1de5a68c9afadb0ca059dcc8100dec227f52a8c46437cd485316e`。
- 主程序 SHA-256：`b99c1f72154c561c24509f6e05ead8e502a1b24cd76a3dec68f014ab278e8817`。

新包的 ⌘K 初始选中/上下键/回车、关闭恢复及 ⌘T/⌘F 原生交互待用户手动确认。分支已推送、不合并 main；现有插件无需重装。

### Command 快捷键与主界面菜单人工验收

用户替换并重新启动候选 `c42c381` 后，逐项确认：

- 主界面按 ⌘K 打开操作面板，首个可用选项自动高亮，无需鼠标。
- ↓ 移动到下一可用选项且主结果选择不变；↑ 返回首项，面板保持打开。
- Enter 执行高亮的“打开设置”，进入设置并关闭面板，无报错。
- 设置页 ⌘T 点亮图钉并固定，再按一次熄灭并取消固定，无报错。
- 设置页 ⌘K 打开面板并自动高亮，再按一次关闭，恢复原焦点，设置页保持显示。
- 插件列表 ⌘F 聚焦搜索框；进入 Demo Multi 详情后按 ⌘F 返回列表并自动聚焦搜索框。

以上主界面与设置页的人工验收通过。Find 窗口对应快捷键、未保存草稿确认及 Esc/输入选区恢复未在本轮单列实测，保持待验收。全部 GUI 操作由用户完成；Mac 生产插件 Runtime 仍未开放。
