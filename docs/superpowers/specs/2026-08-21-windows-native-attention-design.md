# UiPilot Windows 原生提醒协调器设计

## 1. 文档信息

- 日期：2026-08-21
- 状态：Draft，口头设计已确认，等待书面复核
- 范围：Windows Toast、托盘提醒、普通消息提示音、计时器持续闹铃
- 公开 API：不变

本设计是以下已批准规格的增量覆盖：

- [公开插件消息中心与 Windows 通知设计](./2026-08-18-public-plugin-message-center-design.md)
- [公开插件窗口计时 API 设计](./2026-08-20-public-plugin-window-timer-api-design.md)

除本节列出的覆盖项外，原规格中的持久化、未读、权限、插件请求、Timer 状态机、ClaimTicket、
AudioTicket、锁顺序和窗口会话合同继续有效。

本设计明确覆盖以下旧合同：

1. 消息中心旧规格 4.4 中“每条消息都尝试显示 Windows 通知且不按主窗口焦点抑制”改为：主窗口已聚焦时，
   本条消息不显示 Toast、不播放声音、不启动托盘提醒；消息与未读徽标仍正常提交。
2. Timer 旧规格 3.1、4、12.3、18.3、21 中“固定有限时长、只播放一次、不循环”改为：有效 Timer
   完成票证共享一条持续循环闹铃，直到主窗口任意页面获得原生焦点，或所有对应票证均被撤销。
3. 消息中心旧规格 13、16 中仅把 `tauri dev` 作为开发态冒烟的限制改为：普通权限的
   `npm run tauri dev` 和正式安装包都必须能显示具备正确 UiPilot 身份的 Toast。

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
- Timer 待确认票证集合清空，循环闹铃停止。

注意确认绝不读取消息存储、不写 `readAt`、不减少未读数。只有进入 Messages Tab 并成功执行现有
`open_and_mark_read` 合同，两个徽标才消失。`Focused(false)` 只更新焦点状态，不恢复已确认的旧提醒。

## 4. 架构

### 4.1 统一协调器

新增一个进程级 `NativeAttentionCoordinator`。它拥有唯一事件队列和工作线程，并通过窄端口调用：

- `MessageToastPort`：Toast 身份、显示、点击、失败与清理；
- `TrayAttentionPort`：正常图标与透明帧切换；
- `AttentionAudioPort`：单次播放、循环播放和停止。

消息中心在持久化成功并释放消息存储锁后派发消息事件。Timer 服务只在消息保存成功、`fired` 提交成功且
签发 AudioTicket 后派发 Timer 声音意图。主窗口的既有 `WindowEvent::Focused(bool)` 钩子向同一队列派发
焦点事件。

不增加第二套焦点查询，也不由前端上报焦点。生产初始焦点固定为 `false`，之后只接受原生焦点事件。

### 4.2 内部事件

内部事件的语义固定为：

```rust
enum NativeAttentionEvent {
    OrdinaryMessageCommitted(MessagePublished),
    TimerMessageCommitted {
        message: MessagePublished,
        audio_ticket: Option<AudioTicket>,
    },
    TimerAudioCancelled(AudioTicket),
    MainFocusChanged(bool),
    Shutdown,
}
```

`audio_ticket: None` 表示 Timer 完成消息已经保存，但 Reset 或生命周期变化使迟到完成不能播放声音。该消息仍
可显示 Toast、更新徽标和触发托盘，但绝不退化成普通一次提示音。

### 4.3 状态

协调器工作线程独占以下瞬时状态，不向插件或前端公开：

```text
mainFocused: bool
trayAttention: inactive | active | degraded
audio: silent | ordinaryOnce | timerLoop
pendingTimerTickets: set<AudioTicket>
cancelledTimerTickets: set<AudioTicket>
mode: running | terminal
```

不使用跨线程共享互斥锁保护这些字段。生产调用方只向队列发送事件；原生 Toast、托盘和音频调用只发生在
工作线程或各适配器自有回调中。`cancelledTimerTickets` 只用于吸收“取消先处理、同票证播放后到达”的竞态；
匹配的迟到 Timer 事件到达时删除墓碑，Shutdown 清空全部墓碑。实现应复用现有 TimerAlarm 的取消过滤语义，
不得创建第二套独立票证含义。

## 5. 事件顺序与线性化

### 5.1 消息提交

消息持久化成功仍是插件 `publish()` 和 Timer 完成消息不可撤销的成功线性化点。顺序固定为：

1. 在消息存储锁内提交消息与新 revision；
2. 释放消息存储锁；
3. 向 `main` 发送现有消息状态事件；
4. 向原生提醒协调器发送对应事件；
5. 不等待用户看到、点击、关闭 Toast 或听到声音。

第 3 或第 4 步失败不改变持久化成功，也不修改插件 Promise 的成功结果。

### 5.2 焦点与消息竞态

消息与 `MainFocusChanged` 共用一个先进先出事件队列。入队顺序是注意效果的线性化顺序：

- 焦点事件先处理：后到消息读取 `mainFocused = true`，不产生原生注意效果；
- 消息先处理：允许开始原生效果；随后焦点事件会停止声音和托盘；
- `MainFocusChanged(false)` 不会重新播放或重新闪烁已经确认的旧事件。

主窗口请求 `show()` 或 `focus()` 不算确认；只有随后实际收到的 `Focused(true)` 才确认。

### 5.3 Timer 票证竞态

Timer 仍由既有 ClaimTicket/AudioTicket 合同决定消息与声音资格。协调器只接受 Timer 服务签发的
AudioTicket，不自行构造或推断票证。

- `TimerAudioCancelled` 先于 `TimerMessageCommitted`：票证进入取消墓碑集合；匹配的迟到事件删除墓碑，
  不加入待确认集合、不播放；
- `TimerMessageCommitted` 先于取消：先加入集合并可能开始播放，取消随后移除；
- 主窗口焦点先于 Timer 事件：票证视为已确认，不播放；
- Timer 事件先于主窗口焦点：允许播放，焦点随后清空全部集合并停止。

取消、焦点确认和重复事件均幂等。任何票证最多使共享循环从空集合启动一次，绝不保存第二条完成消息。

## 6. Windows Toast 身份

### 6.1 身份值

- Debug：`com.uipilot.launcher.dev`
- Release：`com.uipilot.launcher`

在创建 Tauri 窗口、托盘或 ToastNotifier 前，Windows 入口设置当前进程的显式 AppUserModelID。

### 6.2 每用户快捷方式

未打包桌面应用显示 Toast 必须存在带 `System.AppUserModel.ID` 的开始菜单快捷方式。宿主在普通用户权限下
验证并按需创建：

- Debug：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\UiPilot Dev.lnk`
- Release：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\UiPilot.lnk`

快捷方式目标是当前可执行文件，工作目录是可执行文件父目录，AppUserModelID 必须与当前构建匹配。创建和
替换使用同目录临时文件加原子提交，不留下半写快捷方式。

若已存在的同名快捷方式带有预期 UiPilot AUMID 但目标已过期，宿主修复目标。若同名文件不带 UiPilot
AUMID，宿主不得覆盖未知用户文件，本次进程只将 Toast 适配器降级为 no-op。正式安装器可以预创建同一
Release 快捷方式；运行时验证保持幂等。

微软要求与依据：

- <https://learn.microsoft.com/en-us/windows/win32/shell/enable-desktop-toast-with-appusermodelid>
- <https://learn.microsoft.com/en-us/windows/win32/shell/quickstart-sending-desktop-toast>

### 6.3 Toast 行为

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
- `NativeAttentionCoordinator` 的队列所有权必须在消息发布和 Timer worker 启动前建立。
- 控制器或工作线程构造失败时安装本次进程固定 terminal 的 no-op 原生提醒端口；UiPilot 继续启动，消息和
  徽标仍可用。
- 应用退出发送 `Shutdown`，停止声音、清空票证、恢复托盘并尽力取消活动 Toast。Shutdown 可重复。
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
| 原生提醒事件发送失败 | 消息保持成功 | 本次原生效果丢弃，不重试 |
| 主窗口焦点后的停止失败 | 消息未读保持 | 逻辑上清空票证并停止提醒；记录诊断，不阻塞窗口 |
| 消息持久化失败 | 不提交消息；Timer 按原合同回退 | 不发送徽标、Toast、托盘或声音 |

## 10. 测试合同

### 10.1 自动化

使用 fake Toast、托盘、音频、快捷方式和事件端口覆盖：

1. 普通消息在 `mainFocused=false` 时产生 Toast、托盘和一次声音；为 true 时三项均抑制但徽标增加。
2. Timer 完成产生 Toast、托盘和循环声音，不产生普通一次声音。
3. Timer 循环期间普通消息不调用音频，消息、Toast、托盘和徽标仍成功。
4. 第一张 Timer 票证启动循环，后续票证不重启；逐张取消，集合非空继续、变空停止。
5. 主窗口焦点一次清空全部 Timer 票证、停止当前声音、恢复托盘，但不修改未读。
6. 消息/焦点、播放/取消、播放/Reset、播放/生命周期变化的两种入队顺序。
7. 主窗口已经聚焦时到期不播放，之后失焦也不补响。
8. 普通声音被 Timer 抢占、普通消息之间允许重启，以及 Shutdown 幂等停止。
9. Toast、托盘和声音每种失败都不回滚消息或阻止其他端口。
10. Debug/Release AUMID、快捷方式缺失创建、预期 AUMID 的旧目标修复、未知同名文件拒绝覆盖。
11. Toast XML 纯文本、系统关闭、同步失败、异步失败、点击固定消息路由和退出清理。
12. 现有消息中心、Demo、Pomodoro、Timer 状态机和完整 Rust/前端回归继续通过。

自动化不得调用真实 Toast、改变真实前台焦点或播放真实声音。

### 10.2 人工 Windows 验收

自动化通过后由用户在普通权限环境操作：

1. `npm run tauri dev`，主窗口未聚焦时发布普通消息，确认右下角 Toast、托盘闪烁、两个徽标和一次 WAV。
2. 主窗口已聚焦时发布普通消息，确认消息和徽标增加，但无 Toast、声音或新托盘提醒。
3. Timer 到期，确认消息保存、Toast 与徽标出现，并持续循环播放 WAV。
4. 打开 Launcher、Settings 或 Messages 任一页面，确认窗口真正获得焦点后声音立即停止、托盘恢复；徽标仍在。
5. 进入 Messages Tab 后确认徽标消失，历史消息保留。
6. 两个 Timer 先后到期，确认只存在一条循环声音；取消其中一个不会停止，主窗口聚焦后全部停止。
7. Windows 设置中关闭 UiPilot 通知后发布消息，确认无 Toast，但托盘、徽标和声音仍工作。
8. 使用正式普通权限安装包重复 Toast 身份、标题、图标、点击消息页和声音验收。

Agent 必须在人工验收前通知用户并等待确认，绝不代替操作。

## 11. 验收标准

1. 普通权限 `tauri dev` 和正式安装包都能在主窗口未聚焦时显示具备正确 UiPilot 身份的 Toast。
2. 普通消息只在主窗口未聚焦且无 Timer 循环时播放一次固定 WAV。
3. Timer 到期先保存消息，再凭有效 AudioTicket 进入共享持续闹铃。
4. 多张 Timer 票证只共享一条声音；取消最后一张或主窗口聚焦才停止。
5. 主窗口聚焦确认注意效果但不标记消息已读；进入 Messages Tab 才清除徽标。
6. 系统通知、托盘或音频任一失败不回滚消息、未读或 `fired`，也不阻止其他效果。
7. 插件不能控制 Toast、音频、AUMID、托盘、焦点确认或注意优先级。
8. 所有原生效果发生在消息提交和相关锁释放之后，事件顺序与票证竞态有确定性测试。

## 12. 结论

本设计不扩充插件 API，而是修复和统一宿主原生提醒层。消息中心继续负责持久化与未读，Timer 服务继续负责
权威计时和票证，新的协调器只负责进程内注意效果。统一事件队列让主窗口焦点、托盘和单一 Windows 声音
通道具有明确顺序；独立 Toast 身份让普通权限 `dev` 与正式安装包使用各自稳定的 AUMID。
