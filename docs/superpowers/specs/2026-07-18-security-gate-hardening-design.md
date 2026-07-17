# Foundation 安全门禁加固设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 影响范围：Foundation Task 1 安全配置检查与测试探针

## 问题

当前门禁按已知反例逐项检查，不能证明实际配置对象没有新增权限入口。`withGlobalTauri: true`、窗口远程 URL 和 capability 的 `remote` 字段可以绕过现有检查。探针脚本又只验证被测进程退出码为 0，因此任意正常退出的可执行文件也会被误判为安全探针成功。

## 决策一：完整配置对象合同

`scripts/check-security-config.ps1` 使用一个共享的精确属性检查函数，验证以下对象的属性集合与固定值：

- `tauri.conf.json` 根对象、`build`、`app`、唯一窗口、`security`、`bundle` 和 `bundle.android`。
- `capabilities/**/*.{json,json5,toml}` 的文件集合必须只包含根级 `main.json`。
- `main.json` 根对象必须只包含 `$schema`、`identifier`、`description`、`windows` 和 `permissions`，并保持现有固定值和精确权限集合。
- `tauri.security-probe.conf.json` 根对象只能包含 `$schema` 与 `build`；`build` 只能包含固定的 `beforeBuildCommand`。

任何未知属性、缺失属性、额外 capability 文件、内联 capability、远程来源、宽权限或固定值变化都使检查失败。该规则不尝试理解未来配置字段；新增字段必须先显式修改并评审白名单。

## 决策二：可认证探针结果

`scripts/test-security-probe.ps1` 不再以进程退出码作为成功证据：

1. 每次运行生成一个 128-bit 随机 nonce，以及系统临时目录下唯一的结果文件路径。
2. 使用 `System.Diagnostics.ProcessStartInfo.EnvironmentVariables` 仅向被测子进程传递 nonce 和结果路径，不修改父进程环境。
3. `security-probe.ts` 只把精确错误 `Command load_settings not allowed by ACL` 认定为目标 ACL 拒绝；其他拒绝、命令成功和超时都不是成功。
4. 仅在 `test-instrumentation` feature 中，Rust 验证 nonce 格式和结果路径位于约定的临时目录，然后写入结果。
5. 结果 JSON 固定为 `{ "protocolVersion": 1, "nonce": "...", "assertion": "load_settings_denied_by_acl" }`。
6. PowerShell 要求进程退出码为 0、结果文件存在且三个字段精确匹配本次挑战；任一条件不满足都失败。
7. 结果目录在 `finally` 中验证绝对路径前缀后删除。

只有精确的目标 ACL 拒绝可以写入上述成功结果并以 0 退出。命令成功、其他错误和超时均以非零退出，且不能写入成功 assertion。

该协议防止普通的任意可执行文件仅凭退出码 0 通过。它不尝试抵抗能够读取当前进程环境并主动仿造协议的恶意程序；门禁的目标是认证由本仓库探针协议产生的结果，不是建立代码签名信任链。

## 回归测试

配置回归必须证明以下变更均被拒绝：

- `withGlobalTauri` 改为 `true`。
- 主窗口增加远程 `url`。
- `main.json` 增加 `remote`。
- 增加嵌套 JSON、JSON5 或 TOML capability。
- 基础配置或探针 override 增加 capability。

探针协议回归必须证明：

- Windows `hostname.exe` 正常退出仍不能通过探针测试。
- 当前 feature-only `uipilot.exe` 产生匹配 nonce 的 ACL 拒绝结果并通过。
- nonce、协议版本、assertion、结果路径或 ACL 错误任一不匹配时失败。

以上回归加入 Task 1 与最终完整门禁。生产构建继续排除探针页面和探针 Rust 代码。

## 非目标

- 不增加生产 capability、网络、Shell、通用文件或进程权限。
- 不引入代码签名、证书或加密密钥；这些属于后续签名安装包交付。
- 不进入 Task 2，不实现文件搜索。
