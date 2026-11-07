# macOS 登录项与系统状态

将原计划 Task 10.5 实现为设置页中的“登录时启动”控件，以 macOS ServiceManagement 的实际状态为准。遵循[UI 规范](../../ui-guidelines.md)，Windows 既有 autostart/save_settings 路径保持原行为；Mac 不启用 Tauri 的 LaunchAgent/AppleScript 布尔接口。

## 系统事实与兼容

使用 `SMAppService.mainAppService`，仅注册当前主应用。目标最低 macOS 15.7.4 满足 API 13.0 下限；应用必须有有效代码签名。Apple [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice) 与本机 SDK `ServiceManagement.framework/Headers/SMAppService.h` 定义四种状态：`notRegistered`、`enabled`、`requiresApproval`、`notFound`。`requiresApproval` 也表示用户在系统设置撤销了允许，不能当作已启用；异常或未知枚举映射 `unavailable`。

Mac 不把持久化 `settings.autostart` 当成实际登录项状态。保留此旧字段供兼容，不在启动、读取设置、保存快捷键或重启时对账/自动重新注册。不保存第二套 desired bool，避免设置落盘和系统授权的伪事务。本项替代原计划“设置写入失败恢复原登录项”的假设：系统用户允许状态无法由应用补偿还原；失败必须重读事实，不能报告回滚成功。

## 明确操作与状态转换

仅三个受可信 main 来源保护的独立 IPC：`get_macos_login_item`、`set_macos_login_item`（严格 `{ input: { enabled: boolean } }`）、`open_macos_login_item_settings`。默认生产 ACL 仅 main 接收，不给 Find/插件/远程来源。不经全量 save_settings，不接受 bundle ID、plist 路径或 URL。

原生操作在 AppKit 主线程执行，所有对象在该线程创建和释放。状态读取和一次变更在宿主统一主线程队列串行，不能只依赖某个前端组件的 busy；同步 register/unregister 之后重读 status。不创建 helper、不写 LaunchAgents、不使用 shell/AppleScript、不申请管理员权限、不改其它登录项。

- enable：enabled 时仅返回 unchanged；requiresApproval 时返回 requiresApproval，不重复 register 或自动打开系统设置；notRegistered 时只调用 register 一次；notFound/unavailable 不发变更。
- disable：notRegistered 时仅返回 unchanged；enabled/requiresApproval 时只调用 unregister 一次；notFound/unavailable 不发变更。
- 每次原生变更后，无论 bool 成功与否，都尝试重新读取 status。仅成功且目标事实匹配时返回 applied；成功注册后 requiresApproval 返回 requiresApproval；其余返回 unconfirmed。不存在自动重试或补偿注册/注销。
- 通过明确点击“打开系统登录项”才调用 `openSystemSettingsLoginItems`；仅表示请求打开设置，不表示用户批准。不自动导航、不模拟用户允许。返回设置后用户点击“刷新”获得新事实。

原生 NSError/Objective-C 异常不得进入 UI/日志，不读取 name/reason。每个异常边界固定转换为不可用/不确定状态。原生 register/unregister 可能触发正常系统通知，这是用户明确启停操作的系统效果，不能在后台启动时发生。

## 内部 DTO 与界面

读取返回 `{ status }`；变更返回 `{ status, outcome }`，outcome 为 `applied | unchanged | requiresApproval | unconfirmed`。状态与结果独立：例如返回 false 但重读 enabled，界面应展示实际 enabled 同时说明变更未能确认，不能用失败的乐观值覆盖事实。协议解析拒绝未知字段/值与不完整对象，失败不猜测已关闭。

新增窄 `loginItems` 能力，只有 Mac 内部 `macos-login-item-preview` 构建为 true；默认发布构建仍为 false，IPC 在可信来源检查之后复核 feature，未开启时零原生调用。70% 内部候选显式带该 feature 做人工验收，通过后再决定正式开放。Windows autostart 能力保持原样。Mac 设置使用独立、可键盘操作的紧凑控件：仅 enabled 显示勾选；requiresApproval 显示等待系统允许，并提供独立“停止注册”按钮（不可因勾选已为 false 就跳过），以及打开系统设置按钮；notFound/unavailable 显示不可用，可刷新。读/写期间禁用重复操作，不展示旧的可编辑状态。固定提示，使用既有主题、按钮、标签规范，不新增启动器结果或收藏。

首次进入设置/组件重建时只读；刷新只读。前端区分组件生命周期与请求序号，卸载后的响应不更新新界面；一个组件最多一个变更在途，read/refresh 不抢占变更。再次进入设置始终重读系统状态。主窗口隐藏/切换时迟到 UI 完成不恢复旧设置界面；已由用户提交的系统操作允许完成，不自动反向补偿。Web 设置、改键与登录项互不借用保存接口。主页面持久单例 client 在 set 调用时同步登记 pending，跨组件的新 read 循环等待所有已提交 mutation settle 后才读取；重复 set 固定 busy 且零 IPC，unmount 不清 shared pending。原生响应直到变更与 status 复读实际完成才 settle。

## 验证

默认使用 fake 原生 backend，不读取或改变真实登录项；native binding 只编译/link，不调用 SMAppService。覆盖完整状态转换表：重复启用/关闭零写、requiresApproval 启用零写、外部撤销后刷新反映状态、false/异常后重读、成功但事实不匹配不报成功、一次调用且无重试、未知状态不可用。临时设置样本保留旧 autostart，证明普通保存不触及登录项。

前端测试覆盖严格 DTO、enabled/待批准/不可用三种呈现、显式启停与设置按钮、刷新零变更、操作期间禁用、错误不含原生详情、卸载/重建旧回调隔离及 Windows 原控件保持。ACL 检查三命令只授 main，handler 每次 require_main 在原生之前。

约 70% 时用固定哈希的已签名内部包，在获准测试账户人工验证：启用、系统面板核对、外部撤销、刷新、注销/登录后行为、停用及恢复测试前状态。签名/系统通知/真实登录行为未验证前，不称本项完整验收通过；无需现在操作测试者的系统设置。
