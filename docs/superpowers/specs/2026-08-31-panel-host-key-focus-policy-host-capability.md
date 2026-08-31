# Panel Host Key 焦点策略：宿主能力请求

## 文档信息

- 日期：2026-08-31
- 状态：宿主与公共合同已实现，等待 Windows 人工验收
- 目标平台：Windows
- 场景：剪贴板历史 Panel 使用 Tab、方向键和 Enter 的 Host Key 路由

## 用户需要

用户打开剪贴板历史 Panel 后，主输入框应持续持有键盘焦点。Tab 循环切换左侧分类，方向键移动右侧选择，
Enter 执行粘贴。Tab 不应让主输入框光标消失，也不应让右侧 Panel 获得焦点；连续按 Tab 必须依次切换
“全部 → 图片 → 文件 → 文字”。

## 插件场景

剪贴板历史插件只把 Host Key 当作宿主输入框上的快捷控制信号，不需要在 Panel 内输入文字、打开可编辑
对话框或接收后续原生键盘事件。插件在处理按键后重新调用 `focusHostInput()` 会让焦点在 main WebView 与
Panel WebView 之间发生一次可见往返，导致输入框闪烁。

Notes 等 Panel 的需求不同：Host Key 可能打开对话框或把后续输入交给 Panel 内容，因此现有“投递前聚焦
Panel WebView”的行为不能全局删除。

## 已确认根因

宿主 `start_host_key_pump` 在每张 Host Key ticket 投递前执行：

```text
content.set_focus()
  → mark_host_key_native_focused(ticket)
  → eval __UIPILOT_PLUGIN_PANEL_HOST_KEY__(payload)
  → handler settle / ack
```

这与现有 Host Key 设计中“Native focus child WebView once per ticket”的要求一致。第一次 Tab 因此必然把
原生焦点从主输入框移到 Panel：

- 插件不调用 `focusHostInput()`：主输入框光标消失，第二次 Tab 无法进入宿主路由。
- 插件调用 `focusHostInput()`：第二次 Tab 可用，但焦点先丢失再异步抢回，输入框发生可见闪烁。
- 给 Panel DOM 添加 `tabindex="-1"` 只能排除 DOM 顺序焦点，不能阻止宿主 `content.set_focus()`。

因此当前公开插件 API 无法同时满足“主输入框不闪”“Panel 不获焦”“连续 Host Key 可用”。

## 建议的宿主/API 行为

为 `panel` 增加一个显式、兼容的 Host Key 焦点策略。建议形态：

```json
{
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Tab", "Shift+Tab", "Enter"],
    "hostKeyFocus": "host"
  }
}
```

- `hostKeyFocus` 仅允许 `"content" | "host"`。
- 省略时默认 `"content"`，完整保留 Notes 和既有插件行为。
- `"content"`：沿用当前 ticket 投递前 `content.set_focus()` 的行为。
- `"host"`：验证 main WebView/当前 Panel session 仍有效后直接投递 DTO，不调用 `content.set_focus()`，不产生
  预期的 main→content native blur；主输入框在 handler 与 ack 期间持续持有焦点。
- 两种策略共享现有队列、顺序、route sequence、两秒 ack、超时终止、能力隔离和 stale epoch 防护。
- `focusHostInput()` 的既有公开语义不变；`host` 策略下插件处理 Host Key 后不需要调用它。
- 如果宿主采用不同字段名或等价的每键策略，必须保持默认兼容和上述可观察行为。

## 自动化验收

- [ ] Manifest、Rust、JSON Schema、CLI 和 SDK 对焦点策略有一致验证；未知值、错误类型和额外字段失败关闭。
- [ ] 省略策略等价于 `content`，现有 Notes/Demo Panel 行为和测试不变。
- [ ] `host` 策略的 Host Key ticket 不调用 Panel content 的 native `set_focus()`。
- [ ] `host` 策略仍按顺序投递并等待 handler ack；超时、重排和 stale epoch 仍按现有规则终止或无操作。
- [ ] 连续 Tab 在主输入框保持 `document.activeElement` 的情况下依次产生三个 enqueue/delivery/ack。
- [ ] `host` 策略不伪造 `content_got_focus`，也不消耗 `native_focus_blur_expected`。
- [ ] `content` 策略仍能让 Notes 的 Primary+N 打开可输入对话框，并让方向键列表行为保持现状。
- [ ] Enter 的剪贴板粘贴 ticket、目标窗口恢复和一次性粘贴语义不变。

## Windows 人工验收

- [ ] 打开剪贴板历史 Panel 后，主输入框光标持续可见。
- [ ] 连续按 Tab 至少八次，分类循环正确，主输入框不闪、不失焦，右侧 Panel 无焦点框。
- [ ] 连续按上下方向键，列表选择正确，主输入框持续持有焦点。
- [ ] 图片、文字、文件各执行一次 Enter 粘贴，UiPilot 隐藏、外部窗口恢复、只粘贴一次。
- [ ] Notes 的 Host Key、对话框输入和 Panel 内快捷键人工复验无回归。

## 插件边界

当前剪贴板历史插件不能通过 CSS、`tabindex`、延时或重复 `focusHostInput()` 安全规避宿主的原生
`content.set_focus()`。在宿主能力落地前，插件侧应保留现有实现与回归测试，不再用焦点往返掩盖缺口。
