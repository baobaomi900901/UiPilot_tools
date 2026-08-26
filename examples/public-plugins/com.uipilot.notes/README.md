# UiPilot Notes Plugin

启动器内嵌 Panel 笔记插件：列表、新建、编辑、复制和删除笔记；搜索使用主窗口带 tag 的参数输入框。

## 选型

- `activationMode`: `submit`
- `outputMode`: `panel`
- 权限: `ui.panel`
- `version`: `1.1.0`
- `minimumHostVersion`: `0.3.1`
- `panel.hostKeys`: `ArrowDown`, `ArrowUp`, `Primary+N`
- 业务参考: `com.uipilot.note`
- Panel 合同参考: `com.uipilot.demo-panel`

## 安装与使用

1. 在 UiPilot 的公开插件面板选择 **开发目录**。
2. 选择本目录下的 `package` 文件夹。
3. 确认 `ui.panel` 权限。
4. 在主界面输入 `/notes` 并回车。

### 交互说明

- 左侧：新建按钮与笔记列表（含删除）；右侧：正文编辑、复制与保存。
- **不要**在 Panel 内搜索。在 tag 后的主输入框输入关键词并按 Enter 过滤标题/正文（不区分大小写；空串显示全部）。
- `/notes hello` 首次打开即以 `hello` 过滤。
- 主输入框聚焦时：**↑/↓** 切换可见笔记，**Ctrl+N**（Windows）打开新建弹窗。
- Panel 内按 **Ctrl+F** 将焦点交还给主输入框（不关闭 Panel、不删 tag、不提交）。
- Panel 内容区保留本地方向键、左右键、复制、保存与列表导航。
- **Escape**：无弹窗且无未保存内容时由 Host 隐藏；有弹窗则取消弹窗；有未保存内容则先确认，取消保持可见，保存或放弃后隐藏。
- 新建需输入目录名；删除与未保存切换有确认。
- 数据保存在本插件私有 storage（`notes.entries`），与窗口版 `note` 相互独立。

## 验证

```powershell
node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.notes/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.notes/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.notes/package --platform windows
```

完整合同见 `docs/plugin-sdk/public-plugin-v1.md`。
