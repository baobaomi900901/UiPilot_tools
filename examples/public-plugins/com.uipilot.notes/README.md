# UiPilot Notes Plugin

启动器内嵌 Panel 笔记插件：列表、新建、编辑、复制和目录管理；搜索使用主窗口带 tag 的参数输入框。

## 选型

- `activationMode`: `submit`
- `outputMode`: `panel`
- 权限: `ui.panel`
- `version`: `1.2.2`
- `minimumHostVersion`: `0.3.1`
- `panel.hostKeys`: `ArrowDown`, `ArrowUp`, `Primary+N`
- 业务参考: `com.uipilot.note`
- Panel 合同参考: `com.uipilot.demo-panel`

## 安装与使用

1. 在 UiPilot 的公开插件面板选择 **开发目录**。
2. 选择本目录下的 `package` 文件夹。
3. 确认 `ui.panel` 权限。
4. 在主界面输入 `/notes` 并回车。

开发目录安装会把 `package` **复制**到本地插件库；修改 Panel 资源后需在设置里**卸载并重新选择 `package` 目录安装**，确认版本显示为 **1.2.2** 后再测。

### 浏览器预览

运行 UiPilot 开发服务器后，可直接打开：

`http://127.0.0.1:14321/examples/public-plugins/com.uipilot.notes/preview.html`

预览页复用正式 Panel 的 HTML、CSS 和 JavaScript，并在浏览器中模拟 Host bridge、深色主题与示例笔记。预览数据保存在浏览器 `localStorage`；使用 `?theme=light` 可切换浅色主题。预览文件位于 `package` 外，不会进入插件安装包。

### 交互说明

- 左侧：`目录` 标题、右侧白色居中的加号新建按钮与笔记列表；右侧：正文编辑器与右下角复制图标。
- 每个目录项的三点菜单依次提供重命名、置顶和删除；最近置顶的目录排在最前，删除仍需二次确认。
- **不要**在 Panel 内搜索。在 tag 后的主输入框输入关键词并按 Enter 过滤标题/正文（不区分大小写；空串显示全部）。
- `/notes hello` 首次打开即以 `hello` 过滤。
- 主输入框聚焦时：**↑/↓** 切换可见笔记，**Ctrl+N**（Windows）打开新建弹窗。
- Panel 内按 **Ctrl+F** 将焦点交还给主输入框（不关闭 Panel、不删 tag、不提交）。
- Panel 内容区保留本地方向键、左右键、复制、保存与列表导航。
- 列表聚焦时按 **→** 进入右侧编辑器；编辑器内按 **Ctrl+S** 保存成功后，焦点回到原选中的列表项。
- 编辑器不显示文字复制/保存按钮；复制使用文本域右下角图标，保存使用 **Ctrl+S**。
- 左侧目录列表和无边框编辑器均使用无系统箭头、支持轨道点击和滑块拖动的虚拟滚动条；编辑器占满右侧可用区域，状态气泡显示在文本域顶部中央，成功使用绿色，失败使用危险色，且不抢焦点。
- 列表聚焦时按 **Enter**：复制当前笔记正文，成功后调用 `requestHide()` 隐藏启动器（需 Host `c2ff520`+ 且 commit 路径正常；见下方故障排查）。
- **Escape**：无弹窗且无未保存内容时由 Host 隐藏；有弹窗则取消弹窗；有未保存内容则先确认，取消保持可见，保存或放弃后隐藏。
- 新建和重命名需输入目录名；删除与未保存切换有确认。
- 新建弹窗只保留标题输入框；Tab 顺序固定为输入框、保存、取消。
- 数据保存在本插件私有 storage（`notes.entries`），与窗口版 `note` 相互独立。

### 故障排查（复制成功但不隐藏）

| 状态栏 | 含义 |
|--------|------|
| **已复制**（无其它提示） | 复制成功；`requestHide()` Promise 已 resolve，但 **Host 未执行 hide/commit**（插件无法修复） |
| **复制成功，但无法隐藏窗口** | Host admit 阶段失败（旧 Host 缺 `hideTicketId` 修复） |

请先 **关掉 notes tag 重新打开**（清除可能卡住的 hide ticket），再 **鼠标点击列表** 后 Enter。若仍只显示「已复制」，需 Host 侧修复 commit 路径，见 `docs/superpowers/specs/2026-08-26-panel-request-hide-commit-host-fix-request.md`。

## 验证

```powershell
node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.notes/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.notes/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.notes/package --platform windows
```

完整合同见 `docs/plugin-sdk/public-plugin-v1.md`。
