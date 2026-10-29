# 截图元素探测：开发与打包

UiPilot通过独立进程调用 `rust_uia` 原有的 `elementtree` 接口。按用户要求，rust_uia源码已恢复为截图接入前的9cefba7内容（撤回提交ca3b9eb），不再新增截图专用接口。UIA实现仅保存在独立仓库；主程序只保存协议适配、返回几何的选择逻辑、截图交互和构建工具。

## 启动开发版

本机入口位于 `D:\code\.tools\dev-uipilot.py`，其可恢复模板为 `scripts/dev_uipilot_launcher.py`。

```powershell
python D:\code\.tools\dev-uipilot.py --root D:\code\UiPilot_tools --rust-uia D:\code\rust_uia
```

入口会记住显式提供的主项目路径（脚本旁的 `dev-uipilot.config.json`），仓库内工具记住 `--rust-uia`（主项目根的 `.uipilot-local.json`）。之后可直接运行入口。`UIPILOT_RUST_UIA_ROOT` 可覆盖构建源码目录。

每次启动先在独立rust_uia源码目录运行Release Cargo构建，再验证x64 PE、原有 `win32_elementtree`、`win32_inspect_tree_v2` 与 `win32_observation_sessions` 能力、大小及SHA-256，生成独立暂存目录与清单，然后运行现有Tauri dev流程。Cargo自己复用缓存；源码编译失败就停止，不退回旧EXE。允许用户自己的未提交修改，清单记录dirty状态，脚本不修改源码、不切分支、不拉取或清理代码。不再要求或调用 `point_probe_v1` / `probe.point`。

直接 `npm run tauri -- dev` 也进入同一准备流程。`npm run dev` 仍是 Tauri 的前端启动钩子，不等同于启动完整主程序。修改 rust_uia 后重新启动完整 dev 入口，生成新版本 Provider；前端热更新不会重编独立 Provider。

既有 Everything 服务要求、动态开发端口及 UI 依赖保持生效。开发版通过当前进程环境中的 `UIPILOT_SCREENSHOT_PROVIDER_EXE` 使用本次独立暂存资产；不得把该路径写进正式包。

控制台会实时显示 Provider 准备、Tauri/前端启动和 Cargo 编译日志。`Provider 已就绪` 仅表示截图依赖准备完成；`主程序进程已启动（PID …），正在初始化…` 表示主程序开始执行；`主程序已就绪：主窗口前端已连接，设置已加载，可使用启动器。` 才是初始化完成提示。后两项仅用于 debug 开发版，退出时脚本会显示退出码。更新开发脚本后，请 Ctrl+C 停止旧会话并重新运行，原有子进程的控制台连接不会自动修复。

## 构建 Setup

```powershell
$env:UIPILOT_RUST_UIA_ROOT = 'D:\code\rust_uia'
npm run tauri -- build --bundles nsis
```

也可运行 `python scripts/package.py` 并选择 Setup；该入口最终调用同一 Tauri 包装器。Provider 使用固定 x64 目标，并通过显式 `--target-dir` 隔离输出，避免用户 Cargo 配置将产物写到另处后误取旧文件。

包装器持有整个生产构建会话的锁。`beforeBuildCommand` 编译并暂存 Provider，`beforeBundleCommand` 核验本次 generation、哈希和架构后继续已有 Everything/NSIS 流程。`npm run tauri -- bundle --bundles nsis` 也先准备本次 Provider。绕过包装器直接调用 node_modules 中 Tauri CLI 的生产构建钩子会拒绝缺少会话凭据的执行。

安装资源包含 `resources/provider/UIAutoma.UIAutomation.Provider.exe` 和 `asset-manifest.json`。正式程序只读取安装资源；最终用户不需要 Python、Rust、rust_uia 源码或任何开发路径。构建失败、能力不符、清单损坏不会被当作成功包发布。

## 运行和验收

`/screenshot` 默认进入智能截图，进入时冻结桌面。命中后显示50%黑遮罩，元素矩形内部显示同一帧的原始亮度；视觉镂空不会让鼠标穿透。按下锁定候选，松开后保留遮罩并进入选区调整；越过系统物理拖动容差则自由拖框，松开后也进入调整阶段。独立窗口截图的选中结果同样可调整。进入调整后停止元素探测，四角和四边中点显示八个调整点，拖动可改变范围；不重新取帧。Esc取消，重拍取消保留旧图。

识别尚未完成时可显示“窗口候选”，后续精化到可确定的最小元素。正常的重叠布局不再一律视为歧义；竞争的同面积最小元素仍不作为可靠命中。窗口移动、关闭或遮挡顺序变化会使元素候选失效。控件内部动态布局与入口截图不能保证原子一致，动态内容可直接自由拖框。

鼠标移动会立即更新位置，后台先从已经确认的顶层窗口命中鼠标下的原生子 HWND，再通过原有元素树接口收集该范围的完整几何，避免无关区域耗尽预算。元素几何仍全部来自 Provider；无子节点的叶子使用会话根几何。每个范围最多2048节点、64层、3秒，响应最多4MiB；树未就绪或不完整时保留窗口候选和自由拖框。缓存同时绑定顶层窗口和子 HWND 的身份、位置，跨控件移动会选择对应缓存。单个没有原生子 HWND 的大型 UIA 树仍受预算约束。树准备完成后在会话缓存中查询，不再每个坐标远程遍历；结果完成主动通知高亮，选区不变时不重复绘制整屏。首次进入包含Provider校验、启动及树收集时间，不能视为零延迟。

debug版诊断日志位于 `%TEMP%\uipilot-screenshot-<主程序PID>.log`，记录Provider阶段、采集范围、节点数量、截断与预算状态、几何、输入与取消原因，不记录元素文字或图像。原版Provider不依赖新增的按点诊断环境变量。切换至恢复后的原版EXE时必须重启完整dev入口，热更新主程序不足以替换旧的独立编译物。

防抖采用按下屏幕 DPI 对应的 `SM_CXDRAG` / `SM_CYDRAG` 作为两侧容差，阈值内的移动保持单击。语义依据 [Microsoft GetSystemMetrics 文档](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics) 中对应条目的定义；不能将该数值再减半。

调整阶段的工具栏位于选区右下方，空间不足时自动避让到可见屏内。四个图标从左到右为终止、贴图、保存、复制，悬停显示中文提示。保存直接输出PNG，不弹另存为对话框；重名自动加序号且不覆盖旧文件。Ctrl+C复制当前选区，Esc取消。Tab切换八个调整点和四个动作，方向键微调当前调整点，Shift加速；按钮获焦时Enter/Space执行该动作，调整点获焦时Enter保存。输出成功关闭遮罩；失败保留选区并显示错误，可继续调整和重试。编码与写盘在后台执行，文件不含遮罩或编辑控件。

在“设置 → 通用 → 截图默认位置”选择保存文件夹，或恢复桌面。默认桌面通过系统解析，支持重定向和OneDrive；自定义目录重启后仍保留。每次截图使用进入时已提交的目录设置，目录失效时提示失败，不自动换位置。截图所有动作均不发送系统通知、不写入消息中心；失败只在截图界面内提示。

工具栏随风格切换：深色黑底白线图标，浅色白底黑线图标。四个动作使用相同配色，取消不标红，保存不反色；键盘焦点保留可见线框。

贴图将选区作为无标题栏置顶窗口显示，内容保持原物理像素尺寸，外部投影模拟顶部光源。左键拖动移动；聚焦后Esc或右键菜单关闭。可同时保留多张贴图，关闭一张不影响其他贴图或新的截图。贴图不自动写入文件，退出UiPilot时关闭。新截图会包含桌面上仍可见的贴图。

Provider启动或通信故障会退回可用的窗口/区域选择，不能阻塞截图输入循环。单个控件不支持UIA不代表屏幕图像无法截取。完整人工验收见 `docs/testing/builtin-screenshot-acceptance.md`；特别检查镂空处点击不会触发底层按钮，输出图片不带遮罩和边框。
