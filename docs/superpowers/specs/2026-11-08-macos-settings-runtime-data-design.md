# macOS 插件设置持久化与 Runtime 数据调用

承接私有数据基础及请求通道生命周期规格。完成独立的普通设置文档、宿主管理事务和真实 QuickJS 数据调用；不将 Windows 的整份安装/权限 state.json 误称为已移植。生产插件继续关闭，XPC 身份及沙箱验收仍等待开发签名。仅操作临时目录、固定脚本及仓库跟踪的示例源码，无 GUI、个人文件或系统权限变化。

## 持久设置

使用私有根目录与固定 Settings 文件布局，复用 FD 身份、排他锁、有界读取、原子 rename 和提交不确定状态。设置与 storage 使用不同根目录；文件名由宿主枚举选择，不接受运行时路径。schema、owner、revision 和普通值独立保存，原始 JSON 拒绝重复键与损坏；遵守真实 setting key/definition 规则，不能用 storage key 规则替代。

宿主 configure 捕获完整验证的不可变定义，按共享规则保留兼容值、补默认值、删除废弃项和 secret。configure 与保存持有真实 data gate lease，成功持久化后才发布新内存状态及不可序列化句柄。句柄绑定 store、owner、identity 和配置代次，跨 store、旧代次和错误 identity 在 owner IO 前拒绝。批量保存先全量校验；同值保存保持句柄，实际更新成功后替换句柄。rename 前失败保旧，rename 后不确定时该 owner 拒绝继续使用，必须关闭重开检查；不自动重试。

## 宿主管理与请求切换

普通配置保存只有可信宿主入口，没有 Runtime settings setter。内部控制器按单个活动请求串行切换：关闭旧 gate 和 session，调用传输取消，再在锁外排空旧调用/回复，随后分配新 activation/admission、一次原子 reconcile+updates 并创建新请求。旧 epoch 从不复活。取消失败使控制器不可继续开放；保存失败也不发布新 endpoint。已准入写可以完成，撤销不是回滚。显式开启新请求总会换请求身份，区别于未来设置 UI 的同值保存 no-op。排空后再次检查 deadline，已经过期时不开始配置事务；持久化成功后仍可能过期而无法发布请求，此时已保存配置不回滚，不能把无 endpoint 当作未保存。

Runtime binding 由宿主捕获，wire 不能选择目录、owner、定义或 activation。每个调用由真实 CallSession 验证完整上下文、顺序、deadline 和对象身份，CallPermit 中的真实 identity 传入数据服务。提供 storage get/set/remove 和已声明普通 setting get；缺失值返回 null，secret/未知键拒绝。后端完成后再取得 ReplyPermit，贯穿实际传输写入；撤权之后不能用旧准入发送。公开 SDK、Windows 运行时逻辑和生产启动装配不变。

## 验证

持久化覆盖重开、默认值、升级、false/0/空串、非法批量更新、句柄替代、跨 owner、损坏、配额和提交故障。请求切换分别验证旧回复未释放时必须等待、取消先于排空、失败不开放、旧 context/重复 call/未知操作拒绝。真实 QuickJS 子进程读取实际落盘设置，执行原样 demo-settings，并往返真实私有 storage；重建宿主后读取同一临时文档。原 bridge 的帧大小、累计输出、deadline、双流结束及 kill/wait 验证保留。内部探针的数据帧预算不代表完整公开 SDK 的最大传输容量。

只计内部数据服务与真实引擎接线，不计完整安装、权限生命周期、UI、秘密服务或同包双端生产验收。验收日志需列明实际进程与源码/产物哈希。
