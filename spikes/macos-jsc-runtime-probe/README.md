# JavaScriptCore 无媒体 Runtime 独立探针

这是固定合成脚本的技术实验，**不是插件 Runtime 实现**。不加载插件包，不开放宿主或 SDK；Windows 不变。Panel/Window 的 WK 音频隔离仍受阻。

## 自动构建与验证

```sh
./spikes/macos-jsc-runtime-probe/build.sh
```

需 macOS arm64、Command Line Tools、Python 3.10+；部署下限 15.7.4。默认创建新的 `/private/tmp/uipilot-jsc-probe.XXXXXX`，可传入尚不存在的绝对输出目录，不能覆写已有目录。脚本编译/link/ad-hoc 签名，然后**实际运行无窗口 JSC 子进程**及自动测试；不会创建 GUI、播放音频、访问个人文件、系统剪贴板、网络或权限设置。源码哈希、构建时 HEAD、测试与链接日志随产物保留；HEAD 不代表未提交源码已提交。

测试涵盖裸全局能力缺失、同步返回、async/Promise、脚本错误脱敏、ESM 明确拒绝、畸形/额外字段/重复 JSON 键/重复线协议响应/重复 JS 完成回调/错误归属/过大输出、stdout 与 stderr 合计限额（各 5000 字节，各自未超限但总量超限），以及同步死循环、永不完成 Promise、先回调再死循环。超时案例确认真实子 PID 被 kill + wait 回收，再用新进程成功执行。

## 边界

父进程用直接 `Popen`，不通过 shell；只允许固定 case 枚举和 generation/requestId。输入最多 4 KiB，stdout+stderr 合计最多 8 KiB；stdin/stdout/stderr 均非阻塞，由同一 deadline 驱动。完整唯一响应必须在子进程正常退出后才可接纳，不能看到回调就提前报成功。归属、字段、UTF-8、JSON、重复键与有限值均检查。第二次 JS 完成回调令本次任务失败，返回固定 scriptFailed，不采用 first completion wins；异常只输出固定类别；失败路径在 finally 中 kill 并 wait，不以线程超时冒充终止。等待回收可能受操作系统调度影响，不宣称实时上界。

helper 仅链接 Foundation 与公开 JavaScriptCore，固定脚本是源码内枚举，不接受 source/path 参数。唯一桥接受合成结果字符串；不导出 NSObject、JSExport、任意 native class、文件、进程、网络或媒体服务。传输错误案例是显式原生合成故障，并非提供给 JS 的系统接口。父侧校验与限额不等于 JSC 内部堆分配限制。

本机 SDK 的公开 `evaluateScript` 接受 script，不能直接执行既有 `export onCommand` ES Module；本探针保留负测试，不 regex 重写源码。TextEncoder、URL、定时器等 Web API 不存在；后续兼容范围须另设计。现有 5 秒 live / 30 秒 submit 调度、ESM 图加载、插件权限服务、生产进程沙箱、CPU/内存硬配额和完整生命周期尚未实现。隔离进程可回收不等于已证明生产沙箱或抵抗引擎漏洞。这里的短测试 deadline 只用于固定 fixture 验收。

裸 JSC 无 DOM/media 能力是这条架构的依据；不是“静音”或 WK 原生 GuardProof。不得以这些结果开放生产插件。
