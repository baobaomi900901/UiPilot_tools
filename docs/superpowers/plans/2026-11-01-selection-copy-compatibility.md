# 临时复制取词实现计划

在现有 UIA 划词捕获上增加受控复制回退，缓存并有条件恢复原剪贴板，同时排除 UiPilot 临时记录、定向清理 Win+V 新增条目。合同见 [设计](../specs/2026-11-01-selection-copy-compatibility-design.md)。

技术：Rust、windows 0.61.3 Win32/WinRT、现有 Tauri 生命周期与权限。所有真实取词只由用户快捷键发起；测试使用假数据。保留工作区既有改动，不自动提交。

执行采用 TDD；共享一次编译后运行各模块测试，最终独立审查接口、权限、并发及失败恢复。

### Task 1: UiPilot 历史事务门

**Files:** `src-tauri/src/clipboard_history/service.rs`, `observer.rs`, `mod.rs`。

**Dependencies:** 设计“两套历史”。

- [x] 提供 `with_temporary_copy` 闭包事务及精确序号排除。
- [x] 观察器读取到写入期间持同一门；保留旧的粘贴抑制接口。

**Distinct test coverage:** 复制已发生但观察器尚未入库，事务结束后的延迟回调仍忽略；不同序号和相同文本的后续正常复制必须入库；事务失败释放门。

**Verify:** `cargo test --lib clipboard_history`

### Task 2: Win+V 定向清理

**Files:** `src-tauri/src/selection_capture/windows_history.rs`, `src-tauri/Cargo.toml`。

**Dependencies:** 设计“两套历史”；独立于 Task 1。

- [x] 封装有限时的历史基线和指定新增条目清理。
- [x] 旧 ID、异内容和归属不确定的记录不删除。

**Distinct test coverage:** 旧同文条目、多个新增候选、历史关闭/访问拒绝、等待中剪贴板改变、异步查询超时。

**Verify:** `cargo test --lib windows_history`

### Task 3: 捕获事务与投递集成

**Files:** `src-tauri/src/selection_capture.rs`, `selection_capture/native.rs`, `selection_capture/clipboard.rs`, `selection_translation.rs`, `public_plugins/activation.rs`。

**Dependencies:** Tasks 1–2；设计“流程与权限”和“剪贴板事务”。

- [x] 原格式缓存、一次 Ctrl+C、新序号文本读取和有条件恢复。
- [x] 仅 UIA 无选区接口时回退，沿用授权、目标与取消检查；所有失败保留明确提示。
- [x] 更新旧的 UIA-only 文档，与用户新的恢复要求一致。

## 验证进度

UiPilot 历史事务门、Win+V 清理及最终集成均通过独立审查。新增核心行为通过假数据测试，不访问真实剪贴板。前端 33 项及生产构建通过；最终源码编译出的 Rust 测试程序（2026-11-01 09:34）`selection` 过滤 69 项全部通过，覆盖注册格式白名单及接收端超时后的恢复失败报告。`clipboard_history` 过滤 33 项通过；相关源码此后未改动。定向差异空白检查通过。微信真实取词、原剪贴板恢复以及两套历史检查仍待用户实机反馈，不以自动化策略测试代替实机验收。

**Distinct test coverage:** UIA 成功零复制，空选区不复制，旧剪贴板不作为选区，新复制抢占后不恢复，备份失败无输入，失败仍清理，晚到结果不投递。

**Verify:** `cargo test --lib selection`; `npm test -- src/selection-translation.test.ts`

最终实机验收：微信选区触发翻译，原剪贴板可粘贴，UiPilot/Win+V 无新增临时文字；用户并行新复制得以保留。实际翻译及真实历史检查由用户触发。

## 微信实测后的线程修复（2026-11-01）

用户两次实测均在复制前返回 `selection_history_unavailable`。只读状态探针复现 `IsHistoryEnabled` 在 MTA 返回 `0x8000001D`、在 STA 成功；问题是宿主历史接口的线程环境。修正历史调用的 STA 作用域及有界消息分派，保留 UIA 的 MTA 作用域和既有恢复/清理策略。新增两个真实线程回归测试均先失败后通过；独立历史模块共 14 项通过，不读取真实剪贴板内容。最终 `cargo test --lib selection` 共 71 项全部通过，格式检查通过；新一轮微信实测待用户确认。

## OLE 剪贴板备份兼容（2026-11-01）

后续实测已通过历史接口，停在备份阶段。只读格式清单显示图片剪贴板包含 `DataObject`、`Ole Private Data`；OLE 声明的实际格式则为 DIB、PNG 和历史控制标记。原实现将这两个附加格式直接视为不可备份，导致普通图片也被阻断。改为先校验 OLE 的全部数据声明、重新加锁检查序号，再备份实际数据，避免恢复失效对象引用。新增分类和格式覆盖测试先失败后通过，独立策略测试 9 项通过；最终 `cargo test --lib selection` 73 项全部通过，格式检查通过。新的微信实测待用户确认。

## 微信富文本备份兼容（2026-11-01）

用户确认原剪贴板为图片时已能成功翻译；文字时仍失败。日志显示成功路径完成复制和恢复，而失败路径为 `backup_unknown_format=50021`。只读格式元数据确认该格式为 `WeChat_RichEdit_Format`，另外还有 QQ 富文本与微信 MIME 格式，三者都声明为普通 HGLOBAL。新增需 OLE 校验的格式分类，要求全部匹配实际声明后按原字节备份；缺项、流、索引数据继续拒绝。两个新增回归测试先失败后通过，独立策略测试 11 项通过；最终 `cargo test --lib selection` 75 项全部通过，格式检查通过。微信文字恢复实测待用户确认。

## 连续取词的 OLE 介质标志修复（2026-11-01）

用户确认首次成功、隐藏主窗口后再次失败。日志交替出现完整复制恢复成功与 `backup_ole_payload=13`；只读格式探针确认恢复后 Windows 的 OLE 适配器将各格式声明为 `HGLOBAL | ISTREAM`（5），原来源为 HGLOBAL（1）。修正对 `FORMATETC.tymed` 的全值相等检查，按位检测所需介质。回归测试覆盖恢复前后全部文字格式、只提供流/空类型的拒绝以及位图的 GDI 要求；先失败后通过，独立策略测试 12 项通过；最终 `cargo test --lib selection` 76 项全部通过，格式检查通过。用户主动实测期间，最新进程日志记录连续 6 次捕获成功、6 次原剪贴板恢复成功，无捕获失败；用户随后明确确认：连续三次均成功填入翻译，原剪贴板粘贴正常；本轮连续取词与恢复实机验收通过。
