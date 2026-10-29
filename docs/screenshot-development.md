# 截图元素探测：开发与打包

UiPilot 通过独立进程调用 `rust_uia` 的 `probe.point`。UIA 实现仅保存在 rust_uia 仓库；UiPilot 仓库保存协议客户端、截图交互和构建工具。

## 启动开发版

本机入口位于 `D:\code\.tools\dev-uipilot.py`，其可恢复模板为 `scripts/dev_uipilot_launcher.py`。

```powershell
python D:\code\.tools\dev-uipilot.py --root D:\code\UiPilot_tools --rust-uia D:\code\rust_uia
```

入口会记住显式提供的主项目路径（脚本旁的 `dev-uipilot.config.json`），仓库内工具记住 `--rust-uia`（主项目根的 `.uipilot-local.json`）。之后可直接运行入口。`UIPILOT_RUST_UIA_ROOT` 可覆盖构建源码目录。

每次启动先在独立 rust_uia 源码目录运行 Release Cargo 构建，再验证 x64 PE、`point_probe_v1` 能力、大小及 SHA-256，生成独立暂存目录与清单，然后运行现有 Tauri dev 流程。Cargo 自己复用缓存；源码编译失败就停止，不退回旧 EXE。允许未提交修改，清单记录 dirty 状态，不自动切分支、拉取或清理代码。

直接 `npm run tauri -- dev` 也进入同一准备流程。`npm run dev` 仍是 Tauri 的前端启动钩子，不等同于启动完整主程序。修改 rust_uia 后重新启动完整 dev 入口，生成新版本 Provider；前端热更新不会重编独立 Provider。

既有 Everything 服务要求、动态开发端口及 UI 依赖保持生效。开发版通过当前进程环境中的 `UIPILOT_SCREENSHOT_PROVIDER_EXE` 使用本次独立暂存资产；不得把该路径写进正式包。

## 构建 Setup

```powershell
$env:UIPILOT_RUST_UIA_ROOT = 'D:\code\rust_uia'
npm run tauri -- build --bundles nsis
```

也可运行 `python scripts/package.py` 并选择 Setup；该入口最终调用同一 Tauri 包装器。Provider 使用固定 x64 目标，并通过显式 `--target-dir` 隔离输出，避免用户 Cargo 配置将产物写到另处后误取旧文件。

包装器持有整个生产构建会话的锁。`beforeBuildCommand` 编译并暂存 Provider，`beforeBundleCommand` 核验本次 generation、哈希和架构后继续已有 Everything/NSIS 流程。`npm run tauri -- bundle --bundles nsis` 也先准备本次 Provider。绕过包装器直接调用 node_modules 中 Tauri CLI 的生产构建钩子会拒绝缺少会话凭据的执行。

安装资源包含 `resources/provider/UIAutoma.UIAutomation.Provider.exe` 和 `asset-manifest.json`。正式程序只读取安装资源；最终用户不需要 Python、Rust、rust_uia 源码或任何开发路径。构建失败、能力不符、清单损坏不会被当作成功包发布。

## 运行和验收

`/screenshot` 默认进入智能截图。命中后显示50%黑遮罩和元素矩形镂空；按下锁定候选及画面，松开完成元素截图。越过系统物理拖动容差后改为跨窗口、跨屏自由框选，之后不再切回元素单击。Esc取消，重拍取消保留旧图。

Provider启动或通信故障会退回可用的窗口/区域选择，不能阻塞截图输入循环。单个控件不支持UIA不代表屏幕图像无法截取。完整人工验收见 `docs/testing/builtin-screenshot-acceptance.md`；特别检查镂空处点击不会触发底层按钮，输出图片不带遮罩和边框。
