# macOS 截图与 mac_acc 协作边界

2026-11-16，用户指定截图使用 `/Users/mobytang/Documents/RengarTang/code/mac_acc` 的能力，并授权发 issue 要求接口和打包配合。本轮完成代码能力核对及协作请求；以下为消费方设计方向，新增 Provider 协议须在对方确认后冻结，不代表截图已开放。

## 已核对事实与接口缺口

mac_acc `b561e804e00b3306e144383fb9baf7cdfdc79719`：Swift AXBackend + Rust core，现有 `mac-acc --stdio` 逐行协议 v1 / JSON-RPC 2.0 封包，支持 `system.ping`、`permission.status/request`、app/window/tree、element.at_point 与 probe.confirm/cancel。bounds 使用 `ax_screen_points`，全局左上 point、有限浮点、允许负坐标。已有帧/队列上限、deadline、权限 epoch 和 EOF 清理。

`ProbeNative.hit` 是 system-wide 按点 AX 查询，自身排除仅为 Provider PID；不能据此证明在 UiPilot 不透明遮罩上能命中底层应用。现有预览属性还有标题等截图不需要的信息。mac_acc 明确无屏幕采集、OCR 或自动点击回退，无录屏权限需求；本项目不把它记为现成截图引擎。

Windows 对照以[元素识别](2026-10-29-screenshot-element-probe-design.md)最终版本为准：Provider 仅负责公开控件几何，宿主管理冻结帧与选择输出；不照搬被废弃的 Windows point_probe 协议。标注、调整、直存与贴图按后续[标注](2026-10-29-screenshot-annotations-design.md)、[选区调整](2026-10-29-screenshot-resize-desktop-save-design.md)、[工具栏和贴图](2026-10-29-screenshot-toolbar-pin-design.md)合同逐项对齐，不以最早首版的“不含标注”缩减最终范围。

## 双方分工

UiPilot：显式截图触发及录屏授权、隐藏当前启动器并确认合成时序、冻结显示器帧与布局、窗口候选、完整输入遮罩、AX/AppKit/各屏图像坐标映射、拖框/编辑/标注/复制/PNG 保存及会话所有权。桌面帧候选后端为系统屏幕采集能力，具体 API/系统权限和排除本应用语义在实现前依据本地 SDK 核对，不在本任务调用真实录屏。mac_acc 不替代 UiPilot 的录屏授权。

mac_acc：只读 AX、绑定真实 app 启动身份及窗口的局部 geometry-only 命中、可见裁剪/完整性标记、权限/超时错误、无 GUI Provider 制品。禁止控件动作、标题正文采集、隐藏截图遮罩、激活目标窗口或把 AX 失败回退为 OCR。

主线程不等待 Provider；一次只允许一个在途探测，保留最新待查点。UiPilot 维护 screenshot generation、layout revision、query sequence、target app/window 身份和选择阶段。按下锁定已显示候选，拖动/取消/显示器变化撤销所有在途结果；发布前重验来源和几何归属。AX 采样与冻结图非原子快照，不能承诺动态控件绝对正确；失败、歧义、超时或无权限时退回明确标注的窗口候选/自由拖框。

## 请求的 Provider 合同（初始提案，以下方回传决定为准）

建议单独 capability `screenshot_probe_v1` + `screenshot.probe`，复用 stdio 框架和错误。参数为不透明 request_tag、当前 target app/window_ref、AX point、排除 PID 和有界 budget；响应仅包含回显 tag、复验身份、有效 bounds、可见性/完整性与固定错误。不返回 value/title/description/完整树，不提供任意路径或原生指针。目标窗口必须唯一对应宿主可见窗口候选；不能按标题猜测或使用裸 PID 跨重启定位。

重点要求遮罩存在时仍能限定到目标窗口，只过滤返回的 Provider 自身 PID 不能满足。局部查询或几何遍历具体实现由 mac_acc 评估；若不支持必须明确说不可用，宿主不开启该吸附能力，不在 UiPilot 重写 AX 树遍历。沿用 point 坐标，禁止把不同 scale 的各屏像素拼作一个无定义的全局物理坐标；跨屏裁图、像素预算和输出尺寸由宿主采用每屏映射/确定的合成策略。

现有规范 `element.at_point` 默认 300ms，不证明新接口延迟已经达标；新 budget 包含排队、服务端钳制，AX 卡死由私有进程超时回收兜底。禁止无限重试。撤权立即失效旧引用，重新授权不复活旧结果。

## 打包、权限和安全门槛

请求独立 arm64 Provider 包及依赖/许可证、版本化协议/schema、source commit/dirty、target/minOS、逐文件 size/SHA256、能力与签名/公证状态。无开发路径、构建工具运行依赖或 GUI/Fixture 强绑定。UiPilot Release 只从固定 bundle 资源定位和校验，不从 PATH 找替代可执行文件；开发路径必须显式指定。资源准备使用新暂存并原子发布，失败不沿用旧二进制。

部署拓扑与 TCC 主体尚待 mac_acc 回传：普通包内 CLI 或嵌套 helper .app 哪种可支持，权限属于 UiPilot/helper/终端及升级后行为必须实证。无证书先交协议、fake tests、ad-hoc 内部包；身份稳定性/签名公证/真实嵌入权限列为生产门槛。不能用已授权 Terminal 的结果当作 UiPilot 嵌入已授权。

每截图会话只拥有自己启动的子进程和 stdio 管道，EOF/断管有界退出；挂起时仅终止自己创建且身份匹配的子进程，禁止 killall 或结束用户独立 Mac ACC。业务/日志不输出图片、正文或凭据，permission.status/启动不得请求授权。GUI 和系统权限操作一律由用户执行。

## 跨项目问题与状态

- [mac_acc #1：目标窗口约束的 geometry-only AX 探测](https://github.com/baobaomi900901/mac_acc/issues/1)。包含 DTO 提案、遮罩/坐标/预算/错误及验收要求。
- [mac_acc #2：Provider 制品、TCC 与进程生命周期](https://github.com/baobaomi900901/mac_acc/issues/2)。依赖 #1，包含打包清单、嵌入拓扑及干净环境 smoke。

两项已由本会话按用户授权创建；mac_acc 本地工作区未改，未运行其 GUI、授权或屏幕读取。当前只证明请求已提交，最终接口和制品待回传；不能把 issue 创建记作截图接线通过。

## 可先推进与验收

UiPilot 可独立做显示器快照模型/坐标转换、冻结/选区状态机、旧结果拒绝、输出复用、Provider fake transport 与损坏制品拒绝。消费端真实协议/资源接线需等 #1/#2 冻结；开发者证书不是上述纯开发的前置条件。Mac 截图能力快照和入口保持未开放，直到后端及负面测试通过。截图作为可复用内置入口开放时须同时贯通收藏、启停、快捷键和设置管理。

人工仅在可运行候选完成后集中安排：触发授权/冻结→单击吸附/自由拖框→编辑/复制/保存，加混合 scale/负坐标与取消必要检查；AX 拒绝仍可自由选择，录屏拒绝不能继续取帧。旧应用搜索/快速链接无受影响变更时不重复。main 暂不合入 Mac 的限制保持，截图开发可以在当前分支独立推进。


## 2026-11-16 对方回传与消费方决定

已读取 [#1 功能设计回复](https://github.com/baobaomi900901/mac_acc/issues/1#issuecomment-6909080175)、[#2 制品设计回复](https://github.com/baobaomi900901/mac_acc/issues/2#issuecomment-6909189636) 及其本地未跟踪 `docs/UIPILOT_SCREENSHOT_INTEGRATION_PLAN.md`；未修改/提交对方工作区。对方代码基线仍为 b561e80，回复明确只是方案：新方法、schema、Provider-only 包、嵌入 TCC 尚未实现/交付。不能执行尚不存在的 package-provider.py 或把建议 0.2.0 记为实际版本。

消费方接受以下首轮取舍，仍须 G0 实证再冻结最终接口：

1. `app_hit_test` 是系统 API 的一次命中，不要求完整遍历的最小面积控件。优先验证应用级查询+窗口/祖先归属，不同时预建两套策略；不可用再讨论有界树。
2. 可接受 `unique_geometry` 的弱关联，但同一启动实例、CG 与完整 AX 窗口目录双向唯一、几何未变化、无竞争/遮挡是采用条件。目录不完整、同几何多窗或身份变化直接降级；不通过标题、nearest/fuzzy、私有 WindowNumber 消歧，不把它称为精确 CGWindowID 证明。误差规则必须在 G0 显式冻结，不靠任意容差通过。
3. 冻结图与实时 AX 非原子；request_tag 只关联宿主请求，已知变化撤销候选、始终保留自由拖框。
4. capabilities 是方法名数组（新增 `screenshot.windows` / `screenshot.probe`），扩展版本/策略限制在 `features.screenshot_probe_v1`。初始建议的“独立 capability 字符串”不能作为方法数组元素消费。截图目录避免 `element.read`，探测不创建长期 probe ref。

仍需对方提供可解析请求/响应 schema（尤其窗口目录完整性、候选/非候选状态、关联强度和预算）及 G0 最小开发制品。当前自然语言不足以冻结严格 DTO；不猜字段写死生产客户端，不从旧接口推断新能力。

### 制品布局与许可

首轮普通 CLI 放 `Contents/Helpers/mac-acc-provider`，协议/manifest 文档放 Resources；先验证一种拓扑，真实嵌入证据证明需要时才改 helper.app。分别记录上游哈希和消费方重签名后哈希。TCC 归属、长期授权与生产升级仍未知；没有 Developer ID 不阻碍协议、fake tests 和 ad-hoc 内部验证。

对方指出当前仓库没有明确 LICENSE。许可由权利人决定，UiPilot 不自行赋予 MIT 或再分发权；对方需明确内部协作包范围和正式 LICENSE/NOTICE。未明确前，源码阅读/fake/宿主验证器准备可继续，正式再分发不推进。完整截图功能不因这一项被记作完成。

### G0 双方交付与最小实机点

mac_acc：最小 geometry-only 开发入口及 schema/manifest 示例，精确说明已实现能力；无 UI/无权限提示的 version/ping/status smoke。UiPilot：从实际 Mac 宿主构建隔离 G0 验证器，不改默认 capability，不用终端直接启动的 trusted=true 代替嵌入证据。验证器显式由用户启动，展示普通 CLI 权限检查和受控不透明拦截遮罩，保留取消入口，输出仅脱敏的身份/几何/阶段，不记录窗口正文或图像。实际遮罩、跨窗和 TCC 操作到可运行包完成后再安排；现在无需 GUI 测试。

默认子进程收尾预算为关闭 stdin 后最多 3 秒，再终止自有 Child、最多 1 秒后强制并 wait；与 Provider EOF flush 的 3 秒同界可能走兜底，需进程测试后冻结，不能声称总会优雅退出。不得让 AX 挂起阻塞宿主 UI，不扫描裸 PID 或结束独立 Mac ACC。

本轮回传解除了设计取舍等待，未解除 G0、最终 schema、制品、许可及 TCC 的待验证项。主线不合入 macOS 分支的要求保持。

消费方已回写 [#1 取舍与 G0 材料确认](https://github.com/baobaomi900901/mac_acc/issues/1#issuecomment-6910138640)、[#2 制品/TCC 反馈](https://github.com/baobaomi900901/mac_acc/issues/2#issuecomment-6910094960)。最终 schema 和实现状态仍以上述未验收边界为准。
