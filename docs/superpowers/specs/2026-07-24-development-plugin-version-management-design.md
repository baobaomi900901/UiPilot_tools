# UiPilot 开发插件源与多版本管理设计

## 目标

本次变更面向内部开发者环境，在现有设置页插件管理能力上增加：

1. Debug 宿主读取 worktree 内的 `examples/plugins`，并与 AppData 已安装插件合并为一份清单。
2. 未安装的开发插件可以从设置页安装；开发版本较新时可以原地更新并立即启用。
3. AppData 中每个插件 ID 可以保留多个不可变版本，并由宿主明确记录当前活动版本。
4. 删除只删除当前活动版本；仍有历史版本时自动回退到最高版本。
5. 旧的单版本 AppData 目录在启动时自动迁移到多版本布局。

`%APPDATA%\com.uipilot.launcher\plugins` 仍是唯一可运行插件来源。`examples/plugins` 只是 Debug 开发包来源，不能直接参与查询路由或获得运行时权限。

## 已确认决策

- 仅 Debug 开发环境读取 `examples/plugins`；正式打包版本不捆绑、不扫描该目录。
- 后端统一合并开发包和已安装包，前端不读取路径，也不自行推断安装状态。
- 每次从非插件 Tab 进入“插件”Tab 和点击刷新按钮时重新扫描开发包，无需重启宿主。
- 未安装插件提供“安装”；开发包版本高于活动版本时提供“更新”。
- 更新立即切换；失败时旧活动版本继续工作。
- `可更新` 和 `未安装` 状态显示开发包 README；普通 `已安装` 状态显示活动版本 README。
- 无效开发包仍显示在清单中，展示稳定原因并禁用安装或更新。
- 插件项展示所有已安装版本，但 MVP 不提供手动启用历史版本。
- 删除只删除活动版本；有其他版本时自动启用 canonical 版本最高者。
- 旧单版本目录自动迁移；迁移先完成全局冲突规划，失败时保留未迁移内容并报告错误。

## 非目标

- 不建设插件市场、远程下载、签名发布或自动更新服务。
- 不在 Release 构建中暴露 worktree 或 `examples/plugins`。
- 不支持从任意用户选择路径安装插件。
- 不允许覆盖写入同一个已登记版本。
- 不提供历史版本的手动切换、逐版本独立操作按钮或自动版本清理策略。
- 不修改插件 manifest v1、权限模型、查询协议或 README Markdown 能力范围。
- 不允许插件运行时调用安装、更新、删除或清单管理命令。
- 不提供损坏安装状态的 UI 自动修复；该状态只能 fail closed，由开发者修复磁盘内容。

## 来源与目录模型

### 开发包来源

Development source 必须在编译期消除：

```rust
#[cfg(debug_assertions)]
fn development_plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

#[cfg(not(debug_assertions))]
fn scan_development_plugins() -> DevelopmentInventory {
    DevelopmentInventory::empty()
}
```

`development_plugin_root`、source path 常量和完整扫描实现只存在于 `#[cfg(debug_assertions)]` item。Release stub 固定返回空 development inventory，`install_plugin` 固定 fail closed；Release binary 不保留 `CARGO_MANIFEST_DIR` 或 `examples/plugins` source path。

Debug 后端只枚举该根目录的直接子项。每个子项必须是普通目录，不能是 symlink、junction 或其他 reparse point。有效开发包的目录名必须是合法插件 ID，并与 `plugin.json.id` 完全一致。

开发包仍采用单版本源码布局：

```text
examples/plugins/internal.math/
├─ plugin.json
├─ README.md
├─ runtime.html
└─ runtime.js
```

开发包目录缺失等价于空开发源，不影响 AppData 中已安装插件。单个开发包无效只影响该项，不能使整个清单降级为失败。只有开发根枚举、inventory 一致性重试耗尽或管理器状态不可用才返回 `pluginListFailed`。

### Canonical 版本

本需求沿用现有三段 `u32` 版本能力，但把可接受文本冻结为 canonical `major.minor.patch`：

- 每段只能是 `0` 或不以 `0` 开头的十进制数字。
- 每段解析值不得超过 `u32::MAX`。
- 不接受 prerelease、build metadata、正负号、空白或第四段。
- 解析后的 canonical stringify 必须与输入逐字节相等。

因此 `01.2.3`、`1.02.3` 与 `1.2.03` 均无效，不能与 `1.2.3` 共存。

目录名、manifest version、状态文件 version 和 API version 全部使用同一解析器。版本集合按 `[major, minor, patch]` 去重和排序，不能只按原字符串去重。

### 固定资源预算

所有上限是不可配置常量，枚举一旦越界立即停止。

单包预算：

- 插件根以下最多 64 个目录、256 个普通文件，最大深度 8。
- 单文件最多 2 MiB，全部普通文件合计最多 16 MiB。
- `plugin.json` 另限 64 KiB；`README.md` 沿用 16 KiB。
- UTF-8 相对路径最多 240 bytes，单个 component 最多 100 bytes。
- 拒绝无效 UTF-8、绝对路径、`.`、`..`、尾随点/空格、Windows 保留名和 alternate data stream。
- 只接受普通目录和 link count 为 1 的普通文件。
- 拒绝 symlink、junction、其他 reparse point、hard link、device、socket 和任何特殊文件。
- 普通资产仅允许 `.html`、`.js`、`.mjs`、`.css`、`.json`、`.md`、`.txt`、`.png`、`.jpg`、`.jpeg`、`.gif`、`.webp`、`.ico`、`.svg`、`.woff`、`.woff2`、`.ttf`、`.otf`。
- `plugin.json` 和 `README.md` 使用固定文件名，不通过任意扩展名规则识别。

根和全次扫描预算：

- Development root 最多 128 个直接 entry、64 个 development candidate。
- Installed plugin root 最多 128 个直接 entry、64 个 plugin container。
- 每个 `active.json` 最多登记 32 个 package version。
- 一次 `list_plugins`、startup load 或 global migration plan 最多访问 512 个目录、1024 个普通文件和 64 MiB 文件内容。
- 合并后的 inventory 最多 128 行。

宿主恢复目录预算：

- `plugin-transactions` root只允许固定的`active`、`staging`、`receipts`三个直接子目录；未知直接entry或第4个entry使插件子系统fail closed。
- `active`最多1个active journal和1个可认证atomic-replace temp；`staging`最多8个transaction object；`receipts`最多128个receipt和128个一一对应的可认证temp。
- Receipt容量计算把durable `cleanupPlans`中的receipt ID与已存在的同ID receipt视为同一个slot；不同ID分别计数。
- Quarantine容量计算把durable plan中的`plannedTarget`与该位置上identity匹配的对象视为同一个slot；不同planned target分别计数。
- `plugin-quarantine`最多128个直接cleanup object或durable planned target；单个cleanup object最多64 MiB、512个目录和1024个普通文件。
- `plugin-runtime-data` root最多193个直接entry：最多64个当前进程active ownership、1个当前进程staged ownership，以及128个durable journal plan或receipt引用的entry；同一entry被plan与同ID receipt引用时只计一次。
- 一次startup recovery跨transaction、quarantine和runtime-data root最多枚举590个直接entry、8192个目录、16384个普通文件和1 GiB logical file length；journal/receipt内容合计最多读取16 MiB。
- Cleanup worker每批最多处理8个receipt、8个quarantine object和64 MiB实际重计量字节；达到任一上限即结束本批，后续批次继续。
- Cleanup worker不能按receipt中的声明值直接计费；必须在任何递归删除前完成no-follow重计量并按actual bytes计费。

Runtime-data ownership分类：

- `active`由当前进程manager active slot持有；`staged`由当前进程唯一staged slot持有。
- `journal-referenced`必须由active journal typed object或cleanup plan的location和stable identity证明。
- `receipt-referenced`必须由durable receipt的source或target location和identity证明。
- 其余entry全部为`unknown`。
- Startup创建runtime前不存在当前进程active/staged ownership；既有entry必须由journal或receipt证明归属。
- Unknown entry不得rename或删除；插件子系统进入`transactionRecoveryRequired`或fail closed。
- 达到runtime-data root或startup总预算任一上限时立即停止并fail closed，不能返回截断分类、部分恢复或部分删除。

越界结果：

- 单个 development package 超过单包预算时，该项为 `resourceLimitExceeded`。
- Development root 或全次 list 扫描超过根/总预算时，整次 `list_plugins` 返回 `pluginListFailed`，不能返回截断清单。
- Installed root、startup 或 migration plan 超过根/总预算时，插件子系统 fail closed，不加载不完整 catalog。
- `active.json.packages` 超过 32 项时，该插件为 `stateInvariantViolation`。
- Install 会产生第 33 个 version 时返回 `pluginInstallFailed`，零 durable commit。
- Prepared journal尚未durable时，没有durable owner的partial object必须同步清理；失败或崩溃遗留由startup认证后fail closed。
- Transaction/receipt/quarantine/startup recovery任一预算越界时，整个插件子系统fail closed；不能截断、忽略或删除未完成身份验证的entry。
- Receipt或quarantine容量在prepared journal或standalone receipt持久化前不足时，当前命令零不可逆提交。
- Prepared journal持久化后，其全部receipt ID和planned target占用对应容量，直到journal或receipt按协议删除。
- 不可逆state commit或runtime promotion后发现capacity、ID、location或measure invariant不一致时进入fail-stop，不能作为普通command error继续运行。

全局 trigger 规则：

- 两个 new-layout active plugin 使用相同 trigger 时，双方均进入安装故障，两个 route 都不创建；其他无冲突插件可以加载。
- 两个 development candidate 使用相同 trigger 时，双方均为 `duplicateTrigger`。
- Development candidate 与另一个 active plugin trigger 冲突时，development 项为 `duplicateTrigger`。
- 同一 plugin 的 update 复用自己的当前 trigger 合法。
- Install/update 最终 preflight 必须在 mutation lock 内重新构建 active/development trigger map，不能信任旧 inventory。

### 已安装目录

安装后的目录固定为：

```text
%APPDATA%/com.uipilot.launcher/plugins/
└─ internal.math/
   ├─ active.json
   ├─ 0.1.0/
   │  ├─ plugin.json
   │  ├─ README.md
   │  └─ runtime.*
   └─ 0.2.0/
      ├─ plugin.json
      ├─ README.md
      └─ runtime.*
```

版本目录名必须与内部 canonical `plugin.json.version` 完全一致。排序和“可更新”判断使用解析后的三段数值，不使用字符串排序。

已登记版本目录不可变。开发者修改 `examples/plugins` 中同版本内容不会覆盖 AppData 中同版本；需要提升 `plugin.json.version`。

### 包内容身份

每个已登记版本拥有宿主计算并持久化的 `PackageIdentityV1`：

```text
PackageIdentityV1 {
  algorithm: "sha256-tree-v1",
  digest: LowercaseSha256,
  volumeSerial: u64,
  fileId: CanonicalWindowsFileId
}
```

`sha256-tree-v1` 使用 `sha2` crate 的 SHA-256，byte grammar 固定为：

```text
ASCII "UIPILOT-PACKAGE"              # 15 bytes
u8    0                              # separator
ASCII "SHA256-TREE-V1"               # 14 bytes
u8    0                              # separator
u32le entry_count

repeat entries sorted by canonical_path UTF-8 bytes:
  u8    entry_type                   # 1 = directory, 2 = regular file
  u32le canonical_path_byte_length
  bytes canonical_path_utf8
  if entry_type == 2:
    u64le file_length
    bytes file_sha256                # exactly 32 raw bytes
```

Directory entry 在 path 后结束，不编码 file length 或 digest。File digest 是从 no-follow file handle 读取内容得到的 32 raw bytes，不是 hex text。Tree digest 是上述完整 byte stream 的 SHA-256，并以 64 个小写 hex 持久化。

Package root本身不编码为directory entry，不计入`entry_count`，也不计入“最多64个目录”的目录预算。`entry_count`精确等于package root以下全部descendant directory entry与regular file entry数量之和。Package root的volume/file identity只保存在`PackageIdentityV1`，不进入tree byte stream。

Canonical path 规则：

- 相对 plugin root，separator 固定为 `/`；被编码path至少包含一个component，不存在表示package root的空path。
- 每个 component 必须是有效 UTF-8 且已经是 Unicode NFC；输入不是 NFC 时拒绝，不静默重写。
- 排序使用 canonical path 原始 UTF-8 byte order。
- 在 hash 前对全部 canonical path 做 Windows `CompareStringOrdinal(..., TRUE)` 等价比较。
- 两个不同 path 若在 Windows ordinal ignore-case 下相等，视为 case-fold collision 并拒绝整个 package。
- Manifest/runtime 引用必须与 canonical path 大小写逐字节一致，不能依赖 Windows case-insensitive lookup。

Digest 覆盖 manifest、runtime entry、脚本、样式、全部资产和 README。Directory 的 volume/file identity 另外绑定路径 entry，防止同 digest path 被替换。

如果 development canonical version 已登记但 digest 不同，development 状态为 `versionContentCollision`。只有 version 和 digest 都相同，才可直接激活已登记 inactive version。

### Generation-owned immutable asset snapshot

Package 验证同时构建 `GenerationAssetSnapshot`：

```text
GenerationAssetSnapshot {
  packageIdentity: PackageIdentityV1,
  assets: Map<CanonicalPath, {
    contentType: string,
    bytes: Arc<[u8]>
  }>,
  totalBytes: u64
}
```

构建协议：

1. No-follow 打开 package directory 并验证 volume/file identity。
2. 枚举每个普通文件时，以不允许 write/delete sharing 的方式打开 file handle；若已有不兼容 writer，验证失败。
3. 从这些 handle 读取 bytes、计算 file digest 并累计不超过 16 MiB 的 snapshot。
4. 读取后重新查询每个 handle 的 file identity 和 length；任一变化使验证失败。
5. 所有 bytes 和 tree digest 完成后才关闭 file handle。
6. Runtime entry、script 和 asset 全部来自同一个 snapshot。

`asset_response` 只能从 generation-owned snapshot 返回 bytes，不得在请求时 `fs::read(path)`。Staged ownership 持有 candidate snapshot；promotion 把同一 snapshot 移交 active ownership；runtime 关闭后才释放。

磁盘在 snapshot 建立后被修改不会改变活动 runtime 读到的 bytes。List/reload/delete 发现 active package identity 或 digest 漂移时：

1. 旧 generation 在安全转换线性化前仍只服务旧已验证 snapshot，绝不读取新 path bytes。
2. 管理器取得 write admission，重新确认 active generation 和 inventory revision。
3. 标记该 installed item 为 `packageIdentityMismatch` 或 `packageDigestMismatch`。
4. 取消 pending、移除 route/permission、推进 Plugin domain epoch 和 inventory revision。
5. 关闭 runtime 并释放 snapshot。
6. 当前 list 丢弃旧 revision 结果并重新读取故障状态。

因此 fail closed 的含义是禁用被篡改 package；不存在“报告磁盘故障但继续从已变化 path 读取资产”的窗口。

## active.json v1

`active.json` 是宿主拥有的版本状态文件，不属于插件包内容：

```text
ActivePluginStateV1 {
  schema: 1,
  pluginId: PluginId,
  activeVersion: CanonicalVersion | null,
  packages: PackageRecordV1[]
}

PackageRecordV1 {
  version: CanonicalVersion,
  identity: PackageIdentityV1
}
```

非空状态不使用伪造的固定 digest/file identity 示例。测试和实现必须从实际 package bytes 计算 `PackageIdentityV1`，再序列化以下类型：

```text
ActivePluginStateV1 {
  schema: 1,
  pluginId: PluginId,
  activeVersion: CanonicalVersion,
  packages: NonEmpty<PackageRecordV1>
}
```

合法空安装状态固定为：

```json
{
  "schema": 1,
  "pluginId": "internal.math",
  "activeVersion": null,
  "packages": []
}
```

约束：

- `schema` 必须精确为 `1`；未知 schema fail closed，不做猜测迁移。
- `pluginId` 必须是 canonical manifest ID，并与宿主容器名一致。
- `packages` 是宿主认可的完整版本集合。
- `packages` 按 canonical 版本升序持久化，语义 version 不得重复。
- 非空状态要求 `activeVersion` 为 canonical string，且精确匹配 `packages` 中一个已验证版本。
- `activeVersion: null` 当且仅当 `packages` 为空。
- 任何其他组合都是 `stateInvariantViolation`。
- 未登记目录永远不进入清单和 catalog。
- 合法空状态不创建 runtime、路由或授权。
- 用户手工编辑 `active.json` 不属于受支持流程。
- 缺失、损坏、字段不一致或指向无效目录时，不自动选择其他版本。

状态文件通过同目录临时文件、文件 flush、原子 replace 和父目录 flush 写入。

合法空状态是删除最后版本的永久 durable committed state：

1. 删除最后版本只把 `active.json` 提交为合法空状态。
2. 内存活动entry、route和permission随后提交。
3. 旧runtime关闭后，只使用提交前pin住的package handle隔离最后version目录。
4. Plugin ID container和合法空`active.json`永久保留，不同步删除、不创建container cleanup第二状态。
5. Package隔离失败时，空state继续表示未安装；version目录未登记且由versioned cleanup receipt持有。
6. 重启只需比较old non-empty state与new empty state，不会出现“newState后container absent”的第三种判定。
7. 同一ID重新安装复用空container，原子把empty state替换为non-empty state。

Container不存在只表示从未安装。Container存在但`active.json`缺失始终是安装故障，除非有效legacy migration journal证明该目录属于尚未完成的迁移。

## 统一清单模型

### DTO

前端DTO使用双来源discriminated union：

```text
type DecimalRevision = string

PluginInventorySnapshot {
  revision: DecimalRevision,
  items: PluginInventoryView[]
}

PluginInventoryView {
  key: string,
  id: string | null,
  displayName: string,
  installed:
    | { state: "absent" }
    | {
        state: "valid",
        activeVersion: string,
        versions: string[],
        trigger: string
      }
    | {
        state: "invalid",
        issue: InstalledPluginIssue,
        activeVersion: string | null,
        versions: string[]
      },
  development:
    | { state: "absent" }
    | {
        state: "valid",
        version: string,
        trigger: string
      }
    | {
        state: "invalid",
        reason: PluginSourceIssue
      },
  description:
    | {
        state: "available",
        source: "installed" | "development",
        markdown: string
      }
    | { state: "unavailable" }
}
```

`key`和`displayName`生成规则：

- 已验证ID使用`plugin:<hex(UTF-8 id)>`和原ID。
- 无法建立可信ID的development项使用`development-invalid:<64-lowercase-hex-digest>`；displayName使用`无效开发包 <12-hex-prefix>`。
- 无法建立可信ID的installed项使用`installed-invalid:<64-lowercase-hex-digest>`；displayName使用`无效已安装插件 <12-hex-prefix>`。
- 完整key digest是宿主对来源类型、直接子entry name和volume/file identity做domain-separated SHA-256后的64个小写hex；key不能截断。
- 只有displayName使用前12个hex。
- DTO不返回原basename或物理path。

只有目录名、manifest/state ID和语法全部一致时才设置`id`。`id: null`项不能成为管理command授权输入。

Invalid installed只有在state header可安全解析时才显示canonical versions；否则`activeVersion: null`、`versions: []`。Description和trigger只来自identity/digest验证成功的package。

固定`InstalledPluginIssue`：

- `stateMissing`
- `stateMalformed`
- `unsupportedStateSchema`
- `stateInvariantViolation`
- `packageMissing`
- `packageIdentityMismatch`
- `packageDigestMismatch`
- `packageInvalid`
- `transactionRecoveryRequired`
- `migrationConflict`
- `triggerConflict`

固定`PluginSourceIssue`：

- `invalidManifest`
- `invalidId`
- `invalidVersion`
- `incompatibleHost`
- `missingRuntime`
- `unsafePath`
- `duplicateTrigger`
- `resourceLimitExceeded`
- `versionContentCollision`

README缺失、超限、非法UTF-8或不安全时沿用“未提供说明”降级，不使package invalid。DTO用`description.state`表达无说明。

页面状态和按钮只允许通过`derivePluginPresentation(installed, development)`从两个union推导；DTO不包含`updateAvailable`等冗余状态。

| 安装状态 | Development | 页面状态 | 说明与操作 |
| --- | --- | --- | --- |
| absent | 无 | 不产生清单项 | 无操作 |
| absent | valid | 未安装 | Development README；允许安装 |
| absent | invalid | 开发包不可用 | 无说明；禁止安装/更新 |
| valid | 无 | 已安装 | Active README；允许reload/delete |
| valid | valid且version较高、无collision | 已安装、可更新 | Development README；允许update/reload/delete |
| valid | valid且version相同或较低 | 已安装 | Active README；允许reload/delete |
| valid | invalid或collision | 已安装、开发包不可用 | Active README；允许reload/delete |
| invalid | 任意 | 安装状态故障 | 不读取未验证说明；禁止全部mutation |

Installed versions只来自`active.json.packages` canonical metadata。无效state、identity或trigger collision使整项进入`installed.invalid`，不能悄悄删项、选择其他active或用development覆盖修复。

### Inventory revision

插件管理器内部维护单调递增 `inventoryRevision: u64`，但 Tauri/JSON DTO 固定使用 canonical decimal string：

```text
type DecimalRevision = string

PluginInventorySnapshot {
  revision: DecimalRevision,
  items: PluginInventoryView[]
}

PluginMutationOutcome {
  revision: DecimalRevision
}
```

`DecimalRevision` 必须匹配 `0|[1-9][0-9]*`，解析值不得超过 `u64::MAX`，canonical stringify 必须与输入相等。Rust在序列化边界把u64转为该字符串；前端strict parser禁止JSON number、前导零、符号、小数和overflow。

前端比较器不得转成JavaScript Number。`compareDecimalRevision(a, b)` 先比较字符串长度，再对等长ASCII digit做ordinal lexicographic比较；相等返回0。测试覆盖 `9007199254740991`、`9007199254740992`、`18446744073709551614` 和 `18446744073709551615`。

Revision推进规则：

- 初始成功catalog建立后内部revision为1。
- 安装、更新、reload promotion、回退删除、删除最后版本和active runtime failure状态转换都checked reserve下一revision。
- Recovery或migration导致catalog状态变化也推进revision。
- 页面切换和纯development source扫描不推进revision。
- 内部u64 overflow时plugin manager fail closed；durable mutation必须在文件提交前发现。

`list_plugins`一致快照协议：

1. 取得manager read lock，复制installed metadata、active runtime状态和当前revision。
2. 为已登记目录建立no-follow handle，并释放manager lock。
3. 在锁外按固定预算验证installed identity/digest并扫描development source。
4. 再次取得manager read lock，检查revision与步骤1相同。
5. 同时重新确认所持handle仍对应已复制package identity。
6. Revision未变化时，把内部u64编码为DecimalRevision并返回完整snapshot。
7. Revision已变化时丢弃整个混合结果并重试。
8. 最多尝试3次；连续变化后返回`pluginListFailed`。

等待、递归扫描和digest计算期间不得持有admission、catalog、ResultRegistry或manager state lock。

Mutation成功只返回`PluginMutationOutcome`，不扫描development source，不构造`PluginInventoryView`。这保证durable commit后不会因README、development扫描或合并view失败而把已提交mutation表现成业务失败。

前端inventory只有`list_plugins` snapshot可以写入：

- 每个管理命令settled都签发或合并一次list reconciliation，不论结果是success还是error。
- Success先把outcome revision合并进`highestPluginRevision`；error不携带revision，但仍必须reconciliation，因为backend可能已执行fail-closed转换并推进revision。
- Mutation response永远不直接增加、替换或删除row。
- 当前plugins Tab active时立即reconcile；inactive时只标dirty，下一次激活强制list。
- Mutation属于旧epoch时，其outcome/error同样不能直接写row或当前错误；只按当前active/dirty规则触发reconciliation。
- List snapshot revision小于`highestPluginRevision`时丢弃。
- 相同revision的多个list由最新operation token决定。
- 纯development变化可在相同installed revision下产生不同snapshot，因此仍以最新list owner为准。

## 后端接口

主窗口可调用接口调整为：

```text
list_plugins() -> PluginInventorySnapshot
install_plugin(plugin_id) -> PluginMutationOutcome
reload_plugin(plugin_id) -> PluginMutationOutcome
delete_plugin(plugin_id) -> PluginMutationOutcome
```

规则：

- `install_plugin`同时承担首次安装和更新。
- 后端重新解析当前development source，不能信任先前inventory。
- Development version已登记但不是active时，development staging只验证canonical version和tree digest。Digest相同后，宿主必须重新验证已登记inactive package的完整`PackageIdentityV1`，并从registered package构建runtime snapshot；staging file identity不参与登记identity比较，也不得被promotion。
- Development version等于active、低于active或发生version-content collision时返回`pluginInstallFailed`。
- `reload_plugin`只重建active version，不改变`active.json`，并验证登记package identity。
- `delete_plugin`只删除active version。
- Mutation成功响应只含已提交revision；row最终状态统一由后续`list_plugins`产生。
- 所有管理接口只接受plugin ID；路径、version和README均由后端解析。
- 所有接口沿用main-window caller guard。
- Release中`install_plugin`没有development source，固定fail closed。
- Plugin runtime capability不包含这些命令。

固定命令错误：

- `pluginListFailed`
- `pluginInstallFailed`
- `pluginReloadFailed`
- `pluginDeleteFailed`

错误消息不暴露plugin路径、manifest原文、digest、journal、WebView label或内部异常。

## Generation、domain epoch 与 inventory revision

### Generation high-water

管理器维护 `generationHighWater: HashMap<PluginId, u64>`：

- Runtime identity 是 `plugin_id + window_label + generation`。
- 每次创建 staged runtime 前使用 checked increment 分配 generation。
- 分配成功后立即提高 high-water；即使 staged rollback，也不回退或复用该 generation。
- 删除活动版本、删除最后版本、合法空状态、cleanup receipt 和 transaction journal 清理都不能移除 high-water。
- 同一进程内删除最后版本后重新安装相同 ID，必须继续 checked increment。
- Generation 溢出使该插件 ID 在当前进程 terminal fail closed；不能从 `1` 重新开始。
- 进程重启后旧 runtime 和 callback 已不存在，允许从启动 catalog 重建新的进程内 high-water。

Window label 必须包含完整 generation，例如现有 `plugin-<hex-id>-g<16-hex-generation>`，因此同进程内旧 callback 无法与重装 runtime 发生 label 碰撞。

### Plugin domain epoch reservation

ResultRegistry 增加互斥的 `PluginDomainEpochReservation`：

```text
PluginDomainEpochReservation {
  expected: u64,
  next: u64,
  nonce: u64
}
```

协议：

1. Mutation 或 active runtime failure 先取得 plugin write admission。
2. 在 ResultRegistry lock 内检查 Plugin domain 未 exhausted、没有其他 reservation，并 checked 计算 `next`。
3. 成功后登记 reservation；write admission 保证 reservation 期间不会有 plugin query publish 或 CopyText 副作用穿透。
4. Durable commit 前可以取消 reservation；取消不推进 epoch。
5. Durable commit 后调用 `commit_reserved_plugin_epoch`，只接受 exact nonce/expected/next。
6. Reserved commit 只更新内存 epoch、清除 Plugin current result 和移除 reservation，不包含文件 I/O 或可恢复业务失败。
7. Reservation 计算溢出时，ResultRegistry 把 Plugin domain 标记为 terminal exhausted 并清除当前 Plugin result；mutation 不产生 durable 文件提交。
8. Terminal exhausted 后不再签发 Plugin token，不发布 Plugin result，不执行 Plugin CopyText action。

Inventory revision 同样在 write admission 内、durable commit 前 checked reserve。Generation、Plugin domain epoch 和 inventory revision 任一 preflight 失败，都不能提交 `active.json`。

### 提交后不变量

`active.json` replace 后只允许执行已预留的、不可恢复失败的内存步骤：

1. staged ownership promotion 或 active removal；
2. reserved Plugin domain epoch commit；
3. reserved inventory revision commit；
4. pending query cancellation；
5. route、permission 和 disabled 状态切换。

若 durable commit 后发生 lock poison、reservation mismatch 或任何不可能的 manager invariant failure，宿主必须 fail-stop：

- 保持 write admission，不允许带旧授权继续处理请求。
- 将 Plugin domain terminal exhausted 并清除结果。
- 禁用插件路由和剪贴板副作用。
- 终止当前 core/进程或进入只能通过重启恢复的 fatal 状态。
- 不能向前端返回普通 `pluginInstallFailed`、`pluginReloadFailed` 或 `pluginDeleteFailed` 后继续运行旧授权。

进程崩溃后，旧 token/action 只存在于已终止进程；重启先按 journal 恢复 durable 状态，再创建新的 ResultRegistry 和 runtime。

### 各路径适用范围

- Install/update：候选 ready 后、`active.json` replace 前 reserve epoch 和 revision。
- Reload：无 durable package state，但 promotion 前仍 reserve epoch 和 revision；reserve 失败则 rollback staged runtime。
- 回退删除：fallback ready 后、`active.json` replace 前 reserve epoch 和 revision。
- 删除最后版本：空状态 replace 前 reserve epoch 和 revision。
- Active runtime process-failed/意外 close：取得 write admission 后使用同一 reservation API；无磁盘提交，先完成禁用状态转换，再提交 reserved epoch/revision。
- Runtime failure 遇到 epoch/revision exhausted 时，当前 plugin/domain terminal fail closed，不能继续接受查询。

## 事务目录

宿主在 app data 下维护两个不被插件扫描的同卷目录：

```text
plugin-transactions/
├─ active/
├─ staging/
└─ receipts/
plugin-quarantine/
```

- `plugin-transactions/active`保存唯一active mutation journal，`staging`保存staged package和迁移暂存，`receipts`保存cleanup receipt。
- `plugin-quarantine`保存已提交删除、rollback和迁移恢复产生的cleanup debt。
- 名称由宿主生成且禁止覆盖。
- 两个目录与插件根同卷，但不在插件扫描入口内。

## Transaction journal与cleanup receipt

全局同一时刻最多存在一个active mutation journal，因为所有plugin mutation和migration共享mutation lock。Cleanup debt不占用active journal slot。

### PluginTransactionV1

```text
PluginTransactionV1 {
  schema: 1,
  transactionId: LowercaseHex128,
  operation:
    | "install"
    | "update"
    | "delete-with-fallback"
    | "delete-last"
    | "legacy-migration",
  pluginId: PluginId,
  phase:
    | "prepared"
    | "package-placed"
    | "state-committed"
    | "cleanup-transferred",
  oldState: DurableStateReference,
  newState: DurableStateReference,
  objects: TransactionObjectsV1,
  cleanupPlans: CleanupReceiptPlanV1[],
  cleanupReceiptIds: LowercaseHex128[]
}

CleanupReceiptPlanV1 {
  receiptId: LowercaseHex128,
  condition: "if-old-state" | "if-new-state",
  objectRole:
    | "candidate-package"
    | "candidate-runtime-data"
    | "previous-runtime-data"
    | "deleted-package",
  operation:
    | "rollback-staging"
    | "delete-version"
    | "delete-last-version"
    | "runtime-data",
  plannedTarget: TransactionObjectLocation,
  measure: CleanupMeasureV1
}

CleanupMeasureV1 =
  | {
      kind: "exact",
      bytes: u64
    }
  | {
      kind: "bounded",
      maxBytes: u64
    }

StableObjectIdentityV1 {
  volumeSerial: u64,
  fileId: CanonicalWindowsFileId,
  packageDigest: LowercaseSha256 | null
}

MovableTransactionObjectV1 {
  role: "candidate-package" | "legacy-package",
  identity: StableObjectIdentityV1,
  allowedLocations: TransactionObjectLocation[]
}

FixedTransactionObjectV1 {
  role:
    | "candidate-runtime-data"
    | "previous-runtime-data"
    | "deleted-package"
    | "fallback-package"
    | "activation-package",
  identity: StableObjectIdentityV1,
  location: TransactionObjectLocation
}

TransactionObjectsV1 =
  | InstallObjectsV1
  | DeleteWithFallbackObjectsV1
  | DeleteLastObjectsV1
  | LegacyMigrationObjectsV1

InstallObjectsV1 {
  kind: "install",
  commandOperation: "install" | "update",
  mode: "new-version" | "activate-existing",
  candidatePackage: MovableTransactionObjectV1,
  activationPackage: FixedTransactionObjectV1 | null,
  candidateRuntimeData: FixedTransactionObjectV1,
  previousRuntimeData: FixedTransactionObjectV1 | null
}

DeleteWithFallbackObjectsV1 {
  kind: "delete-with-fallback",
  deletedPackage: FixedTransactionObjectV1,
  fallbackPackage: FixedTransactionObjectV1,
  candidateRuntimeData: FixedTransactionObjectV1,
  previousRuntimeData: FixedTransactionObjectV1 | null
}

DeleteLastObjectsV1 {
  kind: "delete-last",
  deletedPackage: FixedTransactionObjectV1,
  previousRuntimeData: FixedTransactionObjectV1 | null
}

LegacyMigrationObjectsV1 {
  kind: "legacy-migration",
  legacyPackage: MovableTransactionObjectV1
}
```

不存在`committed`、`memory-committed`或`cleanup-pending` journal phase。`state-committed`表示active state已经越过durable提交点；进程内memory promotion不需要额外durable phase，重启从state重建。

Object location invariant：

- Transaction operation `install`和`update`都必须使用`objects.kind == "install"`；`commandOperation`必须与transaction operation相等。
- Install/new-version的candidate package必须保存稳定volume/file ID和digest，`allowedLocations`严格为transaction staging与installed destination两个location，顺序固定且不得包含其他路径。
- Install/activate-existing的candidate package只允许transaction staging一个location。
- Install/new-version要求`activationPackage == null`。
- Install/activate-existing要求`activationPackage`非空、role精确为`activation-package`，location精确为已登记inactive version目录，并逐字段匹配`active.json`中的完整`PackageIdentityV1`。
- Activation package永不进入cleanup plan。Candidate package与activation package允许tree digest相同，但file ID和location必须不同。
- Legacy package的allowedLocations严格为legacy source、transaction staging和installed destination，顺序固定。
- 其他fixed object只允许其唯一`location`。
- 同一stable identity不得被两个object role声明。

Cleanup plan invariant：

- `cleanupPlans`在`prepared` journal首次durable写入时已经完整，按`receiptId` byte order严格递增且不得重复，最多8项。
- Cleanup plan只引用`objectRole`，不复制可能因rename失效的source path。
- Package role必须使用`measure.kind == "exact"`；runtime-data role必须使用`measure.kind == "bounded"`。
- `prepared`、`package-placed`和`state-committed`中`cleanupReceiptIds`必须为空。
- `cleanup-transferred`中`cleanupReceiptIds`必须精确等于current durable state所选condition对应的plan ID集合。
- 后续phase不得增加、删除或修改objects或cleanupPlans。
- 创建receipt时，manager按typed object解析role，在全部allowed location中查找stable identity。
- Movable object必须恰好在一个allowed location匹配；0个、2个或identity不匹配均fail closed。
- Fixed object必须在其唯一location匹配。
- 解析出的当前location与stable identity组成具体`TransactionObjectIdentity`并写入receipt.source。
- 已存在receipt的source必须逐字段匹配同一解析结果。

Cleanup coverage invariant：

- Install/new-version：`if-old-state`精确覆盖candidate-package与candidate-runtime-data；`if-new-state`只覆盖存在的previous-runtime-data；activation-package必须为空。
- Install/activate-existing：`if-old-state`精确覆盖candidate-package与candidate-runtime-data；`if-new-state`精确覆盖candidate-package及存在的previous-runtime-data；activation-package必须存在且永不进入cleanupPlans。
- Delete-with-fallback：`if-old-state`只覆盖candidate-runtime-data；`if-new-state`精确覆盖deleted-package及存在的previous-runtime-data；fallback-package永不进入cleanupPlans。
- Delete-last：`if-old-state`为空；`if-new-state`精确覆盖deleted-package及存在的previous-runtime-data。
- Legacy-migration的cleanupPlans必须为空。
- 任一condition缺少必需role、包含额外role、重复覆盖role或引用不存在role时fail closed。

Package/digest invariant：

- Package object role包括candidate package、activation package、deleted package、fallback package和legacy package；其`StableObjectIdentityV1.packageDigest`必须非空。
- Package cleanup plan必须使用`CleanupMeasureV1 { kind: "exact", bytes }`。
- Runtime-data object role包括candidate runtime data和previous runtime data；其`StableObjectIdentityV1.packageDigest`必须为null。
- Runtime-data cleanup plan必须使用`CleanupMeasureV1 { kind: "bounded", maxBytes }`。
- Receipt.source的packageDigest必须逐字段匹配typed object stable identity：exact package必须非空且相等，bounded runtime-data必须为null。
- Role、measure kind和packageDigest nullability任一不匹配时，journal或receipt strict解析失败并fail closed。

### CleanupReceiptV1

```text
CleanupReceiptV1 {
  schema: 1,
  receiptId: LowercaseHex128,
  originOperationId: LowercaseHex128,
  pluginId: PluginId,
  operation:
    | "rollback-staging"
    | "delete-version"
    | "delete-last-version"
    | "runtime-data",
  phase: "pending" | "quarantined",
  source: TransactionObjectIdentity,
  plannedTarget: TransactionObjectLocation,
  target: TransactionObjectIdentity | null,
  measure: CleanupMeasureV1
}

DurableStateReference {
  kind: "absent" | "active-state-v1",
  sha256: LowercaseSha256 | null
}

TransactionObjectLocation {
  root:
    | "plugin-root"
    | "transaction-root"
    | "runtime-data-root"
    | "quarantine-root",
  relativePath: SafeRelativePath
}

TransactionObjectIdentity {
  role:
    | "legacy-source"
    | "staged-package"
    | "installed-package"
    | "deleted-package"
    | "runtime-data"
    | "quarantine-target",
  root:
    | "plugin-root"
    | "transaction-root"
    | "runtime-data-root"
    | "quarantine-root",
  relativePath: SafeRelativePath,
  volumeSerial: u64,
  fileId: CanonicalWindowsFileId,
  packageDigest: LowercaseSha256 | null
}
```

Journal、plan和receipt只保存host root enum与安全相对路径，不保存绝对路径。ID固定32个小写hex。每个journal/receipt最多64KiB；每个transaction最多8个cleanup plan。

Operation/receipt ID契约：

- 每个管理命令或recovery attempt开始先建立一个16-byte CSPRNG operation ID，并编码为32个小写hex。
- 新管理命令若创建journal，`transactionId`必须等于本attempt的operation ID；该journal转交的每个receipt都令`originOperationId == transactionId`。
- Reload或其他没有journal的standalone cleanup使用本attempt独立创建的operation ID作为`originOperationId`。
- Startup recovery恢复既有journal时不改写durable origin；补建receipt只能使用durable cleanup plan中的原`receiptId`。
- `receiptId`对每个cleanup object独立create-new，不能复用`originOperationId`；receipt文件固定为`receipts/<receiptId>.json`。每个cleanup plan和receipt都必须满足`plannedTarget.root == "quarantine-root"`且`plannedTarget.relativePath == <receiptId>`。
- Journaled路径的ID、receipt path和planned target collision必须在prepared journal持久化前发现。
- `plannedTarget`在cleanup plan或standalone pending receipt首次持久化前确定且之后不可变。
- `pending`要求`target == null`；`quarantined`要求target非空且root/path精确等于`plannedTarget`。

Object resolution契约：

- Cleanup plan通过`objectRole`定位typed object。
- Movable object在创建receipt时枚举全部allowed locations，以stable volume/file ID和digest匹配。
- 必须恰好一个location匹配；0个、2个或identity不匹配时fail closed。
- Fixed object只验证其唯一location。
- Receipt.source必须由当前唯一location和stable identity构造，不能沿用prepared时的旧path。
- Existing receipt.source必须逐字段匹配解析结果；不一致时不得另建receipt。

Measure契约：

- Immutable package在prepared journal前通过稳定no-follow扫描计算`CleanupMeasureV1 { kind: "exact", bytes }`。
- Runtime-data不要求在WebView运行期间冻结内容。它使用`CleanupMeasureV1 { kind: "bounded", maxBytes }`。
- Runtime-data的`maxBytes`来自固定、不可配置的role budget，不能来自调用方、插件或当次粗略扫描。
- Runtime-data root的volume/file identity和唯一location仍必须在prepared journal或standalone receipt前固定。
- Runtime关闭且management lease释放后，worker/recovery才允许no-follow递归计量runtime-data。
- Worker/recovery在任何递归删除前重新验证entry、directory、path和identity预算并计算actual bytes。
- Exact measure要求`actual == bytes`。
- Bounded measure要求`actual <= maxBytes`。
- Worker始终按actual bytes计入64 MiB batch，不能按`bytes`或`maxBytes`直接计费。
- 超限、identity/path不符、exact不相等时零删除，并进入`transactionRecoveryRequired`或fail closed。

Exact package worker/recovery契约：

- Worker/recovery处理exact package前，必须使用与`sha256-tree-v1`相同的no-follow枚举、canonical path、entry排序和hash grammar重新计算actual package digest。
- Exact package只有在root/path、volume/file identity、entry/directory预算、actual bytes和actual package digest全部匹配时才允许rename或递归删除。
- `actual bytes`必须等于`measure.bytes`。
- `actual package digest`必须同时等于receipt.source.packageDigest和typed object stable identity中的packageDigest。
- 同file identity、相同文件长度但内容变化，或者path/type变化导致digest变化时，必须零删除并进入`transactionRecoveryRequired`或fail closed。
- Bounded runtime-data只验证root/path、volume/file identity、entry/directory预算和`actual bytes <= maxBytes`；其typed object和receipt.source的packageDigest都必须为null，不执行package tree digest匹配。
- Worker仍始终按actual bytes计入64 MiB batch。

Receipt creation invariant：

- 从typed object当前唯一location创建exact package receipt时，receipt.source.packageDigest必须复制stable identity中的非空packageDigest。
- 从typed object当前唯一location创建bounded runtime-data receipt时，receipt.source.packageDigest必须写为null。
- Existing receipt只有在source location、volume/file ID、packageDigest和measure全部匹配plan及typed object时才能幂等采用。

### Durable ordering

1. 创建staging或candidate runtime data前，先检查最坏情况下所需的receipt/quarantine容量，并在`plugin-runtime-data` 193-entry上限内预留candidate staged slot；容量不足时零目录和零durable副作用。
2. Prepared journal尚未durable时，任何partial object都没有durable cleanup owner；失败路径必须同步清理并flush parent。
3. Pre-journal同步清理失败时fail-stop。进程崩溃遗留的pre-journal object由startup按host命名、root和identity认证后报告`transactionRecoveryRequired`，不得猜测删除。
4. Candidate ready后构建typed objects。
5. 对package role执行稳定扫描并写exact measure；对runtime-data role写固定bounded measure。
6. 根据typed objects生成完整cleanupPlans，完成capacity、ID、receipt path和planned target collision检查。
7. 临时文件写包含完整objects、cleanupPlans和空cleanupReceiptIds的journal `prepared`；flush文件、atomic replace、flush transaction parent。
8. Candidate package rename后flush双方parent；journal可更新为`package-placed`，但objects和cleanupPlans不得变化。
9. 临时文件写新`active.json`并flush、atomic replace、flushplugin container parent。
10. Journal更新为`state-committed`并flush；cleanupReceiptIds仍为空。
11. 执行已reserved、不可恢复失败的memory commit。
12. 根据current state选择`if-old-state`或`if-new-state` cleanup plans。
13. 若selected plan为空，删除journal并flush parent。
14. 对每个selected plan解析objectRole当前location。Movable object必须恰好在一个allowed location匹配stable identity；fixed object必须在唯一location匹配。
15. 使用解析出的具体identity作为receipt.source，并使用plan中的原receipt ID、operation、plannedTarget和measure写pending receipt。
16. Existing receipt必须逐字段匹配plan及当前解析结果；不得重新生成ID、target或第二个receipt。
17. Pending receipt durable后才允许rename到plannedTarget。
18. Rename成功后flush双方parent并更新同一receipt为quarantined；失败保持pending。
19. 全部selected receipt durable后写`cleanup-transferred`，cleanupReceiptIds精确等于selected plan ID集合。
20. 删除journal并flush parent后释放mutation lock。
21. Cleanup worker在lease释放后重新计量actual bytes并按固定batch处理receipt。

Receipt worker eligibility：

- `originOperationId`等于当前active journal `transactionId`的receipt由journal durable lease持有。
- Journal durable lease从第一个receipt写入前开始，到`cleanup-transferred` journal删除并flush parent后结束。
- Generic cleanup worker必须跳过仍被active journal lease持有的receipt。
- Transaction线程可在持有mutation lock和journal lease时执行本次receipt的rename，但不得执行最终递归删除。
- Journal删除并flush后，全部listed receipt才进入generic worker队列。
- Standalone reload receipt继续使用进程内lease。
- Startup必须先完成active journal recovery，再启动generic cleanup worker。

Prepared journal存在时，rollback根据old state使用if-old-state plans。Prepared journal不存在时不得创建事后receipt认领未知partial object；只有同步清理成功才允许正常返回。

Postcommit阶段若receipt、phase或journal删除失败，当前操作fail-stop。Recovery只重放durable objects和cleanupPlans。

### Recovery判定

启动时在migration、catalog和runtime创建前恢复。判定必须比较current canonical state digest、old/new state digest、typed objects和durable cleanupPlans。

1. Strict解析transaction operation与objects kind。
2. `install`和`update`都只接受objects.kind为install，且commandOperation必须相等。
3. 验证每个operation的字段基数、stable identity唯一性、allowed locations和cleanup plan覆盖关系。
4. Current state等于oldState时只选择if-old-state plans。
5. Current state等于newState时只选择if-new-state plans。
6. State已是new但phase仍prepared或package-placed时newState优先。
7. State既不等于old也不等于new时fail closed。
8. 对缺失receipt的selected plan解析当前object location。
9. Movable object必须恰好在一个allowed location匹配stable identity；0个、2个或identity不匹配时fail closed。
10. Fixed object必须在唯一location匹配。
11. 使用解析出的当前location构造receipt.source，并用plan中的原ID、plannedTarget和measure补建pending receipt。
12. Recovery不得重新生成receipt ID、planned target或measure。
13. Existing receipt.source必须逐字段匹配typed object当前唯一location；不一致时fail closed。
14. Plan外receipt、重复source或额外planned target时fail closed。
15. Prepared/package-placed/state-committed的cleanupReceiptIds必须为空。
16. Cleanup-transferred的cleanupReceiptIds必须精确等于selected plan ID集合。
17. Legacy package必须恰好出现在三个allowed locations之一；cleanupPlans必须为空。

Receipt recovery：

- Pending且receipt.source identity匹配、plannedTarget不存在时rename到plannedTarget。
- Pending且source不存在、plannedTarget identity匹配时rename已完成，更新为quarantined。
- Source与plannedTarget同时存在、同时不存在或identity不符时fail closed。
- Runtime关闭且不存在management lease后才允许处理runtime-data receipt。
- 递归删除前重新验证entry、directory、identity、path和actual bytes。
- Exact measure要求actual等于bytes；bounded measure要求actual不超过maxBytes。
- Worker按actual bytes计入batch。
- 任一不一致时零删除并进入transactionRecoveryRequired或fail closed。
- Recovery完成active journal处理前不创建runtime、不签发Plugin token，也不启动generic cleanup worker。

## 安装与更新事务

所有lifecycle mutation由全局mutation lock串行化，并复用plugin admission、staged ownership和固定500ms ready deadline。

1. 取得mutation lock并完成trigger preflight。
2. 创建staging前检查最坏receipt/quarantine容量，并预留唯一candidate runtime-data staged slot；未成功预留不得创建candidate WebView或data directory。
3. 复制development package到transaction staging。
4. Prepared journal前失败时同步清理partial staging；失败或崩溃遗留由startup认证后fail closed。
5. 从development staging稳定扫描verification package，验证canonical version、计算tree digest和candidate-package exact measure。
6. 判定new-version或activate-existing mode。
7. New-version继续使用staging package snapshot；candidate package stable identity允许staging与installed destination两个location。
8. Activate-existing用staging copy只验证version与tree digest；随后no-follow重新打开`active.json`登记的inactive package，验证完整`PackageIdentityV1`，并从registered package构建最终candidate snapshot。Staging verification snapshot不得被promotion。
9. 分配generation；只使用步骤7或8选定的最终candidate snapshot创建candidate runtime和candidate runtime data并等待ready。
10. 记录candidate runtime data root stable identity并使用固定bounded measure，不冻结运行中的内容。
11. 记录可选previous runtime data root stable identity并使用固定bounded measure。
12. 构建InstallObjectsV1：new-version的activationPackage为空；activate-existing的activationPackage精确指向registered package。
13. 按mode构建完整condition cleanupPlans；activationPackage永不进入cleanupPlans。
14. 检查capacity、ID、planned target、object cardinality和coverage invariant。
15. 取得write admission并最终复核health、generation、revision和state。
16. Reserve Plugin domain epoch和inventory revision。
17. 写入包含完整objects、cleanupPlans和空cleanupReceiptIds的prepared journal。
18. New-version把candidate package从staging rename到installed destination并写package-placed；activate-existing不移动registered package。
19. 在state commit前no-follow重新验证最终package：new-version验证installed destination，activate-existing验证registered inactive package。完整identity/digest必须精确匹配typed object、目标durable record和最终candidate snapshot；本次复核打开的package directory及全部普通文件handles禁止write/delete sharing，并保持到state commit和promotion完成。
20. 原子replace active.json并写state-committed。
21. 执行步骤19已完成preflight对应的不可恢复失败promotion，并commit reserved epoch/revision；此处不得新增磁盘读取或可失败验证。
22. 关闭步骤19的验证handles和旧runtime。
23. 根据new state选择if-new plans。
24. 对candidate package plan解析stable identity当前唯一allowed location；对runtime fixed role验证唯一location。
25. 使用解析出的具体identity写receipt.source，并在对应rename前持久化pending receipt。
26. 全部selected receipt durable后写cleanup-transferred和精确cleanupReceiptIds。
27. 删除journal并flush后返回PluginMutationOutcome。
28. Worker在runtime关闭、journal lease释放后重计量actual并处理receipt。

Rollback：

- Prepared journal存在时选择if-old plans。
- New-version即使phase为package-placed，也在staging与installed destination中解析同一candidate identity；必须恰好一个位置匹配。
- Activate-existing candidate package只允许在staging匹配；activation package保持登记且不得清理。
- Candidate runtime关闭后才能处理bounded runtime-data receipt。
- Prepared journal不存在时partial object只能同步清理。

## Reload事务

Reload只针对`installed.valid` active package，不修改`active.json`，也不创建transaction journal。每次attempt生成独立operation ID。

1. 验证active package并构建candidate asset snapshot。
2. 内容漂移时执行active drift failure转换并返回pluginReloadFailed。
3. 先在193-entry runtime-data root上限内预留唯一candidate staged slot，再分配generation、创建candidate runtime和candidate runtime data并等待ready；容量不足时零目录副作用。
4. 记录previous runtime data root stable identity和唯一location，不冻结运行中的内容。
5. 使用固定、不可配置的runtime-data maxBytes构建bounded measure。
6. Promotion前检查receipt/quarantine容量，生成receipt ID和planned target。
7. Promotion前写durable standalone pending receipt；source为previous runtime data当前具体identity，measure为bounded。
8. Manager登记进程内receipt lease；worker不得处理仍被当前reload lease持有的receipt。
9. 取得write admission并最终复核candidate health、generation和revision。
10. Reserve Plugin domain epoch和inventory revision。
11. 原子promotion candidate runtime/snapshot并commit reserved epoch/revision。
12. 关闭旧runtime。
13. 释放previous runtime data ownership和receipt lease。
14. Worker此后no-follow重计量actual；actual不超过maxBytes时才可rename/delete。
15. Reload成功返回PluginMutationOutcome。

Promotion前失败：

- 删除standalone receipt并flush。
- 关闭candidate runtime并同步清理candidate runtime data。
- Receipt删除或同步清理失败时fail-stop。
- 保留旧runtime、route和permission。

进程在pending receipt durable后崩溃时lease消失，所有runtime已经终止；startup recovery可以按原receipt处理previous runtime data。

## 删除活动版本与自动回退

### Commit前固定identity

Delete取得mutation lock后、durable commit前必须：

- No-follow打开active package directory。
- 验证volume/file identity和digest与`active.json`完全一致。
- 保持deleted package handle和journal ownership，直到对应pending receipt durable；之后无论rename成功或失败，cleanup ownership都由该receipt承接。
- 有fallback时，同样pin fallback package完整identity并构建fallback asset snapshot。
- 把deleted/fallback identity写入并flushtransaction journal。
- 提交后禁止按字符串path重新打开deleted对象再删除。

### 有fallback version

1. 选择canonical最高fallback version。
2. 先在193-entry runtime-data root上限内预留唯一candidate staged slot，再创建candidate runtime和candidate runtime data并等待ready；容量不足时零目录副作用。
3. Stable扫描deleted package并创建exact measure。
4. 为candidate runtime data和可选previous runtime data记录root stable identity、唯一location及固定bounded measure。
5. 构建DeleteWithFallbackObjectsV1和完整condition cleanupPlans。
6. 检查capacity、ID和planned target collision。
7. 取得write admission并最终复核ownership、health、generation、revision和state。
8. Reserve Plugin domain epoch和inventory revision。
9. 写入包含完整objects、cleanupPlans和空cleanupReceiptIds的prepared journal。
10. 原子replace active.json并写state-committed。
11. Promotion fallback runtime/snapshot并commit reserved epoch/revision。
12. 关闭old runtime。
13. 根据new state选择deleted package和previous runtime data plans。
14. Fixed object必须在唯一location匹配stable identity。
15. 使用具体identity写receipt.source；pending receipt必须在对应rename前durable。
16. Rename成功更新为quarantined，失败保持pending。
17. 全部selected receipt durable后写cleanup-transferred和精确cleanupReceiptIds。
18. 删除journal并flush后返回PluginMutationOutcome。
19. Worker在runtime关闭后按exact/bounded规则重计量actual并处理receipt。

若state仍为old，只选择candidate runtime data的if-old plan；deleted package、fallback package和previous runtime data继续保留。

### 删除最后version

1. Stable扫描deleted package并创建exact measure。
2. 为可选previous runtime data记录root stable identity、唯一location及固定bounded measure。
3. 构建DeleteLastObjectsV1和完整cleanupPlans。
4. 检查capacity、ID和planned target collision。
5. 取得write admission并复核state、generation和revision。
6. Reserve Plugin domain epoch和inventory revision。
7. 写入包含完整objects、cleanupPlans和空cleanupReceiptIds的prepared journal。
8. 原子replace active.json为合法empty state并写state-committed。
9. 移除active ownership、route和permission，commit reserved epoch/revision。
10. 关闭runtime。
11. 根据new state选择deleted package和previous runtime data plans。
12. Fixed object必须在唯一location匹配stable identity。
13. 使用具体identity写receipt.source；pending receipt必须在对应rename前durable。
14. Rename成功更新为quarantined，失败保持pending。
15. 全部selected receipt durable后写cleanup-transferred和精确cleanupReceiptIds。
16. 删除journal并flush后返回PluginMutationOutcome。
17. Container和empty active.json永久保留。
18. Worker按exact/bounded规则重计量actual并处理receipt。

若state仍为old，selected cleanup plan为空；active package和previous runtime data不得清理。

删除成功保证version不再登记，当前进程和重启均不能加载。成功不承诺quarantine bytes立即擦除。

### Path replacement

- 当前进程先持久化pending receipt，再使用commit前保持的handle rename原对象。
- Pending receipt始终保存原volume/file identity；`plannedTarget`在首次持久化前按固定ID映射确定，此后rename和recovery只读取该持久化值，不重新计算或改变ownership来源。
- Rename成功但receipt尚未更新时，recovery以receipt中持久化的`plannedTarget`及原identity识别并完成`quarantined`转换。
- Recovery只对identity匹配的source/target操作。
- 原path被替换时不移动、不删除replacement；receipt保持pending并显示recovery fault。
- 新mutation不能占用仍由receipt持有的相同identity/path目标。

## 删除后重装同一ID

Empty state保留在原container中；development source仍显示未安装。同一进程reinstall：

1. 复用empty container和`active.json`。
2. 查询仍保留的`generationHighWater[pluginId]`。
3. Checked分配下一generation和新label。
4. 走完整install transaction和epoch/revision reservation。
5. 原子把empty state替换为non-empty state。
6. 不复用旧generation、label、pending或action identity。

旧runtime延迟callback解析为无ownership；旧pending不能publish；旧CopyText action generation校验失败。

## Legacy 单版本迁移

### 识别与 basename 规则

Legacy 候选是 plugin root 直接子目录，包含根级 `plugin.json` 且没有有效 `active.json`。

现有 loader 允许目录 basename 与 manifest ID 不同。迁移采用以下规则：

- Manifest ID 有效时，以 manifest ID 作为目标 container 名。
- Basename 不等于 manifest ID 不是单独拒绝原因。
- Source 目录通过 no-follow handle 固定 identity 后，可以安全 rename 到 `<manifest.id>/<canonical-version>`。
- 新布局 container 名必须始终等于 state/manifest plugin ID。
- Mixed 布局，即同一目录同时包含 legacy 根级 package 文件和 new-layout state/version 目录，固定为 `migrationConflict`，不猜测。

### 全局 collision plan

迁移任何文件前，宿主先扫描全部 legacy 候选和全部 new-layout container，构建不可变全局 plan：

```text
LegacyMigrationPlan {
  candidates: PlannedLegacyMigration[],
  catalogTriggers: Map<Trigger, PluginId>,
  targetPluginIds: Set<PluginId>
}
```

Preflight 检查：

- 多个 legacy 目录声明相同 plugin ID。
- 多个 legacy 目录声明相同 trigger。
- Legacy trigger 与任一 new-layout active plugin 冲突。
- Legacy ID 与任一 new-layout container 冲突。
- Basename mismatch 后的目标 container 已存在。
- 目标 canonical version 已存在。
- 同一 source identity 被多个 candidate 引用。
- Legacy manifest、version、package digest 或资源预算无效。
- 新旧布局在不同 container 中声明相同 plugin ID。

任一逻辑 collision 时，不移动任何 candidate；冲突项显示 `migrationConflict`。不能先迁移一个包后才发现另一个 ID/trigger 冲突。

全局 plan 通过后，按 plugin ID 排序逐项执行 journaled migration。某个 candidate 后续发生 I/O 失败时，已经提交的早期 candidate 可以保持提交，但失败不能来自遗漏的全局逻辑 collision。下一次启动重新对剩余 legacy 和已迁移 new-layout 构建全局 plan。

### 单项 migration

1. Pin legacy package stable identity。
2. 计算canonical version和PackageIdentityV1。
3. 构建MovableTransactionObjectV1；allowedLocations严格为legacy source、transaction staging和installed destination。
4. 构建LegacyMigrationObjectsV1；cleanupPlans必须为空。
5. 写入包含typed objects、空cleanupPlans和空cleanupReceiptIds的prepared journal并flush。
6. 把legacy package rename到transaction staging并flush双方parent。
7. 更新journal为package-placed；objects不得变化。
8. 创建canonical plugin container。
9. 把同一stable identity rename到installed destination并flush双方parent。
10. 原子写入non-empty active.json并写state-committed。
11. 删除journal并flush parent。
12. 正常catalog load后创建runtime。

Recovery在三个allowed locations中查找stable identity，必须恰好一个匹配。0个、2个或identity不符时fail closed。Legacy migration不生成cleanup receipt，也不删除无法认证的对象。

## 设置页交互

### 覆盖 vertical Tabs 既有契约

本设计明确覆盖 `2026-07-24-settings-vertical-tabs-design.md` 中以下两条：

- “Tab 切换不得调用 `list_plugins`”。
- “每次进入 settings 立即 eager 加载 plugin list”。

新的唯一加载契约：

- 进入 settings 时只 eager 加载普通 settings，不调用 `list_plugins`。
- 新 settings epoch 首帧仍固定选择 `通用`。
- 只有从非 plugins 状态切换为 plugins 时调用一次 `list_plugins`。
- 点击刷新按钮额外调用一次。
- React rerender、焦点变化、重复选中已 active 的 plugins Tab 均不得重复调用。
- 从 plugins 切到 general，再切回 plugins 时再次调用一次。
- 离开 settings 后重新进入，仍从 general 开始；首次进入 plugins 时调用一次。

### LauncherView 到 LauncherCore 所有权

Tab 选择仍由 LauncherView 本地控制，但可见性通过同步内部接口通知 LauncherCore：

```text
setPluginInventoryActive(viewEpoch, active: boolean)
refreshPluginInventory(viewEpoch)
```

规则：

- `false -> true` 且 epoch 为当前 settings epoch 时，Core 签发新的 list owner 和 operation token。
- 重复 `true` 是幂等 no-op。
- 切到 general 时发送 `active=false`，不发 list。
- 离开 settings 时 Core 自动把当前 plugin inventory active 标记为 false。
- Ant Design 方向键只在实际 activeKey 变为 plugins 时触发一次。
- 点击和 focus-capture 同时发生时，由相同 epoch/key transition 去重。
- 刷新按钮即使当前已 active，也显式签发一个新 token。
- View 不直接调用 backend client；所有 owner/revision 规则仍在 Core。

### UI

插件区标题右侧增加刷新图标按钮和 tooltip。右侧插件面板继续使用独立滚动容器和主界面同款滚动条，不增加嵌套卡片。

每个插件项展示：

- `displayName`；
- 活动触发词或有效开发触发词；
- `未安装`、`已安装`、`可更新`、`开发包不可用`或`安装状态故障`；
- 当前版本、development 可用版本和全部已安装版本；
- 当前版本明确标记；
- 安全 Markdown 说明或“未提供说明”。

按钮规则：

| 状态 | 操作 |
| --- | --- |
| installed.absent + development.valid | 安装 |
| installed.valid，无更新 | 重新加载、删除当前版本 |
| installed.valid，可更新 | 更新、重新加载、删除当前版本 |
| installed.valid + development.invalid/collision | 重新加载、删除当前版本；更新禁用 |
| installed.invalid | 全部 mutation 禁用，只允许刷新 |
| development.invalid 且未安装 | 安装/更新禁用 |

删除确认显示被删 active version；存在 fallback 时同时显示将自动启用的最高 canonical version。

### Owner、revision与reconciliation

- Plugin inventory状态独立于普通settings和逐行mutation。
- `loading`显示loading；`error`显示固定错误和retry。
- 只有`ready`且items为空时显示“没有已安装或可用的开发插件”。
- 只有current epoch/token owner的`list_plugins` snapshot可以写inventory rows。
- Current list owner仍必须通过`compareDecimalRevision`和`highestPluginRevision`检查。
- Mutation outcome或error永远不直接增加、替换或删除row。
- 每个install/update/reload/delete命令settled后都签发或合并一次list reconciliation。
- Success先记录outcome revision；error没有revision也照常reconcile。
- Precommit capacity、ID、location、planned target或measure错误即使零revision变化，也执行相同reconciliation。
- Postcommit fail-stop不返回可继续交互的普通row error。
- Settled response属于旧epoch时不能写row或当前错误；若当前plugins active则为当前epoch签发list，否则只标dirty。
- 同批settled通知可以合并为一个当前epoch list owner，但不能吞掉最终刷新。
- Mutation error只在仍拥有当前row operation时显示固定错误；错误状态与inventory snapshot独立，不能阻止reconciliation应用backend清单。
- Plugins inactive时不后台scan；下一次`false -> true`消费dirty并强制list。
- Reconciliation token取代更早list owner。
- List revision低于highest时丢弃；相同revision由最新token决定。
- 一行operation只锁定该行；backend mutation lock负责跨行串行lifecycle。
- Install/update/reload/delete/refresh不保存或覆盖普通settings。

## Runtime callback ownership

- Ready、process-failed 和 close callback 只捕获不可变 `plugin_id + label + generation`。
- Callback 取得 admission 后由 manager 动态解析 identity 属于 staged、active 或无 ownership。
- Staged ready 只设置该 attempt ready 并唤醒 waiter。
- Staged failure 只设置 failed 并唤醒 waiter，不 disable 活动 generation。
- Promotion 在同一 manager 临界区把 asset/ownership 从 staged 原子移动到 active。
- 同一 identity 不能同时属于 staged 和 active。
- Promotion 后 callback 动态解析为 active，可执行当前 generation failure 转换。
- Rollback、旧 active 替换和 delete 已移除 ownership 后，延迟 callback 解析为无 ownership。
- Delete-last 后 reinstall 的新 generation 和 label 不能与旧 callback 碰撞。

## Query、action 与副作用线性化

- PluginRoute、PendingPluginQuery 和内部 CopyText action 均绑定 plugin ID 和 generation。
- Query 签发 token 和登记 pending 时取得 read admission。
- Publish 取得 read admission，验证 callback label、pending generation 和当前 active generation。
- Mutation 和 runtime failure 取得 write admission。
- Mutation durable commit 前 reserve 下一 Plugin domain epoch。
- Commit 后旧 token 即使晚到也不能 publish。
- CopyText action 保留签发 generation。
- Execute 先 resolve 并释放 ResultRegistry lock，再取得 read admission。
- Generation、active permission 校验和 clipboard write 在同一 read admission 内完成。
- Write admission 不能穿透校验与剪贴板副作用之间。
- Domain terminal exhausted 后不签发 token、不 publish、不写 clipboard。

## 锁顺序

固定顺序：

```text
mutation lock（仅管理命令和 migration）
  -> plugin admission gate
    -> active/staged catalog、generation high-water、inventory revision
      -> ready/disabled/timeout/pending 单个状态锁
        -> ResultRegistry
```

约束：

- 不同时持有两个 ready/disabled/timeout/pending 细粒度锁。
- `ResultRegistry::resolve` 在取得 admission 前完成并释放 registry lock。
- 不在任何 admission、catalog、state 或 ResultRegistry lock 下等待 runtime ready。
- 不在 manager/catalog/ResultRegistry lock 下复制、递归扫描、hash、关闭 WebView 或递归清理。
- Write admission 内只允许有界的 state/journal atomic replace、handle rename 和不可失败内存 commit。
- Clipboard 副作用允许持有 read admission，但不能持有 manager/catalog/state lock。
- Runtime failure 与管理 mutation 都通过 write admission 串行 epoch/revision reservation。
- Inventory scan 不持有 admission；通过 revision revalidation 避免 installed snapshot 撕裂。

## 安全边界

- Debug development root 由宿主编译期定位，前端和插件不能修改。
- Source 先复制到 transaction 暂存区，提交只使用重新验证的暂存副本。
- Manifest、runtime、README、assets、version 目录和 migration 目录均拒绝 reparse point。
- 文件操作绑定 no-follow handle、volume/file identity 和 package digest。
- 不能只依赖 canonicalize 后的字符串检查。
- Staging、quarantine 和 transaction 目录与 plugin root 同卷但不在扫描入口内。
- 名称由宿主生成且禁止覆盖。
- Version 目录只来自 canonical version。
- Plugin ID 只来自已验证 manifest/state。
- Markdown 禁用 HTML、链接、图片和外部资源。
- DTO 不暴露 path、permission 详情、runtime 入口、WebView label、generation、digest、journal 或 transaction ID。
- Release development source通过`#[cfg(debug_assertions)]`在编译期消除；production stub不包含source path字符串或scanner引用。
- Release 不扫描 development source，`install_plugin` 固定 fail closed。
- Main-window guard 在任何 source 读取或文件副作用前执行。

## 实施文件清单

本需求在现有文件边界内实施，不预先拆分新的Rust/TypeScript模块。

### Rust后端

- `src-tauri/src/plugins.rs`
  - Canonical version、package/root/recovery/cleanup-worker budget、trigger collision。
  - Active state、package identity、immutable asset snapshot和inventory DecimalRevision DTO。
  - Typed transaction objects、stable identity、movable allowed locations和fixed object location验证。
  - Durable CleanupReceiptPlanV1、operation role/cardinality、condition coverage和phase invariants。
  - Exact/bounded CleanupMeasureV1、runtime-data fixed budget、worker actual remeasurement和batch accounting。
  - Operation/receipt ID create-new、fixed planned target mapping、journal durable lease和standalone reload receipt lease。
  - Runtime-data root bounded inventory、ownership classification和unknown fail-closed。
  - Development scan、install/update/reload/delete、deterministic recovery和legacy migration。
  - `asset_response`改为只读generation snapshot，不再per-request读取filesystem。
  - 扩展现有plugin unit tests。
- `src-tauri/src/result_registry.rs`
  - Plugin domain epoch reserve/cancel/commit。
  - 扩展epoch/token/publish/resolved action tests。
- `src-tauri/src/commands.rs`
  - 更新list DTO；mutation只返回`PluginMutationOutcome`。
  - 新增`install_plugin`并保持main-window guard。
- `src-tauri/src/lib.rs`
  - Startup recovery/migration、command wiring和production contract tests。
- `src-tauri/Cargo.toml`
  - 增加直接依赖`sha2 = "=0.10.9"`，使用已有可审计实现，不新增自写或Win32 Crypto hash封装。
- `src-tauri/Cargo.lock`
  - 由Cargo更新root `uipilot` dependency edge；已有`sha2 0.10.9` package版本不变。
- `src-tauri/build.rs`
  - 把`install_plugin`加入command allowlist。
- `src-tauri/capabilities/main.json`
  - 给main WebView增加`allow-install-plugin`。
- `src-tauri/capabilities/plugin-runtime.json`
  - 不增加任何plugin管理权限。
- `src-tauri/permissions/autogenerated/`
  - 通过现有Tauri build流程生成install command permission；不手写generated内容。

### 前端协议与状态

- `src/protocol.ts`
  - 定义DecimalRevision string、inventory union、issue enum、description和mutation outcome。
  - Strict parser拒绝number revision、非canonical decimal和冗余字段。
- `src/main.ts`
  - 更新list/mutation adapter，新增`install_plugin` invoke。
- `src/launcher-core.ts`
  - 删除settings-entry eager plugin list。
  - 管理Tab active、list owner、dirty reconciliation和DecimalRevision comparator。
  - 所有management mutation settled都触发或合并list reconciliation；success先合并revision，error无revision但同样不直接写row。
- `src/launcher-view.tsx`
  - Tab transition通知Core；增加refresh、derived status、versions和install/update UI。
- `src/styles.css`
  - 只增加inventory status/version/action样式，复用现有scrollbar。
- `src/launcher.test.tsx`
  - 更新fixtures，覆盖invocation count、DecimalRevision、owner/reconciliation、derived state和row operation。

### 示例包

`examples/plugins/internal.math`现有四个文件继续作为development sample。功能实现不提交临时version变更；自动测试使用test temp directory，人工更新测试显式修改后再恢复。

## 自动测试

### Version、state和预算

- 接受canonical `0.0.0`、`1.2.3`和`4294967295.0.1`。
- 拒绝leading zero、第四段、prerelease、build metadata、空白和u32 overflow。
- 等价version不同文本不能进入packages。
- Non-empty state要求active属于packages。
- Empty state只接受`activeVersion:null`和空packages。
- Empty state重启后保持未安装，container和state永久存在。
- Container存在但state缺失且无legacy journal时为`stateMissing`。
- Installed/development root entry、plugin count、每plugin version count和全次scan files/directories/bytes分别覆盖等于上限与超过上限。
- Transaction root直接entry、active journal、staging object、durable planned receipt、实际receipt、planned target、quarantine object和startup recovery总扫描分别覆盖等于上限与超过上限。
- Runtime-data root 193个直接entry为边界，第194个fail closed；startup direct-entry总数590为边界，第591个fail closed。
- Runtime-data扫描覆盖8192目录、16384文件和1 GiB logical length的等于/超过边界。
- Active、staged、journal-referenced、receipt-referenced和unknown分类分别覆盖；startup unknown entry只报告故障且零删除。
- 同一runtime-data entry被plan与同ID receipt引用时只计一次。
- 相同ID的plan与receipt只占一个receipt slot；不同ID分别计数。
- Cleanup worker在8个receipt、8个quarantine object和64 MiB actual bytes边界停止本批。
- 下一个对象会超过batch时完整留到下一批，不能部分删除。
- Bounded runtime-data使用actual bytes计费，不能按maxBytes预扣或少计。
- Root/global/recovery预算越界不返回截断inventory、不部分恢复、不删除未知对象。
- Active trigger duplicate使双方fail closed；development-development和development-other-active冲突使candidate invalid。
- 同plugin update复用自己的trigger合法；mutation preflight发现并拒绝inventory后新产生的trigger冲突。

### Digest、path和asset snapshot

- `sha256-tree-v1` prefix、entry_count、u8 type、u32le path length、u64le file length和32-byte digest有golden vector。
- Package root不编码、不计入entry_count和directory预算。
- 只有plugin.json时golden vector的entry_count为1。
- 64个descendant目录为合法边界，第65个触发resourceLimitExceeded。
- Root identity变化只改变PackageIdentityV1 identity字段，不改变相同descendant内容的tree digest。
- Directory entry不编码file length/digest；file entry编码完整字段。
- Entry排序使用NFC canonical path UTF-8 bytes。
- 非NFC path和Windows ordinal ignore-case alias/collision被拒绝。
- Manifest/runtime path大小写不精确时被拒绝。
- Digest覆盖manifest、runtime、assets和README；任一内容/path/type/length变化改变digest。
- Directory volume/file identity变化产生identity fault。
- Reload拒绝same-version内容漂移。
- Inactive registered version与development same-version不同digest产生`versionContentCollision`。
- Same-version/same-digest允许激活，不覆盖。
- 单包file/directory/depth/size/path/extension预算边界完整覆盖。
- 拒绝reparse、hard link、device、ADS和无效UTF-8。
- Snapshot构建期间存在writer或验证前后identity/length变化时失败。
- Snapshot总bytes严格不超过16MiB。
- `asset_response`在磁盘文件被替换、修改或删除后仍只返回原snapshot bytes。
- Active drift检测在线性化前只服务旧snapshot；write admission转换后route/token/action失效并关闭runtime。
- Staged rollback/promotion/active close按generation ownership释放snapshot，无use-after-close。

### DTO、revision和derived state

- Installed absent/valid/invalid和development union strict parse。
- DTO拒绝冗余`updateAvailable`字段。
- `derivePluginPresentation`真值表覆盖全部union组合。
- Invalid installed仅在header可信时显示canonical versions，不返回未验证description/trigger。
- Invalid development返回full 64-hex key；两个相同12-hex prefix但不同full digest不发生key collision。
- DisplayName只使用12-hex prefix且不泄露basename/path。
- README unavailable不使package invalid。
- DecimalRevision拒绝JSON number、leading zero、sign、decimal和u64 overflow。
- Comparator覆盖2^53边界与`u64::MAX`，不经过JavaScript Number。
- Internal inventory revision接近`u64::MAX`时最后一次reserve成功，overflow在durable commit前fail closed。

### Inventory一致性和mutation outcome

- Scan期间install/update/delete/reload/runtime failure提交时，revision revalidate使旧scan重试。
- 连续3次revision变化返回`pluginListFailed`，扫描不持manager/admission lock。
- Mutation success只返回canonical DecimalRevision，不返回PluginInventoryView。
- Durable commit后development scan失败不改变mutation success outcome。
- Mutation outcome或error不能直接新增、替换或删除row。
- 每个当前epoch management mutation settled合并一次list reconciliation。
- Success先提高`highestPluginRevision`；error不携带revision也必须发list。
- 旧epoch settled response在plugins active时为当前epoch触发list，inactive时只标dirty；旧error本身不能写当前row错误。
- Reload检测active drift、推进revision并返回`pluginReloadFailed`后，reconciliation最终显示backend disabled/fault状态。
- Delete和install preflight触发fail-closed转换并返回error后，reconciliation最终清单与backend活动目录一致。
- Mutation outcome提高highest revision后，较低revision list被拒绝。
- 相同revision development snapshots由最新list token决定。
- 旧mutation response晚于较新same-revision list时不能覆盖development状态。

### Generation、epoch和runtime callback

- Epoch/revision reservation接近`u64::MAX`的成功/overflow行为确定。
- Reservation失败零durable commit并terminal fail closed。
- Durable commit后reservation invariant失败触发fail-stop，不返回普通失败。
- Install/update/reload/fallback delete/delete-last/runtime failure使用同一write admission互斥。
- Delete-last后generation high-water保留。
- Reinstall分配更高generation和不同label。
- Staged rollback消耗generation，不复用。
- Reinstall后旧ready/process-failed/close callback不能影响新active。
- 旧pending不能publish；旧resolved CopyText不能写clipboard。
- Generation overflow使对应ID terminal fail closed。

### Journal、receipt和recovery

- PluginTransactionV1、typed objects、CleanupReceiptPlanV1、CleanupMeasureV1和CleanupReceiptV1 strict验证。
- Transaction operation install/update都接受且只接受objects.kind install，并验证commandOperation一致。
- New-version要求activationPackage为空；activate-existing要求activationPackage exactly-one、完整匹配registered record且永不进入cleanupPlans。
- New-version candidate package只允许staging与installed destination；activate-existing verification candidate只允许staging。
- 各operation/mode的old/new cleanup coverage table逐项覆盖缺失、额外、重复和不存在role。
- Movable object在0个、1个、2个allowed location匹配的行为分别为fail closed、成功、fail closed。
- Fixed object出现于非唯一location或identity不匹配时fail closed。
- Cleanup plan只引用objectRole，不包含source path。
- Receipt.source逐字段匹配typed object当前唯一location解析结果。
- CleanupPlans在prepared journal中durable、严格排序、无重复且最多8项。
- Prepared/package-placed/state-committed要求cleanupReceiptIds为空。
- Cleanup-transferred要求cleanupReceiptIds精确等于selected plan ID集合。
- Pending要求target null；quarantined要求target位于固定planned target。
- PlannedTarget拒绝非`quarantine-root`、relativePath不等于完整receipt ID、截断ID或子目录；recovery只读持久化值且不得重新计算。
- Pending到quarantined不得修改ID、source、plannedTarget、operation或measure。
- State-committed存在部分receipt时只采用plan原ID，不生成第二个receipt。
- New-version phase为package-placed且state仍old时，从installed destination解析candidate identity并创建rollback receipt。
- Staging和installed destination同时出现相同identity时fail closed。
- Exact package measure覆盖相等、低报、高报和内容变化。
- Bounded runtime-data覆盖actual小于、等于、超过maxBytes。
- Runtime-data entry/directory超限、identity/path不符时零删除。
- Worker按actual bytes执行64 MiB batch边界。
- Reload lease存在时worker不处理receipt；crash后lease消失。
- State-committed写出首个pending receipt后，以及cleanup-transferred但journal尚未删除时，generic worker都不得处理；journal删除并flush后才可处理。
- Startup active journal recovery与generic worker不得并发启动。
- Pre-journal partial object同步清理成功、失败和崩溃遗留分别覆盖。
- Legacy package在三个allowed location中必须恰好一个匹配，cleanupPlans必须为空。
- Recovery完成active journal处理前不创建runtime/token。
- Exact package保持相同volume/file ID、文件数和总长度，但修改文件内容：重算digest不一致，零rename、零删除。
- Exact package以等长内容替换单个文件：bytes相等但digest不相等，零删除。
- Exact package receipt.source.packageDigest为null：strict parse失败。
- Exact package receipt digest与typed object digest不同：fail closed。
- Exact package重算digest与receipt和typed object均不同：进入transactionRecoveryRequired或fail closed。
- Bounded runtime-data typed object携带非null packageDigest：strict parse失败。
- Bounded runtime-data receipt.source携带非null packageDigest：strict parse失败。
- Package role使用bounded measure或runtime-data role使用exact measure：strict parse失败。
- Exact package的bytes、entry、identity和digest全部匹配时才允许清理。

### Install、reload和delete

- Install/new-version与activate-existing分别验证typed objects、allowed locations和old/new cleanup plan覆盖。
- Activate-existing的development staging与registered package digest相同、file ID不同时，只promotion registered snapshot。
- Registered inactive package identity/digest在初次验证或ready等待后、commit前复核时漂移，activate-existing都失败；最终复核handles保持到state commit/promotion完成，该窗口内write/delete尝试被拒绝；promotion snapshot identity必须精确等于active.json目标record。
- Activation package进入cleanupPlans或staging verification snapshot成为active generation时strict测试失败。
- New-version candidate package从staging移动到installed destination后仍使用同一stable identity。
- Package使用exact measure；candidate/previous runtime data使用固定bounded measure。
- Candidate timeout、ready后failure、receipt/quarantine或runtime-data slot容量不足、ID/location collision和measure invariant失败均发生在commit前；runtime-data root已满时install/reload/fallback delete均为零candidate目录和零durable副作用。
- Prepared journal前partial staging只允许同步清理。
- Install rollback在staging或installed destination中解析candidate当前唯一location。
- Reload在promotion前写durable standalone bounded receipt并登记lease。
- Reload运行期间不要求冻结WebView data directory内容。
- Reload promotion失败删除receipt并同步清理candidate；失败时fail-stop。
- Reload promotion成功后关闭old runtime、释放lease，再由worker重计量actual。
- Reload崩溃后lease消失，startup使用原receipt。
- Fallback delete的deleted package使用exact，candidate/previous runtime data使用bounded。
- 两条delete都在对应rename前持久化pending receipt。
- Delete old-state只清理candidate runtime data；new-state只清理deleted package和previous runtime data。
- Delete-last old-state selected plan为空；new-state清理deleted package和previous runtime data。
- 两条delete的rename成功和失败路径都保留receipt直到cleanup完成。
- Reinstall复用empty container并原子提交non-empty state。

### Legacy migration

- Basename等于/不等于manifest ID的合法迁移。
- Target存在、legacy duplicate ID/trigger、legacy-new conflict和mixed layout在任何move前失败。
- Root/global预算在plan阶段执行。
- 全局plan通过后才执行第一个move。
- Journal phase使用`state-committed`，不存在undefined`committed` phase。
- Cleanup需要延期时完成receipt handoff再删除journal。
- 每个crash point恢复确定；无法认证identity时不移动、不删除。

### Debug/Release wiring

- Debug build包含`development_plugin_root`和scanner并能列出examples。
- `#[cfg(not(debug_assertions))]`只编译empty source/fail-closed install stub。
- Production source contract验证Release binary/source wiring不包含`CARGO_MANIFEST_DIR`、`examples/plugins` path或Debug scanner引用。
- Plugin runtime capability不能调用install/list/reload/delete。
- Main-window guard在development scan和文件副作用前拒绝其他window。

### 前端调用与UI

- 进入settings不调用listPlugins；首次进入plugins恰好一次。
- Arrow/click/focus-capture同一transition不重复。
- Active plugins rerender不调用；离开再进入触发下一次。
- Refresh每次一个新owner。
- Hidden tab mutation settled只标dirty；下一次进入触发list。
- Current tab mutation error显示固定row错误，同时触发新owner list并应用backend snapshot。
- Old epoch mutation error不写当前row错误，但仍按当前active/dirty规则触发或延迟reconciliation。
- Old epoch/token/lower revision不能覆盖current inventory。
- Mutation success或error都不直接写row，最终row只来自reconciliation list。
- Reload drift、delete preflight和install preflight在error前推进backend revision时，最终row自动对齐backend。
- 正确渲染derived未安装/已安装/可更新/source invalid/installed invalid。
- Installed invalid禁用全部mutation。
- Description source和safe Markdown正确。
- Delete确认显示deleted/fallback version。
- 长list只滚动右侧，scrollbar与主界面一致。

## 人工验收

1. 清空AppData后启动Debug；进入settings不scan，首次点击“插件”后看到`internal.math`未安装。
2. 安装后`/math 1+1`显示并复制`2`；检查version directory、computed identity和active state。
3. 提升examples到`0.3.0`，重新进入plugins看到derived可更新；更新后两个version均登记。
4. 制造candidate timeout，确认旧active继续工作。
5. 修改active runtime file后，现有窗口不读取新bytes；刷新触发drift fail closed并禁用route。
6. 保持same version但修改development内容，显示version-content collision。
7. 删除`0.3.0`后回退canonical最高`0.2.0`。
8. 删除最后version后`/math`失效；container和empty`active.json`保留，重启仍显示未安装。
9. 同进程reinstall复用empty container，generation/label不复用。
10. 制造package/root/global、transaction、receipt、quarantine和startup recovery预算越界，确认没有截断inventory、部分恢复、未知对象删除或部分route。
11. Basename mismatch legacy在global plan无冲突时迁移；duplicate ID/trigger时所有source保持原位。
12. 快速切换Tab、refresh，以及成功和失败的mutation，确认每次settled都按active/dirty规则reconcile，inventory rows只由list snapshot更新。
13. Release build不显示development source，且binary/wiring不保留examples source path。

## 与既有设计的关系

本设计扩展并覆盖 `2026-07-23-plugin-management-settings-design.md` 中的以下内容：

- 插件只能由开发者直接放入 AppData。
- 单版本 package 目录。
- 整插件删除。
- 旧 PluginView DTO 和三命令接口。
- 无全局刷新按钮。
- 删除成功必须移除整个plugin ID container的旧语义；新语义删除当前version，delete-last永久保留合法empty container/state，只隔离最后version目录。

本设计明确覆盖 `2026-07-24-settings-vertical-tabs-design.md` 中的以下内容：

- Tab 切换不得调用 `list_plugins`。
- 每次进入 settings 立即 eager 加载 plugin list。
- Tab 选择完全不向 LauncherCore 传递任何生命周期信号。

Vertical Tabs 的布局、焦点、ARIA、右侧独立滚动和普通 settings 加载规则继续有效。LauncherCore 只接收 plugin inventory active/inactive 信号，不接管 Tab activeKey。

既有插件管理设计中的 README 安全渲染、runtime readiness、generation 副作用线性化、callback 动态 ownership、admission gate、主窗口 caller guard 和隔离目录安全约束继续有效。本设计明确覆盖的 state、version、transaction、delete 和 list 契约以本设计为准。
