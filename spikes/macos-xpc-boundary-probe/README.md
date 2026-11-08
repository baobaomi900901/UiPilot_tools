# macOS XPC 连接前边界探针

此目录构建无窗口 App 和嵌套的固定 ping XPC service。它不接入 UiPilot、插件源码、QuickJS、SDK 或共享 CallSession；生产插件保持关闭。

```sh
spikes/macos-xpc-boundary-probe/build.sh
```

脚本在新的 `/private/tmp/uipilot-xpc-probe.XXXXXX` 构建 arm64 / macOS 15.7.4 产物，默认临时签名。运行的程序只有两个**无沙箱**本地测试 EXE 和 host 的 `--preflight`。绝不执行 service（即使是自检也可能创建容器），不调用 `--run`、`open` 或连接 XPC。

`UIPILOT_XPC_SIGN_IDENTITY` 可为未来明确批准的签名构建指定身份；构建仍只预检，不自动连接。当前机器没有有效开发者签名身份；临时签名预检应返回 `blockedSigningIdentity` / exit 3。无参数 host 和 `--preflight` 在所有签名条件下都零连接；只有另行授权的显式 `--run` 才可能连接，目前未运行。

## 实际边界

- host 先用 Security 检查当前运行进程身份，再检查自己和 service 的磁盘 Apple 信任要求及精确 identifier，再读取可信 TeamIdentifier/entitlements，要求相同非空 team，运行进程与对应磁盘签名 hash 一致，拒绝 ad-hoc。显式使用 NoNetworkAccess。service 的入口也执行相同预检。
- service 最终 entitlements 只允许 CFBoolean 类型的 `com.apple.security.app-sandbox=true`（整数 1 不接受）；host 无 entitlements。静态 validator 检查最终签名、Info、嵌套路径、arm64/minOS。静态沙箱声明不等于实际 OS 拒绝效果已验证。
- 两端仅配置 `xpc_connection_set_peer_team_identity_requirement` 加对方精确 identifier。返回零只代表策略配置完成，不能称真实对端已经认证。setter 失败先退役、取消；owned inactive connection 另做已取消后的 cleanup activation，符合 SDK 释放要求，绝不发送 payload。listener 借用的失败 peer 只取消。
- 只有固定 ping/pong：五个明确类型的平坦字段、最多 256 字节数据、一个请求/连接；service 同时保留最多八个握手。字典类型校验发生在 XPC 已解码之后，不是 OS 解码前内存硬上限。不传 FD、endpoint、安全作用域、任意 API 或源码。
- 串行队列、每握手不可序列化的对象身份及单次完成状态保护回复。旧 owner、重复、超时回复不能变成新请求。错误/超时先退役，再取消。timer 与 handler 弱引用 session；终态移除保留集合并取消 timer/connection。两秒 deadline 是进程队列上的握手超时，非硬实时终止保证。
- cancel、INVALID、reply barrier 不代表 service 进程终止；INTERRUPTED 不复用旧请求，不自动重试。新 connection 不证明独占或新 service 进程。跨插件进程隔离、资源预算及真实终止仍未设计验收。

## 验证证据

`policy.log` 是三种注入操作顺序测试（成功、owned 失败、borrowed 失败），不调用真实 connection API。实际红阶段先让 setter 失败仍返回成功，测试断言失败；修正后绿。`local.log` 使用真实本地 XPC 字典/数据对象验证类型、长度、未知字段和 owner 身份/撤销/超时，仅创建值对象。

`validator-tests.log` 是复制 bundle 后修改并重新签名的反例，包括额外 entitlement 和错误 signing identifier；不运行副本。`validation.json`、`preflight.json`、最终 entitlements、签名信息、工具版本以及源码/产物 SHA256 随产物保存。host 的 `connectionsCreated:0` 是无连接分支的固定控制流诊断，不是 OS 追踪计数器；代码审计确认该分支不进入 `probe_run_host`。

未验证：真实开发者身份下的双端 peer 认证、launchd 启动与重启、取消/崩溃时序、实际文件/网络沙箱拒绝、跨插件进程隔离。适配器中的真实连接路径仅编译，不能作为运行通过证据。本轮没有媒体或生产隔离证明。

## 公共 API 依据

- [Apple Creating XPC Services](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingXPCServices.html)：嵌套 bundle、独立 service 沙箱与 launchd 生命周期。
- 本机构建 SDK 的 `usr/include/xpc/connection.h`：team identity requirement（macOS 14.4 起）、inactive owned connection 释放、cancel/interrupt/reply 顺序限制。
- `Security.framework/Headers/SecCode.h` / `SecStaticCode.h`：先检查签名有效性，再信任 signing information。

本探针不把 pipe 子进程或临时签名当作 Apple XPC 隔离验收；没有证书不触发放宽策略。
