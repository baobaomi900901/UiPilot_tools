# 剪贴板历史 Panel 再次打开被隐藏：宿主能力请求

## 文档信息

- 日期：2026-08-30
- 状态：宿主修复已实现，等待 Windows 人工验收
- 目标平台：Windows
- 场景：Panel 插件完成一次显式返回后，再次显示并打开新 Panel 会话

## 用户需要

用户从外部窗口打开 UiPilot，进入剪贴板历史 Panel，选择图片并按 Enter。宿主完成一次图片粘贴并隐藏
UiPilot。用户随后再次打开 UiPilot，选择同一插件并按 Enter，应正常进入新的 Panel 会话，不能被上一次会话
的失焦或隐藏事件关闭。

## 已确认现象

人工复现顺序：

1. 第一次打开剪贴板历史 Panel，选择图片并按 Enter，图片成功粘贴一次。
2. 再次显示 UiPilot，在主界面选择剪贴板历史插件并按 Enter。
3. 主界面立即消失，外部窗口没有发生第二次粘贴。

诊断证据：

- 消失前后 `uipilot.exe` PID 保持不变，排除进程崩溃或开发监视器重启。
- 插件状态保持 `fault: null`，排除 Runtime/Panel ready 故障。
- 真实图片缩略图在与宿主一致的 CSP 下成功解码为 `256 x 122`，Panel 页面无 JavaScript 错误。
- 图片历史索引没有重复或损坏。
- 第二次消失没有再次粘贴，排除打开 Panel 的 Enter 穿透到 `clipboardHistory.paste()`。

故障因此位于宿主的主窗口/child WebView 焦点与隐藏生命周期，不在公开插件代码或图片 DTO。

## 当前宿主缺口

现有 `PluginPanelController` 使用 `focusRevision` 和 `sessionEpoch` 使同一会话中的 blur ticket 失效，也会在
`host_hidden()` 时清理当前 session。但原生 WebView 的 GotFocus/LostFocus 回调可以晚于窗口隐藏和下一次
显示到达。仅比较当前 `focusRevision` 与当前 `sessionEpoch`，无法证明 LostFocus 属于当前主窗口显示周期。

可能的失败时序：

```text
show generation A -> Panel A -> paste -> hide/teardown A
                                      -> delayed native LostFocus A
show generation B -> open Panel B ----^ callback/ticket is attributed to B
                                      -> blur confirmation hides B
```

插件无法观察原生焦点事件、取消宿主 blur ticket 或保持主窗口显示，公开 API 没有安全的规避方式。

## 建议的宿主行为

- 为主窗口每次成功显示分配单调递增的 visibility/show generation。
- 所有 main/content GotFocus、LostFocus、延迟 blur ticket 和内部焦点转移必须绑定创建时的 show generation。
- `confirm_app_blur` 只有在 show generation、Panel session epoch、focus revision 和当前原生焦点所有权全部匹配时
  才能隐藏。
- `host_hidden()`、新一次 `show_main()` 和 Panel session replacement 必须使旧 generation 的未决 blur ticket
  永久失效。
- 旧回调不得被重新解释为新 Panel 会话的 LostFocus；不得仅用“当前 session 存在”替换事件原所有者。
- 修复不能通过固定延时或永久 suppression token 掩盖真实失焦。当前 generation 中用户切换到其他应用时，
  UiPilot 仍应按现有规则隐藏。
- 显式返回、普通 blur、Escape、插件停用和窗口关闭的既有语义保持不变。

最终实现可以采用 visibility generation、原生焦点 owner token 或等价的线性化所有权模型，但必须满足上述
可观察行为。

## 自动化验收

- [ ] `show A -> open Panel A -> explicit-return hide A -> show B -> open Panel B` 后，Panel B 保持显示。
- [ ] 在 `show B` 之后注入来自 A 的迟到 main LostFocus，不能创建或确认 B 的 blur ticket。
- [ ] 在 Panel B mount 前、mount 中和 mount 后分别注入来自 A 的迟到 content/main LostFocus，结果相同。
- [ ] 来自 A 的迟到 GotFocus 不能覆盖 B 的当前焦点 owner 或使 B 的真实 blur 失效。
- [ ] B 中真实的 main/content LostFocus 仍会在既有 recheck 后隐藏 B。
- [ ] 连续执行至少 20 轮 show/open/explicit-return，不出现新会话被上一轮事件隐藏。
- [ ] 旧 session 的 host-key ack、paste completion、focusHostInput ack 和 hide fallback 均不能影响新 show generation。
- [ ] 修复不改变图片/文字/文件 paste 的一次性 Enter ticket 和目标窗口恢复规则。

## Windows 人工验收

- [ ] 图片：打开 Panel、粘贴图片、再次打开 Panel，连续 10 轮均保持正常。
- [ ] 文字：执行相同流程，连续 10 轮均保持正常。
- [ ] 文件：执行相同流程，连续 10 轮均保持正常。
- [ ] 第二次 Panel 打开后切换到其他应用，UiPilot 仍会正常隐藏。
- [ ] 每轮 Enter 只粘贴一次，第二次打开 Panel 时不会自动粘贴。

## 插件边界

插件不能通过延时、重复调用 `focusHostInput()`、阻止 blur、Tauri 私有命令或窗口置顶规避该缺陷。本请求
不授权当前插件任务修改 `src/`、`src-tauri/` 或 SDK；宿主修复完成后，再恢复 Windows 人工验收。

## 宿主实现记录

- 主窗口每次成功显示时分配新的 show generation，隐藏时使当前 generation 失效。
- Panel session 和 app blur ticket 绑定 show generation；旧 generation 的 main/content focus loss 不能隐藏新会话。
- 延迟 blur 真正隐藏前会在主线程重新确认 UiPilot 是否仍拥有原生前台窗口；若新 Panel 已经重新打开并处于前台，迟到 blur 不会隐藏它。
- 当前 generation 的真实失焦仍沿用既有延迟 recheck 后隐藏语义。
