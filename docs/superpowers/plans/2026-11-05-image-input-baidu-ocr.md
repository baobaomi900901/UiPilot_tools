# 主输入框图片与百度 OCR 实现计划

## 目标与架构

实现图片粘贴、OCR 插件匹配、显式执行百度识别、主列表结果及再次回车复制的完整闭环。技术与行为合同以 [已批准设计](../specs/2026-11-05-image-input-baidu-ocr-design.md) 为准。

宿主维护图片快照及异步归属，声明能力的插件被匹配到列表，百度服务复用宿主管理的凭据与网络授权。插件只编排识别与文本结果，旧文字命令保持兼容。

## 技术与全局约束

- Windows，现有 React 19 / TypeScript、Tauri 2 / Rust 与 Vitest / Node 测试工具链。
- 新能力最低 Host 0.20.0，API/schema 仍为 v1；同步宿主及 CLI 报告版本。
- 依设计“宿主与插件合同”扩展 inputCapabilities、invocation.image 和 services.baidu.ocr。
- 图片只在用户主输入粘贴时导入；显式选择插件后才发送给百度；显式再次 Enter 才复制。
- 服务凭据不进入插件或前端；主窗口调用者、请求、图片、插件代际及授权逐次核验。
- 图片资源边界与取消顺序按设计“图片生命周期与异步顺序”；不放宽通用插件网络大小限制。
- 保留工作区既有未提交改动。已将既有 diff 和重叠文件快照备份到系统临时目录；提交仅含本任务变更。
- 真机测试会启动主程序、读取专用测试图片并写入剪贴板；真实供应商测试只发送已明确选择的测试图，不读取任意用户剪贴板内容。

## 全局执行规则

- 各任务先添加聚焦的失败测试，确认失败原因，再实现并运行相应验证；修复使用同样流程。
- 每个逻辑任务形成原子提交；不得纳入其他任务或用户既有改动。共享 dirty 文件由协调者按差异提交。
- 在当前目录执行，使用 `npm.cmd`；Rust 命令带 `--manifest-path src-tauri/Cargo.toml --offline`。
- 功能模块并行时限定文件所有权；共享协议先达成一致。依赖顺序：Task 1 -> Tasks 2/3 -> Task 4；Task 5 消费 Tasks 1/3；Task 6 汇合全部。
- 每项完成后进行规格与代码质量审查；协调者整合交叉生命周期并完成最终审查。

### Task 1: Manifest、SDK 与兼容性合同

**Files:** `src-tauri/src/public_plugins/manifest.rs`, `src-tauri/src/public_plugins/tests.rs`, `src-tauri/src/bin/generate_public_plugin_schema.rs`, `packages/plugin-cli/src/manifest.ts`, `packages/plugin-cli/tests/manifest.test.ts`, `packages/plugin-cli/src/contracts.ts`, `docs/plugin-sdk/uipilot-plugin-api-v1.d.ts`，生成的两份 Schema/validator，以及宿主版本文件。

- [x] 依设计“宿主与插件合同”定义可选图片 OCR 能力，只允许 Windows submit/mainResult、无重复且有正确版本门槛。
- [x] 定义只读图片描述符与百度服务入参/输出；缺省字段保持旧包兼容。
- [x] 同步 Rust/CLI 语义验证、生成 schema、SDK 和 Host 版本。

**Distinct test coverage:** 旧包缺省成功；单命令/多命令 OCR 能力成功；未知/重复能力、live、Panel/window、旧最低版本、不支持的平台拒绝；生成合同一致。

**Verify:** `npm.cmd test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts`; `cargo test --manifest-path src-tauri/Cargo.toml --offline public_plugins::tests`; `cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema -- --check`。

### Task 2: 宿主图片导入、生命周期与 OCR 路由

**Files:** 新增 `src-tauri/src/launcher_image.rs` 及其聚焦测试；`src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/model.rs`, `src-tauri/src/result_registry.rs`, `src-tauri/src/public_plugins/activation.rs`；按需接入现有 Windows 图像解码工具。

**Dependencies:** Task 1；设计“主界面交互”“图片生命周期与异步顺序”“宿主与插件合同”。

- [x] 有界解码、标准化 PNG、内存图片存储及不透明图片 ID。
- [x] 仅主窗口可导入/清除；采用导入序号与会话归属防止迟到覆盖。
- [x] 主列表按命令能力匹配并显示不可执行原因；执行时复核插件代际、授权和当前图片，再向 Runtime 传递描述符。
- [x] 替换、清除、隐藏、撤权、更新时使图片请求和旧结果动作失效。

**Distinct test coverage:** 损坏/MIME 不符/超限图片不替换旧图；A 导入后 B 先完成、A 迟到仍保持 B；图片删除后旧 ID 与跨插件 ID 拒绝；非 main 调用在读状态前拒绝；A 识别期间替换为 B 或隐藏后 A 响应不能发布/复制；只匹配显式声明的启用兼容命令。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --offline launcher_image`; `cargo test --manifest-path src-tauri/Cargo.toml --offline public_plugins::activation`。

### Task 3: 百度 OCR 宿主业务服务

**Files:** 新增 `src-tauri/src/public_plugins/baidu_ocr.rs` 及测试；`src-tauri/src/public_plugins/host_services.rs`, `src-tauri/src/public_plugins/runtime.rs`, `src-tauri/src/public_plugins/activation.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/credentials/mod.rs`；按需扩展有界网络 transport 的业务专用入口。

**Dependencies:** Tasks 1/2 的图片与请求合同；设计“百度服务与凭据”。共享 activation/commands 由同一后端负责人顺序修改。

- [x] 请求绑定的服务 facade、当前授权/凭据检查、固定官方主机及 general_basic 请求。
- [x] 单次认证和 OCR、图片业务专用上限、固定超时及取消复核；不给通用网络方法提高限额。
- [x] 有界解析 lines/text，固定脱敏错误，不向 Runtime 下发令牌或凭据。

**Distinct test coverage:** 假传输验证表单/端点；未授权、无配置、关闭配置、错误认证/额度/超时/坏 JSON/无文字；请求中凭据变更或撤权后旧成功无效；图片 ID 与上下文不匹配不能产生网络请求；不自动重试，不回传认证材料。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --offline baidu_ocr`; `cargo test --manifest-path src-tauri/Cargo.toml --offline public_plugins::runtime`。

### Task 4: 启动器图片输入与结果状态

**Files:** 新增 `src/launcher-image.ts`、`src/native-input.test.ts`；`src/native-input.ts`, `src/protocol.ts`, `src/launcher-core.ts`, `src/launcher-view.tsx`, `src/styles.css`, `src/main.ts`，以及 `src/launcher.test.tsx`。

**Dependencies:** Tasks 1/2 协议；可先使用假端口实现，最终必须连接真实宿主入口。设计“主界面交互”“图片生命周期与异步顺序”。

- [x] 粘贴图片检测、缩略图/移除按钮、文字筛选与图像状态，保留普通输入路径。
- [x] 插件列表/识别中/结果/错误/Esc 分层；原主列表及复制动作复用。
- [x] 新图/会话归属守卫，迟到搜索/识别/导入丢弃；按键 repeat 不能跨状态提交或复制。
- [x] 双主题、纯文字详情、焦点及固定输入区尺寸。

**Distinct test coverage:** 粘贴不执行插件；第一次 Enter 只识别，第二次独立按键复制；长按与 pending 不重复；文字粘贴和 IME 不回归；替换失败保留原图；清除/隐藏后旧结果不回填；无插件/无文字/错误/复制失败；Esc 菜单优先级。

**Verify:** `npm.cmd test -- src/native-input.test.ts src/launcher.test.tsx`; `npm.cmd run build`。

### Task 5: 百度 OCR 业务插件与开发文档

**Files:** 新增 `examples/public-plugins/com.uipilot.baidu-ocr/{README.md,package/plugin.json,package/dist/runtime.js,tests/runtime.test.js,tests/sdk-contract.ts}`；`docs/plugin-sdk/public-plugin-v1.md`, `docs/plugin-sdk/public-plugin-developer-guide.md`, `docs/plugin-sdk/plugin-examples.md`。

**Dependencies:** Tasks 1/3；设计“插件交付”“验收”。

- [x] 按最小 mainResult 插件模式声明能力/权限/主机/版本，执行服务并返回纯文字及全文 copyText。
- [x] 错误与空结果不提供误导性复制；输出超过合同限额时明确失败，不截断全文。
- [x] 同步现有文档中“OCR 无公开方法”的说明，保留此前用户文档调整。

**Distinct test coverage:** Fake API 验证 imageId 精确传递、requestId 保持；全文/换行/特殊文本；无图片/服务缺失/空文字/服务失败/输出过长；类型检查与 CLI 打包往返。

**Verify:** `node --test examples/public-plugins/com.uipilot.baidu-ocr/tests/runtime.test.js`; `node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.baidu-ocr/tests/sdk-contract.ts`; `npm.cmd run build --workspace @uipilot/plugin-cli`; `node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.baidu-ocr/package --platform windows`。

### Task 6: 集成、原生验收与审查

**Files:** 按集成问题修正以上模块；本计划记录验证证据与交付路径。

**Dependencies:** Tasks 1–5；设计“验收”全部九项。

- [x] 完整前端、Rust/CLI 相关回归，生成合同检查和生产构建。
- [ ] 在实际主窗口验证双主题、截图粘贴与七步闭环；使用专用无隐私测试图调用一次百度并核对复制文字。
- [x] 审查当前变更的请求/图片/结果归属和权限边界；修复后按影响范围复验。
- [x] 输出可安装插件包及实际完成/尚未验证项目，不把静态测试结果当原生验证。

**Distinct test coverage:** 宿主与插件真实组装、安装版本兼容、开启关闭主窗口与重复输入、复制后图钉行为；供应商验收独立于假服务单元测试。

**Verify:** `npm.cmd test`; `npm.cmd run build`; `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib`; `npm.cmd run verify --workspace @uipilot/plugin-cli`; `node scripts/verify-local-foundation.mjs --list`。

## 执行记录

- 初始前端基线首次运行遇到沙箱 `spawn EPERM`；以同一命令申请沙箱外运行，不改测试配置绕过。
- 2026-11-05：设计获用户确认，任务映射及基线检查进行中。
- 用户指定真实验收图片 `C:\Users\moby\Downloads\Snipaste_2026-11-05_22-16-57.png`；279×282，包含六个文件名。只使用此图进行供应商验收。
- Manifest/CLI 合同、SDK 类型、插件 19 项单元测试通过；插件包已生成并通过 archive 校验：`artifacts/ocr-delivery/com.uipilot.baidu-ocr-1.0.0.uipilot-plugin`。
- 前端审查发现命令参数框粘贴和全文 Tab 顺序两处缺口，已补失败测试后修复；TypeScript 与生产构建通过。
- Rust 首轮完整回归：1983 通过、11 忽略、2 个既有原生进程 fixture 测试失败；对应进程控制组串行复跑 9 通过、1 忽略。OCR 相关测试通过；最终审查新增时序回归继续验证。

- 最终前端回归分两组执行：launcher/native 396 通过、18 跳过；其余 35 个文件共 353 通过。合计 749 通过、18 跳过。两处界面修复的复审通过。

- 2026-11-06：后端审查的取消/入队时序、完成后再次识别、请求调试脱敏均修复并复审通过。最终 Rust 完整串行回归 1990 通过、0 失败、11 忽略；日志 `artifacts/ocr-backend-rust-serial.log`。
- 最新开发版已由既有 dev 会话构建并启动；额外启动的验收 dev 会话遇到已有 Everything 资源占用后退出，未停止用户既有会话。
- 原生工具窗口访问未获批准。用户明确选择自行测试并禁止使用 Computer Use，已停止 GUI 自动化。真实百度调用、截图粘贴、双主题和系统剪贴板七步闭环交由用户手动验收，不声明已通过。
- 交付包：`artifacts/ocr-delivery/com.uipilot.baidu-ocr-1.0.0.uipilot-plugin`；使用主程序设置导入并确认权限，沿用已启用的百度 OCR 配置。
- 用户手动验收：粘贴图片与列出百度 OCR 正常，回车识别出现“操作不可用”。定位到 `PublicPluginService::bind_request` 将 `result_publication` 的存在误作 `runtime.resultActions` 协议开关，导致 legacy OCR 在派发前被拒绝。补充按真实服务调用顺序执行、并核对全文复制结果的集成回归；原有 manager-only OCR 测试没有覆盖该分支。
- 修复仅将绑定器分支改为按 `result_actions_enabled` 判断，保留图片请求的发布权限。新增集成回归先复现“没有 dispatch”再通过；相关插件服务 72 项测试通过，精确差异复审通过。既有开发会话已重新编译并启动修复版，通知用户重新粘贴图片测试；未使用 Computer Use。
