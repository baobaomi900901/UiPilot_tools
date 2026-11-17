# macOS 全屏空间唤起修复

用户报告：Chrome 通过绿色按钮进入系统全屏后，再次唤起 UiPilot 主界面不能显示到其上方。

原 Mac 唤起路径只设置 floating window level，没有配置跨应用全屏 Space 的窗口归属。修复在现有主线程 `present` 路径、show/focus 之前设置 `CanJoinAllSpaces`、`FullScreenAuxiliary` 和 `CanJoinAllApplications`，清除互斥的主/辅助角色及 `MoveToActiveSpace`，保留其他原生窗口行为。依据 Apple 的 [collection behavior](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct) 和 [canJoinAllApplications](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallapplications) 合同；这些 API 的系统下限低于本项目 macOS 15.7.4 下限。

此次只修改 Mac 主窗口的显示配置。沿用已有唤起所有权、位置、输入焦点、失焦隐藏及设置图钉生命周期；不修改 Windows、Find 窗口、权限或插件 Runtime，不合入 main。截图开发继续按用户指令等待 mac_acc。

自动验证：

- 新窗口角色测试先以原行为失败，再实现修复后通过；覆盖空配置、互斥角色、重复唤起和无关标志保留，不创建原生窗口。
- 原生宿主测试：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --features macos-find-preview,macos-login-item-preview macos_host::`，27 passed。
- 默认 `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml --all-targets` 通过。
- `npm run build` 通过；ACL 与 bundle 验证脚本测试共 10 passed。

当前构建缓存已清除，本次工具链安装在用户缓存目录，Rust 1.96.1 arm64 的 rustc/cargo/std 下载均按官方版本清单 SHA-256 校验。测试未进行 GUI 自动化、真实窗口激活或权限变更。

原人工检查范围为 Chrome 系统全屏下的唤起、输入及 Esc 隐藏，不重复插件/搜索配置等已验收清单。用户随后回传「可以，现在窗口置顶了」，确认本次 Chrome 全屏下不能显示在上方的问题已修复，此 bug 验收关闭。该回传仅作为全屏置顶的实机证据，不扩记为本轮输入及 Esc 已逐项复测；这些既有行为不另安排重复测试。

候选源码为 `8d17323edbe967e5b3a7e9d2ee73c80948d51b41`。arm64 release App 构建通过；实际 DMG 只读挂载检查通过，核对了签名、书签 entitlement、资源、架构、版本和最低系统；ZIP CRC 与包内清单文件哈希通过。内部候选使用 ad-hoc 签名，不作为公开签名或公证通过证据。未自动启动候选或替换用户已安装应用，所有 GUI 操作由用户完成。本次记录更新不改变交付包及其哈希，也不重新构建。

交付：`/Users/mobytang/Documents/UiPilot-Mac-fullscreen-8d17323.zip`。内含 DMG、安装说明、源码提交及哈希清单。

| 制品 | SHA-256 |
| --- | --- |
| ZIP | `feb0d1be2ab2a4950efed1ef5c718e8d026b5441b74c45cd343307972512e775` |
| DMG | `15a2be6c5d1300d72cc5a587aabab41baa8b95892a54aea6661e147302f4fe09` |
| 主程序 | `f6e84831b31a120d2d440ba19df11c0439a60fbf04101b09216de561728fcb1b` |
