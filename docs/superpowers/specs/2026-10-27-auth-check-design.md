# 实际Host认证HTTPS验收设计

## 目标与选择
按用户既有自主开发授权及本次“继续”，补齐实际Host→凭据引用注入→HTTPS认证服务→主界面结果的手工验收；不扩展宿主API，不要求真实账号/密钥。复用已通过的Host0.14.0。使用新独立插件com.uipilot.auth-check 1.0.0，避免改动已验收demo-credentials与用户配置。

方案比较：现有api.example.com仅占位，不能成功验证；普通bearer回显端点可能接受任意token，无法验证错误值；自建公网TLS服务需要额外部署。选择Httpbun公开测试端点，正确/错误固定合成值可形成正负对照。它是外部公共测试服务，不保证可用性，网络故障不能等同产品认证失败。代理只查文档，不执行认证请求；用户明确操作插件时发送。

## 固定合同
- 新目录examples/public-plugins/com.uipilot.auth-check，package只含plugin.json与dist/runtime.js；tests/runtime.test.js、tests/sdk-contract.ts及README.md在包外。参考demo-credentials。
- API/schema1、minimumHostVersion0.4.0、supportedPlatforms仅windows、submit/mainResult、旧单command defaultName=auth-check，inputRequired=false。不增加runtime.resultActions。
- 包名“认证链路验收”；只声明network.https，httpsHosts精确httpbun.com；唯一credential id=acceptance-token、settingKey=test-token、host=httpbun.com、type=bearer；唯一secret字段test-token，label“公开测试凭据（勿填真实密钥）”。
- 固定URL https://httpbun.com/bearer/uipilot-acceptance-token，GET，credential=acceptance-token。Runtime只能提交上述三个请求字段，不自行添加Authorization、正文、可配置URL或凭据读取器。
- 空输入及status只调用isSecretConfigured('test-token')，本地结果“测试凭据已配置”/“测试凭据未配置”。未知输入显示“请输入 status 或 request”，零网络请求。输入trim+toLowerCase，仅request发起一次请求，无重试/后台请求。
- request成功HTTP200显示“认证成功（HTTP 200）”；HTTP401显示“认证被拒绝（HTTP 401）”；其他合法HTTP状态显示固定前缀+数值（不视为认证成功）。不读取、不展示、不存储、不记录response.body/headers或任何服务回显；不提供默认复制动作。
- 网络错误只暴露SDK allowlist的固定name及静态指导，未知错误映射NetworkFailureError，不展示message/stack。本地状态错误也使用固定提示，不泄漏异常正文。无network时静态报“网络能力不可用”。
- 测试公开值uipilot-acceptance-token不是秘密，固定在测试URL中由服务匹配。错误对照值uipilot-wrong-token。用户仅在Host密码框保存这些值；README、描述、手册明确不填真实密钥。
- Host源码/src、src-tauri、packages/plugin-cli、SDK合同不修改，无新依赖/系统证书/本地服务/管理员设置/真实应用执行。系统网络保持连接，开关只指插件网络授权。

## 验证与交付
离线mock测试验证空/status/未知输入零网络、request恰好一个固定请求、200/401/其他HTTP、SDK错误allowlist、未知异常与状态错误不泄漏、响应body/headers完全不访问（可用throwing getter）。manifest绑定和SDK类型一致。新包纳入既有发现脚本，更新精确库存断言；同时校正该测试中已过时0.10.0当前版本断言至实际0.14.0，不变更产品版本。只运行新插件Node测试、SDK类型、发现协调器自身测试与CLI目录/归档校验，不全仓verify/重建Host。

完整插件切片独立Spec/Quality审查后，用现有CLI pack到.superpowers/deliveries/auth-check-1.0.0-acceptance/plugins/com.uipilot.auth-check-1.0.0.uipilot-plugin；记录源码、输入hash、包hash和旧Host未变。全部原生HTTPS行为待用户。
首组三项：安装/确认httpbun.com网络权限，空输入status未配置；保存公开正确值request→200；改成公开错误值request→401，再恢复正确值→200。后续短组：删除凭据导致Host拒绝；撤销/重新授权网络导致拒绝/恢复。最后才收口实际认证HTTPS证据。

## 来源
2026-10-27查阅[Httpbun官方文档](https://httpbun.com/#authentication)：/bearer/{expectedToken}要求Bearer值匹配路径值，缺失或不匹配返回401。[官方项目](https://github.com/sharat87/httpbun)说明这是HTTP客户端测试服务，禁止将公共实例用于CI；本项目自动测试保持完全离线，仅用户少量人工请求。未声称远端服务实际请求已通过。