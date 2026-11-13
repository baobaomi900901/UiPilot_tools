# 公开插件双平台验收记录

2026-11-10 核对了仓库中的 17 个参考插件。CLI 以 Windows 为目标接受 17 个，以 macOS 为目标接受 4 个，其余 13 个返回 `PLATFORM_INCOMPATIBLE`。macOS 宿主管理器对实际包目录的安装预览接受 3 个，拒绝 14 个。双端生产宿主同包通过数为 **0 / 17**。

| 参考插件（省略 `com.uipilot.`） | CLI 接受的目标 | macOS 管理器 | 生产验收所需能力或操作 |
| --- | --- | --- | --- |
| auth-check | Windows | 拒绝 | 受控 HTTPS、插件秘密引用与认证状态 |
| clipboard-history | Windows | 拒绝 | 面板、读取与复制历史 |
| demo-credentials | Windows | 拒绝 | 受控 HTTPS、插件秘密引用 |
| demo-http | Windows | 拒绝 | HTTPS 白名单及拒绝路径 |
| demo-multi | Windows、macOS | 可预览 | 实时、提交、共享存储。已有 macOS 诊断证据 |
| demo-panel | Windows | 拒绝 | 面板、键盘焦点和导航 |
| demo-recovery | Windows | 拒绝 | 激活失败及恢复 |
| demo-return | Windows、macOS | 可预览 | 主结果、复制授权及撤销。已有 macOS 诊断证据 |
| demo-settings | Windows、macOS | 可预览 | 设置、重启恢复、兼容升级。已有 macOS 诊断证据 |
| demo-win | Windows | 拒绝 | 独立窗口、延迟通知 |
| json-workbench | Windows | 拒绝 | 独立窗口、受控文件读取 |
| note | Windows、macOS | 拒绝 | 独立窗口隔离、写入与重开读回 |
| notes | Windows | 拒绝 | 面板、笔记保存与恢复 |
| pomodoro | Windows | 拒绝 | 独立窗口、计时器、通知 |
| templates | Windows | 拒绝 | 面板、模板保存、恢复和调用 |
| text-snippet | Windows | 拒绝 | 复制、打开 URL、受控粘贴 |
| translate | Windows | 拒绝 | 面板、选区、受控 HTTPS、宿主凭据、语音与复制 |

CLI 检查由 `npm run verify:local` 的 `reference package validation` 阶段执行。macOS 安装预览使用 `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview tracked_reference_packages_respect_the_current_management_boundary`。两个入口都读取仓库中的参考插件目录。

三个主结果插件与一份兼容升级归档由 `scripts/verify-macos-plugin-archives.mjs` 记录 SHA-256 和 macOS 诊断结果。Windows 侧使用同一份报告和归档，入口为 `scripts/verify-windows-plugin-archives.mjs`。Windows 归档管理验证已通过；2026-11-13 用户在 Windows ARM64 虚拟机内使用 x64 安装包，完成上述三个主结果插件的安装、运行、设置、升级、启停、命令改名与收藏、权限撤销和恢复验收。macOS 同包真实 QuickJS 诊断已通过，但生产执行仍关闭，双端生产宿主同包通过数仍为 0 / 17。详细包哈希和手工结果见 `docs/releases/2026-11-06-desktop-port-device-checks.md`。旧插件另行验收。
