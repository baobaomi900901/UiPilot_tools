# 普通插件设置编辑实施计划

目标：交付Host0.8.0插件详情普通设置表单及可验证的离线设置示例。设计来源：[普通设置设计](../specs/2026-10-25-plugin-settings-editor-design.md)。使用现有React19/Ant Design6、Tauri2及API v1，新增独立组件与聚焦回归，不改后端保存语义。

## 全局约束

- 遵循设计的“数据与校验”“交互与异步所有权”“可运行示例与版本”，以及docs/ui-guidelines.md。
- 单一源码写入agent；主agent只编辑本轮文档/忽略的报告及交付目录。W为C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools，原D:/code源码只读；共享Rust target可用于构建。无需新工作树。
- 不运行原生Host、安装真实插件或操作用户数据/剪贴板/网络。用户执行人工功能测试；自动化限类型、构建和必要的边界/异步/持久化检查。
- 针对实际失效风险先写聚焦失败检查再实现，不为简单可逆样式写机械测试。完成提交后独立规格/质量评审，再由主agent交付。保留branch不合并/推送。

### Task 1: 普通设置编辑与离线示例完整交付

**Files:** src/public-plugin-settings.tsx（新建，按需拆纯值辅助模块）；src/public-plugin-panel.tsx；src/styles.css；src/public-plugin-settings.test.tsx（新建）；src/launcher-browser-preview.tsx模拟普通设置patch保存；examples/public-plugins/com.uipilot.demo-settings/（plugin-owned manifest/runtime/tests/README/package脚本）；package.json/package-lock.json、src-tauri/Cargo.toml/Cargo.lock/tauri.conf.json、packages/plugin-cli/src/contracts.ts及相关版本断言；SDK developer-guide添加操作入口说明。

**Dependencies:** 已接受设计。普通设置复用savePublicPluginSettings，不改wire API；所有非secret四类全覆盖。公共插件代码前先读public-plugin-developer-guide及schema/API，参考demo-return/multi。

- [x] 提取字段编辑组件，按设计实现dirty patch/校验/取消/未设置和secret隔离。
- [x] 接入详情的互斥busy、已接受inventory和异步session所有权；实现失败不确定提示与重新读取恢复。
- [x] 创建无副作用的settings-demo及1.0.1兼容升级生成脚本，明确人可读输出；不动recovery数据或旧fixtures。
- [x] 同步Host0.8.0及CLI target，更新相关当前版本断言和最小使用说明。
- [x] 运行聚焦检查与构建，自审提交；将完整结果写入.superpowers/sdd/local-foundation/settings-editor-report.md。

**Distinct coverage:** 仅变化普通字段进入IPC/secret不发送；unset不被隐式初始化；空数字/范围错误不提交；原型特殊key正确；失败保留草稿且不盲重试；成功+刷新失败明确区分；pending重复提交只调用一次；关闭重开/换插件后旧完成不刷新新详情；定义变化不能静默覆盖；保存与开发重载互斥。Runtime测试当前设置读取及无存储写入。

**Verify:** npm test -- src/public-plugin-settings.test.tsx src/public-plugin-recovery.test.tsx；npm run build；node --test examples/public-plugins/com.uipilot.demo-settings/tests/runtime.test.js；npm run build --workspace @uipilot/plugin-cli，然后按本地CLI help验证/打包两个fixture。仅版本配置变动需cargo check --manifest-path src-tauri/Cargo.toml --offline --locked，不重跑全Rust。新增合理的相关聚焦检查可补充并报告。

## 主agent交付与人工检查点

独立评审Task1整个diff，解决阻塞项；冻结源码后构建acceptance独立exe与5资源，校验稳定输入/新exe/两包及旧交付不变，写中文用例。用户先测默认值→四类保存生效→重启保留，再测取消、校验、升级保留和键盘双主题。源码实现、自动验证、人工验收分别记录；本轮不计完整基座完成。
## 2026-10-25 0.8.0普通设置交付状态

普通设置编辑纵向交付已实现：四类字段、仅变化字段提交、未设置值保护、取消修改、错误校验、保存结果不确定时重新读取核对、详情异步所有权及与凭据/网络/开发重载互斥。独立设置演示提供1.0.0/1.0.1两个本地包，Runtime每次读取设置，无存储或网络副作用。源码11979bc，审核修复fe3e196；最终Spec PASS/Quality PASS，无遗留发现。审核修复了连续重新读取失败丢失已确认保存结果提示，以及SDK教程当前版本文案。

聚焦检查：UI16、示例2、CLI52、验证协调器18通过，类型/前端构建及离线cargo check通过；旧Vite体积提示仍为非阻塞债务。未机械重跑全仓。最终原生build exit0，700项输入稳定、两包及五资源核对，旧程序未覆盖。

当前交付 `.superpowers/deliveries/uipilot-0.8.0-acceptance/uipilot.exe`，实际版本0.8.0，acceptance数据标识保留；源码fe3e1960cfc8d1bed9e987293431775cf0dfff2e，SHA256 8da8b172a8e55940cd4c104990c32691a17fbba39aa3a40f86278a33acfe6479。本次后续状态提交只改文档，不改变程序。

人工检查仍待用户：安装/默认值、四项编辑生效、重启保留、取消及数字校验、兼容升级保留、双主题键盘和旧数据回归。首批只下发前三项。普通设置UI已交付不等于人工通过，整个Raycast式基座仍未完成。
## 2026-10-25 0.8.0 人工验收收口

用户已确认实际下发的普通设置核心用例：文本/数字/布尔/选项保存读取、完整退出重启保留、取消修改、空值及越界数字阻止保存、1.0.0→1.0.1升级保留及再次保存、双主题与Tab/Shift+Tab/Escape、停用后不能写入及重新启用后配置保留。当前settings-demo1.0.1基线为人工验收/4/true/boxed。旧Notes读取编辑及templates布局已确认；最后一组确认recovery inspect-v2仍为schema2/计数1/RECOVERY_PRIVATE_SENTINEL_V2，只读无重新初始化。

人工发现的两项UI问题已修复并由用户复验：详情开启开关改为绿色（46da433）；Notes面板边界缩放适配，统一物理像素坐标（6ad4b66）。当前验收程序为.superpowers/deliveries/uipilot-0.8.0-panel-fit/uipilot.exe，源码6ad4b6644d3bb141a617229b98a69bd8ba99dc95，版本0.8.0，SHA256839ea9898340e2469f77479a70633256fe1eda04adfabbb9437bb2363a47a4a1。聚焦前端缩放3项、Rust边界/身份/权限4项及前端/原生构建通过，700构建输入稳定。本次仅更新文档，无重新构建。

本轮普通设置核心人工验收收口。原手册初始默认值逐项核对、保存期间重复点击、真实凭据界面/服务等未获得单独人工确认，不扩大为全部扩展矩阵通过；重复提交等边界的自动检查与人工证据分别记录。完整Raycast式本地基座尚未完成：Window RPC、通用动作与逐命令快捷键、标准UI与授权系统能力及M6/M7剩余验收仍待后续。下一阶段优先评估子窗口调用Runtime的闭环，另行形成设计与实施计划；本记录不代表该功能已经实现。
