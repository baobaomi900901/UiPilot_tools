# macOS 内部插件安装与运行时生命周期

目标是在证书之外完成可独立验证的包安装、升级、普通设置、启停和权限生命周期，连接现有数据 Runtime 与实际结果注册表。遵循[安装生命周期设计](../specs/2026-11-08-macos-plugin-install-lifecycle-design.md)，生产 UI/启动加载及签名 XPC 不属于此事务。

技术保持 Rust 1.96.1、既有 serde/rustix/zip 依赖及 QuickJS 探针。独立私有临时目录承载所有写入；只运行受监督的无窗口测试子进程，不执行真实剪贴板/网络/凭据能力，不修改系统设置，不合并 main。

统一执行：先验证针对失败模式的测试，再实现或修正；通过聚焦与全库检查、独立源码复审后提交并推送当前 macOS 分支。整份 inventory 与绑定运行时是一个不可拆开启用的事务单元，作为一个原子提交。保留具体行为变异的红/绿证据，跳过的实机验证不计通过。

### Task 1: 接通已安装快照、生命周期和监督执行

**Files:** `src-tauri/src/public_plugins/{package_macos.rs,package_zip_macos.rs,package_rules.rs,lifecycle_macos.rs,runtime_data_macos.rs,private_file_macos.rs}` 与对应测试、Mac/Windows 模块声明；`spikes/macos-runtime-bridge-probe/`；发布证据记录。

- [x] 依设计「已验证包」完成不可变目录/ZIP快照，复用既有纯规则，保留 Windows IO。
- [x] 依「单份原子安装清单」接入 install/upgrade/settings/enabled/grants 与严格恢复。
- [x] 依「运行时与权限所有权」托管每次 request 的 retirement，接入权威设置快照、真实 session 准入与结果清理。
- [x] 在真实 QuickJS 子进程完成安装、设置、重启恢复、升级和失活回收测试，封存源文件/工具链/实际 helper 哈希。
- [x] 聚焦测试、all-targets、双 preview 全库检查和独立复审通过，更新验收记录并推送。

**Distinct test coverage:** 不匹配批准/旧 consent token 在取消前拒绝；已撤权且停用的旧版本升级后保持原状态；已准入 effect 在撤权后可结束但旧 reply 不得提交，管理操作排空后才能应答；rename 前失败保留文档但不恢复旧 binding，rename 后不确定状态关闭直到重开；库存损坏和 Drop 都清理已捕获 owner；旧结果已发布后再 disable/corrupt/drop 不得误清更新查询。所有具体合同引用设计「失败与验收」。

**Verify:** `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview`；同 feature 的 `cargo check --all-targets`；`bash spikes/macos-runtime-bridge-probe/build.sh`。Cargo/Node 使用本轮已验证的私有工具链与缓存。
