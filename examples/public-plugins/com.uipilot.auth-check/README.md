# 认证链路验收

独立插件 `com.uipilot.auth-check` 1.0.0，Windows Host 0.4.0+、API/schema 1、`submit + mainResult`。本次人工验收复用已验收的 Host 0.14.0。实际安装目录是 `package/`，包内仅 manifest 和 Runtime，无依赖、面板、窗口或复制动作。

**勿填真实密钥。** 仅在 Host 密码框保存公开正确值 `uipilot-acceptance-token`，或公开错误对照值 `uipilot-wrong-token`。这两个值不是秘密；正确值也位于固定测试 URL 中。

空输入或 `status` 只本地调用 `isSecretConfigured('test-token')`，显示“测试凭据已配置”或“测试凭据未配置”；配置状态不代表格式有效或网络已授权。未知输入只显示用法。输入忽略首尾空白和大小写；只有显式 `request` 触发一次 Host 托管的 `GET https://httpbun.com/bearer/uipilot-acceptance-token`，只提交 `credential: 'acceptance-token'` 引用，没有重试或后台请求。

HTTP 200 显示“认证成功（HTTP 200）”，401 显示“认证被拒绝（HTTP 401）”，其他状态显示“收到其他 HTTP 状态（HTTP 数值）”，不能算成功。Runtime 完全不读取、展示、存储或记录服务响应正文/头，异常只显示 SDK 固定错误名和静态指导。网络失败与 HTTP 401 分开。

## 服务边界与恢复

[Httpbun 官方认证文档](https://httpbun.com/#authentication)说明 `/bearer/{expectedToken}` 要求匹配的 bearer 值，否则返回 401。[官方项目](https://github.com/sharat87/httpbun)将公共实例用于 HTTP 客户端测试，禁止用于 CI。它是外部公共测试服务，不保证可用；服务会收到 Host 注入的公开测试值，也可能回显。每次只做少量人工请求，不自动循环；离线测试从不访问它。服务不可用、DNS/TLS/超时等网络错误不能直接认定为产品认证失败。

`CredentialUnavailableError`：检查 Host 密码框是否保存正确公开值，不带空格或 `Bearer ` 前缀。`PermissionDeniedError`：重新打开插件详情并确认当前 `httpbun.com`、`acceptance-token / test-token / bearer` 网络授权。其他固定网络错误：检查连接和服务可用性，稍后手动重新运行一次。保存/清除失败应刷新状态并检查实际配置；失败不保证回滚，已过期命令不会恢复。可靠撤销需要成功清除或关闭插件网络授权。系统网络保持连接，这里的开关只指插件网络权限。

## 人工验收（由用户执行，实际 HTTPS 尚待验证）

1. 安装 `package/` 或交付归档，确认唯一域名 `httpbun.com` 及上述绑定后授权。运行 `/auth-check` 或 `/auth-check status`，预期“测试凭据未配置”。
2. 在 Host 密码框保存公开正确值 `uipilot-acceptance-token`；运行 `/auth-check request`，预期 HTTP 200。
3. 改为公开错误值 `uipilot-wrong-token`；重新运行 request，预期 HTTP 401；恢复正确值后再运行，预期 HTTP 200。

后续可各做一次：明确删除凭据后 request 应由 Host 返回 `CredentialUnavailableError`；撤销插件网络后应返回 `PermissionDeniedError`，重新确认授权并保存正确值后应恢复 200。最终结果在真实 Host 中记录；键盘和 Light/Dark 交互沿用 Host 主界面和设置页。

代理仅完成离线 Runtime、SDK 和 CLI 静态验证，未安装/启动 Host、读写系统凭据或执行认证网络请求。Mock 只能证明插件请求字段和结果处理，不能证明 Host 凭据注入、DPAPI、授权、TLS 或远端实际返回。

## 离线验证

从仓库根运行，使用已有依赖和已构建 CLI：

```powershell
node --test examples/public-plugins/com.uipilot.auth-check/tests/runtime.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.auth-check/tests/sdk-contract.ts
node --test scripts/verify-local-foundation.test.mjs
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.auth-check/package --platform windows --json
```

参考结构为既有 `com.uipilot.demo-credentials`；SDK 合同及 Host/CLI 源码保持只读。打包及真实 Host 人工结果由交付记录单独跟踪。