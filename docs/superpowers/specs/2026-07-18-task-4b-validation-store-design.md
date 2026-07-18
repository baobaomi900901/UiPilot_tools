# Foundation Task 4B 验证计数与会话对账设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 前置：Task 4A 原子文件协议已批准
- 影响范围：`ValidationStore`、日级聚合计数、会话 marker 和 exactly-once 对账

## 目标与边界

Task 4B 只记录本地日级聚合数据并维护一个 open-session marker。它不安装 `SetUnhandledExceptionFilter`，不写 confirmed-crash marker，不调用 `catch_unwind` 生成崩溃证据，也不导出 `hostCrashes`。

`uncleanSessions` 只表示上一个进程 session 没有完成正常清理。它不能自动归类为宿主崩溃。需求中的宿主崩溃退出标准继续由 WER、Application Error、crash dump 或主持记录在研究流程中分类。

Task 4B 不注册 Tauri command，不接线 Task 5 动作，也不接线 Task 6 的托盘、Tauri run event 或 Windows session-end 消息。它只提供 Task 5/6 后续调用的 crate-private 服务。

## 文件与唯一所有者

预计实现文件：

```text
src-tauri/src/validation_data.rs
src-tauri/src/session_marker.rs
src-tauri/src/lib.rs
```

进程只创建一个 `ValidationStore`。它同时拥有 `validation-data.json`、`validation-data.json.backup`、`open-session.json` 和一个 `Mutex<ValidationState>`；不会另建第二个计数 store 或 session manager。

Task 4B 复用 Task 4A 已批准的 `atomic_file.rs`。所有路径由 Rust 从 Tauri application data directory 构造，前端不能提供。

## 本地状态与导出状态分离

本地文件结构固定为：

```rust
const VALIDATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyCounts {
    pub(crate) launcher_invocations: u64,
    pub(crate) application_launch_requests: u64,
    pub(crate) activation_successes: u64,
    pub(crate) activation_refusals: u64,
    pub(crate) unclean_sessions: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationState {
    schema_version: u32,
    daily_counts: BTreeMap<String, DailyCounts>,
    last_reconciled_session_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionMarker {
    schema_version: u32,
    session_id: String,
    local_date: String,
}
```

导出 DTO 不复用 `ValidationState`，只包含 schema version、来自 `SettingsStore` 的可选 research ID 和 `dailyCounts`。`lastReconciledSessionId`、当前 `sessionId`、marker 字段和文件名永远不导出。

## 日期与事件合同

生产日期通过现有 `windows` crate 调用 `GetLocalTime`，格式严格为本地 `YYYY-MM-DD`。测试向内部纯函数注入固定日期；生产 API 不接受前端日期。

事件枚举固定为：

```rust
pub(crate) enum ValidationEvent {
    LauncherInvoked,
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested,
}
```

映射固定为：

- `LauncherInvoked`：`launcherInvocations += 1`。
- `LaunchRequested`：`applicationLaunchRequests += 1`。
- `ActivationRequested`：`activationSuccesses += 1`。
- `ActivationRefusedLaunchRequested`：`activationRefusals += 1` 且 `applicationLaunchRequests += 1`。

所有加法使用 `checked_add`。任一字段溢出使整个事件失败，磁盘和内存保持旧值。失败动作不调用 `record`，该调用边界由 Task 5 负责。

## 单锁验证事务

```rust
pub(crate) struct ValidationStore {
    paths: ValidationPaths,
    state: Mutex<ValidationState>,
}

impl ValidationStore {
    pub(crate) fn record(&self, event: ValidationEvent) -> Result<(), ValidationError>;
    pub(crate) fn clear_daily_counts(&self) -> Result<(), ValidationError>;
    pub(crate) fn export_snapshot(&self) -> ValidationCountsSnapshot;
    pub(crate) fn reconcile_and_open_session(&self) -> Result<(), ValidationError>;
    pub(crate) fn mark_clean_exit(&self) -> Result<(), ValidationError>;
}
```

`record` 和 `clear_daily_counts` 每次只获取一次 `Mutex<ValidationState>`，在锁内完成克隆、checked mutation、序列化、backup/current 持久化和内存交换。写失败不修改内存。

`export_snapshot` 只在短临界区克隆 `schemaVersion + dailyCounts`。它不读取 `SettingsStore`，避免跨 store 嵌套锁。

`clear_daily_counts` 只清空 `dailyCounts`；必须保留 `lastReconciledSessionId` 和当前 open-session marker，防止清除操作破坏 exactly-once 语义。

validation current/backup 的加载、损坏隔离和默认恢复完全复用 Task 4A 的规则。权限、磁盘和原子移动错误不能降级为 defaults。

## Session ID 与 marker

生产 `sessionId` 使用 Windows CNG 系统 RNG 生成 16 字节，并编码为 `session-` 加 32 位小写十六进制。它只要求进程 session 唯一，不承载用户、设备或时间信息。测试向内部函数注入固定 ID。

`open-session.json` 通过同目录 temp、`sync_all` 和 `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` 写入，不创建 backup。marker JSON 或 schema 损坏时隔离为唯一 `.invalid-*`，记录固定“会话证据无效”类别，不增加 `uncleanSessions`，然后打开新 session。错误和日志不包含 marker 内容、session ID、日期或路径。

## Exactly-once 启动对账

`reconcile_and_open_session` 在启动 setup 阶段、任何验证事件和命令暴露之前执行。算法固定为：

1. 在锁内读取已验证的 `ValidationState` 和旧 `open-session.json`。
2. 若旧 marker 存在且 `sessionId != lastReconciledSessionId`，在 marker 的本地日期上对 `uncleanSessions` 做 checked increment，同时把 `lastReconciledSessionId` 设为旧 ID，并原子持久化 candidate validation state。
3. 若旧 marker 的 ID 已等于 `lastReconciledSessionId`，不再增加计数，也不重写相同 validation state。
4. 生成新 session ID 和当前本地日期，把新 marker 原子替换到 `open-session.json`。
5. 新 marker 替换成功后返回；任何更早失败都不暴露半初始化的 store。

本地验证会话的起点固定为第 4 步成功。该步骤成功前，应用不得注册命令、显示主窗口或记录验证事件；此前进程失败属于启动初始化失败，不由 open-session marker 计入 `uncleanSessions`，只能按外部 WER、Application Error、crash dump 或主持记录分类。

该顺序固定以下恢复行为：

- validation state 持久化前崩溃：旧 marker 与旧 state 保留，下次启动增加一次。
- state 持久化后、新 marker 替换前崩溃：旧 marker 保留，但 last ID 已匹配，下次启动不重复增加。
- 新 marker 替换后崩溃：下次启动看到新 ID，为新 session 增加一次。
- 已正常清理 marker：下次启动没有旧 session，不增加。

这是一项跨文件幂等协议，不宣称两个文件构成单个原子事务；`lastReconciledSessionId` 使所有可观察中间状态都可重放而不重复计数。

## 正常退出边界

`mark_clean_exit` 读取当前 marker，只有其结构有效时才删除。不存在 marker 视为幂等成功；权限或删除错误返回固定类别。

Task 6 拥有生产接线：

- 主窗口隐藏和 CloseRequested 不调用 `mark_clean_exit`。
- `ExitRequested` 与 `WM_QUERYENDSESSION` 不删除 marker。
- 托盘退出、真正的 `RunEvent::Exit` 和 `WM_ENDSESSION(wParam != 0)` 调用 `mark_clean_exit`。

Task 4B 只测试服务状态转换；不创建托盘、不安装窗口消息 hook，也不修改 Task 5 command registry。

## 并发与锁顺序

`ValidationStore` 不在持锁时调用 `SettingsStore` 或 `AppCache`。Task 4C 导出先分别取得短生命周期克隆，再序列化和写文件，因此不存在 Settings -> Validation 或 Validation -> Settings 的嵌套锁。

并发事件通过单一验证锁串行。两个同日事件不会从同一个旧值分别克隆；清除与记录也有确定的锁获取顺序，后取得锁的操作基于前一个已持久化状态执行。

## 测试合同

Task 4B 进入 TDD 前，实施计划至少覆盖：

1. 同日/跨日计数、全部事件映射和 checked overflow。
2. 两个并发事件不丢失，内存与 current 一致。
3. record、clear 的每个 I/O 失败点保持旧状态。
4. validation current/backup 损坏恢复和严格日期 key 验证。
5. stale marker 正常增加一次。
6. 在 validation 持久化前后、新 marker 替换前后模拟崩溃，反复启动仍 exactly once。
7. malformed marker 被隔离但不冒充崩溃或计数。
8. clear 只清 daily counts，保留 last reconciled ID 和当前 marker。
9. 强制终止模拟留下 marker并在下次启动增加 `uncleanSessions`。
10. 导出 snapshot 不含 session ID、last reconciled ID、路径或任何原始输入。
11. 代码和产物不包含 `SetUnhandledExceptionFilter`、confirmed-crash marker 或 `hostCrashes` 字段。

## 非目标与后续所有权

- Task 4C 负责把验证 snapshot 与 research ID 组合为导出文件。
- Task 5 负责在动作结果确定后调用 `record`，并注册验证 commands。
- Task 6 负责唤起事件和正常退出生命周期接线。
- 原生确认崩溃若未来确有价值，必须单独建立 Spike，不能回填本任务。

本设计批准前不得更新 Task 4B 实施计划或进入 TDD。
