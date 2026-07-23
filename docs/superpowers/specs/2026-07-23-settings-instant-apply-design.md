# 设置即时生效与恢复初始化设计

## 状态

- 日期：2026-07-23
- 状态：已按第二轮评审修订，等待复核
- 基线：`main@21cc09b`
- 分支：`codex/settings-instant-apply`
- worktree：`D:\code\UiPilot_tools\.worktrees\settings-instant-apply`

## 背景

设置页当前包含快捷键、开机启动、保存、重新加载设置和插件清单。快捷键已经在录制完成后通过专用 `save_hotkey` 命令立即持久化；插件操作和文件预览偏好也已有各自的即时写入路径。仍需点击“保存”的只有开机启动，而全量保存还会重复提交已经持久化的快捷键。

本需求移除显式保存步骤，让设置页的可编辑配置在操作完成后立即生效，并将“重新加载设置”替换为带确认的“恢复初始化”。

## 目标

1. 快捷键继续在有效录制完成后立即生效并持久化。
2. 勾选或取消开机启动后立即生效并持久化，无需点击保存。
3. 删除“保存”按钮。
4. 将“重新加载设置”替换为“恢复初始化”；确认后一次恢复快捷键 `Alt+Space` 和关闭开机启动。
5. 每次进入设置页都从后端加载受管的最后一次成功持久化快照，替代常驻的手动重新加载入口。
6. 保存失败时不暴露后端错误，也不把不确定的运行时状态误判为可编辑。

## 非目标

- 不重置插件、文件预览、窗口位置、应用使用次数或其他内部状态。
- 不新增通用 settings patch、自动保存队列、防抖器或后端命令。
- 不新增重新打开或解析 `settings.json` 的磁盘重载命令。
- 不改变插件清单及其重载、删除语义。
- 不处理基线中已有的 Windows 插件目录移动测试波动。

## 方案

### 复用现有事务

继续使用现有 `save_settings` 全量事务，不新增 `save_autostart` 或 `reset_settings`：

- 快捷键仍只走现有 `save_hotkey` 专用事务。
- 开机启动切换时，core 先显示新值，再调用一次 `save_settings`，payload 为当前 durable 快捷键和新的 `autostart`。
- 恢复初始化确认后，core 一次调用 `save_settings`，payload 固定为 `{ hotkey: "Alt+Space", autostart: false }`。
- 现有 Rust 事务继续负责运行时快捷键、开机启动和磁盘持久化的顺序、补偿与串行化。

当前设置页没有其他未保存草稿，因此开机启动即时提交携带当前快捷键不会误提交无关字段。文件预览、窗口位置和使用次数不在 `UserSettingsUpdate` 中，不会被覆盖。

### Authoritative snapshot 契约

本规格中的 authoritative settings snapshot 专指 `load_settings_core` 从 `SettingsStore::snapshot()` 返回的后端受管快照，不表示命令重新打开 `settings.json`。`SettingsStore::persist` 先执行 `commit_with_backup`，只有提交成功后才替换内存 `SettingsState.value`；因此持久化失败不会发布候选值，后续 `load_settings` 仍返回最后一次成功持久化的受管快照。

该现有 Rust 顺序是本需求的后端契约。`atomic_file` 的失败测试继续保证提交失败时磁盘状态不被候选值替换；本需求不需要新的磁盘读取命令，也不依赖前端判断文件内容。

### 界面

- 删除“保存”按钮。
- 保留快捷键录制器和开机启动复选框。
- 将“重新加载设置”按钮改为“恢复初始化”。
- 点击“恢复初始化”后显示确认界面，明确将恢复 `Alt+Space` 并关闭开机启动；“取消”不产生 IPC，“恢复”只提交一次。
- 任一 settings operation 进行中，快捷键、开机启动和恢复初始化均禁用，避免交错提交。

不新增新的弹层组件；复用项目现有确认交互和按钮体系。

## 状态与数据流

### 私有状态与投影

不得继续用单一 `settingsNeedsReload` 同时表示加载失败和事务失败。core 至少区分以下私有状态；名称可以随实现调整，但生命周期不得合并：

- `settingsUncertain: boolean`：进程级 latch；`save_settings`/`save_hotkey` mutation 失败，或收到任意目标的有效 `LauncherShown.notice = settingsFailed` 时置为 `true`。当前进程内任何 `load_settings` 成功、notice 消费或视图切换都不得清除。
- 当前 settings view 的 load state：`loading | ready | error`，绑定当前 view epoch；进入新 settings epoch 时重新建立，当前 owner 加载成功可从 `error` 变为 `ready`。
- 最新待满足的 settings load epoch：当 settings 页面需要 authoritative load、但单一 `settingsOperation` 正被 mutation 或旧 load 占用时，记录最新当前 epoch，不建立无界队列。
- 统一的 `settingsOperation`：继续串行 load、save 和 hotkey；启动加载也必须成为带唯一 token 的 `load` operation，其 owner scope 为 `startup`，不再由独立布尔值控制。任何 operation 完成后都必须尝试执行上述最新待加载 epoch。

对外 `SettingsSnapshot` 按以下规则投影：

- `needsReload` 只表示 `settingsUncertain`，即必须重启才能重新获得写权限。
- `readOnly` 在 `settingsUncertain`、当前 load state 不是 `ready`，或 mutation/load operation 正在执行时为 `true`。
- 普通 load 失败不设置 `needsReload`；它只使当前 view 的 load state 进入 `error`。
- load state 为 `error` 时显示仅错误态可见的“重试”入口。重试以当前 epoch 和新 operation token 调用 `load_settings`；成功可清除普通 load 错误并恢复 `ready`，但永远不能清除 `settingsUncertain`。

### 生命周期失败通知

Rust 启动期 `reconcile_runtime_settings_with` 在快捷键注册、autostart 读取或变更失败时，通过一次性 `LauncherShown.notice = settingsFailed` 报告运行时设置不确定。notice 可能随 `launcher` 或 `settings` 目标下发，并在成功 emit 后被 Rust 消费，因此前端必须在任何有效 `LauncherShown` 进入 `shown()` 时、独立于 target 和可见文案，立即设置 `settingsUncertain`。

- notice 随 `launcher` 到达时，可继续在 launcher 投影现有 `shownNotice`；之后 notice 即使不再出现，进入 settings 并成功加载 authoritative snapshot 后仍为 `needsReload/readOnly`。
- notice 随 `settings` 到达时，同样先置 latch，再按当前 settings epoch 请求 authoritative load；加载成功不能恢复写权限。
- 后续 `LauncherShown.notice = null` 可以改变当前提示展示，但不能清除 latch。只有新 core/进程重启把 `settingsUncertain` 初始化为 `false`。
- settings 页面只要投影到 `settingsUncertain`，就显示固定的重启恢复提示；该提示来自 latch，不依赖当前 `LauncherShown` 是否仍携带 notice。

### 进入设置页与加载排队

每次收到目标为 `settings` 的新 view epoch，都先把该 view 的 load state 设为 `loading`，并记录该 epoch 需要一次 authoritative settings load。若没有 `settingsOperation`，立即用新 token 发起 `load_settings`；若旧 mutation、普通 load 或启动 load 仍在途，则保留最新 epoch 的加载请求，待旧 operation 结束后启动。进入页面不能因为 operation 已占用而丢失这次加载。

加载响应只在 operation token、view epoch 和当前 view 都匹配时应用。旧 epoch 的响应无论成功或失败都不得直接写入当前 settings、load error 或 status；它只释放自己的 operation，然后尝试启动最新当前 epoch 的待加载请求。当前不在 settings 时不启动加载，下一次进入会重新记录请求。

### 启动加载桥接

`start()` 注册 shown listener 后仍可立即预加载设置，但该请求必须进入上述统一调度：

1. 启动请求创建唯一 token 和 `startup` load owner，并占用 `settingsOperation`；不再维护能使 `reloadSettings()` 直接返回的独立 `startupSettingsPending` 短路。
2. 启动请求在没有 settings view 请求介入时，可以更新后台设置快照；一旦其存活期间进入 settings，当前 epoch 必须记录为 `loading` 和最新待加载 epoch。
3. 启动 owner 的成功值或错误都不是当前 settings epoch 的 owner 结果，不得直接调用 `replaceSettings`、写入当前 load error 或覆盖 status。
4. 启动请求完成后释放自己的 operation，并为仍在显示的最新 settings epoch 启动新 token 的 authoritative load。若当前已离开 settings，则不启动；下次进入重新记录。
5. 启动 pending 期间反复进入 settings B、离开、再进入 settings C，只保留 C；无论启动请求最终成功或失败，最终都只能由 C owner 的 load 更新 C。

插件清单加载保持独立；设置加载失败不得被展示为插件空态，插件操作也不得阻塞设置写入。

### 开机启动

1. 用户切换复选框。
2. core 检查当前设置可编辑且没有 settings operation。
3. core 乐观显示新值并启动现有 `save` operation。
4. 发送当前 durable hotkey 与新 autostart。
5. mutation 成功后释放 mutation operation，再以当前 settings epoch 和新 token 调用现有设置加载路径，以 authoritative snapshot 替换界面。

处理中再次切换或录制快捷键不会启动第二次操作。

### 恢复初始化

1. 用户点击“恢复初始化”。
2. 用户取消时关闭确认界面，不修改状态、不调用后端。
3. 用户确认时启动单一 `save` operation，界面显示默认值并提交固定默认 payload。
4. 成功后释放 mutation operation，再以新 token 重新加载 authoritative snapshot 并解除锁定。

默认值只来自现有产品默认：快捷键 `Alt+Space`，开机启动 `false`。不得以 `Settings::default()` 覆盖文件预览、窗口位置或使用次数。

## Mutation 完成与跨 epoch reconciliation

mutation operation 可以跨 view epoch 存活，但其 UI 所有权不能跨 epoch。完成时按命令结果处理：

### 成功

- 成功证明该事务的运行时与持久化步骤已经完成，不设置 `settingsUncertain`，也不因离开原 view 而要求重启。
- 若当前仍是 settings，释放旧 mutation operation 后，为当前 epoch 记录并立即启动一个拥有新 token 的 authoritative load。`save_hotkey` 的旧返回值也不得直接覆盖新 epoch。
- 若当前不在 settings，只释放 operation；下次进入设置页的强制加载足够，不记录进程级不确定。

### 失败

- `save_settings` 或 `save_hotkey` 失败立即设置进程级 `settingsUncertain`，并保留固定本地设置状态不确定提示直至进程重启。
- 若当前是 settings，释放旧 mutation operation后，为当前 epoch 启动拥有新 token 的恢复 load；旧 epoch 的响应或错误不得直接写入当前 UI。
- 若当前不在 settings，不启动 load；下次进入时加载 authoritative snapshot，但 `settingsUncertain` 仍保留。
- 恢复 load 成功只用返回的 authoritative snapshot 替换乐观 UI；它不能证明运行时已恢复，因此 `readOnly/needsReload` 和设置状态不确定提示保持不变。

mutation 失败与 lifecycle `settingsFailed` notice 使用同一个 `settingsUncertain` latch 和对外投影；区别仅在于 mutation 失败还会按当前 epoch 安排恢复 load，notice 则由目标为 settings 的正常进入流程安排 load。

## 加载错误与提示

现有后端将输入、平台副作用、持久化和补偿失败统一映射为 `settingsFailed`，启动 reconcile 失败也通过同名 lifecycle notice 报告。前端无法证明运行时快捷键或开机启动已安全恢复，因此采用以下保守语义：

1. 即时保存或恢复初始化失败后，显示固定本地设置错误，不显示 Rust 原始信息。
2. 自动调用 `load_settings` 取得后端受管的最后一次成功持久化快照，用于替换乐观界面。
3. 即使恢复 load 成功，当前进程的设置页仍保持 `needsReload/readOnly`，要求重启后再写入；快照成功只修正展示，不代表运行时已确认恢复。
4. 恢复 load 失败时保持 `settingsUncertain`，同时把当前 load state 设为 `error` 并提供“重试”。
5. `settingsUncertain` 对应的设置状态不确定提示不得被恢复 load、notice 消费或后续进入页面清空；只有进程重启建立新 core 后消失。

该语义同时适用于 `save_settings` 和 `save_hotkey` 失败。后端只有统一错误，前端不得依据命令类型推断某一种失败已经安全回滚。

普通进入页面的 load 或成功 mutation 后的确认 load 失败，只表示当前页面没有取得 authoritative snapshot：显示固定本地加载错误、进入 `error` 并提供“重试”，但不设置 `settingsUncertain/needsReload`。一次当前-owner load 成功可以清除该普通 load 错误并恢复编辑。若同时存在 uncertainty latch，设置状态不确定提示优先且始终保留，重试成功也只能把快照状态恢复为 `ready`，不能恢复编辑。

## API 变化

前端 core：

- `setAutostart` 从纯草稿更新改为启动即时保存。
- 删除公开的 `saveSettings` 操作入口。
- `reloadSettings` 保留为内部 authoritative load 能力；常驻按钮删除，仅在当前 load state 为 `error` 时通过“重试”调用。
- 新增最小的 `resetSettings` 前端操作，复用 `save_settings`。
- 将原私有 `settingsNeedsReload` 拆成进程级 `settingsUncertain` 与当前 view 的 load state，并保留一个最新待加载 epoch。`shown()` 对任意 target 的有效 `settingsFailed` notice 设置同一 latch。
- `start()` 的初始 `load_settings` 改用统一 load owner/operation 完成路径；删除或重新定义 `startupSettingsPending`，不得再用它静默吞掉 settings view 的加载请求。

Tauri/Rust：

- 不新增或修改 IPC 命令、payload、权限和 capability。
- 继续使用 `load_settings`、`save_settings` 和 `save_hotkey`。
- `load_settings` 继续返回 `SettingsStore::snapshot()`，不增加磁盘 I/O。

## 自动测试

### 前端

- 设置页不渲染“保存”和“重新加载设置”，渲染“恢复初始化”。
- 开机启动切换立即调用一次 `saveSettings`，payload 含当前 durable hotkey 和新 autostart。
- 快捷键录制继续只调用 `saveHotkey`，不重复调用全量保存。
- 恢复初始化取消时零调用；确认时只提交一次 `{ hotkey: "Alt+Space", autostart: false }`。
- operation pending 时所有设置写操作被拒绝。
- 即时保存成功后重新加载并应用 fake client 返回的 authoritative snapshot，而不是假设命令真实读盘。
- 即时保存失败后自动加载 authoritative snapshot、保留固定事务错误并保持只读；恢复 load 失败显示重试且仍保持只读。
- `saveHotkey` 失败同样自动加载 authoritative snapshot，并且加载成功也不能清除当前进程的 `settingsUncertain`。
- 普通进入页 load 或成功 mutation 后的确认 load 失败时，`needsReload` 仍为 `false`；错误态显示“重试”，当前-owner 重试成功后清除普通 load 错误并恢复可编辑。
- mutation 失败后的恢复 load 先失败再重试成功时，authoritative snapshot 被应用，但 `needsReload/readOnly` 与设置状态不确定提示仍保留。
- lifecycle notice 覆盖两条路径：先在 `launcher` 收到 `settingsFailed`、随后以无 notice 的新 epoch 进入 settings；以及直接在 `settings` 收到 notice。两者的 authoritative load 成功后仍必须 `needsReload/readOnly`，notice 是否继续可见不得影响 latch。
- autostart 成功与 `saveHotkey` 成功分别覆盖：epoch A mutation pending，离开并重新进入 epoch B；B 的加载请求先排队，mutation 完成后必须以 B 的新 token 自动加载，最终可编辑且不要求重启。
- autostart 失败与 `saveHotkey` 失败分别覆盖同一离开/重进时序；失败只设置进程级 uncertainty，恢复 load 属于 epoch B，旧响应/错误不得覆盖 B，最终应用 authoritative snapshot 但保持只读。
- 旧 load pending 时连续进入新 settings epoch，只保留最新 epoch 的加载请求；旧响应被丢弃后，新 owner load 必须启动。
- 启动 load pending 时进入 settings B：B 立即为 `loading`，启动成功响应不得直接覆盖 B；启动 owner 释放后必须发起 B 的新-token load，只有 B owner 响应可应用。
- 启动 load pending 后进入 settings B、离开并再进入 settings C：分别覆盖启动请求成功和失败；启动值/错误及 B 请求都不得落入 C，最终只应用 C owner load，并且不存在 `startupSettingsPending` 静默吞请求。
- 插件清单草稿和逐行 operation 不受设置操作影响。

### Rust 与回归

- 不修改 Rust 生产实现；现有契约保持：`load_settings_core` 只投影 `SettingsStore::snapshot()`，`SettingsStore::persist` 在 `commit_with_backup` 成功后才发布候选值，持久化失败不更新受管 snapshot。现有 settings 与 atomic-file 测试必须继续通过。
- 前端全量测试、TypeScript/Vite build、Rust 全量测试和格式检查必须执行。
- 已知基线插件目录移动测试若再次出现同一 Windows 偶发失败，必须单独记录，不得在本需求分支顺带修改插件删除实现。

## 人工验收

1. 修改快捷键，不点击其他按钮；当前运行立即使用新快捷键，重启后仍保留。
2. 勾选开机启动，不点击其他按钮；重新进入设置页仍为勾选，重启后仍保留。
3. 取消勾选开机启动，确认同样立即生效并持久化。
4. 点击恢复初始化后取消，快捷键和开机启动均不变化。
5. 再次点击并确认，快捷键变为 `Alt+Space`、开机启动关闭；重启后保持。
6. 恢复初始化前后的插件清单、文件预览、窗口位置和使用记录不变化。

## 完成条件

- 上述自动测试通过，除已记录的基线波动外无新增失败。
- 人工验收通过。
- worktree 仅包含本需求相关提交，等待用户确认后再合并到 `main`。
