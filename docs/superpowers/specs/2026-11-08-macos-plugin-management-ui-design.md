# macOS 插件管理界面接线

在已有安装生命周期之上接入正式设置页和可信 Host 管理 IPC。不依赖 Apple 开发者证书；不开放插件执行。`HostCapabilities.pluginManagement` 为独立、向后兼容的可选能力，Mac 返回 true，`plugins` 继续 false。只有设置的「插件」tab 可见；运行结果、独立权限页、启动快捷键、secret 与旧插件服务不因此启用。Windows 继续使用现有管理组件和接口。

## 管理行为与数据

Mac 管理页提供列表/筛选、详情、导入归档或开发目录、升级确认、普通设置、启用配置、clipboard.write 配置和移除。启用/授权只是保存配置，界面明确插件目前不能运行。普通设置在停用状态也可编辑；不显示 secret 定义或值。固定命令名只供查看，不接改名/收藏/快捷键。包仍受前一阶段 MainResult/无 resultActions/权限子集限制。

移除使用准确的确认语义：**删除安装与普通设置，保留插件私有存储数据**。不会递归遍历、清理 storage 或 secure erase。空安装库可以保留非零 revision/generation；重新安装必须确认全部声明权限，并按新 manifest 初始化普通设置，不恢复已删除的配置或旧 grant。

所有读写使用真实 `MacPluginLifecycle`。`plugin-management-v1/{inventory,storage,settings}` 位于应用数据目录，首次管理请求才建立并加载，启动不加载/执行插件。新根及子目录严格 0700；已有 appdata 可为当前 UID 拥有、无 group/world 写权限的 0755，仍核对本地卷、ACL、nofollow 和路径身份。不修改用户现有目录权限。损坏拒绝加载，不用空库掩盖。

## IPC、选包与所有权

Mac main ACL 只增加八个管理命令，且每个入口在读 state、目录或打开选择器前调用 `require_main`。不授予前端通用 dialog/任意路径能力，不为 Find/runtime/plugin 窗口增加权限。前端发送来源类型和 expectedRevision，由原生 NSOpenPanel 选包；NSURL/security-scoped lease 成功取得时覆盖快照读取，并配对释放。未取得 scope 不虚报权限，后续文件读取仍可失败。

准备结果返回不可猜测、短期、单次 token，绑定原 selection owner、控制器 epoch、revision 与已验证快照。用户明确确认后提交准确新增权限集。所有设置/启停/授权/移除均要求精确十进制 u64 revision，不经 JS number。旧 revision/token、未知字段、重复授权、过期和迟到选择拒绝。

后台管理 IO 串行；窗口隐藏/关闭/退出以独立短锁更换 epoch，不等待 IO mutex。原选择器完成只能清理自己的 owner。旧排队请求不能写入新会话；已经准入的磁盘事务可能完成，结果变为 stale，不声称撤销已经发生的写入。原生 sheet 活动期间保留父窗口；完成后重新检查真实焦点，旧检查不得隐藏新的 invocation。

Storage/CommitUncertain 失败会先撤销 selection/pending，再丢弃控制器、释放其锁并退休旧 epoch。不会自动重试写入；下一条明确请求重新加载并验证实际文档。UI 在这类写入失败后只允许刷新核对，保留草稿供用户查看，刷新需先确认放弃草稿。后端不宣称强制所有调用者必须先 list；仍由每个变更的 revision/token 做准入。

## 界面、键盘与异步状态

遵循 `docs/ui-guidelines.md` 的双主题 token、设置侧栏、稳定工具栏/反馈区、普通表单与危险确认。列表支持名称/命令筛选与 Ctrl/Command+F；按钮使用键盘原生行为。进入详情聚焦标题，返回列表恢复原行或筛选框。安装与移除确认都有取消；Escape 关闭当前确认，不提交。草稿切换 tab、返回、刷新、移除及动态操作的返回/隐藏都经过放弃确认；取消保留选中项和输入。

UI 使用同步 busy 准入，禁重复提交。每次异步操作捕获组件 generation，迟到响应不能覆盖新的客户端会话；迟到准备 token 必须取消。客户端实例有跨挂载串行队列，新列表读取等待旧挂载的变更结束。失败不乐观发布已授权/已保存，stale 和不确定提交不自动重写。

普通设置保留合法空字符串、false、0、多行文本和可选未设置状态。用 own-property 读取值，合法 `constructor` 键不得误读原型属性；未修改项不提交。数值/选项/控制字符前端先校验，最终以 Host manifest 校验为准。

## 验证与未测项

自动测试覆盖真实磁盘准备/确认/设置/撤权/移除重装、755 父目录/700 子目录、reset 与在途 IO、真实 AfterRename 不确定提交恢复、损坏仍拒绝。前端覆盖独立 capability、真实 Launcher 不调用旧插件接口、精确 revision/权限确认、草稿/Modal Escape、多行/constructor、客户端替换迟到回包与重复提交。

浅色/深色使用真实 Launcher 和管理组件的浏览器 fixture 渲染；fixture 只存在浏览器 preview 入口，不打包进生产 main。未运行 Tauri 原生选择器、TCC 授权、用户真实安装库或实际插件执行。原生窗口焦点/选择器和发布包交互需要后续实机验收；签名 XPC/沙箱、完整模块图、真实权限服务及同包双端生产验收仍未完成。
