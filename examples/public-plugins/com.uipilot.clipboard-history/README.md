# UiPilot 剪贴板历史插件

Windows 启动器 Panel 插件，展示 UiPilot 运行期间由宿主管理的最近 20 条文本、图片和文件列表剪贴板记录，并将选中项一次性粘贴回打开 UiPilot 前的应用窗口。

## 合同

- `activationMode`: `submit`
- `outputMode`: `panel`
- `minimumHostVersion`: `0.3.4`
- `hostKeyFocus`: `host`（Host Key 投递时保持主输入框焦点）
- 权限：`ui.panel`、`clipboard.history.read`、`clipboard.history.paste`
- Host Keys：`ArrowDown`、`ArrowUp`、`Tab`、`Shift+Tab`、`Enter`

宿主负责采集、持久化、原始内容、目标窗口恢复和一次性粘贴。插件只接收展示摘要，不会获得完整文本、原图或完整文件路径，也不会调用浏览器剪贴板、网络、Shell 或通用输入模拟。

## 安装

1. 启动 Windows 版 UiPilot `0.3.4+`。
2. 打开 **设置 > 插件 > 公开插件**，选择“开发目录”。
3. 选择本目录下的 `package` 文件夹。
4. 确认剪贴板历史读取与粘贴权限。
5. 在主界面输入 `/clipboard-history` 并回车。

## 交互

- `Tab` / `Shift+Tab`：循环切换全部、图片、文件、文字。
- `ArrowUp` / `ArrowDown`：在当前列表中移动选择，到首尾后停止。
- `Enter`：恢复选中记录，隐藏 UiPilot，返回原窗口并粘贴一次。
- Host Key 不把原生焦点交给 Panel；连续操作时主输入框光标保持可见。
- 鼠标可选择、删除单条记录或清空历史；操作后焦点返回宿主输入框。
- 已移动或删除的文件会显示为不可用，不执行粘贴。

## 浏览器预览

启动仓库 Vite 服务：

```powershell
npm.cmd run dev:vite -- --host 127.0.0.1 --port 14321
```

打开：

- 深色：`http://127.0.0.1:14321/examples/public-plugins/com.uipilot.clipboard-history/preview.html`
- 浅色：`http://127.0.0.1:14321/examples/public-plugins/com.uipilot.clipboard-history/preview.html?theme=light`

预览复用正式 Panel HTML、CSS 和 JavaScript，只模拟宿主摘要与 Host Key，不读取或写入系统剪贴板。

## 验证

```powershell
node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/*.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.clipboard-history/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.clipboard-history/package --platform windows
```

安装独立 CLI 后，在插件目录中可使用等价命令：`uipilot-plugin validate .\package --platform windows`。

真实剪贴板、焦点恢复和微信粘贴按[宿主人工验收清单](../../../docs/superpowers/checklists/2026-08-30-clipboard-history-host-manual-acceptance.md)操作。
