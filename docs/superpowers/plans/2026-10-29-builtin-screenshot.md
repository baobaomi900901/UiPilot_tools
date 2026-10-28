# UiPilot 内置截图实施计划

目标：在主程序提供桌面自由框选、可见窗口选择、预览、复制与 PNG 保存，并接入启动器收藏和可配置全局快捷键。

架构：Rust 持有截图会话和像素，专用 Windows 消息线程管理每显示器原生覆盖层，React 管理独立预览窗口。参考 `rust_uia` 的物理坐标及屏幕采集逻辑，不随产品分发完整 Provider。技术合同以[已确认设计](../specs/2026-10-29-builtin-screenshot-design.md)为准，下文简称“设计”。

## 技术与全局约束

- 沿用现有 Rust/Tauri 2.11.3、windows 0.61.3、png 0.18.1、React 19.2.7、Vitest 4.1.10 与系统保存对话框。只补必要的 Windows feature，不做依赖升级。
- Windows 首版；截图不调用 UIA、PrintWindow、Helper 注入或网络，不提供公共插件 API，不合成 Ctrl+V。
- 遵守 `docs/ui-guidelines.md`；窗口语义、键盘、跨屏、输出与失败行为分别见设计 §4、§6，不能在任务实现中弱化。
- 截图会话锁不得跨越原生窗口、编码、磁盘和剪贴板调用；异步工作带代际身份，返回时再次验证所有权。
- 快捷键沿用现有生命周期操作门和回滚顺序，截图绑定独立于 launcher-only 注册表；不通过伪造插件身份复用命令入口。
- 现有工作区在 `lib.rs`、`lifecycle.rs`、`Cargo.toml`、`package.json`、启动器测试及预览文件等处已有未提交改动。实施前记录最新状态，逐块集成；不批量暂存、覆盖或清理这些改动。
- 设计阶段不编译运行截图。实施使用本会话内执行流程 `executing-plans`；本计划不要求创建新聊天或派发子代理。

## 文件职责与共享实现约定

所有路径相对于仓库根目录。新模块在所属任务落地，任务外不预建空文件。

| 新文件组 | 职责 |
| --- | --- |
| `src-tauri/src/screenshot/{mod,model,geometry,selection,service}.rs` | 数据、几何、输入状态和会话所有权 |
| `src-tauri/src/screenshot/windows/{mod,capture,overlay}.rs` | 桌面采集、窗口候选、原生窗口与资源生命周期 |
| `src-tauri/src/screenshot/{output,window,commands,hotkey}.rs` | 编码输出、宿主窗口、授权接口和专属快捷键 |
| `src-tauri/src/clipboard_image.rs` | 从历史记录模块提取的共享图片剪贴板写入 |
| `src/screenshot-{protocol,core,client}.ts`、`src/screenshot-view.tsx`、`src/screenshot.css` | 严格解析、前端状态、IPC、视图和局部样式 |
| `src/screenshot-{core,view}.test.tsx`、`src/screenshot-protocol.test.ts` | 新前端行为验证 |

补充设计未固定的实现命名，集中定义一次：

- 模式 `ScreenshotMode = region | window`；使用 `PhysicalPoint`、`PhysicalRect`、`MonitorFrame` 描述物理坐标及像素，不使用 CSS 像素 DTO 参与裁剪。
- `SessionId` 使用后端递增的非零整数，在 IPC 中序列化为十进制字符串；重拍另增 `generation`，不能只靠窗口 label 区分旧结果。
- 宿主预览窗口 label 固定为 `screenshot`，一个进程最多一份。重建窗口也更新所有权；关闭失效旧窗口不得关闭新会话。
- 后端接口命名为 `start_screenshot`、`get_screenshot_state`、`read_screenshot_image`、`copy_screenshot`、`save_screenshot`、`retake_screenshot`、`close_screenshot`；快捷键编辑为 `set_screenshot_hotkey`。
- `start_screenshot` 只接受 main；state/image/copy/save/retake/close 只接受当前 screenshot 窗口实例和当前会话。原生快捷键走内部 service，不伪造 WebView 调用。
- 图片优先通过授权后的二进制 IPC 读取，前端为当前结果建立 Blob URL 并在替换/销毁时撤销；避免自定义 URL handler 无法可靠识别调用窗口的问题。需同步检查现有 CSP，只给图片增加必需的 `blob:`。
- 后端事件 `screenshot://state-changed` 只投递给预览窗口。前端先监听，再读取快照；状态带单调 revision，丢弃较旧快照与晚到图片响应。
- 截图拥有的帧、旧结果、新结果、编码缓冲与传输副本纳入统一 512 MiB 预算，分配前检查；超限返回可重试错误并保留已有预览。GDI 和浏览器自身开销单独记录，不把此预算描述为整个进程内存上限。
- 错误按采集失败、布局变化、资源超限、剪贴板占用/写入失败、保存失败、失效会话和无权调用区分。取消是正常结果，不弹失败提示。

## 全局执行与验证规则

每项功能任务执行一次完整 TDD：先增加能暴露合同缺口的测试、确认预期失败，再实现与运行相关测试。每项任务至少形成一个原子提交，修复提交不得混入用户已有改动。依赖任务开始前检查设计符合性和代码质量；同一人执行时进行自审，不以此默认开启子代理。

命令统一从仓库根目录执行：Rust 使用 `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline <filter>`，前端使用 `npm test -- <file...>`。新模块的测试在对应文件的 `#[cfg(test)] mod tests` 内定义，名称与下方筛选器保持一致。无测试被选中不视为通过；离线缺依赖应单独报告，不能删掉测试。

依赖顺序：Task 1 → Task 2 → Task 3；Task 4 依赖 Task 1；Task 5 依赖 Task 3、4；Task 6 → Task 7 → Task 8 → Task 9 顺序推进，其中 Task 6 依赖 Task 5。

自动测试使用假桌面、假原生注册、假焦点和临时文件，不注册真实全局快捷键、不改真实剪贴板。最终人工验证会改变焦点、显示覆盖层并写入测试图片；仅在实际执行该验收且已获对应授权时进行。锁屏、断开显示器、发送粘贴按键或安装程序不由自动测试触发；提供可控测试窗口和清单，真实用户数据不用于像素断言。

### Task 1: 物理坐标、选区与像素预算

**Files:** 新建 `src-tauri/src/screenshot/mod.rs`、`model.rs`、`geometry.rs`、`selection.rs`；修改 `src-tauri/src/lib.rs` 注册模块。

**Dependencies:** 设计 §4.2、§6.1；共享实现约定。

- [ ] 定义模式、帧、矩形和选区 reducer；把窗口候选排序及键盘选择作为纯逻辑输入输出。
- [ ] 实现双向拖动、半开矩形、跨屏交集、间隙补黑和不缩放的帧合成；所有偏移与长度计算使用 checked 运算。
- [ ] 提供可注入预算的分配预检，覆盖保留旧预览时重新采集的峰值，不只限制单张图片。

**Distinct test coverage:** 四方向拖动输出相同区域；负坐标及不等缩放显示器按物理位置拼接；间隙为不透明黑色；空选择不提交；整数溢出与超预算在分配前失败；Space/方向键/Shift/Enter 产生设计要求的坐标。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline screenshot::geometry`；同命令筛选 `screenshot::selection`。

### Task 2: Windows 桌面帧与原生选择层

**Files:** 新建 `src-tauri/src/screenshot/windows/mod.rs`、`capture.rs`、`overlay.rs`；修改 `src-tauri/src/screenshot/mod.rs`、`src-tauri/Cargo.toml`。

**Dependencies:** Task 1；设计 §4.2、§6.1、§6.2。

- [ ] 枚举显示器与可见顶层窗口，采集各屏冻结帧；排除截图自身窗口，窗口候选使用冻结布局而不是再次实时命中桌面。
- [ ] 原生调用封装资源所有者和可替换测试后端；释放 DC、bitmap、选回原 GDI 对象和线程句柄。
- [ ] 在专用消息线程按显示器创建覆盖层，绘制冻结背景、遮罩、选区及像素尺寸；使用主题语义色。统一使用物理鼠标位置，支持跨覆盖窗口拖动。
- [ ] 按设计接入鼠标和键盘；不移动系统鼠标模拟键盘选择，不安装不必要的全局键盘钩子。
- [ ] 接入选择取消、显示配置/DPI变化、会话切换的终止信号；此模块返回结果或取消原因，焦点由上层负责。

**Distinct test coverage:** 注入第二个屏幕采集/窗口创建失败时，第一屏资源仍释放且没有半套覆盖层；纯黑/白帧被接受；选择后布局改变只返回取消；捕获鼠标后异常销毁仍释放捕获；原生对象创建与释放计数配对。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline screenshot::windows`。

### Task 3: 单会话协调、隐藏顺序与焦点恢复

**Files:** 新建 `src-tauri/src/screenshot/service.rs`；修改 `src-tauri/src/screenshot/mod.rs`、`src-tauri/src/lifecycle.rs`、`src-tauri/src/lib.rs`。

**Dependencies:** Task 2；设计 §4.2–4.3、§6.2。

- [ ] 实现设计状态主线、会话代际、重复触发处理和重拍保留旧结果；用 adapter 连接窗口及输出，便于 Task 5 完成真实宿主窗口。
- [ ] 依次完成隐藏、原生可见状态核验、桌面合成同步、采集、覆盖显示；每一阶段失败走统一恢复，不用固定延时替代顺序保证。
- [ ] 选择期间建立生命周期显示门，抑制启动器/插件快捷键打开界面；释放门后不重放过期命令。
- [ ] 复制/保存使用操作租约固定当前结果；关闭请求延迟到输出完成。宿主退出走有界清理，不无限等待普通 UI 消息。

**Distinct test coverage:** Preparing 中再次触发不启动第二次采集；取消 A 后开始 B，A 晚到结果不显示；重拍取消返回旧 Preview；Saving → close → save 完成只清理一次；隐藏/同步失败不显示覆盖层且恢复原来源；恢复前用户已切到第三方窗口则不抢焦点；选择期间插件触发被抑制，结束后不回放；退出时所有租约与原生清理有界收敛。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline screenshot::service`；同命令筛选 `lifecycle::tests::screenshot`，新增测试使用该前缀。

### Task 4: PNG、共享剪贴板与文件提交

**Files:** 新建 `src-tauri/src/clipboard_image.rs`、`src-tauri/src/screenshot/output.rs`；修改 `src-tauri/src/clipboard_history/paste.rs`、`src-tauri/src/screenshot/mod.rs`、`src-tauri/src/lib.rs`。

**Dependencies:** Task 1；设计 §6.3。现有 `atomic_file.rs` 仅作为 Windows 文件提交模式参考，其设置文件 backup 合同不直接套给用户图片。

- [ ] 从历史图片粘贴抽取 PNG/DIBV5/DIB 编码与写入，保留现有调用语义，不引入重复剪贴板实现。
- [ ] 编码完整图片后再写剪贴板，重试有上限且只针对可重试占用；部分格式写入失败返回明确失败，不承诺已清空剪贴板的原内容能原子恢复。
- [ ] 实现同目录临时 PNG、同步和原子提交；已存在目标由系统对话框决定是否覆盖，取消不创建文件。
- [ ] 失败保留结果供重试；不主动新增历史记录，不自动粘贴，不自动保存。

**Distinct test coverage:** PNG 内容和三种剪贴板格式一致；无效图像不会打开剪贴板；占用后成功/耗尽重试/中途格式失败；历史图片行为回归；文件编码/写入/提交失败保留原文件；无目标和覆盖目标成功；路径为中文或含空格；仅清理本操作的临时文件。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline clipboard_image`；同命令筛选 `clipboard_history::paste`、`screenshot::output`。

### Task 5: 授权的预览窗口与二进制资源通道

**Files:** 新建 `src-tauri/src/screenshot/window.rs`、`commands.rs`、`src-tauri/capabilities/screenshot.json`；修改 `src-tauri/src/screenshot/mod.rs`、`src-tauri/src/lib.rs`、`src-tauri/build.rs`、`src-tauri/capabilities/main.json`、`src-tauri/tauri.conf.json`；生成对应 `src-tauri/permissions/autogenerated/*.toml`。

**Dependencies:** Task 3、4；设计 §5、§6.2–6.3；共享实现约定。

- [ ] 注册独立预览窗口与接口；窗口先就绪再投递状态，读资源采用二进制响应，不对插件开放任意图片路径。
- [ ] 所有命令在读取受保护状态前检查实际调用窗口；read/copy/save/retake/close 再校验会话、代际与窗口实例。拒绝前端伪造来源字段。
- [ ] 保存命令在宿主打开 PNG 保存对话框；取消返回 Preview，关闭窗口请求遵守输出租约。仅扩大新窗口所需能力。
- [ ] 连接会话 service 的窗口 adapter，处理预览创建/就绪失败并释放资源；主题来源沿用宿主主题服务。

**Distinct test coverage:** main 可 start 但不可 read/copy；插件/find/伪造 label 对象在资源访问前被拒绝；关闭旧会话后旧 session 请求失败；保存对话框回调晚于会话终止不提交文件；窗口重建后旧 ready/close 事件不影响新会话；就绪快照与推送乱序最终只保留最高 revision。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline screenshot::commands`；同命令筛选 `screenshot::window`。

### Task 6: 宿主预览 UI 与第一条完整截图流程

**Files:** 新建 `src/screenshot-protocol.ts`、`src/screenshot-client.ts`、`src/screenshot-core.ts`、`src/screenshot-view.tsx`、`src/screenshot.css` 及对应测试文件；修改 `src/main.ts`、`src/protocol.ts`、`src/launcher-core.ts`、`src/launcher.test.tsx`、`src-tauri/src/model.rs`、`src-tauri/src/commands.rs`。

**Dependencies:** Task 5；设计 §4.1–4.3、§5；复用 `src/ui-theme.ts` 和现有 WebView 刷新拦截机制。

- [ ] 新增 `/screenshot` 基础结果及执行分支，搜索/补全不产生截图副作用；先闭合默认区域截图路径。
- [ ] 主入口按实际 window label 挂载独立截图视图；图片 client 与业务状态单独成文件，不继续扩张主入口中的业务逻辑。
- [ ] 实现预览、真实尺寸、重新截图、复制和保存、加载/错误/忙碌状态；图片 Blob URL 随资源替换/销毁撤销。
- [ ] 实现 Ctrl+C、Ctrl+S、Esc、Tab、IME 和输出忙碌期间禁用；保存对话框取消保留预览。复用语义 token，保持 44px/48px 宿主几何，不加入图钉。

**Distinct test coverage:** 输入命令不采集，Enter 只执行一次；快照 revision 较旧不覆盖推送；A 图片读取中变成 B，A 返回被忽略并清理其资源；复制后保留图像；保存取消、失败与重试；重拍取消返回原图；忙碌时关闭延迟；按键作用域和可访问名称；两种主题按钮角色一致。

**Verify:** `npm test -- src/screenshot-protocol.test.ts src/screenshot-core.test.tsx src/screenshot-view.test.tsx src/launcher.test.tsx src/webview-reload-guard.test.ts`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline commands::tests::screenshot`，新增启动器路由测试使用该前缀。

### Task 7: 窗口动作菜单、搜索与收藏完整接入

**Files:** 修改 `src-tauri/src/settings.rs`、`model.rs`、`commands.rs`、`lib.rs`、`src/protocol.ts`、`src/launcher-core.ts`、`src/launcher-view.tsx`、`src/launcher.test.tsx`。

**Dependencies:** Task 6；设计 §4.1、§4.2。

- [ ] 补齐“截图 / screenshot”普通搜索、保留命令名与命令补全，避免被公开插件同名入口占用。
- [ ] 增加区域/窗口两种动作路由；窗口动作使用原有结果所有权校验，过期结果不启动新会话。
- [ ] 完成 Rust/TS `BuiltinFeature`、`ResultFavorite::builtin`、`safeResultFavorite`、`favoriteMatchesResult` 和收藏执行链，不只增加 UI 菜单项。

**Distinct test coverage:** 两种动作选择正确模式且只启动一次；收藏设置重启往返保持；右键收藏/取消和收藏列表执行；伪造截图 favorite 配给其他 activation 被拒绝；已隐藏/替换结果的动作请求不启动；内置保留命令与插件命名冲突处理符合既有规则。

**Verify:** `npm test -- src/launcher.test.tsx`；`cargo test --manifest-path src-tauri/Cargo.toml --lib --offline settings::tests`；同命令筛选 `commands::tests::screenshot`。

### Task 8: 专属截图快捷键与设置

**Files:** 新建 `src-tauri/src/screenshot/hotkey.rs`；修改 `src-tauri/src/settings.rs`、`lifecycle.rs`、`command_hotkeys.rs`、`commands.rs`、`lib.rs`、`build.rs`、`src-tauri/capabilities/main.json`、`src/protocol.ts`、`src/main.ts`、`src/launcher-core.ts`、`src/launcher-view.tsx`、`src/launcher.test.tsx`；生成快捷键命令权限。

**Dependencies:** Task 7；设计 §4.1、§6.4；复用 `src/hotkey-recorder-input.tsx`。现有命令快捷键设计的注册串行门与回滚规则见 `docs/superpowers/specs/2026-10-26-command-hotkeys-design.md` 的 Registration, persistence, and locks。

- [ ] 增加默认 null 的持久设置、独立原生注册状态和 active/unavailable/unset 展示；普通设置更新保留该字段。
- [ ] 共用操作门检查主启动器、截图与所有已保存插件命令 chord 的双向冲突，不修改 launcher-only 表用途。
- [ ] 新 chord 注册、旧 chord 卸载、持久化与发布采用受控事务；回滚失败标 unavailable 并禁止派发，保留待清理注册身份。
- [ ] 通用设置增加录制/清除行；录制优先、忽略重复和旧 revision 回调；触发进入截图 service，不先弹出启动器。

**Distinct test coverage:** 截图与插件同时申请同一 chord，由门的顺序只允许一方成功；旧 A → 新 B 注册后持久化失败，恢复 A；恢复 A 失败则 unavailable 且 A/B 均不派发；旧 A 的已排队事件在绑定换成 B 后丢弃；录制期间及其延迟事件不截图；默认不注册，重启恢复，一条注册失败不阻止宿主启动；清除和其他设置保存不影响插件绑定。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline screenshot::hotkey`；同命令筛选 `command_hotkeys::tests`、`lifecycle::tests::screenshot`；`npm test -- src/launcher.test.tsx src/hotkey-recorder.test.ts`。

### Task 9: 发布构建与验收记录

**Files:** 新建 `docs/testing/builtin-screenshot-acceptance.md`；必要修复仅进入上述任务拥有的文件，不扩大功能范围。

**Dependencies:** Task 8；设计 §8 全部验收项。

- [ ] 运行全局检查并保存命令和结果；修复由本次变更引入的失败，对既有失败单独定位和记录。
- [ ] 构建实际 Release 可执行文件，记录构建命令、产物路径与版本，不把普通 Cargo 编译当成完整 Tauri 资产打包验收。
- [ ] 在可用设备验证跨窗口/跨屏、负坐标、混合 DPI、纯色内容、透明窗口、典型应用粘贴、主题与键盘；硬件不具备的项目明确记为未验证。
- [ ] 检查 50 次截图关闭后的内存/GDI 句柄趋势，及取消、布局变化、输出失败和退出无残留；实际安装、锁屏、断屏和粘贴发送由已授权的人机操作完成。
- [ ] 验收记录区分自动通过、人工通过、未验证及已知限制，附截图或测量依据，不直接复制设计预期为测试结果。

**Distinct test coverage:** 本任务承担实际 GDI/显示驱动/桌面合成、剪贴板目标应用及安装产物差异；不以假后端测试代替这些结果。

**Verify:** 从根目录执行 `npm run verify:local`、`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`git diff --check`；构建执行 `npm run tauri -- build --no-bundle`。若需要安装包，再执行 `npm run tauri -- build` 并单列安装验收，构建本身不自动安装。

## 交付检查

- [ ] 设计 §4 的交互全部在 Task 6–8 接入，不遗漏窗口选择、收藏和重拍取消。
- [ ] 设计 §5–6 的授权、顺序、代际、取消、回滚和输出均有实际测试，而非仅有结构或源码字符串断言。
- [ ] 设计 §8 的人工矩阵有逐项结果，未验证硬件/系统场景明确保留。
- [ ] 前端、Rust、权限生成、Release 构建通过；没有引入 Provider EXE、公共插件 API 或未经请求的截图扩展。
- [ ] 用户已有改动保持完整，提交仅包含本功能；最终报告提供可执行文件及验收记录，并如实标明尚未完成的验证。
