# 授权文本粘贴到原应用（Windows Host 0.14.0）

## 目标、授权与方案

总目标是本地插件基座。用户已授权自主规划、agent开发、评审与本地提交，并在标准UI核心验收后对“授权粘贴到前台应用”回复继续；本轮明确为Host能力开发，覆盖宿主/API/CLI及现有文本片段插件，而非单独插件开发越界。继续原worktree，不动D主目录源码。真实系统粘贴、剪贴板及窗口交互全部由用户人工执行。

选择扩展现有不透明结果动作：增加pasteText，沿用整快照授权与一次消费。相较开放Runtime直接输入API，此方案自然绑定用户选择和当前结果；相较借用clipboard.history.paste，不要求插件获得历史读取权限。保留copy默认动作，paste只作为用户主动选择的次级动作。无在线功能、依赖安装、任意按键、目标路径/HWND输入、Panel/Window新桥或历史剪贴板流程重构。

## Manifest、版本及精确合同

Host版本0.14.0；API/schema仍v1。新增权限clipboard.paste，只允许supportedPlatforms恰为[windows]、minimumHostVersion>=0.14.0、runtime.resultActions=true（既有规则限定mainResult），并必须声明clipboard.write。它与历史clipboard.history.paste不同，不互相授权。Host与CLI、生成schema/validator、TypeScript、设置授权列表一致。权限名称“粘贴到原应用”，说明会写入剪贴板并向唤起UiPilot前的应用发送一次粘贴。升级新增权限取消保留旧插件和数据。

动作严格为{id,title,type:"pasteText",text:string}，既有id/title/action数量/整响应大小限制保持。text限1..8192 UTF8字节，拒绝NUL，保持Unicode、换行、空白，不trim。整个无效响应拒绝，不发布部分动作。动作仍可被其他插件显式设为默认；本参考插件默认copy。不存在自动查询或Runtime效果。legacy defaultAction仍仅copy。

完整快照requirements增加paste，包含paste时必须同时验证clipboard.paste与clipboard.write的声明和当前授权；初始发布、所有兄弟动作的执行、Runtime返回新快照均遵守完整requirements。text只在后端快照，不进入公共action DTO/IPC参数。现有execute_result_action仍只收不透明request/result/action ID且限main caller。

增加结果{kind:"pasteSent"}，含义为Windows接受一次输入批次，不声称目标已插入。增加稳定错误pasteTargetUnavailable、pasteInputBusy、pasteFailed，复用clipboardWriteFailed。Frontend严格解析并保留owner保护；前置失败sourceConsumed=false（旧已有inProgress/consumed例外保持），claim后所有结果sourceConsumed=true。成功文案“已请求粘贴”；无目标“无法返回原应用，请从目标应用重新唤起后重试。”；按键未释放“请松开按键后重新执行。”；执行失败“粘贴未确认，请检查目标应用后重新运行命令。”不自动显示窗口抢焦点或重试。正常hide事件仍清空结果，迟到outcome不恢复旧UI。

## 目标归属与执行顺序

新增result paste专用小模块，复用Lifecycle原有外部捕获与ExplicitReturn隐藏/恢复，不改历史paste调度语义。新增内部lease保存当前foreground_capture状态show_generation与capture的完整身份（HWND/PID及原capture generation），独立于旧ClipboardPasteTarget.show_generation。visible UiPilot repeat递增state generation但保留旧capture的情况必须正确：新lease有效、旧lease失效。lease不序列化给插件。Hide后capture被take，故最终发送使用绑定target与当前show generation；不得以capture存在作为发送条件。

1. main命令解析、resolve后捕获当前lease/target；无有效目标则前置失败。为允许Enter/Space选动作后释放触发键，最多500ms在非UI线程每10ms检查Ctrl/Shift/Alt/左右Win/Enter/Space是否释放。只读按键高位，不保持任何管理锁，不写剪贴板、不隐藏、不发键；超时返回pasteInputBusy。这是一次请求的前置等待，不是重试副作用。
2. 完成等待后排入一个run_on_main_thread闭包，async caller通过channel等待typed outcome，不能在UI线程等待另一个UI任务。闭包执行时重查main可见且GetForegroundWindow精确等于main HWND、lease当前、目标仍是原PID/有效非本进程外部窗口、触发/修饰键均已释放；对不满足者零副作用失败。
3. 在legacy admission read → public mutation → scheduler → registry既有顺序下重新验证完整owner/grants/publication/lease并claim。scheduler/registry仅包claim，之后释放它们；在public mutation与legacy read仍持有时写入固定文本，并取得这次写入完成对应的非零clipboard sequence。不能把任意复制发生后的序列误认成本次写入：原生写入以当前main HWND调用OpenClipboard（不得传NULL），在guard内完成Empty/SetCF_UNICODETEXT及获取sequence，释放剪贴板后返回；UTF16终止符及GlobalAlloc由RAII持有，SetClipboardData成功才转移内存所有权，失败释放；无原剪贴板读取/备份/恢复。
4. **必须释放全部legacy/public/scheduler/registry管理锁后**，同一主线程闭包执行clear_and_hide_window(...ExplicitReturn)，恢复绑定原目标。此函数会清理registry和重新进入manager，绝不放在持锁effect回调里。claim后的自身hide使结果lease过期是正常消费，不得用该过期取消自身后续发送。闭包不sleep、不重试，不拆出第二个排队hide任务。
5. native发送helper内最后再检查：show generation未变、目标IsWindow/PID非本进程且GetForegroundWindow精确相同、键已释放、clipboard sequence仍等于本次写入。任何失配立即失败不发键。一个SendInput批次仅Ctrl-down/V-down/V-up/Ctrl-up；检查返回事件数量，不补发、不合成释放用户按住的键。部分成功也作为不确定失败并消费。不得复用忽略target的旧history helper作为安全检查。

主线程序列化防止UiPilot自己的新show在claim/write与hide/send中插入，但Windows外部进程仍可改变前台/剪贴板；采用发送前最终检查，不承诺OS原子目标锁或所有应用支持。目标关闭、管理员完整性级别/安全桌面、不可编辑焦点可能不能粘贴；不提权或回退到其他窗口。剪贴板写入成功后即使后续失败也保留本次文本，不覆盖后来用户复制的内容。

副作用提交点为claim后第一次写剪贴板。提交前撤权/重载/新query/旧show应零写入；提交后允许当前固定副作用流程完成，永不把权限或目标转给新来源。当前窗口失焦或新show发生在前置等待/排队期间必须在主线程前置检查阻止。多个并发请求由现有快照claim串行消费，至多一个执行；不得持锁等待键释放或主线程。

## 参考插件与文档

com.uipilot.text-snippet升级1.2.0，minimumHost0.14.0，新增clipboard.paste（保留copy/url权限、命令ID、设置和snippet.latest存储）。非空原文、大写、最新片段结果加入“粘贴到原应用”；空保存片段保持复制/读取，无paste动作。默认仍copy；保存、网址打开和重读保持原流程。响应容量继续整体检查，增加text副本不可绕过64KiB预算。SDK教程清楚区分复制/粘贴、明确选择、原窗口目标、一次消费和剪贴板保留；不承诺自动保存目标文档。

## 验证与交付

聚焦mock检查：Manifest Host/CLI等价；边界/多字节/NUL/空值拒绝；整快照授权和Runtime replacement；旧owner/grant撤销零效果；重复调用与write失败消费；异步等待→新show/新query/失焦/撤权零write；正常主线程先claim/write、退出所有锁后hide、再send；隐藏/恢复/目标或sequence变化/按键重新按下/partial SendInput不重试且消费；自有hide正常完成；capture generation保留与state generation改变；frontend老结果不恢复UI，旧copy/url/runtime不回归。纯driver测试不得触碰真实剪贴板、按键、窗口。必要类型检查/schema freshness、既有本地CLI打包静态验证。一次源码冻结审查后构建，不机械全仓verify。

交付Host0.14.0（现有acceptance identifier）+文本片段1.2.0，记录源码/前端资源输入与PE版本/hash，保留旧0.13和模板包。人工分组：版本与新增权限取消/确认；从普通记事本唤起，菜单显式粘贴中文一次；重新唤起换目标、快捷键入口、无目标/目标关闭、旧复制/保存/读取/URL短回归。代理不启动程序或执行真实粘贴。完成本切片不等于完整M5/基座。

## 原生行为依据

[Microsoft SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)：返回插入事件数，受UIPI和现有键状态影响，不能等同目标完成编辑。[GetAsyncKeyState](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate)：仅高位判断当前按下状态。[GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)：序列随内容改变，零代表无法取得；不读取内容。

[Microsoft OpenClipboard](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard)：NULL owner配合EmptyClipboard会使SetClipboardData失败，因此传入宿主main HWND。

