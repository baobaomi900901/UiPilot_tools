# macOS 插件 Runtime 的进程隔离接入边界

本记录约束无窗口 Runtime 的下一步接入，不表示 XPC 已实现或沙箱已通过。当前 QuickJS helper 的管道、引擎堆/栈限制和 kill/wait 只提供实验进程的执行与回收证据；生产 Mac 插件继续关闭。请求生命周期核心应独立于具体传输，供后续适配器复用。

## 现有事实与架构选择

现有 `src-tauri/FindDocuments.entitlements` 只有 `com.apple.security.files.bookmarks.app-scope`，没有 `com.apple.security.app-sandbox`。不能从文稿授权或 ad-hoc 签名推断插件执行进程已进入独立沙箱。

Apple 将 XPC Service 用于权限分离，服务可拥有独立沙箱，并放在包含它的 App 的 `Contents/XPCServices` 中。普通子进程的创建和回收不是这个隔离边界。服务由系统管理，连接与某个进程实例不能等同；服务还可能突然终止，因此重连不能恢复旧请求的权限。依据：[Creating XPC Services](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingXPCServices.html)。

下一轮隔离实验采用独立、签名的嵌入式 XPC Service 候选。服务只接收宿主批准的有界源码快照和调用数据，不获取用户目录、网络、剪贴板或任意文件描述符能力。引擎资源限制与 OS 沙箱分别验收；没有 JS 系统 API 也不能替代 OS 级限制。不得以私有 sandbox SPI 或 `sandbox-exec` 替代待验证的发布架构。

## 身份与生命周期

宿主生成不可序列化的实例 token，将实际连接、插件 activation/admission、请求和截止时间绑定。来自服务的插件 ID、PID、请求 ID 均只能用于一致性检查，不能创建权限。服务重启或连接替换必须使用新 token；旧 reply、完成或取消回调不得获得新实例的身份。

连接双方应在接受执行数据前配置并核验 peer 的签名要求。当前 SDK 的公开 `xpc_connection_set_peer_code_signing_requirement` 从 macOS 12 提供，每条消息都校验对端要求；较新的 `xpc_connection_set_peer_team_identity_requirement` 从 macOS 14.4 提供，并要求受支持的 Apple 颁发签名身份。当前 ad-hoc helper 的成功不能外推为 team identity 验证通过。实现时须检查设置函数返回值，并以错误身份的真实连接验证拒绝，不能只核对 bundle ID 字符串。

每端的同一连接只选择一种适用的 peer requirement 设置方式，不能重复或混用 setter 叠加条件；组合条件须表达为单一合法 requirement 或适用的 LWCR。以上 API 条件已核对本机 MacOSX 15.2 SDK 的 `usr/include/xpc/connection.h`；正式实现还需核对所选签名要求。参考 [Code signing requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)。

撤销先关闭宿主新调用与回复准入，再取消具体传输，最后在锁外排空已接纳操作。XPC interruption、invalidation、连接取消、服务终止是不同事件；不能把其中一个当作其他事件已完成。只有宿主生命周期核心可判定旧请求已退役，适配器另提供实际传输和服务清理证据。不得持有请求或 scheduler 锁等待 IPC、进程退出或外部回调。

## 接入前的验证门槛

- 检查内外 bundle 的签名、最终 entitlements、架构和最低系统版本；独立记录开发签名与发布签名证据。
- 用受控夹具验证正确 peer 准入、错误 peer 拒绝、旧连接回调不借新身份、超时、服务崩溃、重连及重复完成。
- OS 隔离测试只使用专门创建的测试资源，分别验证不允许的文件和网络操作；不能读取个人目录来证明拒绝。
- 确认服务不能从协议取得额外 FD、Mach endpoint 或沙箱扩展；限制消息大小、数量、在途数和解析资源。
- 将实际 SDK 服务授权、API 参数/结果校验、包安装快照与调度接线分别验收，最后才评估纯结果插件的生产开放。Panel/Window 的 WK 音频隔离继续单列。

本轮只完成架构与 SDK 可用性核查，没有创建或启动 XPC 服务、修改系统账户/权限、访问个人文件或改变现有 App entitlements。后续实际实验须记录容器和测试资源副作用及清理结果；不可把本记录记为运行验收通过。
