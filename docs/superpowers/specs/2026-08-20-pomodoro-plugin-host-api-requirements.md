# 番茄钟公开插件：需求拆解与宿主 API 缺口评估

## 1. 文档信息

- 日期：2026-08-20
- 状态：需求评估修订稿。现有公开插件 API v1 **没有实现错误**；缺口是尚未交付的新能力。复核提出的 P1（claim 不得持锁跨磁盘 I/O）与 P2（`timerRevision` 跨语言表示）已在第 8.3、8.4 节冻结。本文仍不是已批准的 API 设计，不冻结最终 DTO 字段名、权限字符串或 Schema 形状。
- 读者：UiPilot 主程序（宿主）开发者
- 产品问题：第三方能否用当前公开插件 API 做出可用的番茄钟插件；若不能，宿主要新增哪些能力，文档要先冻结哪些语义
- 目标平台基线：Windows 11 x64；公开合同预留 macOS
- 技术基线：Tauri 2、Rust、TypeScript、WebView2
- 对照实现：`com.uipilot.demo-win`（`submit + window` + 请求内 `notifications.schedule()`）

相关合同：

- [公开插件 API v1](../../plugin-sdk/public-plugin-v1.md)
- [第三方插件开发教程](../../plugin-sdk/public-plugin-developer-guide.md)
- [公开插件命令与单窗口 MVP](./2026-08-13-public-plugin-command-window-mvp-design.md)
- [公开插件消息中心与 Windows 通知](./2026-08-18-public-plugin-message-center-design.md)
- [公开插件宿主延迟消息](./2026-08-19-public-plugin-delayed-message-design.md)

## 2. 结论与最终边界

现有主程序 API 没有实现番茄钟所需能力，**并不代表现有 API 有 Bug**。`notifications.schedule()` 按请求级延迟消息的既定合同工作；窗口内容页只有 `onUpdate()` 也是单窗口 MVP 的既定合同。另一部分问题出在评估文档自身：停止语义、窗口会话、竞态和失败规则此前没有写死。

最终边界：

- **主程序需要新增五块实际能力：** 窗口计时桥、计时器服务、状态同步、到期消息入口、宿主闹铃。权限、TypeScript 类型、Schema、Capability 和开发者文档随这五块一起加，不单独算第六套运行时。
- **不修改** 已发布的 `notifications.schedule()` 语义。番茄钟另立能力，避免破坏请求级 API 心智和兼容性。
- **文档必须先冻结：** 状态机、窗口控制会话、Stop/claim 的 `ClaimTicket` 线性化（锁不跨越消息存储 I/O）、`timerRevision` 规范十进制字符串比较、闹铃 `AudioTicket`、睡眠与时钟、开始时冻结的数据。DTO 可在后续设计规格里定稿。

[命令与单窗口 MVP](./2026-08-13-public-plugin-command-window-mvp-design.md) 已把「后台持久化定时任务」列为后续独立阶段；[延迟消息设计](./2026-08-19-public-plugin-delayed-message-design.md) 的非目标包含番茄钟产品功能和已接管任务的查询/取消/修改。本文评估的是一次新的宿主能力阶段，不是对 v1 的缺陷修复。

## 3. 目标用户合同

开发测试阶段时长固定为 10 秒，用于快速验收。正式产品把 10 秒换成 25 分钟不改变能力形状，只改变延迟参数。

1. 用户在主窗口输入有效启动名称（示例 `/pomodoro`，用户可在设置中改名）并按 Enter。
2. 主窗口隐藏，UiPilot 打开或复用该插件唯一子窗口。
3. 子窗口显示倒计时，测试默认 `00:10`，其下有：
   - 开始计时 / 停止计时
   - 重新计时
4. 用户点击「开始计时」后，显示从 `00:10` 变为 `00:09`… 开始递减。
5. 用户关闭番茄钟子窗口后，倒计时继续；到期时向用户投递一条完成消息，并播放闹铃。

当前 v1 **已经能满足**第 1–3 步，以及第 4 步之前的静止态：打开窗口后保持 `00:10`，**不**自动开始，**不**因 Enter 登记到期消息。缺口从用户点击「开始」之后才出现。

工作假设（完成提示）：到期后走现有消息中心路径（原子写入消息、Windows 通知、托盘提醒、未读徽标），不重新打开 launcher，不向主结果区插入一行。

工作假设（进程退出）：与延迟消息相同，等待中的计时器丢弃且下次启动不恢复。跨重启恢复不在本阶段。

## 4. 从需求拆出的能力

每条能力回答：用户看见什么、谁必须拥有时钟/副作用、现有 v1 是否覆盖。此处「不够」表示**能力未提供**，不是现有接口实现错误。

| ID | 能力 | 用户可见行为 | 必须由谁拥有 | v1 |
| --- | --- | --- | --- | --- |
| C1 | 斜杠命令提交 | 输入 `/pomodoro` 后 Enter 才运行 | 宿主命令路由 | 够用。`submit` |
| C2 | 打开单例子窗口并隐藏主窗口 | 主界面消失，出现插件窗口 | 宿主窗口外壳 | 够用。`outputMode: window` + `ui.window` |
| C3 | 窗口内倒计时 UI | 显示 `00:10` 和三个按钮 | 插件内容页渲染 UI | 够用。HTML/JS/CSS。内容页 JS 本来就要继续处理 UI |
| C4 | 打开窗口之后才开始计时 | 点「开始」才进入递减 | 宿主时钟；内容页只发意图 | 未提供。唯一延迟 API 只能在 `onCommand` 里调用 |
| C5 | 停止计时 | 数字停下，可再开始；未到期不发消息/闹铃 | 宿主暂停并保留 remaining | 未提供。`schedule()` 不可查询、不可取消 |
| C6 | 重新计时 | 回到设定初值且不自动开始；旧任务作废 | 宿主取消未到期任务并回到 idle | 未提供 |
| C7 | 关闭窗口后计时继续 | 子窗口关闭，已开始的倒计时仍走 | 宿主持有任务，不依赖内容页保活 | 未提供该产品形状。隐藏不取消 `schedule()`，但 `schedule()` 绑在 Enter 上 |
| C8 | 到期投递完成消息 | 消息中心出现纯文本 | 宿主消息中心 | 投递面可用，触发面绑在有效 `onCommand` 或预先 `schedule()` |
| C9 | 到期播放声音 | 宿主固定闹铃 | 宿主音频 | 未提供。公开插件没有音频能力 |
| C10 | 生命周期联动 | 禁用/卸载/成功升级后不再计时、不闹铃 | 宿主 generation 绑定 | 延迟消息已有同类取消；新计时器必须同时取消计时器并停止尚未结束的闹铃 |
| C11 | 计时状态同步 | 再打开窗口看到真实剩余时间 | 宿主快照/订阅 | 未提供。内容页只能收到新命令的 `PluginWindowUpdate` |

C1–C3 是现有合同已覆盖的外壳。C4–C11 是新能力。

### 4.1 关闭子窗口后，倒计时还在吗？

不需要一直开着插件子窗口。窗口是控制面和投影；已经开始的时钟必须在宿主。

| 对象 | 关窗后 | 说明 |
| --- | --- | --- |
| C3 倒计时 UI | 用户看不见 | 数字和按钮在内容页上。内容页 JS 可以继续画 UI，但不能当权威服务。 |
| C4/C5/C6 控制 API | 必须失效 | 见第 8.2 节会话世代。隐藏后的页面不得偷偷 Start。 |
| 已经开始的宿主计时（C7） | 必须仍在 | 关窗 = 藏 UI，不等于取消闹钟。 |

当前 v1 没有独立倒计时服务。若只用内容页 `setInterval`，关窗后没有可靠的到期消息和闹铃。宿主把关闭转成隐藏、WebView 可能未销毁，这是窗口外壳实现，不是「插件可在后台执行」的公开合同。

## 5. 现有 API：按设计如此，不是 Bug

### 5.1 文档事实修正

以下内容主程序已经实现正确，此前评估文稿写错或写漏：

1. **消息正文。** 合法性用 `trim()` 判断是否为空，并拒绝控制字符、换行以及超过 500 个 Unicode 标量值。通过校验后保存的是**合法原文**，不会裁掉首尾空白。公开 API 文稿里「去除首尾空白后不变」的表述不能理解成「入库前改写正文」。
2. **请求失效条件。** `ExpiredRequestError` 不只是超时、完成、被新请求淘汰、卸载和升级。已签发请求还会因**禁用、故障停用、reload/replacement、改名、保存设置**而失效。迟到响应仍不得改写更新的 UI 或数据。
3. **Enter 后的静止态。** 当前 v1 可以满足「打开窗口后保持 `00:10` 且不自动开始」。缺口从用户点击「开始」之后才出现。不得把「Enter 没有自动 `schedule()`」写成现有实现缺陷。
4. **插件 JS 边界。** 正确表述是：禁止 **Runtime 请求代码**在请求结束后继续产生副作用（不得把请求期 `api` 交给定时器，不得在 `onCommand` 返回后 `publish`/`schedule`）。插件窗口内容 JS 本来就需要继续处理 UI，包括点击按钮和本地绘制。内容页没有权威时钟，也不等于允许隐藏页面充当后台服务。

### 5.2 命令与窗口外壳

清单可直接使用 `submit + window` 与 `ui.window`。Runtime 返回 `{ requestId, data }` 后，宿主打开或复用单例窗口，发送 `PluginWindowUpdate`。图钉、关闭、拖拽、焦点、位置和主题由宿主外壳管理。关闭转换为隐藏、不销毁窗口，是既定窗口合同。

内容页公开类型目前只有：

```ts
interface UiPilotPluginWindowApiV1 {
  onUpdate(
    handler: (update: Readonly<PluginWindowUpdate>) => void | Promise<void>,
  ): () => void
}
```

只能接收数据。这不是缺陷，而是第一阶段刻意收窄。番茄钟需要在这个门面上**新增**计时控制，而不是指责 `onUpdate()` 实现错了。

### 5.3 Runtime 请求期 API

`onCommand(invocation, api)` 的 `api` 绑定当前 `pluginId`、generation、`requestId`，请求结束后立即失效。submit 的 post-dispatch 超时是 30 秒。用户点「开始」发生在窗口里、发生在 `onCommand` 已经返回之后，现有 Runtime 门面按设计够不到这次点击。

### 5.4 `notifications.schedule()` 按请求级合同工作

它解决的是：在一次仍有效的命令请求中登记一条不可变纯文本，宿主稍后经消息中心发布。隐藏主窗口和子窗口不取消已接管任务。它不是可暂停、可查询、可重置的番茄钟计时器。把番茄钟塞进该 API 会破坏已发布心智，因此不改它的语义。

### 5.5 消息中心投递面可复用

消息中心已能持久化纯文本、派发 main-only 状态事件、Windows 通知、托盘提醒和未读徽标。到期失败不得劫持已被新输入拥有的主界面。番茄钟缺的是：**计时器到期后由宿主内部走同一条提交路径**，不再要求当时存在有效 `onCommand`。

「向主窗口发送消息」不表示重新打开 launcher 或插入主结果。

## 6. 主程序 API 需要扩充的功能

| 能力 | 当前情况 | 需要扩充 |
| --- | --- | --- |
| 窗口内容页调用宿主 | `UiPilotPluginWindowApiV1` 只有 `onUpdate()`，只能接收数据 | 增加窄化的计时控制接口，让窗口内容页可以 Start、Stop、Reset、读取状态 |
| 宿主持有计时器 | 只有请求内的 `notifications.schedule()`，不可取消和查询 | 新增每插件单例、可暂停/取消/重置的一次性计时器服务 |
| 计时状态同步 | 内容页只能收到新命令的 `PluginWindowUpdate` | 增加状态快照/订阅，使重新打开窗口能看到真实剩余时间 |
| 到期发送消息 | 消息只能由有效 `onCommand` 请求调用或预先 `schedule()` | 计时器到期后，由宿主内部直接复用消息中心提交路径 |
| 到期播放声音 | 公开插件没有音频能力 | 增加宿主内置的固定闹铃播放服务 |
| 生命周期联动 | 当前只会取消延迟消息 | 禁用、卸载、成功升级时，同时取消计时器并停止尚未结束的闹铃 |
| 权限与 SDK | 当前没有计时器权限和类型 | 增加明确权限、TypeScript 类型、Schema、Capability 和开发者文档 |

建议把窗口计时桥做成独立能力形状（不是最终 DTO）：

```ts
interface UiPilotPluginWindowTimerApi {
  getState(): Promise<TimerState>
  start(input: TimerStartInput): Promise<TimerState>
  stop(): Promise<TimerState>
  reset(): Promise<TimerState>
  onStateChanged(handler: (state: TimerState) => void): () => void
}
```

调用面在窗口内容页，不在 `onCommand`。权威状态在宿主计时器服务。`onStateChanged` 只推送宿主状态，内容页本地 `setInterval` 只用于刷新显示，失败或节流不得导致漏发消息或漏响铃。

## 7. 禁止的实现捷径

- 修改 `notifications.schedule()` 使之可取消、可暂停或从窗口调用
- 把隐藏未销毁的 WebView 写成插件后台运行时
- 到期后回调任意插件函数，或让 Runtime 请求代码在返回后继续产生副作用
- 内容页调用 Tauri、Shell、网络、任意文件，或用 Web Audio 充当官方闹铃
- 包内音频、周期任务、日历闹钟、跨插件通信、跨重启恢复（均另立规格）
- 通用 `background.schedule` 或独立插件进程

## 8. 本文冻结的合同

下列语义此前在评估文稿中含混或自相矛盾。实现设计规格必须遵守本节；若要改，先改本文再写代码。DTO 名称仍可调整。

### 8.1 状态机：Stop 是暂停

对外状态只有 `idle | running | paused | fired`。`claiming` 是宿主执行消息提交的短暂内部阶段，**不对插件公开**，避免所有插件额外处理一个没有实际操作价值的状态。

公开投影（claiming 期间）：

- `status` 仍为 `running`
- `remainingMs = 0`
- `Stop` 返回当前这份 `running` 投影，不能转成 `paused`
- `Reset` 可以返回 `idle`（撤销 ClaimTicket 与尚未开始的 AudioTicket）
- `onStateChanged` **不必**专门发送 claiming 事件；下一次对外事件是 `fired` 或 `idle`（或 Reset 导致的 `idle`）

Stop / Reset 的产品语义：

- **Stop = 暂停。** 仅当内部仍是尚未签发 ClaimTicket 的 `running` 时，进入 `paused`，保留 remaining。不回到初值。
- **Reset = 取消并回到初值。** 进入 `idle`，remaining 等于本次设定时长，**不**自动开始。撤销未提交的 ClaimTicket 与尚未开始的 AudioTicket。不能撤回已经保存的完成消息。
- 测试默认设定时长 10 秒；设定时长在 `start()` 时冻结，见 8.7。
状态转换（`claiming` 为内部态）：

| 当前状态 | Start | Stop | Reset | 宿主到期 claim |
| --- | --- | --- | --- | --- |
| `idle` | → `running`，按冻结时长登记到期 | 保持 `idle`，返回当前状态 | 保持 `idle`，剩余时间仍为设定时长 | 不可能 |
| `running` | 幂等，返回当前状态，不登记第二条任务 | → `paused`，保留 remaining | → `idle`，回到设定时长 | 锁内 → `claiming`，签发 ClaimTicket |
| `claiming`（对内） | 太迟，保持 claiming | 太迟；对外返回 `running` 且 `remainingMs = 0`，不能转成 `paused` | → `idle`，撤销 ClaimTicket 与尚未开始的 AudioTicket | 同一任务不能再签发第二张 ClaimTicket |
| `paused` | → `running`，用 remaining 重新登记到期 | 幂等，保持 `paused` | → `idle`，回到设定时长 | 不可能（无待领取任务） |
| `fired` | → `running`，按新的 `start()` 输入重新冻结并开始新一轮 | 保持 `fired` | → `idle`，回到该轮设定时长；撤销尚未开始的 AudioTicket | 同一任务最多 claim 一次 |

每个插件最多一个计时器。不存在并行的第二条运行中任务。ClaimTicket 提交结果见 8.3。`getState()` 在 claiming 期间若被调用，返回上述公开投影（`running` + `remainingMs = 0`），其 `timerRevision` 已包含进入 claiming 的那一次递增。

### 8.2 窗口控制会话世代

计时器**服务**与窗口**控制 API** 寿命不同。

- 服务绑定 `pluginId + pluginGeneration`。隐藏或关闭子窗口、隐藏主窗口，**不**取消 `running` 任务。
- 控制 API（`start` / `stop` / `reset` / `getState` / `onStateChanged`）绑定 **timer session generation**。当前实现里关闭只是隐藏，因此必须显式撤销旧会话，否则隐藏中的内容页可以用脚本再次 Start。

会话规则：

1. 窗口因本次命令显示或再次显示时，宿主签发新的 session generation，并把计时桥交给内容页。
2. 窗口隐藏、关闭、插件升级、禁用、卸载、故障停用、reload/replacement 时，立即撤销该 session。旧 API 对象此后返回会话失效错误，不得启动、停止、重置或继续投递 `onStateChanged`。
3. 用户再次用命令打开同一单例窗口时，签发新 session。内容页必须重新 `getState()` 并重新订阅。
4. 撤销控制会话**不等于**取消已经 `running` 的宿主计时器。
5. 禁用、卸载、成功升级：撤销会话，取消计时器，撤销尚未提交的 ClaimTicket 与尚未开始的 AudioTicket，并停止已经开始、尚未结束的闹铃。

### 8.3 Stop 与到期 claim 的线性化

宿主对每个插件的计时器状态机在**计时器锁内**只做内存转换，锁不得跨越消息存储 I/O、原生闹铃、Windows 通知、托盘或前端事件。这与延迟消息「调度器锁不得跨越消息持久化」的锁顺序一致。

claim 的线性化点是锁内 `running → claiming`；Stop 的线性化点是锁内 `running → paused`。两者由同一状态机串行决定先后，都不是消息落盘成功。

固定流程：

1. 到期领取线程获取计时器锁。若当前不是 `running`，放弃。若是 `running`，转入内部 `claiming`，签发唯一 `ClaimTicket`（绑定 `pluginId`、generation、该轮冻结数据和 `timerRevision`），递增 revision，立即释放计时器锁。此步不发送专门的 claiming `onStateChanged`。
2. 锁外执行消息中心原子持久化。计时器锁在此期间可供 Stop / Reset / 生命周期操作使用。
3. 持久化返回后再次获取计时器锁，**凭仍有效的同一张 ClaimTicket** 提交终态：
   - 保存成功且 ticket 仍有效：`claiming → fired`，再签发 `AudioTicket`，释放锁后尽力播放闹铃；
   - 保存失败且 ticket 仍有效：`claiming → idle`，不签发 AudioTicket，不重试；
   - ticket 已失效：不得改为 `fired`，不得签发或使用 AudioTicket。若消息已经落盘，不撤回该消息。

竞态结果：

- **Claim 先赢**（已签发 ClaimTicket）：之后的 Stop 明确太迟，返回当前 `running` 投影（`remainingMs = 0`），不能转成 `paused`，也不能阻止这次持久化尝试。内部保持 claiming，直到 ticket 提交 `fired` 或失败回 `idle`。
- **Stop 先赢**（锁内 `running → paused` 时尚未签发 ticket）：claim 无法取得 ClaimTicket，不得写入完成消息，不得响铃。
- **Reset / 禁用 / 卸载 / 成功升级** 可在 claiming 期间撤销 ClaimTicket，并同时使尚未开始的 AudioTicket 失效。撤销后对外进入 `idle`（或插件已不存在）。随后迟到的持久化成功不得提交 `fired`，迟到的音频不得开始播放。已经开始的播放按 8.5 中止。
- Reset 在 `fired` 之后仍可回到 `idle`，但不能撤回已经保存的消息；此时若 AudioTicket 尚未开始播放，必须失效。

`ClaimTicket` 与 `AudioTicket` 只存在于宿主内部，不暴露给插件 DTO。

### 8.4 状态乱序：`timerRevision`

`timerRevision` 沿用消息中心的跨 Rust/JavaScript 合同，不得使用 JavaScript `number` 传递或比较。

- 宿主内部使用**单一** `u64` 计数器，从 `"0"` 起。**每次权威状态机转换都递增**，包括内部 `running → claiming`、`claiming → fired`、`claiming → idle`。内部转换可以不发送 `onStateChanged`，因此前端观察到的 revision **允许跳号**；只保证严格单调，不保证连续。
- 跨边界字段是**规范无前导零的十进制字符串**，匹配 `^(0|[1-9][0-9]*)$`，值属于 `0..=18446744073709551615`（即 `u64::MAX`）。
- Rust 侧校验格式后按 `u64` 比较。
- TypeScript 业务调用点必须使用已有的 `compareU64Decimal`（或与其语义相同的唯一 helper）：先按字符串长度比较，长度相同再按 ASCII 数字字典序比较。禁止 `Number`、`parseInt`、隐式数值转换、直接的字符串关系运算符，或把 revision 当 `number` 序列化。
- 内容页只接受 **`compareU64Decimal(next, seen) > 0`** 的状态。旧快照、乱序 Promise、已撤销 session 的订阅事件都不能覆盖新状态。重新显示窗口时，以新 session 下第一次 `getState()` 为基准。
- 计数器耗尽（无法再安全加一时）：该插件的计时服务进入**不可用**状态，不回绕、不复用 revision。已 running / claiming 的任务按生命周期取消处理（撤销 ticket、不发新消息、中止闹铃）。后续 `start` / `stop` / `reset` / `getState` 失败关闭，直到进程重启后计数器重新从 `"0"` 开始。不可用不得把已保存的消息撤回。

### 8.5 闹铃语义

- 宿主内置固定音效，播放**一次、有限时长**，不循环，不提供插件自定义文件。
- 仅当 ClaimTicket 成功提交 `fired` 后才签发 `AudioTicket`。播放在计时器锁外启动；启动前必须再次确认 AudioTicket 仍有效。
- 闹铃是尽力副作用，类似 Windows 通知：不回滚已保存的消息。
- **消息原子持久化失败**（ticket 仍有效）：不签发 AudioTicket，不播放闹铃，本任务丢弃、不自动重试；凭 ticket 回到可再次 Start 的 `idle`，并记录受控诊断。不得把计时器留在 `running` 或 `claiming`。
- **消息已保存、闹铃播放失败：** 保持 `fired` 与已保存消息，不重试响铃。
- **Reset、禁用、卸载、成功升级：** 使尚未开始的 AudioTicket 失效，因此「消息已保存、Reset 已完成、迟到音频才 start()」是禁止结果。已经开始、尚未结束的播放立即中止。退出 UiPilot 同样中止播放。
- 窗口隐藏或关闭：不撤销已签发且已开始的这一次播放；若 AudioTicket 尚未开始，仍须在启动前校验 ticket（隐藏本身不撤销 AudioTicket）。

不采用「循环直到用户关闭」。那需要额外的关闭/停止闹铃 UI，超出当前用户合同。

### 8.6 睡眠与时钟

- 到期点在 `start()` / 从 `paused` 恢复时用单调时钟登记，不受用户修改墙上时间影响。
- **系统睡眠计入到期判断：** 睡眠覆盖预定到期点时，进程唤醒后立即尝试 claim 已到期任务。与延迟消息的唤醒领取合同一致。
- 内容页 `setInterval` 在睡眠中停止，不影响宿主 claim。
- 唤醒时若任务尚未到期，继续按剩余单调时间等待。
- 同一任务最多被领取一次。

### 8.7 开始时冻结的数据

到期时不再运行 Runtime 代码，也不再询问内容页。`start()` 线性化成功时，宿主必须冻结：

- 设定时长（测试为 10 秒对应的 `delayMs`）
- 完成消息正文（合法原文；用 `trim()` 判空，不改写入库文本）
- 当时的插件显示名称快照
- `pluginId` 与 `pluginGeneration`

从 `paused` 恢复的 Start 不更换已冻结的消息正文和名称快照，只按 remaining 重新登记到期点。`fired` 之后的新 Start 视为新一轮，重新冻结。

Reset 和生命周期取消丢弃尚未签发 ClaimTicket 的冻结数据。已经凭有效 ticket 保存的消息保留名称快照，不因随后改名而改写历史记录。claiming 期间被撤销的 ticket 若仍把消息写入了存储，该消息同样保留，只是计时器不得进入 `fired`、不得播放闹铃。

## 9. 仍留给设计规格的选择

本节不影响第 8 节已冻结语义，但实现规格仍需写明：

1. 新权限是独立名称，还是复用 `notifications.publish`。复用会把「请求内发一条消息」和「关窗后闹钟」绑在同一授权上。
2. 作为 `apiVersion: 1` 兼容新增，还是 `apiVersion: 2`。建议至少新增明确权限，并更新开发者教程的 MVP 边界。
3. 做成公开插件能力还是仅第一方内置番茄钟。若要验证第三方闹钟类插件，应使用独立验收插件，不要改 `demo-win` 的请求级 `schedule()` 语义。
4. macOS：当前消息权限不可安装。本阶段是否 Windows-only。
5. `TimerStartInput` 的字段集、错误码字符串、是否允许内容页传入消息正文或必须由宿主模板生成。

## 10. 建议的能力阶段

| 阶段 | 交付 | 能验收的用户故事 | 不包含 |
| --- | --- | --- | --- |
| 现有 v1（已完成，无 Bug） | 命令 + 单窗口 + 请求内延迟消息 | `/pomodoro` 打开窗口并保持 `00:10`，不自动开始 | 点开始之后的宿主计时、暂停/重置、关窗后到期、闹铃 |
| 建议下一阶段 | 第 6 节五块能力 + 第 8 节冻结合同 | 第 3 节 1–5 步在 Windows 上可人工验收 | 自定义铃声、循环闹铃、跨重启、多计时器、周期任务、独立插件进程 |
| 更后 | 包内音频或 macOS 通知、跨重启恢复 | 产品化铃声与平台对称 | 通用后台 Runtime |

## 11. 给宿主的验收判据

现有 v1 已满足：

1. 用户 Enter 后窗口立即出现，倒计时保持 `00:10`，不开始递减，不因这次 Enter 登记番茄钟到期消息。

补齐第 6 节能力且遵守第 8 节合同后，还必须为真：

2. 点「开始」后才递减；从点开始起约 10 秒到期，而不是从 Enter 起算。
3. 点「停止」后进入暂停；等到原到期点，消息中心没有新完成消息，也不播放闹铃。再次「开始」从 remaining 继续。
4. 点「重新计时」后显示回到 `00:10` 且不自动开始；尚未签发 ClaimTicket 的任务被取消。若正在 claiming，Reset 撤销 ticket，迟到音频不得播放。
5. 开始后关闭子窗口，等待到期：消息中心出现一条完成消息，宿主闹铃播放一次；主窗口不被这次到期抢占或写入错误。隐藏中的内容页不能再次 Start。
6. 开始后禁用、卸载或成功升级：撤销 ClaimTicket 与尚未开始的 AudioTicket；尚未落盘则不发新消息。已经开始、尚未结束的闹铃停止。
7. 关闭后再打开同一插件窗口：新 session 下 `getState()` 显示宿主权威剩余时间或 `fired`，不是错误地重新从 `00:10` 开始（除非用户已 Reset）。
8. Stop 与到期同时发生时遵守 8.3：claim 的线性化点是锁内 `running → claiming`，Stop 的线性化点是锁内 `running → paused`，由同一状态机串行决定。计时器锁不跨越消息落盘。claim 先赢则 Stop 返回 `running`（`remainingMs = 0`）且太迟；Stop 先赢则无 ClaimTicket。Reset / 禁用 / 卸载 / 升级使尚未开始的 AudioTicket 失效。乱序状态用 `compareU64Decimal` 比较规范十进制 `timerRevision`（允许跳号），遵守 8.4。
9. 睡眠覆盖到期点后，唤醒立即 claim；内容页本地计时停止不影响该结果。

## 12. 摘要

| 需求步骤 | 现有 API | 判定 |
| --- | --- | --- |
| 1. `/pomodoro` Enter | 斜杠命令 + `submit` | 够用，不是 Bug |
| 2. 主窗口隐藏，打开窗口 | `outputMode: window`、`ui.window` | 够用，不是 Bug |
| 3. 窗口内 `00:10` 与按钮 UI，且不自动开始 | 内容页 HTML/JS/CSS + `onUpdate` | 够用，不是 Bug |
| 4. 点开始后才由宿主倒计时 | 无窗口计时桥 / 无可控计时器服务 | 能力未提供 |
| 4. 暂停 / 重新计时 | `schedule()` 按设计不可取消 | 能力未提供 |
| 5. 关窗后继续计时 | 需宿主持有单例计时器 | 能力未提供 |
| 5. 到期发消息 | 消息中心可用，缺到期内部入口 | 能力未提供 |
| 5. 播放闹铃 | 无宿主闹铃服务 | 能力未提供 |

**总评：** 现有 v1 外壳正确。番茄钟要成立，主程序新增窗口计时桥、计时器服务、状态同步、到期消息入口、宿主闹铃五块能力，并先冻结第 8 节状态机、安全会话、竞态和失败语义。不要改 `notifications.schedule()`。
