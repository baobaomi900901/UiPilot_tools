# macOS 主启动器快速计算

在现有 Mac 主启动器接入与 Windows 相同的算术、单位、日期和时区计算，并支持用户明确执行结果时复制。复用 `quick_evaluation` 的解析/计算与测试，不另写一套 Mac 算法。此为原移植计划的本地内置工具部分，不打开公开插件或整个 builtin 能力集合。

## 查询与呈现

调用者先通过主窗口与来源检查，再取得当前 invocation/query ticket。匹配快速计算时，在等待应用目录扫描前计算并发布；NoMatch 保持应用与 Find 原路径。复用已有 ResultItem、iconKind、ExecuteResult/Informational 和 refreshAtUtc。成功行隐藏完整复制正文，正文只存服务端 result registry；失败行不可执行。replaceLocalResults 为 true，时区刷新仍只属于原查询。日期和时钟按 Mac 系统取得，测试注入时钟，不启真实 GUI。

前端在真实 Mac 能力快照下仅放行 calculator/unit/date/timeZone 的无收藏 ExecuteResult 或 Informational，以及 currency 的无收藏 Informational；插件标记先过滤，builtinFeatures 总开关保持关闭。测试必须通过真实能力过滤后的 core 验证显示、Enter 复制和不可执行提示，不能只依赖默认 Windows 预览。

计算值属于一次查询结果，不是可复用的启动器命令，因此不产生收藏入口。布局、主题、键盘和状态沿用既有启动器规范。网络汇率服务本轮不启用；Mac 识别汇率查询后明确显示未启用，不伪造数据、不联网。Windows 的既有汇率实现、缓存与网络测试保持原路径。

## 执行与原生复制

前端只提交 request/result ID，不接受 JS 提供的复制正文。执行入口再次检查调用者，服务端解析 CopyBuiltInText；使用 worker 预备同一 NSPasteboard 实例和原生数据，最终准入原子消费当前主窗口/Application domain 的执行票据。复用[显式复制规格](2026-11-07-macos-clipboard-copy-design.md)的一次声明/一次 setData，不备份、恢复或重试；这是用户主动执行宿主结果，不借用历史读取授权。

消费票据同步递增对应 domain epoch，清理旧结果并让旧 publish token 失效，防止迟到查询重新发布同一操作。完成 owner 绑定 registry、scope generation、invocation、query sequence、domain epoch 与 result-set generation；不可只记录 invocation。原生调用不持 registry 锁，已准入动作可以完成；如果随后已有新查询或新窗口，它只能返回结果，不能隐藏新状态。

成功后将完成 owner 交到主线程，在同一 registry 操作中确认当前 owner 并退役旧 invocation，之后才调用对应 hide。旧 owner、同窗新查询、重新发布和新 invocation 均不隐藏。普通应用打开 DTO、计数和焦点路径不变。

## 失败与验收

准备、版本检查或最终准入失败不 claim；返回既有复制失败/过期错误，保留符合当前消费策略的结果。claim 后失败使用固定 `clipboardWriteUnconfirmed`，告诉用户先检查剪贴板并重新搜索；前端移除该 request 的旧结果，不隐藏窗口、不自动重试、不展示系统错误正文。晚到错误不得清除新查询。

验收覆盖共享算术/单位/日期/时区固定向量，Mac 结果 DTO 与无网络汇率提示，扫描未完成时快速计算可用，NoMatch 保留应用搜索；旧查询/旧 invocation/另一 registry 的票据零复制；票据消费后旧 publish 被拒绝、重复执行拒绝；成功完成时同窗新查询或新窗口不隐藏；claim 后不确定错误移除旧动作且不重试。默认 native driver 用 fake，不访问真实通用剪贴板。原生复制与 M1 主界面实际操作在约 70% 候选包中统一验收。
