# UiPilot macOS 适配计划（Apple Silicon）

## 目标与架构

首版交付可安装的原生 arm64 启动器。M1 或后续 Apple Silicon Mac 用户能从菜单栏或全局快捷键唤起 UiPilot，搜索已安装应用，按 Enter 启动或激活，按 Esc 隐藏，再次唤起。保留 Windows 现有的开发、运行和打包行为。

**完整目标还包括插件。** 常用公开插件使用同一套代码和同一发布包，在 Windows 与 Mac 上完成同样的主要操作。分别报告核心启动器、插件双端实测、平台专属能力的状态。

沿用 React 启动器、Tauri IPC、应用排名和结果注册表，将系统发现、打开、持久化与窗口生命周期接到按目标编译的宿主实现。以宿主公布的能力裁剪查询、设置和权限；平台判断集中在宿主。当前基线为 `40407fa`。计划已通过独立审核并进入实施，行为约定见[首版宿主设计](../specs/2026-11-06-macos-port-design.md)。[历史 MVP 设计 §14.2](../specs/2026-07-17-cross-platform-launcher-mvp-design.md) 记录了风险，其批准范围限于当时的 Windows MVP。

十项任务的操作顺序、逐步验收标准与证据要求见[逐步实施与验收](2026-11-06-macos-port-steps.md)。

## 集成与验收进度

各任务的代码、自动验证与人工验收分别记录。开发分支完成后，由集成分支复核并构建同一份测试包。

**2026-11-07 进度与人工测试安排：** 完整目标（含插件双端通用）当前估计约 68%，按剩余工程与验收工作评估，不按勾选数量计算。用户指定整体达到约 70% 时统一安排人工测试；届时冻结候选版本，交付带 SHA-256 的测试包及清单，再通知测试者。此前继续实现、独立审核和自动验证，非阻塞人工用例汇总保留，不提前宣称通过。无 M2 或其他系统设备的覆盖仍记为未验证。

用户随后明确：约 70% 之前无特殊情况持续推进，不在每个内部小单元完成后结束开发；只有必须由测试者处理的阻碍才提前交接。进度仍依据实际实现与验收工作估算，不能通过调整分母或把待验收标为通过提前达到节点。

| 任务 | 已取得的证据 | 尚待验证 |
| --- | --- | --- |
| 1 设计与原型 | M1 Pro / macOS 15.7.4，原生焦点与快捷键通过；生产及开发 Tauri IPC 各观察到本地成功、外部明确拒绝。见[探针记录](../../../spikes/macos-launcher-probe/README.md) | 实际应用的唤起与 IPC 单独验收 |
| 2 构建入口 | Mac Vite/Tauri 构建与配置测试通过。Windows x64 原生构建生成 NSIS 与 MSI，测试者确认 NSIS 安装后可正常使用 | Windows 服务专项回归 |
| 3、6 宿主和前端 | 正式宿主与前端已合入，内部 arm64 包已生成；本地 ACL、库测试及前端测试通过 | 中文输入法组合输入 |
| 4、5 应用与快捷键 | `ca77db1`、`bad4cb1` 已合入；复审修正 `4354513`、`24d18aa` 进入候选 `a79f612`。候选通过集成终审和 103 项 Rust 库测试；M1 实机通过主题与改键重启保留、睡眠唤醒后搜索并打开计算器 | 多显示器（有设备时）、真实快捷键冲突 |
| 7 发布门槛 | 候选全仓九阶段验证通过，本地签名 app/DMG、真实包检查、34 个平台/示例组合已验证；M1 Pro 从 DMG 安装后启动、快捷键唤起和输入通过；Windows 实机全量本地验证退出码 0，NSIS 哈希和人工安装结果见[设备验收单](../../releases/2026-11-06-desktop-port-device-checks.md) | Windows 升级/卸载等专项及公开发布签名/公证 |
| 8、10 文件与系统工具 | 已选方案 A。M1 实机确认 Find 能命中文稿嵌套目录里的中文 PNG，记住授权后退出并重新启动同一安装包仍可直接搜索。受限预览已接通分类、Finder 操作与文件身份复验。独立系统 QA 路径已实现。Mac 内部候选已接入双击 Command 的录制与可选监听 | 「停止并忘记文稿」的 M1 界面复测，正式签名下全系统范围和系统隐私权限、实际 Finder 焦点/操作、输入监控授权与撤权待集中实测 |
| 9 插件跨平台 | 17 个公开参考包中，4 个 manifest 声明双平台，13 个声明仅 Windows；当前 Mac 核心宿主关闭插件运行 | 同包在两个生产宿主实测通过的参考插件为 0；能力盘点、运行时和逐包验收待完成 |

Task 10.1 的[剪贴板共用基础](../specs/2026-11-07-macos-clipboard-foundation-design.md)已完成：共享模型、格式选择与预览进入 Mac 编译，8 项共享测试通过，Windows 既有指纹固定向量保持兼容；[自动验证记录](../../releases/2026-11-06-desktop-port-device-checks.md#2026-11-07-剪贴板共用基础自动验证)。后续接 Mac 原生观察、私有存储和复制。纯逻辑验证不计入双端插件实机通过数；Windows 原生回归另行安排。原生服务仍须验证停止/重启代际、撤权后无写入和外部剪贴板变更保护。

[原生文本观察器](../specs/2026-11-07-macos-clipboard-observer-design.md)也已实现并通过独立审核：20 项 fake 读取/生命周期测试与 8 项共享测试通过；另在同一 M1 上用唯一合成 pasteboard 验证原生读取、worker 交付与停止后的旧事件拒绝。生产宿主未装配该观察器，图片/文件/混合格式尚未实现，真实通用剪贴板和授权仍待验收。下一单元为私有历史存储及授权生命周期，Task 10.1 保持未完成。

[私有文本历史与授权层](../specs/2026-11-07-macos-clipboard-storage-design.md)已实现并通过独立审核：共享纯历史变换保留 Windows 行为；Mac 提供有界私有存储、提交不确定状态保护、按插件授权 generation 与撤权排空。剪贴板相关测试累计 51 通过，普通 Mac 库 203 通过，Find 预览 237 通过；全仓自动验证退出 0。下一步接观察器的读取前授权批次和原生采集时间，再接宿主权限生命周期与复制。生产服务仍未开放，Windows 原生回归及对应人工测试保持待验收。

[读取前授权与内部采集接线](../specs/2026-11-07-macos-clipboard-collection-design.md)已实现并通过独立审核。无授权不读取正文，授权改变只重建基线；密封事件携带原授权批次和读取前观测时间，撤权及 shutdown 排空原生读取。13 项新增用例实际覆盖 fake 采集、历史落盘及重开，剪贴板相关测试累计 64 通过。后续仍需复制及外部写入保护、宿主权限生命周期与产品接线；本次未开放 SDK 或生产历史服务，整体仍估计约 55%，人工测试保持约 70% 统一安排。

2026-11-07 后续开发：显式文本复制、隔离 Keychain 基础及启动器快速计算已实现并独立复审。计算/单位/日期/时区接入 Mac 主界面与一次性复制，汇率明确不可用；真实 Mac 能力过滤的集成遗漏已修复并补回归。基于这些实际实现，整体工程估计更新为约 60%，仍包括未完成的插件运行时、系统工具、生产授权装配与双端验收。Keychain 和历史采集仍是内部基础，不计为已开放插件能力；统一人工节点仍为约 70%。

Web 普通查询动作与搜索引擎设置已通过独立复审及完整自动验证，整体工程估计约 63%。后续继续登录项的系统状态与主界面接线，再推进插件运行所需的能力基础；同包双端插件实机通过数仍为 0。

登录项内部预览的系统状态机、主线程 IPC 与设置页已通过独立终审及全量自动验证。默认构建仍关闭，70% 内部包显式启用供统一实测；整体工程估计约 65%。剩余重心是插件原生运行/授权接线、更多系统能力与双端实机验收，不能把未开放的内部基础算成插件已经可用。

插件 HTTPS 共用核心在 Mac 真实系统 TLS 后端上通过本地证书、主机名、限额、取消与代理隔离回归，整体工程估计约 68%。没有打开生产插件权限层或运行时；WK 暂停缺少等价原生开关读回/意外恢复通知，SpeechSynthesis 覆盖也尚未证明。下一步完成独立媒体观测探针与候选包，在约 70% 节点交给人工验证。原始同包双端插件实测通过数仍为 0，严格原生隔离缺口不能通过测试数量抵消。

内部候选 `a79f612` 的 DMG SHA-256 为 `3c640e2b8da704d438f59afcb437cc06509273b1bcff05e792e44e04cede77a3`，主程序 SHA-256 为 `5656161a61cb60c9480023ddd4f94b28e3711785ad8e5aeaff7e5a608df02b6d`。已只读挂载 DMG 并确认其中的主程序与单独交付的 .app 一致。

**2026-11-06，M1 Pro / macOS 15.7.4（24G517），正式启动器核心交互验收通过。** 测试者对候选 `a79f612` 的操作清单反馈「全部通过, 能搜索应用, 多次唤起主程序框」。本轮清单覆盖唤起并输入、搜索并打开计算器、再次唤起并激活已运行的计算器、Esc 隐藏及菜单退出。证据为测试者反馈。这轮直接运行候选 .app。随后在同一设备核对 DMG 哈希，从“应用程序”启动安装副本，测试者确认 Option+空格唤起和输入通过。

**中文应用搜索修复候选 `6ec54e8`。** 原包的中文名称查询在 M1 复现，修复后系统计算器的中英文名均能命中同一条目。此包的哈希及独立实测见[设备验收单](../../releases/2026-11-06-desktop-port-device-checks.md)，测试者已确认在 M1 上用中文能搜到计算器并启动；选词瞬间的窗口状态未单列。

**DMG 文件替换验证（同设备，隔离目录）。** 使用 `scripts/check-macos-install.py` 只读挂载上述 DMG，将应用首次复制到新的临时 Applications 目录，然后用 DMG 内的应用替换先前的内部构建 `8efdf6a`。前后主程序 SHA-256 分别为 `b0cf298191745e77dc137f1a65e6023eaec569fec610d558c74cc335a63f5fac` 与上文候选值；两条复制路径均通过签名、标识、arm64 和主程序哈希校验。两个构建版本均为 0.19.0。已安装的当前版本重启后保留主题与快捷键；跨版本升级的数据保留及下载隔离仍待验证。

## 首版支持矩阵与约束

| 范围 | Mac 首版承诺 | 交付边界 |
| --- | --- | --- |
| 核心 | 应用发现、排名与执行；普通组合快捷键；菜单栏显示与退出；单实例；主题、快捷键持久化 | 从干净启动到退出全链路实机验证 |
| 分发 | 原生 arm64 .app 与 .dmg；保留 Windows 安装器 | 仅构建 `aarch64-apple-darwin`。排除 Intel、Universal 二进制及 Rosetta 运行链路 |
| 后续 | 文件/目录搜索与 Finder 定位、插件（含旧版及公开插件）、凭据、其他内置命令、登录时启动、双击修饰键、剪贴板历史/粘贴、选中文本、截图、通知与进程工具 | 首版仅列出核心入口；旧设置、收藏、快捷键、深链或直接 IPC 均须经过同一能力边界 |
| Windows 专用 | Everything、SystemIndex、Provider、提升 Helper、WebView2、NSIS | 继续由 Windows 构建并回归 |

- **最低版本候选为 macOS 15.7.4。** 当前原生 arm64 主机可复现该补丁版本；这是保守的首版产品选择，不代表依赖的理论下限。Task 1 核对锁文件、Xcode/SDK、WKWebView、所需 API 可用性及测试设备。本轮必测 Mac 为 **M1 + 精确最低系统**。2026-11-06 按用户决定跳过后续 Apple Silicon + 较新系统实机行；该组合未实测，不阻塞本轮验收。保留芯片、OS/build、工具链及包哈希；本机的 `uname -m` 与 `sw_vers` 不能替代这些证据。若依赖或设备无法满足，阻断发布；修改下限需更新设计、包元数据、验收行和支持说明并重验。
- 保持 0.19.0 基线、Rust 1.96.1、锁定的 Tauri 2、React 19、TypeScript/Vite 及现有 IPC 调用者边界。按 [UI 规范](../../ui-guidelines.md) 验证明暗主题、焦点、输入法和键盘操作。沿用已有收藏协议（现有应用条目的 `favorite` 为 None）；不借移植改变 Windows 应用结果行为。
- 首版默认路径不触发辅助功能、录屏、输入监控、完全磁盘访问或登录项授权。后续内部候选显式选择双击 ⌘ 时才请求输入监控；从持久设置恢复只检查现有授权。真实切换焦点/打开应用、安装、注册登录项、输入模拟、使用签名身份及对外发布须在对应实机阶段先取得范围明确的确认。本轮仅改计划。

## 全局执行规则

从仓库根目录运行命令。Task 1 为规格审查；每个实现任务先写聚焦失败测试，确认预期失败，再实现并复验。每项至少一个独立提交，不夹带工作区其他改动；规格符合性与代码质量评审通过后才开始依赖任务。使用临时配置、独立测试账户与隔离构建输出；最低 OS、真机和 Windows 验收不可用时标记待验证。测试过滤器须命中新增断言。

顺序：1 → 2 → 3 → 4 → 5 → 6 → 7。Task 7 是首版发布关口。后续为 7 → 8、7 → 10，8 和 10 → 9；Task 9 只能授权前两项已验证的能力。Tasks 8–10 分别先批准对应能力规格，不阻塞首版发布。

### Task 1: 冻结首版合同与证据矩阵

**Files:** 新增 `docs/superpowers/specs/2026-11-06-macos-port-design.md`。

**Dependencies:** 无。

- [ ] 建立「支持矩阵与部署下限」「宿主装配与 IPC」「应用发现与执行」「唤起与退出」「分发与验收」五节。对 Cargo/build.rs、Tauri hooks/resources、模块声明、builder/setup/invoke/exit、前端启动查询/设置及测试夹具逐项记录 Windows 依赖、Mac 替代或关闭路径、归属任务。
- [ ] 用隔离工程验证锁定工具链的 arm64 Tauri/WKWebView 原型可构建，在候选最低系统验证开发与生产页面加载、IPC 来源/CSP、本地应用枚举和窗口唤起。记录工具链与设备缺口，约定默认无冲突候选组合键、冲突时菜单栏恢复入口及 Dock 策略。确定签名内部测试与公开发布的分界。

**Distinct test coverage:** 每个启动依赖及 SDK 权限均映射到矩阵；原型中的外部页面不能调用宿主，已批准核心命令可在 production origin 调用。确认当前设备证据与最低版本验收行分别记录。

**Verify:** 将本任务新增规格按路径暂存后运行 `git diff --cached --check -- docs/superpowers/specs/2026-11-06-macos-port-design.md`；运行 `git diff --cached -- docs/superpowers/specs/2026-11-06-macos-port-design.md` 审阅实际新增内容，逐节核对五节、原型运行记录和待补设备，并取得规格批准。

### Task 2: 拆开开发命令、目标依赖和资源

**Files:** `package.json`、`scripts/tauri.ps1`、`scripts/dev-with-everything.ps1`、`scripts/{build-process-elevation-package.ps1,test_provider_build.py,test-managed-everything-installer.ps1}`、新增 Node 分发入口与 `scripts/platform-launcher.test.mjs`、`src-tauri/{Cargo.toml,Cargo.lock,build.rs,tauri.conf.json}`、新增 `tauri.windows.conf.json` 与 `tauri.macos.conf.json`（均在 src-tauri 下）、Mac 图标。

**Dependencies:** Task 1「支持矩阵与部署下限」「分发与验收」。

- [ ] 为 npm dev/tauri 保留 Windows 的 PowerShell 和动态端口链路；Mac 使用本地 Tauri CLI 与 dev:vite，传递参数、信号、退出码，核对 Vite 14321 与 Tauri devUrl。发布入口锁定 arm64，明确拒绝 Intel、Universal 或非原生环境。
- [ ] 将 windows、everything-ipc、webview2-com、installer helper 和 manifest 链路限于 Windows；共同保留必要的桌面插件。修正 tauri.ps1 的 dev 覆盖生成器（目前仅读共享 tauri.conf.json 的 bundle.resources），从有效 Windows 资源清单生成映射，再替换开发 Provider 和 asset-manifest；拆出 provider_build、Everything 的 prepare/validate/stage、.exe/.ico、NSIS；检查最终有效配置（包括原有 VM/安全探针等覆盖配置）。Mac 配置设 minimumSystemVersion、统一 MACOSX_DEPLOYMENT_TARGET、.icns、app/dmg，检查打包后共享资源仍可读取。迁移受保护安装包 inventory：对实际生效的 Windows 配置、资源及 hooks 生成哈希；同步 Provider 和 managed Everything 的来源断言，避免测试仅检查已移空的共享配置。

**Distinct test coverage:** 模拟两个平台的 argv/失败退出；占用 Windows 默认开发端口后另选端口，devUrl/Vite 一致，覆盖清单包含 Everything、共享声音、当次 Provider/manifest；Windows production 与 VM 保留各自 helper、资源及钩子；Mac 合并配置不执行 PowerShell/Provider 脚本；交叉目标覆盖或 Rosetta 环境不能产出误标 arm64 的发布包。

**Verify:** `node --test scripts/platform-launcher.test.mjs`；`npm run build`。Windows 另运行 `python -m unittest discover -s scripts -p test_provider_build.py`、`npm run test:managed-everything`、`powershell.exe -NoProfile -File scripts/build-process-elevation-package.ps1 -Mode Check`，核对 JSON inventory 的配置、资源与哈希。宿主完整编译由 Task 3 验证。

### Task 3: 建立可启动的宿主边界

**Files:** `src-tauri/src/{lib.rs,commands.rs,atomic_file.rs,settings.rs,webview_reload_guard.rs,apps/,public_plugins.rs}`，受影响的 Win32 模块、bin/tests、新增 scripts/smoke-platform-bootstrap.mjs，`src-tauri/{build.rs,capabilities/,permissions/}`，前后端能力协议。

**Dependencies:** Tasks 1、2；设计「宿主装配与 IPC」。

- [ ] 将 OS import、FFI、测试 fixture 和源结构断言随所属后端条件编译。保留共享 DTO、排名与 result_registry。梳理应用模型中的 BCrypt 与 Win32 专有变体；首版基础宿主可在待接入应用后端时明确返回不可用。
- [ ] 逐项拆分 run/builder、setup_production_lifecycle、全局快捷键回调、invoke 表及 Exit 清理。Mac 不创建 find/插件 webview、Everything supervisor/FileIndex、Provider、截图、DPAPI、插件 runtime、低级 hook 或相应后台线程。共享命令不能预取这些 State；先验证窗口身份，再检查能力，再读取受保护状态。保留 Windows 正常初始化与清理顺序。Mac 提供用于生产 smoke 的最小菜单栏退出入口，Task 5 再接入完整唤起行为。拆出旧 PluginManager.load/create_runtimes 及 panel focus 注册；检查 public_plugins.rs 的宿主构造和 PluginInvocationEnvironment 两处 Windows 常量，任何可达平台值须来自真实目标。首版在插件扫描、安装、恢复、授权、调用及自动启动之前拒绝，不能进入构造 Windows runtime context 的路径。
- [ ] 以系统 app_data_dir 存储配置；实现同目录临时文件替换、备份/恢复和符号链接安全边界，保留旧配置未知/不可用项，隔离 credentials/ 与 public_plugins/secrets.rs 两套 DPAPI 路径，禁止导入或降级存储 Windows 凭据。定义宿主能力快照以供 UI 使用。WKWebView 精确允许主窗口开发/生产来源，延续 CSP、reload guard 与 Tauri ACL。

**Distinct test coverage:** 在无 Windows 服务的生产装配下启动和退出，退出清理仅消费已创建资源；错误窗口在能力/状态访问前拒绝，对合法 main 的禁用操作无副作用；存有已启用插件仍不执行代码；保存候选后替换失败，重启可读旧配置；目录/链接逃逸拒绝写入。

**Verify:** 两台原生平台分别运行 `cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets` 和 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib atomic_file::`，另在隔离测试账户按下列路径做生产 smoke（默认 features，不传 test-instrumentation）。Mac 运行 `npm run tauri -- build --target aarch64-apple-darwin --bundles app` 后运行 `node scripts/smoke-platform-bootstrap.mjs --platform macos`；Windows 运行 `npm run tauri -- build --no-bundle` 后运行 `node scripts/smoke-platform-bootstrap.mjs --platform windows`。新增脚本解析本次构建产物，启动并等待 main 页面 ready，显示 PID；测试员从该实例的菜单栏/系统托盘选「退出」，脚本验证 PID 正常退出、超时返回非零。Mac 断言未创建 Everything/Provider/插件 WebView 且无权限提示；Windows 检查原有生产装配、开发/安装资源按预期可定位，Everything 与插件服务按既有设置运行。记录退出与清理事件。Task 7 再验证最终安装包。执行新增装配用例 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib platform_bootstrap`。

### Task 4: 接通应用发现、搜索和执行

**Files:** `src-tauri/src/apps/{mod.rs,discovery.rs,cache.rs,action.rs,icon.rs,windows_backend.rs}`、新增 apps/macos_backend.rs、`src-tauri/src/{commands.rs,result_registry.rs,model.rs}`。

**Dependencies:** Task 3；设计「应用发现与执行」「宿主装配与 IPC」。

- [ ] 从 /System/Applications、/Applications、~/Applications 枚举可读 .app，覆盖 Utilities 等嵌套目录；止于 bundle 边界，限制遍历、拒绝循环和越界链接。读取系统元数据、本地化显示名和有界图标（缺失时回退）。制定稳定 ID 与重复 bundle 优先级。沿用 AppCache 的串行刷新与快照。
- [ ] 以 bundle 增加 ApplicationLaunchTarget；普通 `search_apps` 只调用已启用数据源，`execute_result` 从当前 main registry 的 request/result ID 获取私有目标，再读取所需后端（当前入口提前读取 FileIndex）。以系统 API/参数数组打开准确 bundle，已运行则激活；区分接受打开请求与真正观察到焦点，失败保留可重试 UI，不把路径交给 shell 或客户端。

**Distinct test coverage:** 中文、空格、同名/同 ID bundle、缺图、损坏/无权限目录、扫描中继续读旧快照；串行刷新 A 成功后 B 失败时保留 A；结果发布后 bundle 删除/替换或请求失效时拒绝执行；输入伪造路径不能打开目标；Windows Shortcut/AUMID 结果及排序保持兼容。

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib apps::`；两平台运行新增端到端命令夹具 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib platform_app_commands`。

### Task 5: 打通唤起、焦点和退出

**Files:** `src-tauri/src/{hotkey.rs,hotkey_hook.rs,double_tap.rs,lifecycle.rs,window_geometry.rs,panel_effects.rs,lib.rs,settings.rs}`，macOS 窗口后端与菜单栏资源。

**Dependencies:** Task 4；设计「唤起与退出」。

- [ ] 接入标准全局组合快捷键、单实例再唤起与菜单栏「显示」「退出」。实现前台激活、输入框焦点、Esc/失焦隐藏、显示器定位、Dock 策略与退出释放。休眠恢复与多屏切换后仍能唤起到可见区域。注册失败保留菜单栏入口，失败原因可见。
- [ ] 绑定替换及设置写入作为一笔事务；管理每次唤起的身份，过期的 blur、应用启动回调及退出过程中的唤起请求均不能抢走下一次会话。

**Distinct test coverage:** 新绑定冲突或持久化失败恢复旧绑定；A 执行应用、B 已唤起时 A 的迟到隐藏无效；激活失败回到可重试状态、再次唤起可输入；Quit 后释放绑定与菜单资源，再启动可重新注册。

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib lifecycle::`；`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib hotkey`。

### Task 6: 裁剪前端启动路径与设置

**Files:** `src/{main.ts,protocol.ts,launcher-core.ts,launcher-view.tsx,hotkey-recorder.ts,launcher.test.tsx,hotkey-recorder.test.ts}`、浏览器预览客户端与主题样式。

**Dependencies:** Tasks 3–5；设计「宿主装配与 IPC」「唤起与退出」。

- [ ] main 的 Tauri client 先取得能力快照，launcher-core.start、设置页初始化、搜索分发仅调用已启用服务；能力读取失败呈现可重试状态。关闭旧收藏/命令键/右键等旁路；Windows 保持现有入口。只呈现可用设置、快捷键录入和可恢复错误。
- [ ] 使用系统修饰键及主操作显示，验证中文输入法组合输入 Enter、箭头选择、焦点圈、Esc、双主题、缩放与高对比。核查 WKWebView dev 与 production 的键盘刷新、IPC 和布局。

**Distinct test coverage:** 以仅有核心能力的 client 冷启、输入、执行和打开设置，未调用插件/Find/凭据接口，主题与快捷键重启后恢复；不可用的旧收藏不能发起执行；组合输入 Enter 不误启动；Windows 能力快照保留原 UI。

**Verify:** `npm test -- src/launcher.test.tsx src/hotkey-recorder.test.ts src/protocol.test.ts`。

### Task 7: 双平台发布门禁

**Files:** `scripts/verify-local-foundation{,.test}.mjs`、新增 `scripts/verify-macos-bundle.mjs`、双平台 CI、`README.md` 和 `docs/releases/` 验收记录。

**Dependencies:** Tasks 1–6；设计「分发与验收」。

- [ ] 选用原生 arm64 runner，预检 uname -m 和 rustc -vV host，另设 Windows 原生 runner。双平台执行共享回归、CLI/schema 和示例验证。当前协调器显式传 --platform windows；按 manifest supportedPlatforms 分组，对声明支持的平台分别 validate，Windows 专用示例保留 Windows 正向和 Mac 拒绝断言。保留语音、选区和窗口权限等 manifest 中现有 Windows-only 限制；不把 CLI 静态验证记作宿主已支持。增加原生 arm64 构建与包检查；Windows 原生复跑 managed Everything/Provider 打包链。
- [ ] 检查 .dmg 中实际交付的 .app，验证 Info.plist 的 LSMinimumSystemVersion、主程序和随附 Mach-O 的架构（lipo -archs 仅 arm64）及 LC_BUILD_VERSION/minos 与下限相容、bundle ID、图标、资源；排除 .exe/.dll 及 Windows 工具。Windows 安装包反向检查 Helper/Provider/Everything、NSIS hooks 和既有安装操作。
- [ ] 对外分发前用授权的 Developer ID 签名、hardened runtime、公证并 staple；以 codesign、stapler、Gatekeeper 和带下载隔离属性的干净安装验收记录为凭。无身份时仅交付明确标记的本地测试包，保留公开发布阻塞项。

**Distinct test coverage:** 错误 slice/最低版本、嵌套二进制要求更高系统或夹带 Windows 资源均导致包门禁失败；候选包在最低 OS M1 安装、替换升级保留设置、卸载后退出清理，二次启动恢复正常；Windows 干净安装仍能搜索并打开应用。

**Verify:** 双平台 `npm run verify:local`；Mac `npm run tauri -- build --target aarch64-apple-darwin --bundles app,dmg`，再从仓库根目录运行新增的 `node scripts/verify-macos-bundle.mjs`（定位该次输出，校验并打印包哈希）；Windows `npm run tauri -- build`。记录两平台完整命令、状态与产物。

## 首版最终验收

- [ ] 每个 Mac 实机行记录芯片、OS/build、原生架构、包哈希、签名状态、验证人及结果。使用菜单栏和全局键各唤起一次，搜索中英文应用、启动未运行应用、激活已运行应用，再次唤起、隐藏、休眠唤醒、重启保留设置；检查不出现非核心系统授权请求。
- [ ] Windows 复验启动、设置、快捷键、应用/文件搜索、插件授权与退出；任何必需平台行缺证据均不能标为发布通过。

## 后续阶段

### Task 8: 文件搜索与 Finder 操作

**Files:** `src-tauri/src/{file_search/mod.rs,file_index/mod.rs,find_window.rs,commands.rs,result_registry.rs}`、新增 `src-tauri/src/file_search/macos.rs`、`src-tauri/capabilities/find.json`、`src/{find-core.ts,find-view.tsx,find-core.test.ts}`。

**Dependencies:** Task 7；另行批准 Spotlight 搜索范围、查询所有权、Find 生命周期与文件操作授权合同。

- [ ] 通过 Spotlight 查询获取有界、可取消的结果集；区分尚未索引、无匹配、访问受限和服务错误，不回退为全盘扫描。接回 Find 的 ready/pin/关闭协议，保留 Windows Everything/SystemIndex 后端。
- [ ] 由宿主保存结果的身份与路径，在打开或 Finder 定位前重新核对对象、授权范围和当前调用窗口；传给系统的 URL/参数与 shell 隔离。

**Distinct test coverage:** A 搜索未结束时 B 已发布，迟到的 A 不改写 B；关闭 Find 后 A 回调不发事件；打开等待期间 pin 变化，完成时遵循当前 owner；中文/空格路径可打开，删除或替换成越界符号链接时拒绝；main 调用 find 专用命令在状态读取之前被拒绝。

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib file_search::`；`npm test -- src/find-core.test.ts src/find-view.test.tsx`；在测试目录验证被索引/排除的文件与 Finder 定位。

### Task 9: 插件宿主、凭据与公开能力

**Files:** `src-tauri/src/{public_plugins.rs,plugins.rs,plugin_panel.rs,plugin_window.rs,credentials/}`、`src-tauri/src/public_plugins/{manifest.rs,package.rs,runtime.rs,secrets.rs,webview_audio_guard.rs}`、`src-tauri/capabilities/plugin-*.json`、`packages/plugin-cli/`、`docs/plugin-sdk/` 与对应参考示例。

**Dependencies:** Tasks 7、8、10；另行批准插件来源/能力矩阵、Keychain 生命周期、旧插件兼容性及 Windows-only manifest 限制的逐项决定。引用 Task 8/10 的实测能力清单；仍被关闭的能力必须在授权前拒绝。

- [ ] 对 17 个参考包逐一记录主要用途、声明平台、实际依赖和状态（同包双端通过 / 待移植 / 平台专属）。先验无权限设置与主结果，再验多命令、剪贴板和独立窗口；Windows-only 声明本身不能作为永久排除理由。旧插件另列迁移需求。共同能力包按同一版本和 SHA-256 在两端安装、授权并运行，留下预期与实测结果，详见[逐步验收 §9](2026-11-06-macos-port-steps.md#step-9)。
- [ ] 覆盖安装、启动、恢复、升级、停用/退出，校验 manifest 和实际 `PluginInvocationEnvironment` 的 macos 值。旧插件 PluginManager 与公共 runtime 分开审查；旧插件若无法证明对应隔离，仍禁用自动加载、恢复与调用。
- [ ] mainResult 同样依赖隐藏 runtime。先在惰性 URL 创建 WKWebView，不加载包代码；核验当前插件来源和 generation、安装导航/下载限制、设置原生静音、注册 unmute 监听，再读回静音状态。仅当上述步骤完成且 generation 仍有效，才导航到受信任的插件入口并允许执行。启动失败或监听到失去静音时，先撤销对应 generation 和 RPC 权限，再销毁该 runtime。Mac 原生 guard 无法满足此顺序时，mainResult 保持关闭。隐藏 runtime 通过实机验收后，Panel/Window 各自复验来源、音频、崩溃恢复与 RPC 所有权，再分别开放。
- [ ] host credential 与 plugin secret 分别建立 Keychain 后端、命名隔离和变更回滚；实现期间先读 SDK 开发指南、UI 规范，联动 CLI/schema/types 和示例。选取最小声明权限的参考包验证，不批量扩大 manifest 支持。

**Distinct test coverage:** 以探针记录惰性创建 → 安装来源/导航/下载边界 → 静音 → 注册监听 → 读回 → 导航 → 插件首条指令；在任一导航前步骤注入失败，不执行包代码，generation 失效且实例销毁。导航后触发 unmute，旧 RPC 在读写前被拒绝；同标签重建后迟到的旧监听不能撤销新 generation。错误来源/其他插件 ID 在读取密钥前被拒绝；旧窗口关闭再创建时，旧 RPC 不能写新会话；升级新增权限须重新授予；Keychain 锁定、写入失败或撤权不发布成功/明文，也不破坏原记录；Windows-only 声音/Panel 等示例继续在 Mac 被拒绝。实机隔离条件未通过，相应 surface 保持关闭。

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib public_plugins::webview_audio_guard::`；`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib public_plugins::`；`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib credentials::`；`npm run verify --workspace @uipilot/plugin-cli`；`npm test -- src/public-plugin-recovery.test.tsx src/public-plugin-diagnostics-acl.test.ts`。用 WKWebView 生产包记录隐藏 runtime 的导航/静音/撤销顺序，用隔离 Keychain 记录一组实机 CRUD/拒绝结果。

### Task 10: 系统工具与可选系统集成

**Files:** `src-tauri/src/{clipboard_history/,selection_capture/,result_paste.rs,screenshot/,native_attention/,process_control/,double_tap.rs,hotkey_hook.rs,settings.rs,lifecycle.rs}`、`src/{launcher-view.tsx,screenshot-view.tsx}` 及对应平台权限说明与设置测试。

**Dependencies:** Task 7；每种能力先批准需求、权限、拒绝/撤销与恢复规格。文件/插件桥接分别由 Tasks 8/9 负责，Task 10 可直接通过宿主测试。

- [ ] 按独立验收单元分批实现：剪贴板监听与复制 → 选区读取/受控粘贴；截图 → 录屏权限与多屏输出；通知 → 首次用户操作时授权与路由。每种能力分别更新宿主快照、UI、测试与支持矩阵。进程工具限定为已证明有权限的操作，Windows 提升 Helper 保持专用。
- [ ] 将登录项设置与系统状态同步，处理系统撤销；双击修饰键单独评估事件监听权限和与普通组合键的冲突。无法可靠验证的选项保持关闭。真实输入、录屏、修改登录项或终止进程须在对应验收前取得具体同意；只用测试目标。

**Distinct test coverage:** 捕获后发生外部剪贴板写入，旧事务不得覆盖；待粘贴时前台目标切换则取消，不发送输入；录屏授权撤回时在途截图失败且缓冲区释放；通知拒绝时不报告已投递；登录项被系统禁用后刷新反映真实状态；双击监听注册失败不影响原组合键；进程退出/PID 复用时拒绝作用于替代进程。

**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib clipboard_history::`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib screenshot::`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib process_control::`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib double_tap::`；`npm test -- src/screenshot-view.test.tsx src/hotkey-recorder.test.ts`。各能力的允许/拒绝/撤销用例逐项在隔离账户实测，记录先后顺序、前台目标与结果。
