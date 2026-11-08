# macOS 主程序插件运行协调接线

完成不依赖证书的 Host 查询、安装生命周期、运行绑定和真实结果注册表接线。遵循[设计](../specs/2026-11-09-macos-launcher-runtime-wiring-design.md)的主程序路径及生命周期边界；沿用现有 Rust/Tauri/React 合同，生产传输关闭，不访问个人数据或启动 GUI。

统一采用聚焦失败测试→实现→回归与独立审查；整体作为一个原子提交推送 macOS 分支，不合并 main。测试使用临时目录与可取消的受控适配器，不能把测试注入变成运行时配置或环境变量开关。

### Task 1: 共享运行协调器与主程序入口

**Files:** `src-tauri/src/public_plugins/launcher_runtime_macos*`、管理/绑定模块、`macos_apps.rs`、模块声明及发布记录。

- [x] 同一库存生命周期、默认关闭的 transport 和真实结果发布。
- [x] 主查询及窗口生命周期调用，并保留应用和内置查询行为。
- [x] 并发回归、全库编译测试、独立审查与证据记录。

**Distinct test coverage:** 关闭路径不触碰库存；旧序号/旧 invocation 不取消新请求；worker 在锁外读取真实普通设置；管理停用或撤权先取消旧权威，迟到结果不可发布；隐藏后重开不受旧清理影响；执行模式、必填参数和内置名称不能绕过准入。

**Verify:** `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview` 与同特性的 `cargo check --all-targets`；精确 IPC ACL 回归。Rust 使用已准备的私有工具链和缓存。

结果：主库 579 通过、8 默认忽略；隔离代理子进程另 1 通过。聚焦 41 项、ACL 4 项、all-targets 检查及独立审查通过。锁内准入、迟到 reset 与等待在途许可三条真实变异均失败，恢复后通过。日志见 `/private/tmp/uipilot-launcher-runtime-evidence.vrfhz1zx`；完整边界见发布验收记录的 2026-11-09 条目。
