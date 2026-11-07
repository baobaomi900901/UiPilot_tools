# macOS 文本采集与授权历史接线

本单元接续[观察器](2026-11-07-macos-clipboard-observer-design.md)与[私有历史](2026-11-07-macos-clipboard-storage-design.md)，将读取前授权、观测时间和历史写入连接为内部 collector。取代上一单元尚待接线的 `bind_capture` / 任意事件与批次配对接口；仍不在生产宿主启动服务，不开放 SDK、插件或用户界面。只支持原观察器规定的单项纯文本。

## 授权快照与变化基线

协调器持有不可伪造的授权集合 revision，以独立 Arc 身份比较，不使用会溢出或复用的整数。新增授权、有效撤回及 shutdown 更新 revision；对当前有效授权的重复 authorize 保持幂等。

observer 每轮在读取正文之前从协调器取得 revision 和可选的单次读取上下文。上下文包括读取前已有的授权 generation 快照和拥有所有权的原生读取许可。没有授权时不取得正文读取上下文，允许检查 count，但正文读取次数必须为零。

启动以及观察到 revision 改变的轮次，只读取当前 count 建立基线，不读取正文。初始 revision 快照必须先于对应 count 基线，或首轮强制重建 revision 基线；不能把早先读取的 count 与后来新增授权的 revision 配对。必须覆盖「无授权期间剪贴板已变化，首次授权发生在下一次 poll 之前」的顺序；不能把该旧内容当作新授权后的复制。授权变化附近的新内容可能被保守丢弃，这比补采授权前内容更符合合同。新授权不能加入已开始读取的批次；撤回再授权得到独立 generation，旧批次不能复活。

## 原生读取准入与撤回

每个授权 generation 有独立 read gate，包含 active、在途计数与排空通知。协调器短暂持 state 锁，快照授权集合并登记各 read gate，随即释放 state。原生读取许可只持 gate，不持 store 或目录 FD；正文读取期间不持协调器 state、store mutex 或写入 gate。

revoke 在 state 锁内关闭该 generation 的新读写准入、更新 revision 并移除授权，在锁外等待已准入原生读取及存储操作结束，然后释放 store。shutdown 对所有 generation 执行相同关闭和排空顺序。相同 ID 的 revoke/regrant 通过 lifecycle 串行。已接纳原生读取可以完成，但撤回返回后不能再有该 generation 的读取或写入；其他插件继续按自己的有效授权工作。

read permit 必须在调用消费回调或 ingest 之前释放。所有基线、无变化、错误、停止和 panic 路径均由 RAII 释放，不允许回调撤权等待自己持有的许可。等待排空不持 state 锁，permit 的 Drop 也不反向取得 state。原生 provider 可能阻塞，revoke/shutdown 与 observer stop 均不承诺固定时限；未来宿主须在 UI 线程及业务锁之外等待。

## 密封事件与时间

仅 observer 能在采集路径构造生产事件。事件不可 Clone，绑定授权上下文、observer session、读取结果和观测时间，通过消费所有权的受控闭包单次读取。禁止生产接口重新组合任意旧 event 与新授权批次；合成构造器只存在于 cfg(test)。history 仍检查批次所属协调器实例和每个原有 generation，写入完整地位于 observer 会话准入内。停止后尚未准入的旧事件拒绝；新 observer 或新协调器不能复活旧票据。

`captured_at` 是正文读取准入后、实际正文读取之前固定的 UTC 观测时间，不宣称是操作系统记录的复制时刻。时钟格式化失败不读取正文，返回固定内部失败。事件排队、延迟消费与存储重试不能改写此时间；已有重复记录继续遵循保留首次 capturedAt 的共享去重规则。

collector 将自身 observer 的回调直接连接到对应 history，不提供可供外部互换事件的 ingest 入口。collector stop 负责自身 observer 的失效与排空；history 的总 shutdown 由整体生命周期显式协调，避免一个 collector 的 Drop 意外关闭共享协调器。生产宿主尚未装配任何 collector。

## 验收

默认只用 fake source、合成文本和本轮新建的临时目录，不读取通用剪贴板或个人历史。

- 无授权多轮变化：正文读取为零；在两次 poll 之间新增授权，只更新基线，下一次变化才能入库。
- 已授权 A 的读取阻塞期间授予 B：该事件只属于 A 的原快照；下轮 revision 变化重建基线，B 不接收旧内容。
- 读取阻塞期间撤回 A：撤回等待原生读取许可释放；完成后旧事件不能写入，重新授权 A 也不能复活它。其他仍有效授权按自身快照处理。
- 回调内撤回、clock 失败、stop、读取或回调 panic：无许可泄漏、无自等；停止/重开后的旧事件拒绝。
- 延迟消费使用采集路径记录的固定时间；跨协调器事件拒绝；正文、批次与时间不能通过生产接口重新配对或重复消费。
- fake observer → collector → 私有 index → snapshot → 关闭重开全链路验证；损坏介质与提交失败仍遵循存储合同，不输出正文日志。

原生唯一命名 pasteboard 用例保持 opt-in；是否本轮重跑单独记录。Mac 普通库、Find 预览与全仓自动验证通过后合入开发分支。此单元不等于 Task 10.1 完成，复制/外部写入保护、图片/文件、真实通用剪贴板授权和插件双端验收另列。
