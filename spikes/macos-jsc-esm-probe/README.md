# macOS ESM 模块兼容实验（无窗口）

本目录验证既有 JavaScript 模块经过构建时转换后，在独立 JavaScriptCore helper 中运行纯 `onCommand` 的结果。它不是生产插件加载器，不开放 Mac 插件、SDK 或权限桥；不会启动 GUI、播放媒体、联网或访问用户数据。`demo-return` 只构造复制动作数据，不执行复制。

在仓库已安装的锁定依赖和 Xcode Command Line Tools 下运行：

```sh
PATH="$HOME/.nvm/versions/node/v22.22.1/bin:$PATH" spikes/macos-jsc-esm-probe/build.sh
```

默认创建 `/private/tmp/uipilot-jsc-esm.XXXXXX`，也可传一个尚不存在的绝对输出目录。脚本不安装依赖；生成 11 个固定 helper，编译为 arm64 / macOS 15.7.4，逐个 ad-hoc 签名并验证，再执行自动测试。运行输入只有固定 case、generation 和 requestId；不接受源码、模块路径或宿主服务请求。

## 验证内容

- 真实 `com.uipilot.demo-return/package/dist/runtime.js` 不改源码与包格式：合法固定输入“中文 😀  保留空格”，完整比对 requestId、标题、描述及复制动作数据，日期来自固定 invocation。
- 合成相对导入、重导出、live binding、异步返回；invocation/context/空 API 冻结；保留标注 PURE 的顶层调用。
- 原 ESM 的 strict 语义：unbound function 的 this 为 undefined，未声明变量赋值与写只读 invocation 抛错，不污染 global。IIFE 输出显式加 strict banner；不能假定 runner 的 strict 会传入另一个独立 evaluate。
- 私有 runner 在包之前创建并捕获必要内建，完成 callback 只作为原生参数进入 closure，未放在 globalThis；包试图调用 `__finish` 不能伪造完成。包篡改 JSON/Promise/Reflect 全局不替换已捕获机制。
- 模块 response.requestId 在 runner 和原生侧另验；复用旧探针 supervisor 验证外层 generation/requestId、唯一 JSON、正常退出、有界双管道与真实 kill/wait 回收。超大或异常响应只有固定失败，不返回异常正文。
- 真实 `translate` 的 runtime/languages/providers 三模块可打包。固定 load-only helper 观测为 `moduleEvaluation.completed=false`，且加载前 `TextEncoder` 未定义；不检查或打印异常正文，不能仅据此宣称确证所有失败根因。它不会调用 onCommand、面板、网络或存储，未支持其 Windows-only 权限。
- 顶层死循环与永不完成 Promise 均由父进程 deadline 杀死并回收准确 PID，然后新的 helper 成功。不是线程超时，也不先收到完成就接受未退出进程。

## 构建边界

使用已锁定 esbuild 0.28.2 和 rolldown 1.1.5 的 AST parser。先快照包内相对 `.js` 模块（最多 32 个，每个 128 KiB，合计 1 MiB），拒绝符号链接、包外路径、裸/远程 specifier 和非 JS 资源；校验读取前后的 fd/路径 identity、长度和修改时间。打包只访问快照，所有 resolver/load 分支均返回已知模块或错误，不回退 node_modules/默认文件系统 resolver。`treeShaking:false` 和 `ignoreAnnotations:true` 保留顶层执行；Node 构建阶段从不执行被测模块。

AST 拒绝动态 import（包括不可达分支和计算参数）及所有 MetaProperty，因此本轮也保守拒绝 new.target。循环图语义、top-level await 和任意复杂模块等价性没有验收；不使用 regex 删除 export，也不修改真实插件 Manifest/SDK/Windows 宿主。

这是受控源码与固定 fixture 的实验。没有 OS 沙箱、生产恶意包隔离、CPU/堆配额、权限 bridge、任意模块加载器或安装事务保护；文件复核不是完整的敌对祖先目录并发替换防护。JSC+esbuild 不代表已选定生产引擎/loader。Panel/Window 的 WK 原生全音频隔离仍然独立阻塞，不因 Runtime 结果成功缩减这两种插件形态。

## 证据

输出含 builder-tests.log、native-tests.log（每次真实 PID/reap）、modules.json（每个输入/依赖/工具版本与 bundle 哈希）、源码和生成文件 SHA256SUMS、Mach-O/链接检查、基础 Git commit。源码哈希包括复用的 supervisor；基础 commit 不冒充尚未提交的实验源码。

开发红阶段分别验证了缺少 builder 实现、占位 helper 导致输出/回收行为断言失败。另一次真实 strict 红测在无 banner 的 IIFE 中得到四个 false；补 strict banner 后对应行为全绿。最终结果以本次 build 生成的日志为准。全程无需人工操作，也不计入“生产同包双平台插件已通过”数量。
