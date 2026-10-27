# 授权文本粘贴实施计划

目标：交付Host0.14.0与文本片段1.2.0的用户主动粘贴到原应用流程。精确合同以[设计](../specs/2026-10-27-plugin-text-paste-design.md)为准。沿用现有结果授权、Lifecycle外部捕获和主线程隐藏/恢复；新增小型result_paste模块承载原生guard与可模拟顺序。

## 技术与全局约束

现有Rust/Tauri/Windows依赖、TypeScript/React、CLI和纯JS参考插件。无新增依赖或安装。W=C:/Users/moby/.codex/worktrees/local-plugin-foundation/UiPilot_tools；D checkout源码只读，已有编译缓存CARGO_TARGET_DIR=D:/code/UiPilot_tools/src-tauri/target仅进程环境复用。真实应用/剪贴板/键盘测试交用户，agent仅mock/静态检查。Host0.14.0、snippet1.2.0、API/schema1；模板1.2.0和其他已验收包保持。

## 执行规则

subagent-driven-development单写入者。针对性RED→最小实现→GREEN并原子提交；源码完整纵向切片一次独立Spec/Quality审查，修复集中复查，不扩展成历史全分支审查。Task1 → 审查冻结 → Task2。无重复全仓verify；不以测试数宣称基座完成。非UI500ms只读键释放等待与主线程零等待合同严格区分。

### Task 1: 授权pasteText动作与真实插件集成

**Files:** 新增src-tauri/src/result_paste.rs；修改src-tauri/src/{lib.rs,lifecycle.rs,commands.rs,public_plugins.rs,result_registry.rs}及public_plugins/{manifest.rs,result_action.rs,activation.rs}中相关位置、对应直接测试；src/{protocol.ts,launcher-core.ts,public-plugin-panel.tsx}及相关测试；docs/plugin-sdk/{public-plugin-v1.md,public-plugin-developer-guide.md,uipilot-plugin-api-v1.d.ts,uipilot-plugin-v1.schema.json}与manifest parity fixtures；packages/plugin-cli的manifest/contracts/schema/generated-validator及相关测试；text-snippet package/runtime/tests/README；根package/lock、Cargo/lock、tauri与实际Host版本源。现有clipboard_history/paste.rs与其调度只读，优先新增adapter复用Lifecycle，不改历史语义。

**Dependencies:** 设计Manifest/精确合同、目标归属/执行顺序、参考插件章节。现有copy/url/runtime和权限consent路径保持。

- [x] 权限/version/Host+CLI schema同步、严格pasteText解析与整快照requirements、public outcome及前端owner反馈。
- [x] 新show lease捕获/检查、键释放异步等待、主线程短执行闭包；claim/write边界和锁释放；guarded native write/sequence/send。输出typed错误，无副作用重试。
- [x] 文本片段1.2.0次级粘贴入口与文档、当前Host版本0.14；现有存储/默认copy保持。

**Distinct test coverage:** 设计验证节全部具体时序分配至相应native driver/registry/manifest/frontend/example测试。特别单列：等待时发生newshow→旧lease拒绝且零write；等待时撤权/newquery→零write；write成功后自身hide撤销source仍允许固定target发送；write之后外部seq/foreground/keys变化→零send且consumed；写失败→不hide、不send且consumed；hide后target失效→不重试；partial batch→不重放；重复visible show保留capture但旧state lease失效；两个并发同source→一个claim。使用生产调度编排或可注入driver，不能仅源字符串断言代替竞态测试。

**Verify:** cargo offline/locked --lib result_action；新增result_paste过滤；相关lifecycle paste lease和manifest过滤。使用已有target缓存并记录每个命令结果。前端node node_modules/vitest/vitest.mjs run src/protocol.test.ts src/plugin-result-action-bootstrap.test.ts，launcher与权限测试按相关名称过滤；CLI manifest/parity测试、CLI check；node --test examples/public-plugins/com.uipilot.text-snippet/tests/runtime.test.js及既有SDK contract typecheck。cargo run --manifest-path src-tauri/Cargo.toml --offline --locked --bin generate_public_plugin_schema（再--check），更新CLI生成validator并check，npm run build一次。依赖只用已安装版本；不触碰真实系统。

### Task 2: 冻结构建、离线打包与人工交付

**Files:** .superpowers/deliveries/uipilot-0.14.0-acceptance（忽略的交付目录）；本计划完成状态及总路线图。

**Dependencies:** Task1完整切片Spec/Quality通过与clean源码commit。

- [x] CLI build一次、pack snippet1.2.0到新归档、目录/归档validate。
- [x] 复制现有五个资源，冻结源码/dist和包hash；沿用0.13 build-host.ps1/verify-build.mjs模式只改目标版本与包清单。
- [x] offline locked native acceptance build一次；核对PE0.14.0/UiPilot Acceptance、identifier、源输入稳定、旧0.13及templates1.2.0不变。
- [x] 写evidence/短人工用例，第一组版本+权限取消确认+记事本中文显式粘贴。后续跨目标和旧能力回归分组；全部native状态待用户确认。

**Verify:** existing CLI pack/validate；cargo build --manifest-path src-tauri/Cargo.toml --bin uipilot --features tauri/custom-protocol --offline --locked，过程环境TAURI_CONFIG指定com.uipilot.launcher.acceptance/UiPilot Acceptance，不得启动exe。旧资源与二进制只读，禁止覆盖交付文件。

## 2026-10-27 交付状态

Task1源码74525b6，独立审查发现的result_paste编译条件遗漏由77ab4f36b05325d2b03340c6bdd6fa75907f9b8c修复并复查；Spec/Quality PASS。实施记录包含针对性的Rust、前端、CLI、SDK、schema及前端构建结果。test-instrumentation专用检查仍受BASE已有command_hotkeys未加cfg影响，已记录待修，不宣称该配置检查通过。

Task2完成：CLI构建、目录打包与归档校验通过；普通offline/locked acceptance构建成功，PE版本0.14.0、产品UiPilot Acceptance。767个源码/前端输入构建前后一致，五资源与旧15个宿主及templates1.2.0包保持。证据位于.superpowers/deliveries/uipilot-0.14.0-acceptance/delivery-evidence.json。源码冻结77ab4f3，后续计划/路线图提交仅记录交付。

宿主SHA256 d6eb14f98cb8ea2bbde4049e82694a7b4756fb7bb2ef0b9a22083772acfc5973；插件包SHA256 db042eccd03f10ec117da0873891d8eb43ab529ec92820567fb64817cb1f76c9。代理未运行真实程序或操作系统剪贴板/按键。第一组版本、授权取消/确认、记事本单次粘贴人工待验收，后续跨目标和失败回归待下发；本计划实现与交付完成不代表整个基座完成。