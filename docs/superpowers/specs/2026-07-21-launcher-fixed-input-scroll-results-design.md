# 启动器固定输入框与结果滚动区设计

## 状态

- 日期：2026-07-21
- 基线：本地 `main` at `d939f1a25c503057b7c7ebb740176e71adccbca4`
- 阶段：交互设计已批准，等待设计文档复核

## 目标与范围

启动器保持三段式布局：顶部搜索输入框固定，中间搜索结果区域取得剩余高度并独立纵向滚动，底部状态栏固定在可视区域内。结果数量增加、状态文本变化或键盘选择移动时，输入框和状态栏都不随结果滚动。

本任务只调整前端布局 CSS，并补最小前端回归测试。它不修改 Rust 后端、Tauri 命令或事件、前端协议、搜索状态机、结果排序、执行行为、窗口尺寸、设置页结构或依赖。

## 已审计基线

当前 DOM 已具备所需语义和分层：

- `#app` 内由 Ant Design `App` 生成一层直接子元素 `.ant-app`，其内才是 `.launcher-surface`。
- `.launcher-surface` 已用 Grid 将当前视图和 `.status-region` 分成上下两行。
- `.launcher-view` 已用 Grid 将搜索输入框和 `Spin` 分成上下两行。
- `Spin` 生成 `.ant-spin-nested-loading` 与 `.ant-spin-container` 两层包装；`.result-list` 位于最内层。
- `.result-list` 已是 `listbox`，选中行是 `option`；输入框已通过 `aria-controls` 和 `aria-activedescendant` 关联结果。
- 键盘选择变化已经调用 `scrollIntoView({ block: 'nearest' })`。

内部 Grid、Spin 和结果列表规则已经接近目标，但 `.ant-app` 没有从 `#app` 继承确定高度。这样 `.launcher-surface` 的百分比高度缺少可靠的包含块，内容增多时可能按内容扩张，再被页面根部的 `overflow: hidden` 裁切，而不是把剩余高度交给结果列表滚动。

## 方案选择

### 采用：补齐根高度链，复用现有 Grid 与 Spin 包装层

在 `src/styles.css` 中为 `#app > .ant-app` 传递 `height: 100%`。保留现有 DOM、两级 Grid 和结果列表滚动规则。这个选择只修正高度约束来源，不改变组件树或状态所有权。

### 不采用：增加结果区包装组件

现有 `Spin` 包装层已经能承担中间 Grid 行。新增 React 容器只会重复高度所有权，并扩大 DOM 与测试范围。

### 不采用：JavaScript 测量窗口和三段高度

浏览器 Grid 可以直接分配剩余空间。监听 resize、读取元素高度或写入内联高度会引入时序、缩放和重排问题，也没有必要。

### 不采用：移除 Ant Design `App` DOM 包装层

通过 `component={false}` 改变 `App` 输出会触及组件行为和 Ant Design 上下文边界。为现有包装层补一条受 `#app` 约束的 CSS 规则更小、更明确。

## 三段高度所有权

高度从外到内只有一条确定链：

1. `html`、`body`、`#app` 继续拥有 WebView 可用高度并阻止页面级滚动。
2. `#app > .ant-app` 取得 `#app` 的完整高度，使后代百分比高度有确定基准。
3. `.launcher-surface` 继续限制在可用高度和现有 `420px` 上限内，并以 `minmax(0, 1fr) minmax(24px, auto)` 分配“当前视图”和“状态栏”。
4. 启动器视图内，`.launcher-view` 继续以 `44px minmax(0, 1fr)` 分配“搜索输入框”和“结果区域”，两行之间保留现有间距。

因此三段最终表现为：

- 顶部输入框：固定占用现有 `44px` 行，不参与滚动。
- 中间结果区：取得扣除输入框、间距、状态栏和外层 padding 后的全部剩余高度。
- 底部状态栏：位于 `.launcher-surface` 的独立末行，最小高度保持 `24px`；长文本允许换行并向上占用空间，但不进入结果滚动容器，也不覆盖输入框或结果。

这里的“固定”由 Grid 行所有权实现，不使用 `position: fixed`、绝对定位或硬编码视口坐标。

## Ant Design Spin 高度链与滚动所有权

中间行沿用以下现有链路：

```text
.launcher-view 的 minmax(0, 1fr) 行
  -> .ant-spin-nested-loading
    -> .ant-spin-container
      -> .result-list
```

`.launcher-view`、两个 Spin 包装层和 `.result-list` 都必须允许收缩到 `min-height: 0`；Spin 包装层和结果列表继续占满该行高度。加载指示器只覆盖中间结果区域，不覆盖搜索输入框或状态栏。

在启动器视图中，`.result-list` 是唯一使用 `overflow-y: auto` 的元素。页面根、外层表面和中间包装层只负责约束或裁切，不建立竞争的纵向滚动容器。设置页现有 `.settings-form` 滚动属于另一互斥视图，不在本任务中改变。

## 键盘、鼠标与状态行为

- 输入焦点继续留在搜索框；上下方向键仍由现有搜索状态机改变 `selectedIndex`。
- 选中项变化后继续调用现有 `scrollIntoView({ block: 'nearest' })`，只在选中项离开结果区可视范围时做最小滚动。
- Enter、Escape、输入法组合态和搜索请求行为保持不变。
- 鼠标滚轮、触控板和可见滚动条只滚动 `.result-list`；顶部输入框和底部状态栏保持原位。
- 不为结果行新增点击选择、悬停选择或双击执行行为；现有键盘优先交互不扩展。
- 搜索加载、空结果、错误和结果计数继续由现有状态文本与 `Spin` 表达。

## 无障碍

- 保留输入框的 `combobox`、结果区的 `listbox`、结果行的 `option` 以及现有 ARIA 关联。
- 滚动容器仍是已有 `listbox`，不增加新的可聚焦包装层或 Tab 停靠点。
- `aria-activedescendant` 与 `aria-selected` 继续反映同一选中项；程序滚动不转移输入焦点。
- `.status-region` 继续使用 `role="status"`、`aria-live="polite"` 和 `aria-atomic="true"`，且始终位于结果滚动区之外。
- 长标题、副标题、状态文本、系统缩放和浏览器缩放不得造成三段重叠；中间结果区可以缩小并滚动。
- 深色模式、强制颜色和现有可见焦点样式保持不变。

## 文件边界

预期实现只涉及：

- `src/styles.css`：补齐 `.ant-app` 根高度链；只有测试证明现有 Spin 链不足时才调整同一文件中的既有高度选择器。
- `src/launcher.test.tsx`：保留现有 `scrollIntoView({ block: 'nearest' })` 行为测试，并增加最小布局契约检查。

`src/launcher-view.tsx`、`src/launcher-core.ts`、协议文件、Rust 文件、依赖清单和窗口配置保持不变。

## 验证范围

自动验证：

- 先用一个聚焦前端测试证明根高度链缺失，再做最小 CSS 修复使其通过。
- 保持现有键盘选中、焦点、ARIA 和 `scrollIntoView({ block: 'nearest' })` 测试通过。
- 运行完整 `npm.cmd test -- --run` 和 `npm.cmd run build`。
- 用源码检查确认启动器只有 `.result-list` 拥有 `overflow-y: auto`，并确认没有新增 DOM、依赖、后端或协议变更。

人工验证使用现有开发启动方式，在结果足以超过可视高度时确认：

- 滚轮或拖动滚动条只移动结果行；输入框和状态栏不动。
- 连续按上下方向键时选中项始终可见，滚动距离遵循 nearest 行为。
- 加载中、无结果、错误状态和长状态文本不与输入框或结果重叠。
- 100%、150% 和 200% 缩放下三段仍有明确边界，结果区在空间不足时滚动。

## 完成标准

- 启动器三段高度所有权可由 CSS 和现有 DOM 唯一解释。
- 页面本身不滚动，启动器结果列表是该视图唯一纵向自动滚动区。
- 输入、选择、执行、隐藏、状态发布和无障碍语义均无行为变化。
- 变更保持 CSS-only 生产实现，不新增 DOM、抽象、依赖或后端改动。
