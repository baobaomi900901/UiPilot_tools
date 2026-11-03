# 在 Chrome 中调整设置样式

在项目根目录运行，保持这个终端开启：

```powershell
cd D:\code\UiPilot_tools
npm.cmd run preview:settings
```

在 Chrome 打开：

- 深色：http://127.0.0.1:14339/dev/settings-preview.html?theme=dark
- 浅色：http://127.0.0.1:14339/dev/settings-preview.html?theme=light

这是实际的 `LauncherView` 设置界面，使用现有内存模拟数据。通用、消息、插件、密钥配置页签可以点击切换，插件详情也可打开。它不需要启动 Rust 或正式 UiPilot，不写真实应用配置，不进行插件安装或真实服务调用。刷新会重置模拟状态。

## 修改位置

| 文件 | 内容 |
| --- | --- |
| `src/styles.css` | 主要布局、左侧导航、边距、字号、圆角、描边；搜索 `.settings-` / `.public-plugin-` |
| `src/ui-theme.css` | 浅色/深色语义颜色和共享尺寸 |
| `src/ui-theme.ts` | Ant Design 组件主题、按钮/输入框尺寸等；颜色变更应与 CSS token 保持一致 |
| `src/credential-settings.css` | 密钥配置页面专用样式 |
| `src/launcher-view.tsx` | 设置页面结构和文字 |
| `src/launcher-browser-preview.tsx` | 示例插件、示例消息和模拟客户端 |

编辑源文件并保存，Vite 会自动更新 Chrome 中的页面。Chrome 的 F12 → Elements → Styles 可临时试调；开发者工具的临时修改通常会在刷新后丢失，需要手动写回对应源文件。

可以用 F12 的设备工具栏设置为 720×480 或 520×360，比较普通窗口和窄窗口。浏览器缩放建议保持 100%。

## 开发者工具中的行内样式

项目自行编写的固定视觉样式放在 CSS 中，但运行时 DOM 并非没有 `style` 属性。设置页使用的第三方组件仍会写入计算结果：

| DOM 节点 / 属性 | 来源和用途 | 调整外观的位置 |
| --- | --- | --- |
| 滚动容器内部的 `margin-*`、`width: calc(...)`、`padding` | OverlayScrollbars 用于原生滚动条补偿和视口布局 | 内容边距改 `src/styles.css` 的 `.settings-scroll-content`；容器布局改 `.settings-tab-panel` |
| `.os-scrollbar` 的 `--os-*` 变量 | 滚动位置、可见比例和滚动方向 | 滚动条外观使用 `.os-theme-uipilot` 的 CSS 变量 |
| `.ant-tabs-nav-list` 的 `transform`、`.ant-tabs-ink-bar` 的尺寸和位置 | Ant Design 页签导航和指示条的运行时计算 | 修改 `src/styles.css` 中 `.settings-tabs` 下的规则 |

不要通过删除 `style` 属性、修改 `node_modules` 或用固定值覆盖这些计算结果来调整外观；这些值会在组件更新时重写，也可能影响小窗口下的滚动。要让最终 DOM 完全没有行内样式，需要另行替换这些组件的实现，不能仅做 CSS 提取。

## 预览边界

- 版本来自原生应用 API，浏览器中显示「版本 无法读取」属于预期。
- 图钉、全局快捷键录制、开机启动和系统文件选择器不能在该预览中验证。
- 浏览器原生 Ctrl+R/F5 可用于刷新预览；正式应用的刷新键拦截不会注入这个页面。
- 退出服务：回到启动终端按 Ctrl+C。端口已占用时可使用现有服务；不要同时启动第二个相同端口的实例。
