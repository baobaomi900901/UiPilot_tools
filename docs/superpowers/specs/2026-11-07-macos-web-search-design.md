# macOS 主启动器 Web 搜索 fallback

在 Mac 主启动器的普通查询结果中增加 Web 搜索动作，并接通现有搜索引擎设置。沿用 [UI 规范](../../ui-guidelines.md)、[快速计算的票据与完成归属](2026-11-07-macos-quick-evaluation-design.md)，不开放公开插件、任意 URL IPC 或全部内置功能。

## 产品范围与收藏语义

这是一条针对当前查询的搜索动作，不是可复用的 `/web-search` 命令入口。非空普通查询在快速计算判定为 NoMatch 后，继续保留应用排序和已启用的 Find 结果，并在其后追加一条搜索结果；没有应用命中也可以显示该行。快速计算已经匹配的成功或错误结果不混入 Web fallback。

空输入和 slash 查询不产生 Web fallback。`/web-search`、命令补全、`builtinActivation`、稳定 builtin target、命令别名和命令热键均不在本轮开放。Mac 原有不支持的稳定命令参数继续拒绝，不能把 slash 参数当成绕过命令校验的普通搜索文字。

结果使用现有 `ResultItem`：`iconKind: webSearch`、`activation: executeResult`、`hasDefaultAction: true`；标题取共享 provider 名称，副标题为“搜索：<当前查询>”。`favorite` 为 None，不提供收藏菜单。Windows 的查询搜索行可收藏，是因为其收藏指向已经存在的可复用 Web 搜索入口；Mac 本轮没有该入口，不能照抄这个收藏元数据。已有持久化 WebSearch 收藏也不因此生成一个 Mac 入口。

本轮不增加页面布局、颜色或快捷键。沿用当前列表的单行截断、主题、选中态、Enter 和点击执行路径。完整的 `/web-search` 入口以后必须同时实现 `BuiltinFeature`、activation、收藏读写、稳定目标、设置和对应前端校验，不能只显示一个可以收藏但不能再次打开的结果。

## 查询、编码与网络边界

共享 `web_search.rs` 的 provider 元数据、`search_url` 和同组编码测试。Bing、百度、Google 分别使用固定 HTTPS endpoint 和固定 query key；通过 `url::Url::query_pairs_mut().append_pair` 编码为一个查询值。不得从查询内容取得 scheme、host、模板或额外参数。输入看起来像 URL 时仍作为搜索文字编码，不自动导航该 URL。

Mac 对 trim 后的 Web 查询采用 4,096 UTF-8 字节上限；空白、NUL 和超过上限的查询不产生 Web fallback，不截断或悄悄改写查询。这个边界只限制新增的 Mac Web 行，不改变应用搜索或 Windows 已有 URL 行为。组装后的原生 URL 再做 16 KiB 上限检查。

输入、排序、结果发布、引擎设置及错误呈现均不发 HTTP 请求，不做 DNS、网络图标、建议词或可达性探测。只有用户明确执行当前结果，宿主才把固定 provider URL 交给系统默认浏览器。浏览器随后产生的网络访问不属于宿主在输入阶段请求的内容，也不被报告为网页已成功加载。

服务端注册 `ResultAction::OpenWebSearch { engine, query }`；前端执行只传 request/result ID。不能接受 JS 传入 URL、搜索引擎模板或执行正文。每条结果固定其发布时的引擎，执行时不把同一行悄悄切换到另一个 provider；设置成功影响后续查询。

## 设置、能力与调用者

沿用 `SettingsStore.web_search_engine`、已有严格 `WebSearchEngine` 枚举及其持久化路径，默认值保持 Bing。新增 Mac 的 `set_web_search_engine` IPC wrapper，参数与 Windows 的 `{ preference: { engine } }` 合同一致。保存失败不报告成功，前端恢复最后已确认的设置；不接受未知 engine 或额外危险配置。

内部同包 HostCapabilities 新增必填 `webSearch` 布尔字段，Windows 与 Mac 均为 true；设置控件和结果过滤使用这个窄范围的 Web 搜索支持判断，不能为此令 Mac 的 `builtinFeatures` 全部变为 true。允许的 Mac Web 行仅为 `webSearch` 图标、`executeResult`、无收藏和无插件动作的宿主结果；仍拒绝 Web completion、builtin activation、插件图标或伪造收藏。既有 WebSearch 停用设置若存在应被尊重；本轮不新增启停、重命名或热键管理接口。

搜索、执行和设置入口均先调用 `require_main`，校验窗口标签和可信来源；不能仅凭 `main` 标签。打包应用必须匹配 `tauri://localhost`；开发环境只接受配置中的 loopback dev origin，包括正确端口。Find、插件窗口和远程页面不获得这些权限。Tauri ACL 仅向 Mac main 增加所需设置命令，invoke handler 与动态权限生成清单保持一致。

## 原生打开 API 与证据

新增 Mac worker adapter，使用同一线程内创建和释放的 `NSWorkspace`、`NSURL` 及 autorelease pool。先验证完整 URL 和准备原生对象，再进入最终结果准入；不使用 shell、`open` 命令、进程参数拼接，也不修改默认浏览器设置。

原生 helper 仅接受有 host 的 http/https URL，拒绝 userinfo、NUL、无 host、其它 scheme 和超出上限的 URL。本轮调用者只会提供共享 provider 生成的 HTTPS URL；这个 helper 不是一个公开的“打开任意 URL”接口。`NSURL` 构造失败时零系统打开调用。

使用 `NSWorkspace.openURL:` 的一次同步提交。Apple 的 [openURL 文档](https://developer.apple.com/documentation/AppKit/NSWorkspace/open(_:)?language=objc)说明：该方法返回位置是否成功打开的布尔值，并且在 macOS 10.6 及以后可以从任意线程安全调用。此返回值不证明浏览器已完成页面下载或搜索服务已响应。SDK `AppKit.framework/Headers/NSWorkspace.h` 与项目缓存的 `objc2-app-kit 0.3.2` generated `NSWorkspace.rs` 均提供该 selector；现有 NSWorkspace、NSURL、NSString features 足够，不需要新增网络库。

不调用 [setDefaultApplication](https://developer.apple.com/documentation/appkit/nsworkspace/setdefaultapplication(at:toopenurlswithscheme:completion:)) 或请求改变系统默认应用。只交由系统处理当前 URL。Objective-C 异常通过窄 `objc2::exception::catch` 捕获，异常对象直接丢弃，不记录 URL、查询、异常 name/reason 或系统错误正文。

## 一次性执行与完成归属

复用快速计算建立的执行票据和完成 owner，把允许的 Mac 主窗口动作扩展为明确的 `CopyBuiltInText` 与 `OpenWebSearch`，不泛化为所有 registry action。Web 执行端还必须要求预期类型为 `OpenWebSearch`，不能借通用消费接口调度另一类动作。

worker 开始时解析原 registry 的当前 request/result，取服务端 engine/query 并构造 URL。准备成功后立即执行最终 admission：核对 registry 身份、Main scope、Application domain、invocation、当前结果集和动作类型；原子消费该结果集，同时递增 domain epoch，使旧 publish token 也无法重新发布。跨 registry 数字计数相同的票据必须失败，身份比较使用指针身份而不是 Arc 所含值的相等性。

原生调用不持 registry 锁。已准入的打开操作允许完成；随后出现新查询或新窗口，只影响完成归属，不能通过自动撤销、重试或再次打开去“修正”系统操作。同一 request/result 最多调度一次原生打开。

成功使用现有 `{ status: launchRequested }`，不添加“网页已打开完成”的承诺。完成 owner 保留 registry、scope generation、invocation、query sequence、domain epoch 与 result-set generation。主线程只在原子确认 owner 仍当前并退役其 invocation 后调用对应隐藏；同窗新查询、新 invocation、隐藏后重开和迟到 completion 都不能关闭新状态。应用启动的使用计数只属于应用动作，Web 搜索不增加应用计数。

## 错误与前端清理

| 阶段 | 后端结果 | 票据与界面 |
| --- | --- | --- |
| 调用者错误 | `invalidCaller` | 不访问目标，不调用原生打开 |
| 旧 request/result、错误 registry 或最终 admission 失效 | `staleRequest` / `unknownResult` | 零原生调用；不隐藏新状态 |
| URL 校验或原生对象准备失败 | `webSearchFailed` | 未消费时不调用打开；无自动重试 |
| admission 后 openURL 返回 false 或调用抛异常 | `webSearchOpenUnconfirmed` | 已消费，不恢复旧票据；清理该 request 的旧结果与动作，不隐藏，不自动重试 |
| openURL 返回 true | `launchRequested` | 已消费；仅当前完成 owner 可隐藏 |

`webSearchOpenUnconfirmed` 表示这次已提交的动作没有得到可用的成功确认；false 是负结果，异常则不能确定系统是否已有副作用，两者都不允许重放旧票据。固定提示让用户检查浏览器并重新搜索。前端收到晚到错误时，只能清理仍属于同一 request、query/view epoch 和 invocation 的结果，不能清掉新输入的结果或影响其它窗口。重新查询可产生新动作，旧动作本身不可重试。

## 自动验证与人工边界

默认验证只使用 fake opener、纯 URL 构造和专用临时设置目录，不访问真实浏览器、通用剪贴板、Keychain 或个人设置。不把 NSURL 构造或函数存在测试当作默认浏览器已经实机通过。

验收至少覆盖：

- 三个固定 provider 的 endpoint、单 query value 编码，中文、空格、`&`、`#`、`?`、URL 形状文字和非 ASCII 内容；空、NUL、字节预算边界拒绝。
- 普通应用结果及 Find 顺序保留；零应用命中仍有 fallback；空、slash、快速计算匹配时没有 Web 行；不存在收藏、completion 或稳定 builtin 入口。
- 已保存引擎重载和严格枚举校验；固定元数据与服务端 action 一致；设置保存失败回滚呈现，错误 caller/origin 无写入。
- 查询与设置路径不调用 opener；执行时 prepare 完成后才 admission，stale/foreign/forged/action-type mismatch 均为零打开。
- 一次准入只提交一次；consume 使旧 publish token 无效；false/异常退役动作，无自动 retry；晚到错误不清新 query。
- 成功动作返回 `launchRequested`；同一窗口的新 query、新 invocation、hide/reopen 和另一 registry 都不能被旧 completion 隐藏。
- Mac 窄能力过滤允许真正 Web fallback 与设置，同时继续拒绝未开放的命令、收藏及插件形态；Windows URL 构造、执行行为与测试保持原路径。

真实默认浏览器、页面跳转和主界面键盘操作留到约 70% 候选包统一人工验收，本轮不要求人工操作，也不启动浏览器。

## 源文件分工

后端实现者负责共享 `web_search.rs` 的平台边界、新 `macos_browser_open.rs` 及 fake tests、`macos_apps.rs` 的 fallback/执行、registry 的窄 host-action 消费接口与回归。协调者负责 `platform.rs` module 接线、Mac 设置 IPC、ACL/handler、前端能力与设置/错误呈现、规格计划和最终集成验证。Windows `browser_open` 与 Windows 命令流程保持原实现。
