# 双击修饰键释放门控 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 防止长按 Ctrl/Alt 的重复 keydown 被误判为双击，同时保留正常双击和现有 400ms 窗口。

**Architecture:** `DoubleTapDetector` 负责 modifier 按下状态与双击时间窗口；`hotkey_hook` 负责把 Windows 普通/system keydown 和 keyup 分类后转发。重复 keydown 在检测器边界被忽略，hook 仍不拦截任何系统事件。

**Tech Stack:** Rust、Windows `WH_KEYBOARD_LL`、现有 `cargo test`。

## Global Constraints

- 不改变 `DOUBLE_TAP_WINDOW`。
- 不改变快捷键配置字符串、持久化、窗口显示或设置页代码。
- 不过滤或吞掉键盘事件，始终调用 `CallNextHookEx`。
- 不增加依赖或可配置项。
- 人工测试是最终验收依据。

---

### Task 1: 要求释放后才能形成第二次点击

**Files:**
- Modify: `src-tauri/src/double_tap.rs`
- Modify: `src-tauri/src/hotkey_hook.rs`
- Test: `src-tauri/src/double_tap.rs`
- Test: `src-tauri/src/hotkey_hook.rs`

**Interfaces:**
- Produces: `DoubleTapDetector::on_key_up(key: TapKey)`。
- Consumes: `DoubleTapDetector::on_key_down(key: TapKey, now: Instant)`。
- Produces: `key_action_from_message(message: u32) -> Option<KeyAction>`，覆盖 Windows 普通/system down/up。

- [ ] **Step 1: 写长按 RED 测试**

```rust
#[test]
fn held_modifier_repeats_never_fire_without_release() {
    let start = Instant::now();
    for key in [TapKey::Ctrl, TapKey::Alt] {
        let mut detector = DoubleTapDetector::default();
        assert_eq!(detector.on_key_down(key, start), None);
        for elapsed in [50, 100, 200, 399] {
            assert_eq!(
                detector.on_key_down(key, start + Duration::from_millis(elapsed)),
                None
            );
        }
    }
}
```

- [ ] **Step 2: 运行 RED**

Run: `cargo test double_tap::tests::held_modifier_repeats_never_fire_without_release -- --exact`

Expected: FAIL，第二次 keydown 当前返回 `Some(Ctrl)`。

- [ ] **Step 3: 实现按下状态与 keyup 门控**

`DoubleTapDetector` 增加 Ctrl/Alt 按下状态。`on_key_down` 在 modifier 已按下时立即返回 `None`；首次 down 标记按下后沿用现有 pending/window 逻辑。新增 `on_key_up`，只清除对应 modifier 的按下状态。

更新正常双击测试为：

```rust
assert_eq!(detector.on_key_down(TapKey::Ctrl, start), None);
detector.on_key_up(TapKey::Ctrl);
assert_eq!(
    detector.on_key_down(TapKey::Ctrl, start + Duration::from_millis(399)),
    Some(DoubleTapModifier::Ctrl)
);
```

- [ ] **Step 4: 写 hook 消息分类测试并实现转发**

测试精确映射：

```rust
assert_eq!(key_action_from_message(0x0100), Some(KeyAction::Down));
assert_eq!(key_action_from_message(0x0104), Some(KeyAction::Down));
assert_eq!(key_action_from_message(0x0101), Some(KeyAction::Up));
assert_eq!(key_action_from_message(0x0105), Some(KeyAction::Up));
assert_eq!(key_action_from_message(0), None);
```

生产 hook 根据 `KeyAction` 分别调用 `on_key_down` 或 `on_key_up`；只有 down 返回匹配时调用现有 callback。

- [ ] **Step 5: 运行 GREEN 与回归**

Run: `cargo test double_tap::tests`

Run: `cargo test hotkey_hook::tests`

Expected: detector 与 hook 单测全部 PASS。

Run: `cargo test -- --skip plugins::tests::delete::no_follow_handle_move_removes_original_path_and_preserves_identity`

Expected: 除已登记并明确排除的 Windows 插件目录移动测试外全部 PASS。

Run: `cargo fmt --check`

Expected: exit 0。

- [ ] **Step 6: 提交并交付人工复测**

```powershell
git add src-tauri/src/double_tap.rs src-tauri/src/hotkey_hook.rs
git commit -m "fix: require key release for double tap"
```

保留 worktree，不合并 `main`；人工验证长按 Ctrl 不显示窗口、真实双击 Ctrl 只显示一次。
