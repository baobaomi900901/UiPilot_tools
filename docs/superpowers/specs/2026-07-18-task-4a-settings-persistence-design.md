# Foundation Task 4A 设置持久化设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 影响范围：Windows MVP-A 的原子文件协议、`SettingsStore`、应用别名和 `useCounts`

## 目标与边界

Task 4A 只实现结构化设置的加载、验证、恢复和持久化。它不注册 Tauri command，不修改 invoke handler，不应用全局快捷键或开机启动副作用，也不实现搜索、执行、激活、隐藏或重新扫描命令。

可信快捷方式和可执行文件路径只来自进程内 `AppCache`。`settings.json` 不保存路径、应用名称或 `ResultAction`，磁盘中的 `appId` 不能被提升为启动能力。

Task 4A 创建一个可由 Task 4B 和 Task 4C 复用的 crate-private 原子字节文件 helper。该 helper 只接受宿主构造的固定路径，不接受前端路径。

## 文件与进程所有权

预计实现文件：

```text
src-tauri/src/atomic_file.rs
src-tauri/src/settings.rs
src-tauri/src/lib.rs
src/protocol.ts
```

Tauri application data directory 由 Rust 在 setup 阶段通过应用句柄获取。生产代码固定构造 `settings.json`、`settings.json.backup` 和同目录临时文件；TypeScript 不提供目录、文件名或完整路径。

进程只创建一个 `SettingsStore` 并交给 Tauri 管理。Task 3 已有的 `Arc<AppCache>` 保持唯一，Task 4A 不创建第二份缓存。

## 持久化结构

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) use_counts: BTreeMap<String, u64>,
}

pub(crate) struct SettingsUpdate {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
    pub(crate) aliases: BTreeMap<String, Vec<String>>,
}

struct SettingsState {
    value: Settings,
    current_is_valid: bool,
}

pub(crate) struct SettingsStore {
    paths: AtomicPaths,
    state: Mutex<SettingsState>,
}
```

默认值固定为 `Alt+Space`、autostart 关闭、无 research ID、空 aliases 和空 `useCounts`。

`SettingsUpdate` 永远不包含 `useCounts`。普通设置保存从锁内当前值克隆 candidate，只覆盖用户可编辑字段并保留全部 `useCounts`。因此前端 DTO 缺少计数字段不能清空最近使用次数。

## `appId` 与别名更新

`appId` 验证使用标准库实现，不增加 regex 依赖：长度固定为 68 字节，以 ASCII `app-` 开头，后续 64 字节全部为小写十六进制。

加载时，结构有效但当前 `AppCache` 中暂时不存在的 alias 或 use-count key 继续保留。装饰应用克隆时只复制当前缓存中存在的 ID；不存在的条目不能产生路径或动作。

保存 `SettingsUpdate` 时：

1. 输入 alias key 必须格式正确且存在于同一个 `AppCache` 快照；未知 key 使整个保存失败。
2. 从旧 aliases 克隆 candidate。
3. 对当前缓存中的每个应用，以输入值设置或删除 aliases。
4. 暂时不在缓存中的旧 aliases 原样保留。
5. `useCounts` 始终从旧值原样保留。

hotkey 和 autostart 在 Task 4A 中只持久化候选设置。Task 6 必须先成功应用对应系统副作用，再调用持久化方法；Task 4A 不调用插件。

## 单锁事务

所有设置变更使用同一个 `Mutex<SettingsState>`。每个方法只获取一次锁，并持有到以下步骤全部结束：

1. 读取旧值并验证可信 `appId`。
2. 使用 `checked_add` 或字段合并构造 candidate。
3. 序列化旧值与 candidate。
4. 完成 backup 和 current 的磁盘协议。
5. 将 candidate 赋给锁内内存值。

验证、`checked_add`、序列化、写入、同步或原子移动失败时，内存值保持旧状态。代码中不存在检查后解锁再写入的分支。

公开的 crate-private 接口固定为：

```rust
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
    pub(crate) fn research_id(&self) -> Option<String>;
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> Settings;
}
```

`research_id()` 只在短临界区内克隆一个字符串，不与 `ValidationStore` 同时持锁。

## Windows 原子文件协议

`atomic_file.rs` 只提供字节级 helper，JSON 序列化和结构验证仍由各 store 负责。所有 current、backup 和 temp 必须是同一目录中的 sibling，禁止 `MOVEFILE_COPY_ALLOWED`。

临时文件名由固定基础名、当前 PID 和进程内原子计数器组成；使用 `create_new` 防止覆盖现有文件。临时文件只包含即将持久化的结构化 JSON。

一次已有 current 的提交固定执行：

1. 把 candidate bytes 写入唯一 current temp，`write_all`、`sync_all` 并关闭句柄。
2. 把锁内旧值重新序列化为 previous bytes，写入唯一 backup temp，`write_all`、`sync_all` 并关闭句柄。
3. 调用 `MoveFileExW(backup_temp, backup, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`。
4. 调用 `MoveFileExW(current_temp, current, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`。
5. current 移动成功后立即交换锁内内存值，不再执行可能失败的操作。

第一次没有有效 current 时跳过 backup 更新，直接以 `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` 把 current temp 移到 current。若内存值来自有效 backup，原 backup 保持不变。

该顺序的可接受状态固定为：

- candidate temp 失败：current、backup、内存均不变。
- backup temp 或 backup 移动失败：current、内存不变；backup 保持旧有效版本或变为同一个旧值。
- current 移动失败：current 和内存保持旧值，backup 是旧有效值。
- current 移动成功：current 是 candidate，backup 是旧值；进程随后崩溃时重启从 current 恢复。

失败后只 best-effort 删除由本次调用创建的 temp。删除失败返回固定清理类别或留下可识别 temp；加载忽略 temp，绝不把 temp 当 current/backup。错误和日志不包含绝对路径或临时文件名。

## 加载、隔离与恢复

加载顺序固定为 current、backup、defaults：

1. current 不存在时继续 backup；权限和其他 I/O 错误直接失败。
2. current 可读但 JSON、schema 或任一 `appId` 无效时，用不覆盖现有文件的唯一 `.invalid-{pid}-{counter}` sibling 隔离；隔离失败则初始化失败。
3. backup 按相同规则加载或隔离。
4. 只有 current 与 backup 都不存在或均已成功隔离为无效文件时使用 defaults。

加载设置不根据当前 `AppCache` 删除结构有效的 key。单个损坏文件、恢复来源和错误只记录固定类别，不记录字段值、应用 ID、别名或路径。

## 测试合同

Task 4A 进入 TDD 前，实施计划至少覆盖：

1. defaults 与 current/backup 恢复顺序。
2. malformed `appId` 使整个文件无效；结构正确但暂时缺失的 ID 被保留。
3. 普通设置保存保留 `useCounts` 和暂时缺失应用的 aliases。
4. 未知 alias key 与未知 use-count 增量不修改内存或磁盘。
5. 两个并发 use-count 增量最终得到 2，内存与 current 一致。
6. backup/current 每个 I/O 阶段的失败注入及固定状态矩阵。
7. current 替换成功后 backup 仍为上一份有效值。
8. TypeScript DTO 和序列化输出不包含 `useCounts`、路径或动作。
9. 生产代码不注册命令、不修改 invoke handler，也不调用快捷键或 autostart 插件。

## 非目标与后续所有权

- Task 4B 复用原子文件 helper，但独立拥有验证计数和 session marker。
- Task 4C 复用 current 原子移动规则写出导出文件，但不创建 backup。
- Task 5 才包装和注册 Tauri commands。
- Task 6 才应用 hotkey、autostart 和退出生命周期副作用。
- Task 7 才实现设置 UI。

本设计批准前不得更新 Task 4A 实施计划或进入 TDD。
