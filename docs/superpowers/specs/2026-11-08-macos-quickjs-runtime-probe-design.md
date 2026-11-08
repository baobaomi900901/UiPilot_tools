# macOS 原样 ESM 与引擎资源限制实验

在 [JSC 模块转换实验](2026-11-08-macos-jsc-esm-probe-design.md)之后，评估无需 Node/esbuild 转换的独立原生运行时。本单元只验证固定源码和受监督 helper，不替代生产插件宿主设计，不增加 SDK 能力，不改变 Windows 或现有 Mac 应用的行为。

## 固定依赖与执行范围

新建 `spikes/macos-quickjs-runtime-probe/` 独立 Cargo workspace。固定 rquickjs 0.14.0、serde_json 1.0.150，并提交该实验自己的 Cargo.lock；不修改 `src-tauri/Cargo.toml` 或主锁文件。rquickjs-sys 所含 QuickJS-NG 头文件宏版本为 0.16.2，以锁定 crate 的校验和为源码依据。

rquickjs 关闭默认 feature，只显式启用 Rust `std` 与 `loader`；Rust std 不代表给 JavaScript 注入 std 模块。不启用 rust-alloc、自定义 allocator、动态原生库、调试 dump 或 bindgen。记录实际 feature tree；hashbrown 的 allocator-api2 依赖不等于开启 rquickjs 自定义 allocator。先 fetch 固定依赖并审阅 Mac 构建路径，随后仅 offline/locked 构建。rquickjs-sys 的 WASI 下载分支不属于此 Mac 目标。

公开接口参照[固定版本文档](https://docs.rs/rquickjs/0.14.0/rquickjs/)及下载后的对应源码。依赖 fetch 只下载源码；不运行上游安装脚本、不改系统信任或权限。本机仅运行无窗口固定 fixture，没有音频、个人数据、网络服务或剪贴板效果。

## 模块与调用边界

只从内存白名单加载编译嵌入的 UTF-8 JS 源码，包括未经转换的真实 demo-return 与固定合成模块图。虚拟模块名暂限固定 ASCII 夹具，是实验子集，不是 SDK 全部合法包路径的实现。resolver 仅规范化白名单内相对路径，loader 再次核对白名单；二者都拒绝 import attributes，包括模块已经加载并缓存后再次带 attributes 导入。不安装 file/native/script 备用 loader，不回退到磁盘、node_modules、URL、std/os 或系统命令。

允许固定图内的静态及动态 import，图外动态导入和静态导入都必须拒绝。原生入口不接受任意源码、文件路径或外部字节码；不得调用 Module::load/JS_ReadObject 来执行外部编译产物。import.meta 不注入宿主文件路径或系统服务。

等待模块 evaluation Promise 成功完成后才读取 onCommand。invocation/context 与空 API 冻结，输入符合公开合同：去掉边界空白、保留内部空格，日期固定，旧式单命令不补 commandId。私有 runner 由原生持有，不向包暴露全局完成回调；原生持有命令 Promise 并驱动 pending jobs，独立检查结果 requestId。需要真实 settings/storage/network 的调用不以成功占位补齐。

## 资源限制与回收

创建 Context 前配置有效的有限堆/栈上限与单调时钟 interrupt。堆限制只描述引擎分配，栈限制只描述引擎检查，不称为进程 RSS、宿主内存或 OS 沙箱。明确区分配置/模块初始化错误与资源夹具实际执行失败。

同一固定分配脚本分别在低、高堆限制下失败/成功；同一递归脚本分别在低、高栈限制下失败/成功。不能把任意异常当作上限生效，也不能把执行后的 memory_usage 称为峰值。栈上限不用会关闭检查的 0 或过大数值。

区分引擎 interrupt 触发并正常退出，与父进程到期强制回收。另以关闭 interrupt 的固定循环、永不完成 Promise 验证父 deadline 仍独立生效。父进程直接 spawn，输入最多 4 KiB、stdout/stderr 合计最多 8 KiB，严格检查唯一响应、generation/requestId 和成功退出；失败路径 kill + wait，本次子进程回收后才返回。每类失败后以新进程运行正常命令，不能依赖继续复用已经失败的引擎。

## 自动证据与剩余工作

验收原样 demo-return 完整结果、相对依赖/循环/live binding、Promise/top-level await、无系统/媒体全局入口、模块拒绝、引擎中断、堆/栈低高对照和父超时回收。记录锁文件、crate/引擎版本、feature tree、源码/产物哈希、逐案例实际状态、原生签名与 Mach-O 检查。源码运行不等于完整插件安装、权限或 UI 通过。

生产包快照、安装事务、请求调度/撤权、Host API bridge、进程 OS 沙箱、签名与打包、Windows 原生执行及 Web API 子集仍需后续实现；Panel/Window 的 WK 音频隔离依然独立阻塞。生产 plugins 保持关闭，同包双端生产通过数不因本实验增加。

锁定 rquickjs 的 Ctx pending-job 布尔接口会清除并折叠独立 job 异常；当前固定案例检查所持 Promise 的终态并保留父超时，未实现所有独立 job 异常的精确分类。生产调度接线前必须重新审查该路径，不能把固定案例通过扩大成完整运行时错误处理保证。
