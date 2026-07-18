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
src/protocol.ts
```

公开的 crate-private 接口固定为：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExportStatus {
    Exported,
    Cancelled,
}

pub(crate) fn export_validation_data(
    owner: HWND,
    settings: &SettingsStore,
    validation: &ValidationStore,
) -> Result<ExportStatus, ExportError>;
```

该函数没有 path、filename、payload 或任意 JSON 参数。Task 5 wrapper 也必须保持零参数，从单一 main window 取得 HWND 和 managed stores 后调用本服务。

`src/protocol.ts` 在 Task 4C 中只定义 `ExportStatus` 类型，不调用 invoke；实际 invoke 封装由 Task 5/7 的相应计划负责。

## 对话框线程与 COM

原生 adapter 使用 `IFileSaveDialog`，并固定在 Task 5 同步 command 所在的 Tauri main UI 线程调用。显示原生 modal dialog 是该同步命令唯一允许的阻塞操作；Task 5 不把它放入通用 blocking worker。

调用线程先执行 `CoInitializeEx(NULL, COINIT_APARTMENTTHREADED)`：

- `S_OK` 或 `S_FALSE` 都建立 RAII guard，并在同一线程配对一次 `CoUninitialize`。
- `RPC_E_CHANGED_MODE` 和其他错误返回固定 `ExportError::ComUnavailable`，不切换 apartment，不回退到 WebView 文件选择器。

`Show` 固定传入 main window HWND，使对话框拥有正确 owner。对话框至少设置：

```text
default extension: json
file type: JSON (*.json)
options: FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT | FOS_NOCHANGEDIR
```

`HRESULT_FROM_WIN32(ERROR_CANCELLED)` 映射为 `ExportStatus::Cancelled`；cancel 不读取 store snapshot、不创建 temp、不写文件。其他 HRESULT 只映射为固定类别，不输出原始 HRESULT 或路径。

## Snapshot 与锁边界

保存对话框显示期间不持有 `SettingsStore` 或 `ValidationStore` 的锁。

用户确认目标后，服务按以下顺序取得数据：

1. 调用 `settings.research_id()` 取得独立 `Option<String>` 克隆并释放设置锁。
2. 调用 `validation.export_snapshot()` 取得 `schemaVersion + dailyCounts` 克隆并释放验证锁。
3. 组合 `ValidationExport` 并序列化。
4. 写出文件。

两个 store 从不同时持锁。导出是用户确认后的一个时间点 snapshot；之后发生的事件留待下一次导出，不修改已经生成的文件。

## 导出 JSON 合同

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationExport {
    schema_version: u32,
    research_id: Option<String>,
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

research ID 使导出属于假名化数据。服务不添加用户姓名、设备 ID 或身份映射；身份映射仍由产品负责人单独保管。

## 导出文件写入

从 dialog 返回的 filesystem path 只驻留 Rust。服务在目标同目录创建唯一 sibling temp，使用 `create_new`、`write_all`、`sync_all` 并关闭句柄，再调用：

```text
MoveFileExW(temp, destination, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
```

该写入不创建 `.backup`；用户通过 `FOS_OVERWRITEPROMPT` 已确认覆盖。目标替换成功前，原目标文件保持不变。替换失败时 best-effort 删除本次 temp，并返回固定错误类别。

导出错误、日志和 TypeScript 响应不包含目标路径、文件名或临时文件名。服务不打开导出文件、不启动 Explorer，也不把路径写回设置或验证数据。

## 测试 seam

生产函数只组合三个私有步骤：dialog、snapshot、atomic export write。测试通过私有 `export_with(dialog, snapshot, writer)` 闭包 seam 注入结果，不新增 trait、运行时或测试依赖，也不把 seam 暴露为 Tauri command。

Task 4C 进入 TDD 前，实施计划至少覆盖：

1. cancel 返回 `Cancelled`，不取得 snapshot、不调用 writer。
2. confirm 后才取得 research ID 与 validation snapshot。
3. 输出 JSON 精确等于批准字段，明确不存在所有内部/敏感字段。
4. writer 失败返回固定类别，不泄露路径。
5. 目标 sibling temp、`sync_all` 和 MoveFileEx flags 的固定调用顺序。
6. COM S_OK/S_FALSE 的配对释放和 changed-mode 固定失败。
7. Show 使用 main HWND；cancel HRESULT 与普通错误分流。
8. TypeScript/command 输入不存在 path 或 payload。
9. Task 4C diff 中不存在 `#[tauri::command]`、`generate_handler` 变更、`commands.rs` 或 Task 5 action 文件。

真实 native dialog 的人工 smoke 归入 Task 5 命令接线验收；Task 4C 自动测试不弹出对话框。

## 非目标与后续所有权

- `clear_validation_data` 只是 Task 4B 的 store 方法，Task 5 才包装 command。
- Task 5 负责 command 注册、managed state 获取和 fixed AppError 转换。
- Task 7 负责 Export/Clear 按钮、cancel UI 和无障碍状态提示。
- Task 4C 不发送网络请求，也不实现自动上传。

本设计批准前不得更新 Task 4C 实施计划或进入 TDD。
