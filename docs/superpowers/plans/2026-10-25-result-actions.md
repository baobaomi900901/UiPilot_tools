# 主结果动作与文本片段实施计划

目标：交付Host0.10.0的公开插件多动作结果、可发现的动作菜单，以及独立文本片段1.0.0本地插件。技术合同以[设计](../specs/2026-10-25-result-actions-design.md)为准。复用Rust/Tauri2宿主、TypeScript/React启动器与已有Runtime，不升级依赖。

## 全局约束

- Windows only；runtime.resultActions=true要求minimumHostVersion>=0.10.0、全部mainResult及onAction导出。APIv1、状态schema2、CLI报告schema1、CLI包0.1.0保留；旧示例最低版本不变。
- 新execute_result_action/retire_result_actions仅main；opaque handles；onCommand旧键省略合同不变。新copy保留主界面，旧copy行为保持。runtime动作返回整个新结果快照，无自动重试。
- 执行设计“Private IPC and identity”“One snapshot claim and origin lease”“Admission, locks and effects”“Completion, cancellation and failures”；registry不得回调先序manager/scheduler锁；已受理副作用与迟到UI反馈分开。源失效立即撤销lease，物理JS槽只在完成/deadline/确认替换后释放。
- 工作区C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools。原D:/code/UiPilot_tools源码只读；已有node_modules只读；共享CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target可用。单一源码writer，父agent管理状态/交付；无新依赖、merge/push/清理。
- 用户授权自主开发/agent/评审/本地提交；原生Host运行、安装、真实数据/凭据/剪贴板/网络/服务留给用户人工验收。读repo UI规范并遵守；插件任务只改新包/tests/docs及发现断言，不修改Host。
- Task1→独立Spec/Quality review→Task2→独立review→Task3→组合review→冻结构建/交付→人工验收。每项单独提交、报告BASE与HEAD/真实命令与结果。每次分派前父级记录BASE，diff覆盖全任务而非HEAD~1。
- 对权限、并发、持久化、边界等风险先聚焦失败回归再修复。样式/真实操作主要人工检查。用已通过证据，不反复重跑全仓；新增变化或具体未解风险才扩测。
- cargo使用--manifest-path src-tauri/Cargo.toml --offline --locked，进程级共享target；schema生成与检查顺序进行。不并行修改同一源码或共享编译产物。

### Task 1: 主结果动作合同与宿主执行闭环

**Files:** src-tauri/src/public_plugins/{manifest,activation,runtime,scheduler,content_action}.rs、public_plugins.rs、result_registry.rs、commands.rs、plugins.rs及必要新result_action.rs；main生命周期失效调用点；协议DTO、lib.rs/build.rs、capabilities/generated ACL；packages/plugin-cli相应验证/版本与fixtures；SDK schema/types/contract/tutorial；Host package/Cargo/Tauri/lock版本及验证协调器版本断言。仅为具体交界调整已有模块，不做无关拆分。

**Dependencies:** 独立设计审核通过；设计“Manifest, compatibility and public SDK”至“Completion, cancellation and failures”。

- [x] 声明式opt-in、精确结果/action解析、onAction readiness/调用/snapshot、Host/schema/CLI/SDK与0.10版本一致。
- [x] registry私有动作bundle、opaque IDs、一次性snapshot claim、source lease及所有使源不可用的失效入口；绑定新main-only IPC与严格DTO。
- [x] 在共享admission/mutation/authority/scheduler/registry顺序内原子受理和completion claim+replace；固定typed outcomes，网络/API租约失效，后台deadline与单次settlement。
- [x] 聚焦检查、自审、原子提交；报告.superpowers/sdd/local-foundation/result-actions-task-1-report.md，冻结供Task2消费的Rust DTO/IPC。

**Distinct test coverage:** optional absent/false/true及不兼容manifest；旧invocation/parse不变；实际adapter/ACL拒绝非main/未知/缺失字段与旧execute绕过。跨command/set相同business ID不别名；Busy不消耗、重复/失败消耗、precommit失败无ghost请求。registry每个退休入口使旧API失权但JS物理槽仍占用；新命令等待。初次onCommand发布及动作完成均须在权限/确认owner保护内，不能用无条件authorize回调；旧完成在权限撤销/新query之后不得发布；completion后的正常成功不要求已过期facade。真实共享admission barrier与legacy reserve commit/cancel、公共操作partial error；无锁倒序。emit不确定且无后续输入时watchdog自主完成/重建、只settle一次、不损伤新generation。copy使用假executor验证权限与副作用受理，不碰真实剪贴板。

**Verify:** cargo test --manifest-path src-tauri/Cargo.toml --offline --locked --lib result_action；同参数--lib result_registry及public_plugins::scheduler，必要现有Panel/Window权限/桥精确回归；新增可执行bootstrap检查用npm test -- <对应测试文件>；CLI manifest/validate聚焦测试、check/build；schema generate/check与CLI同步检查；node --test scripts/verify-local-foundation.test.mjs；npm run build。准确过滤可按实际模块命名，但不能零测试冒充通过；报告数量和退出码。

### Task 2: 启动器动作菜单与结果所有权

**Files:** src/protocol.ts及桥client、launcher-core.ts、launcher-view.tsx、ui-theme.css；对应protocol/launcher测试及必要窄动作菜单模块。

**Dependencies:** Task1独立review通过；设计“Private IPC and identity”“Launcher interaction and frontend ownership”及失败文案。docs/ui-guidelines.md。

- [ ] 校验sanitized action metadata及typed outcomes；default/no-default选择、新结果安装保留suffix控制与caret；不调用finishSearch或自动执行返回结果。
- [ ] 统一底栏“动作 Ctrl+K”、右键与键盘菜单；pending/consumed反馈和显式“重新运行”；收藏入口/非动作结果行为保持。
- [ ] edit/departure立即发source-bound退休通知；捕获query/value/request/command等owner，迟到cleanup/反馈/刷新不污染新界面；主题/焦点遵循现有规范。
- [ ] 聚焦检查、自审、提交；报告.superpowers/sdd/local-foundation/result-actions-task-2-report.md。

**Distinct test coverage:** no-default打开菜单；三入口执行同一选择且不会额外触发行默认动作；重复点击单受理；复制/受理失败禁用旧动作且rerun只走普通submit；busy可重试不消耗；staleRequest/unknownResult即使未消耗也禁用当前源并提示重新运行。编辑在debounce前退休；陈旧retire不影响新set；旧response/error/pendingcleanup不覆盖newquery/view；改变highlight不阻断合法替换；新handle原子安装且无autoexecute。原收藏、旧复制与Find不退化。键盘/双主题真实显示交用户测试，保留必要DOM事件路由/可访问名称检查。

**Verify:** npm test -- src/protocol.test.ts src/launcher.test.tsx（必要按明确新模块/场景聚焦）；npm run build。无需重跑未变Rust。

### Task 3: 文本片段插件及0.10验收交付

**Files:** examples/public-plugins/com.uipilot.text-snippet/{package,tests,README.md,package.json}；scripts/verify-local-foundation.test.mjs仅发现项/数量；忽略交付.superpowers/deliveries/uipilot-0.10.0-acceptance/及报告。

**Dependencies:** Task1/2独立review通过；设计“Local example: Text Snippet”“Review and acceptance”；先读SDK教程/UI规范，参考demo-multi及json-workbench私有记录校验。

- [ ] 两命令transform/latest、原文/大写行与copy/save/read动作；onCommand只读，严格记录/revision/CAS/字节限额，写一次且失败不重试；纯插件实现。
- [ ] 业务边界和SDK合同检查，fresh CLI目录/归档验证打包，中文人工步骤；更新发现计数，自审提交并报告result-actions-task-3-report.md。
- [ ] 父级组合review后冻结源码/dist，构建新Host0.10验收kit，保存exe/两侧hash/资源/包/人工用例。此复选项在父级完成交付前保持未勾选。

**Distinct test coverage:** 两command相同business ID隔离；格式转换及UTF8上限；只读command不写入；save/revision冲突/overflow/坏schema拒写/失败不重试；空值与缺失记录；fresh SDK types。Native菜单/复制/重启数据由用户确认。

**Verify:** node --test examples/public-plugins/com.uipilot.text-snippet/tests/runtime.test.js；既有TypeScript strict/noEmit SDK检查；node --test scripts/verify-local-foundation.test.mjs；npm run build --workspace @uipilot/plugin-cli；CLI validate package、pack到新.uipilot-plugin、validate archive（--platform windows --json）。

## 交付门槛

组合review解决Critical/Important；记录从本轮BASE4a3b994至最终HEAD的全diff。冻结后npm run build、前置hash、TAURI_CONFIG={"identifier":"com.uipilot.launcher.acceptance","productName":"UiPilot Acceptance"}及cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked。新kit保留五resources、文本片段包、中文README及provenance；不覆盖0.9及更早kit。核对实际版本、全部实际发现构建输入、旧交付hash；数量按发现记录。

首批人工：安装并用三个菜单入口；复制已知文本到临时编辑器；保存→latest读取→重启保留。后续重复交互/输入变更/主题键盘、旧插件短回归。只记录实际下发并获确认项；总基座仍未完成。