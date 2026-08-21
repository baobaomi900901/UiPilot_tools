# UiPilot Windows 原生提醒协调器设计

## 1. 文档信息

- 日期：2026-08-21
- 状态：Draft，第三轮独立审核修订完成，等待复审
- 范围：Windows Toast、托盘提醒、普通消息提示音、计时器持续闹铃
- 公开 API：不变

本设计是以下已批准规格的增量覆盖：

- [公开插件消息中心与 Windows 通知设计](./2026-08-18-public-plugin-message-center-design.md)
- [公开插件窗口计时 API 设计](./2026-08-20-public-plugin-window-timer-api-design.md)

除本节列出的覆盖项外，原规格中的持久化、未读、权限、插件请求、Timer 状态机、ClaimTicket、
AudioTicket、锁顺序和窗口会话合同继续有效。

本设计明确覆盖以下旧合同：

| 旧规格章节 | 被覆盖的旧规则 | 当前唯一规则 |
|---|---|---|
| 消息中心 4.4、15、17 | 每条消息都尝试 Toast，不按主窗口焦点抑制 | 主窗口已聚焦时抑制 Toast、托盘和声音；消息与徽标仍提交 |
| 消息中心 10、11、13 | Toast 与托盘独立派发、各自拥有顺序和状态 | 前端 ready 事件后只构造一个 `PublishedAttention`，由统一队列串行处理 Toast、托盘和声音 |
| 消息中心 13、16 | `tauri dev` 只作为开发态冒烟，不承担正式身份合同 | Debug 与 Release 使用独立 AUMID；普通权限 dev 和安装包都必须通过身份验收 |
| Timer 3.1、3.2、4、18.3、23 | 固定有限音效、只播放一次、不循环 | 有效 Timer 票证共享一条持续循环声音，直到焦点确认或最后一票撤销 |
| Timer 7.3、7.5、13 | 每轮独立开始/停止音频，Reset 或生命周期变化无条件停止本轮音频 | 每票只改变自身权威 audio 状态；只有共享集合变空或主窗口聚焦才调用全局 stop |
| Timer 8 | 每次内部权威转换都递增 `timerRevision` | Timer 阶段、轮次和内部 `claiming` 转换递增；仅服务原生提醒的 `issued/admitted/confirmed` audio 子状态不递增，并共享对应 `firedRevision` |
| Timer 12.3、14 | 每张 AudioTicket 独立尝试播放，可混音或串行 | 每票先在 Timer 锁内执行权威 audio-start admission；原生调用在锁外由单一声音通道执行 |
| Timer 20.3、20.5、20.6 | 单票播放/停止的旧测试矩阵 | 使用本设计第 10 节的有界 audio 状态、多票共享、焦点确认和控制事件矩阵 |
| Timer 21 | 到期播放一次有限闹铃 | 到期循环播放，主窗口任意页面获得原生焦点后停止 |

“每票独立播放”“取消一票即无条件调用全局 stop”“固定单次有限闹铃”和旧的 Toast/托盘独立副作用顺序
不再是实施合同。表中未列出的旧规则继续有效。

## 2. 目标与非目标

### 2.1 目标

1. 修复普通权限 `tauri dev` 下成功保存消息但不显示 Windows 右下角 Toast 的问题。
2. 普通消息到达且主窗口未聚焦时，使用宿主固定 WAV 播放一次提示音。
3. Timer 到期且主窗口未聚焦时，使用同一 WAV 持续循环播放，直到主窗口获得焦点。
4. Toast、托盘、徽标和声音独立降级，任一原生效果失败都不回滚已保存消息。
5. 用一个宿主协调器串行决定焦点、托盘和声音优先级，避免多个 `PlaySoundW` 调用互相抢占。

### 2.2 非目标

- 不增加或修改公开插件 API、权限、Manifest、Timer DTO 或消息 DTO。
- 不允许插件指定音频、音量、循环方式、Toast 标题、Toast 动作或托盘图标。
- 不实现用户音量设置、静音时段或单插件提醒偏好。
- 不承诺 UiPilot 进程退出后点击历史 Toast 能冷启动应用。
- 不实现 macOS 原生通知或音频。
- 自动化和 Agent 不控制用户鼠标、键盘、真实前台焦点或听觉验收。

## 3. 用户合同

### 3.1 普通消息

普通消息原子持久化成功后，宿主始终更新消息摘要和两个未读徽标。若主窗口当时未聚焦，宿主还会：

1. 显示一张 Windows Toast；
2. 启动或保持托盘闪烁；
3. 在没有 Timer 持续闹铃时异步播放一次固定 WAV。

若主窗口已经聚焦，上述三项原生注意效果全部抑制。消息仍保存，未读数仍增加，当前视图、焦点、查询和
输入内容不被改变。

密集到达的普通消息不排队播放音频。若前一次普通提示音尚未结束，后到消息可以从头重新播放同一 WAV。

### 3.2 Timer 完成消息

Timer 仍先保存完成消息，再凭有效 ClaimTicket 提交 `fired` 并签发 AudioTicket。已保存完成消息照常更新
徽标，并在主窗口未聚焦时显示 Toast、启动托盘提醒。

Timer 完成消息不播放普通消息的一次提示音。若 AudioTicket 在原生提醒队列中仍有效且主窗口未聚焦，
协调器把它加入待确认集合，并启动或保持一条共享循环闹铃。

若到期时主窗口已经聚焦，本轮声音提醒视为已确认：不启动循环，不在之后主窗口失焦时补响，但已保存消息
和 `fired` 状态保留。

### 3.3 多 Timer 共享声音

协调器维护有效且尚未确认的 AudioTicket 集合：

- 第一张票证加入空集合时启动一条循环闹铃；
- 后续票证只加入集合，不叠加、混音或重启闹铃；
- Reset、插件禁用、故障停用、卸载、版本替换或应用退出只撤销相应票证；
- 撤销后集合仍非空则继续播放，集合变空则停止；
- 主窗口获得焦点时一次确认并清空当前全部待确认票证，同时停止声音。

Timer 持续闹铃具有最高声音优先级。只要待确认集合非空，普通消息仍保存、显示 Toast、更新徽标并触发托盘，
但不播放一次提示音。

### 3.4 焦点与已读分离

只有 `main` 原生窗口真实产生 `Focused(true)` 才算用户打开主界面。Launcher、Settings 和 Messages 都在
同一个 `main` 窗口中，因此任意页面获得焦点都执行以下注意确认：

- 托盘立即恢复正常图标；
- 当前普通提示音停止；
- Timer 服务把当时所有 `issued/admitted` audio 槽位提交为 `confirmed`，协调器活动集合清空，循环闹铃停止。

注意确认绝不读取消息存储、不写 `readAt`、不减少未读数。只有进入 Messages Tab 并成功执行现有
`open_and_mark_read` 合同，两个徽标才消失。`Focused(false)` 只更新焦点状态，不恢复已确认的旧提醒。

## 4. 架构

### 4.1 统一协调器

新增一个进程级 `NativeAttentionCoordinator`。它拥有唯一有界合并邮箱和工作线程，并通过窄端口调用：

- `MessageToastPort`：Toast 身份、显示、点击、失败与清理；
- `TrayAttentionPort`：正常图标与透明帧切换；
- `AttentionAudioPort`：单次播放、循环播放和停止。

消息中心在持久化成功并释放消息存储锁后构造唯一分类派发对象。Timer 完成消息无论最终是否取得可播放
AudioTicket，都恰好构造一次 Timer 分类。主窗口的既有 `WindowEvent::Focused(bool)` 钩子向同一邮箱提交
焦点事件。

邮箱 admission 在一个短临界区内分配严格单调的内部 `attentionSequence`，并按第 4.4 节的固定槽位与合并
规则保存事件。普通消息原生效果最多允许 64 个待处理配额，超额时只丢弃该消息的 Toast、托盘和一次声音。
Timer、焦点、取消、Toast 回调和 Shutdown 不与普通消息争用这 64 个配额；它们使用各自有界槽位，达到
边界时按第 4.4、8、9 节 fail-closed，绝不进入无界备用队列。

不增加第二套焦点查询，也不由前端上报焦点。生产初始焦点固定为 `false`，之后只接受原生焦点事件。

### 4.2 内部事件

内部事件的语义固定为：

```rust
struct PublishedAttention {
    message: MessagePublished,
    origin: AttentionOrigin,
}

enum AttentionOrigin {
    Ordinary,
    TimerCompletion { audio_ticket: Option<AudioTicket> },
}

struct MainFocusChanged {
    focused: bool,
    // Focused(false) 固定为空；Focused(true) 最多 64 张。
    confirmed_admitted_tickets: Vec<AudioTicket>,
}

enum NativeAttentionEvent {
    Published(PublishedAttention),
    TimerAudioCancelled(AudioTicket),
    MainFocusChanged(MainFocusChanged),
    ToastCallback {
        notification_id: NativeNotificationId,
        kind: ToastCallbackKind,
    },
    Shutdown,
}

enum ToastCallbackKind {
    Activated,
    Failed,
    Dismissed,
}
```

`TimerCompletion { audio_ticket: None }` 表示 Timer 完成消息已经保存，但 Reset 或生命周期变化使迟到完成
不能播放声音。该消息仍可显示 Toast、更新徽标和触发托盘，但绝不退化成 `Ordinary` 或普通一次提示音。

`PublishedAttention` 只存在于宿主内存，不进入持久化 DTO 或公开插件协议。成功保存的消息恰好构造一个该
对象；一个统一 post-guard dispatcher 先发送前端 ready 事件，再向有界邮箱提交 `Published` 原生事件。

### 4.3 状态

协调器工作线程独占以下瞬时状态，不向插件或前端公开：

```text
mainFocused: bool
trayAttention: inactive | active | degraded
audio: silent | ordinaryOnce | timerLoop
activeTimerTickets: set<AudioTicket>
mode: running | terminal
```

不使用跨线程共享互斥锁保护这些字段。生产调用方只向队列发送事件；原生 Toast、托盘和音频调用只发生在
工作线程或各适配器自有回调中。`activeTimerTickets` 只包含已通过权威 admission、当前参与共享声音的票证，
每个 TimerKey 最多一张。

票证的权威状态只存放在对应 TimerRecord 的单一有界槽位：

```text
audio: none | issued(AudioTicket) | admitted(AudioTicket) | confirmed(AudioTicket)
```

新 round 替换旧槽位；Reset 或生命周期取消清空槽位。协调器不保存取消墓碑，也不根据历史事件自行恢复
票证。重复或迟到事件必须先匹配 TimerRecord 当前的 `issued` 状态，因而不会补响或随历史轮次增长内存。
`confirmed` 表示该票证不会再启动声音，原因可以是主窗口焦点确认或原生播放终态失败；不等同于消息已读。
协调器的 `activeTimerTickets` 同样硬限制为 64 张。集合已满时不执行 `issued -> admitted`，而是执行匹配的
`issued -> confirmed` 并只保留该消息的 Toast/托盘效果，避免第 65 张活动票扩大常驻内存。

audio 槽位是宿主原生副作用的权威状态，但不是公开 Timer 状态。`issued -> admitted -> confirmed`、清空
audio 槽位以及派发 `TimerAudioCancelled` 都不得递增公开 `timerRevision`、发送 Timer 状态事件或改变
`getState()` 投影。`issued` 在既有 `claiming -> fired` 权威转换中一并创建，并共享该次 `fired` revision；
`AudioTicket.firedRevision` 此后保持不变。Reset、新 Start 等公开 Timer 转换仍各自只递增一次 revision，
附带的 audio 取消不再额外递增。

### 4.4 有界合并邮箱

邮箱没有通用无界 FIFO，固定由以下存储组成：

- `ordinaryPublished`：最多 64 条，保持 FIFO；满时丢弃新消息的原生效果，消息、徽标和前端事件不回滚；
- `timerEffects`：最多 64 个 `TimerKey`；每个 key 最多一个待处理 `Published` 和一个待处理取消。旧 pending
  `Published` 被同 key 新轮次替换时，必须先终结旧 `issued` AudioTicket；已 admitted 的旧票通过对应取消
  槽位保留，不能被新 `Published` 覆盖；
- `focusEvents`：固定容量 128 的 FIFO，保存每次 `Focused(true/false)` 及其准确 sequence；焦点事件不合并，
  以保留消息与多次快速焦点切换之间的全部可观察顺序；
- `toastCallbacks`：最多 64 个活动通知 ID，每个 ID 最多保留第一个终态回调；活动 Toast 句柄也最多 64 个；
- `shutdown`：独立原子 terminal 标记与唤醒信号，不依赖任何数据槽位容量。

Timer 第 65 个 key 无法 admission 时，本条 Timer 消息仍保持成功并更新徽标，但跳过全部原生效果；若携带
`issued` 票证，必须在返回前调用 Timer 权威终止方法把它提交为不可迟到播放的终态。活动 Toast 已达 64
时只跳过新 Toast，托盘和声音仍可继续。

每个已保存条目携带 admission 时分配的 sequence，worker 总是选择最小 sequence。普通 `Published` 捕获
`focusedAtAdmission`，只有该快照为 true 才整体抑制本条原生效果。后到的 `Focused(true)` 不得回溯丢弃
sequence 更早的消息：worker 先处理旧消息的 Toast、托盘和普通声音，再处理焦点事件并停止托盘/声音；已经
显示的 Toast 不撤回。失焦后新 admission 的消息捕获 false，可重新产生提醒。

邮箱唤醒、sequence 或固定槽位出现不可恢复的耗尽/不一致时，协调器进入本次进程 terminal：拒绝新原生
效果、执行幂等 emergency-stop，并终结所有 `issued/admitted` audio 票证。计数器不得回绕；消息和徽标仍可用。

## 5. 事件顺序与线性化

### 5.1 消息提交

消息持久化成功仍是插件 `publish()` 和 Timer 完成消息不可撤销的成功线性化点。顺序固定为：

1. 在消息存储锁内提交消息与新 revision；
2. 释放消息存储锁；
3. 普通消息构造 `PublishedAttention { origin: Ordinary }`；Timer 消息先调用 `complete_claim`，再构造
   `PublishedAttention { origin: TimerCompletion { audio_ticket } }`，其中票证允许为 null；
4. 唯一 post-guard dispatcher 向 `main` 发送现有消息状态事件；
5. 同一 dispatcher 向原生提醒协调器发送对应 `Published` 事件；
6. 不等待用户看到、点击、关闭 Toast 或听到声音。

第 4 或第 5 步失败不改变持久化成功，也不修改插件 Promise 的成功结果。Timer 分类永远不会因
`audio_ticket: None` 退化为普通消息；消息保存成功后也不会因为 complete_claim 未签票而漏掉 Toast/托盘。

### 5.2 焦点与消息竞态

消息与 `MainFocusChanged` 共用一个有界邮箱 admission 临界区。`attentionSequence` 是注意效果的线性化顺序：

- 焦点事件先处理：后到消息读取 `mainFocused = true`，不产生原生注意效果；
- 消息先处理：允许开始原生效果；随后焦点事件会停止声音和托盘；
- `MainFocusChanged(false)` 不会重新播放或重新闪烁已经确认的旧事件。

焦点事件不得合并或用高水位回溯抑制旧消息。固定 FIFO 满时按第 4.4 节进入 terminal emergency-stop，不能
静默丢弃某次焦点确认，也不能退化到无界队列。

主窗口请求 `show()` 或 `focus()` 不算确认；只有随后实际收到的 `Focused(true)` 才确认。

`Focused(true)` 的 admission 使用固定锁顺序 `attention admission -> Timer`：在邮箱短临界区内先分配焦点
sequence 并更新最新焦点快照，再获取 Timer 服务锁，把该线性化点已经存在的全部 `issued/admitted` audio
槽位提交为 `confirmed`，同时收集当时准确的 admitted AudioTicket 列表；列表受活动票证上限约束，最多 64
张。随后释放 Timer 锁、把该列表附到焦点事件、释放邮箱锁并唤醒 worker。所有 Timer 完成、Reset 和生命周期
路径都必须先释放 Timer/plugin mutation 锁才进入 attention admission，禁止 `Timer -> attention admission`
反向锁序。

worker 处理该 `Focused(true)` 时只从 `activeTimerTickets` 移除事件携带的准确列表，并停止当前普通声音、
循环声音和托盘提醒；不得再次调用无 sequence 截止点的 `confirm_all_current_audio()`。该焦点线性化点之后才
签发的票证不在列表中，因此 `Focused(false)` 后产生的新 Timer 可以正常 admission。已经在焦点前排队、但
尚处于 issued 的票证已被同步确认，其旧 `Published` 仍可按 sequence 显示 Toast/托盘，但不得迟到启动循环。

### 5.3 Timer 票证竞态

Timer 仍由既有 ClaimTicket/AudioTicket 合同决定消息与声音资格。协调器只接受 Timer 服务签发的
AudioTicket，不自行构造或推断票证。

协调器准备把一张 Timer 票加入共享声音前，必须调用 `TimerService::admit_audio_start(ticket)`。该方法与
Reset、禁用、故障停用、卸载和版本替换使用同一 Timer 状态锁，并只允许当前 TimerRecord 的
`issued(ticket) -> admitted(ticket)`：

- 撤销先完成：槽位已清除，admission 拒绝；消息仍有 Toast/托盘/徽标，但不加入声音集合；
- admission 先完成：释放 Timer 锁后才加入 `activeTimerTickets` 并调用 `PlaySoundW`；后续撤销通过控制事件
  移除该票，集合变空时停止；
- `PlaySoundW` 循环启动失败：从活动集合移除该票，并在 Timer 锁内执行匹配的 `admitted -> confirmed`；
  本票不重试，后续新 round 或其他 Timer 票证仍可独立尝试；
- 主窗口已经聚焦：协调器调用 `confirm_audio_without_start(ticket)`，只允许当前 `issued -> confirmed`；
- 每个 `MainFocusChanged(true)` 都携带第 5.2 节 admission 临界区同步确认后得到的准确 admitted 列表；worker
  只移除这些票证并停止声音，不重新扫描 TimerRecord；尚未到期、没有 audio 槽位以及焦点线性化点之后才
  签发的 Timer 不受影响；
- 重复或迟到提交：当前状态不是匹配的 `issued`，admission 拒绝；
- 新 round、Reset 或生命周期取消后，旧票证不再匹配当前槽位。

Timer 锁只跨越上述内存状态转换，不跨事件发送、Toast、托盘或 `PlaySoundW`。取消、焦点确认和重复事件均
幂等。任何票证最多使共享循环从空集合启动一次，绝不保存第二条完成消息。

所有可能删除或替换 `admitted(ticket)` 的 Timer 操作统一返回锁后效果，不允许只靠成功返回路径派发取消：

```rust
struct TimerOperation<T> {
    result: Result<T, TimerError>,
    post_lock_effects: Vec<TimerPostLockEffect>,
}

enum TimerPostLockEffect {
    AudioCancelled(AudioTicket),
}
```

Reset、`fired -> start(new round)`、禁用、故障停用、卸载、成功版本替换，以及 revision/round/audio identity
耗尽导致的 `TimerUnavailable`，只要移除或替换的是 `admitted`，都必须恰好产生一个 `AudioCancelled`。调用方
先释放 Timer/plugin mutation 锁，再派发全部 `post_lock_effects`，最后才向公开命令返回 `result`；因此即使
结果是错误，也不能漏发取消。清除尚未 admission 的 `issued` 只需让后续 admission 失败；清除 `confirmed`
不产生取消。取消事件发送失败按 terminal emergency-stop 处理，不能让协调器中的活动票继续循环。

## 6. Windows Toast 身份

### 6.1 身份值

- Debug：`com.uipilot.launcher.dev`
- Release：`com.uipilot.launcher`

Windows `main.rs` 必须在调用 UiPilot/Tauri Builder、创建任何窗口、托盘、Jump List 或 ToastNotifier 前，
调用 `SetCurrentProcessExplicitAppUserModelID` 设置当前构建对应的值。失败时记录稳定诊断并只禁用 Toast，
不阻止应用、消息、托盘、徽标或声音启动。

### 6.2 每用户快捷方式

未打包桌面应用显示 Toast 必须存在带 `System.AppUserModel.ID` 的开始菜单快捷方式。宿主在普通用户权限下
验证并按需创建：

- Debug：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\UiPilot Dev.lnk`
- Release：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\UiPilot.lnk`

快捷方式验证/创建在独立的一次性 STA 线程完成。该线程调用
`CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`，只使用
`IShellLinkW + IPropertyStore + IPersistFile`，并在所有成功初始化路径平衡 `CoUninitialize`。完成后退出，
再允许 Toast worker 创建 notifier。

快捷方式目标是当前可执行文件，工作目录是可执行文件父目录，AppUserModelID 必须与当前构建匹配。创建和
替换使用同目录临时文件加原子提交，不留下半写快捷方式。

同名快捷方式满足任一条件即视为 UiPilot 自有：

1. 解析后的目标与当前 `uipilot.exe` 规范路径按 Windows ordinal-ignore-case 完全相等；
2. 已携带编译期固定允许列表中的 UiPilot AUMID。MVP 允许列表仅包含
   `com.uipilot.launcher` 和 `com.uipilot.launcher.dev`；未来新增历史值必须显式修改该列表和测试。

自有快捷方式可以原子重建并补齐当前目标、工作目录、图标和 AUMID。目标不是当前程序且不带已知 UiPilot
AUMID 时视为未知用户文件，宿主不得覆盖，本次进程只将 Toast 适配器降级为 no-op。

Debug 快捷方式与 Release 快捷方式隔离。所有实际发布的安装器目标必须使用同一 Release AUMID，或至少
生成目标指向当前程序的 `UiPilot.lnk`，使首次普通权限启动能在创建 notifier 前安全补齐属性。干净安装、
首次启动和升级后的快捷方式目标/AUMID 检查是发布放行条件。

微软要求与依据：

- <https://learn.microsoft.com/en-us/windows/win32/shell/enable-desktop-toast-with-appusermodelid>
- <https://learn.microsoft.com/en-us/windows/win32/shell/quickstart-sending-desktop-toast>

### 6.3 Toast worker 与回调

统一协调器 worker 启动时调用 `RoInitialize(RO_INIT_MULTITHREADED)`。初始化成功后在该线程调用
`CreateToastNotifierWithId(current_aumid)`；`ToastNotifier`、`ToastNotification` 和活动句柄只在该线程
创建、显示、隐藏、移除处理器和释放。worker 退出时平衡 `RoUninitialize`。

`Activated`、`Failed` 和 `Dismissed` 处理器可以由 WinRT 在回调线程执行，但回调只向第 4.4 节的固定槽位
提交 `ToastCallback { notification_id, kind }`。回调线程不得持活动句柄锁调用生命周期、窗口或原生适配器。
每个活动通知只接受第一个终态回调；重复 ID、未知 ID 和 Shutdown 后的迟到回调直接忽略。

worker 处理 `Activated` 时先移除处理器并清理对应 Toast，再请求固定 `ShowTarget::Messages`；窗口路由失败
只记录诊断。`Failed` 记录稳定错误并清理，`Dismissed` 只清理；后二者不打开窗口，三者都不修改消息已读。
回调 admission 只使用活动通知 ID，不接受插件数据或任意 launch 参数。

worker 或回调不依赖任何调用线程已经初始化 COM/WinRT。STA 快捷方式线程、MTA Toast worker 和 Tauri
线程之间不传递 apartment-bound COM 对象。

### 6.4 Toast 行为

Toast 继续使用固定宿主 XML DOM 模板，插件名称和正文只能通过文本节点写入。通知点击只路由到现有
`ShowTarget::Messages`，不允许插件提供 launch 参数或动作。

发送前读取 `ToastNotifier.Setting()`。系统通知关闭、身份注册失败、同步 `Show()` 失败、异步 `Failed`、
点击路由失败或退出清理失败只记录脱敏、稳定分类的诊断；不影响托盘、徽标、声音或已保存消息。

## 7. 音频资源与适配器

### 7.1 固定资源

唯一音频来源为用户确认的：

```text
C:\Users\moby\Downloads\196838__idepe__alarm_clock.wav
```

实现时复制到 `src-tauri/resources/sounds/attention-alarm.wav` 并由 Tauri 资源打包。运行时绝不读取 Downloads。

- 格式：RIFF/WAVE
- 大小：1,724,844 字节
- SHA-256：`9F66E473EEEE7AAF75AB2761423DAD1D04FA3F019744899DD154350F4117A8F3`

### 7.2 Windows 播放

继续使用 `PlaySoundW`，由单一音频端口串行调用：

- 普通消息：`SND_FILENAME | SND_ASYNC | SND_NODEFAULT`；
- Timer 循环：`SND_FILENAME | SND_ASYNC | SND_NODEFAULT | SND_LOOP`；
- 停止：`PlaySoundW(NULL, NULL, 0)`。

Timer 循环到达时可以替换正在播放的普通声音。Timer 待确认集合非空时，普通声音调用被抑制。普通消息
之间不排队，后一次调用可以重启同一 WAV。

音频路径缺失、格式不受支持、设备不可用、播放失败或停止失败不回滚消息、`fired`、Toast、托盘或徽标。
每次失败只记录稳定诊断，不自动重试。逻辑状态仍继续转换，防止失败的设备调用永久阻塞退出或焦点处理。

## 8. 锁与生命周期

- 消息存储锁、插件 mutation 锁、Timer 锁和窗口会话锁不得跨越事件发送、Toast、快捷方式 I/O、托盘、
  `PlaySoundW`、窗口 show/focus 或前端 emit/evaluate。
- Timer worker、Reset 和生命周期路径必须先提交 Timer 内存转换并释放 Timer/plugin mutation 锁，再发送
  原生提醒事件。协调器调用 audio admission/confirm 时只获取 Timer 服务锁，Timer 服务在该锁内绝不反向
  调用协调器。
- `NativeAttentionCoordinator` 的邮箱所有权必须在消息发布和 Timer worker 启动前建立。
- 普通消息配额只限制新建原生效果；Focused、Timer 完成、Timer 取消、Toast 回调清理和 Shutdown 使用
  第 4.4 节各自的有界控制槽位。控制槽位 admission 失败必须进入 terminal emergency-stop，不能静默丢弃、
  阻塞调用线程或退化到无界通道。
- 控制器或工作线程构造失败时安装本次进程固定 terminal 的 no-op 原生提醒端口；UiPilot 继续启动，消息和
  徽标仍可用。
- worker 外层必须使用 `catch_unwind` 和拥有音频/托盘/Toast 端口的 `CleanupGuard`。正常 Shutdown、receiver
  断开或 panic 都尽力停止声音、恢复托盘、清理活动 Toast，并调用 Timer 服务级 `terminate_all_audio()`，
  在一个 Timer 锁内把全部 `issued/admitted` audio 状态提交为 confirmed/terminal。协调器另保留一个可跨
  线程、幂等的 emergency-stop 端口；它也必须调用 `terminate_all_audio()`，并只允许调用进程级音频
  stop、线程安全的托盘恢复和 Timer audio 终止，不得接触 apartment-bound Toast 对象。控制事件发送发现
  receiver 已断开时再次执行该窄化清理；Toast 已由 worker 自身 CleanupGuard 在 `RoUninitialize` 前处理。
- 应用退出顺序固定为：先停止插件请求、延迟消息和 Timer 生产者；再关闭新的原生提醒 admission；然后发送
  `Shutdown`；worker 清理并 join；最后吸收所有终态后的事件和回调。Shutdown 和 emergency-stop 可重复。
- Timer Reset、禁用、故障停用、卸载和成功版本替换继续撤销相应 AudioTicket；失败升级不影响旧票证。
- 重命名和设置保存不撤销 Timer 票证，也不改变已经冻结的消息名称与正文。

## 9. 失败行为

| 失败 | 消息/Timer | 其他原生效果 |
|---|---|---|
| Toast 身份注册失败或系统关闭通知 | 消息与 `fired` 保持成功 | Toast no-op；托盘、徽标、声音继续 |
| Toast 显示、回调或清理失败 | 消息保持成功 | 不停止托盘或声音 |
| 托盘线程、通道或图标切换失败 | 消息保持成功 | Toast、徽标、声音继续；尽力恢复原图 |
| 单次声音失败 | 消息保持成功 | Toast、托盘、徽标继续 |
| Timer 循环声音失败 | 消息与 `fired` 保持成功 | 票证仍按焦点/取消合同确认；不影响其他效果 |
| 普通消息效果配额耗尽或发送失败 | 消息保持成功 | 只丢弃本条 Toast、托盘和一次声音，不重试 |
| Timer key、Toast 句柄或控制槽位达到硬边界 | 消息与 Timer 权威状态保持 | 按第 4.4 节跳过对应效果；携票 Timer 必须终结票证，控制槽位异常则 terminal cleanup |
| attention sequence 耗尽或邮箱不一致 | 消息与 Timer 业务结果保持 | 协调器进入 terminal，停止声音/托盘、清理 Toast、终结 issued/admitted 票证，不回绕 |
| Timer 完成事件发送时 receiver 已断开 | 消息与 `fired` 保持成功 | 调用 Timer 权威终止方法吸收 `issued` 票证；不播放 |
| Focused、取消或 Shutdown 发送时 receiver 已断开 | 消息/Timer 状态保持 | 协调器进入 terminal，调用幂等 emergency-stop，不允许静默丢弃 |
| worker panic 或 receiver 断开 | 消息/Timer 状态保持 | CleanupGuard 停止声音、恢复托盘、清理 Toast 并终止全部 issued/admitted 票证 |
| 主窗口焦点后的停止失败 | 消息未读保持 | 逻辑上清空票证并停止提醒；记录诊断，不阻塞窗口 |
| 消息持久化失败 | 不提交消息；Timer 按原合同回退 | 不发送徽标、Toast、托盘或声音 |

## 10. 测试合同

### 10.1 自动化

使用 fake Toast、托盘、音频、快捷方式和事件端口覆盖：

1. 普通消息在 `mainFocused=false` 时产生 Toast、托盘和一次声音；为 true 时三项均抑制但徽标增加。
2. `PublishedAttention` 恰好派发一次；`TimerCompletion(None)` 有 Toast/托盘/徽标但绝不退化成普通声音。
3. barrier 测试：Timer 事件已排队，Reset 在 `admit_audio_start` 前提交，admission 拒绝且没有播放调用。
4. barrier 测试：admission 先提交，Reset 随后执行，只允许一次开始意图并必须处理取消停止。
5. Timer 在主窗口已聚焦时到期只执行 `issued -> confirmed`；`Focused(true)` admission 同步确认当时准确的
   issued/admitted 集合，worker 只移除事件携带的 admitted 列表，之后失焦、迟到或重复事件都不补响。
6. 焦点确认后再 Reset 不创建墓碑；同票证重复派发被吸收，新 round 仍可正常 admission。
7. 第一张 Timer 票证启动循环，后续票证不重启；逐张取消，集合非空继续、变空停止。
8. 主窗口焦点把全部 admitted 票证提交 confirmed、停止当前声音、恢复托盘，但不修改未读。
9. Timer 循环期间普通消息不调用音频；普通声音可被 Timer 抢占，普通消息之间允许重启。
10. 普通消息配额已满时，Focused、Timer 完成、取消和 Shutdown 仍被接收并执行。
11. receiver 断开、控制事件发送失败和 worker panic 都触发 CleanupGuard/emergency-stop；pending Timer 仍为
    issued、已经 admitted 以及两者混合时都由 `terminate_all_audio()` 终结；Shutdown 顺序幂等。
12. Timer 完成事件发送失败会终止 issued 票证，不留下可迟到播放的权威状态。
13. Toast、托盘和声音每种失败都不回滚消息或阻止其他端口；循环启动失败把匹配票证
    `admitted -> confirmed` 并移出活动集合，后续新票证仍可尝试。
14. AUMID 必须在 Builder 前设置；STA 快捷方式初始化/反初始化、MTA Toast worker 创建/释放、迟到回调
    吸收和固定点击路由使用 fake apartment/port 验证。
15. Debug/Release AUMID、快捷方式缺失创建、当前目标无 AUMID 的安全接管、已知旧 AUMID/旧目标修复、
    未知同名文件拒绝覆盖，以及安装/升级后的属性检查。
16. Toast XML 纯文本、系统关闭、同步失败、异步失败和退出清理。
17. Toast `Activated/Failed/Dismissed` 使用固定 `ToastCallback` DTO；同 ID 第一个终态胜出，重复/未知/迟到
    回调被吸收，Activated 只路由 Messages，Failed/Dismissed 不修改已读。
18. 有界邮箱压力测试覆盖 ordinary=64、TimerKey=64、Toast=64、focus FIFO=128、同 key 合并、满额新 key、
    sequence 耗尽和 Shutdown 独立唤醒；不存在无界备用队列。
19. 焦点顺序测试覆盖 `message -> true`、`true -> message`、`true -> false -> Timer` 和 worker 阻塞期间多次
    切换；旧消息先产生效果再由焦点停止，Toast 不撤回，失焦后签发的 Timer 票证不被旧焦点确认误伤。
20. `fired -> start(new round)`、Reset、生命周期撤销、revision/round/audio identity 耗尽都验证 admitted 票证
    产生锁后取消；即使公开操作返回 `TimerUnavailable` 也必须停止最后一票。
21. audio `issued -> admitted -> confirmed`、播放失败、焦点确认和取消都保持同一 fired revision；Reset 或
    新 Start 只因公开 Timer 转换递增一次，AudioTicket 的 firedRevision 不变。
22. 打包产物中的 WAV 必须解析为 RIFF/WAVE，大小和 SHA-256 与第 7.1 节一致，Tauri 资源路径可解析。
23. 现有消息中心、Demo、Pomodoro、Timer 状态机和完整 Rust/前端回归继续通过。

自动化不得调用真实 Toast、改变真实前台焦点或播放真实声音。

### 10.2 人工 Windows 验收

自动化通过后由用户在普通权限环境操作：

1. 在全新当前用户环境普通权限执行 `npm run tauri dev`，确认首次启动即创建/修复 Debug 身份并能显示
   右下角 Toast；不要求用户预先运行安装器。
2. 主窗口未聚焦时发布普通消息，确认 Toast、托盘闪烁、两个徽标和一次 WAV。
3. 主窗口已聚焦时发布普通消息，确认消息和徽标增加，但无 Toast、声音或新托盘提醒。
4. 主窗口已聚焦时让 Timer 到期，确认不响；随后让主窗口失焦，确认旧票证不补响。
5. 主窗口未聚焦时让 Timer 到期，确认消息、Toast、徽标及持续循环 WAV。
6. 打开 Launcher、Settings 或 Messages 任一页面，确认真实 `Focused(true)` 后声音立即停止、托盘恢复；
   非 Messages 页面保持徽标，进入 Messages 后徽标消失且历史消息保留。
7. 两个 Timer 先后到期，确认只有一条循环声音；取消一个仍继续，取消最后一个才停止。
8. 循环期间点击 Toast，确认打开并聚焦 Messages、停止声音且按既有合同标记已读。
9. 循环期间从托盘干净退出 UiPilot，确认声音停止、托盘恢复且进程退出不挂起。
10. Windows 设置中关闭 UiPilot 通知后发布消息，确认无 Toast，但托盘、徽标和声音仍工作。
11. 使用所有实际发布的普通权限安装包执行干净安装与升级安装，检查快捷方式目标/AUMID，并重复 Toast
    身份、标题、图标、点击消息页和声音验收。

Agent 必须在人工验收前通知用户并等待确认，绝不代替操作。

## 11. 验收标准

1. 普通权限 `tauri dev` 和正式安装包都能在主窗口未聚焦时显示具备正确 UiPilot 身份的 Toast。
2. 普通消息只在主窗口未聚焦且无 Timer 循环时播放一次固定 WAV。
3. Timer 到期先保存消息，再凭当前 TimerRecord 的 `issued -> admitted` 权威转换进入共享持续闹铃；
   Reset 或生命周期撤销先赢时绝不迟到播放。
4. 每个 TimerRecord 只保留一个有界 audio 状态；重复、旧轮次和已 confirmed 票证不补响，也不积累墓碑。
5. 多张 Timer 票证只共享一条声音；取消最后一张或主窗口聚焦才停止。
6. 主窗口聚焦确认注意效果但不标记消息已读；进入 Messages Tab 才清除徽标。
7. 普通消息和所有控制路径都有明确硬边界；焦点 FIFO 保留准确 sequence，Timer 票证在满额、耗尽、错误
   返回或 worker 失败时都终结全部 issued/admitted 状态，不存在无界备用队列。
8. 系统通知、托盘或音频任一失败不回滚消息、未读或 `fired`，也不阻止其他效果。
9. Debug/Release 在正确 COM/WinRT apartment 和 AUMID 下创建快捷方式/notifier；未知同名文件不被覆盖。
10. 插件不能控制 Toast、音频、AUMID、托盘、焦点确认或注意优先级。
11. 所有原生效果发生在消息提交和相关锁释放之后；事件顺序、票证 admission/取消、audio revision 边界、
    Toast 终态回调、控制槽位耗尽和 shutdown 均有确定性测试。

## 12. 结论

本设计不扩充插件 API，而是修复和统一宿主原生提醒层。消息中心继续负责持久化与未读，Timer 服务继续负责
权威计时和票证，新的协调器只负责进程内注意效果。有界合并邮箱让主窗口焦点、托盘和单一 Windows 声音
通道具有明确顺序且不会无限积压；独立 Toast 身份让普通权限 `dev` 与正式安装包使用各自稳定的 AUMID。
