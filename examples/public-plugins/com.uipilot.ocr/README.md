# ocr

在主窗口面板中预览图片、识别并编辑文字，复制当前编辑后的全文。第一版使用主程序已配置的百度 OCR，布局和交互参考翻译插件。

- 插件：`com.uipilot.ocr`，版本 `1.1.0`，默认命令 `/ocr`。
- 要求：Windows、UiPilot `0.22.0+`；API/schema v1。
- 形态：`submit + panel`，声明 `image.ocr` 输入能力和 `panel.runtimeActions`。
- 权限：`ui.panel`、`clipboard.write`、`network.https`、`credentials.baiduOcr`；网络主机为 `aip.baidubce.com`。
- 安装包：`com.uipilot.ocr-1.1.0.uipilot-plugin`。

## 两种入口

先在主程序设置中配置并启用百度 OCR，并为插件授予上述权限。

**先粘贴图片：** 在主输入框粘贴图片，选择列表中的 `ocr` 后回车。列表按能力匹配，可以同时显示多个 OCR 插件。进入后顶部变为 `ocr` tag，图片移到内容区左侧，右侧自动显示识别结果。

**先打开插件：** 输入 `/ocr`，选择插件后回车，进入空面板；焦点仍在主输入框。在这里粘贴图片后，左侧显示预览，右侧自动识别。继续粘贴可换图，不必退出再进入。

右侧文字可以编辑。点击复制按钮，或在空的主输入框中按一次 Enter，复制当前编辑后的完整文本。文本域内 Enter 正常换行；按住 Enter 不会连续复制。

## 面板操作

识别成功后，Ctrl+K → 分享可把当前编辑全文交给宿主接收者选择器。选择翻译后进入翻译标签并立即翻译；选择前取消保留 OCR，选择后目标加载或翻译失败都在目标侧报错，不返回 OCR。来源无有效文字时禁用分享；翻译目标上限为 8192 UTF-8 字节，超限显示不可选择原因，不截断。

- 左侧清除按钮移除图片和识别文字，仍留在面板中，之后可继续粘贴。
- 右侧下方可选择 OCR 源，目前只有“百度 OCR”；重复选择同一源不会重新识别。
- 有效换图会清空旧文本，并识别新图片。连续换图时只显示最新图片的结果，旧回复不会回填。损坏或超限图片不替换原有效内容。
- 无图、识别中、未识别到文字或识别失败时不能复制。失败后可显式重试，不自动循环调用服务。
- Tab / Shift+Tab 在主输入与内容控件之间移动；Esc 先关闭源菜单，再按面板规则退出。主输入光标位于开头且无选区时，Backspace 可移除 tag 并退出；输入法组词期间保留正常编辑行为。
- 隐藏或退出后再次进入为空面板，不恢复图片或文字。

百度服务使用通用文字识别标准版，识别使用宿主保存的原始标准化图片，左侧有界预览仅用于显示。API Key、Secret Key 和访问令牌由宿主持有；插件不读取系统剪贴板、不另存凭据，也不持久化图片或正文。

识别结果和可复制文本最多为 16 KiB UTF-8；超限时明确提示，不截断后冒充完整结果。图片格式和大小限制见 [SDK 图片能力合同](../../../docs/plugin-sdk/public-plugin-v1.md)。

## 从百度 OCR 旧版升级

`com.uipilot.ocr` 是新的插件身份。先使用 UiPilot `0.22.0+`，再按正常安装流程安装新包并确认权限；主程序中现有的百度 OCR 配置可以继续使用。

旧版 `com.uipilot.baidu-ocr` 的授权、收藏和快捷键不会自动迁移，已安装的旧版也不会自动卸载。若只需新版，可在设置中手动停用旧版，避免图片列表同时出现两个条目。历史旧安装包保留。

## 本地验证

从仓库根目录运行：

```powershell
node --test examples/public-plugins/com.uipilot.ocr/tests/*.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.ocr/tests/sdk-contract.ts
npm.cmd run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.ocr/package --platform windows
```

打包时输出目录须已存在，输出文件名须未占用：

```powershell
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.ocr/package --output artifacts/sharing-delivery/com.uipilot.ocr-1.1.0.uipilot-plugin --platform windows
node packages/plugin-cli/dist/cli.mjs validate artifacts/sharing-delivery/com.uipilot.ocr-1.1.0.uipilot-plugin --platform windows
```

自动测试使用合成图片、假服务和假剪贴板，覆盖 Runtime、面板状态与实际桥接代码。原生验收由用户手动进行：两种入口、真实图片粘贴、百度识别、编辑后复制、快速换图、清除、关闭重开、键盘与焦点，以及 Light/Dark 显示。离线通过不代表这些原生场景已经验收；本任务不使用 Computer Use。
