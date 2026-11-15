---
name: uipilot-public-plugin
description: Create, update, test, package, or diagnose UiPilot public plugins using this repository's SDK and maintained examples. Use for plugin manifests, Runtime actions, Panel/Window UI, permissions, settings, networking, and plugin lifecycle; host feature implementation requires a separate host task.
---

# UiPilot 公开插件开发

这是随 UiPilot 仓库维护的开发 skill，项目 `AGENTS.md` 指向此文件。路径和命令以下均以仓库根目录为基准；入口相对仓库根为 `skills/uipilot-public-plugin/`。它依赖仓库 SDK 与示例，不是可脱离仓库单独复制使用的 SDK。

## 确定实现边界

先读 [AGENTS.md](../../AGENTS.md)、[开发教程](../../docs/plugin-sdk/public-plugin-developer-guide.md) 和 [UI 规范](../../docs/ui-guidelines.md)。使用[插件与开发示例索引](../../docs/plugin-sdk/plugin-examples.md)选最接近的参考包，并阅读该包的 README、Manifest、Runtime 和相关测试。

从用户需求确定输出形态（mainResult / panel / window）、live / submit、目标平台、最低 Host、所需权限和持久化数据。普通实现选择可直接沿用已确认的需求；只澄清会改变插件边界或业务行为的信息。

以下参考目录已加 `demo-` 前缀；目录名不等于插件身份，读取各自 `package/plugin.json` 获取实际 `pluginId`：

| 场景 | 参考目录 | 保留的 pluginId |
| --- | --- | --- |
| Window Runtime actions、文本导入 | [com.uipilot.demo-json-workbench](../../examples/public-plugins/com.uipilot.demo-json-workbench/README.md) | `com.uipilot.json-workbench` |
| Panel Runtime actions、表单与持久化 | [com.uipilot.demo-templates](../../examples/public-plugins/com.uipilot.demo-templates/README.md) | `com.uipilot.templates` |
| Host 认证链路验收 | [com.uipilot.demo-auth-check](../../examples/public-plugins/com.uipilot.demo-auth-check/README.md) | `com.uipilot.auth-check` |

实现保留在插件自身的 `package/`、测试和文档中。宿主 `src/`、`src-tauri/`、`packages/plugin-cli/` 与 SDK 合同保持只读；只有用户另行明确指派宿主开发时才修改它们。

若当前公开 API 无法满足需求，停止依赖该能力的实现，在 `docs/superpowers/specs/YYYY-MM-DD-<short-slug>-host-capability.md` 写清用户需要、插件场景、现有 API 缺口、期望宿主/API 行为和验收点，交给宿主开发任务。设计稿、类型中预留的权限以及 facade 存在都不能作为能力已实现的证据。

## 按需查合同

- Manifest、平台、版本、权限、资源和打包：读 [API v1](../../docs/plugin-sdk/public-plugin-v1.md) 对应章节及 [Schema](../../docs/plugin-sdk/uipilot-plugin-v1.schema.json)。
- Runtime、结果动作、Panel/Window 桥、网络、设置或存储：同时核对 [TypeScript API](../../docs/plugin-sdk/uipilot-plugin-api-v1.d.ts) 的准确签名及合同的生命周期/失败规则。
- UI：以 UI 规范为准。`packages/plugin-ui/` 是可随包分发的组件子集，按需读其 [README](../../packages/plugin-ui/README.md)；它不会由宿主自动安装。旧 templates 的包内 ActionPanel 不替代新 Panel 的宿主 `onActions` 菜单。
- 版本：区分插件版本、最低 Host、API/schema、插件私有数据 schema。使用新增能力时取所需门槛的最高值；保持未改变业务身份的 pluginId / commandId 稳定。

## 实现时保留的约束

1. 从最小示例复制结构，改为插件自己的 ID、命令、名称、图标和测试数据。只带所需权限；移除未使用的通知、测试域名与凭据引用。测试/README/package.json 留在 `package/` 外。
   本仓库的基座参考、配套 Demo 和测试验收插件，显示名称 `plugin.json.name` 以小写 `demo ` 开头；正式业务插件使用业务名称。仅改显示名称时保持已有 ID、目录和启动命令。
2. Runtime 用 ES Module 导出 `onCommand`；结果动作、Panel/Window Runtime actions 按 opt-in 分别导出对应 handler。按稳定 commandId 分派多命令并拒绝未知 ID，原样返回本次 requestId。
3. 每次 invocation/API 和内容桥都绑定请求或会话。丢弃旧会话回复；不要在 onUpdate/ACK 或 Host Key 回调内等待 Runtime request。Busy 不自动排队，变更操作不自动重试。
4. 对持久化记录校验完整格式、数据版本和边界。未知/较新版本阻止写入；按需做 revision 冲突保护。取消、超时或保存失败不能证明未写入，先显式重新读取，再决定重试。
5. 外部请求经 Runtime 的宿主 HTTPS，检查 HTTP 状态和固定错误名。Bearer secret 由宿主保存和注入，不进源码、测试包、日志或 UI。网络与业务服务授权分开检查；自动测试使用假服务。
6. 内容页使用公开桥、宿主主题与状态栏。Host 0.16.0+ Panel 用 onActions 提供菜单；回调复核对象/会话，shortcut 文案不注册按键。键盘兼容 IME、模态、焦点恢复及未保存草稿。
7. 业务服务按 SDK 的最低 Host 和输出形态核对；`selection.read` 当前只有翻译专用入口。OCR 的 `submit + mainResult + image.ocr` 保留 Host 0.20.0 兼容；新图片 Panel 要求 Host 0.21.0、Windows 单命令 submit、ui.panel 与 panel.runtimeActions。参考 [ocr](../../examples/public-plugins/com.uipilot.ocr/README.md)：先注册图片/Ready 接收器，Ready 后用独立 imageId 请求 action，Runtime 用请求绑定的 invocation.image.id 调 api.services.baidu.ocr。一个在途加最新 pending，换图/清图立即使旧正文失效，复制当前编辑全文；预览不是原图，凭据仍由宿主持有。详细桥字段、上限和失败规则见 API v1“Host 0.21.0：图片 OCR Panel”。DeepSeek 尚无公开业务方法；未支持形态按能力缺口处理。

## 验证与交付

按改动运行插件自己的 Node 测试和 SDK 类型检查。假服务测试应覆盖正常业务、权限或能力缺失、坏数据和实际涉及的过期回复/保存不确定性；不调用真实服务来代替离线测试。

在仓库内可使用现有 CLI；若尚无构建或目标版本落后，先从当前源码构建，不修改 CLI 使插件绕过校验：

```powershell
npm.cmd run build --workspace @uipilot/plugin-cli
```

以下 `<plugin-directory>` 替换为实际目录名（不从 pluginId 推导），输出文件名替换为本次归档，测试文件按该插件实际内容选取：

```powershell
node --test examples/public-plugins/<plugin-directory>/tests/*.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/<plugin-directory>/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/<plugin-directory>/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/<plugin-directory>/package --output <existing-output-dir>/<new-package-name>.uipilot-plugin --platform windows
node packages/plugin-cli/dist/cli.mjs validate <existing-output-dir>/<new-package-name>.uipilot-plugin --platform windows
```

输出在源包之外，父目录已存在，文件名未被占用；详细规则见教程打包章节。不要沿用旧 Demo ZIP 脚本代替生产 CLI 校验。仅维护文档时无需生成安装包。涉及示例目录或仓库验证入口时，运行 `node scripts/verify-local-foundation.mjs --list` 检查发现结果；整库发布才按仓库要求运行完整验证。

真实 Host 验收按任务授权执行并如实登记：安装/更新授权、命令、双主题、键盘/焦点、关闭重开、重启数据、撤权、失败恢复，以及本次涉及的系统动作。静态校验不会执行 Runtime；Node/JSDOM 不证明原生交互。尚未执行的检查标为待验收。

交付说明包含改动、插件 ID/版本/最低 Host、开发目录或实际归档路径、所需配置/新增授权、已执行验证和剩余限制。保留用户数据与无关工作区改动，不把测试通过写成真实供应商或宿主已验收。
