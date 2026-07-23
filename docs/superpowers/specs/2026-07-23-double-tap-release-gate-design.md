# 双击修饰键释放门控设计

## 状态

- 日期：2026-07-23
- 分支：`codex/settings-instant-apply`
- 结论：长按误触发属于应用层双击检测缺陷，不是 Windows 无法区分。

## 问题

低级键盘 hook 当前只转发 `WM_KEYDOWN` 和 `WM_SYSKEYDOWN`。`DoubleTapDetector` 只要在 400ms 内收到两次相同修饰键的 keydown 就判定双击，不要求中间出现 keyup。Windows 长按产生的重复 keydown 因此会被误判为第二次点击。

## 目标

1. 长按 Ctrl 或 Alt 产生任意数量的重复 keydown 都不得触发双击。
2. 只有完整的 `down -> up -> down` 且两次 down 间隔不超过现有 400ms 窗口时才触发。
3. 正常双击 Ctrl、双击 Alt、超时和其他按键取消语义保持不变。
4. hook 继续把所有事件传给 `CallNextHookEx`，不拦截系统键盘输入。

## 非目标

- 不改变双击时间窗口。
- 不改变快捷键录制、持久化或设置页行为。
- 不增加可配置参数。
- 不在本需求中改变 injected-event 策略。

## 方案

`DoubleTapDetector` 为 Ctrl 和 Alt 分别记录当前是否处于按下状态：

- 第一次 modifier keydown：记录 pending 及时间，并标记按下。
- modifier 仍按下时的重复 keydown：直接忽略，不改变 pending。
- 对应 keyup：清除按下标记，使下一次物理按下具备成为第二次点击的资格。
- 释放后的第二次 keydown：若 modifier 相同且仍在 400ms 内则触发；否则重新建立 pending。
- `Other` keydown：继续清除 pending；`Other` keyup 不影响状态。

`hotkey_hook` 同时识别 `WM_KEYUP` 和 `WM_SYSKEYUP`，并把 down/up 动作传给检测器。消息分类使用可单测的纯函数，避免生产 hook 与测试契约分离。

## 测试

- Ctrl down 后连续重复 keydown 不触发。
- Ctrl `down -> up -> down` 在窗口内触发一次。
- Alt 使用相同释放门控。
- 超出窗口重新建立 pending。
- Other keydown 清除 pending。
- hook 消息分类覆盖普通和 system 的 down/up，以及未知消息。

## 人工验收

1. 设置快捷键为“双击 Ctrl”并重启程序。
2. 长按 Ctrl 超过 1 秒，主界面不出现。
3. 快速按下、释放、再次按下 Ctrl，主界面出现一次。
4. 连续重复上述操作，确认不存在单次长按误触发。
