# 本地插件确定性打包设计

日期：2026-10-23。所属阶段：本地插件基座 M1。基于 [总设计](2026-10-23-local-plugin-foundation-design.md)。本功能属于宿主开发工具，不涉及远程发布。

## 目标与接口

提供 `uipilot-plugin pack <source-directory> --output <file.uipilot-plugin> [--platform windows|macos] [--json]`。只接收一个本地目录和一个显式输出路径；输出父目录必须已存在。没有 --force，不覆盖既有路径，不自动安装、不执行插件、不下载依赖。validate 与其输出/退出码保持兼容。顶级 --help 同时列出 validate/pack。

pack 成功退出 0，非法包或输出操作的预期失败退出 1，参数错误或未知内部异常退出 2。普通输出简洁显示成功文件和 SHA-256；失败不得显示堆栈或源码/凭据。JSON 成功为 `{schemaVersion:1,created:true,output:{path,sha256},validation,warnings?}`；warnings 非空时为 `[{code:"TEMP_CLEANUP_FAILED",message:string}]`，只用于已发布产物的临时文件清理失败；JSON 预期失败为 `{schemaVersion:1,created:false,output:{path},validation?,error:{code,message}}`。code 仅 SOURCE_INVALID、PACKAGE_INVALID、OUTPUT_INVALID、OUTPUT_EXISTS、OUTPUT_WRITE_FAILED；validation 存在时使用既有 PluginValidationReportV1。未知异常沿既有 CLI_INTERNAL 错误格式，参数错误沿 CLI_USAGE。

## 数据流

1. 解析参数，platform 规则与 validate 一致。source/output 按 cwd resolve，输出后缀必须严格 `.uipilot-plugin`。
2. 用现有 `readDirectorySnapshot` 读取一次不可变包快照，保留 Windows reparse、文件身份、路径与大小边界。禁止先验证目录再另读一遍打包。
3. 用 `validateSnapshot` 校验该同一快照。失败返回 PACKAGE_INVALID 和报告，不创建产物。
4. 按 UTF-8 字节序排列归档条目，用同一快照构造 ZIP。再用 `readArchiveSnapshot` 和 `validateSnapshot` 校验生成字节，确保目录/归档两种载体遵守同一安装策略。
5. 输出路径必须在源目录之外。检查输出父目录及已有祖先路径安全（普通目录、无符号链接或 Windows reparse），使用规范路径验证包含关系，拒绝静态祖先别名；不承诺抵御同账户恶意并发替换父目录；存在任何输出对象均返回 OUTPUT_EXISTS，不能跟随链接。父目录缺失不创建。
6. 在输出父目录以独占方式创建随机临时文件，写完并关闭；以无覆盖的原子发布方式创建最终输出（同目录 hard link 可用），最后清理本次临时文件。输出被其他进程抢先创建时失败，保留对方文件。失败只清理本次拥有的临时文件，不能删除其他文件或已有产物。
7. SHA-256 对最终归档字节计算，返回绝对输出路径。最终文件已发布但临时文件清理失败时，返回成功退出 0、created:true 和 TEMP_CLEANUP_FAILED warning，普通输出也显示警告；不能返回 created:false，不能删除有效结果冒充回滚。

## ZIP 格式

复用既有 crc32 实现；不新增压缩库。ZIP 使用 stored entries、UTF-8 flag、固定 DOS 时间 1980-01-01 00:00:00、无评论/extra/data descriptor/Zip64/平台私有元数据。central directory 的 version made by 固定为 DOS creator（高字节 0），目录 external attributes 固定为 0x10，普通文件为 0；目录名以 / 结尾。按快照保留目录信息，条目顺序固定；包最大规模沿 PACKAGE_LIMITS，生成归档含头部超过 maxArchiveBytes 也失败。同一快照在不同源目录/mtime 下产生完全相同字节。不把磁盘文件时间、绝对源路径或随机数据写入 ZIP。

## 模块与兼容

新 `archive-writer.ts` 只负责受限快照到 ZIP 字节；`pack.ts` 负责校验和输出发布。`cli.ts` 只负责参数/显示，`bin.ts` 注入真实 pack 实现。保留现有 runCli(validate) 测试调用兼容，新增可选或显式 pack dependency。生成的 CLI bundle 无外部运行依赖；不改变 host/apiVersion/manifest schema 或插件格式。

## 验收

- 有效 demo-return、demo-panel、pomodoro 可 pack 并由现有 CLI 验证归档；pack smoke 使用临时副本，不安装插件。
- 新增小型确定性 ZIP fixture：TypeScript 测试断言 writer 生成字节与 fixture 完全一致，Rust 测试通过现有 package::stage 解包、验证内容，覆盖空目录和嵌套目录；不调用真实安装。fixture 由测试内同一固定 snapshot 生成，记录生成方法。`tests/fixtures/public-plugin-pack/roundtrip.uipilot-plugin` 不含真实数据。
- 两个相同包在不同目录、不同 mtime 下字节与哈希一致；改变内容后哈希变化。
- 错误 manifest、额外资源、链接、路径冲突、超限、源变化沿既有拒绝策略；无失败产物。
- 输出在源内部、后缀错误、父目录不存在/链接、已有文件或目录均失败且原内容保持不变。
- 发布竞态中对方占据目的路径后不覆盖；写失败清理自身临时文件；测试带空格路径。
- hard link 发布成功后 unlink 临时文件失败：最终文件保持正确，created:true、退出 0、带清理 warning；通过故障注入验证。
- CLI 缺少/重复参数、未知 flag、--json 与退出码符合合同；既有 validate 输出不变。
- 包装后的 npm tarball 在无网络 consumer 中实际执行 pack/validate smoke 成功，输出文件名不靠 shell 拼接。

## 分发 smoke 的权限边界

既有 validate smoke 保持仅临时目录（Windows 另含系统目录）的读取权限。pack 的真实祖先 lstat/realpath 检查会走到磁盘根；Node permission model 不能仅授权目录元数据读取。因此 pack smoke 可使用更广的读取权限，但写权限只授予本次临时输出目录，并保留离线 npm consumer、本地 tarball 安装和 network trap。不得宣称 pack smoke 阻止了仓库读取；无需仓库依赖由独立安装产物运行和既有 bundle 审计共同验证。不得放宽仓库写权限，不为测试创建系统卷映射或引入特权 helper。