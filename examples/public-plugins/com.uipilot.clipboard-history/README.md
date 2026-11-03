# UiPilot 剪贴板历史插件

Windows 启动器 Panel 插件，查询并复制宿主管理的最近 50 条文本、图片和文件列表记录。单张图片最多 10 MiB，图片总量最多 500 MiB。

## 合同与安装

插件 1.2.1 要求 Host 0.17.0+，使用 submit / panel，hostKeyFocus 为 content。Host Keys 为 ArrowDown、ArrowUp、Tab、Shift+Tab、Delete，不接管输入框 Enter。权限为 ui.panel、clipboard.history.read、clipboard.history.copy。

在「设置 → 插件 → 公开插件」选择本目录 package 开发目录，确认历史读取与复制权限，再运行 /clipboard-history。已有粘贴授权不会自动授予复制权限。

宿主负责采集、查询、持久化和完整内容写入。插件只接收摘要，不会获得完整文本、原图或完整文件路径，不调用浏览器剪贴板或通用输入模拟。

## 查询与复制（Host 0.17.0+）

主输入框输入文字后按 Enter 查询，按全文或文件组内任一文件名匹配；最多 512 个 Unicode 字符。查询与分类取交集，图片仅在空查询显示。ArrowDown 将焦点移至当前记录，记录内上下键移动，Enter 仅复制完整记录，不隐藏窗口或自动粘贴。Ctrl+F 返回搜索框，Tab / Shift+Tab 切换分类。成功或失败显示在宿主状态栏。

Ctrl+K 根据输入框或内容区上下文显示操作；输入框使用宿主的搜索动作，记录区提供复制、上一条、下一条和删除。插件申请 `clipboard.history.read` 和 `clipboard.history.copy`；已有粘贴授权不会自动授予复制权限。预览只模拟查询和复制结果，真实剪贴板和跨 WebView 焦点仍需 Host 人工验收。

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

真实剪贴板、焦点和完整复制按[宿主人工验收清单](../../../docs/superpowers/checklists/2026-08-30-clipboard-history-host-manual-acceptance.md)操作。
