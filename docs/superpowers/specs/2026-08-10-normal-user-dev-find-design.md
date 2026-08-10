# 普通权限 Dev `/find` 修复设计

## 状态

- 日期：2026-08-10
- 范围：仅解决普通权限 `npm run tauri dev` 下 `/find` 无结果
- 已确认：UiPilot 和 Everything 用户态客户端必须以普通权限运行；允许一次 UAC 安装 Everything Service

## 问题

当前 `scripts/dev-with-everything.ps1` 只按进程名判断 Everything 是否运行，并直接启动便携客户端。普通权限且没有 Everything Service 时，Query2 IPC 可以连接，但 NTFS 索引为空，空查询与 `/find windows` 都返回 `total=0`。

安装 Service 后还存在第二个边界：Service 进程和用户态客户端都叫 `Everything.exe`。脚本可能把 Session 0 的 Service 当成可查询客户端，从而跳过当前交互会话的客户端启动。

## 本轮目标

1. 开发者一次性通过官方安装方式把默认 Everything Service 安装到受保护的 `Program Files` 目录。
2. 此后 `npm run tauri dev` 不请求管理员权限。
3. dev 脚本只把当前 Windows 会话中的 Everything 进程视为用户态客户端。
4. 没有当前会话客户端时，以普通权限启动仓库锁定版本的 `Everything.exe -startup`。
5. `/find str` 通过默认 Query2 实例返回真实文件结果。
6. dev 退出时只关闭脚本自己启动的客户端，并优先优雅退出。

## 非目标

- NSIS 或正式安装包。
- UiPilot 私有 Service、pipe、命名实例和生产 ownership manifest。
- 升级、卸载、多用户授权和 Windows VM 矩阵。
- folder-index fallback。
- 自动提升 UiPilot、Cargo、Vite 或 Everything 用户态客户端。

## 开发运行链路

```text
Everything Service (已由用户一次性安装，LocalSystem)
        ^
        | NTFS 索引
普通权限 Everything 用户态客户端（当前交互会话）
        ^
        | Query2 IPC
普通权限 UiPilot dev
```

`scripts/dev-with-everything.ps1` 在启动 Vite 前执行以下检查：

1. 查询默认 Everything Service；不存在或未运行时立即失败，并打印一次性人工安装说明。
2. 验证 Service binary path 位于受保护的 `Program Files`，拒绝指向仓库、临时目录或其他普通用户可写位置的 Service。
3. 只查找与当前 PowerShell 相同 `SessionId` 的 `Everything.exe` 用户态客户端。
4. 若客户端不存在，普通权限启动锁定的仓库资源客户端并记录精确 PID。
5. 等待客户端存活和 IPC 启动窗口；不按进程名终止任何进程。

Rust `/find` 后端本轮继续连接默认实例。当前用户已有默认 Everything 客户端时，脚本复用它且退出时不停止；脚本自己启动客户端时，退出阶段先请求正常退出，超时后才按精确 PID 终止。

## 失败行为

- Service 缺失或停止：dev 命令失败，不启动 Vite/Tauri 的半可用状态。
- Service 路径不受保护：dev 命令失败，提示重新使用官方安装程序安装。
- 客户端启动失败或提前退出：dev 命令失败并报告稳定错误。
- IPC 成功但空查询总数持续为零：`/find` 显示搜索不可用，不能显示“未找到文件”。
- 任一失败都不得触发 UAC 或启动管理员权限客户端。

## 测试

- PowerShell 边界测试覆盖 Session 0 Service 与当前会话客户端的区分、缺失 Service、非受保护 Service 路径、owned/unowned 退出行为。
- Everything adapter 单元测试覆盖“索引未就绪/持续为空”映射为 unavailable。
- 本机集成验证使用普通权限 token 运行 IPC probe：空查询 `total > 0`，`windows` 或已知文件名返回结果。
- 最终人工验证只要求用户在普通终端运行 `npm run tauri dev` 并输入 `/find windows`；Codex 不操作鼠标。

## 验收

1. 一次性 Service 安装后，日常 dev 启动不出现 UAC。
2. UiPilot 与 Everything 用户态客户端均为 Medium Integrity。
3. Service 为 LocalSystem，且 binary path 不在仓库或用户可写目录。
4. `/find windows` 在索引 ready 后返回至少一个结果。
5. 本轮不包含正式打包与 VM 验收。
