# demo HTTPS

`com.uipilot.demo-http` 是插件开发 skill 的宿主 HTTPS 参考，同时用于网络边界验收，不是业务插件。安装目录为 `package/`，默认命令 `/demo-http`，Windows Host 0.3.2+，权限 `network.https`，唯一声明域名为 `httpbin.org`。

| 输入 | 演示内容 |
| --- | --- |
| `get` / `get TEXT` | GET 与查询参数 |
| `post TEXT` / `text TEXT` / `form TEXT` | JSON、纯文本、表单请求体 |
| `status 503` | HTTP 非成功状态仍是正常响应 |
| `timeout` | 宿主请求超时 |
| `denied` | 未声明域名拒绝 |
| `protected` | 受保护的 Host 请求头拒绝 |

业务插件应替换为自己的精确域名与响应解析，按需求保留错误分支。这个示例不处理真实凭据；需要 Bearer 引用时读相邻 `demo-credentials`。

离线检查从仓库根目录执行：

```powershell
node --test examples/public-plugins/com.uipilot.demo-http/tests/runtime.test.js
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-http/package --platform windows
```

测试使用假网络 API，不请求公共服务。安装后手动运行命令才发起真实请求，公共测试服务的可用性不作为离线测试通过条件。安装与打包按[开发教程](../../../docs/plugin-sdk/public-plugin-developer-guide.md)执行；不要把 mock 通过称为真实 Host 网络验收通过。
