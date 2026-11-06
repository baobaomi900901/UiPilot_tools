# 单 Panel 文本分享交付记录（2026-11-06）

当前 Host **0.22.0** 代码、Windows、API/schema 1。OCR 编辑全文 → Ctrl+K → 分享 → 用户选择翻译 → 翻译接收原文并立即处理。每次只打开一个 Panel；选择前取消保留来源，选择后失败留在目标，不恢复 OCR、不自动重发。接收插件决定是否处理；独立草稿接收者测试只保存到内存、零业务请求。合同以 [单面板设计](../superpowers/specs/2026-11-06-plugin-sharing-design.md) 和 [API v1](public-plugin-v1.md#host-0220-text-sharing) 为准。

## 安装包

生产 CLI 当前源码通过严格构建守卫后执行 `validate <package> --platform windows`、`pack <package> --output <archive> --platform windows`、`validate <archive> --platform windows`。新输出位于 `artifacts/sharing-delivery/`，历史包保留；本次没有安装或修改用户授权。

| 包 | 精确源文件数 | SHA-256 |
| --- | --- | --- |
| `com.uipilot.ocr-1.1.0.uipilot-plugin` | 7 | `48bf9d56b24dcaf0502c6cdfe42c875eb3ff732585e7d3d00a141d7f8038a7c1` |
| `com.uipilot.translate-1.5.0.uipilot-plugin` | 10 | `1daa6c0f6c7b9998c283d73b8a2ce5c61873cde3446cee5d64adfc309c291ccb` |

归档无重复路径、文件集合和每个文件字节均等于各插件当前 `package/`，Manifest 版本、Host 0.22.0 和 API1 已核对。OCR 声明分享来源；翻译声明来源及 `translate` 接收者，上限 8192 UTF-8 字节。正文保留空格/换行，不截断，不进入 launcher query/history 或通用事件总线。

## 自动验证

所有命令针对实际工作树，包含用户原有 Demo 名称/Window 夹具修改；这些结果不是隔离干净提交的证据。原始 RED、环境失败及最终日志保存在本地 `.superpowers/sdd/sharing/simple-task2-*.log`，不纳入发布包。

- `npm.cmd run verify --workspace @uipilot/plugin-cli`：16 文件、264 项通过；含类型检查、生成文件 freshness、完整运行时守卫及隔离离线安装 smoke。新增回归先复现两个 Ajv runtime helper，再通过等价生成器展开消除；公开 schema 与守卫未削弱。canonical Ajv 对照覆盖 Unicode 80/81 码点、组合字符、尾随 LF/CR/U+2028/U+2029、孤立代理项及精确单元素 MIME 数组。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：2015 通过、0 失败、11 忽略。进程局部 `TAURI_CONFIG={"bundle":{"resources":[]}}`，未改发布配置。初次沙箱执行的管道/TLS 失败在允许本地测试子进程后通过；未改生产网络或管道代码。现有 ScreenshotError dead-code 警告保留。
- 递归发现的 32 个公开插件 Node 测试文件：357 项通过；全部 14 份插件 SDK contracts 及共享分享 contract 严格类型检查通过。OCR 集成夹具已适配私有 Runtime receiver。
- `npm.cmd test`：39 文件、785 项通过、18 跳过；启动测试允许挂载时的只读 `plugin_share_state`，仍禁止失败启动加载设置或调用业务；保留 JSDOM pseudo-element 提示。
- `npm.cmd run build`：通过；保留现有大 chunk 提示。后续只改测试夹具和文档，没有变更前端生产代码。

## 用户手动验收（未执行）

1. 使用含本次实现的 Host 0.22.0 代码，按正常安装流程选择上述两包，确认所需百度/有道、网络和面板授权，配置并启用服务；本次离线测试没有替用户执行这些步骤。
2. 两种 OCR 入口各粘贴真实图片；编辑多行文字与空格，分别从主输入焦点和内容焦点 Ctrl+K 分享，检查筛选、上下键、Enter、Escape；取消只关闭选择器，选中后显示翻译标签、主输入为空，原文逐字一致且只翻译一次。
3. 测试空/识别中/失败来源禁用，超过 8192 字节时翻译不可选、无截断；测试多个接收者与独立草稿接收者，草稿不自动处理；译文可经再次显式选择分享。
4. 模拟目标加载/初始化/接收失败以及有道业务错误：均留在目标，业务失败保留原文，显式重试；不回 OCR、不自动重发。退出/隐藏/新导航后迟到结果不得覆盖新会话。
5. 验证 Light/Dark、原生子 WebView 可见性、键盘/IME/跨 WebView 焦点、真实复制、换图/清图/关闭重开，以及普通翻译与划词不回归。

Node/JSDOM、MockRuntime 和归档验证不证明真实 WebView、供应商、系统剪贴板或原生交互已验收。本次没有 Computer Use、浏览器自动化、真实供应商请求、安装、授权或用户进程操作。
