# UiPilot 公开插件计时闹铃资源设计

## 1. 文档信息

- 日期：2026-08-22
- 状态：Draft，已完成分节确认，等待用户复核书面规格
- 范围：公开插件计时闹铃的包内资源、安装校验、运行时解析与原生播放
- 公开 JavaScript API：不变
- Manifest 字段：不变
- 新权限：无

本设计是以下已批准规格的增量覆盖：

- [公开插件窗口计时 API 设计](./2026-08-20-public-plugin-window-timer-api-design.md)
- [UiPilot Windows 原生提醒协调器设计](./2026-08-21-windows-native-attention-design.md)

除本文明确覆盖的音频资源归属、播放源和多 Timer 声音仲裁外，原规格中的消息持久化、未读、Toast、托盘、
Timer 状态机、ClaimTicket、AudioTicket、窗口会话、焦点确认、锁顺序和失败关闭合同继续有效。

本文覆盖以下旧规则：

| 旧规格章节 | 旧规则 | 当前唯一规则 |
|---|---|---|
| Timer 3.1、3.2、4、6.1、12.3、18.3、19、21、23 | Timer 使用宿主固定闹铃，插件不能携带自定义音频 | Timer 使用所属插件包内固定路径的闹铃；插件仍不能通过 API 控制播放 |
| 原生提醒 2.1、2.2、3.1、3.2、7.1 | 普通消息与 Timer 可以共用同一宿主 WAV | 普通消息始终使用宿主公共提示音；Timer 只能使用插件闹铃 |
| 原生提醒 3.3、4.3、5.3、7.2、8、10、11 | 多张 Timer 票证共享同一声音，最后一票撤销才停止 | 第一张取得播放权的票证独占本轮循环音频；后续票证不叠加、不切换、不候补 |

## 2. 目标与非目标

### 2.1 目标

1. 保持普通消息提示音为主程序公共能力。
2. 让每个 `timer.control` 插件携带自己的计时到期闹铃。
3. 由宿主验证、持有并播放插件闹铃，插件代码不能直接控制音频设备。
4. 保持消息提交与音频副作用隔离；闹铃失败不回滚消息、Toast、托盘或徽标。
5. 固定资源路径、格式、大小、时长和生命周期，避免任意文件与通用音频 API。

### 2.2 非目标

- 不新增 `audio.play`、暂停、停止、音量、声道或文件选择 API。
- 不允许 Runtime 或插件窗口传递路径、URL、字节、MIME 或循环参数。
- 不允许插件替换普通消息提示音。
- 不支持 MP3、AAC、OGG、WebAudio、远程音频或系统外部路径。
- 不增加用户级或单插件音量设置、静音时段和声音选择器。
- 不保留当前预发布版本的旧插件兼容逻辑。
- 不修改 `notifications.publish()`、`notifications.schedule()` 或消息 DTO。

## 3. 用户合同

### 3.1 普通消息提示音

主程序继续内置：

```text
resources/sounds/message-notification.wav
```

普通消息成功持久化且主窗口未聚焦时，宿主按既有原生提醒合同播放该声音一次。所有插件共用此提示音，
插件不能声明、覆盖或选择普通消息提示音。

普通消息提示音缺失或播放失败只使本次宿主音频副作用降级，不改变插件状态，不回滚消息，也不尝试插件闹铃。

### 3.2 插件计时闹铃

任何声明 `timer.control` 的插件包都必须包含精确路径：

```text
assets/sounds/timer-alarm.wav
```

路径和文件名区分大小写并必须完全匹配。Manifest 不增加音频路径字段；固定路径是 `timer.control` 包合同的一部分。

Timer 到期消息成功持久化、ClaimTicket 成功提交 `fired` 且 AudioTicket 通过原生播放 admission 后，宿主循环
播放该插件 generation 对应的 `timer-alarm.wav`。插件不能决定是否循环、循环次数或停止条件。

用户让主窗口产生原生 `Focused(true)` 后，宿主立即停止当前循环闹铃。注意确认不标记消息已读，消息记录和
未读徽标继续保留，直到用户进入消息页并完成既有已读流程。

### 3.3 声音优先级

Timer 循环闹铃具有最高声音优先级：

- 循环闹铃正在播放时，普通消息仍保存、显示 Toast、更新徽标并触发托盘，但不播放
  `message-notification.wav`；
- 闹铃停止后到达的新普通消息可以恢复播放公共提示音；
- 已被抑制的普通提示音不补播。

## 4. Manifest、权限与包结构

### 4.1 权限

不新增音频权限。现有 `timer.control` 继续要求现有完整权限组合：

```json
"permissions": ["ui.window", "notifications.publish", "timer.control"]
```

`timer.control` 同时授权窗口内计时控制和宿主在到期时播放该插件的受限闹铃。它不授予任意播放权限。

安装或更新仍要求用户完整同意 Manifest 声明的权限；运行时权限检查继续作为纵深防御。

### 4.2 固定资源

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
- 闹铃进入包摘要和只读安装快照，不能引用包外文件。

## 5. WAV 合同

安装器必须解析文件内容，不能按扩展名、调用方 MIME 或文件头片段直接放行。

合法闹铃必须同时满足：

| 项目 | 限制 |
|---|---|
| 容器 | RIFF/WAVE |
| 编码 | little-endian integer PCM，format tag `1` |
| 声道 | 1 或 2 |
| 采样率 | 44,100 Hz 或 48,000 Hz |
| 位深 | 16-bit 或 24-bit |
| 单文件大小 | `1..=2 MiB` |
| 有效音频时长 | `> 0` 且 `<= 15 秒` |

解析器必须验证 RIFF 长度、chunk 边界、`fmt`/`data` 一致性、block alignment、byte rate、sample frame 整除和
整数运算溢出。畸形、截断、重复冲突、伪装格式或超限输入统一返回现有安装失败结果，不向插件暴露宿主磁盘
路径或底层解码器错误。

## 6. 安装、更新与 generation 所有权

### 6.1 Staging 校验

目录安装和 `.uipilot-plugin` 归档安装使用同一条 staging 流程：

1. 将来源复制或解压为受控 staging 快照；
2. 扫描资源并解析 Manifest；
3. 根据 `timer.control` 验证固定闹铃存在性和唯一性；
4. 解析并完整验证 WAV；
5. 将闹铃纳入资源表、包摘要和只读快照；
6. 再次扫描并确认前后摘要一致；
7. Runtime ready 后才原子提交新 generation。

任何一步失败都删除 staging 事务，不写入插件列表，不替换当前已安装版本。更新失败时旧 generation、旧资源和
旧运行状态继续有效。

### 6.2 运行时身份

宿主为每个成功激活的 `timer.control` generation 建立内部只读 `AlarmAssetIdentity`。它至少绑定：

```text
pluginId + pluginGeneration + packageDigest + fixed relative path
```

插件看不到该身份和实际安装路径。Timer 新轮次冻结所属 `pluginId + generation`；AudioTicket 继续绑定既有
`pluginId + generation + roundId + firedRevision`，原生协调器只能解析同一 generation 的闹铃资产。

禁用、故障停用、卸载或成功更新会取消旧 generation 的 Timer、ClaimTicket、AudioTicket 和音频所有权。
失败更新不影响旧 generation。更新后的闹铃只对新 generation 启动的新计时生效。

## 7. 原生播放与多 Timer 仲裁

### 7.1 单一播放所有者

原生提醒协调器维护至多一个 `timerAudioOwner`：

```text
none | owner(AudioTicket, AlarmAssetIdentity)
```

处理顺序固定为：

1. Timer 到期消息先按既有合同原子持久化；
2. 有效 ClaimTicket 提交 `fired` 并签发 AudioTicket；
3. 原生协调器在 Timer 锁外执行票证 admission；
4. 当前没有 owner 时，第一张成功 admission 的票证成为 owner；
5. 宿主解析同 generation 的只读闹铃资产并启动循环播放；
6. 当前已有 owner 时，后续 AudioTicket 不叠加、不切换、不重启当前声音，并提交为不再迟到播放的终态；
7. 主窗口 `Focused(true)`、owner 生命周期撤销、owner Reset、owner 新轮次或进程退出会停止循环并清除 owner。

如果 owner 被取消，即使此前还有其他 Timer 完成，也不把播放权转交给旧的后续票证。后续票证已经终结，不能
在 owner 消失后迟到补响。owner 清除后，新到期的 Timer 可以取得下一次播放权。

### 7.2 锁与 I/O

Timer 状态锁、插件 mutation guard、原生提醒邮箱锁和包资源注册表锁均不得跨越以下操作：

- 文件打开、读取或 WAV 解析；
- `PlaySoundW` 或其他原生音频调用；
- 消息文件 I/O；
- Toast、托盘、窗口或前端 emit/evaluate。

安装阶段已经完成内容验证；运行时只允许通过 `AlarmAssetIdentity` 解析只读安装快照。运行时打开文件和启动声音
发生在所有权及票证内存转换之后、相关锁释放之后。迟到成功必须再次匹配当前 owner，不能复活已撤销票证。

## 8. 失败行为

### 8.1 安装期失败

以下情况拒绝安装或更新：

- `timer.control` 插件缺少固定闹铃；
- 非计时插件携带 WAV；
- 路径大小写错误、路径穿越、符号链接、硬链接、重解析点或大小写折叠冲突；
- WAV 内容、大小、时长或 PCM 参数不符合第 5 节；
- staging 前后摘要不一致。

安装期失败不存在“已安装但使用默认闹铃”的状态。

### 8.2 运行期失败

若已安装闹铃被外部删除、替换、损坏、无法读取或无法播放：

1. 已保存的完成消息、`fired`、Toast、托盘和徽标保持不变；
2. 不回退到 `message-notification.wav`，也不存在宿主默认闹铃；
3. 本次 owner 票证进入不可重试的音频终态，不得迟到补响；
4. 宿主记录不包含敏感路径的稳定诊断；
5. 对应插件 generation 进入现有运行故障状态，并取消其 Timer、会话和音频权威；
6. 用户重新安装或成功更新插件后才恢复。

普通消息公共提示音失败不会把消息来源插件标记为故障，因为该资源属于主程序。

## 9. SDK 与开发者体验

公开 JavaScript API、Timer DTO 和 Manifest Schema 不增加字段。需要同步更新：

- 第三方插件开发指南：说明固定路径、格式限制、普通消息音与插件闹铃的边界；
- 番茄时钟示例：在包内提供 `assets/sounds/timer-alarm.wav`；
- 安装权限说明：`timer.control` 到期后会播放插件随包提供的闹铃；
- 打包和发布检查清单：验证闹铃存在且最终归档包含正确路径。

插件开发者不需要、也不能从 Runtime 或窗口 JavaScript 获取闹铃路径。

## 10. 迁移

当前尚未发布，不提供旧格式兼容：

1. 主程序保留 `resources/sounds/message-notification.wav`；
2. 主程序删除宿主 `resources/sounds/attention-alarm.wav` 及对应打包声明；
3. 将现有闹铃复制到
   `examples/public-plugins/com.uipilot.pomodoro/package/assets/sounds/timer-alarm.wav`；
4. 番茄时钟示例提高版本号；
5. 已安装的旧番茄时钟插件需要完全卸载后重新安装新包；
6. 启动时发现旧计时插件缺少闹铃时，不自动补文件或使用宿主资源，按现有包重验证/故障路径处理。

## 11. 测试合同

### 11.1 包校验

- 合法的目录包和归档包均能安装；
- `timer.control` 缺少闹铃、错误大小写、额外 WAV、非计时插件携带 WAV 均被拒绝；
- 表驱动覆盖伪造 RIFF、截断 chunk、错误编码/声道/采样率/位深、错误 block alignment、零时长、超时长和超大小；
- 路径穿越、链接、重解析点、大小写折叠冲突和 staging 竞态被拒绝；
- 更新失败保留旧 generation 和旧闹铃。

### 11.2 Timer 与原生协调器

- 消息保存失败时不签发 AudioTicket、不播放；
- 第一张有效票证取得 owner 并循环，后续票证不调用 start、不切换声音；
- owner 取消立即停止，旧后续票证不能补响；
- owner 清除后新到期票证可以取得新 owner；
- 主窗口焦点停止循环但不清除未读；
- 闹铃期间普通消息不播放公共提示音，停止后的新普通消息可以播放；
- 禁用、故障停用、卸载、更新、Reset、新轮次和 shutdown 都按 generation/round/ticket 精确撤销；
- 文件运行时丢失、摘要不匹配和原生播放失败保留消息并使插件进入运行故障，不使用回退声音；
- 所有外部 I/O 和原生调用都发生在锁外。

### 11.3 资源与文档

- 主程序包只包含公共 `message-notification.wav`，不再包含 `attention-alarm.wav`；
- 番茄时钟插件包包含唯一固定路径的合法闹铃；
- Schema 仍无音频路径字段；开发者指南不暗示插件能自定义普通消息提示音；
- 打包产物与开发目录使用同一校验结果。

真实声音、主窗口焦点和 Windows 通知验收需要用户明确允许并手动操作；自动化与 Agent 不控制鼠标或键盘。

## 12. 验收标准

1. 普通消息始终使用主程序公共提示音播放一次，插件不能替换。
2. 每个 `timer.control` 插件必须携带合法的固定路径闹铃，否则安装或更新失败。
3. 插件没有任意音频 API；宿主只在有效 Timer 到期后循环播放对应 generation 的闹铃。
4. 第一张到期票证独占当前循环；后续票证不混音、不切换、不迟到补响。
5. 主窗口聚焦、owner 生命周期撤销、Reset、新轮次或退出按合同停止闹铃。
6. 闹铃期间普通消息仍完整送达，但其公共提示音被抑制且不补播。
7. 安装期无默认回退；运行期故障不回滚消息，并使对应插件 generation 进入运行故障。
8. 更新失败保留旧版本，成功更新取消旧 generation 并只让新计时使用新闹铃。
9. 音频解析、包路径、资源摘要、锁顺序和票证乱序均有确定性自动化测试。
10. 当前番茄时钟示例重新安装后，关闭插件窗口仍可在到期时播放其包内闹铃，打开主界面即停止。

## 13. 结论

UiPilot 的普通消息提示音属于主程序公共能力；计时闹铃属于声明 `timer.control` 的插件包能力。插件只提供一份
固定、受限、安装期验证的 PCM WAV，不能直接控制播放。宿主继续拥有计时、消息、焦点、音频仲裁和失败关闭，
从而在允许插件具有自身闹铃音色的同时，不扩大为通用音频或后台执行接口。
