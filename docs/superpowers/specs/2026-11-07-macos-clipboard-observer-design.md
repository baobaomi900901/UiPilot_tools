# macOS 剪贴板文本观察器

本单元接续[共用基础](2026-11-07-macos-clipboard-foundation-design.md)，实现原计划 Task 10.1 的原生读取和观察生命周期。实现编译进 Mac 库，但生产宿主不创建观察器，插件权限、历史服务、存储、复制和粘贴均不因此开放。Windows 原生实现保持原装配。

## 原生读取与范围

原生对象在专属 worker 内创建、使用并销毁，不将 `NSPasteboard` 或 item 通过不安全 Send 声明跨线程传递。每轮使用 autorelease pool，并重新取得 item，避免沿用另一所有者的数据对象。本单元不调用高版本的剪贴板授权 API，不变更最低系统版本。

版本标识使用完整 `NSInteger` 对应的有符号整数，只作相等比较。零是有效值；不截断为 Windows 的 u32，不推断版本大小顺序。Apple 将该值定义为 pasteboard 所有权变更计数，不能据此保证捕获每一次快速变化：[changeCount](https://developer.apple.com/documentation/appkit/nspasteboard/changecount)。

本次只支持一个 item 且仅声明 `public.utf8-plain-text` 的快照。多 item、文件、图片、富文本、HTML 及混合或未知类型返回 Unsupported，不能取其附带文字当作完整内容。原生接口可以先分配返回对象，以下限制约束后续检查、复制和交付，不宣称限制系统服务本身的分配：

- 最多检查 32 个类型，每个类型标识最多 256 字节；超限返回 TooLarge。
- 文本原始数据最多 1 MiB，先检查长度再复制，严格校验 UTF-8。明确的零长度数据为有效空文本；nil 和解码失败为 Unavailable。
- 读取前后比较同一 pasteboard 的 count；变化时返回 ChangedDuringRead，不交付旧文本。

内部固定结果区分 Text、Unsupported、Unavailable、TooLarge、ChangedDuringRead。不把无法读取解释成用户拒绝授权，也不记录文本、类型列表或原始系统错误。原生读取不会清空、修改或恢复系统剪贴板，不合成按键、不切换焦点。

## 启动与采集

start 创建独立会话和 worker，等待基线 count 建立后才返回可用句柄。初始化失败须清理 worker，不留下活动 lease。基线只读取版本，不读取既有剪贴板内容。worker 每 250ms 检查变化，等待可以被停止请求唤醒。

每个观察到的不同 count 最多读取一次；Unsupported、Unavailable 或 TooLarge 不对同一版本无限重试。正文读取开始前也要取得会话准入；已接纳读取可以完成，但交付回调须再次检查会话。读取期间变化则不交付旧内容，下轮检查最新版本。该策略可能漏掉快速连续变化和暂时不可读的内容，不承诺无损历史。

生产 general pasteboard 的构造函数本次仅定义，不由宿主调用。自动测试默认使用 fake source，禁止默认测试读取 general pasteboard。

## 停止、重启与延迟消费

每次 start 创建永久独立的 session。request_stop 在同一 admission 状态锁内将 session 标为失效并唤醒 worker；它不等待原生读取或回调完成，可在回调内使用。失效会话不能重新启用，重启必须新建 session 并重新取得基线。

捕获事件不直接公开 payload getter；消费者通过 `with_current` 同步闭包取得内容。闭包进入前，在 session 锁内检查 active 并登记在途消费；退出时由 RAII 释放登记，包含 panic 路径。回调本身也属于在途工作。已经排队但尚未接纳的旧事件，在 request_stop 后必须拒绝。后续存储接线必须在该受控闭包内完成需要保护的写入，不能将正文转存到不受会话控制的延迟任务。

stop 先请求失效，再等待 worker 和已接纳的消费者结束；仅全部结束后返回 Stopped。并发 stop 共用同一次 join 和终态；等待时不得持有消费者需要的 session 锁。回调、该会话消费者或 worker 收尾析构中重入 stop 时，只返回 RequestedFromCallback，不自等、不声称已停止；检查 worker 身份不能只依赖回调的作用域。可由外部再次 stop 完成等待。Drop 同样请求停止；重入路径不等待自身。

原生读取可能等待外部内容提供者，Stopped 不承诺固定时限；未来宿主集成不得在 UI 线程或持有宿主业务锁时等待 stop。worker/回调 panic 必须退役 lease、释放在途登记，并由 stop 报告 WorkerFailed。Rust panic 处理不等于 Objective-C 异常捕获；异常、系统授权提示和实机挂起仍需后续原生验收。

## 自动与人工验收

用 fake source 和确定性的屏障分别验证：

1. 启动没有正文读取；零和超过 u32 范围的 count 不被混淆；相同 count 不重复读取。
2. 空文本与 nil 不混淆，中文/emoji/非法 UTF-8/上限边界有实际断言；拒绝多 item、混合格式及超限类型。
3. 读取期间 count 改变不交付旧文本；阻塞读取期间 request_stop 后，读取完成也不能新增交付。
4. 已排队的事件在停止后不能接纳，重启后的新 session 不能让旧事件复活。
5. stop 等待已接纳消费者完成，再报告 Stopped；回调内 request_stop、回调/消费者重入 stop、并发 stop 和 Drop 分别测试。
6. 初始化失败与 callback panic 都关闭会话，不遗留活动 worker 或 admission。

原生测试仅使用系统生成唯一名称的合成 pasteboard，写入固定测试文本，不取得 general pasteboard。该测试默认 ignored，执行前核对代码；结束时只释放测试自身创建的 board。Apple 要求唯一 board 显式释放其服务端资源：[withUniqueName](https://developer.apple.com/documentation/appkit/nspasteboard/withuniquename())。通过该测试不代表已验证真实通用剪贴板、用户授权、跨应用行为或历史存储。

真实 general pasteboard 的采集、授权/拒绝、退出与重启验收延后。完整 Task 10.1 仍需私有持久化、插件授权生命周期、复制与外部写入保护；Task 10.2 的选区与粘贴另行实现。
