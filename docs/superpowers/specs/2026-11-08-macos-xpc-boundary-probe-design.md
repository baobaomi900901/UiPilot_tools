# macOS XPC 打包与连接前边界实验

本单元落实[XPC 接入边界](2026-11-08-macos-runtime-xpc-boundary.md)中不依赖真实开发者证书的部分：构建独立无窗口 App 与内嵌固定 ping 服务，验证签名预检、连接配置顺序、请求归属、严格消息解析和 bundle 结构。现有 App、SDK、插件开关和 Windows 行为不变。

2026-11-08 本机 `security find-identity -v -p codesigning` 返回 0 个有效身份。用户明确暂无开发者账号，先完成其余开发。因此本轮只运行本地原生测试和不连接服务的预检；真实 XPC peer 认证、launchd 行为及 OS 沙箱验收延期。临时签名只用于校验生成物的签名自洽，不能替代 Apple 开发签名。

## 构建与签名

新增 `spikes/macos-xpc-boundary-probe/`，App 的 `Contents/XPCServices` 内放置单个固定 identifier 的 `.xpc` bundle。host 与 service 均以公开系统 SDK 构建，arm64、最低 macOS 15.7.4；不引入 QuickJS、包源码加载或任何 SDK 服务。

service entitlements 精确限定为 `com.apple.security.app-sandbox=true`，不授予 inherit、网络、用户选中文件、临时目录例外或应用组等额外能力。host 不因实验取得文件/网络权限。校验器检查内外路径、Info.plist、executable、签名 identifier、最终签名中的 entitlements、架构及最低系统版本；不能仅检查签名前的输入 plist。

默认 build-only：编译、签名、验证并输出证据，不使用 `open`、启动 XPC、注册登录项或修改现有应用。真实签名入口也只负责构建；必须另有验收步骤才能运行连接。host 无参数默认只预检，连接另用显式入口。不得运行带 app-sandbox entitlement 的 service 自检或预检入口，启动本身就可能创建容器；本地测试使用独立、不带沙箱 entitlement 的测试可执行文件。对错误 entitlement、bundle 结构或签名元数据的反例要明确拒绝，不自动修补为成功。

## 连接前鉴别与配置

host 在创建 XPC connection 前完成 Security 签名预检：用 `SecCodeCopySelf` 检查正在运行的代码，再核验磁盘 host/service 的签名及精确 identifier/Apple 信任要求；只有 validity 成功后才读取可信 signing information。核对 host/service 的 TeamIdentifier 一致，并比对正在运行的身份与对应磁盘代码的 team、代码哈希。validity 显式禁止网络访问。缺失、无效、临时签名或不匹配均返回固定 blocked 诊断，不创建连接。服务自身的启动检查同样不能接受无效身份。权限值必须是实际 CFBoolean true，数字 1 不可替代。

双方只调用一次公开 `xpc_connection_set_peer_team_identity_requirement`，传入对端精确 identifier；不降级为 identifier-only，也不叠加其他 peer setter。该 API 的运行认证依然需要后续真实证书实测；本地注入测试只证明宿主控制流。

正常连接必须先设置 handler 和 peer requirement，再激活并发送。setter 失败后拒绝业务准入且不发送消息。owned inactive connection 的释放遵守 XPC 生命周期：先 cancel，必要的 cleanup activation 与正常准入明确区分；listener 借用的 peer 可 cancel 后返回，不能按自有引用随意释放。测试不以“任何 activate 都必须为零”的错误断言破坏系统规定的释放流程。

## 消息与回调

协议只包含固定 ping 及回复，字段为平坦的原始类型，最多五个键、payload 最多 256 字节；单连接最多一个业务请求、一个在途回复。拒绝未知字段、错误类型、嵌套容器、文件描述符、endpoint 或额外能力对象。实际 XPC 接收后才可检查对象，不能把应用层字段限额宣传为系统反序列化前的内存硬上限。

串行队列处理 event、reply 和 deadline。每个回调捕获宿主持有的对象身份；替换后的旧回调、重复 reply、超时后的 reply 均不能提交当前结果。错误和超时先退役请求再取消连接，无自动重发或自动恢复旧请求。

本地适配器的 owner 只负责这次固定握手，不取代[共享 CallSession](2026-11-08-macos-runtime-call-session-design.md)。当前没有 backend 调用，因此不为演示增加 Rust FFI 或假 gate；真实 API 接入必须复用共享核心并再次验证整条路径。

XPC connection 不代表一个独占进程。服务可能处理多个连接；新 token、cancel、invalidated 不能证明新进程或旧进程已经终止。本单元不加载多个插件，也不证明跨插件进程隔离。未来选择进程复用策略、真实终止观察和资源预算后，才能接入不可信包代码。

## 自动验收和未验事项

原生测试分开记录：实际本地 XPC 对象的类型/字段校验；连接控制流的注入测试；回调 owner、重复和超时的确定性测试；构建与最终签名内容校验；无证书时真实 host 预检拒绝且不连接。注入成功结果不能计为原生 peer 认证成功。

记录工具版本、源码哈希、内外 binary 哈希、最终 entitlements、签名信息和测试数量。任何可执行的预检都必须先证明其路径不调用连接、服务启动、GUI、个人文件、网络或权限 API。本轮不创建用户容器，无需用户安装测试包。

真实签名后的正确/错误 peer、取消和崩溃事件、service 重启、受控文件/网络拒绝及多插件实例隔离仍未验。生产 Mac 插件继续关闭，同包双端生产通过数仍为 0。证书准备前继续开发其他宿主能力，不能将本实验当作沙箱验收完成。
