# macOS 插件主结果校验与内部发布

承接设置持久化与 Runtime 数据接线，完成不依赖开发签名的结果处理层。共享真实 Windows legacy mainResult 合同，接到真实 QuickJS 完成消息和 Mac 的 ResultRegistry。生产插件加载、安装、XPC、GUI IPC 仍未装配；不运行 GUI 或剪贴板操作。这里只开放内部固定 MainResult 路由，不能作为完整 manifest 输出类型分发或权限服务。

## 同源结果合同

从 activation.rs 提取 PublicMainResult、wire DTO、limits 和 parser，原路径保留 re-export。完整 PluginRuntimeError 从 runtime.rs 原样移动到无平台依赖模块，原导出不变。保留现有 20 行、64 KiB compact JSON、title 256 字符、subtitle 512 字符、detail 16 KiB 的边界、空 title/任意非空唯一 row ID、optional null 行为；不借提取改变 Windows 输入合同。copy payload 只在宿主结果对象中保留，不放入显示 DTO。

Mac raw 输入先限 64 KiB 并拒绝重复 JSON 键，再运行同源 parser。结构合法不代表动作授权；本轮没有真实插件复制权限及执行服务，带 copyText 的整个响应明确拒绝，不能剥离动作或注册可执行 CopyText。resultActions、Panel/Window 不属于此内部路由。

## 完成与发布所有权

RuntimeDataBinding 校验结果后调用真实 session.prepare_completion，产生绑定原 endpoint 的不可序列化结果票据，私有持有完整 rows。传输适配器证明双输出 EOF、正常退出等终结条件后，才调用 submit_completion。不同 binding、旧请求、重复完成、撤权与过期均拒绝。主库复用同一 API，探针不绕过它自行把 JSON 当成功结果。

Mac MainResultPublisher 持有实际 ResultRegistry；begin 捕获当前 invocation/query 的 QueryToken，铸造绑定 publisher 实例的 opaque owner。owner 绑定对应 RuntimeDataBinding。即使两个 registry 的数字 token 完全相同也不能互换 owner。准备与提交均校验 owner；提交的同步闭包调用实际 publish_if_latest，检查查询与写入位于同一 registry 锁内，不能先检查再异步写。

owner 的 completion 状态仅允许一次 pending→terminal：成功、失败或拒绝都消费本次完成机会。错误清理仅对 pending owner 生效，并且在同一 registry 锁内匹配 query 后才失效该 plugin query。成功后的迟到错误不能清掉已发布结果。宿主生命周期 retire 是独立可信入口，可撤销该 owner 的已发布结果，但必须同样核对 query；新 query、hide/show 或其他 domain 不受旧 owner 清理影响。取消传输与 gate 排空仍由 RuntimeDataHost 负责，结果失效不能冒充进程退出。

发布使用已有 ResultItem/SearchResponse，宿主分配 opaque request/result IDs，所有行均为 Informational、无默认动作、无 favorite/图标/执行载荷。不将插件 row ID 或任意字段升级为执行能力。生产前端仍不接收本模块消息，GUI 显示与结果执行留后续装配验收。

## 验证

共享测试覆盖精确字段、ID/数量/Unicode字符与字节上限、null 和默认动作边界，旧 Windows caller 保持原错误及返回类型。真实注册表测试包括两个数字相同的 registry owner 互换、错误/缺失来源拒绝、成功后迟到失败、并发新 query 后旧提交、hide/show、清理原 owner 不波及新结果、非法结果无部分发布，以及 opaque 显示 DTO 不包含 copy payload。完成票据测试覆盖 gate 撤销、错误 endpoint、在途 reply、准备后过期和单次消费。

QuickJS 执行原样 demo-settings 后经过共享完整 parser/完成票据，核对真实 rows；固定负例覆盖未知字段、重复 row ID、超量 rows、copy action 不支持及坏结果 ID。保留既有真实 settings/storage 往返与进程回收测试。主库全回归和独立 reviewer 通过后提交推送到 macOS 分支，不合并 main。内部结果链路通过不计为双端生产插件验收。
