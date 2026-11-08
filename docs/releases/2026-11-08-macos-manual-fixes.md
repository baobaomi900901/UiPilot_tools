# 70% 实机反馈修复与复验

本轮基于 42a094b 的 M1 Pro / macOS 15.7.4 实测反馈。主线仍为 `codex/macos-port-plan`，不合并 main；生产插件继续关闭。

## 修复内容

- 主输入的键盘准入同时检查 composition 生命周期、WK 的 keyCode 229 与 compositionend 同任务尾窗，防止确认输入的 Enter 执行下方结果。keyup/blur/控件退役清理保护，不以长时间延迟吞掉下一次正常 Enter。真实 WK 跨任务事件时序仍需用户复验。
- 快捷键设置通过可信 main 专用只读 IPC 取得持久配置和当前有效注册，显示 active/fallback/unavailable；首次进入、回到窗口和录制完成后重读，旧响应不得覆盖新状态。首次输入监控授权后的提示包含完整退出重启步骤。读取不申请权限或注册按键。
- Mac 主窗口顶部 8px 留白及底部状态文字区域支持拖拽；输入框、结果与操作按钮不作为拖拽区。对应原生权限只给本地主窗口。Windows 保留原有拖拽路径和布局。
- 登录项 NotFound 保留真实状态，新增“尝试启用”。只有明确点击才尝试一次 mainAppService 注册，无后台注册、重试或对账；成功、失败或异常后均复读系统状态。固定错误分类区分签名、授权、服务不可用、未找到与未知，不输出原生错误正文。此处修复了无法进一步操作和诊断的问题，不能提前声称实机登录项已能启用。
- 文件结果默认在 Finder 中定位并选中文件符合既有 UI 合同。纠正旧验收文案，不改变默认动作。

独立审核已通过。默认自动测试不调用真实登录项、输入监控授权、通用剪贴板或 GUI/媒体播放。实际安装、输入法、窗口拖动和登录项恢复保持待验收。

## 复验顺序（对话中一次一个动作）

1. 退出旧应用，使用新候选 DMG 替换“应用程序”内副本，再从该目录启动。包版本仍为 0.19.0，务必按候选文件名与 SHA-256 区分。
2. 使用此前复现问题的同一中文输入法输入 `today`，回车只确认输入，不执行 Find；待查询完成后再按 Enter 才执行当前结果。另验中文选词、鼠标选词后正常 Enter。
3. 从顶部留白或底部状态文字拖动窗口；输入框仍可选中文字，按钮/结果保持可点击。验证深浅主题下状态提示不遮挡。
4. 查看快捷键设置，当前 Option + 空格应显示当前生效。需要再次验证降级时，再按逐步指引测试双击 Command 的权限撤销/重启；设置页应明确显示备用键，与菜单一致。无需为此立即重新授权。
5. 登录项若显示“系统尚未找到当前应用的登录项”，点击一次“尝试启用”，反馈随后状态及错误分类。此操作确实会尝试登记登录启动；如显示签名要求等错误，不连续点击或手工改系统策略。若启用成功，再按步骤核对系统登录项、撤销与恢复原状态。

## 媒体阻塞项

旧独立探针已经退出且没有声音。其暂停案例中，HTML 未推进，但 WebAudio resume 后推进且用户听到声音，即使原生返回 Suspended；这一失败仍存在，本轮没有重打媒体探针、没有让用户重复相同播放测试、没有将状态读回当作 GuardProof。

后续须验证可覆盖全部音频出口的原生机制，或单独设计不具备网页媒体能力的 runtime。不能通过覆盖 JavaScript、CSP 或轮询暂停降低原合同后宣布插件可用。语音、reload/新导航、替换实例及 WebContent 进程更换仍未实测；Windows 新候选仍未实测。

## 自动验证结果

2026-11-08，Node 22.22.1 / Rust 1.96.1。全仓 `npm run verify:local` 退出 0：前端 797 通过/18 跳过，Mac 库 402 通过/5 默认忽略，代理子进程另 1 通过；CLI 186、公开插件 323 通过；脚本、SDK/schema/参考包通过。Mac 双预览特性 436 通过/8 默认忽略，all-targets 检查通过。忽略与跳过不算通过。

输入法真实 React 事件回归先复现 keyCode 229 被当普通 Enter，修复后通过；登录项 UI“尝试启用”回归先 4 通过/1 失败，修复后协议与控件共 10 通过。独立审核修正了初版拖拽对 Windows 的影响，保留原有 Windows 不使用 Tauri 拖拽属性的断言。Rust 补充 NotFound 显式注册/失败分类/事实独立及有效快捷键状态回归。日志 `/private/tmp/uipilot-1108-{verify,preview,check}.log`、`/private/tmp/uipilot-login-recovery-{red,green,rust}.log`、`/private/tmp/uipilot-ui-{wk-red,fixes-full-final}.log`。

## 固定候选

源码提交：`ae687c09a874d2560de7b122d4ef98f21b008bca`。

| 文件 | SHA-256 |
| --- | --- |
| UiPilot-Mac-fixes-0.19.0-arm64-ae687c0.dmg | `19b5134956b693fb7dd1a028dc331ff09947e745dbe52c111168ca890b59304b` |
| UiPilot-source-ae687c0.zip | `3c8317b9543f8ef7bd6f054a535444de18ad9066360df3f88b3c8b14b0255018` |

DMG 主程序 SHA-256：`a3ea3f384dcae9d6f16f9e819cab676218ac622e49006b99d5cb4af8acf4aeae`。启用 `macos-find-preview,macos-login-item-preview`，仍为内部 ad-hoc 签名、未公证。打包初次在受限环境失败，获准使用打包工具后成功；只读挂载 verifier 确认签名、arm64、最低 15.7.4、书签 entitlement 及资源，无启动 GUI。

交接目录 `artifacts/macos-internal/ae687c0/`，内含 DMG、同提交源码 ZIP、候选专属预检脚本、manifest 和 SHA256SUMS。Mac 预检命令：

```sh
sh preflight-macos-ae687c0.sh ./UiPilot-Mac-fixes-0.19.0-arm64-ae687c0.dmg
```

不要使用旧 42a094b 的预检脚本校验新包。Windows 源码只完成归档校验，原生构建和安装未测；`preflight-windows-ae687c0.py` 已固定新候选源码哈希，不沿用旧 Windows 安装包结论。
