# Runtime 调用归属与关闭实验

本单元交付同源、transport-neutral 的宿主请求核心，以及真实 QuickJS 子进程的双向管道测试。只有明确命名的 **testMemory 内存测试 port**；它不是 SDK storage/settings，不宣称产品服务已可用。Mac 生产插件仍关闭，Windows 的宿主路径只改纯 DTO/validator 原名 re-export，没有改变运行逻辑。

## 共享核心

`src-tauri/src/public_plugins/call_session.rs` 同时由产品 Mac/Windows 模块和本探针编译。复用真实 `data_call_gate.rs`；请求 DTO 从既有 runtime/scheduler 原样提取到 request_contract.rs，plugin ID validator 原样移到 identity.rs，旧路径保留 re-export。没有伪造 scheduler Owner、manifest 校验或成功授权服务。

`CallSession` 是一条宿主私有请求 lane。`open` 要求已有真实 data gate 授权，绑定不可序列化的 Arc endpoint、完整 context、activation/admission 和 deadline；endpoint 由 adapter 在 spawn/connect 时捕获，不能由子进程自报 PID 或数字身份构造。两个 lane 即使 wire context 完全相同也不能互用 endpoint/完成票据。

每个 session 的 callId 严格从 1 连续增加，最多 32 次。call 准入检查 context、当前 Arc、状态、实际 gate lease 与当场单调时间；permit 跨实际 backend 执行，排队回复也仍被计数。reply 再取得新的 gate lease，跨锁外实际写入持有。prepare completion 必须所有 call/reply 排空，关准入并铸单次 opaque ticket；最终 submit 再检查当前 Arc/gate/deadline，用计数许可跨锁外同步宿主接纳闭包，不返回可缓存的 accepted bool。

close 先退役并阻止新 call/reply/submit，返回 drain 在锁外等待；shutdown 永久禁止重开。replacement 返回旧 drain，不表示旧 backend 已停。**已经准入的宿主接纳闭包可以在 replacement 后完成**，旧 drain 覆盖它；真实 UI 后续仍需原 owner 复核，或由宿主先等旧 drain 再开放共享可变资源，不能把旧回调当作新窗口的动作。

## 实际 adapter

每次运行独立 spawn 固定 QuickJS helper；父进程拥有 endpoint，子进程只回显 context 供比较。四字节大端长度帧，每帧最多 4096 字节；两个方向分别最多 8192 字节，子进程 stdout/stderr 合计计入出站预算。直接反序列化 typed deny_unknown_fields，测试重复 kind/context/callId/response.requestId 与未知字段拒绝。reader 队列最多 2 项，writer 队列最多 1 项且 try_send；每次 writer ack 后才处理后续消息，回复许可先释放再 ack。

父进程使用绝对 deadline（通常 2 秒，固定挂起负例 150ms），不因片段重置。complete 后仍检查完整 stdout **和 stderr** EOF，拒绝第二份完成、尾字节、reader 故障及超限，确认 child 正常退出后才提交。真实回归覆盖先关闭 stdout、随后超量 stderr 但正常退出的漏洞；旧实现误接纳的红测保留，双 EOF 后拒绝。

清理先退役，再断开/kill/wait，释放队列并 join reader/writer，最后 drain。子进程创建后立即由 OwnedChild RAII 持有，线程逐个登记到 Harness；失败路径不等待一个被阻塞的 reply drain 后才杀 child。helper 原生回调同步等待 stdin，不能被引擎 interrupt 打断，退出依赖父 deadline/kill。

集成测试实际执行 JS `onCommand` → testMemory.write → testMemory.read → 返回结果；另覆盖阻塞 backend 时撤回（先确定新调用 Expired，再确认 ack 尚未返回，释放已准入操作后 drain）、错误 owner、重复调用、提前退出、迟到/重复完成、部分 header/body、超量帧、尾字节和 stderr 尾超限。每类失败后新 helper 恢复。已准入 backend 可在撤回开始后结束，但确认排空之后不会继续执行该许可的写入。

## 构建与证据

```sh
PATH="/private/tmp/uipilot-rust-20261108/cargo/bin:$PATH" \
CARGO_HOME=/private/tmp/uipilot-rust-20261108/cargo \
RUSTUP_HOME=/private/tmp/uipilot-rust-20261108/rustup \
CARGO_TARGET_DIR=/private/tmp/uipilot-bridge-1108 \
spikes/macos-runtime-bridge-probe/build.sh
```

只 offline/locked，独立 Cargo workspace/lock；不改产品 Cargo。默认输出 `/private/tmp/uipilot-bridge-probe.XXXXXX`，也可传尚不存在的绝对目录。真实进程测试明确运行输出目录中 **签名后的 helper**；artifact-evidence.json 分别记其哈希与 Cargo target 原文件哈希，不能把只签名但没执行的文件称为已测试。bridge-cases.json 记录实际 PID、回收、接纳与 backend 次数，源码哈希包含同源 core/DTO/gate/validator 和旧路径集成文件。

没有 GUI、音频、个人数据、系统权限或网络服务操作。管道 adapter 只证明当前实验的协议/回收，不是生产进程架构或 OS 沙箱证明。现有 App 签名不证明独立收窄权限；未来 bundled XPC 服务的签名、peer 校验、失效与独立沙箱仍需分别设计验收。Panel/Window 的 WK 媒体隔离也没有因本单元解除。
