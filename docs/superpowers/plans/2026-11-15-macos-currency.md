# macOS 货币换算接线

补齐 Mac 主启动器货币换算，复用共享 Rust 服务和现有结果 UI。遵循 [设计](../specs/2026-11-15-macos-currency-design.md) 的服务与持久化、查询与执行、验证边界；Rust/Tauri/React 和锁定依赖不变。不使用 GUI 自动化，不合并 main，生产插件 Runtime 仍关闭。

统一流程：聚焦失败断言 → 实现 → 回归 → 合同及代码自查 → 原子提交。使用临时目录、fake resolver 与本地 TLS，不连接线上汇率；实机联网和复制由用户完成。任务顺序 1 → 2。

### Task 1: 共享服务与 Mac 查询接线

**Files:** quick_evaluation/currency、atomic_file_macos、macos_apps、macos_quick_evaluation。

- [x] Mac 编译共享 provider/service/cache/transport，添加 Mac 原子缓存适配。
- [x] 主窗口异步接线，成功复制、错误信息行和查询所有权保持一致。

**Distinct test coverage:** 迟到请求在新查询、隐藏、新 invocation 之后完成时均不可发布；原目录被替换后待写入仍落原目录；超限文件/链接拒绝；共享缓存恢复、网络期限和并发故障场景。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview --locked --offline`。

### Task 2: 前端准入与交付

**Files:** host-capabilities、launcher.test、发布记录及内部产物。

**Dependencies:** Task 1；设计「查询、界面与执行」。

- [x] Mac currency 复制结果准入，不放行其他受限项。
- [ ] 默认/preview all-targets、前端回归和 App/DMG 校验，交付人工候选。

**Distinct test coverage:** Mac 能力过滤后成功汇率可 Enter 复制，错误仍不可执行，插件及不相关 builtin 仍被过滤。

**Verify:** `npm test -- src/launcher.test.tsx src/macos-text-navigation.test.ts`；`npm run build`；默认/preview `cargo check --all-targets`；`verify-macos-bundle.mjs --documents`。

人工节点：替换包后验证同币种零联网、不同币种来源与日期、复制、快速改查和网络不可用提示。对话一次一个动作。

## 自动验证与自查

共享汇率模块首次在 Mac 参与测试，聚焦计算测试 140 通过；最终 Rust 库 701 通过、22 项既有忽略。Mac 结果准入测试先确认失败，再启用 currency；最终前端 416 通过、18 项既有跳过。默认和 preview all-targets、TypeScript/Vite 构建通过。新增晚到成功/失败在同窗新查询、隐藏、新 invocation 三种转换均拒绝发布；缓存超限/符号链接及目录替换测试通过。

代码自查：Windows 保持原 atomic_file 路径及运行逻辑；Mac 的待写入缓存克隆目录句柄，不重新解析可被替换的目录。caller 校验和查询票据早于缓存/网络访问；结果继续经私有复制动作与一次性执行入口。未使用 GUI、真实剪贴板或线上汇率请求。内部包与人工验收待交付。
