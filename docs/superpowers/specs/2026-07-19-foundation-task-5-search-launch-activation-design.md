# Foundation Task 5 应用搜索、启动与尽力激活设计

## 状态

- 日期：2026-07-19
- 功能设计：Task 5 行为合同已 Go；本次只修订 warning/cfg 边界
- 安全状态：`TaskSecurityReviewRequired`
- 实施状态：`TaskCodeGo No-Go at f204c0c45050de979beb7311cf52a3e5c2c57ee8 / corrective lint TDD authorized`
- 发布状态：`ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001`
- implementation/trust baseline：`2788e9a275e0406e70d7597a4a78da274d8c55c0`

`SettingsView` / `UserSettingsUpdate` 与唯一 `clear_and_hide` 两项 corrective 行为已经代码级 Go。当前
No-Go 只因 `lib.rs` 的 module-wide warning suppression 让 normal production 与 feature-only probe 的
Clippy 证据可假绿。本修订冻结复审已授权的最小 lint/cfg 纠正；不改变行为合同、baseline 或 release block。

## 目标与边界

Task 5 只交付以下 Rust 能力：

1. 从唯一 `AppCache` 快照搜索应用，以唯一 `SettingsStore` 装饰别名和 `useCount`，并通过唯一
   `ResultRegistry` 发布冻结的 `SearchResponse` / `ResultItem` DTO。
2. 只解析 `ResultRegistry` 中的可信 `ResultAction`，在 Windows 上执行正常 `.lnk` 启动或尽力激活。
3. 包装 `build.rs` 已预声明的八个窄 Tauri command，并接线 Task 4 的设置、验证计数和导出服务。
4. 在成功的应用动作后记录批准的 `ValidationEvent`，并持久化可信 `appId` 的使用次数。

Task 5 不实现主窗口显示、全局快捷键、失焦/关闭事件、托盘、开机启动或进程退出清理；这些属于
Task 6。它不实现 UI、前端协议类型或设置页；这些属于 Task 7。它不实现性能埋点和最终 smoke；
这些属于 Task 8。

本任务也不实现文件搜索、`/find`、插件、macOS、安装包、签名或发布。Task 4C 的 native save
dialog 与 writer 保持原所有权；Task 5 只提供其批准的零业务参数 async wrapper。

## 已冻结依赖

Task 5 复用以下现有实例和接口，不创建第二个 store、cache 或 registry：

```rust
impl ResultRegistry {
    pub(crate) fn begin_query(
        &self,
        invocation_id: &str,
        query_sequence: u64,
    ) -> Option<QueryToken>;
    pub(crate) fn publish_if_latest(
        &self,
        token: QueryToken,
        entries: Vec<(ResultItem, ResultAction)>,
    ) -> Option<SearchResponse>;
    pub(crate) fn resolve(
        &self,
        request_id: &str,
        result_id: &str,
    ) -> Result<ResultAction, RegistryError>;
    pub(crate) fn hide_and_clear(&self);
}

impl AppCache {
    pub(crate) fn snapshot(&self) -> Vec<Application>;
    pub(crate) fn refresh(&self) -> Result<DiscoveryDiagnostics, DiscoveryError>;
}

impl SettingsStore {
    pub(crate) fn decorate_applications(&self, applications: &mut [Application]);
    pub(crate) fn update_user_settings(
        &self,
        update: SettingsUpdate,
        cache: &AppCache,
    ) -> Result<(), SettingsError>;
    pub(crate) fn increment_use_count(
        &self,
        app_id: &str,
        cache: &AppCache,
    ) -> Result<(), SettingsError>;
    pub(crate) fn snapshot(&self) -> Settings;
}

impl ValidationStore {
    pub(crate) fn record(&self, event: ValidationEvent) -> Result<(), ValidationError>;
    pub(crate) fn clear_daily_counts(&self) -> Result<(), ValidationError>;
}

pub(crate) fn choose_export_destination(
    owner: HWND,
) -> Result<Option<ExportDestination>, ExportError>;

pub(crate) fn write_validation_export(
    destination: ExportDestination,
    settings: &SettingsStore,
    validation: &ValidationStore,
) -> Result<(), ExportError>;
```

`SettingsStore::snapshot` 已存在但当前仅在测试编译。Task 5 只移除该方法的 `#[cfg(test)]`，不改变
锁或持久化行为；command 随后映射为不含 `useCounts` 的用户设置 DTO。其余 Task 4 服务文件不因
command wrapper 改变接口。

## Command 合同

`src-tauri/src/commands.rs` 拥有全部 IPC DTO、固定 command 错误和八个 wrapper。Tauri 注入的
`State`、`WebviewWindow` 或 `AppHandle` 不属于前端参数。

| Command | 前端输入 | 返回 | 规则 |
|---|---|---|---|
| `search_apps` | `query`, `invocationId`, `querySequence` | `SearchResponse | null` | 只发布最新有效查询 |
| `execute_result` | `requestId`, `resultId` | `ExecuteOutcome` | 只执行 registry 中的可信动作 |
| `load_settings` | 无 | `SettingsView` | 投影全部当前 cache 应用，不返回 `useCounts` 或路径 |
| `save_settings` | 一个 `UserSettingsUpdate` | 空成功 | 转为现有 `SettingsUpdate` 后由 store 验证 |
| `rescan_apps` | 无 | 空成功 | blocking worker 中刷新唯一 cache |
| `export_validation_data` | 无 | `ExportOutcome` | main thread chooser，blocking writer |
| `clear_validation_data` | 无 | 空成功 | 只调用 `clear_daily_counts` |
| `hide_launcher` | 无 | 空成功 | 先清 registry，再隐藏调用方 main window |

读取与保存使用两个独立 DTO：

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppAliasTarget {
    app_id: String,
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsView {
    hotkey: String,
    autostart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    research_id: Option<String>,
    applications: Vec<AppAliasTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserSettingsUpdate {
    hotkey: String,
    autostart: bool,
    research_id: Option<String>,
    aliases: BTreeMap<String, Vec<String>>,
}
```

`load_settings` 在 main caller guard 成功后取得 `SettingsStore::snapshot` 与唯一
`AppCache::snapshot`，并按 cache snapshot 的既有顺序为每个当前应用生成一个 `AppAliasTarget`。
每个 target 都包含 opaque `appId`、`displayName`、可选安全 `icon` 和该 ID 在 store 中的 aliases；
没有旧 alias 的当前应用也必须以空 `aliases` 返回，两个同名应用必须以各自不同 `appId` 独立返回。
暂时不在 cache 的旧 aliases 不返回前端，仍只保留在 store 内。`SettingsView` 不包含 shortcut、
executable、其他路径、`useCounts` 或动作 payload。该过程只是 command DTO 投影，不修改
`SettingsStore` 接口、锁或持久化事务，也不调用 Windows adapter。
`SettingsView.researchId` 遵守冻结的 `researchId?: string`：`None` 时字段完全缺失，`Some(value)` 时
序列化为该精确字符串，不得输出 JSON `null`。`UserSettingsUpdate.researchId` 输入仍可把 missing 或
`null` 反序列化为 `None`。

`save_settings` 只在 `UserSettingsUpdate.aliases` map 中接受 opaque `appId`，不接受 `displayName` 或
`icon` 作为更新依据。Task 7 从 `SettingsView.applications` 为全部当前 target 构造该 map，包括空 aliases。
wrapper 先把 DTO 转为现有
`SettingsUpdate`，再由 store 对每个 key 执行固定格式验证和当前 `AppCache` 成员验证；任一伪造、
格式错误或未知 key 使整个设置更新失败且不修改状态。store 继续从旧 aliases 克隆 candidate，只对
当前 cache 应用输入并隐式保留暂时缺失应用的旧 aliases。因此由合法 `SettingsView` 构造的完整
`UserSettingsUpdate` 可以被保存，不会丢失未投影的旧 alias 或内部 use counts。

结果 DTO 保持 Task 2 冻结结构，搜索和执行 command 不返回或接受 `appId`。只有 settings read DTO 的
`applications` 和 save DTO 的 aliases map 可以包含 `appId`；它只用于区分当前设置目标和
`SettingsStore::update_user_settings`，永远不能进入 `ResultAction`、Windows adapter
或启动/激活定位。前端永远不能提交路径、PID、HWND、Shell operation、命令行参数、工作目录或任意
payload；`requestId + resultId` 是唯一应用动作输入。

`ExecuteOutcome` 固定为：

```rust
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum ExecuteOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested { message: &'static str },
}
```

`ExportOutcome` 只有 `Exported` 与 `Cancelled`。cancel 是成功结果，不是错误。

command error DTO 只允许以下固定 code，并为每个 code 配一个固定 message：

```text
invalidCaller
staleRequest
unknownResult
applicationEntryUnavailable
settingsFailed
validationFailed
windowFailed
scanFailed
scanWorkerFailed
mainThreadDispatchFailed
exportFailed
exportWorkerFailed
```

底层 `RegistryError`、`SettingsError`、`ValidationError`、`DiscoveryError`、`ExportError` 和 Tauri
dispatch/join 错误只映射到上述类别，不序列化或拼接底层值。

## Production handler 接线

`lib.rs` 在非 `test-instrumentation` 构建中创建并 `manage` 唯一
`ResultRegistry::default()`，与 Task 3 的唯一 `Arc<AppCache>`、Task 4 的唯一 `SettingsStore` 和
`ValidationStore` 一起供 command 读取。八命令只在该 production 分支通过一个
`tauri::generate_handler!` 注册，集合与现有 `build.rs` / `main.json` 精确一致。

八个 command 都注入调用方 `WebviewWindow`，并把同一个 crate-private main-label guard 作为函数体
第一步。guard 只接受精确 label `main`；非 main 调用返回 `invalidCaller`。任何 command 都不得在
guard 成功前读取 registry/cache/store、调用 Windows adapter、派发 blocking/main-thread 工作或改变
窗口。capability 是第一道限制，该 Rust guard 是独立的第二道限制。

`test-instrumentation` 构建继续只注册现有 `security_probe::load_settings`。production
`commands::load_settings` 不得进入该 handler，也不得改变 probe command 的名称、返回值或结果通道。
`commands` 模块可以为单元测试编译，但 feature-only 产物不得暴露 production 八命令。Task 6 后续从
同一个 managed registry 调用 `on_show`，不得替换 registry 实例。

## 搜索数据流

`search_apps` 按以下顺序执行：

1. 先调用 `ResultRegistry::begin_query(invocationId, querySequence)`。旧 invocation、非递增 sequence
   或已隐藏 generation 立即返回 `null`，不读取 cache。
2. 克隆唯一 `AppCache` 的当前 snapshot。
3. 调用唯一 `SettingsStore::decorate_applications`，只装饰该克隆。
4. 复用现有 `apps::rank`；空 query 返回空 items，非空 query 最多 20 项。
5. 复用现有 `apps::registry_entry`，把私有 `appId`、`.lnk` 和可选 executable 留在 Rust action。
6. 只通过 `publish_if_latest` 发布；查询期间出现更新查询或隐藏时返回 `null`，不替换当前结果。

搜索不读磁盘、不触发 rescan、不记录 query、应用名或路径。初始后台扫描尚未产生 snapshot 时，
有效查询可以发布空结果；后续查询自然读取刷新后的 snapshot。

## Windows 进程唯一性

采用方案 A'。`windows` 直接依赖显式声明产品代码实际使用的三个 feature：

```text
Win32_System_Diagnostics_ToolHelp
Win32_System_Threading
Win32_UI_WindowsAndMessaging
```

只有 `Win32_System_Diagnostics_ToolHelp` 是当前编译图的新 feature。另两个目前由 Tauri/tao
间接启用，但产品代码直接使用其 API，不能依赖传递 feature 偶然保留。不得增加
`Win32_System_ProcessStatus`、新 crate、版本或 lockfile 变化。

进程判定固定为：

1. `ResultAction.executable == None` 时不枚举进程，直接启动可信 `.lnk`。
2. 使用 `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)` 枚举 PID。snapshot handle 由 RAII
   关闭。
3. 先以 `PROCESSENTRY32W.szExeFile` 和可信 executable basename 做 ordinal-ignore-case 预筛选。
4. 只对同 basename 候选以 `PROCESS_QUERY_LIMITED_INFORMATION` 调用 `OpenProcess`，并用
   `QueryFullProcessImageNameW` 取得 Win32 full path。process handle 由 RAII 持有到激活决策结束。
5. full path 使用 Windows ordinal-ignore-case 比较。不得 `canonicalize`、读取目标 metadata、遍历
   目标目录、解析网络位置、展开环境变量或接受前端路径。
6. 完整枚举后只有精确一个 full-path 匹配 PID 才进入窗口枚举。零匹配、多匹配、snapshot/枚举
   不完整，或任一同 basename 候选无法打开/查询，都标记为 indeterminate 并走可信 `.lnk` 回退。

同 basename 候选的失败不能被忽略，否则剩余一个 PID 会被错误声明为唯一。对不同 basename
进程不打开 handle。目标在枚举期间退出、PID/window race 或查询 buffer 无法得到完整路径，都不得
降低为“唯一”；它们只会禁止激活并进入正常启动回退。

拒绝方案 B：不手写 ToolHelp extern、结构体或 ABI。拒绝方案 C：不以可见窗口集合替代全部匹配
进程的唯一性判断，因为无窗口的第二个匹配进程仍会使映射不唯一。

## 窗口选择与系统动作

只有唯一进程映射成功时才调用 `EnumWindows`。Windows 按 Z-order 枚举 top-level windows；候选
必须同时满足：

- `GetWindowThreadProcessId` 等于唯一 PID；
- `IsWindowVisible` 为真；
- `GetWindow(hwnd, GW_OWNER)` 为空；
- extended style 不含 `WS_EX_TOOLWINDOW`。

callback 第一次遇到满足条件的 HWND 时只记录它，后续候选不得替换；callback 始终返回 `TRUE`，
继续枚举到 Windows 报告完整成功。禁止用返回 `FALSE` 表示“已经找到”，因为这会使
`EnumWindows` 自身返回零并与真实失败混淆。只有完整枚举成功且没有任何目标 PID 相关属性处于
不确定状态时，才使用记录的第一个 HWND。没有候选、窗口在枚举中消失、目标窗口属性无法安全确认
或 `EnumWindows` 失败时不猜测窗口，直接启动可信 `.lnk`。

对候选 HWND 只调用一次 `SetForegroundWindow`：

- 返回真：`ActivationRequested`，不轮询窗口、不再启动。
- 返回假：视为 Windows 明确拒绝，调用可信 `.lnk`；成功返回
  `ActivationRefusedLaunchRequested` 和固定提示。

正常启动只调用 `ShellExecuteW` 打开 registry action 中的 `.lnk`，operation 使用系统默认 open，
不传参数和工作目录。返回值大于 32 表示请求已交给 Windows，映射为 `LaunchRequested`；不等待新
窗口出现。失败返回固定“应用入口不可用，请重新扫描”类别，不预先探测目标，不泄露路径。

Task 5 不提升权限、不注入输入、不 attach thread input、不结束进程、不轮询 500 ms，也不验证目标
应用最终获得焦点。

## 执行、失效与计数顺序

Task 5 在 `commands.rs` 生产并冻结唯一 crate-private 隐藏接口：

```rust
pub(crate) fn clear_and_hide(
    registry: &ResultRegistry,
    window: &WebviewWindow,
) -> Result<(), CommandError> {
    clear_and_hide_with(
        || registry.hide_and_clear(),
        || window.hide().map_err(|_| ()),
    )
}

fn clear_and_hide_with<C, H>(clear: C, hide: H) -> Result<(), CommandError>
where
    C: FnOnce(),
    H: FnOnce() -> Result<(), ()>,
{
    clear();
    hide().map_err(|_| CommandError::window_failed())
}
```

crate-private helper 不接受前端参数，并只通过该 private closure seam 固定先清空并失活同一 managed
registry，再隐藏调用方已验证的精确 `main` window。`execute_result`、`hide_launcher` 以及 Task 6 的失焦、关闭、modal 恢复、show/focus/emit 失败
路径都必须复用该函数；不得保留直接 clear 后再调用 helper 的双 clear 路径。

`execute_result` 先调用 `ResultRegistry::resolve`。stale 或 unknown ID 返回固定错误，Windows adapter、
stores 和窗口均不被调用。

系统动作失败时保留 registry 和窗口，不记录事件、不增加使用次数。系统动作成功后顺序固定为：

1. 立即调用唯一 `clear_and_hide(registry, window)` 精确一次。helper 先使 mapping 失效，再尝试物理
   hide；保存可选 `windowFailed`，但不得早退、补偿重显或再次调用 helper。
2. 按 outcome 调用一次 `ValidationStore::record`，保存可选 `validationFailed` 但不提前返回：
   - `LaunchRequested` -> `ValidationEvent::LaunchRequested`
   - `ActivationRequested` -> `ValidationEvent::ActivationRequested`
   - `ActivationRefusedLaunchRequested` ->
     `ValidationEvent::ActivationRefusedLaunchRequested`
3. 无论 helper 或 validation 是否成功，都调用一次
   `SettingsStore::increment_use_count(trusted_app_id, shared_cache)`，保存可选 `settingsFailed`。
4. 全部步骤完成后按固定业务优先级
   `validationFailed > settingsFailed > windowFailed` 返回一个错误；三者均无错误才返回已确定的
   `ExecuteOutcome`。该优先级与错误实际发生时间无关。

系统动作成功后，registry 在任何持久化前已经失效，系统动作、helper、validation 和 settings 都只调用
一次。window hide 失败时 registry 仍 inactive/empty；validation/settings 失败时物理 hide 已被尝试且
不补偿重显。只有 hide 本身失败时窗口才可能保持可见。两个 store 不同时持锁；可能出现
一个写入成功而另一个失败的明确部分状态，但不会用跨 store 事务或补偿重试扩大实现。

`LauncherInvoked` 由 Task 6 在真实 show 生命周期记录，Task 5 不伪造该事件。

## 编译目标与 warning 合同

normal production 不得存在 module-wide `allow(dead_code)` 或 `allow(unused_imports)`。`atomic_file`、
`apps`、`model`、`result_registry`、`session_marker`、`settings`、`validation_data`、
`validation_export` 与 `commands` 在 normal production 以普通模块编译；feature-only
`test-instrumentation` 非测试产物只编译 probe 所需代码，不编译这些产品模块。单元测试仍编译全部产品
模块。因此产品模块声明统一使用精确 `#[cfg(any(test, not(feature = "test-instrumentation")))]`，不绑定
任何 lint suppression；probe handler 仍只注册 `security_probe::load_settings`。

Task 6 尚未消费的五个逻辑 API 只允许以下 item-level 临时属性：

```rust
#[cfg_attr(
    all(not(test), not(feature = "test-instrumentation")),
    allow(dead_code)
)]
```

精确 item 为 `ResultRegistry::on_show`、`read_marker_for_clean`、
`ValidationEvent::LauncherInvoked`、`ValidationError::SessionOwnershipLost`、
`ValidationStore::mark_clean_exit` 及其私有 `mark_clean_exit_with` seam。不得扩大到 impl、enum、module 或
crate；不得增加 `unused_imports` 例外。Task 6 真实接线消费这些 item 时必须逐项删除对应临时属性。

## 其他 wrapper 的线程边界

`rescan_apps` 是零参数 async command。它只 clone 唯一 `Arc<AppCache>`，在
`tauri::async_runtime::spawn_blocking` 内调用 `refresh`，并等待 worker 完成。COM 初始化与释放因此
都在 discovery worker 上。失败保留上一份 snapshot；join 失败与 discovery 失败使用不同固定类别。

`export_validation_data` 是零业务参数 async command，并拒绝非 `main` 调用窗口。它使用现有
`tauri::async_runtime::channel(1)` 接收 main-thread closure 的结果，不增加 Tokio 直接依赖：

1. `WebviewWindow::run_on_main_thread` 内取得 main HWND 并调用一次
   `choose_export_destination(HWND)`。
2. `None` 直接返回 `Cancelled`，不派发 writer。
3. `Some(ExportDestination)` 才把 owned destination 与 cloned `AppHandle` 移入
   `spawn_blocking`；worker 从 app handle 取得 managed `SettingsStore` / `ValidationStore`，调用一次
   `write_validation_export`。
4. 成功返回 `Exported`。dispatcher、channel、join 或 export service 失败都映射为固定无路径错误。

dialog 显示期间不持 store 锁。前端不能传 owner、path、filename 或 payload。

`hide_launcher` 同样拒绝非 `main` 调用窗口，然后只调用一次统一 `clear_and_hide`；hide 失败时 registry
仍为空。
它不调用 `mark_clean_exit`。`clear_validation_data` 只清 daily counts，不删除 session marker。

## 固定失败语义

| 条件 | 结果 | registry / window |
|---|---|---|
| 任一 command 的 caller 不是 `main` | `invalidCaller` | 零 state 访问、零副作用 |
| 旧 invocation、旧 sequence、隐藏中的查询 | `search_apps -> null` | 不发布 |
| stale / unknown result ID | 固定 command error | 不调用 adapter，保持窗口 |
| 零/多匹配进程或进程判定 indeterminate | 尝试可信 `.lnk` | 启动成功后按正常成功流程 |
| 唯一进程但无合格窗口或窗口枚举失败 | 尝试可信 `.lnk` | 同上 |
| `SetForegroundWindow == false` | 尝试可信 `.lnk` 并返回 refusal outcome | 启动成功后按成功流程 |
| `ShellExecuteW` 失败 | 固定 application-entry error | 不计数，保持 registry / window |
| 成功动作后的 validation/settings 失败 | 完成全部步骤并按固定业务优先级返回 | helper 已调用，registry 已失效且 hide 已尝试一次 |
| 成功动作后的 hide 失败 | 固定 window error | registry 已失效，窗口保持 |
| rescan discovery 失败 | 固定 scan error | 旧 cache 保持 |
| export cancel | `Cancelled` | 不启动 writer |
| export service/worker 失败 | 固定 export error | 不返回路径 |

错误 DTO 只包含固定 code/message，不拼接 query、ID、应用名、PID、HWND、HRESULT、路径或系统错误文本。
日志遵守同一限制。

## Security trust-input change

Task 5 不能继承只批准给 Task 4C 的 trust-input 零变化例外。本设计要求独立安全设计/计划和代码复审
checkpoint，覆盖 Cargo feature、生产 command wiring 与 native adapter 整体。

实现阶段允许变化的精确产品文件为：

```text
src-tauri/Cargo.toml
src-tauri/src/lib.rs
src-tauri/src/commands.rs
src-tauri/src/apps/action.rs
src-tauri/src/apps/windows_backend.rs
src-tauri/src/apps/mod.rs
src-tauri/src/settings.rs
src-tauri/src/result_registry.rs
src-tauri/src/session_marker.rs
src-tauri/src/validation_data.rs
```

其中 `Cargo.toml`、`lib.rs` 和新增生产 command 源 `commands.rs` 属 security trust input；
`commands.rs` 必须加入 Task 5 获批 trust manifest。`apps/windows_backend.rs` 虽不注册 Tauri 权限，
仍按安全敏感 native adapter 做代码级复审。

以下文件必须相对 Task 5 implementation/trust baseline 保持 byte-identical：

```text
src-tauri/build.rs
src-tauri/Cargo.lock
src-tauri/capabilities/main.json
src-tauri/permissions/**
src-tauri/tauri.conf.json
src-tauri/tauri.security-probe.conf.json
src-tauri/src/security_probe.rs
security-probe.html
src/security-probe.ts
scripts/check-security-config.ps1
scripts/test-security-config.ps1
scripts/build-security-probe.ps1
scripts/test-security-probe.ps1
package.json
package-lock.json
vite.config.ts
tsconfig.json
index.html
```

`build.rs` 和 `main.json` 已预声明精确八命令，不需要修改。不得新增通用 `run`、`open`、`shell`、
`readFile`、`writeFile`、`request` 或路径 command，不得扩大 window selector 或 capability。

Task 5 设计/计划复审必须另行裁决实现是否可以得到
`TaskCodeGo + ReleaseSecurityBlocked`；当前 Task 4C 解耦决定不自动授权。无论任务代码状态如何，
`ReleaseSecurityBlocked / SEC-RUNTIME-PROBE-001` 持续生效。

## 共享文件与 Task 4C 依赖

- `src-tauri/src/lib.rs` 同时是 Task 4 setup 所有者和 frozen trust input。Task 5 只能基于获批 Task 4C
  集成提交修改，不能从当前 main 文档分支提前形成代码 patch。
- `src-tauri/Cargo.toml` 在 Task 4C 分支已包含 dialog 所需 feature。Task 5 只能在集成结果上增加 A'
  的三个显式 feature，不能覆盖或重排 Task 4C 依赖。
- `src-tauri/src/settings.rs` 只移除现有 `snapshot` 的测试条件，不能改变 Task 4A 持久化事务；当前
  application/alias DTO 投影只存在于 `commands.rs`。
- `src-tauri/src/commands.rs` 生产唯一 `pub(crate) clear_and_hide(registry, window)`；Task 6 只消费该接口，
  不复制 registry/window 隐藏顺序。
- `result_registry.rs`、`session_marker.rs` 与 `validation_data.rs` 只增加上述精确 Task 6 item-level 临时属性；
  不改变行为。`validation_export.rs` 只通过批准接口消费，不修改。
- `model.rs`、`apps/cache.rs`、`apps/discovery.rs` 和 `apps/rank.rs` 只复用，不修改。
- `src/protocol.ts` 与全部前端文件留给 Task 7。

## 测试合同

实施计划必须先写失败测试并至少覆盖：

1. 搜索的 old invocation、sequence 竞争、hide 竞争、空 query、20 项上限和 registry 私有 action。
2. stale/unknown result，以及 settings aliases 中伪造、格式错误或未知 `appId`，不触达任何 Windows
   adapter seam；非法 alias update 不修改 settings。
3. ToolHelp 零/一/多 full-path 匹配；同 basename query 失败使结果 indeterminate；不同 basename 不打开。
4. ordinal-ignore-case basename/full-path 比较且没有 canonicalize、目标 metadata 或目录遍历。
5. snapshot/process handle 在成功、错误和 race 分支各关闭一次；唯一 process handle 持有到激活完成。
6. EnumWindows 只接受唯一 PID 的 visible、unowned、non-tool window；第一次命中后仍以 `TRUE`
   完成枚举，后续候选不替换第一个 Z-order 候选，任何相关不确定性都放弃激活。
7. 进程退出、PID/window race、EnumWindows 失败和窗口消失都不激活错误窗口，而是可信启动回退。
8. activation true 不启动；false 只启动一次并返回 refusal outcome；launch failure 不计数、不隐藏。
9. 成功 action 立即调用唯一 helper 一次并使 registry 失效；helper 内 registry-before-window，window
   先失败后 validation/settings 仍各执行一次，系统动作不重做，所有错误组合按固定业务优先级
   `validationFailed > settingsFailed > windowFailed` 返回。已有 active mapping 在 hide/focus/show 失败
   后立即 inactive/empty，且没有双 clear/double generation。
10. 三种成功 outcome 到四个批准计数字段的映射，且不记录失败动作或 query 内容。
11. rescan 只在 blocking worker 调用 refresh，join/discovery 失败保持旧 cache。
12. export chooser 在 main thread；cancel 不派发 writer；confirm 后 snapshot/write 全在 blocking worker。
13. store 含 populated-current/absent aliases，cache 顺序固定为 empty、同名 A、同名 B 时，load 按同一
    顺序返回每个 target 的精确 `appId/displayName/icon/aliases`：A 保留 seed alias，empty/B 为空，absent
    不输出。Task 7 据此构造完整 aliases map 并保存后，A 与 absent aliases 及全部 use counts 原样保留。
    `researchId=None` 完全省略字段，`Some` 输出精确字符串；DTO 不序列化 shortcut/executable/path 或
    `useCounts`，伪造/未知新 key 仍整体失败；clear 不破坏 session marker。
14. 八 command 与 `build.rs` / capability 精确一致；每个 command 都在任何 state 访问或副作用前以
   同一 guard 拒绝非 main window，禁止前端 path/PID/HWND 输入。
15. A' 只有一个新 windows feature；Cargo.lock 和全部冻结 trust 文件 byte-identical。
16. source oracle 拒绝所有 module-wide warning suppression，认证 probe-only 产品模块 cfg 与上述六处精确
    item-level 属性；default/all-features test、check 与 Clippy `-D warnings` 都通过。

真实 Windows smoke 只在实施计划 Go 后对人工选择的测试应用执行，不在自动测试中任意启动应用，
不运行失败 security probe worktree，也不声称 runtime ACL positive evidence 已通过。

## 设计完成门禁

当前行为设计已 Go。代码复审只授权从 clean 产品 HEAD
`f204c0c45050de979beb7311cf52a3e5c2c57ee8` 执行本节精确 lint/cfg corrective TDD；不得扩展行为或安全
边界。

Task 5 实施开始必须同时满足：

1. 本设计与同步实施计划保持上述复审授权的精确 lint/cfg 边界；
2. corrective TDD 先以 source oracle 产生预期 RED；
3. corrective TDD 只在现有 `codex/foundation-task-5` implementation worktree 继续，且该分支仍以
   `2788e9a275e0406e70d7597a4a78da274d8c55c0` 为 implementation/trust baseline；
4. 精确十文件 allowlist、frozen inventory 与 security trust checkpoint 保持不变；
5. 修正后重新运行完整验证和 trust checkpoint，并再次请求 `TaskCodeGo + ReleaseSecurityBlocked`。

任一条件未满足时立即停止并回传审核线程。不得移动现有安全 tag、清理失败 probe worktree、合并、
推送、签名、试用或发布。
