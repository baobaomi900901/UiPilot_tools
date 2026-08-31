# UiPilot 第三方插件开发教程

本教程面向第一次为 UiPilot 编写公开插件的开发者。示例使用原生 JavaScript，不需要前端框架或构建工具，并覆盖三种 MVP 插件：

- **主界面结果型**：处理命令后在 UiPilot 主界面显示结果，用户再次按 Enter 执行复制。
- **单例子窗口型**：处理命令后打开一个由 UiPilot 托管的子窗口。
- **启动器面板型**：处理命令后在主启动器内挂载插件面板（基础能力需宿主 `0.3.0+`，Host 按键与主动隐藏需 `0.3.1+`，扩展 `Tab` / `Shift+Tab` / `Enter` 与剪贴板历史需 `0.3.3+`，显式 Host Key 焦点策略需 `0.3.4+`）。

三种插件的 Runtime 都可以按需使用 Windows Host `0.3.2+` 提供的受控 HTTPS 能力。网络请求由宿主代理执行，不会向 Runtime 或内容 WebView 开放浏览器 `fetch`。

仓库中的完整参考实现：

- [`com.uipilot.demo-return`](../../examples/public-plugins/com.uipilot.demo-return)：主界面结果与复制。
- [`com.uipilot.demo-win`](../../examples/public-plugins/com.uipilot.demo-win)：单例子窗口。
- [`com.uipilot.demo-panel`](../../examples/public-plugins/com.uipilot.demo-panel)：启动器内嵌面板。
- [`com.uipilot.demo-http`](../../examples/public-plugins/com.uipilot.demo-http)：Host 托管 HTTPS 请求。
- [`com.uipilot.pomodoro`](../../examples/public-plugins/com.uipilot.pomodoro)：窗口隐藏后仍由宿主计时的番茄钟。

完整字段、上限和安全合同见 [`Public Plugin API v1`](./public-plugin-v1.md)、[`plugin.json` Schema](./uipilot-plugin-v1.schema.json) 和 [TypeScript API 类型](./uipilot-plugin-api-v1.d.ts)。

## 1. 选择插件类型

| 需求 | `activationMode` | `outputMode` | 权限 | 参考 Demo |
| --- | --- | --- | --- | --- |
| 按 Enter 后在主界面显示结果 | `submit` | `mainResult` | 复制时使用 `clipboard.write` | `demo-return` |
| 按 Enter 后打开子窗口并延迟发布消息 | `submit` | `window` | `ui.window`、`notifications.publish`（仅 Windows） | `demo-win` |
| 子窗口控制宿主持有的单计时器 | `submit` | `window` | `ui.window`、`notifications.publish`、`timer.control`（仅 Windows） | `pomodoro` |
| 在启动器内挂载面板并提交参数 | `submit` | `panel` | `ui.panel`（使用基础 `hostKeys` 时 `minimumHostVersion` ≥ `0.3.1`；使用 `Tab` / `Shift+Tab` / `Enter` 或剪贴板历史时 ≥ `0.3.3`） | `demo-panel` |
| 在面板中显示宿主管理的剪贴板历史摘要并粘贴一次 | `submit` | `panel` | `ui.panel`、`clipboard.history.read`、可选 `clipboard.history.paste`（仅 Windows，`minimumHostVersion` ≥ `0.3.3`） | 暂无产品 Demo |
| 在 Runtime 中请求声明的 HTTPS 服务 | 任意现有模式 | 任意现有输出 | `network.https`（仅 Windows，`minimumHostVersion` ≥ `0.3.2`） | `demo-http` |
| 输入时立即计算并预览 | `live` | `mainResult` | 按结果动作决定 | 无独立 Demo |

MVP 中每个插件只能注册一个启动键（Manifest 字段是 `command.defaultName`）。用户可以在 UiPilot 设置中修改该启动键，所以 Runtime 不应硬编码 `/命令名`。

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
- `command.defaultName` 不包含 `/`，只能有一个。
- `runtime.entry` 必须指向包内 JavaScript 文件。
- 当前真正可用的权限只有 `clipboard.write`、`ui.window`、`ui.panel`，以及仅 Windows 可用的 `network.https`、`notifications.publish`、`timer.control`、`clipboard.history.read` 和 `clipboard.history.paste`。`clipboard.read` 仍是保留权限，不等同于剪贴板历史。
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
        authorization: 'Bearer development-only-token',
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

开发阶段可以暂时把测试凭据打进插件包，但插件包和 Runtime 源码都可被用户检查，不能视为安全存储。当前 `secret` 设置只允许 Runtime 查询 `isSecretConfigured()`，不能读取明文，也不能把 secret 注入网络请求；正式生产密钥消费能力留待后续独立 Host 合同，MVP 不提供变通接口。

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

非空 `panel.hostKeys` 要求页面在 ready 前恰好注册一次 `onHostKey(handler)`。声明只允许 `ArrowDown`、`ArrowUp`、`Primary+N`、`Tab`、`Shift+Tab`、`Enter`，并按 `ArrowDown < ArrowUp < Primary+N < Tab < Shift+Tab < Enter` 规范化。方向键和 `Tab` 只匹配无修饰键；`Shift+Tab` 只匹配仅带 Shift 的 Tab；`Enter` 只匹配无修饰键且非 IME composing；Windows 的 `Primary+N` 只匹配 Ctrl+N，macOS 只匹配 Meta+N。未声明按键、普通字符和其他组合不会路由。handler 串行执行；抛错或拒绝会 ack 但不重试，超过 2 秒未完成会隐藏并销毁会话。调用 unsubscribe 也会结束会话。

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

UiPilot 提供纯 TypeScript 的 `@uipilot/plugin-cli`。第三方开发者只需要 Node.js 20 或更高版本和已提供的 npm `.tgz`，不需要安装 Rust、UiPilot 或下载 UiPilot 源码。CLI 只读取并验证包，不安装插件、不运行 Runtime/窗口代码、不联网，也不修改源目录。

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

使用 `network.https` 时，CLI 只验证 Manifest 的 Windows 平台、`minimumHostVersion >= 0.3.2`、权限配对和精确主机语法；CLI 自身不会联网，也不会验证第三方服务或凭据。

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

更新会在 Runtime 通过 ready 校验后原子生效。更新失败时，当前已安装版本保持可用。

## 12. 打包 `.uipilot-plugin`

`.uipilot-plugin` 是 ZIP 内容，归档根目录必须直接包含 `plugin.json`，不能再包一层 `package/`。

在插件项目根目录运行：

```powershell
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$packageRoot = (Resolve-Path '.\package').Path
$output = Join-Path $PWD 'com.example.hello-return.uipilot-plugin'
$temporary = "$output.$([Guid]::NewGuid().ToString('N')).tmp"

try {
  [System.IO.Compression.ZipFile]::CreateFromDirectory(
    $packageRoot,
    $temporary,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false
  )
  Move-Item -LiteralPath $temporary -Destination $output -Force
} finally {
  if (Test-Path -LiteralPath $temporary) {
    Remove-Item -LiteralPath $temporary
  }
}

$output
```

把输出文件名改成自己的 `pluginId`。在发布前，用 UiPilot 的“选择插件包”重新安装该文件做最终验证。

仓库内固定输出 Demo 可使用现有脚本打包；番茄钟示例可直接选择其 `package` 开发目录安装：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-return
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-win
```

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

Manifest 必须声明 `clipboard.write`，安装时用户必须授权，结果还必须包含唯一的 `copyText` 默认动作。

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

- 一个插件注册多个命令或多个窗口。
- 插件代码长期后台运行、插件自建计时器或任意后台回调；单轮窗口计时必须交给宿主的 `timer.control`，延迟纯文本消息交给 `notifications.schedule()`。
- 浏览器/WebView 网络、原始套接字、任意文件、原生二进制和 Shell；外部访问仅限 Runtime 请求期内的 Host 托管 HTTPS。
- 输入模拟或控制鼠标键盘。
- 多动作结果、自定义动作回调、HTML/Markdown 结果。
- 插件间通信、远程资源、依赖安装、市场发布和自动更新。

遇到未覆盖的需求时，不要通过 WebView 或 Tauri 私有对象绕过限制；应等待宿主公开相应版本化 API。

## 16. 发布前检查清单

- [ ] `pluginId` 使用自己的稳定命名空间。
- [ ] `version` 已提高，`minimumHostVersion` 合理。
- [ ] `description`、`summary`、`inputPlaceholder` 各司其职。
- [ ] `outputMode`、权限和 `window` / `panel` 入口组合正确；基础面板要求 Windows 与宿主 `0.3.0+`，Host 按键与主动隐藏要求 `0.3.1+`，扩展 Host 按键和剪贴板历史要求 `0.3.3+`，显式 Host Key 焦点策略要求 `0.3.4+`。
- [ ] 使用网络时，仅面向 Windows Host `0.3.2+`，同时声明 `network.https` 与完整精确的 `network.httpsHosts`，并处理 `api.network` 缺失、HTTP 非成功状态和九种固定错误名。
- [ ] 使用剪贴板历史时，仅面向 Windows Host `0.3.3+`，同时声明 `ui.panel` 和 `clipboard.history.read`，需要粘贴时再声明 `clipboard.history.paste`，并处理六种固定 `Error.name`。
- [ ] 网络插件未记录凭据、签名字段或完整请求体；了解内置测试凭据可被检查，正式 secret 消费尚未开放。
- [ ] Runtime 始终原样返回 `requestId`。
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
