# UI / UX 统一验收（2026-10-28）

本轮范围：主启动器、设置、`/find`，以及公开插件 `mainResult`、`panel`、`window` 三种输出。规范见 [UI 规范](ui-guidelines.md)。开发和浏览器检查完成不代表用户已验收；下表的原生应用用例等待人工反馈。

## 如何查看这次修改

宿主代码位于 `D:\code\UiPilot_tools`。退出旧的 UiPilot 后运行：

```powershell
cd D:\code\UiPilot_tools
npm.cmd run tauri -- dev
```

本次没有生成 Setup 安装程序，版本仍为 0.14.0。已安装插件的文件是独立副本，需要更新才能看到新样式。使用设置 → 插件 → **选择插件包**（上传图标），打开：

```text
D:\code\UiPilot_tools\artifacts\packages\plugins
```

| 插件 | 本轮本地包 |
| --- | --- |
| Notes 内嵌笔记 | `com.uipilot.notes-1.2.5.uipilot-plugin` |
| 剪贴板历史 | `com.uipilot.clipboard-history-1.0.9.uipilot-plugin` |
| 文本模板 | `com.uipilot.templates-1.2.0.uipilot-plugin` |
| JSON 工作台 | `com.uipilot.json-workbench-1.1.1.uipilot-plugin` |
| Note 独立笔记窗口 | `com.uipilot.note-1.1.5.uipilot-plugin` |
| 番茄钟 | `com.uipilot.pomodoro-1.2.0.uipilot-plugin` |
| Panel 示例 | `com.uipilot.demo-panel-1.0.5.uipilot-plugin` |
| Window 示例 | `com.uipilot.demo-win-1.0.4.uipilot-plugin` |

均为相同版本的样式修订，安装时确认替换；没有修改数据格式或增加权限。开发目录方式也可以：在插件详情选择「重新载入开发目录」，选择对应 `examples\public-plugins\com.uipilot.<名称>\package` 文件夹。不要在文件夹选择器里选择 `.uipilot-plugin` 文件。

mainResult 的外观由宿主绘制，现有 `/text-snippet`、`/latest-snippet` 或 demo-multi 不必更新包。

## 第一轮：约 10 分钟的人工验收

请先在深色主题完成 1–6，再切浅色快速检查外观、按钮和模态。

| 编号 | 操作 | 预期 |
| --- | --- | --- |
| 1 主界面 | 查询应用、用上下键切换结果；进入主列表型插件查看结果并按 Ctrl+K | 字体与结果密度一致；标题 14px、说明 12px；选择态清楚，长文字不越界；动作菜单可正常关闭 |
| 2 设置 | 切换通用、消息、插件；打开插件详情、滚动到底部 | 左栏文字起点一致；标题、字段、按钮间距一致；无被遮住的控件；checked 开关为绿色；滚轮和滚动条可用 |
| 3 文件搜索 | 输入有结果的查询，切分类，关闭/开启预览；使用长文件名/路径 | 搜索框与主界面同高；分类固定在上面，列表在左、预览在右、统计与开关在底；关闭预览后列表铺满；路径单行截断，不撑高行 |
| 4 内嵌 Panel | 打开 `/notes`，选择已有笔记；再打开 templates 或剪贴板历史 | 命令 tag 与主列表型插件相同；顶部没有重复输入栏；侧栏与详情各自滚动；按钮、标题、输入框、选中态使用同一视觉层级 |
| 5 独立 Window | 打开 JSON 工作台与番茄钟；切换浅色/深色后重新进入 | 标题栏、图钉和关闭按钮一致；内容与宿主同主题；番茄钟暗色主按钮为浅底深字，不出现白底白字；倒计时大字号保留 |
| 6 模态 | 用测试笔记打开新建/删除/未保存确认，或在模板/JSON中触发替换确认；分别取消、点关闭、按 Escape | 背景不可操作，标题与动作清晰；取消/关闭不丢当前输入；确认才执行对应动作；焦点可回到原界面 |
| 7 键盘 | 从主界面、设置左栏空白、`/find`、插件独立窗口内容区按 Ctrl+T | 图钉一次切换；快捷键录制时 Ctrl+T 仍可录制；长按不会反复切换 |
| 8 分类与刷新 | `/find` Tab / Shift+Tab 切分类，再 Enter / Escape；各形态按 Ctrl+R/F5 | 分类无额外外框；Enter 执行所选文件，Escape遵循原有固定规则；刷新键不导致卡住、丢 tag 或重新加载。业务刷新按钮仍可使用 |

涉及文件打开、删除或替换时只使用测试文件/测试笔记。不要用现有重要内容做破坏性确认。

## 可重复的浏览器预览

启动 `npm.cmd run dev:vite -- --host 127.0.0.1`，使用终端实际显示的端口。以下 URL 的 `theme` 可设为 `light` 或 `dark`：

| 形态 | 路径 |
| --- | --- |
| 主界面 | `/dev/main-preview.html?theme=dark` |
| 设置 | `/dev/main-preview.html?mode=settings&theme=dark` |
| 文件搜索 | `/dev/find-preview.html?theme=dark` |
| mainResult | `/dev/main-preview.html?mode=main-result&theme=dark` |
| Panel | `/dev/main-preview.html?mode=panel&command=notes&theme=dark` |
| 独立窗口 | `/dev/plugin-window-preview.html?theme=dark` |

mainResult 支持 `scenario=loading|empty|error` 和 `actionScenario=pending|error`。独立窗口预览使用真实 JSON 界面与 Runtime，数据只在内存中，导入是固定示例。预览不是已安装应用，不测试真实图钉、系统文件选择器或 WebView2 刷新拦截；设置版本可能显示「无法读取」。

## 本轮验证记录

- 宿主六类界面：Light/Dark、720×480 与 520×360，共 24 组真实组件截图，检查字体、区域尺寸、截断与页面错误。
- 实测主搜索 44px、外层 60px、底栏 36px；设置侧栏 144px、标题栏 44px；宿主和插件采用同一字体回退栈。
- 8 个可视插件：Light/Dark、760×520 与 520×316。Notes/剪贴板使用现有假 Host 预览；其他插件补静态样式检查，JSON另有真实 Runtime 预览。
- 主界面/设置 374 项通过、18 项既有跳过；宿主主题/Find/独立窗口/刷新拦截 74 项通过；原生窗口 27 项通过；插件与共享 UI 回归通过，7 个笔记对话框关闭行为另经针对性测试。
- 前端生产构建、共享 UI 同步检查、插件目录及生成归档校验通过。
- 独立代码复核提出的浅色危险文字对比不足已修正；页面/对话框分别达到 5.25:1 / 5.62:1，并加入主题回归检查。
- **待人工验证：** 上表原生应用交互、Windows 缩放下的观感与焦点。未把浏览器预览作为人工验收通过记录。
