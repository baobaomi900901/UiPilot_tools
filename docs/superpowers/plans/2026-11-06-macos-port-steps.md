# UiPilot macOS 移植逐步实施与验收

目标是在 M1 及后续 Apple Silicon 上交付原生 arm64 启动器，并让使用共同 SDK 能力的插件以同一发布包在 Windows 和 Mac 运行。共享 React 界面、Tauri IPC、Rust 排名与结果注册表；系统调用、生产装配、资源和打包按目标分开。本文细化[总计划](2026-11-06-macos-port.md)，保留其任务编号、依赖与发布范围。下文均为**待执行的工作和预期结果**；这里只审查文档，不代表已编译、已签名或已通过实机验收。

**实施顺序**：1 → 2 → 3 → 4 → 5 → 6 → 7 为首版。发布后 8、10 可独立开展，两者的支持矩阵均确定后才进行 9。Task 1 的新设计在批准后成为接口、状态、失败语义的唯一合同；Tasks 8–10 各自需要后续规格。本文按该合同组织实施，不预先定义新的 IPC 字段、默认按键或授权规则。

**技术边界**：0.19.0 基线，Rust 1.96.1、锁定的 Tauri 2 与 React 19；Windows 原生构建链继续回归。仅发布 aarch64-apple-darwin，不发布 Intel 或 Universal，不以 Rosetta 启动。最低版本候选 15.7.4；本轮验证 M1 + 精确最低系统；按用户决定跳过较新芯片和系统组合，未实测。macOS SIP、Gatekeeper、TCC 与 CSP 按正常设置运行。

**执行与证据**：从仓库根目录运行命令。每项实现先令聚焦测试按预期失败，再作最小实现并复跑；每项形成独立提交，规格/质量审查通过才移交。每个编号步骤按顺序执行，在本任务记录中以同一编号关联：前置版本/环境、操作、实际结果与测试名或日志片段。Rust 过滤器用同命令追加 -- --list 核对新增用例，再运行并检查通过数量；0 tests 不算通过。前端测试文件也须有实际命中的断言。未测、阻塞和失败分开记录，不得视为通过。保存脱敏证据（commit、设备芯片及 OS/build、命令/退出码、包 SHA-256），不记录密钥和用户内容。测试用临时目录及隔离账户。改变真实前台、安装、输入控制、系统权限、登录项及正式签名分发，应先取得对应操作的明确确认。

**导航**：[1 设计](#step-1) · [2 构建](#step-2) · [3 宿主](#step-3) · [4 应用](#step-4) · [5 生命周期](#step-5) · [6 前端](#step-6) · [7 发布](#step-7) · [8 文件](#step-8) · [9 插件](#step-9) · [10 系统工具](#step-10)

<a id="step-1"></a>
### Task 1: 冻结首版合同与证据矩阵

**Files:** 新增 `docs/superpowers/specs/2026-11-06-macos-port-design.md`；隔离原型放测试目录，产品实现从 Task 2 开始。
**Dependencies:** 无。需取得 Windows 构建环境、候选最低系统原型环境和后续设备验收安排。

- [ ] **1.1 记录基线。** 从 `rust-toolchain.toml`、两个锁文件、`src-tauri/Cargo.toml` 和 Tauri 配置整理目标、最低工具链及运行依赖。在 Mac 记录 `uname -m`、`sw_vers`、`system_profiler SPHardwareDataType` 中的芯片型号（删序列号）、`xcodebuild -version`、`xcrun --show-sdk-version`、`rustc -vV`。输出一行一设备的矩阵；缺少 M1 设备或精确版本时该行标为待测。
- [ ] **1.2 追踪一条真实调用链。** 顺读 npm dev/tauri → provider/Everything hooks → build.rs/Builder → setup/RunEvent → main.ts/core.start → search_apps → registry → execute_result → 退出。逐一列出每个构造函数、State 读取、前端自动调用、后台线程、权限和资源的 Windows 行为、Mac 首版决定及验收负责人；对照 `src-tauri/{capabilities,permissions}` 与 SDK manifest 权限逐项销账。存在无归属的启动调用时补清单再评审。
- [ ] **1.3 验证阻塞性假设。** 用锁定版本制作临时 arm64 Tauri 原型，在候选最低 OS 分别从 dev server 与已构建 .app 加载 WKWebView；主页面调一个允许的 IPC，打开外部页尝试同一命令；从允许根枚举一个系统应用和临时测试应用，按批准的快捷键唤起并聚焦。记录实际 origin、允许/拒绝、扫描与焦点结果。任何受信来源不明确、依赖要求更高 OS 或非预期权限弹窗均要回到规格；不可通过放宽 ACL 或下调声明版本解决。
- [ ] **1.4 审批五节。** 写入「支持矩阵与部署下限」「宿主装配与 IPC」「应用发现与执行」「唤起与退出」「分发与验收」。明确重复 bundle 策略、错误行为、默认组合键冲突回退、Dock 行为、内部测试/公开发布与证据要求。逐节核准后才允许 Task 2；更改下限要替换后续全套环境行和包断言。

**Distinct test coverage:** 区分原型通过与最终发布包验收；外部页面试探被拒，受信页面成功，设备表能区分 CPU 型号与仅有 arm64 的记录。
**Verify:** 在未来实施中只暂存新规格，执行 `git diff --cached --check -- docs/superpowers/specs/2026-11-06-macos-port-design.md`；`git diff --cached -- docs/superpowers/specs/2026-11-06-macos-port-design.md`。以实测记录、五节审查结果及批准作为交接材料。

<a id="step-2"></a>
### Task 2: 拆开开发命令、目标依赖和资源

**Files:** `package.json`、`scripts/{tauri.ps1,dev-with-everything.ps1,build-process-elevation-package.ps1,test_provider_build.py,test-managed-everything-installer.ps1}`、新增分发入口与 `scripts/platform-launcher.test.mjs`；`src-tauri/{Cargo.toml,Cargo.lock,build.rs,tauri.conf.json,tauri.windows.conf.json,tauri.macos.conf.json,icons/}`。
**Dependencies:** Task 1「支持矩阵与部署下限」「分发与验收」批准；各平台装好锁定依赖。Windows Provider 使用脚本选定的外部源码，按 7.1 核对来源和 revision。

- [ ] **2.1 从正常 npm 入口分发。** Windows 保留 tauri.ps1、provider prepare 及动态端口；Mac 转给本地 CLI，beforeDevCommand 运行 dev:vite，固定 devUrl 与 Vite 一致。在子进程探针中捕获 argv、信号、退出码；Mac 发布需检查本机及 Rust host 原生 arm64、最终解析的 target，拒绝 Intel/Universal/转译和覆盖参数，拒绝后不得产生候选包。
- [ ] **2.2 分离原生依赖。** 将 windows、everything-ipc、webview2-com 移到 Windows target 依赖，隔离 installer helper、build.rs 的 Windows manifest 与链接参数。对比两平台 `cargo tree --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin -e normal` 与 Windows 目标树，确认 Mac 正常依赖链不编入这三项；保留锁文件中 Windows 所需包。
- [ ] **2.3 分离配置并校验合并结果。** 基础配置仅承载共享资源与 CSP；Windows 覆盖挂回 prepare/validate/stage、NSIS、Everything 和 Provider；Mac 覆盖提供批准的 minimumSystemVersion、匹配的编译部署目标、已有的 src-tauri/icons/icon.icns 和 app/dmg。测试解析 production、dev、VM、elevation、security-probe 各场景的最终资源/命令，断言 Windows 钩子顺序与原行为一致。
- [ ] **2.4 回归 Windows 消费者。** 修正 tauri.ps1 仅从共享配置读资源的问题。占用 14321 再启动，捕获 devUrl 与 Vite 实际端口及资源映射，要求端口相同、Provider/asset-manifest 来自当次开发目录，Everything 和声音仍在清单。按实际有效配置生成 protected inventory 的配置、hooks 和资源哈希；Provider/installer 来源测试改查 Windows 合并结果。

**Distinct test coverage:** 在 Mac 分发时伪造非原生环境与错误 target，在 Windows 子程序返回非零；分别要求提前拒绝、原样传回失败码。VM helper/production helper 分别按原钩子分发。
**Verify:** `node --test scripts/platform-launcher.test.mjs`、`npm run build`；Windows `python -m unittest discover -s scripts -p test_provider_build.py`、`npm run test:managed-everything`、`powershell.exe -NoProfile -File scripts/build-process-elevation-package.ps1 -Mode Check`。保存实际参数、端口、资源差异及 inventory，完整原生编译归 Task 3。

<a id="step-3"></a>
### Task 3: 建立可启动的宿主边界

**Files:** `src-tauri/{Cargo.toml,build.rs,permissions/,capabilities/}`、`src-tauri/src/{lib.rs,commands.rs,atomic_file.rs,settings.rs,apps/,public_plugins.rs,webview_reload_guard.rs}`、相关平台模块、`src/protocol.ts`、新增 `scripts/smoke-platform-bootstrap.mjs`。
**Dependencies:** Tasks 1、2，设计「宿主装配与 IPC」；具备无现有 UiPilot 实例的隔离账户。

- [ ] **3.1 拆编译边界。** 按 Task 1 表拆 OS imports、BCrypt/Shortcut、低级 hook、Everything/FileIndex、截图/进程、两套 DPAPI、WebView2 和测试夹具。保留共享模型、排名和错误协议。两台原生宿主跑 all-targets，审核 bin、lib 和测试均实际编译；不要用禁用全部测试来通过。
- [ ] **3.2 拆生产装配与退出。** 从 lib.rs 的 builder/manage/setup_production_lifecycle/RunEvent 移出不支持服务；尤其跟踪旧 PluginManager.load/create_runtimes、panel focus、Everything supervisor.start_async、关闭时的 State 获取。Mac 仅准备 main、配置、最小菜单退出及核心服务；Windows 按既有顺序启动、退出并清理所有权。以服务工厂记录构造/销毁，Mac 预置插件和快捷键配置仍不能扫描、恢复、申请权限或启动线程。拆开 lifecycle.rs 启动时 reconcile_runtime_settings 取 app.autolaunch() 并在注册快捷键后调用 read_autostart/change_autostart 的路径。首版 Mac 启动只恢复核心注册，不构造登录项端口；以持久化 autostart=true、系统实际为 false 的夹具启动，另将端口配置为一旦调用就报错。两种情况均须完成生产初始化、main ready 并正常退出，原字段仍为 true、端口获取/读/写计数均为零；实际组合键唤起在 5.1 复验。Windows 保留原启动对账。
- [ ] **3.3 能力与身份先行。** 在 commands、Tauri capability 和前端 protocol 接通核心能力读取。错误窗口调用时用探针证明没有读取受保护 State；核心 main 请求禁用功能时按规格拒绝且无服务实例化。验证 dev/production 主 origin 与外部导航；Mac 不可到达 public_plugins.rs 中硬编码 Windows 的 host/invocation 路径。
- [ ] **3.4 做持久化故障注入。** 在独立 app_data_dir 预置旧配置、未知项、备份，更新一个支持字段；在候选写入、备份替换和主文件替换处分别注入失败。重启应读到合同允许的完整版本，保留未支持项，临时文件不被当作已提交。放置越界链接后重试；外部哨兵内容保持不变，未产生明文秘密文件。
- [ ] **3.5 生产烟测。** 实现 smoke 脚本定位**本次**构建，等待真实 main ready，给出 PID，测试员点本实例菜单「退出」；脚本等该 PID 结束并检查清理日志，超时/启动中止均失败。Mac 获取不到任何禁用服务启动记录，Windows 必须保留 Everything、插件服务及各自退出路径。若当时 UI 仍预取不可用服务，先为冷启做最小接线；完整设置/搜索界面归 Task 6。不得借用 instrumentation 假装生产启动。

**Distinct test coverage:** 初始化失败到退出仅清理已创建资源；错误调用者/已禁用操作的状态访问计数为零；失败保存后再启动不丢原设置。
**Verify:** 双平台 `cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib platform_bootstrap`（新增夹具）及同命令过滤 `atomic_file::`。Mac `npm run tauri -- build --target aarch64-apple-darwin --bundles app` 后执行 `node scripts/smoke-platform-bootstrap.mjs --platform macos`；Windows `npm run tauri -- build --no-bundle` 后执行 `node scripts/smoke-platform-bootstrap.mjs --platform windows`。保存生产命令、main ready、PID 正常退出、两平台服务创建/清理轨迹。

<a id="step-4"></a>
### Task 4: 接通应用发现、搜索和执行

**Files:** `src-tauri/src/apps/{mod.rs,discovery.rs,cache.rs,action.rs,icon.rs,windows_backend.rs}` 与新增 `macos_backend.rs`，`src-tauri/src/{commands.rs,result_registry.rs,model.rs}`。
**Dependencies:** Task 3；设计「应用发现与执行」「宿主装配与 IPC」；准备隔离的 .app 测试根和无副作用的启动探针。

- [ ] **4.1 建立后端与模型。** 给 ApplicationLaunchTarget 增加私有 Mac 变体，保留 Shortcut/AUMID 及既有稳定标识。扫描 /System/Applications、/Applications、~/Applications，含 Utilities 子目录；限制深度、避免跟入 .app 内容或越界链接。用重复标识、同名不同应用、坏 plist、中文、空格、缺图及链接环夹具核对去重次序与图标回退。
- [ ] **4.2 接回缓存与排名。** 在 AppCache.scan 串行区内刷新，完整成功后换快照。让扫描 A 阻塞、并发读取旧快照，再释放 A 并令 B 失败；读者只见完整旧值或 A，B 不抹掉 A。通过实际 `search_apps` 搜索普通文本，保存排名/诊断和返回 DTO，要求无路径泄露、无插件或货币等禁用服务读取。
- [ ] **4.3 验证 IPC 执行。** 调整 `execute_result` 先按窗口/registry ticket 解析 action，再按 action 获取需要的 State。宿主复核 bundle 身份并调用系统打开/激活 API；不解释命令字符串。依次运行当前结果、旧 request、外部伪造路径、扫描后删除/替换 bundle；仅当前有效目标可产生系统调用，失败留在可重试状态。
- [ ] **4.4 实机区分打开与激活。** 在批准的测试应用上分别从未运行/已运行状态调用，记录系统接受打开、实际前台焦点及是否出现重复实例。Windows 用快捷方式和 AUMID 样例检查相同查询/执行路径；保留 app 结果原有 favorite=None。

**Distinct test coverage:** 单测使用私有目标替身验证调用次数，真实设备验证前台行为；刷新失败、陈旧执行、当前执行的终态各自断言。
**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib apps::` 与 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib platform_app_commands`（新增命令夹具）。交付脱敏 DTO、缓存时序及两平台打开/失败记录。

<a id="step-5"></a>
### Task 5: 打通唤起、焦点和退出

**Files:** `src-tauri/src/{hotkey.rs,hotkey_hook.rs,double_tap.rs,lifecycle.rs,window_geometry.rs,panel_effects.rs,settings.rs,lib.rs}`、Mac 窗口后端和菜单资源。
**Dependencies:** Task 4；设计「唤起与退出」。真实激活/切换屏幕应使用获准的测试会话。

- [ ] **5.1 接入普通组合键。** 按批准的默认键注册；换绑按「试注册 → 写配置 → 切换生效」合同完成，冲突或保存失败须恢复原注册、持久化值和设置提示。首次注册失败仍能从菜单打开；验证只保留一个有效注册，不启动双击/输入监控 hook。拆开 RuntimeSettings::apply_hotkey_binding 与其生产调用者中无条件获取的 app.autolaunch()/read_autostart/change_autostart；Mac 换绑成功、注册冲突、配置写入失败回滚三条路径均只操作组合键及配置。沿用 3.2 的不一致/报错 spy，三条路径都须保留原 autostart 字段且登录项端口获取、读取、修改计数为零。Windows 继续回归原有快捷键和登录项联合事务及失败回滚。
- [ ] **5.2 接单实例、菜单和 Dock。** 让首次启动、第二次启动、菜单显示、快捷键均经过同一唤起路径。观察 main 可见并能直接输入；第二次启动前后确认常驻 PID、菜单和窗口只有一套。Esc/正常失焦后隐藏，点击退出真正结束。启动失败及退出中重复唤起不得重新创建窗口。
- [ ] **5.3 验证事件归属。** A 正在启动目标应用，B 已再次唤起，此时送入 A 的完成与旧 blur，窗口保持 B 的输入焦点。模拟激活失败/超时，释放旧所有权；下一次菜单唤起可恢复。退出后再启动确认旧快捷键已释放。
- [ ] **5.4 实机验证空间和缩放。** 分别在不同缩放的显示器唤起，移走当前显示器后再次唤起，再休眠/唤醒重试。记录窗口工作区、输入焦点与快捷键响应；任何窗口落在不可见区域、重复事件或原窗口无法退出均须修复。

**Distinct test coverage:** 故障注入后同时核对原生注册、磁盘值和显示值；旧事件不改变新会话。睡眠恢复和多屏行为需要实机证据。
**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib lifecycle::`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib hotkey`；留存冲突错误、双唤起事件序列、PID/屏幕位置及退出后重启记录。

<a id="step-6"></a>
### Task 6: 裁剪前端启动路径与设置

**Files:** `src/{main.ts,protocol.ts,launcher-core.ts,launcher-view.tsx,hotkey-recorder.ts,launcher.test.tsx,hotkey-recorder.test.ts,protocol.test.ts}`、launcher/settings 预览客户端及主题样式；新增真实入口集成用例 src/main-bootstrap.test.tsx。
**Dependencies:** Tasks 3–5；设计「宿主装配与 IPC」「唤起与退出」，逐屏套用 [UI 规范](../../ui-guidelines.md)。

- [ ] **6.1 让 startup 先读取宿主能力。** 在 src/main.ts 的 main 分支，能力获取须先于现有 core.start() **之前**的 preparePanelHostInputFocusListener 和 command-hotkey 预注册，浏览器预览提供一致协议。按快照决定哪些 listener 是前置 ready 条件，再进入 launcher-core.start：其重复的 Panel focus、Panel status/reset/error、Screenshot error 和 messageCenter.start 都按能力绑定，退出时只清理成功注册项。禁用这些能力不阻止 main ready；能力获取失败显示可重试，重试成功才进入正常搜索。
  - 通过真实 main.ts 入口注入 Tauri transport，令所有被禁用的 listen/invoke 一旦被调用即抛错。在 Mac 冷启、重试、pagehide 路径断言 main ready、可输入搜索、禁用调用计数始终为零，必要 listener 恰好注销一次。Windows 同入口提供全能力，要求 Panel 前置监听就绪才启动 core，并保留原必需监听失败时清理及拒绝完成 ready 的行为。
- [ ] **6.2 覆盖所有可见入口。** 搜索分发、设置页、旧收藏/快捷键、右键 action、深链分别经过能力过滤。预置历史禁用项，逐个点击或键盘触发，断言不会调用对应 IPC；后端的直接调用拒绝由 Task 3 保证。Windows 运行原能力快照，匹配原有菜单/设置/快捷键。
- [ ] **6.3 完成设置与输入验证。** 保存主题和组合键，退出重开后核对 UI 和实际键；冲突只显示已生效值并保留错误信息。中文 IME 组合中 Enter 只确认候选，组合结束后 Enter 执行选中项。验证方向键、Esc、焦点恢复、Mac 修饰键文案、双主题、高对比和缩放；在 dev/production WKWebView 分别验证不可误刷新应用。

**Distinct test coverage:** 状态读取失败 → 成功重试、禁用操作无调用、IME Enter 不执行、旧 Windows 入口可用。浏览器预览截图不可代替生产键盘验收。
**Verify:** `npm test -- src/main-bootstrap.test.tsx src/launcher.test.tsx src/hotkey-recorder.test.ts src/protocol.test.ts`。提交调用顺序与未调用接口断言，留存生产双主题、IME 与重启后设置的结果。

<a id="step-7"></a>
### Task 7: 双平台发布门禁

**Files:** `scripts/{verify-local-foundation.mjs,verify-local-foundation.test.mjs,verify-macos-bundle.mjs}`（最后一个新增），新增 CI 工作流，`README.md`、`docs/releases/`。
**Dependencies:** Tasks 1–6 完成，设计「分发与验收」；Mac 原生 arm64 runner、M1 系列最低系统实机、Windows 原生 runner 及匹配的独立 Provider 源码。公开发布另需可用的 Developer ID 与公证授权。

- [ ] **7.1 组合 CI。** 为 Mac/Windows 分别预检 host、工具链、缓存和外部依赖。Mac 记录 uname 与 rustc host；Windows 从 scripts/provider_build.py 实际选择的 UIPILOT_RUST_UIA_ROOT、.uipilot-local.json 或默认 D:/code/rust_uia 获取 Provider 源码，记录 revision、runner、构建清单/hash；缺失即阻塞，不能取不匹配 exe 代替。两平台执行完整本地协调器；按 manifest 分组验证正向平台与不支持平台，保留 Windows-only 示例合同。保存每阶段状态及实际验证包数。
- [ ] **7.2 检查实际包。** CI 从干净输出生成 app/dmg 和 Windows 安装包。新增脚本只读挂载本次 DMG 并定位其中的 .app，取 Info.plist 的 CFBundleExecutable/LSMinimumSystemVersion/标识和图标；遍历全部 Mach-O，以 `lipo -archs`、`otool -l` 核对仅 arm64、部署下限不高于声明版本，并核对主程序的部署目标。注入错误 slice、较高 minos、.exe/.dll 或错误最低版本的夹具，逐个要求脚本非零。验证结束释放挂载；Windows 检查 NSIS/Helper、Provider 及 Everything 清单与产物哈希。
- [ ] **7.3 验证公开分发。** 在获授权的发布流程中签名、公证、staple，保留工具结果；用 `codesign --verify --deep --strict --verbose=2 "$app"`、`xcrun stapler validate "$dmg"` 和 `spctl --assess --type execute "$app"` 校验（变量分别指该候选 DMG 内 .app 与 DMG）。使用真实下载、带隔离属性的候选包执行安装；确认签名身份、hardened runtime 与 Gatekeeper 验收，记录最终 DMG SHA-256。未能签名/公证时只登记内部测试包，公开发布状态为阻塞。
- [ ] **7.4 做 M1 和 Windows 实机验收。** M1 + 精确最低 OS 使用已核验哈希的包，干净安装、菜单和快捷键唤起、中英文搜索、启动/激活、Esc、睡眠恢复、重开、替换升级保留设置、正常退出和卸载应用；核对无非核心权限弹窗。Windows 在干净测试环境安装该次 NSIS，核对 apps、文件搜索、插件授权、托盘退出及原资源；填独立运行结果、视频/日志和证据位置。任一必需行缺失或失败则发布未通过。

**Distinct test coverage:** M1 最低系统设备上的最终 DMG 安装、不满足架构或最低版本要求的反例，以及 Windows 从安装器到启动的独立实测。
**Verify:** 双平台 `npm run verify:local`；Mac `npm run tauri -- build --target aarch64-apple-darwin --bundles app,dmg`、`node scripts/verify-macos-bundle.mjs`；Windows `npm run tauri -- build`。验收记录包含检查的 .app 来源、最终签名后哈希、安装/升级版本和本轮 M1、Windows 实测结果。至此即可判定**首版**，以下各任务单独发版。

<a id="step-8"></a>
### Task 8: 文件搜索与 Finder 操作

**Files:** `src-tauri/src/{file_search/mod.rs,file_search/macos.rs,file_index/mod.rs,commands.rs,find_window.rs,result_registry.rs}`、`src-tauri/capabilities/find.json`、`src/{find-core.ts,find-view.tsx,find-core.test.ts,find-view.test.tsx}`。
**Dependencies:** Task 7；单独批准 Spotlight 索引范围、Find 初始化/固定、对象身份授权及错误展示规格；准备已索引与明确排除的临时目录。

- [ ] **8.1 建立查询后端。** 增加有取消能力的 macOS 查询，查询长度/批次/上限依批准规格执行；禁止调用 Windows Everything。用 fake provider 返回成功、空、明确排除、拒绝和服务错误，检查各自可呈现的状态。真实系统无法判别「未索引」与「无匹配」时按规格提示范围限制，不能杜撰索引状态或全盘补扫。
- [ ] **8.2 绑定 Find 请求和生命周期。** 测试 A 开始、B 发布、A 迟到，最终仅 B 可见；关闭 Find 后 A 再回调，不向已关闭窗口推送。验证准备初始化不会消费排队转发，commit ready 后仅现 owner 消费；固定状态在文件打开等待期间改变时，晚到 completion 按新状态决定隐藏。
- [ ] **8.3 执行与定位。** registry 私存对象身份；开文件和 Finder 定位前重新鉴别当前调用者、结果 ticket 与文件，系统 URL/参数不经过 shell。用特殊字符/空格/中文路径正向测试；将目标换成越界链接、删除文件或从 main 调 find 专属命令，各自拒绝，系统调用计数为零。
- [ ] **8.4 实机复核。** 在 Spotlight 可检索与排除目录各放同名样例，搜索、取消、打开、Finder 定位、固定、关闭再打开，记录命中对象及界面反馈；Windows 重跑原 Find 查询和打开。对应宿主、UI、授权均通过才开放能力。

**Distinct test coverage:** 区分迟到查询与已关闭窗口；记录打开期间 pin 变化与当前 owner；拒绝先于受保护状态读取。
**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib file_search::`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib find_window::`、`npm test -- src/find-core.test.ts src/find-view.test.tsx`。交付查询时序、系统调用计数与 Finder 实机结果。

<a id="step-9"></a>
### Task 9: 插件宿主、凭据与公开能力

**Files:** `src-tauri/src/{public_plugins.rs,plugins.rs,plugin_panel.rs,plugin_window.rs,credentials/}`、`src-tauri/src/public_plugins/{package.rs,manifest.rs,runtime.rs,secrets.rs,webview_audio_guard.rs}`、插件 capabilities、`packages/plugin-cli/`、`docs/plugin-sdk/` 与授权示例。
**Dependencies:** Tasks 7、8、10；核准来源、Keychain 与能力规格。参考 [开发指南 §3、4、5–7、10–12](../../plugin-sdk/public-plugin-developer-guide.md) 及 [UI 规范](../../ui-guidelines.md)；以批准的 SDK v1/schema/types 为准。

- [ ] **9.1 审核装载边界。** 逐项维护[双端能力表](../../plugin-sdk/public-plugin-developer-guide.md#windows-与-macos-兼容性)。共同能力复用公开 SDK 的请求/响应、错误、权限和生命周期合同，原生实现由各端宿主负责；共用的行为用同一组输入、授权/撤权和预期结果对两端验收。 针对每种 manifest 权限记录 Task 8/10 真实可用性。校验安装、升级、禁用/重启恢复 与 `PublicPluginHost`、`PluginInvocationEnvironment` 的平台，未验证能力在权限读取前拒绝。旧 PluginManager 单独演练；不能证明隔离就保持关闭。升级增加权限应重新授权；安装失败保留原版本/原授权。
- [ ] **9.2 先验证隐藏 runtime。** mainResult 也需惰性创建 WKWebView。安装来源、导航/下载边界 → 原生静音 → 注册 unmute → 读回静音 → 复核 generation → 导航受信包入口 → 第一条包指令。为各导航前步骤注入失败，要求零包指令、撤销 generation/拒绝 RPC、销毁实例。成功后触发 unmute，同样先撤销权限再销毁；旧回调晚于同标签新实例时只能指向旧 generation。生产 WKWebView 不能证明顺序就继续关闭 mainResult。
- [ ] **9.3 将宿主凭据与插件秘密隔离。** 分别接入 credentials/ 与 public_plugins/secrets.rs 的 Keychain 后端，按 approved 命名与所有者检查。测试创建/读取/更新/删除、Keychain 锁定、写入失败、撤权和插件冒用 ID；验证未授权请求不触及 Keychain，失败保留旧记录、状态不报成功、日志/JSON 不含测试秘密。
- [ ] **9.4 分别开放可见界面。** 仅在 9.2 通过后，对 Panel 和 Window 各运行生产 origin、导航/下载/音频边界、销毁重建、崩溃恢复、旧 RPC 重放与跨插件访问。新实例只接受自己的 RPC；达不到边界的 surface 继续关闭。用匹配权限的 tracked reference 验证 host/CLI/schema/types 一致（例如 demo-return 声明 clipboard.write，须先取得 Task 10 对应证据）。对 Windows-only 示例仍要求 Mac 校验拒绝。

- [ ] **9.5 用同一插件包在两个生产宿主验收。** 每个版本只打包一次，记录包 SHA-256、两端 UiPilot 版本、主要操作和结果。先用 `demo-settings` 安装、改设置、执行并显示主结果，重启和兼容升级后核对设置；升级版本的新包也在两端使用同一哈希。再用 `demo-return` 验主结果与授权复制、`demo-multi` 验实时/提交两种命令、`note` 验打开独立窗口、写笔记并重开读回。需要授权的操作分别走允许/拒绝路径。Mac 上对声明仅 Windows 的包在安装或恢复阶段拒绝，保留 Windows 的正向验证。
- [ ] **9.6 给出可复核的覆盖结果。** 以 17 个参考包为起点，列出逐包状态、未通过的能力及真实平台限制。对仅声明 Windows 但主要用途可移植的包列出宿主待补能力，在开发候选中验证能力与平台声明，通过后才发布扩大的 manifest。面板优先用 demo-panel 验证焦点与键盘，再以 notes 或 templates 验证一项完整用户流程（例如保存模板、退出重开、调用模板）；同时核对宿主实际提供的平台值、CLI、schema 和 SDK 类型。分别统计声明双平台数、实机双端通过数和平台专属数。旧插件单列，不计入公开 SDK 包分母。

**Distinct test coverage:** 导航前失败、导航后失去静音与旧回调迟到是三条不同序列，分别留 guard 状态/包代码执行次数/实例销毁日志。另查升级新增权限、跨 ID 与两套 Keychain 存储。
**Verify:** `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib public_plugins::webview_audio_guard::`；同命令分别过滤 `public_plugins::`、`credentials::`；`npm run verify --workspace @uipilot/plugin-cli`、`npm test -- src/public-plugin-recovery.test.tsx src/public-plugin-diagnostics-acl.test.ts`。保留真实 WKWebView 时序、隔离 Keychain CRUD/拒绝记录及各 surface 的开放决定。

<a id="step-10"></a>
### Task 10: 系统工具与可选系统集成

**Files:** `src-tauri/src/{clipboard_history/,selection_capture.rs,selection_capture/,result_paste.rs,screenshot/,native_attention/,process_control/,double_tap.rs,hotkey_hook.rs,lifecycle.rs,settings.rs}`，`src/{launcher-view.tsx,screenshot-view.tsx}`、对应权限说明与设置测试。
**Dependencies:** Task 7；逐能力批准最小系统权限、失败回滚、输入控制及实机测试流程。每个子项独立开关，Task 9 只消费实测通过的子项。

- [ ] **10.1 剪贴板历史。** 先接 clipboard_history 的观察、存储及退出取消，再接复制；只采集已获同意的测试内容。设置哨兵 A，开始捕获，另一应用写入 B，再送旧事务完成；确认剪贴板保留 B，停止/退出后无旧 observer 事件。单独记录权限拒绝、禁用状态及重启。
- [ ] **10.2 选区和粘贴。** 在 selection_capture.rs 及 selection_capture/、result_paste.rs 与 clipboard_history/paste.rs 绑定源窗口、目标与事务身份；取选区和发送前各自复核。等待粘贴时切换前台，要求零输入；已发送部分输入时禁止重试。拒绝辅助功能/失去目标时不发送按键，不覆盖后来用户复制的内容。只对获准的临时编辑器执行真实输入。
- [ ] **10.3 截图。** 将截图触发、显示器几何、输出与录屏授权接入 screenshot/ 和前端。主动请求时检查授权；拒绝、捕获中撤销要取消输出、释放缓冲和窗口。使用测试屏幕完成多屏不同缩放截图，核对选择区域、图像尺寸及临时资源清理。Provider 的 Windows 语义保持 Windows-only。
- [ ] **10.4 通知与进程操作。** native_attention/ 连接 macOS 通知授权和真实投递回调，按允许、拒绝、撤销各测一次；拒绝不能记已送达，重复终态回调只消费一次。process_control/ 在执行前重查目标；只对测试进程验证正常操作，再模拟退出或 PID 复用，替代进程不得受影响。Windows 提升 helper 仍走其独立 Windows 链。
- [ ] **10.5 登录项。** 在 lifecycle.rs 为经授权的登录项变更新增独立入口，接入 autolaunch 读写和回滚路径、settings.rs 及设置客户端；普通换键继续验证 5.1 的零调用约束。用临时账户启用 → 系统登录项核对 → 外部撤销 → 刷新 UI → 重启，验证反映真实系统值；设置写入失败应恢复原登录项并报告错误。记录系统和 UI 状态，恢复到测试前设置。
- [ ] **10.6 双击修饰键。** 先实现不取代普通组合键的监听生命周期，再验证权限拒绝/撤销、重复修饰事件、睡眠恢复及退出释放。监听申请失败仍可用普通组合键和菜单；未获明确输入监控权限不得在后台悄然启用。逐项更新宿主与设置的能力快照，失败项留在关闭状态。

**Distinct test coverage:** 记录每个子项调用前后的权限、前台目标、资源所有者和最终状态；不同权限结果不能合并成单个「已测试」标记。将结果交给 Task 9 作为授权上限。
**Verify:** 对每个下列 Rust 过滤器执行完整命令 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib FILTER`（将 FILTER 换成表中单个值），先用同命令加 `-- --list` 确认该平台列出对应边界用例；零命中不通过。

| 子项 | Rust 过滤器 | 另需收集的验证 |
| --- | --- | --- |
| 10.1 | `clipboard_history::` | 用户写 B 后的实际内容与 observer 注销记录 |
| 10.2 | `selection_capture::`、`result_paste::` | 取消后的注入次数及部分输入未重试的调用轨迹 |
| 10.3 | `screenshot::` | `npm test -- src/screenshot-view.test.tsx`；测试图的尺寸与释放事件 |
| 10.4 | `native_attention::`、`process_control::` | 通知拒绝/撤销及进程身份替换记录 |
| 10.5 | `macos_login_item`（在 lifecycle/settings 新增聚焦用例） | `npm test -- src/launcher.test.tsx`；系统面板与应用状态的对应记录 |
| 10.6 | `double_tap::` | `npm test -- src/hotkey-recorder.test.ts`；普通组合键继续有效的实机记录 |

每个子项成功需具备自动断言与获准的实机证据。未具备条件的子项记录原因、负责人及下一次验证条件，继续关闭对应能力。
