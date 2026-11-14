# macOS 安装快照模块图接线

目标：让无证书内部运行链使用可复用的真实安装模块图。沿用 Rust、固定 rquickjs 0.14 和现有传输生命周期，遵循[模块图规格](../specs/2026-11-14-macos-runtime-module-graph.md)。不运行 GUI，不打开生产 Runtime。

统一先添加聚焦失败断言，再实现和回归。临时目录与受监督诊断进程提供证据；源码审查检查解析、限额、取消与快照归属。各阶段记录验证并提交，不合并 main。

### Task 1: 不可变模块图与安装快照绑定

**Files:** `public_plugins/runtime_modules.rs`、`package_macos.rs`、`launcher_runtime_macos.rs` 及测试。

- [x] 实现包规则内的模块图、精确路径解析与 wire 重验。
- [x] 运行输入携带当前安装快照生成的图。

**Distinct test coverage:** 未知导入者、路径别名/冲突、边界限额、静态快照在源目录替换后保持原内容；关闭的生产路径仍不扫描插件。

**Verify:** 主库 `runtime_modules`、`launcher_runtime_macos` 聚焦测试。

### Task 2: 真实 QuickJS 诊断接线

**Files:** `spikes/macos-runtime-bridge-probe`、主库 process tests。

**Dependencies:** Task 1；规格“接线与限额”。

- [x] 引擎 resolver/loader 使用同一已验证图；单独放宽一次初始化帧。
- [x] 执行真实安装包的多文件、Unicode、大源码和拒绝用例。
- [x] 回归既有数据/权限/安装/升级/归档诊断，记录证据及剩余边界。

**Distinct test coverage:** 大初始化帧不扩大后续 RPC 限额；重复初始化拒绝；取消阻塞写入后回收子进程；模块失败不发布且后续新请求可恢复。

**Verify:** bridge 的 locked/offline 构建与测试；主库诊断 helper 的 ignored process tests；主库完整回归和 all-targets 检查。

## 验证结果（2026-11-14）

本单元的模块图与 transport 接线一并提交，避免单独的诊断构建缺少共享依赖。诊断工具先前直接依赖 launcher 的保留命令函数，已经改为共享纯规则；主库回归确认所有 ManagedBuiltinFeature 默认名称仍受保护。

- `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview`：605 passed，22 个依赖独立环境/进程的用例 ignored。
- 同一主库命令追加 `launcher_runtime_macos -- --include-ignored --skip process::archive --test-threads=1`，指定新 helper：26 passed。覆盖 43 个模块、1 MiB 控制字符注释（JSON 首帧超过 6 MiB）、中文/空格目录、循环/live binding、重导出、动态导入与源替换不影响安装快照；拒绝系统/URL/属性/缺模块/越界，失败后新调用恢复；重复初始化与 RPC 单帧/累计超限拒绝；大写入被取消后回收停止读取的子进程。
- `bash spikes/macos-runtime-bridge-probe/build.sh /private/tmp/uipilot-modules-final-20261114`：143 passed（129 单元、6 数据进程、2 安装进程、1 loader、5 传输）。51 个进程报告全部 reaped。helper SHA-256：`eef012ab7af0a14e4c02fc67804aeb28c62a6c43246fb0c56c9711f0ae046848`。
- `node scripts/verify-macos-plugin-archives.mjs`：四份正式归档双目标 CLI 校验、4 项宿主归档诊断通过；报告 `/private/var/folders/jf/wmyzg_6x42x1qf0klbbsd_wm0000gp/T/uipilot-plugin-archives.fQuiZC/verification.json`。第一次直接包含 archive 测试因缺少脚本专用环境变量失败，随后由正确入口通过，不作为产品失败或跳过。
- `cargo check --offline --locked --manifest-path src-tauri/Cargo.toml --all-targets --features macos-find-preview,macos-login-item-preview` 与 `git diff --check` 通过。

所有调用只使用临时合成包、测试私有数据和诊断子进程；未操作 GUI、用户插件库或系统权限。生产 Runtime 仍关闭，没有提升双端生产通过数。现有包 `8c0ca12` 可继续进行独立的 Find 预览授权撤销人工回归，无需为内部模块诊断替换 App。
