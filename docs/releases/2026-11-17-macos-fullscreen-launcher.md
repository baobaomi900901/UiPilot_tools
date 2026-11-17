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

真实全屏 Space 和焦点行为留给用户验收，不能由配置测试代替：替换候选后，在 Chrome 系统全屏状态下用已配置的全局快捷键唤起 UiPilot，确认显示在 Chrome 上方、可直接输入；随后 Esc 隐藏回到 Chrome。仅测试本次新增行为，不重复插件/搜索配置等已验收清单。

候选包构建及签名/资源检查完成后，在此补充精确制品路径与校验值。内部候选继续使用 ad-hoc 签名，不作为公开签名或公证通过证据。
