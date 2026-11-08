# macOS 无窗口 JavaScriptCore 子进程探针

WKWebView 暂停路径已在 M1 实机发生 WebAudio 发声，依据见[媒体探针结论](2026-11-07-macos-media-probe-design.md#2026-11-08-实机结论暂停路径未通过)。本单元调查一个没有浏览器媒体入口的独立 JS 引擎能否完成固定异步任务，并在任务卡死后被父进程回收。它是独立技术探针，不改变批准的公开插件合同，不替代尚未完成的 Runtime 架构设计。

## 执行与归属

实现位于 `spikes/macos-jsc-runtime-probe/`。公开 JavaScriptCore API 的原生 helper 只执行源码内固定 case；不接收 JS 源码、插件包或文件路径。没有 AppKit/WKWebView 窗口、音频、个人数据访问、网络请求或系统权限操作。仅导出接收合成 JSON 结果字符串的回调，不导出任意 native 对象或文件、进程、网络服务。

父 supervisor 直接启动 helper，不使用 shell；子进程以最小环境和私有标准管道通信。每次运行对应一个 generation/requestId，最多发送 4 KiB 输入，stdout/stderr 合计最多读取 8 KiB。非阻塞输入输出共用单调时钟 deadline；同步死循环、永不完成的 Promise、已经回调后继续死循环都不得让父端提前报告成功。失败路径必须终止并 wait 回收本次子进程，回收受操作系统调度影响，不承诺实时上界。

仅当唯一响应通过结构、归属、UTF-8、JSON 和有限值检查，且子进程以成功状态退出，才接纳结果。重复字段、额外字段、重复响应及旧归属均拒绝；异常只给固定类别。父端有界传输不能被描述成 JS 堆内存配额或操作系统沙箱。

## 验证与边界

自动检查实际编译/link、ad-hoc 签名并执行固定合成子进程，无需用户启动 App。验证同步返回、Promise 完成、所列浏览器/系统全局入口缺失、错误脱敏、协议故障、超时 kill/wait 和新 helper 恢复。错误测试单列旧 generation 与错误 requestId；超时后核对旧 PID 已被回收，再启动新进程完成正常任务。

现有插件 `runtime.entry` 是 ES Module。本探针必须保留 `export onCommand` 无法由当前 evaluateScript 路径直接执行的负例，不得 regex 去掉 export 后宣称兼容。真实包相对 import、模块图、Web API 子集、权限 bridge、调度、完整生命周期和生产进程沙箱另行设计与验收。

无 DOM 的隐藏引擎不能解决 Panel/Window 的 WK 音频隔离，也不能签发 WebViewGuardProof。生产 plugins 保持关闭；不缩减原定双端插件共用目标，不增加同包双端插件通过数。
