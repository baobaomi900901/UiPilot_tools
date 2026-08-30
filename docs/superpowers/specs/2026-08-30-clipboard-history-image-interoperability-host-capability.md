# 剪贴板历史图片显示与粘贴互操作：宿主能力请求

## 文档信息

- 日期：2026-08-30
- 状态：宿主修复已实现，等待 Windows 人工验收
- 目标平台：Windows
- 场景：公开 Panel 插件显示图片缩略图，并把图片粘贴到微信等外部应用

## 用户需要

剪贴板历史 Panel 已能收到图片摘要，并能在 Enter 时调用
`clipboardHistory.paste({ id, routeSequence })`。用户需要图片条目显示缩略图，按 Enter 后隐藏
UiPilot、恢复原窗口，并把图片粘贴到微信输入区。

## 当前缺口

### 图片缩略图被内容安全策略阻止

公开 API 将图片摘要定义为 `previewDataUrl: data:image/png;base64,...`，宿主持久化记录中的原图和
缩略图也均可正常解码。但公共插件资源响应当前使用：

```text
img-src 'none'
```

因此 Panel 中合法的 `<img src="data:image/png;base64,...">` 会被宿主 CSP 拒绝。插件无法同时遵守
公开 API 与宿主安全策略。

### 图片剪贴板格式不足

图片恢复当前只向 Windows 剪贴板写入注册格式 `PNG`。该格式能保留编码数据，但不能覆盖只读取
标准位图格式的目标应用；微信人工验收中没有发生图片粘贴。插件只提交 opaque 记录 ID，无法自行访问
原图、转换 DIB 或向系统剪贴板追加格式。

## 建议的宿主行为

### 允许受控图片摘要

- 公共插件内容 CSP 的 `img-src` 至少允许 `data:`，继续拒绝 `http:`、`https:`、`blob:` 和任意外部来源。
- 只接受现有桥已校验的 `data:image/png;base64,...` 图片摘要；不扩大插件的网络、文件或原图访问能力。
- Runtime 的无 UI 文档继续使用原有严格 CSP，不因 Panel 缩略图放宽。
- SDK 文档、宿主响应 CSP 和测试必须对 `previewDataUrl` 的可显示性保持一致。

### 提供 Windows 图片格式互操作

- 在清空系统剪贴板前，先从宿主持久化 PNG 完整准备全部目标格式，避免转换中途留下部分状态。
- 一次图片恢复至少同时发布注册格式 `PNG`、`CF_DIBV5` 和兼容的 `CF_DIB`。
- DIB 数据必须使用正确的 Windows bottom-up/top-down 行约定、DWORD 行对齐、通道布局和 alpha 语义。
- 所有格式仍代表同一条图片记录；监听回流不得生成重复历史。
- 任一必需标准格式无法准备或写入时，返回既有 `ClipboardWriteFailed`，不隐藏 Panel、不发送 Ctrl+V。
- 不向插件暴露原始 PNG、DIB、系统剪贴板句柄或新的通用写入接口。

## 验收检查

### 自动化

- [ ] 公共 Panel 的资源响应 CSP 包含 `img-src data:`，且不允许网络图片来源。
- [ ] 合法 `previewDataUrl` 在 Panel WebView2 中解码，`naturalWidth` 和 `naturalHeight` 均大于 0。
- [ ] 非 PNG Data URL、外部 URL 和 `blob:` URL 仍被桥校验或 CSP 拒绝。
- [ ] 图片恢复在一次 `OpenClipboard`/`EmptyClipboard` 事务中发布 `PNG`、`CF_DIBV5` 和 `CF_DIB`。
- [ ] DIB 转换覆盖横向、纵向、含 alpha、无 alpha和非 4 字节对齐宽度。
- [ ] 任一必需格式准备失败时不清空剪贴板；写入失败返回脱敏的 `ClipboardWriteFailed`。
- [ ] 恢复图片产生的监听回流只移动原记录，不新增副本。

### Windows 人工验收

- [ ] 复制一张图片后打开剪贴板历史 Panel，条目显示可辨认的缩略图而不是破图。
- [ ] 从微信聊天输入区打开 UiPilot，选中图片并按 Enter；UiPilot 隐藏，焦点返回微信，图片出现一次。
- [ ] 同一记录可以粘贴到画图等读取标准 DIB 的应用。
- [ ] 文字和文件粘贴行为不回归。

## 插件边界

该缺口不能通过公共插件 WebView、Tauri 私有对象、Shell、通用剪贴板访问或输入模拟绕过。本请求不授权
当前插件任务修改 `src/`、`src-tauri/`、CLI 或 SDK 合同；宿主修复完成后，再执行上述人工验收。

## 宿主实现记录

- 公共 Panel CSP 已允许 `img-src data:`，继续拒绝网络、文件和 `blob:` 图片来源。
- 图片恢复会在清空系统剪贴板前完成 PNG 解码和 DIB 数据准备，并在同一剪贴板事务中发布 `PNG`、`CF_DIBV5` 和 `CF_DIB`。
- SDK 文档已补充 Panel 图片缩略图可显示性与安全边界说明。
