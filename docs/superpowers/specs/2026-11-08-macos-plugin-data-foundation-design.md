# macOS 插件私有存储与普通设置规则

本单元实现可自动验证的宿主数据基础：共享真实插件作用域、JSON/存储文档规则和普通设置规则；为 Mac 提供受 data gate 保护的真实私有文件存储。生产插件继续关闭，不连接 XPC、Runtime gateway、设置 UI 或真实插件包，不增加 SDK 能力声明。

## 共享规则与 Windows 行为

`PluginDataScope`、作用域检查和 `valid_json_value` 从现有宿主逐字提取，保留原导出路径及错误。scope 只绑定数据所属插件，不代表当前请求、用户授权或有效 activation；不能用一个 ID 字符串代替 gate。

共享存储文档保留 schema 1、pluginId、有序 values map 与原错误类型。values 序列化后的配额仍为 5 MiB；key 校验、JSON 非有限数及危险对象键拒绝保持一致。Windows 仅委派纯数据处理，原路径 IO、备份、损坏文件隔离及卸载行为不在本次变更范围。

普通设置规则继续使用真实 `PublicSettingV1`：合法旧值保留，失效旧值回落到声明默认值，无默认值的可选项不凭空生成；移除未声明和 secret 字段。普通更新拒绝 secret、未知 key、错误类型和越界值。数值 step 延续现有合同，不额外要求步长整数倍。空字符串、false、0 都是有效值，不能按真假值改成默认值。

Windows state 的错误与加锁顺序不变：读取时先 scope，再 definition/secret 检查，再锁和插件存在检查，最后读存储值或默认值；读取不顺便再次 reconcile。保存先校验全部 updates，再进入原事务。Mac 只共享这些纯规则，本轮不伪造持久 settings store、安装状态或用户授权服务。

## Mac 介质与身份

调用者先提供一个已存在的本地私有根目录。以 nofollow 的目录 FD 相对操作 owner 目录及保留文件，绑定根目录与 owner 的文件系统身份；操作前复验当前名称仍指向持有对象。根/owner 要求当前用户所有、0700、本地文件系统且无扩展 ACL；数据与锁文件要求 0600、普通文件、单硬链接、当前用户所有、无扩展 ACL。

根目录以独立、稳定的锁对象做整 store 排他保护，覆盖其中所有 owner；不把将被 rename 的数据文件当作永久锁。拒绝符号链接、硬链接、目录替换、非普通文件和不满足权限的对象，不通过 chmod 或全盘扫描去修复外部对象。所有测试在新建临时目录中执行，不接触现有插件数据。

Mac 原始文档读取上限为 5 MiB + 256 字节，先有界读取再解析；这是 Mac 文件输入限制，不改 Windows 既有恢复逻辑。Mac 文件额外拒绝 JSON 重复键，包括嵌套对象，保留 serde 的递归限制；Windows parser 行为不在本次调整范围。共享 5 MiB 配额仍只计算 values。固定 schema 字段及最长合法 pluginId 的额外编码必须在该余量内验证。损坏/超限/owner 不匹配返回错误，不能报告空数据库或静默回退为成功。

Mac store 按 owner 惰性打开，支持 set/get/remove 及关闭重开读回，不遍历其他 owner。每次重新打开、有界读取并校验实际磁盘文件，打开的 FD 与当次同名对象须一致，不用旧内存缓存掩盖数据损坏。首次读取绑定数据文件不存在或其文件系统身份；本实例成功 rename 后更新身份。外部替换即使字节相同也拒绝，重新打开 store 才能重新绑定；根和 owner 目录仍固定身份。卸载数据删除和跨进程多实例调度留后续单元。

## 写入、失败与授权撤销

候选数据完整校验和配额检查后，写入私有 staging 文件、同步文件，再原子 rename 并同步目录。rename 之前失败保留旧文档；rename 之后同步失败时磁盘可能已有新值，内部标记提交结果不确定并禁止继续使用该 owner 的已打开存储，不能报旧值仍在或自动重试。关闭整体 store 后才可重新打开检查，其他 owner 不因这个局部标记自动失效。对外沿用既有 Storage 错误，保留内部不确定状态用于后续宿主故障处理。

在任何 owner 目录 IO 前验证真实 scope 与 gate identity 的 pluginId 一致，再取得当前 generation/activation/admission 的实际 lease；lease 覆盖完整读写。关闭 gate 后新操作拒绝，撤销确认等待已准入操作在锁外排空。已准入写入可以完成，撤销不是事务回滚；旧 gate 不能借新授权操作另一 owner。

文件路径、作用域和 gate 是不同边界。测试必须证明 scope A 与 gate B 拼接、关闭 gate、旧 identity 及错误 owner 都不会创建或修改目标文件；不能只测输入参数形式。

## 验收与仍未开放的能力

真实文件测试覆盖多 owner 隔离、Unicode 值、set/get/remove、重开读回、配额、损坏、链接/权限/替换拒绝、排他锁、rename 前/后故障以及阻塞 IO 时撤销排空。共享设置测试覆盖升级保留、默认值、删除字段、secret 排除和错误类型；核对 Windows caller 仍委派同一份规则，运行主 crate 聚焦测试与全库回归。

成功只表示内部数据基础可用。Mac 持久安装/配置事务、SDK gateway、Runtime 调度、UI 和同包双端实机仍待接入；真实 Windows 原生运行另行记录。本单元不需要开发者证书、GUI、个人数据或人工安装包。
