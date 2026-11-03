# UiPilot 第三方插件开发教程

本教程面向第一次为 UiPilot 编写公开插件的开发者。示例使用原生 JavaScript，不需要前端框架或构建工具，并覆盖三种 MVP 插件：

- **主界面结果型**：处理命令后在 UiPilot 主界面显示结果，用户再次按 Enter 执行复制。
- **单例子窗口型**：处理命令后打开一个由 UiPilot 托管的子窗口。
- **启动器面板型**：处理命令后在主启动器内挂载插件面板（基础能力需宿主 `0.3.0+`，Host 按键与主动隐藏需 `0.3.1+`，扩展 `Tab` / `Shift+Tab` / `Enter` 与剪贴板历史需 `0.3.3+`，显式 Host Key 焦点策略需 `0.3.4+`）。

三种插件的 Runtime 都可以按需使用 Windows Host `0.3.2+` 提供的受控 HTTPS 能力。网络请求由宿主代理执行，不会向 Runtime 或内容 WebView 开放浏览器 `fetch`。

仓库中的完整参考实现：

- [`com.uipilot.demo-return`](../../examples/public-plugins/com.uipilot.demo-return)：旧形态单命令主界面结果与复制。
- [`com.uipilot.demo-multi`](../../examples/public-plugins/com.uipilot.demo-multi)：Host `0.5.0+` 的双 mainResult 命令、一个 Runtime 与共享设置/存储；完整可安装目录为 package/。
- [`com.uipilot.demo-win`](../../examples/public-plugins/com.uipilot.demo-win)：单例子窗口。
- [`com.uipilot.demo-panel`](../../examples/public-plugins/com.uipilot.demo-panel)：启动器内嵌面板。
- [`com.uipilot.demo-credentials`](../../examples/public-plugins/com.uipilot.demo-credentials)：Windows Host `0.4.0+` 的 bearer 凭据引用与离线假服务测试。
- [`com.uipilot.demo-http`](../../examples/public-plugins/com.uipilot.demo-http)：Host 托管 HTTPS 请求。
- [`com.uipilot.pomodoro`](../../examples/public-plugins/com.uipilot.pomodoro)：窗口隐藏后仍由宿主计时的番茄钟。

完整字段、上限和安全合同见 [`Public Plugin API v1`](./public-plugin-v1.md)、[`plugin.json` Schema](./uipilot-plugin-v1.schema.json) 和 [TypeScript API 类型](./uipilot-plugin-api-v1.d.ts)。

主结果列表的底栏由宿主管理：右下固定“操作 Ctrl+K”，左侧显示结果数量和状态反馈。操作列表与快捷键收进动态面板；结果 `subtitle` 仅描述内容或状态。插件无需自行绘制底栏。

用户可在「设置 → 权限」按能力分类调整插件授权（包括网络），因此插件安装成功不代表权限始终有效。调用能力时应处理权限不足；关闭权限会使相关旧请求或会话失效，重新开启后需要发起新的操作。普通重载、更新和重启会保留仍然申报的权限的关闭状态；新增权限仍需安装确认。撤销权限不会删除插件数据或已有剪贴板历史。

## 1. 选择插件类型

| 需求 | `activationMode` | `outputMode` | 权限 | 参考 Demo |
| --- | --- | --- | --- | --- |
| 按 Enter 后在主界面显示结果 | `submit` | `mainResult` | 复制时使用 `clipboard.write` | `demo-return` |
| 按 Enter 后打开子窗口并延迟发布消息 | `submit` | `window` | `ui.window`、`notifications.publish`（仅 Windows） | `demo-win` |
| 子窗口控制宿主持有的单计时器 | `submit` | `window` | `ui.window`、`notifications.publish`、`timer.control`（仅 Windows） | `pomodoro` |
| 在启动器内挂载面板并提交参数 | `submit` | `panel` | `ui.panel`（使用基础 `hostKeys` 时 `minimumHostVersion` ≥ `0.3.1`；使用 `Tab` / `Shift+Tab` / `Enter` 或剪贴板历史时 ≥ `0.3.3`） | `demo-panel` |
| 在面板中显示宿主管理的剪贴板历史摘要并粘贴一次 | `submit` | `panel` | `ui.panel`、`clipboard.history.read`、可选 `clipboard.history.paste`（仅 Windows，`minimumHostVersion` ≥ `0.3.3`） | 暂无产品 Demo |
| 在 Runtime 中请求声明的 HTTPS 服务 | 任意现有模式 | 任意现有输出 | `network.https`（仅 Windows，`minimumHostVersion` ≥ `0.3.2`） | `demo-http` |
| 输入时立即计算并预览 | `live` | `mainResult` | 按结果动作决定 | `demo-multi`（Host `0.5.0+`） |
| 一个包提供多条独立搜索、改名和收藏的主结果命令 | 每项 `live` / `submit` | 每项 `mainResult` | 包级共享 | `demo-multi`（Host `0.5.0+`） |

旧 `command` 形态注册一个启动键；Host `0.5.0+` 的 `commands` 形态注册 1–16 条 mainResult 命令。每项 `defaultName` 是初始启动键，用户可分别修改并收藏。身份是稳定的 `(pluginId, commandId)`，Runtime 按 ID 分派，不硬编码 `/命令名`，也不按数组位置识别命令。两种形态仍共用一个 Runtime、设置、存储、权限和包级启停。

## 2. 准备目录

建议把开发文件和实际安装包分开：

```text
my-plugin/
  README.md
  package.json             # 仅供本地 Node.js 测试使用
  tests/
    runtime.test.js
  package/
    plugin.json
    icon.png               # 可选，但推荐
    dist/
      runtime.js
```

子窗口型插件还需要：

```text
package/
  dist/
    window.html
    window.js
    window.css
```

UiPilot 安装时选择的是 `package` 目录，而不是它的父目录。

包内只允许 `plugin.json`、`.js`、`.html`、`.css`，以及包根目录唯一的 `icon.png`。不要放入 README、测试、源码映射、依赖目录或其他图片。

开发根目录的 `package.json` 不会被安装，只用于让 Node.js 按 ESM 加载 Runtime 及其相对导入：

```json
{
  "private": true,
  "type": "module"
}
```

## 3. 编写 `plugin.json`

三个容易混淆的文案字段用途不同：

| 字段 | 显示位置 | 示例 |
| --- | --- | --- |
| `description` | 设置页插件介绍 | `将输入转换为可复制文本。` |
| `command.summary` | 输入 `/d` 时的插件匹配提示 | `生成一条文本` |
| `command.inputPlaceholder` | 命令进入 tag / 参数输入后的用法提示 | `请输入内容后回车` |

公共规则：

- `pluginId` 是永久身份，使用自己的反向域名，例如 `com.example.hello-return`。
- `version` 和 `minimumHostVersion` 必须是 `major.minor.patch`。
- 旧 `command.defaultName` 不包含 `/`；新 `commands` 每项也使用相同名称规则。`command` / `commands` 必须二选一，显式 null 无效。
- `runtime.entry` 必须指向包内 JavaScript 文件。
- 当前真正可用的权限只有 `clipboard.write`、`ui.window`、`ui.panel`，以及仅 Windows 可用的 `network.https`、`notifications.publish`、`timer.control`、`clipboard.history.read`、`clipboard.history.paste`、`url.open` 、`files.openText` 和 Host 0.15.0+ 的 `credentials.youdaoTranslation`、`credentials.baiduOcr`、`credentials.deepseek`，以及 Host 0.16.0+ 的 `selection.read`（用户主动触发划词翻译时获取选区；宿主可临时复制并有条件恢复剪贴板）。`clipboard.read` 仍是保留权限，不等同于剪贴板历史。
- `settings` 是可选字段；教程显式保留 `"settings": []` 只是为了让 Manifest 结构更直观。

### 分支 A：主界面结果型 Manifest

创建 `package/plugin.json`：

```json
{
  "schemaVersion": 1,
  "pluginId": "com.example.hello-return",
  "version": "1.0.0",
  "apiVersion": 1,
  "minimumHostVersion": "0.2.0",
  "name": "Hello Return",
  "description": "将输入和日期返回到 UiPilot 主界面。",
  "supportedPlatforms": ["windows", "macos"],
  "command": {
    "defaultName": "hello-return",
    "summary": "生成一条可复制文本",
    "activationMode": "submit",
    "outputMode": "mainResult",
    "inputRequired": true,
    "inputPlaceholder": "请输入内容后回车"
  },
  "runtime": {
    "entry": "dist/runtime.js"
  },
  "permissions": ["clipboard.write"],
  "settings": []
}
```

`mainResult` 不能声明 `window` 或 `ui.window`。

### 分支 A2：多条主界面结果命令（Host 0.5.0+）

把单个 command 换成 commands，minimumHostVersion 至少为 0.5.0（单项数组也一样）：

```json
{
  "commands": [
    { "id": "upper", "defaultName": "demo-upper", "activationMode": "live", "outputMode": "mainResult", "inputRequired": true },
    { "id": "lower", "defaultName": "demo-lower", "activationMode": "submit", "outputMode": "mainResult", "inputRequired": true }
  ]
}
```

这是字段片段；完整离线包见 [demo-multi](../../examples/public-plugins/com.uipilot.demo-multi/README.md)。数组限 1–16 项，每项必填唯一 id，匹配 `^[a-z][a-z0-9-]{0,31}$`；defaultName 也须包内唯一。合法 ID 包含 constructor，勿用继承属性进行分派。每项只支持 mainResult，不能声明 window/panel 及 ui.window、ui.panel、timer.control、clipboard.history.* 权限。任一项无效则整包拒绝，不能忽略或退回第一项。

Manifest schemaVersion、Runtime apiVersion 仍为 1；内部 state schema 为 2，inventory 和 CLI report schema 仍为 1。插件 version、Host 版本、CLI npm 包版本（当前 0.1.0）各自独立。current Host 和 CLI Host target 已同步为 0.17.0，可验证合法 commands 包（含单项数组）；本地发布准备的最终独立评审与真实 Windows Host 人工验收单独登记。旧示例的最低版本及凭据功能的 0.4.0 门槛不变。

### 分支 B：子窗口型 Manifest

创建 `package/plugin.json`：

```json
{
  "schemaVersion": 1,
  "pluginId": "com.example.hello-window",
  "version": "1.0.0",
  "apiVersion": 1,
  "minimumHostVersion": "0.2.0",
  "name": "Hello Window",
  "description": "在 UiPilot 单例子窗口中显示输入信息。",
  "supportedPlatforms": ["windows", "macos"],
  "command": {
    "defaultName": "hello-window",
    "summary": "打开信息子窗口",
    "activationMode": "submit",
    "outputMode": "window",
    "inputRequired": true,
    "inputPlaceholder": "请输入内容后回车"
  },
  "runtime": {
    "entry": "dist/runtime.js"
  },
  "window": {
    "entry": "dist/window.html"
  },
  "permissions": ["ui.window"],
  "settings": []
}
```

`window` 输出必须使用 `submit`，并同时声明窗口入口和 `ui.window`。

### 分支 C：启动器面板型 Manifest

面板基础模式要求 UiPilot `0.3.0+`；下面示例声明基础 Host 按键，因此要求 `0.3.1+`。如果声明 `Tab`、`Shift+Tab`、`Enter` 或剪贴板历史权限，要求 `0.3.3+`；显式声明 `hostKeyFocus` 要求 `0.3.4+`。面板仅支持 Windows、`submit` 激活和独立的 `panel.entry`：

```json
{
  "schemaVersion": 1,
  "pluginId": "com.example.hello-panel",
  "version": "1.0.0",
  "apiVersion": 1,
  "minimumHostVersion": "0.3.1",
  "name": "Hello Panel",
  "supportedPlatforms": ["windows"],
  "command": {
    "defaultName": "hello-panel",
    "summary": "在启动器内打开面板",
    "activationMode": "submit",
    "outputMode": "panel",
    "inputRequired": false
  },
  "runtime": { "entry": "dist/runtime.js" },
  "panel": {
    "entry": "dist/panel.html",
    "hostKeys": ["ArrowDown", "ArrowUp", "Primary+N"]
  },
  "permissions": ["ui.panel"],
  "settings": []
}
```

`panel` 不能与 `window`、`ui.window`、`timer.control` 或 `live` 组合。面板内容由宿主放在独立 child WebView 中，不会注入主界面 DOM。

### 通用能力：声明 HTTPS 目标

需要调用外部服务时，插件必须面向 Windows Host `0.3.2+`，同时声明 `network.https` 和完整的精确目标主机：

```json
{
  "minimumHostVersion": "0.3.2",
  "supportedPlatforms": ["windows"],
  "permissions": ["clipboard.write", "network.https"],
  "network": {
    "httpsHosts": ["api.example.com"]
  }
}
```

`httpsHosts` 只能包含 1-8 个唯一、全小写 ASCII DNS 主机名。它不是 URL：不能填写协议、端口、路径、通配符、IP、`localhost`、`.local`、Unicode/IDN。Manifest 有 `network` 时必须有 `network.https`，反之亦然；`network: null` 也无效。Schema、CLI 和 Host 都会执行同一套校验。

安装时 UiPilot 会显示完整排序后的域名并单独请求网络授权。更新新增域名时必须重新确认；仅移除域名会直接收窄权限。用户可以在设置中立即撤销网络访问，重新开启时必须再次确认当前完整域名列表。授权是全部域名的整体授权，不支持只批准其中一部分。

## 4. 理解 Runtime 调用

`runtime.js` 是 ES Module，必须导出异步函数 `onCommand`：

```js
export async function onCommand(invocation, api) {
  // 返回 PluginResponse
}
```

一次调用中常用的输入：

```js
invocation.requestId       // 本次请求 ID，响应必须原样返回
invocation.input           // 去掉首尾空白，保留内部空格
invocation.context.platform // 'windows' 或 'macos'
invocation.context.theme    // 'dark' 或 'light'
invocation.context.invokedAt // 带本地时区偏移的 RFC 3339 时间
```

新 commands 包的每次 invocation 必有准确的 commandId；旧 command 包完全省略该 key，不发送 null 或内部 default。SDK 的 commandId?: string 同时兼容两种 wire。一个 onCommand 内按 ID 分派，例如：

```js
switch (invocation.commandId) {
  case 'upper': return uppercaseResult(invocation)
  case 'lower': return lowercaseResult(invocation)
  default: throw new Error('Unsupported command ID')
}
```

示例结果函数由插件实现；不可因缺失或未知 ID 执行第一条命令。响应仍只使用 requestId，不自行返回 commandId 改变归属。Host 保存候选 ID 和输入，切换同包命令后旧响应不会覆盖新结果。

`invocation` 和 `api` 都由宿主冻结，并且只在当前请求生命周期内有效。不要保存它们供定时器或后续请求使用。

### 4.1 发起 Host 托管的 HTTPS 请求（仅 Windows）

只有声明了 `network.https` 的 Runtime 才会收到可选的 `api.network`。调用前检查一次能力；缺失时返回插件自己的可理解失败结果：

```js
export async function onCommand(invocation, api) {
  if (!api.network) {
    return unavailableResult(invocation.requestId)
  }

  try {
    const response = await api.network.request({
      url: 'https://api.example.com/translate',
      method: 'POST',
      headers: {
        accept: 'application/json',
      },
      body: {
        type: 'json',
        value: { text: invocation.input },
      },
    })

    if (response.status < 200 || response.status >= 300) {
      return providerErrorResult(invocation.requestId, response.status)
    }
    return translatedResult(invocation.requestId, response.body)
  } catch (error) {
    return networkErrorResult(invocation.requestId, error?.name)
  }
}
```

示例中的结果构造函数由插件自行实现；Host 不负责翻译、解析供应商 JSON 或解释 4xx/5xx。所有 HTTP 状态都会正常 resolve，插件必须检查 `response.status`。返回值包含最终整数 `status`、按小写名称组织的 `headers: Record<string, string[]>`，以及严格 UTF-8 `body`。

支持的请求只有：

- `GET`，且不能带 body。
- `POST`，body 可省略，或使用 `{ type: 'json', value }`、`{ type: 'text', value: string }`、`{ type: 'form', value: Record<string, string> }`。
- 可设置 `authorization`、`accept`、`accept-language` 和供应商自定义请求头。

Host 自己设置 `Host`、`Content-Length`、`Content-Type`、`User-Agent` 和 `Accept-Encoding`，并拒绝 Cookie、Origin/Referer、代理、连接管理、`sec-*`、`forwarded`、`via`、`x-forwarded-*` 等受保护头。目标必须是已授权的精确 HTTPS 域名和 443 端口；即使两个域名都已声明，也不允许跨域重定向。HTTP、IP、localhost、私有/特殊用途地址和宽松 TLS 都会被拒绝。

常用错误名及处理方向：

| `Error.name` | 插件建议处理 |
| --- | --- |
| `InvalidNetworkRequestError` | 修正 URL、方法、请求头/body 或请求大小 |
| `PermissionDeniedError` | 提示用户检查安装授权或设置中的网络开关 |
| `NetworkTargetDeniedError` | 检查 Manifest 精确域名、HTTPS、DNS 与重定向 |
| `NetworkTimeoutError` | 提示超时并允许用户重试 |
| `NetworkFailureError` | 显示一般网络故障，不展示请求密钥或完整请求 |
| `NetworkResponseTooLargeError` | 供应商响应超过 Host 上限 |
| `NetworkResponseInvalidError` | 供应商响应不是合法受支持的 UTF-8 HTTP 响应 |
| `NetworkLimitExceededError` | 本次命令调用过多或并发已满 |
| `ExpiredRequestError` | 当前命令已被替换、取消，或插件已停用/升级/卸载 |

固定上限为：URL 2048 字节，请求头 32 项/16 KiB，请求体编码后 64 KiB，响应头 64 项/32 KiB，响应体 1 MiB，总时限 10 秒，同域重定向最多 3 次；每次命令最多调用 8 次、最多并发 2 次，全 Host 公共插件最多并发 16 次。达到并发上限时不会排队。

命令过期或被新命令替换，以及权限撤销、停用、升级、卸载、Runtime teardown 和 Host 退出，都会中止尚未完成的请求并丢弃过期响应。不要把它当作后台任务接口。

### 4.2 使用 Host bearer 凭据引用（Windows `0.4.0+`）

生产密钥通过 Host 设置保存，不能写进包中。沿用 `network.https`，将最低宿主升到 `0.4.0`，为每个精确目标声明绑定：

```json
{
  "minimumHostVersion": "0.4.0",
  "supportedPlatforms": ["windows"],
  "permissions": ["network.https"],
  "network": {
    "httpsHosts": ["api.example.com"],
    "credentials": [
      { "id": "service-token", "settingKey": "api-token", "host": "api.example.com", "type": "bearer" }
    ]
  },
  "settings": [{ "type": "secret", "key": "api-token", "label": "API token" }]
}
```

这是相关字段片段。`credentials` 可省略；出现时必须有 1–8 项，不可为 `null` 或空数组。每项只能含这四个字段，`id` 唯一，`id` / `settingKey` 都匹配 `^[a-z][a-z0-9.-]{0,63}$`，不修剪、不忽略大小写；`settingKey` 精确引用 secret，`host` 是 `httpsHosts` 的精确成员，`type` 只能为 `bearer`。一个 secret 发给两个 host 需要两个声明。

Runtime 只提交 ID：

```js
const response = await api.network.request({
  url: 'https://api.example.com/v1/items',
  method: 'GET',
  credential: 'service-token',
})
```

`credential` 不接受 token 对象、settingKey、空串、`null` 或额外参数。不要同时传任何大小写的 `Authorization` 头；Host 会内部注入 `Authorization: Bearer <token>`，并将其计入请求头限额。目标必须精确匹配绑定 host，重定向仍只能留在同一 host。未传 `credential` 时保留旧请求行为，包括普通 Authorization 头。

保存的 bearer 值必须是 1–8192 字节 ASCII，匹配 `^[A-Za-z0-9._~+/-]+=*$`；不要加 `Bearer ` 前缀、空白、换行或 Unicode，Host 不 trim 或转换 Base64。secret 存储上限仍为 64 KiB，能保存不代表可作 bearer。`api.settings.isSecretConfigured('api-token')` 只表示已配置，不表示格式有效或网络已授权；没有明文 getter，Panel / Window 也不能读取或消费 secret。

安装/升级提示展示完整 `(id, settingKey, host, type)` 绑定并单独确认。新增或修改任一四元组需重新同意完整候选网络；重排无需确认，移除后重加需确认，普通旧 host 授权不自动获得 credential 授权。取消升级保留旧版，同声明升级保留撤销状态。设置关闭网络会清空两类授权；重新开启必须确认当前完整绑定，Host UI 带上所展示的 inventory revision，陈旧状态先刷新、再由用户确认。

增加并处理 `CredentialUnavailableError`：缺失、读取/解密失败或 bearer 格式不适用均映射为这个固定错误，不区分内部原因。未知/未授权 ID 或因坏记录关闭的 authority 是 `PermissionDeniedError`；形状/Authorization 冲突/注入后超限是 `InvalidNetworkRequestError`；绑定目标不符是 `NetworkTargetDeniedError`。`ExpiredRequestError` 表示当前请求已结束或因配置变更取消。完整错误表见 [API v1](./public-plugin-v1.md#host-managed-https)；不要展示原始异常文字。

Host 设置逐 key 使用 password 输入保存/显式清除；空白未编辑输入不表示删除。保存/删除会取消整个插件当前网络上下文，旧 requestId 永不恢复；后续新命令才可用新值，旧 handler 尚未完成时新命令等待。取消前准备失败不影响旧请求，取消后的提交失败仍保留取消效果。多 key 和普通/secret 文件不是批次原子事务：错误不承诺回滚，已提交 key 可保留，磁盘也可能是旧值或新值。失败后先刷新状态，不把“保存失败”当作成功撤销。

提交失败本次关闭网络，但不是永久锁存。后续启动、启用、显式授权或 Runtime 恢复/替换必须检查实际磁盘与当前完整授权；有效旧/新值或缺失文件可恢复普通网络，已有坏记录仍关闭。缺失是合法的未配置状态，带引用请求会返回 `CredentialUnavailableError`。要可靠撤销，应等到清除或关闭网络成功；任何恢复都不复活旧请求。

服务会收到凭据并可能在正常响应 header/body 原样或编码回显。Host 不静默改写合法响应、不自动回填请求头；错误/Debug/日志不含 secret，但插件必须按目标服务的信任边界决定是否展示响应。取消不能追回已发送字节或已投递结果，不提供远端事务回滚。HTTPS 库可能持有临时明文副本，不承诺所有内存副本彻底清零。OAuth、刷新、签名、自定义凭据头模板与明文 getter 均不支持。

完整最小实现见 [demo-credentials](../../examples/public-plugins/com.uipilot.demo-credentials/README.md)。它只含占位目标和离线 fake provider；测试使用合成 token，不请求公网，不将 mock 通过称为真实 Host 验收。

## 5. 分支 A：返回主界面结果

创建 `package/dist/runtime.js`：

```js
function localDate(invokedAt) {
  return invokedAt.slice(0, 10)
}

export async function onCommand(invocation, _api) {
  const text = `${invocation.input} ${localDate(invocation.context.invokedAt)}`
  return {
    requestId: invocation.requestId,
    results: [
      {
        id: 'hello-return-copy',
        title: text,
        subtitle: '按 Enter 复制',
        defaultAction: {
          type: 'copyText',
          text,
        },
      },
    ],
  }
}
```

交互流程：

1. 用户输入 `/hello-return I am  Jack` 并按 Enter。
2. Runtime 收到保留内部双空格的 `I am  Jack`。
3. 主界面显示并默认选中插件结果。
4. 用户再次按 Enter，宿主执行 `copyText`。

复制由 UiPilot 执行。插件不能直接访问系统剪贴板，也不能返回任意回调。

不需要复制时可以移除 `defaultAction`，同时从 Manifest 移除 `clipboard.write`。

## 6. 分支 B：打开单例子窗口

### 6.1 Runtime 返回窗口数据

创建 `package/dist/runtime.js`：

```js
function localDate(invokedAt) {
  return invokedAt.slice(0, 10)
}

export async function onCommand(invocation, _api) {
  return {
    requestId: invocation.requestId,
    data: {
      returnText: `${invocation.input} ${localDate(invocation.context.invokedAt)}`,
    },
  }
}
```

`data` 必须是可序列化 JSON。Runtime 不创建窗口；它只把数据交给宿主。

### 6.2 发布或安排请求绑定消息（仅 Windows）

Manifest 同时声明 `notifications.publish` 且用户授权后，Runtime 可以在当前命令请求中发布一条消息：

```js
export async function onCommand(invocation, api) {
  const returnText = `${invocation.input} ${invocation.context.invokedAt.slice(0, 10)}`
  await api.notifications.publish({ content: returnText })
  return {
    requestId: invocation.requestId,
    data: { returnText },
  }
}
```

`content` 必须是 1 到 500 个 Unicode 字符、去除首尾空白后不变的单行纯文本，不能包含控制字符。每次请求最多成功发布一次；API 对象在请求完成、超时、被新请求淘汰、插件更新或卸载后立即失效。不要保存 API 对象给定时器或后台任务使用。

`publish()` 在消息原子持久化后 resolve。此后的 Windows 系统通知或托盘提示属于宿主尽力执行的副作用，即使系统通知被关闭或发送失败，已经保存的消息和本次插件响应也不会回滚。常见拒绝包括 `InvalidNotification`、`AlreadyPublished`、`ExpiredRequestError` 和 `MessageStoreUnavailable`。

需要窗口立即显示、消息稍后送达时，可把同一请求中的唯一通知动作改为宿主持有的延迟任务：

```js
await api.notifications.schedule({
  content: returnText,
  delayMs: 10_000,
})
```

`delayMs` 必须是 JavaScript 安全整数，范围为 `1_000..=86_400_000`（1 秒到 24 小时）；每个插件最多同时等待 32 条消息。`schedule()` 在宿主接管任务后 resolve，不等待消息到期，因此插件可立即返回窗口或主结果。隐藏主窗口或插件窗口不取消任务；禁用、卸载或更新插件会取消等待任务，退出 UiPilot 会直接丢弃且下次启动不恢复。常见新增拒绝为 `InvalidDelay` 和 `ScheduleLimitExceeded`。

不要把 API 对象交给 `setTimeout`，也不要期待插件代码在请求结束后继续运行。`schedule()` 保存的是一条纯文本宿主任务，不支持后台回调、重复定时、查询、取消、修改、失败重试或跨重启恢复。`publish()` 与 `schedule()` 合计每次请求只能成功一次，第二次调用返回 `AlreadyPublished`。

### 6.3 创建窗口页面

`package/dist/window.html`：

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Hello Window</title>
    <link rel="stylesheet" href="./window.css" />
  </head>
  <body>
    <main>
      <h1>Hello Window</h1>
      <p id="input"></p>
      <p id="context"></p>
      <p id="result"></p>
    </main>
    <script type="module" src="./window.js"></script>
  </body>
</html>
```

`package/dist/window.js`：

```js
const input = document.querySelector('#input')
const context = document.querySelector('#context')
const result = document.querySelector('#result')

window.uipilotPluginWindow.onUpdate((update) => {
  input.textContent = update.input
  context.textContent = `${update.platform} / ${update.theme} / #${update.instanceNumber}`
  result.textContent = String(update.data.returnText ?? '')
})
```

必须尽早注册 `onUpdate`。它返回取消监听函数，可在页面销毁前调用。

窗口更新对象还包含 `requestId` 和 `invokedAt`。`instanceNumber` 在 v1 中始终为 `1`，因为每个插件只能有一个子窗口。

### 6.4 使用宿主主题

`package/dist/window.css`：

```css
:root {
  color: var(--uipilot-color-text);
  background: var(--uipilot-color-background);
  font-family: var(--uipilot-font-family);
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
}

main {
  padding: 24px;
}

p {
  padding: 10px 0;
  border-bottom: 1px solid var(--uipilot-color-border);
}
```

宿主提供以下 CSS 变量：

- `--uipilot-color-background`
- `--uipilot-color-surface`
- `--uipilot-color-text`
- `--uipilot-color-text-muted`
- `--uipilot-color-border`
- `--uipilot-color-accent`
- `--uipilot-color-danger`
- `--uipilot-font-family`

图钉、关闭、拖拽、焦点交接、窗口位置和主题切换均由 UiPilot 外层窗口管理。插件页面不要实现自己的标题栏，也不能调用 Tauri 命令控制窗口。

### 6.5 使用宿主持有的窗口计时器（仅 Windows）

需要窗口隐藏后继续计时时，Manifest 必须同时声明 `ui.window`、`notifications.publish` 和
`timer.control`，并使用 `submit + window`。三项权限必须在安装时全部确认；`timer.control` 不能用于
Runtime，也不能脱离插件窗口调用。完整参考实现见
[`com.uipilot.pomodoro`](../../examples/public-plugins/com.uipilot.pomodoro)。

计时插件必须在包内提供唯一的固定闹铃，路径和大小写必须完全一致：

```text
package/assets/sounds/timer-alarm.wav
```

该文件属于宿主私有资源，不写入 Manifest 路径字段，也不能由 Runtime 或窗口读取。UiPilot 在安装时完整解析
WAV，只接受 1/2 声道、44.1/48 kHz、16/24-bit little-endian PCM，文件最多 2 MiB、有效时长最多 15 秒；
未知或重复 chunk、尾随字节及包内其他 WAV 都会导致安装失败。声明 `timer.control` 却缺少此文件，或未声明
权限却携带此文件，同样拒绝安装。修改闹铃后必须重新打包并重新安装插件。

普通插件消息始终使用 UiPilot 自带的公共提示音播放一次，插件不能替换。只有有效 Timer 到期后，宿主才会
从内存循环播放该轮开始时冻结的插件闹铃；打开主窗口会停止声音，但不会自动清除未读消息。插件没有任意
音频 API，也不要在 WebView 中使用 `<audio>` 或 WebAudio。

每个 active plugin generation 最多有一个宿主持有的计时器。窗口每次收到 `onUpdate` 时都会获得新的
控制会话，因此应先订阅，再读取基准状态：

```js
let unsubscribe = null

window.uipilotPluginWindow.onUpdate(async (update) => {
  unsubscribe?.()
  const timer = window.uipilotPluginWindow.timer
  unsubscribe = timer.onStateChanged(renderTimerState)
  renderTimerState(await timer.getState())
})
```

窗口准备期间只允许 `getState()` 和 `onStateChanged()`；Start、Stop、Reset 只有在宿主完成原生显示与
焦点交接后才可调用。窗口隐藏、关闭、新 invocation、插件改名或保存设置会撤销当前控制会话，旧 API
引用随后返回 `ExpiredWindowSessionError`。隐藏窗口不会取消已经运行或暂停的宿主计时器；重新打开窗口
后必须重新订阅和读取。

```js
await timer.start({
  durationMs: 600_000,
  completionMessage: '番茄钟完成',
})
await timer.stop()  // running -> paused；再次调用幂等
await timer.start() // paused -> running；恢复时不能再传 input
await timer.reset() // 回到 idle，保留显示时长但不自动开始
```

`durationMs` 是 `1_000..=86_400_000` 的 JavaScript 安全整数。`completionMessage` 是 1 到 500 个
Unicode 标量值的纯文本。首次 `idle` 的 `durationMs / remainingMs` 都是 `null`；参考番茄钟页面自己显示
`10:00`，用户点击 Start 后宿主才冻结输入。`running` 时再次无参 Start、以及 idle/paused/fired 的幂等
操作都返回当前权威状态；需要 input 时省略会得到 `TimerInputRequired`，不允许 input 时传入会得到
`TimerInputNotAllowed`。

状态的 `timerRevision` 是规范十进制 `u64` 字符串，不能转成 JavaScript `number`，也不能直接按字符串
比较。应先比较长度，再在长度相等时按字典序比较。较低 revision 必须丢弃；相同 revision 通常丢弃，
只有当前会话最新一次 `getState()` 返回的 running 状态，且 phase 和 duration 都相同，才可刷新本地
remaining 锚点。页面可以用 `performance.now()` 插值显示数字，但本地 interval/animation frame 不能拥有
到期、消息或后台副作用。

到期时宿主先把完成消息原子保存到消息中心，成功后才进入 `fired` 并尝试循环播放该轮冻结的插件闹铃。消息保存
失败会回到 `idle`，不响铃、不重试；系统通知或音频失败不删除已保存消息。禁用、卸载、故障停用或成功
升级会取消当前 generation 的计时器。退出 UiPilot 会丢弃所有计时器且下次启动不恢复、不补发。

常见错误包括 `PermissionDenied`、`ExpiredWindowSessionError`、`InvalidTimerInput`、
`TimerInputRequired`、`TimerInputNotAllowed`、`MessageStoreUnavailable` 和 `TimerUnavailable`。

### 6.6 在子窗口保存插件私有状态

所有合法插件内容窗口都可以使用冻结的 `window.uipilotPluginWindow.storage`，无需新增 Manifest 权限。它与
Runtime 的 `api.storage` 共用该插件的私有命名空间：

```js
const storage = window.uipilotPluginWindow.storage
const previous = await storage.get('pomodoro.duration-minutes')
await storage.set('pomodoro.duration-minutes', 25)
await storage.remove('pomodoro.duration-minutes')
```

`get()` 可在 `onUpdate` 的 Prepared 阶段使用；`set()` 和 `remove()` 要等窗口完成显示并进入 Active 后才可
调用。窗口隐藏、会话被替换、插件禁用、升级或卸载后，保存的旧 `storage` 引用会返回
`ExpiredWindowSessionError`。Runtime 与窗口都只能使用匹配 `^[a-z][a-z0-9.-]{0,63}$` 的 key，并拒绝
`__proto__`、`prototype` 和 `constructor`。值必须是有限 JSON，所有入口共享每插件 5 MiB 配额与原子写入；
非法 key/value 返回 `InvalidOperation`，配额或持久化失败返回 `StorageError`。

参考番茄钟使用 `pomodoro.duration-minutes` 保存 10、15、25、30 或 45 分钟。首次打开时默认为
10 分钟；窗口重新打开、UiPilot 重启、插件升级或保留数据重装后恢复上次选择。运行或暂停期间修改
选择只影响下一轮，当前轮继续使用 Start 时已冻结的 `durationMs`。写入失败时，页面应恢复上次已持久化的
有效值；没有有效值时恢复 10 分钟。由于 `get()` / `set()` 是异步调用，完成时必须确认原窗口会话仍是
当前会话；过期会话的迟到结果不得覆盖新窗口的选择或错误状态。

## 7. 分支 C：在启动器内挂载面板

Runtime 每次 Enter 都会重新执行，并返回当前面板数据：

```js
export async function onCommand(invocation) {
  return {
    requestId: invocation.requestId,
    data: { echo: invocation.input },
  }
}
```

`panel.js` 必须尽早注册独立面板桥；它可以接收更新、访问插件私有存储，并请求聚焦宿主参数输入框：

```js
window.uipilotPluginPanel.onUpdate(async (update) => {
  document.querySelector('#result').textContent = String(update.data.echo ?? '')
  await window.uipilotPluginPanel.storage.set('last-input', update.input)
})

window.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
    event.preventDefault()
    void window.uipilotPluginPanel.focusHostInput()
  }
})
```

Panel 首次打开成功后，宿主会自动把光标放到命令 tag 后的参数输入框；插件不需要在启动时调用 `focusHostInput()`。当用户点击列表、编辑器等 Panel 内容后，插件可在 Ctrl+F 等明确交互中调用该方法，把键盘焦点交还给参数输入框。

`focusHostInput()` 不接收参数，只把焦点交回当前会话带命令 tag 的参数输入框；它不会关闭面板、删除 tag、提交或改写参数与选择区。输入框已经聚焦时可以重复调用。会话已经隐藏、替换或销毁时调用会安静地无操作完成；当前会话的宿主聚焦失败会拒绝 Promise。

非空 `panel.hostKeys` 要求页面在 ready 前恰好注册一次 `onHostKey(handler)`。声明只允许 `ArrowDown`、`ArrowUp`、`Primary+N`、`Tab`、`Shift+Tab`、`Enter`、`Delete`，并按 `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter < Delete` 规范化。方向键和 `Tab` 只匹配无修饰键；`Shift+Tab` 只匹配仅带 Shift 的 Tab；`Enter` 只匹配无修饰键且非 IME composing；Windows 的 `Primary+N` 只匹配 Ctrl+N，macOS 只匹配 Meta+N。未声明按键、普通字符和其他组合不会路由。handler 串行执行；抛错或拒绝会 ack 但不重试，超过 2 秒未完成会隐藏并销毁会话。调用 unsubscribe 也会结束会话。

`panel.hostKeyFocus` 可省略或设为 `content` / `host`。省略等价于 `content`：每次 Host Key 投递前，宿主把原生焦点交给 Panel WebView，适合需要打开对话框或继续接收内容键盘输入的面板。`host` 保持带命令 tag 的宿主输入框焦点，适合只用 Host Key 切换分类或移动选择的面板；插件不需要在 handler 后调用 `focusHostInput()`。显式字段要求 `minimumHostVersion >= 0.3.4`；错误类型、`null` 和未知值都会被拒绝。焦点策略不改变串行队列、ack、超时、会话销毁或 Enter 粘贴权限。

`requestHide()` 不接收参数。当前会话的 Promise 在 Host 接纳隐藏后、WebView 销毁前 resolve；resolve 后下一个 macrotask 即可销毁文档，不要再启动 DOM 工作。旧会话或已销毁会话安静完成；当前接纳失败以 `windowFailed` 拒绝。renderer 在观察接纳前挂死时 Promise 可能永不 settle，Host 最迟 30 秒回收；正常观察后有 500ms fallback。

Panel 内容中的 Escape 由 Host capture listener 在同一轮同步事件结束后的 microtask 仲裁。同步 `preventDefault()`、打开的 `<dialog>` 或 IME 会阻止隐藏；`await` 之后再 `preventDefault()` 已来不及。显式返回会 best-effort 恢复 UiPilot 显示前捕获的外部窗口；失焦隐藏和启动交接不恢复。

面板桥没有通用 `close()`、计时器或通知接口，也不能调用 Tauri `invoke`、网络或 Shell；主动隐藏只使用窄接口 `requestHide()`。宿主拥有命令 tag 和参数输入框：第一次 Enter 打开面板，后续 Enter 提交当前参数，并仅在提交后通过 `onUpdate.input` 把新参数交给面板；`focusHostInput()` 不提供实时按键流，`onHostKey()` 只交付 Manifest 声明的 Host chord。× 或参数光标位于 0 时的 Backspace 退出。Escape、失焦隐藏、插件停用、卸载或升级都会销毁当前面板，下次打开主界面从空白启动器开始。

声明 `clipboard.history.read` 的 Windows 面板会收到 `window.uipilotPluginPanel.clipboardHistory`。该桥只提供宿主管理的摘要：`list()`、`onChanged(handler)`、`remove({ id })`、`clear()`；声明 `clipboard.history.paste` 后还可在收到 `Enter` Host Key 的同一会话中调用 `paste({ id, routeSequence })`。Panel 只会看到文字预览、可显示的 `data:image/png;base64,...` 图片缩略图、文件名/数量/可用性、`id`、`capturedAt` 和 `revision`；公共插件 CSP 仍会拒绝网络、文件和 `blob:` 图片。Panel 不会看到完整文本、原图、完整路径、HWND、PID 或任意按键模拟能力。用户授权后，UiPilot 运行、插件启用且权限有效期间的文字、图片和文件列表剪贴板变化会被宿主记录到本机插件隔离数据目录；第一版不会自动识别密码框或敏感输入来源。

完整实现见 [`com.uipilot.demo-panel`](../../examples/public-plugins/com.uipilot.demo-panel)。

## 8. 添加插件图标

图标是可选的固定文件，不写入 `plugin.json`：

```text
package/icon.png
```

要求：

- 文件名必须精确为小写 `icon.png`。
- 必须位于包根目录。
- 必须是完全可解码的静态 PNG，不能是 APNG。
- 尺寸必须是 `128 x 128`。
- 文件不超过 `128 KiB`。
- 包内不能再有其他 PNG。

未提供 `icon.png` 时，UiPilot 会显示默认插件符号。提供了图标但格式、尺寸或大小不合法时，插件会在准备安装阶段被拒绝；已经通过安装的图标若在界面加载时失败，才会回退到默认插件符号。

## 9. 编写 Runtime 测试

Node.js 内置测试足以验证纯 Runtime。创建 `tests/runtime.test.js`：

```js
import assert from 'node:assert/strict'
import test from 'node:test'

const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)

async function loadRuntime() {
  return import(runtimeUrl.href)
}

function createApiMock({
  settings = {},
  storage = new Map(),
  publish = async () => {},
  schedule = async () => {},
} = {}) {
  return Object.freeze({
    storage: Object.freeze({
      async get(key) {
        return storage.has(key) ? storage.get(key) : null
      },
      async set(key, value) {
        storage.set(key, value)
      },
      async remove(key) {
        storage.delete(key)
      },
    }),
    settings: Object.freeze({
      async get(key) {
        return Object.prototype.hasOwnProperty.call(settings, key)
          ? settings[key]
          : null
      },
      async isSecretConfigured() {
        return false
      },
    }),
    notifications: Object.freeze({ publish, schedule }),
  })
}

test('runtime preserves request ownership', async () => {
  const runtime = await loadRuntime()
  const invocation = Object.freeze({
    apiVersion: 1,
    requestId: 'test-request-1',
    input: 'I am  Jack',
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-18T12:00:00+08:00',
    }),
  })

  const response = await runtime.onCommand(invocation, createApiMock())
  assert.equal(response.requestId, invocation.requestId)
})
```

运行：

```powershell
node --test .\tests\runtime.test.js
```

测试使用设置时，可传入 `createApiMock({ settings: { prefix: '[demo]' } })`；测试消息时传入 `publish` 或 `schedule` spy，并验证调用次数和完整 DTO。这个 mock 只覆盖 Runtime 单元测试需要的异步接口，不模拟宿主的权限、配额、请求过期和数据持久化检查；这些边界仍以 UiPilot 宿主为准。

还应针对自己的业务结果添加精确断言。子窗口型插件应额外检查 `window.js` 使用 `window.uipilotPluginWindow.onUpdate`；面板型插件应检查 `panel.js` 使用 `window.uipilotPluginPanel.onUpdate`。两者都不得包含 `invoke(`、`fetch(`、`WebSocket` 或窗口置顶逻辑。

仓库参考测试：

```powershell
node --test examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js
node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js
node --test examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js
node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js
```

## 10. 使用独立 CLI 验证

UiPilot 提供纯 TypeScript 的 `@uipilot/plugin-cli`。第三方开发者只需要 Node.js 20 或更高版本和已提供的 npm `.tgz`，不需要安装 Rust、UiPilot 或下载 UiPilot 源码。CLI 提供本地 pack 与静态 validate，不安装插件、不运行 Runtime/窗口代码、不联网，也不修改源目录；validate 只读，pack 写入指定归档及同目录临时文件。

当前包尚未发布到 npm Registry。取得 `.tgz` 后，可在本机或 CI 工作目录安装：

```powershell
npm install --global .\uipilot-plugin-cli-0.1.0.tgz
```

验证开发目录或最终归档：

```powershell
uipilot-plugin validate .\package --platform windows
uipilot-plugin validate .\com.example.hello-return.uipilot-plugin --platform windows
```

`--platform` 可选 `windows` 或 `macos`；省略时使用当前受支持的系统。CI 可增加 `--json`，标准输出将只包含稳定的 `PluginValidationReportV1`：

```powershell
uipilot-plugin validate .\package --platform windows --json
```

退出码含义：

- `0`：包对所选平台有效。
- `1`：包结构、Manifest、兼容性或资源无效；JSON 模式仍返回验证报告。
- `2`：命令用法错误或 CLI 自身发生意外故障。

使用 `timer.control` 时，CLI 会检查完整的 Windows `submit + window` 权限组合，以及唯一固定文件 `assets/sounds/timer-alarm.wav`。缺少/多带 WAV、使用其他 WAV 路径、非规范 RIFF/WAVE、错误声道/采样率/位深、超过 2 MiB 或 15 秒都会返回 `RESOURCE_INVALID`。CLI 通过后仍应在目标平台用 UiPilot 做最终安装和交互验收。

使用 `network.https` 时，CLI 会静态验证 Manifest 的 Windows 平台、权限配对和精确主机语法；普通 HTTPS 要求 `minimumHostVersion >= 0.3.2`。声明 `network.credentials` 时，还要求 `minimumHostVersion >= 0.4.0`，并验证凭据引用元数据：数组含 1–8 项、每项仅含规定字段、`id` 唯一、`id` / `settingKey` 语法合法、`settingKey` 精确引用 secret 设置、`host` 是 `httpsHosts` 的精确成员、`type` 为 `bearer`。CLI 不读取已保存的 secret 值、不验证 token 能否通过第三方服务认证，也不发送网络请求。

使用 `clipboard.history.read` / `clipboard.history.paste` 时，CLI 只验证 Windows `submit + panel + ui.panel`、`minimumHostVersion >= 0.3.3`、`paste` 依赖 `read`、以及 `clipboard.read` 不能作为别名；真实剪贴板采集、持久化、粘贴和焦点恢复仍以 UiPilot 宿主验收为准。

## 11. 使用开发目录安装

1. 启动 UiPilot。
2. 打开 **设置 > 插件 > 公开插件**。
3. 点击“选择开发目录”图标按钮。
4. 选择插件的 `package` 目录。
5. 检查插件名称、版本、图标、权限；网络插件还要逐项检查完整 HTTPS 域名列表。
6. 点击“确认安装”。
7. 在主界面输入 `/` 加启动键测试插件。

也可以点击“选择插件包”图标按钮安装 `.uipilot-plugin` 归档。公开插件页提供“筛选插件名称”输入框；列表中可查看插件图标、名称、版本、当前启动命令、简介、详情、删除和启用状态。详情页可查看权限列表、网络 Host，并编辑“启动键”；启动键输入框失焦后保存，旁边的恢复默认按钮会回到 Manifest 中的 `command.defaultName`。

删除插件时需要在弹窗中选择“全部卸载”或“保留数据卸载”。“保留数据卸载”会移除安装状态和权限授权，但保留插件私有数据，供后续重新安装或升级恢复使用。

用户可能修改启动键。实际生效名称以 UiPilot 设置页为准，不以 `defaultName` 永久锁定。

主界面测试时：

- 主界面结果型插件进入命令 tag / 参数输入状态；提交后在主界面显示结果，带 `copyText` 默认动作的结果可再次按 Enter 复制。
- 子窗口型插件在选择或输入完整命令并回车后直接打开宿主管理的单例子窗口。
- 面板型插件在选择或输入完整命令并回车后直接挂载到主启动器内，并把光标放到命令 tag 后的参数输入框。

修改插件后：

1. 提高 `plugin.json` 的 `version`。
2. 再次选择同一个开发目录。
3. 确认更新。

更新会在完整候选与 Runtime ready 校验通过后原子发布。验证/名称冲突拒绝，或确定未提交且旧字节复核成功的持久化失败，保留旧配置；持久结果不确定或旧字节复核失败时遵循 fail-closed 恢复，不能承诺任何更新失败后旧版都可用。

每条命令只有一个 nameOverride：覆盖会替换 effectiveName 并释放旧路由，不是同时保留多个别名。设置页逐命令改名，包级设置和启停仍只有一份。同 ID 升级保留覆盖/收藏，使用新 manifest 默认名；删除 ID 成功后状态移除，删除→重启→重加是 fresh。更换 ID 即删除再新增，不按同名或数组位置转移状态。旧 command 只由显式 default 继承；没有 default 的 commands 包不会自动继承旧值。

新 Host 内可按普通校验、冲突、授权和生命周期规则回退包版本，只保留当前仍存活的 ID；先前删除的 default 在回退旧包时重新创建。Host 二进制降级不同：一旦正常事务或必要恢复写出 state schema 2，只懂 schema 1 的旧 Host 不保证能读取这个数据目录；不提供自动回写旧 schema 或利用 backup 降级的承诺。

## 12. 打包 `.uipilot-plugin`

在插件项目根目录使用已安装的独立 CLI。输出目录应在源 `package` 目录外，并事先存在：

```powershell
uipilot-plugin validate .\package --platform windows
uipilot-plugin pack .\package --output .\com.example.hello-return.uipilot-plugin --platform windows
uipilot-plugin validate .\com.example.hello-return.uipilot-plugin --platform windows
```

`pack` 从同一份经过校验的目录快照生成确定性 ZIP 归档，根目录直接包含 `plugin.json`，不会多包一层 `package/`。成功时显示绝对输出路径和 SHA-256。将文件名换成自己的 `pluginId`；CLI 不会执行或安装插件。

必须显式提供 `--output`，后缀严格为 `.uipilot-plugin`。父目录和祖先路径不能是符号链接或 Windows reparse point，输出不能位于源目录内。已有文件、目录或链接一律拒绝覆盖，没有 `--force`；再次打包请选择新文件名。输出文件系统须支持硬链接（Windows 通常为 NTFS），FAT/exFAT 等不支持的文件系统会返回 `OUTPUT_WRITE_FAILED`。

`--platform` 与 validate 一致：Windows/macOS 默认当前平台，其他系统必须显式选择。可加 `--json` 获取 `{schemaVersion:1,created:true,output:{path,sha256},validation,warnings?}`；预期失败为 `{schemaVersion:1,created:false,output:{path},validation?,error:{code,message}}`。成功退出 `0`，包无效或预期输出失败退出 `1`，参数错误或未知内部异常退出 `2`。`TEMP_CLEANUP_FAILED` 表示有效归档已生成但临时文件清理失败，仍退出 `0`，应保留并验证生成的归档。

最后在 UiPilot 的 **设置 > 插件 > 公开插件** 点击“选择插件包”，选择生成的归档，检查权限并确认本地安装。在真实宿主验证交互、焦点、双主题和数据持久化；CLI 通过不能替代这些人工检查。本流程不包含在线发布。

## 13. 可选：插件私有状态与设置

Runtime API 还提供请求期内的插件隔离存储和设置读取；窗口存储与其共享同一命名空间，窗口端会话规则见
6.6：

```js
const previous = await api.storage.get('last-value')
await api.storage.set('last-value', invocation.input)
await api.storage.remove('last-value')

const prefix = await api.settings.get('prefix')
const hasToken = await api.settings.isSecretConfigured('token')
```

存储只能保存 JSON。插件不能读取 secret 明文。设置字段定义、类型和限制见完整 API 文档与 Schema。

## 14. 常见问题

### 准备安装时失败

检查：

- 选择的是 `package` 目录，而不是插件项目根目录。
- `plugin.json` 没有未知字段、重复字段或错误大小写。
- 版本号是完整的 `major.minor.patch`。
- 当前系统包含在 `supportedPlatforms` 中。
- 包内没有未允许的文件、嵌套包、符号链接或路径穿越。
- `icon.png` 满足固定名称、尺寸、大小和静态 PNG 规则。

### 点击确认安装后失败

检查：

- `runtime.entry` 文件存在并能作为 ES Module 加载。
- Runtime 导出了 `async function onCommand`。
- Manifest 只声明宿主当前实现的权限。
- 输出模式、权限和窗口入口组合一致。

### 输入命令没有匹配到插件

检查插件是否已启用、是否有运行故障，以及用户是否在设置页修改了启动键。

### 结果出现但不能复制

Manifest 必须声明 `clipboard.write`，安装时用户必须授权。未声明 resultActions 的结果使用唯一的 `copyText` 默认动作；Host 0.10.0+ 的显式 opt-in 可使用下文的命名复制动作。

### 子窗口没有内容

确认窗口脚本尽早注册了 `window.uipilotPluginWindow.onUpdate`，并且回调能在五秒内完成。不要在内容页调用 Tauri、Shell、网络或其他宿主能力。

### 面板没有内容或隐藏后仍残留

确认基础 panel 的 `minimumHostVersion` 至少为 `0.3.0`；使用基础 `hostKeys`、`onHostKey` 或 `requestHide` 时至少为 `0.3.1`；使用 `Tab`、`Shift+Tab`、`Enter` 或剪贴板历史时至少为 `0.3.3`；显式使用 `hostKeyFocus` 时至少为 `0.3.4`。Manifest 使用 `submit + panel + ui.panel`；`panel.js` 尽早注册 `onUpdate`，非空 `hostKeys` 还必须在 ready 前注册一次 `onHostKey`。面板会话不会跨主窗口隐藏保留；重新显示后需要再次执行命令。

### 剪贴板历史为空或粘贴被拒绝

确认目标系统是 Windows、`minimumHostVersion` 至少为 `0.3.3`，Manifest 使用 `submit + panel + ui.panel` 并声明 `clipboard.history.read`；需要自动粘贴时还要声明并授权 `clipboard.history.paste`，且必须在收到声明的 `Enter` Host Key 后使用同一事件的 `routeSequence` 调用 `clipboardHistory.paste()`。查看 `Error.name` 区分 `PermissionDenied`、`ExpiredPanelSession`、`RecordNotFound`、`RecordUnavailable`、`PasteTargetUnavailable` 和 `ClipboardWriteFailed`。不要把剪贴板正文、完整路径、图片内容或系统窗口信息写入日志。

### 网络请求被拒绝或没有返回

确认目标系统是 Windows、`minimumHostVersion` 至少为 `0.3.2`，Manifest 同时声明 `network.https` 和精确的 `network.httpsHosts`，并检查设置中的网络访问开关。目标必须是 HTTPS 443 且与授权域名完全相同；子域名不会自动继承授权。查看 `Error.name` 区分权限、目标策略、超时、传输、响应和生命周期错误，不要把凭据、签名参数或完整请求体写入日志。

## 15. MVP 边界

当前不支持：

- 多命令 panel/window、多个窗口、每命令快捷键或独立别名集合；Host `0.5.0+` 仅新增多 mainResult。
- 插件代码长期后台运行、插件自建计时器或任意后台回调；单轮窗口计时必须交给宿主的 `timer.control`，延迟纯文本消息交给 `notifications.schedule()`。
- 浏览器/WebView 网络、原始套接字、任意文件、原生二进制和 Shell；外部访问仅限 Runtime 请求期内的 Host 托管 HTTPS。
- 输入模拟或控制鼠标键盘。
- HTML/Markdown 结果、未经声明的动作回调。
- 插件间通信、远程资源、依赖安装、市场发布和自动更新。

遇到未覆盖的需求时，不要通过 WebView 或 Tauri 私有对象绕过限制；应等待宿主公开相应版本化 API。

## 16. 发布前检查清单

- [ ] `pluginId` 使用自己的稳定命名空间。
- [ ] `version` 已提高，`minimumHostVersion` 合理。
- [ ] `description`、`summary`、`inputPlaceholder` 各司其职。
- [ ] `outputMode`、权限和 `window` / `panel` 入口组合正确；基础面板要求 Windows 与宿主 `0.3.0+`，Host 按键与主动隐藏要求 `0.3.1+`，扩展 Host 按键和剪贴板历史要求 `0.3.3+`，显式 Host Key 焦点策略要求 `0.3.4+`。
- [ ] 使用网络时，仅面向 Windows Host `0.3.2+`，同时声明 `network.https` 与完整精确的 `network.httpsHosts`，并处理 `api.network` 缺失、HTTP 非成功状态和固定错误名（包括 `CredentialUnavailableError`）。
- [ ] 使用剪贴板历史时，仅面向 Windows Host `0.3.3+`，同时声明 `ui.panel` 和 `clipboard.history.read`，需要粘贴时再声明 `clipboard.history.paste`，并处理六种固定 `Error.name`。
- [ ] 网络插件未记录凭据或完整请求；生产 token 不打包，使用 bearer 引用时要求 Windows `0.4.0+`、完整绑定授权，并理解服务端回显、取消不能撤回远端请求、保存失败不回滚且后续核验可恢复网络的边界。
- [ ] Runtime 始终原样返回 `requestId`；commands 包显式按 commandId 分派并拒绝未知/缺失 ID，重排或改启动名不改变身份。
- [ ] 使用消息能力时，仅在 Windows Manifest 中声明并授权 `notifications.publish`，且每个请求只提交一次 `publish()` 或 `schedule()`。
- [ ] 使用窗口计时时，同时声明 `ui.window`、`notifications.publish`、`timer.control`，并按十进制字符串 revision 合并状态。
- [ ] `timer.control` 包只包含唯一的 `assets/sounds/timer-alarm.wav`，且 WAV 满足固定 PCM、大小与时长限制。
- [ ] 内部空格不会被插件意外压缩。
- [ ] 子窗口使用宿主 CSS 变量并只通过 `onUpdate` 接收数据。
- [ ] 面板内容只通过 `uipilotPluginPanel.onUpdate`、`onHostKey`、`focusHostInput`、`requestHide`、`storage` 与授权后的 `clipboardHistory`，隐藏/停用/卸载/升级后不保留会话。
- [ ] `icon.png` 满足固定规则。
- [ ] Runtime 测试通过。
- [ ] 开发目录和最终 `.uipilot-plugin` 均通过 `uipilot-plugin validate`。
- [ ] 开发目录和最终 `.uipilot-plugin` 都完成安装验收。


## Panel Runtime actions (Windows Host 0.6.0+)

Legacy `command` packages with `activationMode: "submit"`, `outputMode: "panel"`, granted `ui.panel`, Windows-only `supportedPlatforms`, and `minimumHostVersion: "0.6.0"` or later may declare `panel.runtimeActions: true`. Absent or false preserves the existing API; null is invalid. Opt-in requires an exported `onPanelAction` function during Runtime readiness. CLI validates the declaration, not JavaScript exports. API v1 and historical version minima are unchanged.

Use capability detection: `if (window.uipilotPluginPanel.request) { ... }`. `await panel.request({action: "preview", payload: {...}})` invokes this panel's existing Runtime, whose `onPanelAction(invocation, api)` returns `{requestId: invocation.requestId, data: ...}`. The Promise resolves with frozen `data`; action replies do not call `onUpdate` or publish launcher results. Each invocation and its API are new and deeply frozen. Existing storage/settings/notification/optional HTTPS permissions and expiry checks apply. Direct content networking remains forbidden.

Action names match `^[a-z][a-z0-9.-]{0,63}$`. Payload is required (explicit null is allowed); extra envelope fields reject. Both directions accept finite JSON primitives, dense arrays and plain data records only. Cycles, accessors, symbols, undefined, functions, BigInt, nonfinite numbers and record keys `__proto__`, `prototype`, `constructor` reject without executing getters/toJSON. Each compact UTF-8 envelope is at most 64 KiB, nesting at most 32, and at most 4096 values. Reply ID must match exactly; no truncation.

One unresolved call and one physical Runtime slot per plugin are enforced by Host. Busy rejects rather than queueing/retrying. Do not await requests inside `onUpdate` or `onHostKey`; wait until Ready. Host command submission cancels action ownership but waits for the running handler to finish or its original 30-second deadline before dispatching another handler. Close, reload, replacement, disable, permission/credential changes and faults revoke the exact old API context. Already-admitted effects may finish: cancellation after save does not mean “not saved”; reload before deliberate retry.

Fixed Error.name values: `InvalidPanelRequestError`, `PanelRequestBusyError`, `ExpiredPanelSession`, `PanelRequestCancelledError`, `PanelRequestTimeoutError`, `InvalidPanelResponseError`, `PanelRequestFailedError`, `PanelRequestUnavailableError`. Business validation belongs in ordinary data (for example `{ok:false, code:"missing-variable"}`); unexpected handler errors expose no plugin stack or arbitrary native text. There is no multi-command panel, streaming, AbortSignal or new clipboard API. Window requests require the separate Host 0.9.0 opt-in below.


## Host 0.7.0：本地开发刷新、恢复和诊断

“刷新列表”只重新读取已安装清单。“重新载入开发目录”位于插件详情，需要重新选择 package 目录，将其复制、验证为不可变快照；它只接受当前插件 ID 和完全相同的规范版本。宿主不监听目录，也不记住源路径。准备后仍需确认完整权限、HTTPS 主机及凭据绑定。此前关闭且声明未变的网络访问不会自动开启。改版本（包括降级）请在列表使用“选择开发目录”或“选择插件包”。确认使用一次性令牌；尝试提交后需重新选择，不能重复提交旧确认。

刷新成功会关闭旧面板/窗口，请重新运行命令查看新代码。刷新保持停用/故障状态；修复代码后需显式“重新启用”。已经启用的插件再次启用不会重启健康 Runtime。静态验证或就绪失败保留旧包；只有验证了旧落盘字节的确定未提交情况才可继续使用旧配置。状态未确认时应重启宿主后核对；持久化后发布失败并不等于回滚。

插件包 semver、Host 版本、SDK API v1、插件私有数据 schemaVersion 相互独立。刷新不会主动清除插件数据；包替换/降级也不会转换或恢复私有 JSON。插件作者应验证完整数据文档，遇到未知/较新数据版本时阻止写入，不能静默重置。宿主没有自动数据迁移、包历史回滚或备份恢复功能。已获准的旧请求效果仍可能完成；取消或保存失败不能证明数据/远端效果未发生，重试前先重新读取。

详情中的“查看诊断”提供显式允许字段构造的 JSON 预览；“复制此诊断”仅复制当前预览快照，不自动更新、上传或读取剪贴板。报告含 Host/平台、插件 ID/版本、已确认配置状态及最新管理操作阶段/固定错误码，不含内容、路径、设置、私有数据、凭据、域名、请求 ID 或原始异常。状态 null 表示未知；enabled 不是 Runtime 健康证明。最新操作仅驻留内存，最多 64 个插件槽，重启清空。stateCommitOutcome 只描述 Host 配置持久化，不描述插件业务数据或远端效果。


## Host 0.8.0：普通设置编辑

在设置 → 插件 → 详情 → 普通设置中编辑 Manifest 声明的 text、number、boolean、select，然后显式“保存设置”。取消修改只丢弃草稿；未设置且未编辑的字段不会写入空字符串、false 或 0。数字遵循 min/max，step 仅作输入提示。只有已启用且无运行故障的插件可保存；保存后重新运行命令读取当前值。凭据继续使用独立密码操作。

保存失败不保证回滚，界面保留草稿并重新读取；无法读取时使用“重新读取”，核对实际值并取消修改后重新编辑，不要盲目重复保存。当前不提供删除覆盖值或恢复默认值协议。离线示例 [com.uipilot.demo-settings](../../examples/public-plugins/com.uipilot.demo-settings/README.md) 提供四类设置与同定义 1.0.0 → 1.0.1 兼容升级包生成脚本，便于人工核对重启、升级后的值保留。


## Window Runtime actions (Windows Host 0.9.0+)

Legacy `command` packages may declare `window.runtimeActions: true` with Windows-only `supportedPlatforms`, `activationMode: "submit"`, `outputMode: "window"`, a window entry, granted `ui.window`, and `minimumHostVersion: "0.9.0"` or later. Absent/false keeps the historical Window API with no `request` property; null and other values reject. Opt-in readiness requires an exported `onWindowAction` function; CLI performs static Manifest validation without executing Runtime. API v1, state schema and historical feature minima are unchanged.

```js
// Runtime: all business logic uses the existing request-bound API.
export async function onWindowAction(invocation, api) {
  const value = await api.storage.get('draft')
  return { requestId: invocation.requestId, data: value }
}
// Content: obtain the current capability when the user performs an action.
const request = window.uipilotPluginWindow.request
if (request) {
  const data = await request({ action: 'load', payload: null })
}
```

The action invocation contains exactly `apiVersion: 1`, `requestId`, `action`, `payload` and `context` (`platform: "windows"`, `theme`, RFC 3339 `invokedAt`). The handler returns exactly `{requestId, data}` with the same ID. `request` resolves to deeply frozen JSON data; replies do not trigger `onUpdate` or launcher results. Runtime invocation/API objects are fresh and frozen, with the existing storage/settings/notification/optional HTTPS permissions and expiry rules.

Window requests share Panel's bounded JSON contract: action names match `^[a-z][a-z0-9.-]{0,63}$` and reject `__proto__`, `prototype`, `constructor`; `payload` is required and may be null. Request/reply envelopes have exact fields, at most 64 KiB compact UTF-8, depth 32 and 4096 values. Only finite JSON primitives, dense arrays and plain data records are accepted. Accessors, toJSON execution, cycles, sparse arrays, symbols, functions, undefined, BigInt, nonfinite numbers and dangerous record keys reject. Host independently revalidates both directions.

The optional readonly `request` getter supplies a new function before each session's first `onUpdate`. Capture it in `onUpdate` if needed, but call only after that callback has returned and Host has activated the window. Calling during update/ACK returns Busy immediately; never await a request as part of `onUpdate`. A retained old function remains bound to its old full window owner, session and document, and expires after hide, submit replacement, navigation/reload, destruction or lifecycle revocation. Read the getter again for the new session. Internal bootstrap arguments cannot grant authority.

Each window permits one unresolved call and each plugin one physical Runtime handler. Busy rejects without queueing or retry. A new Host command cancels logical Window request ownership, then waits for the running physical handler to finish or its original 30-second deadline. Hide starts revocation immediately without waiting on the handler; hide failure does not revive ownership. Existing timer/storage leases and background timer completion keep their previous behavior. Disable, update, development reload, permission/settings/credential changes, faults and uninstall also revoke old contexts. Completion rechecks exact window identity and a single completion ticket before success. Already-admitted writes may finish; cancellation or failure is not proof that a save did not happen. Read current state before a deliberate retry.

Fixed `Error.name` values are `InvalidWindowRequestError`, `WindowRequestBusyError`, `ExpiredWindowSessionError`, `WindowRequestCancelledError`, `WindowRequestTimeoutError`, `InvalidWindowResponseError`, `WindowRequestFailedError`, `WindowRequestUnavailableError`. Business validation belongs in normal data such as `{ok:false, code:"revision-conflict"}`. Handler/native exception text and plugin stacks are hidden. There is no streaming, AbortSignal, automatic retry, arbitrary invoke or content networking capability. The private bridge is restricted to the actual plugin-window content WebView; main, shell, panel, runtime and find surfaces have no grant.


## Main-result actions (Windows Host 0.10.0+)

Declare `runtime.resultActions: true`, `minimumHostVersion: "0.10.0"` or later, exactly `["windows"]`, and only `mainResult` commands. Both `command` and `commands` support this opt-in. Absent/false preserves historical copy/hide behavior; null, unknown Runtime fields and incompatible declarations reject. Export callable `onCommand` and `onAction` even for a copy-only package. CLI validates statically and never evaluates JavaScript. API/schema v1, state schema 2 and historical feature minima remain unchanged.

Return `{requestId, results}`. An opted-in result is `{id, title, subtitle?, detail?, actions, defaultActionId?}`. `actions` has 0–8 entries: `{id,title,type:"copyText",text}` or `{id,title,type:"runtime",payload}`. Empty actions means informational. Omit `defaultActionId` to require menu selection; present values must identify one action. Null is invalid. `actions` cannot coexist with `defaultAction`, including null. Legacy rows may be mixed in an opted-in response; their copy action follows the same snapshot consumption policy. Undeclared packages keep the legacy parser.

Action IDs match `^[a-z][a-z0-9-]{0,31}$` and are unique per row. Business IDs such as `constructor` are valid string values. Action titles contain 1–80 Unicode scalars, at least one non-whitespace scalar, and no C0/C1 controls. Result IDs are unique across the complete response, nonempty, at most 128 UTF-8 bytes, and free of C0/C1 controls. Row title/subtitle/detail retain their current limits (256 scalars/512 scalars/16 KiB). Copy text allows empty strings, at most 16 KiB UTF-8, no NUL; publication and copying both require current declared/granted `clipboard.write`. Runtime payload is required and may be null. Storage adds no permission.

The exact-key response has at most 20 rows, 64 KiB compact UTF-8, depth 32 and 4096 total values (root depth zero and root counted). Only finite JSON primitives, dense arrays and plain data records are supported. Cycles, accessors, symbols, undefined/functions/BigInt, nonplain objects and record keys `__proto__`, `prototype`, `constructor` reject without invoking getters or toJSON. Unknown fields, malformed variants/defaults, duplicates or overflow reject the whole response; no truncation or stringification fallback. Host independently validates after IPC conversion; duplicate raw JSON keys cannot be recovered at that stage.

`onAction(invocation, api)` receives exactly `apiVersion:1`, a fresh scheduler `requestId`, saved normalized `commandId` (including `default` for legacy command), business `resultId`/`actionId`, immutable saved `payload` and original `input`, and `context:{platform:"windows",theme,invokedAt}` sampled at dispatch. Invocation and API are fresh and deeply frozen. Retained APIs from completed commands are expired. Return only a bounded `{requestId,results}` matching the new ID. Success replaces the complete source snapshot, including an empty result list, with fresh Host handles; there is no patch, retained-old mode, command retargeting, hide/open request or content `data` reply.

One source snapshot permits one admitted copy or Runtime action across all rows. Busy/pre-admission denial leaves it available; accepted success, failure, cancellation and uncertain transport consume it. Duplicate pending execution reports in-progress; replay reports already-executed. This is not durable exactly-once delivery. A fresh ordinary command or Host restart is new intent, so plugins own domain idempotency. Never automatically retry mutations. After an uncertain save, read current data before deliberate retry.

Runtime actions share the plugin's existing physical handler slot and 30-second deadline. They do not queue or interrupt JavaScript. A newer command retires old API authority and waits for the old handler's physical completion or deadline. Input edits, query/view replacement, hide/show and lifecycle/settings/permission changes revoke the source lease and block late publication and new API effects. Already-admitted effects may finish. Handler completion expires its API before parsing and uses one terminal ticket; malformed replies and handler throws have distinct fixed failures. Transport loss is settled once and the original autonomous deadline remains armed.

The main launcher exposes footer **动作 Ctrl+K**, right-click and a keyboard menu. Enter runs the explicit default, or opens the menu when there is no default. New copy actions keep the launcher open, show **已复制**, and disable the consumed source's actions. Admitted failure shows **操作未完成，请重新读取核对后再运行**. Stale source shows **结果已过期，请重新运行**. **重新运行** explicitly submits the ordinary command; it is never an automatic action retry. Selection, scrolling and theme changes alone do not cancel a source. Public result rows remain non-favoritable.

```js
export async function onCommand(invocation) {
  return { requestId: invocation.requestId, results: [{ id: 'text', title: invocation.input,
    actions: [{ id: 'copy', title: '复制', type: 'copyText', text: invocation.input },
      { id: 'save', title: '保存', type: 'runtime', payload: { text: invocation.input } }], defaultActionId: 'copy' }] }
}
export async function onAction(invocation, api) {
  if (invocation.commandId !== 'default' || invocation.resultId !== 'text' || invocation.actionId !== 'save') throw new Error('invalid action')
  // Validate domain payload and stored schema/revision before writing; never retry an uncertain write.
  await api.storage.set('latest', invocation.payload)
  return { requestId: invocation.requestId, results: [{ id: 'saved', title: '已保存', actions: [] }] }
}
```

Use **选择开发目录** with the `package` directory, or **选择插件包** with a produced `.uipilot-plugin` file. The project root is not an installable directory. Validate both forms with the fresh CLI target before manual installation.

## Host 0.12.0：显式打开浏览器链接

Windows 主结果插件可声明 `url.open`，同时使用 `minimumHostVersion: "0.12.0"`、`supportedPlatforms: ["windows"]`、`runtime.resultActions: true` 和 `mainResult` 命令。安装或更新必须确认完整权限；取消新增授权会保留旧插件和数据。即使只返回静态动作，也要导出 `onAction`。

```js
// 添加到 actions；默认仍设为 copy，只有用户选择 open 时才打开。
{ id: 'open', title: '在浏览器打开', type: 'openUrl', url: 'https://example.com/Case' }
```

Host 在发布和执行前严格校验：原文与规范化 URL 各限 4096 UTF-8 字节，必须显式以 HTTP(S):// 开始且紧接非空 authority，不允许凭据或 authority 中的 @、字面空白、C0/C1/DEL 控制字符及反斜杠。不会 trim 或修复非法输入；转义后的空白和控制字节仍可作为 URL 数据。Unicode 域名、正常转义、查询和片段受支持。具体规则见 [URL 动作合同](./public-plugin-v1.md#browser-url-result-actions-windows-host-0120)。

URL 只保存在 Host 私有快照，界面只接收动作标题与不透明句柄。整个快照使用的复制/URL 权限均须声明并授权；任何一项动作获准后，无论打开成功还是失败，所有兄弟动作都会消耗。成功显示“已请求打开链接”，表示 Windows 已接受请求；失败请显式重新运行，绝不自动重试。没有 Runtime 直接打开 API，也不会因为查询或返回结果而自动打开。浏览器自己的重定向、登录和网络策略由浏览器负责，`network.https` 不授权此动作。

参考 [文本片段 1.1](../../examples/public-plugins/com.uipilot.text-snippet/README.md)：只给合法 URL 的原文和最新片段加“在浏览器打开”，大写行保留复制/保存。先确认新权限，再输入 `https://example.com`，用 Ctrl+K 选择打开并核对一次浏览器请求；普通文本仍按原流程复制、保存和读取。


## User-selected text import (Windows Host 0.13.0+)

Declare `files.openText`, `ui.window`, exactly `["windows"]`, `minimumHostVersion >= 0.13.0`, and legacy `command` with `submit + window`. This permission does not require `window.runtimeActions`. Installation/update requires explicit permission consent; cancelling preserves the prior installation, configuration and data. API/Manifest v1 and persisted schemas stay unchanged.

Only opted-in Window content exposes optional readonly `window.uipilotPluginWindow.files`. Capture it in `onUpdate`, then call `files.openText()` with no arguments from an explicit user action after update/ACK. The frozen result is `{name, text, byteLength}` or `null` for cancellation. `name` is a basename, never a path. Maximum original file size is 8192 bytes including an optional leading UTF-8 BOM; the Host strips that BOM once, decodes strict UTF-8, rejects NUL and preserves other whitespace. Empty text and invalid JSON are allowed. The chooser accepts one ordinary local Windows file, with fixed text/all-file filters. No arbitrary path input, file writes, Runtime slot, persistent grant or handle is exposed.

One physical picker is allowed globally. Each facade captures its exact session/document; retained old functions and late success/cancel/error cannot borrow replacement authority. Explicit close, revocation and replacement remain nonblocking. Automatic blur hiding is suppressed only for the physical picker's document until completion; focus is restored only to its still-current authorized original shell.

Errors have sanitized names/messages: `FileOpenBusyError`, `ExpiredWindowSessionError`, `FileOpenPermissionError`, `FileOpenUnsupportedError`, `FileOpenTooLargeError`, `FileOpenEncodingError`, `FileOpenFailedError`. Passing any argument rejects `TypeError` before IPC. Host never logs the selected path, basename, text or native errors. Files outside ordinary local drives, reparse/device/directory/ADS targets and other platforms fail closed.

See [JSON workbench](../../examples/public-plugins/com.uipilot.json-workbench/README.md) for an explicit import/format/save workflow with edit/session ownership and inline dirty-draft confirmation. Native Windows behavior remains user-operated acceptance.

## Host 0.14.0：显式粘贴到原应用

Windows `mainResult` 插件可声明 `clipboard.paste` 与 `clipboard.write`，要求 `minimumHostVersion: "0.14.0"`、`supportedPlatforms: ["windows"]`、`runtime.resultActions: true` 并导出 `onAction`。安装或升级时用户须确认完整权限；取消新增授权会保留旧插件和数据。历史权限 `clipboard.history.paste` 不授权此动作。

```js
{ id: 'paste', title: '粘贴到原应用', type: 'pasteText', text: '你好，世界' }
```

文本为 1..8192 UTF-8 字节且不含 NUL，不 trim，保留中文和换行；额外字段、空值、过长或整响应超过 64 KiB 时整体拒绝。整快照同时校验两项权限，包括执行其复制/保存等兄弟动作和 Runtime 返回新结果。文本及窗口目标仅存在后端，前端只拿不透明句柄；没有 Runtime 任意按键或目标窗口 API。

与复制相比，粘贴会先写入剪贴板，然后隐藏 UiPilot，返回本次唤起前捕获的原应用并请求一次 Ctrl+V。请从目标编辑器唤起 UiPilot，再通过动作菜单主动选择“粘贴到原应用”，并松开触发键。成功“已请求粘贴”只表示 Windows 接受输入批次；不会确认目标已插入或自动保存文档。关闭目标、变更焦点/剪贴板或仍按住按键会阻止发送；不自动重试或重新显示抢焦点。写入后失败仍保留本次文本，绝不恢复覆盖后来复制的内容。所有兄弟动作一次消费；需核对目标后显式重新运行。

[文本片段 1.2](../../examples/public-plugins/com.uipilot.text-snippet/README.md) 保留 Enter 默认复制，在非空原文、大写和最新片段上增加次级粘贴。保存、URL、重读及 `snippet.latest` 记录合同保持。真实系统效果需用户从记事本自行验收；代理测试只使用注入 driver 和假存储。


## Host 0.15.0：主程序密钥业务服务

Host 0.15.0–0.17.x 的有道业务调用仅支持旧 `command` 形态的 `mainResult` 包，且 `runtime.resultActions` 必须省略或为 `false`。`commands` 多命令包、Panel、子窗口及启用结果动作的包均不能调用有道业务方法；即使权限已授予，也会拒绝 `InvalidServiceRequestError`。其他插件形态仍可声明和管理三项服务授权，以供未来能力使用。`api.services` 随 Runtime 网络能力提供，其存在不代表服务已授权，也不代表插件命令/输出形态兼容。百度 OCR 与 DeepSeek 仍无公开业务方法。


Windows Runtime 的 `api.services?.youdao.translate({text, from, to})` 使用宿主配置的有道翻译密钥；插件不接收密钥、签名、URL 或认证参数。声明 `credentials.youdaoTranslation`、`network.https`，最低版本 `0.15.0`，并声明 `network.httpsHosts: ["openapi.youdao.com"]`。用户在设置 → 权限单独授权，在设置 → 密钥配置中配置并启用服务；两者独立，关闭配置不会清除授权。

Host 0.15.0–0.17.x 仅允许 `en` → `zh-CHS` 或反向。text 不 trim，须为 1..8192 UTF-8 字节且不含 NUL。只接受三个自有可枚举数据属性，不执行 getter、不接受额外字段。Host 0.15.0–0.17.x 返回深冻结的 `{translations: string[]}`；facade 与原请求绑定，处理结束后保留的方法拒绝 `ExpiredRequestError`，不能借用后续调用。

固定错误名：`InvalidServiceRequestError`、`ServicePermissionDeniedError`、`CredentialNotConfiguredError`、`CredentialDisabledError`、`CredentialUnavailableError`、`PermissionDeniedError`、`NetworkTargetDeniedError`、`NetworkTimeoutError`、`NetworkFailureError`、`NetworkResponseTooLargeError`、`NetworkResponseInvalidError`、`NetworkLimitExceededError`、`ExpiredRequestError`、`ProviderAuthenticationError`、`ProviderQuotaError`、`ProviderFailureError`。错误不包含真实凭据或供应商原始响应。

`credentials.baiduOcr` 和 `credentials.deepseek` 是独立的默认关闭授权，目前没有公开业务 API，也不能替代有道授权。声明服务权限仅支持 Windows，最低 Host 为 0.15.0。翻译示例申报三项服务权限，但仅调用有道服务。

## Panel 操作提示

Host 0.16.0 起，Panel 保留宿主底部状态栏。调用 `uipilotPluginPanel.setStatus({ message: "已保存", tone: "success" })` 显示反馈，`tone: "error"` 显示失败，空消息清除。成功清除计时器应绑定操作修订号；不要另建面板内提示气泡。SDK 的方法绑定当前面板会话，异步任务应保存方法引用并遵守自身的会话取消规则。


### Panel 动态操作（Host 0.16.0+）

使用 `uipilotPluginPanel.onActions(({ source }) => ({ context, global }))` 注册一个同步提供器，返回解除注册函数。source 为 hostInput/content，提供器在菜单抢焦点前调用，可据 document.activeElement 区分列表和编辑框。对话框打开时返回 null。

每组最多 12 项；每项是 id/title/run，可选 shortcut/icon/disabledReason。id 唯一且匹配 `[a-z][a-z0-9-]{0,63}`，host- 前缀保留；title、shortcut、disabledReason 上限分别 80、32、160 个 Unicode 字符且不得含控制符。图标为 SDK 枚举，禁用项有原因且不会执行。run 在原本焦点恢复后调用，执行前须再次核对捕获的笔记/对象和会话。菜单关闭或 Panel 更新后的旧回调自动作废。权限仍由原 API 检查，此注册不增加系统权限。

宿主为主输入框补充搜索动作，内容区才补充“返回主输入框”（Ctrl+F）。插件自行提供返回动作时也应仅在 source 为 content 时返回，避免主输入框中出现多余的返回操作；该动作使用单次票据，输入或会话变更后失效。菜单只显示分组与操作项，不显示点击、方向键说明；Esc/Ctrl+K 关闭菜单，原有功能快捷键保持不变。

### Panel Delete Host Key（Host 0.16.0+）

`panel.hostKeys` 新增 `Delete`，规范排序位于 `Enter` 之后；声明它需要 `minimumHostVersion >= 0.16.0`。SDK 的 `PluginPanelHostKeyEvent.key` 相应增加 `Delete`。

仅对声明此键的当前 Panel，在主输入框为空、无 Ctrl/Meta/Shift/Alt、非 IME composing 且非重复 keydown 时转发。主输入框含文字时保留原生 Delete 编辑行为。Ctrl+K 菜单拥有输入时不投递给背后列表。既有串行投递、会话/序号、ack、超时和焦点策略保持不变，Delete 不授予 Enter 粘贴票据。

`onActions` 的 `shortcut` 仍只负责展示，不注册快捷键；插件需要声明 hostKeys 并通过 onHostKey 执行相同动作。内容区直接处理 Delete 时同样保护可编辑控件、IME、修饰键、长按及已消费事件。删除回调必须捕获并复核选中对象/会话，进行中禁用重复操作。不可用时用 disabledReason 解释，反馈用 setStatus。

### Clipboard history search and copy (Windows Host 0.17.0+)

`clipboardHistory.list(input?: { query?: string })` accepts only the optional `query` field. No argument and empty/whitespace queries return all summaries. Queries are limited to 512 Unicode characters before trimming; invalid input rejects with `TypeError`. The Host matches a case-insensitive contiguous substring against the complete text or any file name (including extension, excluding parent directories). Nonempty queries exclude images. Results preserve recent-first ordering and the history revision. A filtered result never replaces the unfiltered `onChanged` snapshot cache; when history changes, query again and discard stale requests using the Panel session and a request generation.

`clipboardHistory.copy({ id })` requires declared and granted `clipboard.history.read` plus `clipboard.history.copy`, exact Windows support, `submit + panel + ui.panel`, and `minimumHostVersion >= 0.17.0`. New copy permission is never implicitly granted by an upgrade. It uses the current active Panel session and resolves to `{ outcome: 'copied' }` only after the Host writes the complete text, original image, or file list to the system clipboard. It preserves the Panel and focus and does not simulate paste. Unknown fields are rejected. Stable errors are `PermissionDenied`, `ExpiredPanelSession`, `RecordNotFound`, `RecordUnavailable`, and `ClipboardWriteFailed`; error messages are redacted. There is no retry and no routed Enter ticket requirement. The existing `paste` API and permission retain their behavior.


### Translation Panel and detection (Windows Host 0.18.0+)

The Youdao service keeps legacy single-command `mainResult` support (without
`runtime.resultActions`). It additionally admits a legacy `command` with
`submit + panel`, `panel.runtimeActions: true`, declared and granted `ui.panel`,
and `minimumHostVersion >= 0.18.0`, only from its current `onPanelAction` Runtime
request. Multi-command, Window and result-action packages remain unsupported.
The content Panel uses its request bridge; it does not contact Youdao directly.

The fixed supported languages are English (`en`) and Simplified Chinese
(`zh-CHS`). Source additionally accepts `auto`; target never accepts `auto`.
Auto and explicit same-language inputs require `minimumHostVersion >= 0.18.0`.
The existing bounded input and independent `network.https`,
`credentials.youdaoTranslation`, HTTPS host allowlist and enabled Host credential
checks still apply. An explicit same-language call returns the original text
without provider transport after all admission checks; it cannot bypass revoked
permissions, a stale request or disabled credentials.

The frozen response is `{translations, sourceLanguage, targetLanguage, unchanged}`.
`sourceLanguage` is `en`, `zh-CHS` or `null`, based only on the provider `l` pair;
unknown/unsupported detection is never guessed from text. Host requests
`strict=true`, normalizes language spelling and rejects an actual target that
mismatches the requested target with `NetworkResponseInvalidError`. A missing
legacy provider language field yields `sourceLanguage: null`. The target is
always the requested supported language. `unchanged` is true for explicit
same-language calls or provider detection equal to target. Existing `translations`
remains available, and fixed service error names stay unchanged.

`selection.read` also admits this authorized Panel shape on Host 0.18.0+. The
Host still owns explicit selection capture and restoration, passes the captured
text as this Panel invocation's input, and invalidates late capture after intent,
plugin generation or permission changes. Failed captures never translate stale
clipboard or Panel text. The previous mainResult entry remains supported.
