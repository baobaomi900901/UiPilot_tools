# 截图元素探测调整设计（已批准）

用户于 2026-10-29 批准执行本设计及对话中的交互、构建补充。本设计替代原截图设计中不依赖 Provider、默认进入即冻结及纯区域选择的条款，其余输出、权限、预算和生命周期合同继续有效。

## 1. 边界

rust_uia 保持独立仓库，新增轻量 probe.point JSON-RPC 方法；UiPilot 仅通过独立命名管道调用编译后的 EXE，不复制 UIA 实现。Provider 返回几何，宿主管理遮罩、输入、桌面取帧及已有预览/输出。首版探测只走 UIA，不注入 Helper、不 Attach Java、不连接浏览器扩展，不写元素库或截图。

## 2. 探测协议

ping 能力 point_probe_v1=true。probe.point params 为 {x:i32,y:i32,exclude_process_ids:[u32],exclude_hwnds:[decimal string],timeout_ms:u32}，默认150ms，合法范围1..1000；排除列表各最多128。响应为 {status:hit|no_hit|timeout|unavailable,coordinate_space:screen_physical,rect?:{x,y,width,height},root_hwnd?:decimal string,process_id?:u32,backend?:uia}。hit 才有正面积 rect，包含查询点；矩形采用半开物理坐标。JSON-RPC id 匹配请求。非法参数返回 -32602。

排除宿主和 Provider HWND/PID 参与原生目标窗口命中，不能只过滤最终结果。依据原生 Z 顺序选择该点可见目标窗口，再从其 UIA 根下钻，不能依赖覆盖层上全局 ElementFromPoint。只返回可靠可访问性目标，不承诺视觉像素分割或所有应用的绝对最小控件。过期、超时、歧义或跨窗口结果不能冒充命中。单次 UIA 调用不能强制终止，预算在调用间检查，宿主进程隔离提供硬超时。

## 3. 交互与取帧

默认区域模式升级为智能截图。悬停实时探测；命中后所有显示器绘制50%黑色遮罩，元素矩形内部透明、边界高亮并显示真实像素尺寸。遮罩与边框共用矩形；镂空不允许输入穿透到底层。无命中清除过期元素；窗口回退必须显式为窗口候选，不冒充元素。Provider故障可显示窗口候选/提示并继续自由拖框。保留独立窗口截图入口。

鼠标按下锁定当前显示候选、起点和取帧；移除装饰并等待桌面合成生效，取原始桌面帧后显示冻结选择层。悬停期间不能使用进入时旧帧冒充实时画面。鼠标和取消输入在取帧切换期间仍由截图会话拥有，不激活底层控件。输入队列按顺序处理。显示变化或捕获丢失取消。

系统拖动容差按按下显示器 DPI 换算物理坐标。未越界的抖动松开后选择按下时元素；越界永久切到拖框，以原始按下点为锚，跨窗口跨屏任意方向；回到原点不切回单击。零面积拖框不提交，恢复悬停；无候选单击不提交。Esc取消。重拍取消保留旧图。空闲Enter选择当前候选，Space设自由框起点，方向键延续物理像素选择。

## 4. 宿主通信

专用后台工作线程持有本会话 Provider 子进程，启动参数唯一pipe和parent-pid，检查ping能力；退出或取消仅关闭本会话进程。主UI/原生消息循环不等待管道。一次最多一个在途请求，等待期间只保留最新坐标。响应按会话、序号、位置和当前阶段核验；按下/拖动/取消后晚到响应不能更新候选。至少50ms节流，目标150ms软预算，单次管道操作最多1000ms硬超时。失败停用本次识别，保留自由选择，不自动无限重启。读取有长度上限。Release仅从安装资源加载，debug允许脚本提供暂存EXE路径。复用原有权限、预算和输出会话检查。

## 5. 编译物、dev与Setup

新增 D:/code/.tools/dev-uipilot.py，参考同目录build-test.py。薄入口调用UiPilot仓库内构建/开发工具；路径参数和本地配置确定两个仓库，默认当前已指定目录。每次启动在rust_uia目录运行cargo build --release --locked（目标x86_64-pc-windows-msvc与宿主匹配），利用Cargo缓存；开发允许未提交修改并记录dirty。不得自动拉取/切分支/清理用户改动。只复制EXE和清单到生成暂存目录，失败不得回退旧EXE。

构建共享模块生成commit、dirty、target、构建参数、size、SHA256和ping能力清单；并发构建使用互斥/独立暂存避免EXE与清单混配。开发脚本构建核验后启动现有npm run tauri -- dev，保留Everything、端口配置；退出只清理拥有的进程。

Setup同样从独立rust_uia源码构建，通过Tauri构建钩子覆盖package.py和项目包装器入口 `npm run tauri -- build --bundles nsis`（对话中“直接tauri build”指此入口）。`npm run tauri -- bundle --bundles nsis` 同样在持锁会话内准备一次；绕过项目包装器直接运行 node_modules CLI、npx 或 cargo tauri 不属于支持入口，钩子在缺少持锁会话时拒绝执行，避免资源并发替换；一次构建准备产物，打包前校验同一份。resources显式包含EXE与清单，安装后只依赖资源目录，不需Python/Rust/开发路径。源码缺失、能力不符、构建失败阻断发布。保留既有Everything构建/NSIS逻辑。打包日志增加Provider阶段。安装升级/卸载由父进程退出及进程清理保障资源可替换，不能结束别的Provider。

## 6. 验收

协议参数/负坐标/最小节点/排除遮罩/超时；手势抖动、阈值越界后返回起点、空选区；晚到/错序响应；Provider缺失/崩溃仍可拖框；混合DPI及跨屏镂空；取帧无黑罩边框；点击不触发底层；重拍取消与资源释放。构建失败不复用旧EXE、清单一致性、dev调用链、Setup资源完整性及全库相关回归。真实桌面验收与自动测试分别报告；不自动安装Setup或干扰用户当前应用。
