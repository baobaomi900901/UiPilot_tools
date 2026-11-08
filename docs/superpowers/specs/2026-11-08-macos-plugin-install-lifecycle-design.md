# macOS 插件安装快照、升级与权限生命周期

承接普通设置、私有存储、无窗口 Runtime 与主结果验证。此变更只装配可在临时目录测试的内部 Host 控制器；不连接生产安装 UI、启动扫描或真实 XPC 服务。保持生产公共插件关闭。开发签名、服务对端验证、沙箱、Panel/Window、动作执行与 Keychain 不在本轮通过范围。

## 已验证包

目录与 `.uipilot-plugin` ZIP 都经过真实 manifest/platform 校验、同源资源路径与 CSS/icon 规则，捕获不可变的完整资源字节。拒绝符号链接、特殊文件、越界、大小/数量/深度超限、路径碰撞与读取中的替换。运行时不重新读取来源目录。私有 VerifiedPackage 不可从反序列化直接生成；重启恢复必须重验全部资源、manifest 和带域分隔的 SHA-256 digest。

内部输出只支持 mainResult，拒绝 resultActions 与 Panel/Window。当前平台 manifest 仍是权限边界，生命周期另只接无权限与 clipboard.write 的普通授权状态。clipboard.write 仅用于内部准入、撤权与恢复验证，不实现剪贴板操作；copyText 结果继续拒绝。网络域名、凭据及其他服务权限未开放，因此没有以枚举授权替代范围授权。

保留每包 256 文件、64 目录、8 层、单文件 2 MiB、解压总量 16 MiB 的既有上限；内部序列化快照额外限 5 MiB。ZIP 本轮仅接受标准单盘、非 Zip64、无自解压前缀的有界包；原始中央目录先拒绝重复条目，再交给 zip 库，避免库按名称去重掩盖输入。Windows 原 ZIP IO 保持不变。

## 单份原子安装清单

独立私有根目录与固定 Host owner `com.uipilot.inventory/plugins.json` 保存所有已装快照、普通设置、enabled、grants、generation 和全局 revision。整库上限 5 MiB，可能早于单包上限触发；这是内部容量限制。通过既有 FD/no-follow/owner/mode/ACL/本地卷约束和原子 rename+fsync 提交。不写多个互相依赖的权威文件；普通设置运行时直接读此安装快照，不读写派生 settings.json。

PreparedInstall 是不可反序列化、单次消费的 Host 对象，绑定当前控制器实例、清单 revision、已验证包和五分钟期限。初装需准确批准全部声明；升级只批准新增声明。旧已撤权项保持关闭，已删除声明从 grants 删除，disabled 状态保留。普通设置按既有规则保留兼容值，删除过时项，不兼容值采用新默认值。先完整验证权限、冲突、设置、总容量和计数器溢出，再终止旧运行时。未改变内容的安装/设置/开关操作零写入、零取消。

generation 与 revision 使用 checked_add；一次有效管理变更分配新 generation。重启严格重验 owner、版本、包摘要、权限子集、设置规范化和计数器，不自动执行插件。损坏或外部变更不能降级为空库或恢复旧备份授权。当前只检查已装插件之间的命令名冲突；生产 launcher 的保留命令与命令覆盖配置装配另行完成。

## 运行时与权限所有权

open_request 只接受已装插件 ID、实际 command ID、合法 request ID 与截止时间。代码、manifest、权限与 generation 均由 Host 从清单选择，不能由子进程 JSON 指定。InstalledRequest 私有捕获不可变代码/设置/权限与真实 session endpoint。普通设置与 storage 调用沿用真实 CallPermit、gate lease 和 ReplyPermit。

每次 request 的可信 attach 回调创建传输与结果 owner，并返回单次 Retirement，生命周期控制器负责持有。attach 返回错误时，构造器须以 RAII 清理尚未移交的资源。成功移交后若已过期，立即关闭并调用 Retirement，不返回可用请求。

升级、停用、撤权、设置改变或显式关闭遵循：关闭 gate/session → 调用捕获的结果 retirement 与 transport cancel/reap → 等待在途 permits → 原子提交候选 → 才允许新请求。真实结果注册表仍在最终发布时检查 query owner，旧清理不能误伤更新查询。正常失活不重复调用清理。检测清单异常也使用捕获清理；Drop 关闭准入并触发清理，但不声称已完成 permit drain，正式退出应显式 close。

权限准入必须先取得真实 session CallPermit，再检查捕获的 grants，执行可信 Host effect，最后取得 fresh ReplyPermit。撤权前已经准入的 effect 可以结束，撤权后不能提交旧 reply；管理操作会排空它。新授权不会复活旧 endpoint。测试 effect 仅操作计数/通道，不接触用户剪贴板或系统权限。

## 失败与验收

批准不匹配、stale token、非法设置/命令、容量/计数器溢出在取消前拒绝。rename 前失败保留旧清单，但不复活旧绑定；调用者可显式开新请求读取旧安装。rename 后不确定状态保持关闭，直到重新打开并验证实际文档。取消失败也保持关闭，不能回滚为旧运行时仍有效。

单元测试使用真实私有临时目录与真实 ResultRegistry，覆盖目录/ZIP、快照不可变、重启、设置升级、授权拒绝/保留撤权/移除权限、停用、stale token、在途 effect/reply、结果退休、文件替换/损坏、提交故障与预算。独立 QuickJS 探针使用临时安装快照的实际入口字节，经真实数据服务与结果合同完成往返，并验证 helper 回收。只覆盖有界单入口 ESM；完整模块图装配保持后续项。

本轮不包含卸载/数据清理、生产安装管理 UI、公开分发验收或相同插件包双平台生产宿主验收。没有执行的 Windows 原生回归、签名 XPC、实机权限和系统能力继续保持未验证。
