# macOS 主启动器货币换算

将 Windows 已有的宿主汇率服务接入 Mac 主启动器，不依赖 Apple Developer 证书。沿用 [共享快速计算设计](2026-09-03-launcher-quick-evaluation-design.md) 的 Currency Conversion、Launcher Result And Wire Contracts、Data Flow 和 Failure And Safety Rules。此文替代 [早期 Mac 快速计算设计](2026-11-07-macos-quick-evaluation-design.md) 中「汇率未启用」的临时限制，其他计算、复制和所有权合同保持不变。

## 服务与持久化

共用同一个解析器、decimal 计算、固定 Frankfurter HTTPS transport、provider、缓存和 coalescing service；不复制平台算法。固定 GET /v2/rate/{BASE}/{QUOTE}，只发送币种，不发送金额或原始查询；TLS 校验、无重定向、2 秒总网络期限和 64 KiB 响应上限不变。同币种本地计算；一小时新鲜缓存、七天故障回退及时间标签沿用现有合同。官方文档 https://frankfurter.dev/ 的单币种对端点已核对，属于参考汇率而非实时报价。

Mac 在首次识别完整货币查询时于 worker 初始化共享 service，缓存异常不阻止本地计算或启动。缓存只保留币种对/汇率/日期/抓取时间。Mac 使用目录描述符与 no-follow 读写、读取上限、原子 current/backup 更新；待写入事务持有原目录，路径替换不能转向另一目录。缓存写入失败不把有效换算变为失败。不访问用户文稿或剪贴板来推断查询。

## 查询、界面与执行

主窗口 caller/source 校验先于状态访问。begin_query 在服务初始化、网络等待前取得票据；只通过 publish_if_latest 发布。同窗新查询、隐藏或新 invocation 先发生时，迟到成功/失败均不能发布，成功仍可更新纯汇率缓存。识别后的汇率查询不降级为应用/网页搜索。网络期间使用现有 pending 状态。

Mac 前端仅增加 currency 的无收藏 ExecuteResult 准入；成功与缓存结果采用现有图标、结果行、来源及日期副标题，失败行不可执行。回车仍使用私有 CopyBuiltInText 和原有一次性准入及完成归属。不新增可收藏命令，不开放 builtinFeatures 总开关或插件 Runtime。

## 验证边界

共享 provider、TLS 本地 fixture、缓存、逆向并发合并与故障测试在 Mac 编译运行；新增 Mac 私有动作、失败不可执行、延迟响应的三种所有权转换、有界读及目录替换断言。前端真实 Mac 能力快照允许 currency 复制但仍屏蔽无关内置/插件项。自动测试不访问线上汇率、不操作真实剪贴板或 GUI。实际联网显示、复制和快速切换查询由用户在内部包中验收。
