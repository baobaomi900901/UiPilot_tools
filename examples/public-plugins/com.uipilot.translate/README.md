# 翻译 1.4.0

Windows UiPilot **0.19.0+** 启动器 Panel 插件。保留插件 ID `com.uipilot.translate` 和默认启动命令 `translate`，使用宿主管理的有道翻译服务；支持[有道官方文本翻译文档](https://ai.youdao.com/DOCSIRMA/html/trans/api/wbfy/index.html)列出的全部 **112 种语言**（2026-11-03 核对、去重），源语言另有自动检测。语言目录见 `docs/plugin-sdk/youdao-languages.json`，其中 **83 种**标注支持自动识别，其余需手动选择。服务选择器目前只有有道翻译。

## 操作

- `/translate` 打开空面板；`/translate hello` 填入并翻译。宿主顶部输入回车会替换原文并提交，不会增加第二条搜索栏。
- 上方多行原文编辑后防抖 500 ms 翻译，输入法组词期间不请求。普通 Enter 换行，原文框内 Ctrl+Enter 立即翻译。源语言和目标语言支持中文名、英文名、语言代码搜索；源语言列表标注需要手动指定的语言。服务入口也可搜索，方向键选择、Enter 确认、Escape 关闭并恢复焦点。
- 自动检测显示本次识别语言；未知显示“未识别”。交换需要有效译文和明确源语言，以本次译文作为新原文反向翻译。
- 两个文本区均提供复制和朗读。复制保留面板与焦点，通过宿主状态栏反馈；朗读再次点击停止，两侧互斥。没有系统对应语言声音时按钮说明原因；支持翻译不等于系统已经安装朗读声音。简繁中文、塞尔维亚文字变体和粤语不会混用声音。
- 编辑、换方向、重新提交或新划词会立即清除旧译文/识别并停止旧音频；旧响应不覆盖新正文。关闭后重新进入不恢复正文。

仅保存源语言、目标语言和服务偏好，不保存原文、译文或历史。译文只读、可选中，保留换行。原文和译文根据文本自动确定排版方向，支持阿拉伯语等从右向左的语言。

## 配置与升级

有道密钥在宿主设置 → 密钥配置中配置和启用。插件不读取密钥、不直连供应商、不调用浏览器语音或剪贴板历史。需要网络、有道翻译服务授权；复制、朗读和面板分别申报 `clipboard.write`、`speech.speak`、`ui.panel`。旧有百度 OCR 和 DeepSeek 权限声明保持不变，它们不是已实现的翻译服务。申报不代表授予，本次不会自动改变用户授权。

划词使用宿主管理的 `selection.read`：用户主动开启后，在外部应用选中文字并使用既有划词快捷键（默认 Alt+A）。宿主捕获选区并填入 Panel；捕获失败不沿用旧正文或剪贴板。具体原生捕获与权限行为须在 Host 中验收。

从 1.2.1 或 1.3.0 更新时在设置中选择此插件 `package/` 开发目录或版本化 `.uipilot-plugin` 归档。保留插件 ID、默认命令与既有用户命令改名、收藏和快捷键绑定的身份；新增权限仍需用户自行授权。**低于 0.19.0 的 Host 不兼容 1.4.0，应保留旧版并先升级宿主。** 1.2.1 为旧 command/mainResult 版本，不能把它的旧调用边界当作本版 Panel 能力。安装、旧 Host 拒绝和升级取消保留旧插件尚待原生验收，未由本次离线测试证明。

失败显示安全错误提示，无任意供应商异常文本。若有道拒绝所选语言方向，会提示重新选择源语言或目标语言；不自动改用其他方向或反复请求。网络或服务撤权后不会显示成功或保留旧译文；检查网络、授权、密钥或额度后，手动提交或修改输入重试，不自动反复调用。输入最多 8192 UTF-8 字节。

## 离线验证与交付

从仓库根目录运行：

```powershell
node --test examples/public-plugins/com.uipilot.translate/tests/*.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict --target ES2021 --module ESNext --moduleResolution Bundler examples/public-plugins/com.uipilot.translate/tests/sdk-contract.ts
npm.cmd run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.translate/package --platform windows
```

`package.test.js` 检查发布合同、包内入口/资产/模块依赖、缺失入口拒绝及语言/服务一致性。`panel-host-contract.test.js` 使用实际 Runtime、controller、model、provider 和 capability 模块，从 onCommand → onUpdate/onReady → request → onPanelAction → DOM 完成闭环；边界采用假有道、假剪贴板和假语音，还覆盖撤权、空划词输入、旧请求和音频事件隔离。这是离线自动化，不是实际供应商或原生 Host 验收。双主题、键盘与长文本的离线浏览器证据见工作记录；强制颜色仅有 CSS/测试覆盖，未模拟原生强制颜色环境。这些检查不能代替原生 WebView 验收。

本次交付归档为 `artifacts/com.uipilot.translate-1.4.0-windows-multilang.uipilot-plugin`，通过本地 CLI（目标 Host 0.19.0）从源包严格校验、打包并再次校验；不自动安装，不覆盖旧包。

待用户在真实 Host 主动验收：安装/升级取消保留旧版与旧 Host 拒绝、命令改名/收藏/绑定保留、真实有道请求与撤权、外部选区捕获及失败恢复、系统复制和 SAPI 朗读/声音缺失、隐藏再进入、新划词取代正在翻译/朗读的内容、原生 WebView 双主题/强制颜色/键盘/长文本。当前测试没有运行真实网络、剪贴板、音频、选区或权限变更。

多语言验收：日语→简体中文、英语→法语、自动识别韩语→英语、手选粤语→简体中文、阿拉伯语文本排版，以及新语种的检测后交换/复制/有声音与无声音朗读行为。以上真实供应商及系统行为仍需原生验收，离线测试不代表全部语言方向已经实测。
