# macOS 历史文本的显式复制

接续[内部采集](2026-11-07-macos-clipboard-collection-design.md)，实现 Task 10.1 的单次显式文本复制。生产宿主与 SDK 继续关闭。本单元不合成输入、不改变焦点、不临时备份或恢复剪贴板；Task 10.2 的选区与粘贴另列。

## 可保证的原生边界

Windows 的条件恢复在 OpenClipboard 独占期间复验版本并写入。macOS 公开 NSPasteboard 接口没有同等的跨进程比较并交换；先检查 changeCount 再 declareTypes 不是原子操作，不能承诺该间隙的外部内容绝不被替换。

本单元只实现用户主动发起、原本就要替换剪贴板的 ExplicitCopy。临时恢复返回 Unsupported，不构造备份、不取得通用剪贴板正文、不用重试或回滚掩盖平台差异。先准备有界原生数据，再检查已观察到的变化，变化则在声明所有权前放弃。一次 declareTypes 声明纯 UTF-8 类型并取得所有权，随后仅一次 setData；没有第二次 clear、补写或恢复。

Apple 的 [declareTypes](https://developer.apple.com/documentation/appkit/nspasteboard/declaretypes(_:owner:)) 会清空旧内容并声明类型；[setData](https://developer.apple.com/documentation/appkit/nspasteboard/setdata(_:fortype:)) 在所有权已改变时返回 false。这提供声明后的所有权检查，不提供声明前的原子条件写。Accepted 只表示该次 setData 被接受，不保证返回时内容仍未被其他应用替换。外部应用在本次写入以后写入 B，旧完成回调不能再写任何东西，B 不会被恢复事务覆盖。

## 权限与单次请求

读取授权不自动授予复制。宿主内部显式批准独立 copy generation，绑定同一 history 实例及原读取 generation；撤回复制不撤回读取，也不改变采集 revision。读授权撤回、shutdown 同时使所属复制授权失效。再次授权得到新身份，旧请求不能借此复活。

CopyRequest 是不可 Clone 的内部对象，绑定原复制 token、记录 ID 与用途，不持 store/FD，不给调用方公开正文。只允许 ExplicitCopy，TemporaryRestore 在访问记录和原生接口前拒绝。准备及原生提交前均验证原授权和记录；记录已删除、授权过期、跨实例请求均拒绝。未来宿主接入还必须在最终执行点校验 UI 会话、用户动作和 IPC 调用者，不能把内部 token 当作完整的 SDK 权限实现。

执行消费请求所有权。原生对象及 payload 在同一 worker 创建、使用和销毁，不使用不安全 Send 跨线程搬运 NSPasteboard。读取记录并准备 NSData/类型后，在同一读/复制权限准入中复验记录 ID 和内容，再做 count 检查及唯一一次原生写入。store mutex 在调用系统 API 前释放；只保留独立授权准入，撤权需等待该次已接纳提交结束。锁序为读取 generation → 复制 generation → 短 state/store；关闭和排空不得反向持 state 锁等待。

复制成功不主动修改历史 recency；由 collector 的正常观察和文本去重处理，不采用 skip-next 吞掉随后其他应用写入的内容。

## 失败与验收

准备/只读检查失败为固定 Unavailable；观察到版本已变为 ClipboardChanged，零所有权声明。进入 declare 前标记开始修改；之后 Objective-C 异常报告 WriteUnconfirmed，setData 返回 false 报 OwnershipLost。捕获异常对象直接丢弃，不输出其正文、name/reason。失败后不回滚、不重试、不报告原剪贴板必然未变。已进入原生修改后撤权可能发生，撤权等待已接纳操作结束，再保证旧请求不能再次执行。

默认使用 fake driver 与专用临时历史，验证：无复制权限或恢复用途零原生调用；准备后删除记录/撤权/重授权/跨实例拒绝；正文、空文本与 Unicode 边界；观察到外部变化时零 claim；claim 后被抢占只尝试一次 setData；修改后异常不重试或恢复；写入后外部 B 不被完成动作覆盖；撤权/关闭排空正在写入的动作。fake 不能虚构原生 CAS，因此明确保留 check→claim 竞态限制。

原生唯一命名 pasteboard 测试默认 ignored，留作合成内容验收，不触 general。本轮不改 Windows 复制行为，不将临时 restore 标为已支持，也不把内部复制实现等同于双端插件实机通过。
