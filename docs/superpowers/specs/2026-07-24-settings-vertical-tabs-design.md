# UiPilot 设置页垂直 Tabs 设计

## 目标

本需求只调整设置页的信息架构、焦点入口和滚动布局：

1. 标题“设置”保留为页面一级标题，但不能获得焦点。
2. 标题栏下方改为左右布局的垂直 Tabs，入口在左，当前页面在右。
3. 首期固定两个 Tab：`通用`和`插件`。
4. 每次进入设置页默认打开并聚焦`通用`Tab。
5. 保留现有设置即时生效、插件管理、主题和错误恢复行为。

## 非目标

- 不修改 Rust、Tauri command、DTO 或持久化格式。
- 不持久化当前 Tab，也不记住本次进程内上一次选择。
- 不改变设置和插件清单的加载时机。
- 不增加新设置项、插件能力或全局导航。
- 不为窄窗口增加折叠、抽屉或横向 Tabs 变体。
- 不重构 LauncherCore 中已经独立的普通设置与插件清单状态域。

## 当前行为与问题

当前设置页在 `LauncherView` 内把普通设置和插件清单纵向堆叠在同一个 `.settings-form` 滚动容器中。进入设置视图时，`useLayoutEffect` 会调用 `headingRef.current?.focus()`，同时标题带有 `tabIndex={-1}`。因此标题虽然不在顺序 Tab 导航中，仍会被程序主动聚焦。

普通设置和插件清单在状态层已经相互独立：进入设置页会分别加载设置快照和插件清单，插件逐行 mutation 也不占用普通设置 operation。本需求可以只重排前端视图，不改变数据所有权。

## 选定方案

使用项目已经依赖的 Ant Design `Tabs`，配置 `tabPosition="left"` 和受控 `activeKey`。不自行实现 tablist、方向键和 ARIA 行为。

页面结构为：

```text
+------------------------------------------------------+
| 设置                                          关闭   |
+-------------+----------------------------------------+
| 通用        | 快捷键                                 |
| 插件        | 开机启动                               |
|             | 风格                                   |
|             | 恢复初始化                             |
|             |                                        |
|             |              右侧独立滚动              |
+-------------+----------------------------------------+
| 全局状态栏                                           |
+------------------------------------------------------+
```

选择`插件`后，右侧替换为现有插件清单、说明和操作按钮。两个页面不会同时可见。

## 组件与内容归属

### 顶部栏

- 保留现有 `.settings-header`。
- 左侧仍为语义化 `<h1>设置</h1>`。
- 从标题删除 `headingRef`、`tabIndex` 和所有主动聚焦逻辑。
- 右侧保留现有关闭按钮及 `core.requestHide()` 所有权。

### 通用 Tab

包含现有普通设置 UI：

- 快捷键录制
- 开机启动
- 风格选择
- 恢复初始化
- 普通设置加载失败时的重试入口

即时保存、只读锁定、失败回滚和 `settingsUncertain` 语义保持不变。

### 插件 Tab

包含现有插件 UI：

- 插件清单 loading、error、empty 和 ready 状态
- 插件 ID、版本、触发词和 Markdown 说明
- 重新加载和删除操作
- 插件清单失败重试和逐行错误

插件项目继续使用无卡片的列表样式，不改变 Markdown 安全边界和确认交互。

## Tab 状态与生命周期

Tab 选择只属于 React 视图，不进入 `LauncherCore` 或协议层。

视图保存一个带设置页 epoch 的本地选择：

```text
{ viewEpoch, key: "general" | "plugins" }
```

渲染时，只有保存的 epoch 等于当前 `snapshot.viewEpoch` 才采用保存的 key；否则有效 key 固定为 `general`。因此新的设置页 epoch 在首帧就显示`通用`，不会先闪现上一次的`插件`页面，也不需要额外状态复位渲染。

用户切换 Tab 时，只更新当前 epoch 的本地 key。Tab 切换不得调用：

- `load_settings`
- `list_plugins`
- 任意保存或插件 mutation command

离开设置页不会取消后端已经提交的普通设置或插件逐行操作。操作完成后仍由现有 owner、epoch 和 reconciliation 规则更新核心状态；再次进入对应 Tab 时展示最终投影。

## 焦点与键盘行为

进入设置页时的唯一主动焦点入口改为当前选中的`通用`Tab。

实现约束：

1. 设置 Tabs 根节点保存 DOM ref。
2. 仅当 `snapshot.view` 进入 `settings` 或 `snapshot.viewEpoch` 变化时，在 layout effect 中查找根节点内 `[role="tab"][aria-selected="true"]` 并调用 `focus()`。
3. 普通设置、插件清单、主题或 mutation 状态更新不得重新抢夺焦点。
4. 标题没有 `tabIndex`，也没有任何 ref 驱动的 `focus()`。
5. Tab 点击、方向键切换、选中态和 ARIA 属性由 Ant Design `Tabs` 负责。
6. 切换 Tab 后，焦点保持在 Tab 入口；不自动跳进右侧表单或插件操作按钮。

这样既满足标题不可聚焦，也给键盘用户一个稳定且可操作的设置页入口。

## 加载与错误投影

普通设置与插件清单继续独立加载：

- 每次进入设置页仍立即启动现有 settings load 和 plugin list load。
- `通用`页面只投影普通设置的 loading、error、retry 和 mutation 锁定。
- `插件`页面只投影插件清单的 loading、error、retry、empty 和逐行 operation。
- 普通设置加载期间仍可切换到`插件`，插件清单失败也不禁用`通用`设置。
- 隐藏的 Tab 不清除数据、错误或 pending 状态。
- 全局底部状态栏和固定错误文案保持现有行为。

## 布局与滚动

设置视图继续使用“顶部栏 + 可用内容区”的两行网格。Tabs 根节点占满第二行：

- `.settings-tabs`：`min-width: 0`、`min-height: 0`、`height: 100%`。
- 左侧 Ant Design nav 固定约 `112px`，不随右侧内容滚动。
- 右侧 content holder 使用 `min-width: 0` 和 `min-height: 0`，占据剩余宽度。
- 每个右侧页面使用统一 `.settings-tab-panel`，负责 `overflow-y: auto`。
- `.settings-tab-panel` 复用现有主界面细滚动条变量、6px 宽度、透明轨道、深色 thumb 和 forced-colors 规则。
- Tabs、Tab 入口、右侧内容和所有控件都属于 `app-region: no-drag`，不会触发窗口拖拽。
- 标题栏边框和 Tabs 左右分隔线沿用当前浅色、深色和高对比度边界色。

本需求固定左右结构。当前窗口设计宽度足够容纳 112px 导航与右侧设置内容，不增加响应式折叠逻辑。

## 无障碍

- 页面仍通过 `<section aria-label="设置">` 和 `<h1>` 提供语义结构。
- `Tabs` 输出 `tablist`、`tab`、`tabpanel`、选中态和关联关系。
- 页面只包含`通用`和`插件`两个 Tab，不增加不可见占位入口。
- 每次进入设置页只有选中的`通用`Tab 作为主动焦点目标。
- 标题不在顺序焦点中，也不能被程序聚焦。
- loading 和 error 继续使用现有 Spin、status/alert 语义。
- 右侧滚动容器不吞掉 Tab 的方向键导航。

## 自动测试

前端测试覆盖：

1. 设置标题仍为`设置`，但没有 `tabindex`，进入设置页后 `document.activeElement` 不是标题。
2. 新设置页 epoch 默认选中并聚焦`通用`Tab。
3. tablist 中按顺序且仅存在`通用`和`插件`。
4. 默认只显示快捷键、开机启动、风格和恢复初始化，不显示插件项目内容。
5. 点击`插件`后只显示插件 loading/error/empty/item 页面，不显示普通设置表单。
6. 使用 Ant Design 的键盘导航后，选中态和右侧页面一致。
7. 选择`插件`、关闭设置并以新 epoch 重新进入后恢复`通用`。
8. 切换 Tab 不增加 `loadSettings`、`listPlugins` 或 mutation command 调用。
9. 普通设置加载失败不阻止访问插件页；插件清单失败不把通用设置设为只读。
10. 插件逐行操作 pending 时切回通用，完成后再次进入插件页显示最终状态。
11. CSS 源码约束左侧固定导航、右侧 `minmax`/`min-height: 0` 和独立滚动容器。
12. 右侧面板在浅色、深色和 forced-colors 下复用现有细滚动条样式。

Rust 和后端协议没有行为变化，现有 Rust 测试作为回归验证，不增加新 Rust 单元测试。

## 人工验收

1. 打开设置页，标题“设置”可见，但焦点框出现在左侧`通用`Tab，而不是标题。
2. 左侧只显示`通用`和`插件`，右侧默认显示快捷键、开机启动、风格和恢复初始化。
3. 点击`插件`，右侧切换为插件清单；左侧入口不滚动。
4. 使用方向键在两个 Tab 间切换，焦点、选中态和右侧内容一致。
5. 在`插件`页关闭设置，再次打开后默认回到`通用`。
6. 插件清单很长时只有右侧内容滚动，滚动条样式与主界面一致。
7. 分别验证 Light、Dark 和跟随系统，Tab、分隔线、文本和滚动条均清晰可见。
8. 快捷键、开机启动、风格即时生效、恢复初始化、插件重新加载和删除行为与改版前一致。

## 预计修改范围

- `src/launcher-view.tsx`：替换标题焦点入口，增加受控垂直 Tabs，并拆分两个页面内容。
- `src/styles.css`：增加左右 Tabs、右侧滚动和主题样式。
- `src/launcher.test.tsx`：增加焦点、Tab 生命周期、内容隔离、命令调用和 CSS 回归测试。

不预计修改 `src/launcher-core.ts`、`src/protocol.ts` 或 `src-tauri` 生产代码。
