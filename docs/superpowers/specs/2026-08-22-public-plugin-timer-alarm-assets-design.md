# UiPilot 公开插件计时闹铃资源设计

## 1. 文档信息

- 日期：2026-08-22
- 状态：Draft，第一轮独立审核 findings 已处理，等待复审
- 范围：公开插件计时闹铃的私有包资源、安装校验、激活身份、原生播放与声音仲裁
- 公开 JavaScript API：不变
- Manifest 字段：不变
- 新权限：无

本设计是以下已批准规格的增量覆盖：

- [公开插件窗口计时 API 设计](./2026-08-20-public-plugin-window-timer-api-design.md)
- [UiPilot Windows 原生提醒协调器设计](./2026-08-21-windows-native-attention-design.md)

除本文明确列出的音频条款外，原规格中的消息持久化、未读、Toast、托盘、Timer 状态机、ClaimTicket、
AudioTicket、窗口会话、焦点确认、邮箱边界、锁顺序和失败关闭合同继续有效。

### 1.1 精确覆盖矩阵

| 旧规格位置 | 被替换的条款 | 当前唯一条款 |
|---|---|---|
| Timer 3.1 第 3 项、3.2“插件自定义音频”、4“闹铃”、6.1 权限说明、12.3 资源与播放、18.3、19、20.5 音频分支、21 第 6 项、22 第 4-5 项、23 音频结论 | Timer 使用固定宿主闹铃，插件不携带音频 | `timer.control` 插件必须携带固定私有闹铃；插件仍不能通过 JavaScript 控制播放 |
| 原生提醒 2.1 第 3 项、2.2“插件指定音频”、3.2 Timer 声音、7.1 固定资源、7.2 文件播放 | 普通消息与 Timer 使用宿主资源路径 | 普通消息使用宿主公共提示音；Timer 使用签票时冻结的插件私有内存闹铃 |
| 原生提醒 3.3、4.3 的 `activeTimerTickets`/共享声音字段、5.3 多票共享分支、8 中相应音频生命周期、10.1 第 6-9 项的共享集合断言、11 第 5 项 | 多张票共享一条声音，最后一票撤销才停止 | 单一 `timerAudioOwner + alarmEpoch`；后续票证不叠加、不切换、不候补 |
| Timer 18.3 与原生提醒 9 的音频失败条款 | 固定资源或设备失败只记录诊断，或无法区分地归入单一音频失败 | 安装期资产错误拒绝包；运行期原生播放失败属于宿主音频降级，不停用插件 |

以下合同明确不被覆盖：原生提醒 4.4 邮箱硬边界、5.1 消息提交、5.2 焦点 sequence 与双锁线性化、6 Toast
身份、8 中非音频锁顺序与 shutdown、9 中 Toast/托盘失败、10 中非音频测试，以及 Timer 的公开状态、
revision、ClaimTicket、消息 delivery admission 和窗口会话合同。

## 2. 目标与非目标

### 2.1 目标

1. 保持普通消息提示音为主程序公共能力。
2. 让每个 `timer.control` 插件携带自己的计时到期闹铃。
3. 把插件闹铃作为宿主私有资源，不让 Runtime 或插件内容 WebView 读取或播放。
4. 用不可复用激活身份和不可变内存字节把 Timer 票证绑定到准确的插件安装实例。
5. 由宿主验证、持有和循环播放闹铃，不扩大为通用音频 API。
6. 保持消息提交与音频副作用隔离；音频失败不回滚消息、Toast、托盘或徽标。

### 2.2 非目标

- 不新增 `audio.play`、暂停、停止、音量、声道或文件选择 API。
- 不允许 Runtime 或插件窗口传递路径、URL、字节、MIME 或循环参数。
- 不允许插件替换普通消息提示音。
- 不支持 MP3、AAC、OGG、WebAudio、远程音频或系统外部路径。
- 不增加用户级或单插件音量设置、静音时段和声音选择器。
- 不保留当前预发布版本的旧插件兼容逻辑。
- 不修改 `notifications.publish()`、`notifications.schedule()`、消息 DTO 或公开 Timer DTO。

## 3. 术语与身份

- **public package resource**：可经公开插件自定义协议返回给 Runtime 或插件窗口的 HTML、JavaScript、CSS 等资源。
- **host-private alarm resource**：参与包校验和摘要、但永远不进入 Web 资源映射的固定闹铃。
- **activationId**：每个原生进程内全局分配、永不复用的内部 `u64` 激活身份，不跨 Rust/JavaScript 边界。
- **AlarmAssetIdentity**：绑定一次精确安装实例及闹铃内容的不可变身份。
- **ValidatedAlarmAsset**：`AlarmAssetIdentity` 与已完整验证的 `Arc<[u8]>` PCM WAV。
- **AlarmAssetRegistry**：ActivationBundle 管理器内部按 activationId 索引 ValidatedAlarmAsset 的宿主私有映射；
  它与 Bundle 在同一线性化点发布或移除，不是独立持久化事实源。
- **ActivationBundle**：一次激活原子发布的 config、RuntimeSnapshot、私有闹铃、generation 和 activationId。
- **alarmEpoch**：原生提醒邮箱内划分闹铃 owner 竞争批次的单调 `u64`。
- **timerAudioOwner**：当前唯一获准循环播放的 AudioTicket 与 ValidatedAlarmAsset。

`pluginGeneration`、`activationId`、`packageDigest`、`roundId`、`audioId`、`firedRevision`、`alarmEpoch` 和
`attentionSequence` 含义不同，不能互换。

`AlarmAssetIdentity` 至少包含：

```text
pluginId
+ pluginGeneration
+ activationId
+ packageDigest
+ resourceSha256
+ fixedRelativePath
```

`TimerKey` 必须包含 `pluginId + activationId`；可以同时保留 generation 供诊断，但不能只凭 generation
授权。ClaimTicket 和 AudioTicket 通过 TimerKey 继承 activationId。AudioTicket 的完整匹配仍包括
`roundId + audioId + firedRevision`。

## 4. 用户合同

### 4.1 普通消息提示音

主程序继续内置：

```text
resources/sounds/message-notification.wav
```

普通消息成功持久化且主窗口未聚焦时，宿主按既有原生提醒合同播放该声音一次。所有插件共用此提示音，插件
不能声明、覆盖或选择普通消息提示音。

普通消息提示音缺失或播放失败只使本次宿主音频副作用降级，不改变插件状态，不回滚消息，也不尝试插件闹铃。

### 4.2 插件计时闹铃

任何声明 `timer.control` 的插件包都必须包含精确路径：

```text
assets/sounds/timer-alarm.wav
```

路径和文件名区分大小写并必须完全匹配。Manifest 不增加音频路径字段；固定路径是 `timer.control` 包合同的一部分。

Timer 到期消息成功持久化、ClaimTicket 成功提交 `fired`、AudioTicket 通过权威 admission 且取得当前
alarmEpoch owner 后，宿主循环播放签票时冻结的 ValidatedAlarmAsset。插件不能决定是否循环、循环次数或
停止条件。

用户让主窗口产生原生 `Focused(true)` 后，宿主立即停止当前循环闹铃。注意确认不标记消息已读，消息记录和
未读徽标继续保留，直到用户进入消息页并完成既有已读流程。

### 4.3 声音优先级

Timer 循环闹铃具有最高声音优先级：

- 循环闹铃实际播放期间，普通消息仍保存、显示 Toast、更新徽标并触发托盘，但不播放
  `message-notification.wav`；
- 闹铃停止后到达的新普通消息可以恢复播放公共提示音；
- 已被抑制的普通提示音不补播。

## 5. Manifest、权限与包结构

### 5.1 权限

不新增音频权限。现有 `timer.control` 继续要求完整权限组合：

```json
"permissions": ["ui.window", "notifications.publish", "timer.control"]
```

`timer.control` 同时授权窗口内计时控制和宿主在到期时播放该插件的受限闹铃。它不授予任意播放权限。

安装或更新仍要求用户完整同意 Manifest 声明的权限；运行时权限、session、activationId 和票证检查继续作为
纵深防御。

### 5.2 固定资源

合法计时插件的相关结构为：

```text
package/
  plugin.json
  assets/
    sounds/
      timer-alarm.wav
  dist/
    runtime.js
    window.html
    window.js
    window.css
```

规则固定为：

- 声明 `timer.control` 但缺少固定闹铃：拒绝安装或更新；
- 未声明 `timer.control` 却携带固定闹铃：拒绝安装或更新；
- 包内出现其他 `.wav` 文件：拒绝安装或更新；
- 闹铃计入既有单文件、文件数量和包总大小边界；
- 闹铃进入包资源索引、SHA-256 树摘要和受控 staging 快照；
- 闹铃不得进入 RuntimeSnapshot 的公开资源映射，也不能引用包外文件。

### 5.3 源文件与 staging 文件

开发目录中的源闹铃允许是硬链接，因为 staging 必须复制内容而不是移动、复用或继续引用源文件。复制后的
staging 闹铃必须是普通独立文件，链接数必须为 `1`。源符号链接和 Windows 重解析点仍在复制前拒绝；
staging 中的符号链接、重解析点或多链接文件同样拒绝。

归档解压也必须创建普通独立文件。后续校验、激活和播放只使用 staging 产生的受控快照或其内存字节，源目录
变化不再影响插件。

## 6. 严格 WAV 合同

安装器必须解析完整文件内容，不能按扩展名、调用方 MIME 或文件头片段直接放行。

### 6.1 物理结构

MVP 只接受以下精确结构：

```text
RIFF header
fmt  chunk (size = 16)
data chunk
optional one-byte zero padding for odd data length
EOF
```

规则固定为：

1. 文件物理长度必须精确等于 RIFF size 字段加 `8`，禁止尾随字节。
2. bytes `0..4` 必须是 `RIFF`，bytes `8..12` 必须是 `WAVE`。
3. 必须恰好有一个 `fmt ` chunk，位于 RIFF header 后，长度必须为 `16`。
4. `fmt ` 后必须恰好有一个 `data` chunk。
5. 不允许 `LIST`、`JUNK`、`bext`、`fact`、重复 chunk 或其他未知 chunk。
6. `data` 长度为奇数时，EOF 前必须有且仅有一个值为 `0` 的 RIFF padding byte；偶数时不得有 padding。
7. 所有长度与 offset 运算使用检查算术，任何溢出、越界或截断均拒绝。

### 6.2 PCM 参数

| 项目 | 限制 |
|---|---|
| 编码 | little-endian integer PCM，format tag `1` |
| 声道 | 1 或 2 |
| 采样率 | 44,100 Hz 或 48,000 Hz |
| 位深 | 16-bit 或 24-bit |
| 单文件大小 | `1..=2 MiB` |
| 有效音频时长 | 至少 1 frame，最多 15 秒 |

必须满足：

```text
bytesPerSample = bitsPerSample / 8
blockAlign = channels * bytesPerSample
byteRate = sampleRate * blockAlign
dataLength % blockAlign = 0
frames = dataLength / blockAlign
1 <= frames <= sampleRate * 15
```

全部计算使用检查整数，不使用浮点数。任何字段不一致、伪装格式或超限输入统一返回现有安装失败结果，不向
插件暴露宿主磁盘路径或底层解析错误。

## 7. 宿主私有资源与 WebView 隔离

### 7.1 资源映射隔离

ActivationBundle 必须把包资源拆成：

```text
RuntimeSnapshot.publicResources
ValidatedAlarmAsset (host-private)
```

固定闹铃只存在于后者。以下请求无论来自 staged Runtime、active Runtime 还是插件内容窗口，均固定返回 `403`：

```text
/assets/sounds/timer-alarm.wav
```

通用 `asset()`、`window_asset()` 或等价协议处理器不能通过遍历包资源表间接返回该文件。错误响应不泄露插件
安装路径、资源摘要或是否已加载到内存。

AlarmAssetRegistry 是 ActivationBundle 内存索引的一部分。读取方必须先取得一个完整 Bundle 快照，再从其中
取得私有资产；不得把 config、RuntimeSnapshot 和 alarm registry 分别读取后自行拼装当前状态。

### 7.2 WebView 音频隔离

所有公开插件 Runtime WebView 和插件内容 WebView 必须在原生层永久静音：

- Windows WebView2 在第一次导航和执行插件脚本前设置 `IsMuted = true` 或等价原生能力；
- reload、窗口隐藏/显示、重新建立 session 和内容重载后仍保持静音；
- 插件 JavaScript 不获得解除静音的桥接命令；
- 原生静音失败时，相关 Runtime 或插件窗口创建失败关闭，不能以未静音状态继续；
- 主窗口 WebView 不受影响；普通消息音和 Timer 闹铃由宿主原生音频通道播放。

插件协议响应的 CSP 还必须包含：

```text
media-src 'none'
```

CSP 和固定路径 `403` 是纵深防御；原生 WebView 静音才是阻止 `<audio>`、data URL、WebAudio oscillator 等任意
插件声音的最终边界。

## 8. ActivationBundle 与不可复用身份

### 8.1 Bundle

一次候选激活必须在可见提交前完整构造：

```text
ActivationBundle {
  config,
  runtimeSnapshotWithPublicResourcesOnly,
  validatedAlarmAsset,
  pluginGeneration,
  activationId,
}
```

非 `timer.control` 插件的 `validatedAlarmAsset` 为 null；计时插件必须为非 null。

### 8.2 activationId

原生进程维护一个所有公开插件共用的检查递增 `u64` 分配器。每个候选 Bundle 在建立内部身份时取得新的
activationId；准备失败产生的空洞不复用。完全卸载、保留数据卸载、故障恢复、同版本重装和更新都不能在
同一进程复用旧 activationId。

activationId 不持久化也不跨 JavaScript 边界。原生进程重启会清除所有 Timer、票证、邮箱事件和 owner，
因此新进程可以重新开始分配。分配器不得回绕；耗尽时本进程拒绝继续准备或激活公开插件，已有插件保持当前
状态，重启后恢复。

完全卸载后重新安装即使公开 pluginGeneration 再次为 `1`，activationId 也必然不同。任何旧事件、取消、
播放结果或故障结果都必须比较 activationId 和完整票证，不能影响重装后的 Bundle。

### 8.3 私有内存资产

候选阶段读取闹铃后必须：

1. 按第 6 节完整解析；
2. 重新计算 SHA-256 并匹配包资源索引；
3. 构造完整 AlarmAssetIdentity；
4. 将完整文件保存为 `Arc<[u8]>`；
5. 在提交前再次确认 packageDigest 与候选 Bundle 一致。

激活后不再从插件安装目录打开闹铃文件。Timer 新轮次从当前 Bundle 克隆 ValidatedAlarmAsset，并与完成消息
一起冻结到该 round。成功提交 `fired` 后，内部 TimerCompletion 携带：

```text
AudioTicket + AlarmAssetIdentity + Arc<[u8]>
```

原生协调器不能根据“当前 pluginId + generation”重新查询或替换声音。即使旧包随后删除，已冻结内存也保持
有效；票证失效仍会阻止它播放。

## 9. 原子安装、更新与卸载

### 9.1 候选准备

目录安装和 `.uipilot-plugin` 归档安装使用同一条 staging 流程：

1. 复制或解压为受控 staging 快照；
2. 扫描资源并解析 Manifest；
3. 按 `timer.control` 验证固定闹铃存在性和唯一性；
4. 完成严格 WAV 校验、资源摘要和私有内存资产构造；
5. 构造只含公开 Web 资源的 RuntimeSnapshot；
6. 设置 staged Runtime WebView 原生静音并完成 Runtime ready；
7. 将新包写入唯一、尚未激活的持久目录；
8. 准备新的持久状态临时文件和完整候选 ActivationBundle。

上述步骤失败会删除候选事务，不写入插件列表，不替换旧 Bundle。

### 9.2 提交协议

所有插件 mutation 路径必须识别每插件 commit reservation。提交顺序固定为：

1. 短暂取得 plugin mutation guard，CAS 验证准备时捕获的旧 activationId、generation 和 packageDigest；
2. 为该 pluginId 安装唯一 commit reservation，释放 guard；其他 mutation 在 reservation 清除前失败或等待，
   读取方继续看到完整旧 Bundle；
3. 在所有运行时锁之外执行持久状态临时文件的原子替换；失败则重新取得 guard、清除 reservation、删除候选，
   旧 Bundle 和旧 Timer 完全不变；
4. 持久提交成功后重新取得 plugin mutation guard，验证同一 reservation，再按固定顺序取得 Timer 与
   ActivationBundle 内存锁；
5. 在不执行 I/O 的临界区内，以不可失败的内存交换发布新 Bundle，同时撤销旧 Timer、ClaimTicket、
   AudioTicket、窗口 session 和 owner 权威；
6. 清除 reservation 并释放所有锁；
7. 锁外派发取消效果、销毁旧窗口并清理旧包。

第 5 步的可见 Bundle 交换是运行时激活线性化点。持久状态已在第 3 步提交，但 reservation 阻止进程内观察到
半提交状态；若进程在第 3 与第 5 步之间退出，新进程从已提交状态和新包重建完整 Bundle，旧内存 Timer 已随
进程消失。

线性化点后的窗口销毁或旧包清理失败只记录并延后重试，不能回滚新 Bundle。更新失败时旧 generation、旧
内存闹铃、旧 Runtime 和旧 Timer 均继续有效。

### 9.3 卸载与生命周期

禁用、故障停用、卸载或成功更新必须先在 `plugin mutation -> timer -> activation bundle` 固定锁顺序下撤销
对应 activationId，再在锁外停止 owner、销毁窗口和删除包。不得先删除闹铃文件或注册项再派发取消。

卸载持久化使用相同 reservation 协议。完全卸载可以删除公开 generation 状态，但不影响进程内 activationId
分配器；立即重装仍取得不同 activationId。

## 10. Timer 冻结与票证资格

Timer 新 round 只能在 active window session 中从同一当前 ActivationBundle 取得：

```text
TimerKey(pluginId, activationId)
FrozenCompletion
ValidatedAlarmAsset
```

`pluginGeneration + packageDigest` 同时保存在冻结身份中供完整核对。到期流程继续遵守既有 ClaimTicket 和
消息 delivery admission：

1. `running -> claiming` 签发 ClaimTicket；
2. lifecycle admission 复核同一 activationId；
3. 锁外原子保存完成消息；
4. 票证仍有效才提交 `fired` 并签发含 `audioId` 的 AudioTicket；
5. 内部 TimerCompletion 携带 AudioTicket 与签票时冻结的 ValidatedAlarmAsset；
6. 消息保存失败或票证撤销时不产生可播放提交。

Runtime、窗口 session、TimerRecord、TimerCompletion、取消事件和原生播放结果均必须匹配 activationId。
完全卸载后旧事件即使其 generation、roundId、audioId 和 revision 数值碰巧与新安装相同，也因 activationId
不同而被拒绝。

## 11. 单 owner 与 alarmEpoch

### 11.1 邮箱身份

原生提醒邮箱维护检查递增的 `alarmEpoch`。每个 TimerCompletion 在邮箱 admission 时记录：

```text
attentionSequence + alarmEpoch + AudioTicket + AlarmAssetIdentity
```

alarmEpoch 不跨公开边界，不代替 attentionSequence。attentionSequence 继续决定 Toast、托盘、焦点和控制事件
的总顺序；alarmEpoch 只决定某批 Timer 是否仍有资格竞争声音。

### 11.2 owner 竞争

Worker 对每个 TimerCompletion 执行：

1. 若事件 epoch 不是当前 epoch，或当前 epoch 已被 claim，凭票证提交为不播放终态；
2. 否则先通过 Timer 权威 admission 验证完整 AudioTicket；无效票证不 claim epoch，下一张有效票仍可竞争；
3. admission 成功后，短暂取得邮箱锁，CAS 验证事件仍属于当前未 claim epoch；
4. CAS 成功即在任何原生播放 I/O 前原子设置 `epochClaimed = true` 并建立 timerAudioOwner reservation；
5. 同一 epoch 内其他票证永久失去竞争资格，即使 owner 启动失败、取消或焦点随后到达也不能候补；
6. owner 清除时 alarmEpoch 检查递增，并重新开放新 epoch；只有新 epoch 开始后 admission 的新事件可以竞争。

如果步骤 2 已把票证转为 admitted，但步骤 3 的 CAS 失败，必须把同一票证提交为 confirmed，不得留下活动
audio 状态。

### 11.3 owner 清除

以下事件仅在完整身份匹配当前 owner 时停止循环并清除 reservation：

- 主窗口 `Focused(true)`；
- owner Reset 或从 fired 开始新 round；
- owner 插件禁用、故障停用、卸载或成功更新；
- owner 原生播放启动失败；
- 进程 shutdown。

清除 owner 与递增 alarmEpoch 是同一邮箱临界区内的线性化动作。已 stamp 旧 epoch 但尚未处理的事件不能在
清除后成为候补。焦点 admission 继续使用既有 `Timer -> attention admission` 双锁规则：同步确认当时当前的
Timer audio 权威并推进 alarmEpoch；worker 随后按 attentionSequence 停止真实声音。

alarmEpoch 不得回绕。耗尽时 Timer 闹铃 admission 进入进程内失败关闭，终结 pending owner/票证并停止声音；
消息、Toast、托盘、徽标和普通消息公共提示音继续按各自合同运行，重启后恢复 Timer 闹铃 admission。

## 12. Windows 内存播放与缓冲区生命周期

Timer 闹铃使用：

```text
PlaySoundW(memoryPointer, ..., SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT)
```

不得为 Timer 使用 `SND_FILENAME`，也不得在播放阶段重新打开插件文件。

timerAudioOwner 必须持有完整 `Arc<[u8]>`，直到宿主成功调用全局停止并确认返回后才能释放。停止顺序固定为：

1. 先从权威状态撤销 owner，禁止新事件复活它；
2. 调用 `PlaySoundW(NULL, ..., 0)` 或等价停止；
3. 停止成功后释放 owner 字节；
4. 停止失败时把该 Arc 移入进程生命周期 quarantine，保持地址有效直到进程退出，并把 Timer 音频通道设为
   terminal，避免异步 WinMM 继续读取已释放内存。

启动返回失败时 Windows 未接受异步播放，当前票证终结且不保留 owner 字节；同一 epoch 的旧后续票证仍不能
候补。清除并推进到新 epoch 后，新到期轮次可以再次尝试，除非音频通道已因不安全的停止失败进入 terminal。

Windows 音频后端仍是一个进程级声音通道。Timer owner 开始前停止当前普通提示音；Timer 实际循环期间抑制
后续普通提示音。

## 13. 失败行为

### 13.1 安装期失败

以下情况拒绝安装或更新：

- `timer.control` 插件缺少固定闹铃；
- 非计时插件携带 WAV；
- 路径大小写错误、路径穿越、符号链接、重解析点、staging 多链接或大小写折叠冲突；
- WAV 物理结构、大小、时长或 PCM 参数不符合第 6 节；
- staging 前后摘要不一致；
- 私有资源仍出现在 RuntimeSnapshot 公开映射；
- Runtime WebView 无法在执行插件代码前原生静音。

安装期失败不存在“已安装但使用默认闹铃”的状态。

### 13.2 运行期身份与原生失败

- activationId、票证或资产身份不匹配：视为迟到/已撤销事件，静默拒绝，不停用当前插件；
- `PlaySoundW` 启动失败：保持已保存消息与 `fired`，终结当前票证，不重试本轮，不停用插件；
- 停止失败：按第 12 节 quarantine 字节并使 Timer 音频通道 terminal，不停用插件；
- 宿主按错误类型限频记录不包含敏感路径的稳定诊断；
- 后续新 Timer round 可以重新尝试启动，除非 Timer 音频通道已 terminal；
- 普通消息公共提示音失败同样不改变消息来源插件状态。

资产内容问题只能在构造 ValidatedAlarmAsset 前出现并拒绝候选。激活后使用不可变内存，不存在“磁盘文件后来
损坏导致停用插件”的路径。

### 13.3 原子提交失败

- reservation 前失败：旧 Bundle 完整保留；
- reservation 后持久替换失败：清除 reservation，删除候选，旧 Bundle 和旧 Timer 完整保留；
- 持久替换后进程退出：新进程从新状态重建，旧内存副作用随进程结束；
- Bundle 线性化后清理失败：新 Bundle 保持成功，旧清理延后重试，不回滚。

## 14. SDK 与开发者体验

公开 JavaScript API、Timer DTO 和 Manifest Schema 不增加字段。需要同步更新：

- 第三方插件开发指南：说明固定路径、严格 WAV、普通消息音与插件闹铃的边界；
- 番茄时钟示例：在包内提供 `assets/sounds/timer-alarm.wav`；
- 安装权限说明：`timer.control` 到期后会播放插件随包提供的受限闹铃；
- 打包和发布检查清单：验证闹铃存在且归档包含精确路径。

插件开发者不需要、也不能从 Runtime 或窗口 JavaScript 获取闹铃路径、字节、摘要或播放控制。

## 15. 迁移

当前尚未发布，不提供旧格式兼容：

1. 主程序保留 `resources/sounds/message-notification.wav`；
2. 主程序删除宿主 `resources/sounds/attention-alarm.wav` 及对应打包声明；
3. 将现有闹铃复制到
   `examples/public-plugins/com.uipilot.pomodoro/package/assets/sounds/timer-alarm.wav`；
4. 番茄时钟示例提高版本号；
5. 已安装的旧番茄时钟插件需要完全卸载后重新安装新包；
6. 启动时发现旧计时插件缺少闹铃时，不自动补文件或使用宿主资源，按现有包重验证/故障路径处理。

## 16. 测试合同

### 16.1 包与 WAV

- 合法目录包和归档包均能安装；
- `timer.control` 缺少闹铃、错误大小写、额外 WAV、非计时插件携带 WAV 均被拒绝；
- 精确覆盖 RIFF size、唯一 `fmt`/`data`、固定顺序、未知/重复 chunk、奇数 padding、尾随字节和检查算术；
- 表驱动覆盖错误编码、声道、采样率、位深、blockAlign、byteRate、零帧、超时长和超大小；
- 源硬链接被复制为 staging 单链接文件；符号链接、重解析点和 staging 多链接拒绝；
- 更新失败保留旧 Bundle、旧 Timer 和旧闹铃。

### 16.2 Web 隔离

- Runtime 与窗口请求固定闹铃均得到 `403`，其他合法公开资源仍可读取；
- RuntimeSnapshot 资源枚举不包含闹铃，AlarmAssetRegistry 包含匹配 activationId 的私有内存资产；
- CSP 包含 `media-src 'none'`；
- Runtime 与插件内容 WebView 在首个脚本执行前已原生静音，reload/重建仍静音；
- 原生静音设置失败时窗口/Runtime 创建失败关闭；
- 插件无法通过 `<audio>`、data URL 或 WebAudio oscillator 产生可听声音。

### 16.3 身份与卸载重装

- 完全卸载后立即重装可以让 generation 再为 `1`，但 activationId 不同；
- 旧 publish、cancel、focus、start success/failure 结果不能操作新 Bundle；
- AudioTicket 比较包含 TimerKey activationId、roundId、audioId 和 firedRevision；
- TimerCompletion 携带签票时冻结的资产身份与字节，不按当前 pluginId/generation 重查；
- activationId 和 alarmEpoch 耗尽均不回绕并按各自范围失败关闭。

### 16.4 原子激活

- 候选准备、Runtime ready、包持久化、状态替换和 Bundle 发布逐步故障注入；
- reservation 阻止并发 enable/disable/update/uninstall 观察或制造半提交状态；
- 持久替换失败旧 Bundle 完整保留；
- 线性化点同时发布 config/runtime/asset 并撤销旧 Timer 权威；
- 卸载先撤销 Bundle/owner，再锁外删除包；
- 线性化后清理失败不回滚新 Bundle。

### 16.5 owner 与竞态

- 第一张有效票证 claim epoch 并循环，后续同 epoch 票证不调用 start、不候补；
- 第一张无效票证不 claim epoch，下一张有效票可竞争；
- start 进行中并发到期事件 stamp 旧 epoch，start 失败后不能候补；
- owner 取消、焦点、Reset、更新和 shutdown 与 start 使用 barrier 覆盖两种线性化顺序；
- owner 清除后 admission 的新 epoch 票证可以成为新 owner；
- 闹铃实际播放期间普通消息不播放公共提示音，停止后的新普通消息可以播放；
- 启动失败不禁用插件；停止失败保留内存 quarantine 并使 Timer 音频通道 terminal；
- 所有外部 I/O、WebView 和原生音频调用都发生在相关运行时锁外。

### 16.6 资源与人工验收

- 主程序包只包含公共 `message-notification.wav`，不再包含 `attention-alarm.wav`；
- 番茄时钟插件包包含唯一固定路径的合法闹铃；
- Schema 仍无音频路径字段；开发者指南不暗示插件能自定义普通消息提示音；
- 打包产物与开发目录使用同一校验结果。

真实声音、主窗口焦点和 Windows 通知验收需要用户明确允许并手动操作；自动化与 Agent 不控制鼠标或键盘。

## 17. 验收标准

1. 普通消息始终使用主程序公共提示音播放一次，插件不能替换。
2. 每个 `timer.control` 插件必须携带合法的固定路径闹铃，否则安装或更新失败。
3. 闹铃是宿主私有内存资源；Runtime/窗口协议返回 `403`，插件 WebView 永久原生静音。
4. 插件没有任意音频 API；宿主只在有效 Timer 到期后循环播放签票时冻结的闹铃。
5. activationId 防止完全卸载重装后的旧票证、事件或结果操作新插件实例。
6. 第一张有效票证独占当前 alarmEpoch；后续票证不混音、不切换、不候补。
7. 主窗口聚焦、owner 生命周期撤销、Reset、新轮次或退出按完整身份停止闹铃。
8. 闹铃期间普通消息仍完整送达，但其公共提示音被抑制且不补播。
9. 安装期无默认回退；运行期原生音频失败不回滚消息，也不错误停用插件。
10. 更新失败保留完整旧 Bundle；成功更新原子发布新 Bundle 并只让新计时使用新闹铃。
11. 音频解析、Web 隔离、激活身份、内存生命周期、原子提交、alarmEpoch 和票证乱序均有确定性测试。
12. 当前番茄时钟示例重新安装后，关闭插件窗口仍可在到期时播放其包内闹铃，打开主界面即停止。

## 18. 结论

UiPilot 的普通消息提示音属于主程序公共能力；计时闹铃属于声明 `timer.control` 的插件包能力。插件只提供一份
固定、受限、安装期验证的 PCM WAV；该文件进入宿主私有内存资产，不进入 Web 资源协议。不可复用 activationId、
完整 AudioTicket、原子 ActivationBundle 和 alarmEpoch 共同封闭卸载重装、更新、迟到副作用与多 Timer 竞争。
宿主继续拥有计时、消息、焦点、播放和失败关闭，插件 WebView 永久静音，因此不会扩大为任意音频或后台执行接口。
