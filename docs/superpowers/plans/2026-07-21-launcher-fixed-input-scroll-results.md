# 启动器固定输入框与结果滚动区实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐 Ant Design `App` 根包装层的确定高度，使启动器顶部输入框和底部状态栏固定，中间结果列表独立占用剩余高度并滚动。

**Architecture:** 按已批准规格 `docs/superpowers/specs/2026-07-21-launcher-fixed-input-scroll-results-design.md` 复用现有 DOM、两级 CSS Grid、AntD `Spin` 包装层和 `.result-list` 滚动规则。生产实现只增加 `#app > .ant-app` 的根高度传递；不改变 React 组件、状态机或协议。

**Tech Stack:** React 19、Ant Design 6.5.1、CSS Grid、Vitest 4、Vite 8

## Global Constraints

- 基线是本地 `main` at `d939f1a25c503057b7c7ebb740176e71adccbca4`。
- 生产变更只允许修改 `src/styles.css`；不新增 DOM、抽象、依赖或 JavaScript 高度计算。
- `src/launcher-view.tsx`、`src/launcher-core.ts`、协议、Rust 后端、窗口配置和设置页保持不变。
- 输入焦点、上下选择、Enter、Escape、输入法组合态和 `scrollIntoView({ block: 'nearest' })` 行为保持不变。
- 启动器视图只有 `.result-list` 使用 `overflow-y: auto`；设置页现有滚动不在本任务范围内。

---

### Task 1: 补齐根高度链并保留现有滚动交互

**Files:**
- Modify: `src/launcher.test.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: Node 标准库 `readFileSync` 读取的 `src/styles.css`、现有 `.launcher-surface`/`.launcher-view`/AntD `Spin`/`.result-list` DOM 与 CSS 类名。
- Produces: `#app > .ant-app` 到 `.launcher-surface` 的确定百分比高度基准；不产生新的 TypeScript 或 Rust 接口。

- [ ] **Step 1: 写一个聚焦失败测试**

在 `src/launcher.test.tsx` 顶部加入标准库导入：

```ts
// @ts-expect-error Vitest provides the Node standard library without project-wide Node types.
import { readFileSync } from 'node:fs'
```

在现有源码常量旁读取同目录 CSS：

```ts
const stylesSource = readFileSync('src/styles.css', 'utf8')
```

在启动器视图测试组中加入：

```ts
it('passes the viewport height through AntD App to the launcher grid', () => {
  expect(stylesSource).toMatch(/#app\s*>\s*\.ant-app\s*\{[^}]*height:\s*100%;/s)
  expect(stylesSource).toMatch(/\.launcher-view\s*\{[^}]*grid-template-rows:\s*44px minmax\(0, 1fr\);/s)
  expect(stylesSource).toMatch(/\.result-list\s*\{[^}]*overflow-y:\s*auto;/s)
})
```

- [ ] **Step 2: 运行聚焦测试并确认 RED**

Run: `npm.cmd test -- src/launcher.test.tsx`

Expected: 只有新测试失败；失败信息指出 `stylesSource` 不匹配 `#app > .ant-app` 的 `height: 100%` 规则。既有 Grid 和 `.result-list` 断言应已满足。

- [ ] **Step 3: 写最小 CSS 实现**

在 `src/styles.css` 的 `html, body, #app` 根规则之后加入：

```css
#app > .ant-app {
  height: 100%;
}
```

不修改现有 `.launcher-surface`、`.launcher-view`、Spin 高度链或 `.result-list` 规则。

- [ ] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `npm.cmd test -- src/launcher.test.tsx`

Expected: `src/launcher.test.tsx` 全部通过，且没有 warning 或未处理错误。

- [ ] **Step 5: 运行完整前端验证**

Run: `npm.cmd test -- --run`

Expected: 现有 66 项测试加 1 项新测试全部通过。

Run: `npm.cmd run build`

Expected: TypeScript 检查和 Vite 构建成功；允许仓库既有的单 chunk 大于 500 kB 提示，不允许新增错误或 warning。

- [ ] **Step 6: 自审范围并提交**

Run: `git diff --check`

Run: `git diff -- src/styles.css src/launcher.test.tsx`

Expected: 生产 diff 只有一条受 `#app` 限定的 `.ant-app` 高度规则；测试 diff 只有 raw CSS 导入和一个聚焦测试；没有 DOM、状态机、协议、后端或依赖变更。

```bash
git add src/styles.css src/launcher.test.tsx
git commit -m "修复：固定启动器输入与状态区域" -m "补齐 AntD App 根包装层的高度传递，使现有 Grid 能把剩余高度交给结果列表滚动。保留键盘选择、nearest 滚动、ARIA、协议和后端行为。"
```
