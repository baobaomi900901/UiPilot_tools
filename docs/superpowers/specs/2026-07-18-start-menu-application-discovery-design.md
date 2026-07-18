# Foundation Start Menu 应用发现合同设计

## 状态

- 日期：2026-07-18
- 状态：已批准
- 影响范围：Foundation Task 3 的应用发现、快捷方式解析、reparse point 与 `appId`

## 目标与边界

Task 3 只从 Windows 开始菜单发现可启动的 `.lnk` 入口。快捷方式路径和可执行文件路径只保存在 Rust；TypeScript 仍只能接收 Task 2 已批准的展示 DTO，并通过 `requestId + resultId` 请求执行。

本合同不使用目标程序身份，也不对快捷方式或目标调用 `canonicalize`。应用身份来自开始菜单入口本身；实际启动始终使用可信的 `.lnk`，解析出的可执行文件仅供后续尽力激活窗口。

本合同不改变排序、别名、最近使用次数、命令接线或 UI 合同。它们继续由 Foundation 计划的后续步骤负责。

## 扫描作用域

生产扫描器只能通过 `SHGetKnownFolderPath` 查询并扫描以下两个 Known Folder：

```text
user   = FOLDERID_Programs
common = FOLDERID_CommonPrograms
```

调用固定使用 `KF_FLAG_DONT_VERIFY` 和 `hToken = NULL`（当前用户），不创建目录，也不读取 `%APPDATA%`、`%ProgramData%` 或其他环境变量作为回退。该标志让根查询与目录存在性检查分离；API 返回的当前 Known Folder 路径可以是系统配置的重定向位置。返回内存必须通过 `CoTaskMemFree` 释放。微软将 `SHGetKnownFolderPath` 定义为按 `KNOWNFOLDERID` 获取完整路径的接口：[SHGetKnownFolderPath](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath)、[KNOWNFOLDERID](https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid)。

生产根提供者不接受前端、配置文件、命令行或环境变量提供的路径。测试只能绕过根提供者，向内部扫描函数注入临时根；该注入接口不得注册为 Tauri 命令或进入生产配置。

`rootKind` 的身份字节固定为 ASCII `user` 与 `common`，不得使用 Known Folder GUID、本地化名称、绝对根路径或枚举序号代替。

扫描结果必须一次性生成：只有两个 Known Folder 查询和两个根扫描都完成且没有硬错误时，调用方才可替换旧缓存。任何 Known Folder 查询失败都使整个扫描失败并保留旧缓存；查询成功但返回的根路径不存在时，该根贡献零条结果。根已存在但不是目录、无法读取，或根自身带 `FILE_ATTRIBUTE_REPARSE_POINT` 时，整个扫描失败并保留旧缓存。

目录遍历使用 `std::fs::read_dir`。每个条目在进入目录或接受文件前读取自身元数据；任何带 `FILE_ATTRIBUTE_REPARSE_POINT` 的子目录或文件都直接跳过，不跟随、不解析目标。其他无法读取的子项、损坏快捷方式和无效 Unicode 路径只跳过该项，不使整个扫描失败。

只接受普通文件，扩展名以 ASCII 大小写不敏感方式等于 `.lnk`。遍历顺序按规范化相对路径排序，保证相同输入得到相同顺序。路径范围由配置根与遍历时得到的相对组件确定，不调用 `canonicalize`、不扫描任意磁盘，也不根据快捷方式目标扩展扫描范围。

## 路径与 `appId`

相对路径从当前配置根通过词法 `strip_prefix` 得到。规范化算法固定为：

1. 路径至少包含一个组件，且所有组件都必须是普通组件；出现前缀、根、当前目录或父目录组件时跳过该项。
2. 每个组件必须能通过 `OsStr::to_str` 无损表示为 UTF-8。不得使用 `to_string_lossy` 或替换无效码点；无法无损转换时跳过该项。
3. 用反斜杠 `\` 连接组件，保留 `.lnk` 扩展名，不修剪或解析组件。
4. 对完整相对路径使用 Rust 与现有排序合同一致的、与区域设置无关的 Unicode `to_lowercase`。规范化只折叠大小写，不改变目录结构或文件名的其他字符。

预映像固定为以下 UTF-8 字节串：

```text
"start-menu-v1\0" + rootKind + "\0" + normalizedRelativeShortcutPath
```

使用 Windows CNG SHA-256 计算摘要，输出固定为 `app-` 加 64 位小写十六进制。显示名称、别名、绝对根路径、快捷方式目标、参数、图标和文件元数据都不得进入预映像。

因此，同一根下仅大小写变化的重命名保持同一 `appId`；改变文件名中的非大小写字符、改变相对目录、在两个根之间移动，均产生新 `appId`。不同根中的同名同相对路径入口也保持不同身份。快捷方式目标变化不改变 `appId`。

一次扫描内出现重复 `appId` 时，整个扫描失败并保留旧缓存，不合并或覆盖条目。

## 快捷方式解析

解析器在后台 Rust 线程初始化 COM，通过 `IShellLinkW` 与 `IPersistFile` 只读加载 `.lnk`。COM 初始化或 Shell Link 对象创建失败属于扫描级硬错误；某个 `IPersistFile::Load` 失败只表示该快捷方式损坏并跳过该项。

加载成功后，只调用：

```text
IShellLinkW::GetPath(..., SLGP_RAWPATH)
```

不得调用 `IShellLinkW::Resolve`，不得展开环境变量，不得解析相对路径，也不得读取或探测目标。微软说明 `SLGP_RAWPATH` 可能返回不存在或含环境变量的原始路径，因此解析结果不作为入口有效性的前提：[IShellLinkW::GetPath](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-getpath)。

`IPersistFile::Load` 成功即保留该快捷方式。只有 `GetPath` 返回非空、可无损转换、无 `%` 环境变量标记，并且是带盘符的绝对 `.exe` 路径时，才设置 `executable = Some(path)`。以下任一情况固定为 `executable = None`：

- `GetPath` 返回 `S_FALSE`、空值或错误；
- 环境变量形式的路径；
- 相对路径；
- UNC、verbatim、device namespace 或其他非盘符路径；
- 扩展名不是 ASCII 大小写不敏感的 `.exe`；
- 路径无法无损转换。

解析器不检查目标是否存在，不调用文件元数据 API，也不保留参数、工作目录或图标位置。一个不存在但满足上述词法条件的 `C:\...\app.exe` 仍可作为进程身份候选；后续启动仍使用 `.lnk`。

显示名称固定为 `.lnk` 文件名去掉扩展名后的无损 Unicode 文本，不读取 Shell 显示名称或目标描述。

## 图标

Task 3 不从快捷方式或目标程序提取应用专属图标，所有应用记录固定 `icon = None`。展示层使用仓库内置的通用应用图标。

本任务不需要调用 `SHGetFileInfo`。若未来另行批准由系统生成通用文件类型图标，只能配合 `SHGFI_USEFILEATTRIBUTES` 和常量类型输入，不能传入发现到的快捷方式或目标路径；微软说明该标志使 API 不访问指定文件：[SHGetFileInfo](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shgetfileinfoa)。应用专属图标提取不在 MVP-A 范围内。

## 诊断与错误

扫描成功时只返回应用记录和以下类别的聚合计数：

- 缺失根；
- 跳过的不可访问子项；
- 跳过的 reparse point；
- 跳过的无效 Unicode 路径；
- 跳过的损坏快捷方式；
- 未得到可执行文件映射的有效快捷方式。

扫描失败只返回固定错误类别：Known Folder 查询失败、根不是目录、根不可访问、根为 reparse point、COM 不可用、哈希失败或重复 `appId`。日志、错误和诊断不得包含快捷方式路径、目标路径、应用名称、原始 HRESULT 文本或其他条目标识。内部可用 HRESULT 决定固定分类，但不得直接输出。

## 测试合同

进入实现前，Task 3 计划必须把以下最小测试写成先失败的门禁：

1. 生产根提供者只以 `FOLDERID_Programs` 和 `FOLDERID_CommonPrograms` 调用 `SHGetKnownFolderPath`，固定使用 `KF_FLAG_DONT_VERIFY`，不读取环境变量或生产配置路径；内部扫描函数允许测试注入临时根。
2. 内部扫描函数只接受两个注入根中的普通 `.lnk`，扩展名匹配不区分 ASCII 大小写。
3. 任一 Known Folder 查询失败使扫描失败；查询成功但缺失的根按空结果处理；已存在但不可访问、不是目录或为 reparse point 的根使扫描失败，旧缓存不变。
4. 子目录 junction、symlink 及 reparse 文件均被跳过，根外 sentinel 不进入结果。
5. 不可访问子项与损坏 `.lnk` 被跳过，其他有效入口仍返回。
6. 无法无损转换为 UTF-8 的相对路径被跳过，代码与诊断中均不出现替换字符。
7. `appId` 已知向量固定；重复扫描和仅大小写重命名保持 ID，非大小写重命名、移动目录或切换根产生新 ID。
8. 同名入口保持不同 ID；改变目标、参数或绝对根前缀不改变同一入口的 ID。
9. 重复 `appId` 使整次扫描失败且不替换旧缓存。
10. 快捷方式解析固定请求 `SLGP_RAWPATH`，不调用 `Resolve`。
11. 环境变量、相对路径、UNC、device path、非 `.exe` 和无效 Unicode 目标均得到 `executable = None`。
12. 不存在但词法有效的盘符绝对 `.exe` 得到 `Some(path)`，证明没有目标存在性检查。
13. 每个应用的 `icon` 为 `None`；前端 DTO 不包含 `appId`、快捷方式或可执行文件路径。
14. 诊断和错误只含批准的类别与计数，不含应用名称或任何路径。

Windows 边界脚本必须在进程临时目录内创建允许根、根外 sentinel 和指向根外的 junction，在 `finally` 中验证清理目标仍位于该临时目录后删除。脚本只证明扫描边界，不启动快捷方式或目标程序。

## 非目标

- 不发现没有开始菜单 `.lnk` 的便携应用、AppX 清单或任意文件系统程序。
- 不解析、展开、规范化或验证快捷方式目标。
- 不提取应用专属图标，不增加图标编码、WIC 或缓存依赖。
- 不实现文件搜索、翻译、macOS、第三方插件或通用 Shell/进程能力。
- Foundation Task 3 实施计划复审通过前不进入 TDD。
