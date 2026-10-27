# Windows 可视化打包工具

日期2026-10-27。用户已选择可双击启动的Windows桌面窗口：Setup/插件包两个页签、选择输出目录、实时日志、打开产物目录。复用既有构建链，不改变UiPilot运行时或插件合同。

## 方案
采用Windows PowerShell5.1 + WinForms，根目录Pack.cmd启动STA GUI；不新增Python、浏览器服务、npm或.NET包依赖。相比本地网页省去服务器/端口，相比新增Tauri工具省去独立应用构建。当前工作树已有依赖，保持D主目录未提交内容不变。使用原生可访问控件、布局容器、系统字体和颜色、Tab顺序、可调整窗口；日志可滚动复制。此工具独立于宿主，不注入启动器。

## 操作
仓库根由脚本位置决定并显示只读路径及版本。Setup页显示生产Release/Windows x64/NSIS Setup；插件页扫描examples/public-plugins/*/package供选择，并允许浏览外部含plugin.json的package目录，显示插件ID及版本。公共输出目录默认仓库artifacts/packages，可浏览修改。按钮：开始打包、取消、打开输出目录。状态为就绪/预检/构建阶段/成功/失败/取消，耗时可见；不编造百分比。日志保留每次运行目录。

只允许一个任务，运行中锁定配置/开始，取消仅终止本工具拥有的worker进程树，不能按名称杀死其他node/cargo。窗口运行中关闭应先阻止并引导取消，终止确认后才允许退出；取消不显示成功，不删除已存在文件。出错后可调整配置重试。

## 执行边界
纯函数/核心脚本负责预检、参数、路径和产物识别；独立worker执行命令和写结果，GUI仅管理任务与显示。通过UTF8 JSON传递请求和结果，不将用户路径拼接进可执行PowerShell表达式；子进程参数正确引用，支持中文、空格、&等路径。脚本含中文时使用UTF8 BOM以兼容Windows PowerShell5.1，cmd保持ASCII。后台进程隐藏控制台。

输出根必须在插件源目录之外；每次创建唯一子目录，产物和日志互不覆盖；拒绝输出重解析点/无效路径并给明确提示。完成只基于子进程exit0、当前运行预期产物存在、归档校验（插件）、SHA256记录，不把旧文件或某条日志当成功。失败和取消保留诊断日志，不自动重试或自动安装产物。

## Setup链路
复用npm run tauri -- build --bundles nsis，正常执行既有beforeBuildCommand/frontend+production Everything helper与beforeBundleCommand暂存钩子。禁止VmInstrumented、fault-injection、acceptance标识覆盖。只构建不运行setup、不修改服务、不提权。
预检Windows x64、node/npm、cargo、node_modules及本地Tauri CLI、必要配置/Everything资源。不自动安装依赖，缺失时显示npm ci等修复说明。清除worker进程内继承的CARGO_TARGET_DIR、CARGO_BUILD_TARGET、TAURI_CONFIG等会改变约定输出/身份的构建覆盖，或明确拒绝这些覆盖，不能改变用户全局环境。
预期构建目录src-tauri/target/release/bundle/nsis。构建前记录状态/开始时间；成功后只接受本次生成且与当前版本匹配的生产*-setup.exe，不挑目录中第一个exe，不接受旧版本/VM产物。复制到本次唯一输出目录，保留原始名称和SHA256证据。x64平台默认工具链约束需明确检查；不承诺签名或VM安装验收。首次打包可能需要工具链下载依赖，应在UI说明而非自动装依赖。

## 插件链路
读取所选package/plugin.json仅作展示/文件名；manifest最终由现有CLI验证。生产文件名pluginId-version.uipilot-plugin须校验为安全文件名，不能由任意字符串逃逸路径。先构建当前仓库插件CLI（npm run build --workspace @uipilot/plugin-cli），随后依次node CLI validate源目录、pack到本次输出目录、validate归档，--platform windows --json。所有命令失败立即中止，不运行插件代码。遵守CLI现有不覆盖和普通目录规则，不用旧demo压缩脚本绕过校验。

## 验证与交付
聚焦PowerShell测试采用临时目录及fake runner验证：Setup固定命令/生产模式，plugin顺序与失败短路，缺失依赖、路径/版本错误、中文空格特殊字符、旧Setup误报防护、产物hash、取消状态不会成功。PS5.1语法解析与UI wiring静态检查；真实无UI插件CLI smoke允许，但agent不启动GUI、不跑昂贵Setup构建或安装。
交付用户双击入口和三项人工用例：窗口/预检、已知插件打包安装归档校验、Setup点击构建及产物（只生成不运行）。Setup实际生成和GUI交互待用户确认，明确区分模拟与原生证据。