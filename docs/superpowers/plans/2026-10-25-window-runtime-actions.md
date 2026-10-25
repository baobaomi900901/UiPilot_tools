# 子窗口 Runtime 请求实施计划

目标：交付Host0.9.0的Window→Runtime有界调用及JSON工作台1.0.0本地包。合同来源：[设计](../specs/2026-10-25-window-runtime-actions-design.md)。既有Rust/Tauri2、TypeScript/React及原生插件JS，不升级依赖。

## 全局约束与执行

- Windows only，Host/CLI target0.9.0，API v1和旧版本门槛保持；本地安装，不扩展在线范围。
- 执行设计的“公开合同”“会话、线性化与生命周期”“集成边界”；声明式opt-in，物理槽与逻辑撤权分离，锁顺序controller→manager mutation→network transition→scheduler，pending锁不跨controller。
- 工作区C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools；原D:/code源码只读，现有node_modules只读，共享D:/code/UiPilot_tools/src-tauri/target可用于构建。单一源码writer，父agent管理设计/计划与忽略报告。
- 用户授权自主规划、分派、实现、评审、本地提交。保留当前分支，不merge/push/清理。禁止运行原生Host、安装真实插件、访问真实数据/凭据/剪贴板/网络/服务。人工功能测试由用户执行。
- Task1→独立规格/质量评审→Task2→组合最终评审与交付→人工验收。每个任务独立提交与报告，记录真实命令/结果；关键失效风险先聚焦失败回归后修复，不为低风险样式新增机械测试。不重复全仓验证。
- cargo命令使用--manifest-path src-tauri/Cargo.toml --offline --locked，进程级CARGO_TARGET_DIR指向上述共享缓存。生成schema和提交顺序执行。功能已有正确聚焦检查后不因等待而重复跑。
- 主agent记录每次dispatch前HEAD；review package含从该BASE到最终HEAD的完整diff，不能只用HEAD~1。

### Task 1: Window请求、权限与生命周期集成

**Files:** src-tauri/src/plugin_window.rs，public_plugins/{manifest,runtime,scheduler,activation,panel_action}.rs、public_plugins.rs，commands.rs/lib.rs/build.rs及capabilities/generated permissions；必要新增窄window_action/content_action模块；packages/plugin-cli/src及tests相关合同；docs/plugin-sdk/{public-plugin-v1.md,public-plugin-developer-guide.md,uipilot-plugin-api-v1.d.ts,uipilot-plugin-v1.schema.json}；Host/CLI版本配置与相关断言。

**Dependencies:** 设计“公开合同”“会话、线性化与生命周期”“集成边界”，既有Panel/Timer授权。独立设计审核通过后开始。

- [x] opt-in manifest/CLI/schema/SDK及onWindowAction readiness、版本同步。
- [x] 有类型WindowAction dispatch与共享有界JSON/单物理Runtime槽、独立pending settlement与completion ticket所有权。
- [x] Window controller内精确注册与全部生命周期撤销，旧函数会话绑定、bootstrap请求桥及最小ACL。
- [x] 完成边界/竞态回归、旧Panel与window timer重点回归、类型/构建，自审提交，写.superpowers/sdd/local-foundation/window-runtime-task-1-report.md。

**Distinct coverage:** Prepared/AwaitingAck为Busy且onUpdate不会等自身ACK；有效Visible会话可调用；main/shell/panel/runtime不可调用；旧request函数不能借新会话。Window调用期间hide/submit立即逻辑退休且不等待长RPC，旧API失权但物理槽直到completion/deadline才释放。物理完成后凭据/权限变化先于settlement则不能成功；重复回复只settle一次；新generation出现后旧timeout不能销毁新窗口。emit不确定失败仍有watchdog。小版本manifest/handler错误不能发布包；旧window timer/storage不因新增request改变。

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib window_action；同参数--lib plugin_window；同参数--lib public_plugins::scheduler；同参数--lib public_plugins::activation；必要现有panel_action/权限bootstrap的精确过滤；cargo run --manifest-path src-tauri/Cargo.toml --offline --locked --bin generate_public_plugin_schema -- --check；npm test --workspace @uipilot/plugin-cli -- tests/manifest.test.ts tests/validate.test.ts；npm run check --workspace @uipilot/plugin-cli；npm run build。实际test命名可调整，必须报告发现数量，不可零测试通过。新SDK签名按仓库既有tsc合同检查。

### Task 2: JSON工作台与可人工验收交付

**Files:** examples/public-plugins/com.uipilot.json-workbench/package/plugin.json、dist/{runtime.js,window.html,window.css,window.js}、README.md、tests/runtime.test.js、tests/sdk-contract.ts及必要package.json；示例发现断言若有固定计数仅更新对应项。忽略目录.superpowers/deliveries/uipilot-0.9.0-acceptance/、.superpowers/sdd/local-foundation/window-runtime-task-2-report.md。

**Dependencies:** Task1独立审查通过；设计“JSON工作台插件”“验证与人工交付”；docs/plugin-sdk/public-plugin-developer-guide.md与docs/ui-guidelines.md。插件代码实现只在新包/tests/docs，不修改Host/SDK来适配插件；发现API缺口报告主agent处理。

- [x] 实现onCommand只读初值与onWindowAction load/format/save，严格输入与存储校验、revision冲突、无自动重试或数据重置。
- [x] 实现紧凑双主题子窗口、草稿/请求所有权、忙碌/错误/不确定保存和显式重新读取，内容端不调用storage。
- [x] 插件业务边界测试、SDK签名检查、CLI验证打包，自审提交并提供人工步骤；主agent冻结源码构建Host与交付hash。

**Distinct coverage:** 格式化标准JSON、非法语法、1e400非有限、嵌套/数量/UTF-8限额；保存空/未完成JSON草稿、revision冲突、invalid-stored-data不重置、保存失败不重试。UI的重复按钮、关闭重开与陈旧结果、键盘/主题主要由用户测试。

**Verify:** node --test examples/public-plugins/com.uipilot.json-workbench/tests/runtime.test.js；npm run build --workspace @uipilot/plugin-cli；node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.json-workbench/package；SDK合同使用既有编译方式。按实际CLI help执行pack并验证archive。无需代理执行插件原生窗口。

## 最终交付与验收

最终whole-change reviewer审查Task1与Task2范围；解决Critical/Important后交付。冻结源码及dist记录前后hash，TAURI_CONFIG使用{"identifier":"com.uipilot.launcher.acceptance","productName":"UiPilot Acceptance"}，cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked。独立新kit包含exe、已有五资源、JSON插件包、中文README和校验记录，旧kit保持。

首批用户测试安装/打开、正常与错误JSON格式化、保存/关闭重开/进程重启。之后快速交互与键盘主题、旧Notes/templates/settings/recovery短回归。实际反馈逐项记录，未完成总基座不得宣称完成。


## Task1交付门槛状态

2026-10-25 Task1实现48a61c9，独立评审修复e69184d079573a477d9560007accb96048d08b50。最终Spec PASS/Quality PASS，无遗留发现，Task2已可执行。修复了原生窗口清理及focus/blur/close的document身份竞态、验证协调器版本遗漏。相关实际检查与两次未落盘审批拒绝/窄化替代见window-runtime-task-1-report.md；既有Vite体积提示保留。此为源码与自动边界验证，不代表新窗口原生人工验收通过。

## 2026-10-25 0.9.0交付门槛

Task2提交e770d6a19a973438bf2624a6d7a80b4e4a850738，新增JSON工作台1.0.0与示例发现断言。13项Runtime、6项窗口所有权、18项协调器检查共37项通过；SDK严格类型检查、CLI构建、目录与归档验证通过。Task2/最终组合独立评审Spec PASS、Quality PASS，无遗留发现；Task1及修复的审查证据保留，未机械重跑全仓。

最终npm run build与离线锁定原生cargo build均exit0。717项源码/dist输入前后稳定，两包及五资源匹配，旧六个交付程序hash未变。程序实际ProductVersion/FileVersion均0.9.0，ProductName为UiPilot Acceptance，保留com.uipilot.launcher.acceptance数据标识。构建源码e770d6a19a973438bf2624a6d7a80b4e4a850738；交付.superpowers/deliveries/uipilot-0.9.0-acceptance/uipilot.exe，38829056字节，SHA256 e5424769896c3645f5686873a822559b6af748327c128239753f8217d546895d。JSON包SHA256 054eb645a6375ced9023098118b3772ed4a746d8fcd4c2e097826ef569030b40。构建记录位于该kit的delivery-evidence.json；实现与评审记录位于.superpowers/sdd/local-foundation/window-runtime-task-2-report.md及window-runtime-final-review.md。

全部0.9原生人工用例仍待用户执行。首批为安装/打开、正常及错误JSON格式化、保存后关闭重开及完整退出重启；后续草稿/快速交互/主题键盘和旧插件短回归。代理未运行原生Host或操作真实数据，完整本地基座尚未完成。本次状态提交只改文档，不改变已冻结程序。

## 2026-10-25 0.9.0核心人工验收收口

用户逐组确认全部实际下发用例符合预期。第一组：JSON工作台安装与打开、合法格式化、非法JSON错误后可编辑、显式保存后关闭重开及完整退出重启保留。第二组：未保存编辑关闭重开恢复已保存文本；重新读取的保留/放弃确认；快速格式化与关闭重开无异常或陈旧结果覆盖；深浅主题、Tab/Shift+Tab焦点、长文本区域滚动且按钮无遮挡。第三组：settings-demo既有配置、Notes原笔记读取、templates预览、recovery inspect-v2原schema2/计数1/sentinel只读数据保留；番茄钟1.2.0本地包安装、10分钟计时关闭子窗口后继续、到期消息/响铃、打开主窗口停止响铃及重开操作。第三组用户明确回复“插件安装成功, 并且所有用例都测试过, 符合预期”。

Windows文件选择框曾因粘贴正斜杠链接路径提示文件名无效，改用完整反斜杠路径后用户确认安装成功，无产品源码修改。

当前已验收程序仍为.superpowers/deliveries/uipilot-0.9.0-acceptance/uipilot.exe，源码e770d6a19a973438bf2624a6d7a80b4e4a850738，版本0.9.0，SHA256 e5424769896c3645f5686873a822559b6af748327c128239753f8217d546895d。本次仅记录人工反馈，不重新构建或重跑已通过检查。独立评审与自动边界证据见前文，实际用户反馈单独记录。

本轮Window→Runtime与JSON工作台核心人工验收通过。未单独下发或确认的保存期间继续编辑、不确定保存、真实超时/故障注入等扩展人工场景不因此标为已执行。完整Raycast式本地插件基座尚未完成，后续仍包括通用结果动作与逐命令快捷键、标准UI及授权系统能力、M6/M7剩余生命周期与总验收；继续保持本地包范围，不涉及在线安装/发布。
