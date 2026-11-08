# macOS 原样 ESM / QuickJS-NG 实验

独立 helper 验证未经转换的 ES Module、内存模块白名单与引擎资源限制。生产插件、SDK、Windows 和现有 Mac 宿主均不接入。运行固定合成夹具与真实 `demo-return` 纯命令，不创建 GUI、不播放媒体、不读写剪贴板、不调用权限服务或网络。

## 构建和验证

要求 Apple Silicon macOS、Xcode Command Line Tools、Rust 以及已获取的精确锁定 crate 缓存。本脚本只 offline/locked 构建，不下载依赖或运行上游安装脚本。

```sh
PATH="/private/tmp/uipilot-rust-20261108/cargo/bin:$PATH" \
CARGO_HOME=/private/tmp/uipilot-rust-20261108/cargo \
RUSTUP_HOME=/private/tmp/uipilot-rust-20261108/rustup \
CARGO_TARGET_DIR=/private/tmp/uipilot-quickjs-1108 \
spikes/macos-quickjs-runtime-probe/build.sh
```

可替换工具链目录，也可传尚不存在的绝对输出目录；默认输出 `/private/tmp/uipilot-quickjs-probe.XXXXXX`。脚本使用 debug 配置、macOS deployment target 15.7.4，运行 Rust 单测、原生进程测试、传输协议测试，检查 ad-hoc 签名与 Mach-O/链接。基于源码校准的资源对照属于这一个实际构建，不推广为所有优化级别/架构的固定递归深度。

独立 Cargo workspace 锁定 rquickjs 0.14.0、serde_json 1.0.150；rquickjs-sys 内置 QuickJS-NG 0.16.2。rquickjs 仅 `std,loader`，关闭默认 feature。Rust std 不给 JS 提供 std/os。没有 rust-alloc/custom allocator/dyn-load/字节码 loader；feature tree 与 metadata 会校验 rquickjs 系列实际 feature。hashbrown 的 allocator-api2 不是 rquickjs 自定义 allocator。

## 实际合同

- 原样嵌入真实 demo-return 源码，完整比对中文/emoji、内部双空格、固定日期、requestId 和复制动作数据；复制动作不执行。
- 固定相对模块图验证重导出、循环、live binding、Promise、top-level await 与白名单内动态 import。真正等待模块 evaluation Promise 完成，再取 onCommand；不能仅凭 Rust `Evaluated` 类型判断顶层 await 已完成。
- Resolver 和 Loader 双重检查固定内存 UTF-8 `.js` 图，无磁盘、node_modules、URL、native library、std/os 或外部 bytecode 回退。静态与动态越界均拒绝，已缓存模块也不能绕过 import attributes 拒绝。虚拟路径只支持本实验的 ASCII 子集，不宣称实现完整 SDK 路径或包安装验证。
- Graph 最多 32 文件，每个 128 KiB、合计 1 MiB。源码编译嵌入；IPC 只能选择固定 case，不能提交模块/路径/代码。未注入 import.meta 宿主文件路径或服务。
- 私有 runner 在模块前创建，由 Rust 保留；捕获必要 intrinsics、冻结 invocation/context/空 API。没有公开完成回调或成功假 settings/storage 服务。Rust 持有返回 Promise，单独核对 response.requestId；外层 supervisor 再验证 generation/requestId、唯一 JSON、成功退出。
- 异常不输出正文。引擎任务队列通过锁定库的 `Ctx::execute_pending_job` 驱动，其 bool 不区分成功任务和任务异常；本轮依靠对应 Promise 状态、interrupt flag 与父 deadline，不宣称精确分类所有独立异步任务错误。

## 资源与回收证据

Context 创建前配置有限堆/栈和单调时钟 interrupt。正常堆 8 MiB，低堆 2 MiB；同一脚本真实分配并触碰 4 MiB Uint8Array。正常栈 1 MiB，低栈 64 KiB；同一脚本执行 100 层非尾递归。测试要求低档在 `command` 阶段失败、高档在该阶段成功；不能把 Context/模块初始化失败算作资源测试通过。第一次 400 层脚本两档都失败的日志保留，未把它当作成功对照。

`resources.json` 记录实际限额、阶段、结果、interrupt 是否触发，以及 Context 尚存活时、执行结束后的 memory_usage；这些值不是峰值或进程 RSS。`status=ok` 表示诊断 envelope 完整，必须再检查 `outcome=completed/failed/interrupted`，不是所有资源操作成功。

真实循环触发引擎 interrupt 后正常退出；另有关闭 interrupt 的循环、命令 pending Promise、模块顶层 pending Promise，被父 deadline 直接 kill 并 wait 回收。每类失败后新 helper 成功。引擎中断不能打断宿主阻塞回调或没有 JS 执行的 pending Promise，父回收不能替代 OS 沙箱或总内存预算。

父进程沿用前一探针严格 supervisor 实现，只有文档说明和固定 case 白名单变化：输入 4 KiB、stdout/stderr 合计 8 KiB、deadline 优先、重复字段/第二份响应/非有限数/额外字段拒绝，必须正常退出后才接纳。协议夹具另测双流各 5000 字节超限、完成后死循环、持续 stderr、父侧读取异常的真实回收。

## 产物和边界

产物包含单测/native/protocol 日志、resources.json、features.log、限定 Mac 平台的 cargo metadata、锁文件/引擎源码/header/build 脚本/可执行文件哈希、全部实验与复用源码 SHA256SUMS，以及编译器和签名/链接证据。源码基础 Git commit 不替代未提交源码哈希。

本实验不等于生产 Runtime、任意 ESM 兼容、安装事务、Host bridge、撤权调度、进程沙箱、签名发行或跨平台插件验收。Panel/Window 的 WK 全音频隔离仍独立阻塞；没有把它们从“插件通用”的最终目标移除。
