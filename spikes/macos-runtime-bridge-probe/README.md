# Runtime 调用归属与关闭实验

本单元交付同源、transport-neutral 的宿主请求核心，以及真实 QuickJS 子进程的双向管道测试。保留明确命名的 **testMemory 内存测试 port**，另将普通 settings.get、storage.get/set/remove 接到实际 Mac 私有文件服务。后者不是成功假服务，但仍是内部受监督传输，未装配生产插件加载器或开放完整 SDK。Mac 生产插件仍关闭，Windows 原运行逻辑不变。

## 共享核心

`src-tauri/src/public_plugins/call_session.rs` 同时由产品 Mac/Windows 模块和本探针编译。复用真实 `data_call_gate.rs`；请求 DTO 从既有 runtime/scheduler 原样提取到 request_contract.rs，plugin ID validator 原样移到 identity.rs，旧路径保留 re-export。没有伪造 scheduler Owner、manifest 校验或成功授权服务。

`CallSession` 是一条宿主私有请求 lane。`open` 要求已有真实 data gate 授权，绑定不可序列化的 Arc endpoint、完整 context、activation/admission 和 deadline；endpoint 由 adapter 在 spawn/connect 时捕获，不能由子进程自报 PID 或数字身份构造。两个 lane 即使 wire context 完全相同也不能互用 endpoint/完成票据。

每个 session 的 callId 严格从 1 连续增加，最多 32 次。call 准入检查 context、当前 Arc、状态、实际 gate lease 与当场单调时间；permit 跨实际 backend 执行，排队回复也仍被计数。reply 再取得新的 gate lease，跨锁外实际写入持有。prepare completion 必须所有 call/reply 排空，关准入并铸单次 opaque ticket；最终 submit 再检查当前 Arc/gate/deadline，用计数许可跨锁外同步宿主接纳闭包，不返回可缓存的 accepted bool。

close 先退役并阻止新 call/reply/submit，返回 drain 在锁外等待；shutdown 永久禁止重开。replacement 返回旧 drain，不表示旧 backend 已停。**已经准入的宿主接纳闭包可以在 replacement 后完成**，旧 drain 覆盖它；真实 UI 后续仍需原 owner 复核，或由宿主先等旧 drain 再开放共享可变资源，不能把旧回调当作新窗口的动作。

## 实际 adapter

每次运行独立 spawn 固定 QuickJS helper；父进程拥有 endpoint，子进程只回显 context 供比较。四字节大端长度帧。模块图初始化可使用一次单独的 24 MiB 输入预算；随后的 RPC 每帧最多 4096 字节，两个方向分别最多 8192 字节，子进程 stdout/stderr 合计计入出站预算。直接反序列化 typed deny_unknown_fields，测试重复 kind/context/callId/response.requestId 与未知字段拒绝。reader 队列最多 2 项，writer 队列最多 1 项且 try_send；每次 writer ack 后才处理后续消息，回复许可先释放再 ack。

父进程使用绝对 deadline（通常 2 秒，固定挂起负例 150ms），不因片段重置。complete 后仍检查完整 stdout **和 stderr** EOF，拒绝第二份完成、尾字节、reader 故障及超限，确认 child 正常退出后才提交。真实回归覆盖先关闭 stdout、随后超量 stderr 但正常退出的漏洞；旧实现误接纳的红测保留，双 EOF 后拒绝。

清理先退役，再断开/kill/wait，释放队列并 join reader/writer，最后 drain。子进程创建后立即由 OwnedChild RAII 持有，线程逐个登记到 Harness；失败路径不等待一个被阻塞的 reply drain 后才杀 child。helper 原生回调同步等待 stdin，不能被引擎 interrupt 打断，退出依赖父 deadline/kill。

集成测试实际执行 JS `onCommand` → testMemory.write → testMemory.read → 返回结果；另覆盖阻塞 backend 时撤回（先确定新调用 Expired，再确认 ack 尚未返回，释放已准入操作后 drain）、错误 owner、重复调用、提前退出、迟到/重复完成、部分 header/body、超量帧、尾字节和 stderr 尾超限。每类失败后新 helper 恢复。已准入 backend 可在撤回开始后结束，但确认排空之后不会继续执行该许可的写入。

## 实际持久数据接线

`runtime_data_macos.rs` 与设置、存储、文件介质和 manifest 均引用产品目录的真实同源实现。可信宿主先保存设置，再创建捕获 scope/settings handle 的 binding；子进程不能提供定义、路径或 activation。每次数据操作经过真实 CallSession，读取 CallPermit 的真实 identity，持有后端 lease；ReplyPermit 覆盖实际 writer。管理切换关闭旧 gate/session、取消传输、排空，然后以新 epoch 原子 reconcile+保存，成功才发布新 binding。取消失败后不重开，Drop 立即撤销但不能替代显式传输清理。排空后已过期则不写配置；若配置已成功持久化、随后请求发布因过期失败，保留已保存配置，不报告回滚。

真实进程执行未修改的 `demo-settings/package/runtime.js`，四次 settings.get 读取落盘值；重建宿主和 helper 后读回同值。另验证 JSON storage 写/读/重开、未知设置拒绝、伪造 owner、设置写操作和重复 call 拒绝。数据完成载荷的原始 requestId 由 helper 和父进程分别核对，外层正确 ID 不能掩盖包结果的错误或缺失 ID。撤销案例在已提交 storage 写后杀死等待回复的 helper，丢弃未发送回复并排空；重开读到已提交值，不虚报回滚。

本探针每帧 4 KiB/累计 8 KiB 仍为诊断预算，不表示完整 SDK 的值传输上限。所有 raw 帧先拒绝重复 JSON 键再 typed 解析。数据读写夹具保留内部 data 结果格式；`dataDemo` 与 `dataMain*` 则经过产品同源完整 legacy mainResult parser 和 Runtime 完成票据，不把两种格式混作公开 SDK。

## 主结果完成接线

`main_result_contract.rs` 从 Windows activation 逐字提取，原导出、错误、20 行/64 KiB 及字段边界不变；`runtime_result_macos.rs` 先校验原始 JSON、真实请求 ID 和完整 rows，再 prepare completion，双 EOF/正常退出后 submit completion。探针接纳的是解析后的实际 typed rows。未知字段、重复 row ID、21 行和不可用 copyText 动作分别拒绝；空结果保留合法。复制权限/动作执行尚未装配，不能通过删除 defaultAction 假装支持。

产品内部 `main_result_macos.rs` 另外接到真实 ResultRegistry，主库测试验证 publisher 实例/查询 owner、一次完成、迟到错误和旧提交被新查询淘汰；这一层不在独立探针假造 registry。GUI 消息与前端仍未接通；生产包安装、OS 权限服务和 XPC 沙箱仍待装配。内部发布结果均为无动作的信息行。

## 已安装包快照链路

`installedDemo` 使用产品同源 package_macos/lifecycle_macos：真实开发目录或归档校验后的不可变快照，经独立私有 inventory 提交；持久普通设置、enabled 和声明权限状态由 lifecycle 管理。新测试复制 tracked demo-settings 到私有临时目录，安装并保存设置后删除该临时源文件，首次执行与宿主控制器重开（同一测试进程 drop/load，加新 helper PID，并非宿主进程重启）都仍从 `InstalledRequest.package.resource(runtime.entry)` 取源码，不再回源文件或使用 include_str。升级在临时包中改变版本/固定 build 标记，安装后实际返回新的标记。

`installedDemo` 在初始化消息中传输一个入口，最多 2048 UTF-8 字节，由 QuickJS 编译执行。该模式只运行单文件夹具。`launcherDemo` 通过独立的模块快照字段接收入口和包内源码。

每个真实 child 由 open_request 的 trusted attachment 创建，lifecycle 持有捕获原 Harness 的 retirement。停用/撤回测试在第一个真实 settings.get 已完成、尚未给 child 回包时丢弃 reply permit，再执行管理变更；lifecycle 先关闭旧调用、运行 cleanup kill/wait/join，再排空。测试核对旧 binding 拒绝，旧 child 已回收，释放所有旧 request/store Arc 后才重开私有目录，新请求恢复。此处 clipboard.write 仅测试声明授权状态，不执行任何剪贴板效果，也不宣称已获得 OS 权限。撤回已经准入的读不作回滚。

包层保持同源路径/数量/解压与快照恢复校验，Mac ZIP 是受限 standard 单盘非 ZIP64 子集。单包编码与整个 inventory 各有独立 5 MiB 上限，完整 SDK 16 MiB 包并未因此全支持。文件系统稳定性复核、协作 flock 和 digest 不等于跨进程文件 CAS、签名或同 UID 恶意篡改防护。Panel/Window/resultActions 仍不支持，生产插件开关仍关闭。

## 构建与证据

```sh
PATH="/private/tmp/uipilot-rust-20261108/cargo/bin:$PATH" \
CARGO_HOME=/private/tmp/uipilot-rust-20261108/cargo \
RUSTUP_HOME=/private/tmp/uipilot-rust-20261108/rustup \
CARGO_TARGET_DIR=/private/tmp/uipilot-bridge-1108 \
spikes/macos-runtime-bridge-probe/build.sh
```

只 offline/locked，独立 Cargo workspace/lock；不改产品 Cargo。默认输出 `/private/tmp/uipilot-bridge-probe.XXXXXX`，也可传尚不存在的绝对目录。真实进程测试明确运行输出目录中 **签名后的 helper**；artifact-evidence.json 分别记其哈希与 Cargo target 原文件哈希，不能把只签名但没执行的文件称为已测试。bridge-cases.json 记录实际 PID、回收、接纳与 backend 次数，源码哈希包含同源 core/DTO/gate/validator 和旧路径集成文件。

## 宿主协调器进程测试

在仓库根目录运行下列命令，将 helper 路径替换为上一步生成的绝对路径。

```sh
UIPILOT_LAUNCHER_TEST_HELPER=/private/tmp/uipilot-bridge-probe.XXXXXX/helper \
  cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib \
  --features macos-find-preview,macos-login-item-preview \
  launcher_runtime_macos::tests::process -- --ignored --skip process::archive --nocapture
```

宿主测试从临时安装库调用正式宿主协调器。原样示例核对四项设置和真实注册表结果，停用场景核对等待首条回复的子进程在管理操作返回前已回收。多文件场景删除导入来源后执行快照中的嵌套入口，验证静态导入、重导出、动态导入、绑定更新及真实设置读取。缺失模块和越界导入在调用宿主数据服务和发布结果前失败。

原样 `demo-return` 验证复制动作的两次授权检查。未授权的动作在发布前拒绝，已发布结果在撤权后无法进入写入。授权有效时，测试驱动接收私有结果中的文本，消费原结果票据后只执行一次写入。测试使用注入驱动。

命令候选测试覆盖前缀搜索、参数提示和带身份的提交。预览与不匹配的提交不启动引擎，合法提交执行已安装的脚本并返回当前命令和参数。

### 正式归档验证

在 Apple Silicon Mac 上准备仓库的 npm 依赖和离线 Cargo 缓存，然后从仓库根目录运行 `node scripts/verify-macos-plugin-archives.mjs`。脚本调用正式 CLI 为三个参考插件及设置示例的升级版分别打包一次，让 Windows 和 macOS 目标的静态校验读取同一份归档，再运行 macOS 宿主进程测试。

两个独立的宿主测试进程验证设置重启恢复和兼容升级。归档测试还覆盖双命令共享存储，以及复制授权和撤权。输出目录包含可复用的插件包、SHA-256 清单、逐平台 CLI 报告和宿主测试日志。`verification.json` 分别记录 CLI 与 macOS 诊断宿主的结果，正式宿主验收单独登记。

模块图由宿主在传输前从 VerifiedPackage 的已拥有资源生成，并在 helper 再验。复用 package_rules 的 256 文件、单文件 2 MiB、总源码 16 MiB、8 层和路径限制；安装快照原有 5 MiB 序列化上限仍约束实际包。支持精确 Unicode/空格路径、循环依赖、re-export、live binding、动态 import 和 TLA。拒绝缺模块、大小写/NFC 别名、目录冲突、NUL/无效 UTF-8，以及 URL、裸系统模块、越界、import attributes 和百分号形式。无文件系统回退。

首个初始化帧单独允许 24 MiB，仍校验唯一 JSON 键和深度。后续 RPC 的 4 KiB 单帧/8 KiB 累计不变；重复初始化拒绝。真实安装测试包含 43 个模块、超过 1 MiB 源码和超过 6 MiB 的转义 JSON 首帧，源目录替换后仍执行安装快照。阻塞的大首帧写入取消后会 kill/wait 并回收线程。QuickJS 仍限 8 MiB 堆、1 MiB 栈及 1 秒引擎截止；这些是诊断预算，不代表所有包可在这些资源下运行。

这个适配器仅编译进宿主测试。普通构建仍关闭插件执行。签名 XPC、系统沙箱及 GUI 交互另行验收。

没有 GUI、音频、个人数据、系统权限或网络服务操作。管道 adapter 只证明当前实验的协议/回收，不是生产进程架构或 OS 沙箱证明。现有 App 签名不证明独立收窄权限；未来 bundled XPC 服务的签名、peer 校验、失效与独立沙箱仍需分别设计验收。Panel/Window 的 WK 媒体隔离也没有因本单元解除。
