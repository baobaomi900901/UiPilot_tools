# Window Runtime 请求与离线 JSON 工作台设计

日期：2026-10-25。基线 c2cca90；用户已授权自主规划/agent开发并回复继续推进子窗口Runtime交互。目标交付Host0.9.0与可本地安装的JSON工作台1.0.0，用户执行原生人工验收。范围仅Windows、本地插件；不包含在线发布、通用动作菜单、逐命令快捷键或UI组件库。

## 方案与结果

采用window.runtimeActions显式选择加入，window.uipilotPluginWindow.request调用既有Runtime的onWindowAction。共享现有Panel请求的有界JSON、物理运行槽、权限和超时机制，Window保持独立的会话所有权。把按钮编码成onCommand输入会混淆启动器与窗口事务；另开worker/任意invoke桥会重复权限边界，两者均不采用。

参照2026-10-24-panel-runtime-actions-design.md的Public contract、Cancellation and authority proof，保留Panel公开API与错误名。允许提取窄的公共内容动作类型/工具以避免复制调度算法；不做无关重构，不伪造PanelSessionIdentity或启动器提交所有者。

## 公开合同

新增可选window.runtimeActions:boolean；缺省/false保持旧行为，null和其他类型拒绝。true只允许Windows、旧command的submit/window、window配置及ui.window权限，minimumHostVersion至少0.9.0。commands数组仍不支持window。Runtime readiness在opt-in时要求函数导出onWindowAction，缺失时不得发布候选包。CLI静态验证manifest，不执行Runtime。

SDK新增PluginWindowRequest {action:string,payload:JsonValue}；PluginWindowActionInvocation {apiVersion:1,requestId:string,action:string,payload:JsonValue,context:{platform:'windows',theme:'dark'|'light',invokedAt:string}}；PluginWindowActionResponse {requestId:string,data:JsonValue}；PluginWindowActionHandler及RuntimeModule可选onWindowAction。UiPilotPluginWindowApiV1增加只读可选request(input):Promise<JsonValue>，仅opt-in存在，返回深冻结data。API版本、state schema和旧最低版本不变。Host及CLI target同步0.9.0，CLI npm包版本不必改。

请求/响应的JSON规则、动作名称、64KiB完整消息上限、最大深度32/值4096与Panel严格相同：动作^[a-z][a-z0-9.-]{0,63}$，拒绝__proto__/prototype/constructor；payload必填可null；只允许精确顶层字段；无getter/toJSON执行的快照；拒绝非有限数字、循环、稀疏数组、symbol/function/undefined/BigInt、危险record键。Host独立复验，响应requestId必须完全匹配。公共请求不携带pluginId、commandId或任何session/epoch选择器。

固定错误名：InvalidWindowRequestError、WindowRequestBusyError、ExpiredWindowSessionError、WindowRequestCancelledError、WindowRequestTimeoutError、InvalidWindowResponseError、WindowRequestFailedError、WindowRequestUnavailableError。异常不泄露插件堆栈/原生错误；业务校验以正常data返回。一个窗口仅一个未决桥调用，一个插件仍仅一个物理Runtime槽；不排队、不自动重试、不接受AbortSignal、不流式响应。未选择加入的旧窗口无request属性。

## 会话、线性化与生命周期

Window有既存window owner（generation/activation/admission/request）和每次submit递增的timer session generation；动作身份同时绑定内容WebView label、完整owner及该generation。不把长运行action计入阻塞隐藏的timer in_flight租约。可复用数字作为会话身份，但动作注册/撤销与timer功能分离，不能改变番茄钟后台计时语义。

仅Visible且会话Active且无待ACK/update时允许请求；Prepared/Awaiting*为Busy，关闭/隐藏/撤销或owner丢失为Expired。不要在onUpdate中await request：ACK完成前请求应立即Busy，不能造成死锁。旧捕获的request函数必须绑定旧会话，重开不能借新session发送；同一复用文档中新会话提供新request能力。JS可调用/伪造内部bootstrap入口不能绕过Host检查。

 admission在Window controller锁内验证精确身份并注册retirement handle，再通过manager检查已安装/启用/非故障、ui.window、opt-in、完整activation tuple及Runtime ready。调度器idle-only原子拒绝running/waiting/rebuilding，不扰动已有工作。内部增加真实WindowAction owner/dispatch或等价有类型content owner，绝不伪造Panel或launcher身份。

锁顺序controller core -> manager mutation（如需要）-> network transition -> scheduler。service pending-response锁不得跨controller入口；所有完成路径先释放service/manager/scheduler锁再进入controller。Window action registration只负责撤销精确context，不回调controller、同步等待、发事件或销毁WebView。

发出Runtime事件前先设置30秒deadline观察者；不确定emit失败保留物理占用直至完成/超时。逻辑撤销立即使后续Runtime API请求失权并settle一次，但不提前释放物理槽。新Host command最新优先：撤销WindowAction为Cancelled，等待旧物理执行结束或超时后运行；WindowAction不能取代已有command。已获准执行的写入不能承诺回滚，UI需重新读取核对。

完成后必须先复核Window当前完整身份，再claim精确完成ticket/authority，才能发布成功。沿用Panel完成→权限变化→settle之间的单一有界ticket机制；所有生命周期/凭据/授权变动必须覆盖Window owner与ticket，不能只遍历Panel。重复/迟到回复不二次settle，不触发onUpdate/启动器结果。

下列路径在状态变更时统一撤销注册：submit替换（即使复用同文档）、begin hide/close、remove/uninstall、generation invalidation、session revoke、ready/ACK/focus失败、非预期导航、内容销毁、停用/权限和设置变更/凭据变化/更新与开发重载。隐藏开始即撤权，不等待动作完成；隐藏失败也不复活旧请求。重新打开获新session。老generation超时不能关掉替代窗口或Runtime。退出进程由原有teardown处理。不扩大窗口自动隐藏/置顶/位置或timer规则。

## 集成边界

涉及manifest/schema/CLI/SDK、Runtime tagged dispatch/readiness、scheduler和activation authority、PublicPluginService pending settlement、plugin_window controller/bootstrap与实际生命周期hook。新增private plugin_window_request只授权plugin-window-content，main/shell/panel/runtime/find拒绝，Host以真实WebView label决定身份。build.rs AppManifest、lib注册、生成权限及capability必须同批更新，避免上轮诊断ACL遗漏。保持Panel capability最小权限。共享JSON工具只抽取有必要的窄接口，不复制一份相同validator。

## JSON工作台插件

独立com.uipilot.json-workbench，version1.0.0，minimumHostVersion0.9.0，command默认json-workbench、submit/window、inputRequired=false，window.runtimeActions=true，仅ui.window。参考demo-win窗口结构与templates Runtime存储/草稿所有权，但只写新插件包/tests/docs。无网络、真实凭据、剪贴板或用户文件访问。

窗口展示JSON输入、格式化结果、清晰的校验/保存状态，按钮“校验并格式化”“保存草稿”“重新读取”，遵循docs/ui-guidelines.md双主题、沿用现有520×360窗口（含44px宿主标题栏，内容区约520×316）、Tab焦点和可滚动编辑区域。不自动保存；onCommand只读加载初值，onUpdate同步接收/更新后及时ACK，不await request。所有业务处理与storage在Runtime完成，内容WebView不调用直接storage。

动作只有load(payload:null)、format(payload:{text})、save(payload:{text,expectedRevision})，精确字段、未知动作返回{ok:false,code:'unknown-action'}。text最多8192 UTF-8字节；format解析JSON、拒绝非有限数字、深度超过16或值超过2048，正常返回{ok:true,formatted}，格式错误/超限返回固定code（不回传异常堆栈）；formatted最多32768 UTF-8字节。不能通过JSON.parse(1e400)再stringify为null静默改变值。save保存草稿原文本（允许语法未完成，但长度合法），不自动format。

storage单key workbench.draft，完整record {schemaVersion:1,revision:正安全整数,text}；无记录基线revision0/text空。load成功返回{ok:true,revision,text,hasSaved}；存储结构/版本不兼容时返回invalid-stored-data且禁止保存，不重置。save需expectedRevision为非负安全整数，与当前revision一致，拒绝溢出；读取校验后一次storage.set写revision+1；冲突不覆盖，返回revision-conflict；成功返回新revision/text。写入抛异常不假定未落盘、不重试。

UI维护编辑revision和会话所有权，busy时不重复发送。迟到format/load/save不能覆盖较新的编辑或onUpdate会话。保存不确定时保留草稿并提示重新读取；用户显式重新读取可放弃未保存修改，需有明确文案/确认。已确认保存与当前未保存编辑清楚区分，空白/换行保留。关闭重开以持久化内容为准，不承诺未保存草稿存活。

## 验证与人工交付

聚焦自动检查：manifest/CLI/schema parity和readiness missing handler；实际bootstrap有界JSON/捕获旧函数/ready门禁/error映射；Window admission vs hide/submit、撤销后API失权但物理槽保留、完成ticket与权限变化、重复迟到完成/旧generation超时、不确定emit watchdog；main/shell/panel错误caller ACL；旧Panel及Window timer重点回归。使用确定性barrier或状态机测试，不能只靠源码字符串。插件验证format正常/非法/非有限/深度、revision冲突、invalid-stored-data、保存失败不重试/不清空。用户主导UI功能验收，不机械重跑全仓矩阵。

交付独立uipilot-0.9.0-acceptance目录、exe/resources与本地插件包，沿用com.uipilot.launcher.acceptance；记录source commit、输入前后hash、exe/archive/resource SHA256、旧kit不变，禁止代理运行原生Host、安装插件、操作真实数据或系统服务。用户人工用例分批：安装打开/格式化/无效JSON反馈；保存关闭重开及进程重启；未保存关闭/重复按钮/开关窗口无陈旧结果；双主题键盘滚动；settings-demo/Notes/templates/recovery短回归。保持线上范围排除，不称完整基座完成。

## 设计调查补充

request能力以会话getter/绑定函数暴露，在首次onUpdate之前的内部PREPARE中安装；onUpdate中可以捕获该函数但调用必须Busy，页面模块初始化尚无会话时不能永久缓存undefined。旧引用在后续PREPARE后不可变成新引用。隐藏后的窗口phase可能仍Visible，必须同时验证Active，不能只验证Visible；隐藏失败依旧撤权并沿用现有destroy回退。

Window文档跨submit复用，导航/重载/销毁回调应绑定实际document身份并撤销当前属于该document的会话，不可直接捕获最初create时的session generation而漏掉后续owner；旧document延迟回调不能撤销替代document。需检查真实navigation/destroy hook而非仅controller测试。

版本同步必须包含src-tauri/src/public_plugins.rs::PublicPluginHost::current，它在当前代码仍声明0.7.0；与Cargo/package/Tauri/CLI统一0.9.0，避免新声明在真实Host被旧常量拒绝。历史插件minimumHostVersion不抬高。

## 冻结审核结论

独立架构审核通过，无阻塞。实现时按opt-in在API freeze之前决定是否安装request getter；旧窗口API必须完全没有该属性，不能只是值为undefined。PREPARE仅替换绑定函数，内部参数即使被内容脚本伪造也不能成为Host授权依据。自动回归需验证旧timer/storage引用失效及新引用可用；人工收尾增加既有pomodoro：开始计时→隐藏→正常到期→重开，证明此次共用bootstrap改动没有破坏后台计时（使用交付提供的既有插件包，不访问外部服务）。
