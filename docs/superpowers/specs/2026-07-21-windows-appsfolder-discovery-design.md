# Windows AppsFolder 打包应用发现与启动设计

## 状态

- 日期：2026-07-21
- 状态：已批准
- 实现基线：`bb4a03c48ee0bb5b03b5b69294a54bb3bc76cd24`
- 影响范围：Rust 应用发现、结果排序、可信动作注册与 Windows 原生启动

## 目标与边界

保留现有 Start Menu `.lnk` 扫描，并通过 Windows Shell 原生枚举 `FOLDERID_AppsFolder`，补充计算器、Windows 设置等具有有效打包 AUMID 的应用。发现结果继续只通过现有 `ResultItem` DTO 暴露标题、副标题和图标；AUMID、快捷方式路径、可执行文件路径与真实动作只保存在 Rust。

本次不抓取图标，不搜索网页或文件，不接入 Windows 11 全局搜索，不增加自由文案字段，不启动 PowerShell 子进程，不读取注册表或包清单重造系统枚举，也不提供 PowerShell fallback。前端协议和组件不变。

## 领域模型

新增结构化入口类型：

```rust
enum ApplicationEntryKind {
    DesktopShortcut,
    PackagedApp,
}

enum ApplicationLaunchTarget {
    Shortcut {
        shortcut: PathBuf,
        executable: Option<PathBuf>,
    },
    PackagedApp {
        aumid: String,
    },
}
```

`Application` 保存一个 `ApplicationLaunchTarget`，并由目标变体派生 `ApplicationEntryKind`，不重复存储可能互相矛盾的类型字段。现有独立 `shortcut` 与 `executable` 字段被移除，因此打包应用不可能携带快捷方式路径，桌面快捷方式也不可能携带 AUMID。

`ResultAction::LaunchApplication` 只保存 `app_id` 与同一个可信 `ApplicationLaunchTarget`。它不实现序列化，前端仍只能用当前结果集的 `requestId + resultId` 触发动作。

## 原生发现流程

生产扫描继续先查询并扫描 `FOLDERID_Programs` 与 `FOLDERID_CommonPrograms`。同一个后台扫描线程随后在已初始化的 STA COM apartment 中执行 AppsFolder 枚举：

1. `CoCreateInstance(KnownFolderManager)` 得到 `IKnownFolderManager`。
2. `GetFolder(FOLDERID_AppsFolder)` 得到 `IKnownFolder`。
3. `IKnownFolder::GetShellItem` 得到根 `IShellItem`。
4. `IShellItem::BindToHandler(BHID_EnumItems)` 得到 `IEnumShellItems`。
5. 循环调用 `IEnumShellItems::Next`；`fetched == 0` 是正常结束，失败 HRESULT、数量异常或空 item 是整体枚举失败。
6. 每个 item 使用 `SIGDN_NORMALDISPLAY` 读取本地化名称，转换为 `IShellItem2` 后用 `PKEY_AppUserModel_ID` 读取 AUMID。
7. `GetDisplayName` 与 `GetString` 返回的 `PWSTR` 无论后续转换成功与否都由 `CoTaskMemFree` 释放。字符串必须是非空、无损 UTF-16；不使用 lossy 转换。
8. 使用 `ParseApplicationUserModelId` 的长度查询和实际解析两次调用验证 AUMID。只有完整解析成功的 packaged AUMID 才进入结果。

单个 item 缺少或无法读取名称时跳过并增加 `invalid_packaged_names`；缺少、无法读取、无法无损转换或无法解析 AUMID 时跳过并增加 `invalid_packaged_aumids`。这两个计数扩展现有定长 `DiscoveryDiagnostics`；诊断不保存名称、AUMID、路径、HRESULT 文本或其他 item 标识。

Known Folder manager 创建、AppsFolder 获取、根 item 获取或 handler 绑定失败统一返回固定、无路径的 `AppsFolderUnavailable`。枚举中的 `Next` 失败或返回无效状态返回固定、无路径的 `AppsFolderEnumeration`。这些错误使整个 refresh 失败；`AppCache::refresh_with` 仍只在完整 `DiscoverySnapshot` 成功后一次性替换缓存，因此 Start Menu 已成功而 AppsFolder 失败时也保留上次完整快照。

## 身份、去重与合并

Start Menu 身份算法保持不变。打包应用使用以下 UTF-8 预映像进入现有 Windows CNG SHA-256 流程：

```text
"packaged-aumid-v1\0" + aumid.to_lowercase()
```

输出仍为 `app-` 加 64 位小写十六进制。显示名不进入身份。相同 AUMID 的大小写差异先按 Unicode lowercase key 去重；候选按 lowercase AUMID、lowercase 显示名和原显示名排序后保留第一项，使结果不依赖 Shell 枚举顺序。

Start Menu 与 AppsFolder 的记录按 `app_id` 做最终冲突检查后合并。命名空间不同，因此桌面入口和打包入口即使名称相同也保持两个身份；发现逻辑不按显示名去重。任何真正的跨来源 `app_id` 冲突仍使扫描失败。

## 展示与排序

`registry_entry` 复用现有 `ResultItem.subtitle`：

- `DesktopShortcut`：`应用程序`
- `PackagedApp`：`打包的应用程序`

现有 React 渲染已经支持副标题，不新增字段或组件。

排序顺序保持现有匹配等级、别名匹配和 `use_count` 语义：匹配等级、别名优先级、`use_count`、lowercase 名称均相同时，`PackagedApp` 排在 `DesktopShortcut` 前，之后才以 `app_id` 打破平局。入口类型不得越过更高 `use_count`。因此全新配置中查询同名“设置”时 Windows 打包设置优先；使用次数更高的普通同名入口仍优先。

## 启动与 COM 生命周期

桌面快捷方式继续沿用现有“尽力激活已运行进程，否则 `ShellExecuteW` 启动可信 `.lnk`”策略。打包应用不进入进程路径匹配，直接通过 `IApplicationActivationManager::ActivateApplication(aumid, null, AO_NONE)` 启动，并返回现有 `LaunchRequested` 结果。

启动前调用 `CoInitializeEx(COINIT_APARTMENTTHREADED)`：

- `S_OK` 与 `S_FALSE` 表示本次调用取得一个必须配对的 COM 初始化引用，作用域 guard 在同一线程调用一次 `CoUninitialize`。
- `RPC_E_CHANGED_MODE` 表示线程已由宿主用另一 apartment 模式初始化；继续使用现有 COM 初始化，但本次调用不拥有引用且不得调用 `CoUninitialize`。
- 其他失败返回固定 `ApplicationEntryUnavailable`，不调用激活 API。

创建 `ApplicationActivationManager` 后尽力调用 `CoAllowSetForegroundWindow`；该调用失败不阻止 `ActivateApplication`。manager 创建或激活失败映射到现有固定、无 AUMID 的动作错误，不重试、不回退到 shell 文本命令。

## Windows crate 特性

继续使用 `windows = 0.61.3`，不新增 crate。现有 `Win32_System_Com` 与 `Win32_UI_Shell` 已覆盖 COM、Known Folder、Shell item、枚举和激活接口；只补充 `Win32_Storage_Packaging_Appx` 用于 `ParseApplicationUserModelId`，以及生成 `PKEY_AppUserModel_ID` 所需的 `Win32_Storage_EnhancedStorage`。

## 测试合同

实现必须按 TDD 覆盖：

1. 打包 AUMID 身份固定向量、大小写稳定性以及与 Start Menu 命名空间隔离。
2. AUMID 大小写去重；同显示名不同入口保留。
3. 缺失名称、缺失/无效 AUMID 逐项跳过并只增加固定计数。
4. `IEnumShellItems` 正常结束与枚举错误严格区分；整体错误不产生部分成功快照。
5. Shell 返回字符串在成功、空指针和无效 UTF-16 路径都释放一次。
6. AppsFolder 整体失败时 `AppCache` 保留上次完整快照。
7. 两种入口生成正确副标题，DTO 序列化不包含 AUMID或路径，Rust 注册表保留正确目标变体。
8. 排序只在现有条件及名称相同后优先打包入口，更高 `use_count` 仍优先。
9. 动作执行链分别路由快捷方式和打包目标，打包目标不会调用桌面激活或 `.lnk` 启动。
10. 打包启动在 `S_OK`、`S_FALSE`、`RPC_E_CHANGED_MODE` 和其他 COM 失败下具有正确调用与 `CoUninitialize` 所有权；`CoAllowSetForegroundWindow` 失败仍执行激活。
11. Windows 真实人工验证能搜索并分别启动计算器、Windows 设置；同名结果副标题可区分且各自启动自己的入口。

## 非目标

- 不抓取或缓存 AppsFolder 图标。
- 不引入通用 Shell 枚举框架、单实现 trait、factory 或专用线程。
- 不改变 TypeScript DTO、Tauri 命令参数或 UI 组件。
- 不读取注册表、AppX manifest、文件系统包目录或远程内容。
- 不提供 PowerShell、`explorer.exe shell:AppsFolder` 或其他降级启动路径。
