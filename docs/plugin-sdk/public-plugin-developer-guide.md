# UiPilot 第三方插件开发教程

本教程面向第一次为 UiPilot 编写公开插件的开发者。示例使用原生 JavaScript，不需要前端框架或构建工具，并覆盖两种 MVP 插件：

- **主界面结果型**：处理命令后在 UiPilot 主界面显示结果，用户再次按 Enter 执行复制。
- **单例子窗口型**：处理命令后打开一个由 UiPilot 托管的子窗口。

仓库中的完整参考实现：

- [`com.uipilot.demo-return`](../../examples/public-plugins/com.uipilot.demo-return)：主界面结果与复制。
- [`com.uipilot.demo-win`](../../examples/public-plugins/com.uipilot.demo-win)：单例子窗口。
- [`com.uipilot.pomodoro`](../../examples/public-plugins/com.uipilot.pomodoro)：窗口隐藏后仍由宿主计时的番茄钟。

完整字段、上限和安全合同见 [`Public Plugin API v1`](./public-plugin-v1.md)、[`plugin.json` Schema](./uipilot-plugin-v1.schema.json) 和 [TypeScript API 类型](./uipilot-plugin-api-v1.d.ts)。

## 1. 选择插件类型

| 需求 | `activationMode` | `outputMode` | 权限 | 参考 Demo |
| --- | --- | --- | --- | --- |
| 按 Enter 后在主界面显示结果 | `submit` | `mainResult` | 复制时使用 `clipboard.write` | `demo-return` |
| 按 Enter 后打开子窗口并延迟发布消息 | `submit` | `window` | `ui.window`、`notifications.publish`（仅 Windows） | `demo-win` |
| 子窗口控制宿主持有的单计时器 | `submit` | `window` | `ui.window`、`notifications.publish`、`timer.control`（仅 Windows） | `pomodoro` |
| 输入时立即计算并预览 | `live` | `mainResult` | 按结果动作决定 | 无独立 Demo |

MVP 中每个插件只能注册一个启动名称。用户可以在 UiPilot 设置中修改该名称，所以 Runtime 不应硬编码 `/命令名`。

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
| `command.inputPlaceholder` | 命令补全后的用法提示 | `请输入内容后回车` |

公共规则：

- `pluginId` 是永久身份，使用自己的反向域名，例如 `com.example.hello-return`。
- `version` 和 `minimumHostVersion` 必须是 `major.minor.patch`。
- `command.defaultName` 不包含 `/`，只能有一个。
- `runtime.entry` 必须指向包内 JavaScript 文件。
- 当前真正可用的权限只有 `clipboard.write`、`ui.window`，以及仅 Windows 可用的 `notifications.publish` 和 `timer.control`。
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

### 6.3 使用宿主主题

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

### 6.4 使用宿主持有的窗口计时器（仅 Windows）

需要窗口隐藏后继续计时时，Manifest 必须同时声明 `ui.window`、`notifications.publish` 和
`timer.control`，并使用 `submit + window`。三项权限必须在安装时全部确认；`timer.control` 不能用于
Runtime，也不能脱离插件窗口调用。完整参考实现见
[`com.uipilot.pomodoro`](../../examples/public-plugins/com.uipilot.pomodoro)。

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
  durationMs: 10_000,
  completionMessage: '番茄钟完成',
})
await timer.stop()  // running -> paused；再次调用幂等
await timer.start() // paused -> running；恢复时不能再传 input
await timer.reset() // 回到 idle，保留显示时长但不自动开始
```

`durationMs` 是 `1_000..=86_400_000` 的 JavaScript 安全整数。`completionMessage` 是 1 到 500 个
Unicode 标量值的纯文本。首次 `idle` 的 `durationMs / remainingMs` 都是 `null`；示例页面自己显示
`00:10`，用户点击 Start 后宿主才冻结输入。`running` 时再次无参 Start、以及 idle/paused/fired 的幂等
操作都返回当前权威状态；需要 input 时省略会得到 `TimerInputRequired`，不允许 input 时传入会得到
`TimerInputNotAllowed`。

状态的 `timerRevision` 是规范十进制 `u64` 字符串，不能转成 JavaScript `number`，也不能直接按字符串
比较。应先比较长度，再在长度相等时按字典序比较。较低 revision 必须丢弃；相同 revision 通常丢弃，
只有当前会话最新一次 `getState()` 返回的 running 状态，且 phase 和 duration 都相同，才可刷新本地
remaining 锚点。页面可以用 `performance.now()` 插值显示数字，但本地 interval/animation frame 不能拥有
到期、消息或后台副作用。

到期时宿主先把完成消息原子保存到消息中心，成功后才进入 `fired` 并尝试播放一次有限音效。消息保存
失败会回到 `idle`，不响铃、不重试；系统通知或音频失败不删除已保存消息。禁用、卸载、故障停用或成功
升级会取消当前 generation 的计时器。退出 UiPilot 会丢弃所有计时器且下次启动不恢复、不补发。

常见错误包括 `PermissionDenied`、`ExpiredWindowSessionError`、`InvalidTimerInput`、
`TimerInputRequired`、`TimerInputNotAllowed`、`MessageStoreUnavailable` 和 `TimerUnavailable`。

## 7. 添加插件图标

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

## 8. 编写 Runtime 测试

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

还应针对自己的业务结果添加精确断言。子窗口型插件应额外检查 `window.js` 使用 `window.uipilotPluginWindow.onUpdate`，且不包含 `invoke(`、`fetch(`、`WebSocket` 或窗口置顶逻辑。

仓库参考测试：

```powershell
node --test examples/public-plugins/com.uipilot.demo-return/tests/runtime.test.js
node --test examples/public-plugins/com.uipilot.demo-win/tests/runtime.test.js
node --test examples/public-plugins/com.uipilot.pomodoro/tests/runtime.test.js
```

## 9. 使用开发目录安装

1. 启动 UiPilot。
2. 打开 **设置 > 插件 > 公开插件**。
3. 点击“选择开发目录”。
4. 选择插件的 `package` 目录。
5. 检查插件名称、版本、图标和权限。
6. 点击“确认安装”。
7. 在主界面输入 `/` 加启动名称测试插件。

用户可能修改启动名称。实际生效名称以 UiPilot 设置页为准，不以 `defaultName` 永久锁定。

修改插件后：

1. 提高 `plugin.json` 的 `version`。
2. 再次选择同一个开发目录。
3. 确认更新。

更新会在 Runtime 通过 ready 校验后原子生效。更新失败时，当前已安装版本保持可用。

## 10. 打包 `.uipilot-plugin`

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

## 11. 可选：插件私有状态与设置

Runtime API 还提供请求期内的插件隔离存储和设置读取：

```js
const previous = await api.storage.get('last-value')
await api.storage.set('last-value', invocation.input)
await api.storage.remove('last-value')

const prefix = await api.settings.get('prefix')
const hasToken = await api.settings.isSecretConfigured('token')
```

存储只能保存 JSON。插件不能读取 secret 明文。设置字段定义、类型和限制见完整 API 文档与 Schema。

## 12. 常见问题

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

检查插件是否已启用、是否有运行故障，以及用户是否在设置页修改了启动名称。

### 结果出现但不能复制

Manifest 必须声明 `clipboard.write`，安装时用户必须授权，结果还必须包含唯一的 `copyText` 默认动作。

### 子窗口没有内容

确认窗口脚本尽早注册了 `window.uipilotPluginWindow.onUpdate`，并且回调能在五秒内完成。不要在内容页调用 Tauri、Shell、网络或其他宿主能力。

## 13. MVP 边界

当前不支持：

- 一个插件注册多个命令或多个窗口。
- 插件代码长期后台运行、插件自建计时器或任意后台回调；单轮窗口计时必须交给宿主的 `timer.control`，延迟纯文本消息交给 `notifications.schedule()`。
- 网络、任意文件、原生二进制和 Shell。
- 输入模拟或控制鼠标键盘。
- 多动作结果、自定义动作回调、HTML/Markdown 结果。
- 插件间通信、远程资源、依赖安装、市场发布和自动更新。

遇到未覆盖的需求时，不要通过 WebView 或 Tauri 私有对象绕过限制；应等待宿主公开相应版本化 API。

## 14. 发布前检查清单

- [ ] `pluginId` 使用自己的稳定命名空间。
- [ ] `version` 已提高，`minimumHostVersion` 合理。
- [ ] `description`、`summary`、`inputPlaceholder` 各司其职。
- [ ] `outputMode`、权限和 `window` 入口组合正确。
- [ ] Runtime 始终原样返回 `requestId`。
- [ ] 使用消息能力时，仅在 Windows Manifest 中声明并授权 `notifications.publish`，且每个请求只提交一次 `publish()` 或 `schedule()`。
- [ ] 使用窗口计时时，同时声明 `ui.window`、`notifications.publish`、`timer.control`，并按十进制字符串 revision 合并状态。
- [ ] 内部空格不会被插件意外压缩。
- [ ] 子窗口使用宿主 CSS 变量并只通过 `onUpdate` 接收数据。
- [ ] `icon.png` 满足固定规则。
- [ ] Runtime 测试通过。
- [ ] 开发目录和最终 `.uipilot-plugin` 都完成安装验收。
