# macOS 宿主与插件秘密的 Keychain 基础

本单元实现移植计划 Task 9.3 的内部存储边界，尚不连接宿主凭据 UI、网络服务或插件 RPC。沿用现有 service 种类及插件 ID/设置 key 校验规则，Windows DPAPI 路径不变。

## 身份、授权与介质

宿主凭据使用 `com.uipilot.launcher.host-credentials.v1`，公开插件秘密使用 `com.uipilot.launcher.public-plugin-secrets.v1`，两者均为 generic password。account 由类型化 owner 和 key 按字节长度分隔编码；宿主 owner 只允许四种既有服务且 key 固定为 record，插件只允许有效插件 ID 和已有设置 key。不接受调用方指定 service、任意 Keychain 查询、通配符或搜索前缀。

授权由可信宿主显式创建。token 绑定当前 Vault 实例、owner 和不可复活的 generation；操作只从 token 推导 owner，不额外接受可冒用的插件 ID。失权、跨实例、非法 owner/key 在调用后端之前拒绝。撤回先关准入，在状态锁外等待已准入操作结束；同 owner 的重授权与撤回串行。shutdown 永久关闭并排空。撤权不删除已保存秘密，显式 delete 才删除。重新打开没有任何恢复的授权。

原生后端只使用 Data Protection Keychain。每次 CRUD 均指定 `kSecUseDataProtectionKeychain=true`、`synchronizable=false` 和 `WhenUnlockedThisDeviceOnly`，不传 classic search list/use-keychain，也不指定可跨应用的 access group，由系统根据签名决定默认组。不能在签名、entitlement 或现代后端失败后回退 classic。缺少签名条件时固定失败，签名稳定与升级访问仍需实机验收。本基础仅将已锁定的 security-framework-sys 2.17.0 加为直接依赖，复用现有 Core Foundation 0.10.1。

每次查询显式指定 `kSecUseAuthenticationUIFail`。它在现代 Security 实现中转为非交互认证参数；不调用全局交互开关，不设置同步、不修改系统账户。该常量在当前 SDK 可用但已弃用，后续改用 LAContext 须保持同一合同。最初 classic 方案因公开实现未证明处理该参数而被独立审核否决，未启用或实机执行。新版只允许现代路径，默认测试直接检查每个 native dictionary 和模拟调用轨迹。参考 Apple [现代 SecItem 实现](https://raw.githubusercontent.com/apple-oss-distributions/Security/main/OSX/sec/Security/SecItem.m)、[后端分流实现](https://raw.githubusercontent.com/apple-oss-distributions/Security/main/OSX/libsecurity_keychain/lib/SecItem.cpp)及 [TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains)。这些源码证据不代替目标系统上的原生锁定/签名验收。

## 数据与失败

宿主单记录最多 128 KiB，插件单秘密最多 64 KiB。值封装固定格式头及 service/account 的 SHA-256 身份摘要，读取时先验证长度、格式与身份，再向内部消费者返回正文；将原始记录搬到另一 owner/key 必须拒绝。该摘要不是加密或 MAC，不声称防御任意同用户恶意进程；机密性由 Keychain 及其访问控制提供，插件隔离由宿主授权提供。

Rust 持有的秘密缓冲不实现 Clone，Debug 始终隐藏内容，Drop 逐字节 volatile 清零。原生返回的 CFData 先检查大小再复制进 Rust；不能限制系统分配或保证不可变 CFData 的底层内存被主动清零。错误不带秘密、系统原始描述或个人路径，不写明文文件，不缓存秘密正文。

写入先对精确地址执行一次 SecItemUpdate，仅 item-not-found 才执行一次 SecItemAdd。并发创建导致 duplicate 时返回 Conflict，不删旧记录、不重试覆盖。其他失败如需交互单独返回；无法确认修改结果时返回 MutationUnconfirmed，不报告成功或声称已回滚。删除同样只作用于精确地址，not-found 返回未删除。底层失败不通过 delete-then-add 实现更新。

本单元的单 owner 准入锁覆盖后端调用，但 authority 状态锁不跨原生操作。撤回可能等待系统服务，未来宿主不得在 UI 线程或持业务锁等待。当前无公开 IPC 连接，未来凭据版本、服务租约及权限弹窗须按既有 SDK 合同独立装配，不能凭内部 token 宣称生产授权完整。

## 验收

默认 fake 后端及注入的 SecItem 调用层：宿主/两插件同 key 隔离、精确地址删除、非法及跨实例请求零后端调用、撤回/重授权旧 token 拒绝、错误与秘密脱敏、空值/Unicode/大小边界、错误身份封装拒绝、一次失败无删除与重试。阻塞写入时撤回或 shutdown 必须先关闭新准入，再等待原操作完成，旧 token 不能复活。重新创建 Vault 后仅再次明确授权才能访问原记录。

另检查真实 CFDictionary 的现代后端、无交互、非同步、本机解锁策略与精确地址；update 非 not-found 失败不得 add，duplicate 不重试，删除无回退。原生 CRUD、Keychain 锁定、签名升级后的访问控制尚待独立合成夹具验收；默认测试不打开或修改用户 Keychain。人工统一安排仍为整体约 70%。基础单元完成不等同于 Task 9.3 或插件凭据能力已经开放。
