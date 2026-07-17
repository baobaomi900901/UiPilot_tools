# Foundation 安全门禁加固设计

## 状态

- 日期：2026-07-18
- 状态：已批准，待实施
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

## 决策二：固定退出码探针

探针使用单一状态和固定专属退出码，不引入测试结果文件协议：

1. `security-probe.ts` 仅在收到精确错误 `Command load_settings not allowed by ACL` 时设置 `acl-denied` 状态。
2. 命令成功、其他错误和超时均不设置 `acl-denied`。
3. 仅在 `test-instrumentation` feature 中，Rust 将 `acl-denied` 映射为固定退出码 `73`；所有其他路径不得退出 `73`。
4. `scripts/test-security-probe.ps1` 只接受退出码 `73`。退出码 `0`、命令成功、其他错误、其他非零退出码和超时全部失败。

固定退出码足以防止普通的任意可执行文件仅凭正常退出通过门禁。代码签名信任链属于后续签名安装包交付，不在本探针合同内。

## 回归测试

配置回归必须证明以下变更均被拒绝：

- `withGlobalTauri` 改为 `true`。
- 主窗口增加远程 `url`。
- `main.json` 增加 `remote`。
- 增加嵌套 JSON、JSON5 或 TOML capability。
- 基础配置或探针 override 增加 capability。

探针回归必须证明：

- Windows `hostname.exe` 正常退出仍不能通过探针测试。
- 当前 feature-only `uipilot.exe` 仅在精确 ACL 拒绝时以 `73` 退出并通过。
- 退出码 `0`、命令成功、其他错误、其他非零退出码和超时均失败。

以上回归加入 Task 1 与最终完整门禁。生产构建继续排除探针页面和探针 Rust 代码。

## 非目标

- 不增加生产 capability、网络、Shell、通用文件或进程权限。
- 不引入代码签名、证书或加密密钥；这些属于后续签名安装包交付。
- 不进入 Task 2，不实现文件搜索。
