# macOS 既有插件 ESM 转换独立实验

在[无窗口 JSC 子进程探针](2026-11-08-macos-jsc-probe-design.md)已经验证固定脚本回收的基础上，验证不改现有包格式的 ES Module 能否经过构建时转换，在独立 JavaScriptCore 进程得到预期的纯命令结果。本单元只处理受控源码与固定测试调用，不是生产插件加载器，也不开放 Mac 插件。

## 输入与模块图

实现位于 `spikes/macos-jsc-esm-probe/`。选用现有 `demo-return` 的真实 Runtime 验证纯 `onCommand` 输出，用 `translate` 验证真实多文件依赖图及缺少 Web API 的负结果；合成夹具覆盖相对导入、重导出、live binding 和异步返回。原始插件包、Manifest、SDK 和 Windows 宿主均保持原样。

使用仓库已锁定的 parser 与 esbuild，不下载依赖。先将受控目录中的模块读入有界快照：最多 32 个文件、每个 128 KiB、合计 1 MiB；拒绝符号链接、非 JS 文件和包外路径。AST 识别静态 import/export-from；只解析包内相对 JS 模块，不启用默认磁盘或 node_modules resolver。动态 import、import.meta 等本轮不支持的语法明确拒绝，不以 regex 移除模块语法。

打包只使用快照中的内容，输出单个 IIFE。保留模块顶层副作用，不用 tree shaking 或 sideEffects 注解把有副作用的模块删成“成功”。构建过程不 import 或执行被测模块；记录每个输入文件、依赖图、工具版本及输出内容的哈希。这些措施限定固定仓库/合成实验的输入，不能被宣称为生产安装器对并发替换的完整保护。

## 执行与结果归属

生成的 bundle 编译嵌入固定 helper，不为运行器增加任意源码或路径入口。全部包代码执行，包括模块顶层代码，都发生在父进程可终止并回收的 helper 内。沿用前一探针的有界管道、单调截止时间、唯一响应、成功退出和 generation/requestId 校验。

宿主完成通道不暴露为包可调用的全局函数，也不把宿主私有变量和 bundle 拼接在同一词法作用域。原生侧先保留私有 runner，再单独执行 bundle；传入深冻结的固定 invocation 和空 API。没有成功占位的 settings/storage/network 服务；需要这些能力的调用应失败，不计作宿主能力通过。模块 response.requestId 另行检查，不能用它替代父侧请求归属。

真实 `demo-return` 比对完整预期结果，包括中文/emoji 输入和固定日期形成的标题及复制动作数据；输入无边界空白、保留内部空格，仅构造动作数据，不操作剪贴板。`translate` 使用 load-only helper，不调用 onCommand、面板或服务；分别记录模块加载未完成与加载前 TextEncoder 未定义，不从通用失败码推断完整异常根因，也不记为其 Windows-only 权限已在 Mac 支持。

## 验证与下一阶段边界

验证成功与拒绝分开：包内模块语义、只读冻结输入；包外/远程/裸 specifier/符号链接/动态导入/超量输入的构建拒绝；伪造全局完成入口拒绝；顶层死循环真实超时回收。生成证据以固定源码哈希和实际运行日志为准，不增加生产同包双端通过计数。

ESM 严格模式不得因 IIFE 转换丢失：独立 bundle 显式加 strict banner，验证普通函数无绑定 this 为 undefined、未声明变量赋值与冻结输入写入抛错、不产生额外全局变量。不能假定另一个 evaluate 调用中 runner 的 strict 自动作用于包。此处修复保留实际无 banner 红例与添加后的绿例证据。

ESM 转换子集通过不等于任意模块语义完全等价；循环、top-level await、动态模块和 Web API 应按实际测试明确边界。不得把 Node/esbuild 开发时工具链直接视为已决定的生产安装依赖。生产引擎/模块加载、权限 bridge、进程沙箱与资源配额仍需独立实现决策；Panel/Window 的网页媒体隔离仍是另一项阻塞。

## 生产路线候选（仅调研，尚未采用）

后续应评估有公开 ESM loader 的嵌入引擎，例如独立 helper 中的 QuickJS-NG/rquickjs，而不是默认把 Node/esbuild 放入用户安装包。rquickjs 上游当前包装 QuickJS-NG；其 Runtime 提供 resolver/loader、中断及内存限制接口，但自定义 allocator 配置会影响内存上限的有效性，必须锁定具体版本与 feature 后实测，不能仅调用 setter 就宣称配额成立。参照[上游 README](https://github.com/DelSkayn/rquickjs/blob/master/README.md)与[Runtime 源码](https://github.com/DelSkayn/rquickjs/blob/master/core/src/runtime/base.rs)，这些是调研来源，不是本仓库已安装依赖的证据。

该候选仍必须使用包内模块白名单，禁止暴露 std/os、原生库加载或接受外部预编译字节码；父 deadline、kill/wait、有界 IPC 和 OS 进程沙箱仍独立存在。Promise 永不完成、宿主回调阻塞和进程整体内存不等于引擎自身执行中断/堆上限。下一单元若采用此路线，须先验证原样 ESM、相对依赖、循环/live binding、异步返回、真实中断/OOM 和恢复；本轮未安装、下载或接入该引擎，仍不解决 Panel/Window 的 WK 隔离。
