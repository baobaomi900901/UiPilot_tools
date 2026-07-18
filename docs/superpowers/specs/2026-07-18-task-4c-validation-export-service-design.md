# Foundation Task 4C 验证数据导出服务设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 前置：Task 4A 与 Task 4B 设计已批准
- 影响范围：crate-private 导出服务、Windows 原生保存对话框和导出文件边界

## 目标与边界

Task 4C 实现一个 crate-private Rust 服务：由 Windows 原生保存对话框取得目标路径，在用户确认后写出本地聚合验证数据。

Task 4C 不声明 `#[tauri::command]`，不修改 `invoke_handler`，不创建 `commands.rs`，也不实现 Task 5 的搜索、执行、隐藏或重扫命令。Task 5 后续只增加一个零参数 Tauri wrapper，并从 Rust 内部取得 main window owner；TypeScript 永远不传目标路径。

设置页面的 Export 点击行为属于 Task 7。Task 4C 只证明服务在被调用时的 dialog、cancel、snapshot 和 write 行为，不声称已经存在 UI 点击入口。

## 文件与接口

预计实现文件：

```text
src-tauri/src/validation_export.rs
src-tauri/src/lib.rs
```

公开的 crate-private 接口固定为：

```rust
pub(crate) struct ExportDestination(PathBuf);

pub(crate) fn choose_export_destination(
    owner: HWND,
) -> Result<Option<ExportDestination>, ExportError>;

pub(crate) fn write_validation_export(
    destination: ExportDestination,
    settings: &SettingsStore,
    validation: &ValidationStore,
) -> Result<(), ExportError>;
```

两个函数和 `ExportDestination` 类型均为 crate-private，但 tuple field 保持 module-private。只有 `validation_export.rs` 内的原生对话框 adapter 能构造该值；Task 5 和其他模块只能接收并移动，不能从 `PathBuf` 或前端数据构造。测试只允许在同模块内构造 fixture。Tauri command 没有 path、filename、payload 或任意 JSON 参数，并且必须保持零参数。

Task 4C 不修改 `src/protocol.ts`。Task 5 command wrapper 首次定义 Rust command 返回类型，Task 7 出现首个 TypeScript 消费者时再增加对应前端类型。

## 对话框线程与 COM

`choose_export_destination` 使用 `IFileSaveDialog`，并固定在 Tauri main UI 线程调用；UI 线程只执行原生 modal dialog 和取得用户选择的路径。Task 5 必须使用 async command：先通过 Tauri main-thread dispatcher 执行该函数；返回 `None` 时立即映射为固定 `Cancelled` command 结果，不得派发 writer；返回 `Some(destination)` 时才把 owned `ExportDestination` 和 cloned `AppHandle` 交给现有 `tauri::async_runtime::spawn_blocking`。worker 在内部取得 managed stores，调用一次 `write_validation_export` 完成 snapshot、序列化和落盘，成功后映射为固定 `Exported` command 结果。不新增运行时，也不把借用的 Tauri state 跨线程传递。

调用线程先执行 `CoInitializeEx(NULL, COINIT_APARTMENTTHREADED)`：

- `S_OK` 或 `S_FALSE` 都建立 RAII guard，并在同一线程配对一次 `CoUninitialize`。
- `RPC_E_CHANGED_MODE` 和其他错误返回固定 `ExportError::ComUnavailable`，不切换 apartment，不回退到 WebView 文件选择器。

`Show` 固定传入 main window HWND，使对话框拥有正确 owner。对话框至少设置：

```text
default extension: json
file type: JSON (*.json)
options: FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT | FOS_NOCHANGEDIR
```

`HRESULT_FROM_WIN32(ERROR_CANCELLED)` 映射为 `Ok(None)`；cancel 不读取 store snapshot、不创建 temp、不写文件。其他 HRESULT 只映射为固定类别，不输出原始 HRESULT 或路径。

`Show` 成功后取得路径的合同固定为：

1. 调用 `IFileSaveDialog::GetResult` 取得 `IShellItem`。
2. 只调用 `IShellItem::GetDisplayName(SIGDN_FILESYSPATH)`；禁止 `SIGDN_NORMALDISPLAY`、URL 或其他显示名称。
3. 调用成功取得 `PWSTR` 后，在任何指针检查或转换之前立即交给 module-private RAII guard；guard 在全部成功和错误路径的 `Drop` 中调用 `CoTaskMemFree` 一次。
4. 空指针、空字符串或无效 UTF-16 返回固定 `ExportError::InvalidDestination`。转换使用严格 UTF-16，不调用任何 lossy API。
5. 只有验证成功的 filesystem path 才被封装为 `ExportDestination`；路径、Shell display name 和原始指针不写日志或错误响应。

## Snapshot 与锁边界

保存对话框显示期间不持有 `SettingsStore` 或 `ValidationStore` 的锁。

用户确认目标后，blocking worker 按以下顺序取得数据：

1. 调用 `settings.research_id()` 取得独立 `Option<String>` 克隆并释放设置锁；若为 `None`，返回固定 `ExportError::MissingResearchId`，不取得 validation snapshot、不创建 temp、不写文件。
2. 调用 `validation.export_snapshot()` 取得 `schemaVersion + dailyCounts` 克隆并释放验证锁。
3. 以必需的 research ID 组合 `ValidationExport` 并序列化。
4. 在同一 blocking worker 中完成原子导出写入。

两个 store 从不同时持锁。导出是用户确认后的一个时间点 snapshot；之后发生的事件留待下一次导出，不修改已经生成的文件。

## 导出 JSON 合同

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExport {
    schema_version: u32,
    research_id: String,
    daily_counts: BTreeMap<String, DailyCounts>,
}
```

schema version 固定为 1。`DailyCounts` 只包含：

```text
launcherInvocations
applicationLaunchRequests
activationSuccesses
activationRefusals
uncleanSessions
```

导出不得包含 `hostCrashes`、session ID、last reconciled ID、marker、hotkey、autostart、aliases、useCounts、查询词、应用名称、快捷方式、可执行文件或任意路径。

research ID 使导出属于假名化数据，并且是成功导出的必需字段；设置中仍允许暂未配置。服务不添加用户姓名、设备 ID 或身份映射；身份映射仍由产品负责人单独保管。

## 导出文件写入

从 dialog 返回的 filesystem path 只驻留 `ExportDestination`，并移交 blocking worker。`write_validation_export` 消费该值，在目标同目录创建唯一 sibling temp，使用 `create_new`、`write_all`、`sync_all` 并关闭句柄，再调用：

```text
MoveFileExW(temp, destination, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
```

该写入不创建 `.backup`；用户通过 `FOS_OVERWRITEPROMPT` 已确认覆盖。目标替换成功前，原目标文件保持不变。替换失败时 best-effort 删除本次 temp，并返回固定错误类别。

导出错误、日志和 TypeScript 响应不包含目标路径、文件名或临时文件名。服务不打开导出文件、不启动 Explorer，也不把路径写回设置或验证数据。

## 测试 seam

dialog adapter 和 atomic export writer 各自保留一个私有闭包 seam，用于注入系统结果和 I/O 失败。不新增 trait、运行时或测试依赖，也不把 seam 暴露为 Tauri command。

Task 4C 进入 TDD 前，实施计划至少覆盖：

1. dialog cancel 返回 `None`；Task 5 合同要求在 `spawn_blocking` 前返回 `Cancelled`，不取得 snapshot、不调用 writer。
2. confirm 后才取得 research ID；缺少 ID 返回 `MissingResearchId`，不取得 validation snapshot、不创建文件。
3. 输出 JSON 必含 research ID 且精确等于批准字段，明确不存在所有内部/敏感字段。
4. writer 失败返回固定类别，不泄露路径。
5. 目标 sibling temp、`sync_all` 和 MoveFileEx flags 的固定调用顺序。
6. COM S_OK/S_FALSE 的配对释放和 changed-mode 固定失败。
7. Show 使用 main HWND；cancel HRESULT 与普通错误分流。
8. GetResult 后只请求 `SIGDN_FILESYSPATH`；`PWSTR` 在解析前由 guard 接管，成功、空指针、空字符串和无效 UTF-16 路径都精确释放一次。
9. writer 只接受字段私有的 `ExportDestination`；Task 5/前端不存在 path、filename 或 payload 输入。
10. Task 4C 不修改 `src/protocol.ts`，diff 中也不存在 `#[tauri::command]`、`generate_handler` 变更、`commands.rs` 或 Task 5 action 文件。
11. Task 5 后续集成测试必须证明 dialog 在 main UI 线程、cancel 不派发 worker，且 snapshot/序列化/`sync_all`/MoveFileEx 全部在 blocking worker。

真实 native dialog 的人工 smoke 归入 Task 5 命令接线验收；Task 4C 自动测试不弹出对话框。

## 非目标与后续所有权

- `clear_validation_data` 只是 Task 4B 的 store 方法，Task 5 才包装 command。
- Task 5 负责 async command 注册、command 返回类型、main-thread dialog 调度、`spawn_blocking` writer 调度、managed state 获取和 fixed AppError 转换。
- Task 7 负责 Export/Clear 按钮、cancel UI 和无障碍状态提示。
- Task 4C 不发送网络请求，也不实现自动上传。

本设计批准前不得更新 Task 4C 实施计划或进入 TDD。
