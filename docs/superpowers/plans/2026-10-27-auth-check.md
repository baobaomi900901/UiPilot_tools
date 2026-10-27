# 认证HTTPS验收插件实施计划

目标：交付独立auth-check1.0.0，以已验收Host0.14.0完成实际认证HTTPS人工闭环。[设计](../specs/2026-10-27-auth-check-design.md)是固定身份、端点、请求/响应和错误处理合同。复用demo-credentials结构，宿主只读。

现有原生JS/Node测试、TypeScript SDK、CLI，无新增依赖。W为既有local-plugin-foundation工作树，D源码只读；所有实际Host/凭据/网络操作由用户进行。先RED再实现及GREEN，一位写入者，独立Spec/Quality审查；只做下述聚焦验证，禁止重复历史全仓检查。Task1→审查→Task2。

### Task 1: 独立认证测试插件
**Files:** examples/public-plugins/com.uipilot.auth-check/{package/plugin.json,package/dist/runtime.js,tests/runtime.test.js,tests/sdk-contract.ts,README.md}；scripts/verify-local-foundation.test.mjs精确库存及当前版本断言。
**Dependencies:** 设计固定合同、来源及验证章节；先读公开插件教程、UI规范及SDK。
- [ ] 按固定合同实现本地status/显式request、固定凭据引用和静态结果，包内不出现自行构造的Authorization。
- [ ] 离线模拟测试先失败再通过；SDK类型覆盖实际请求字段，拒绝明文getter/凭据对象。
- [ ] README记录公开测试值、外部服务边界、少量人工请求、失败恢复及代理未执行原生请求；发现断言纳入新插件且校正过时版本常量。
**Distinct test coverage:** body/headers getter不得被访问；恶意错误message不进结果；空/status/未知输入不得发请求；每个request最多一次；HTTP401与网络错误区分，其他状态不能成功；真实Host结果不得由mock推断。
**Verify:** node --test examples/public-plugins/com.uipilot.auth-check/tests/runtime.test.js；node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.auth-check/tests/sdk-contract.ts；node --test scripts/verify-local-foundation.test.mjs；现有CLI validate新package。

### Task 2: 打包与人工交付
**Files:** .superpowers/deliveries/auth-check-1.0.0-acceptance（忽略目录）；本计划、总路线图。
**Dependencies:** Task1完整切片审查通过。
- [ ] 现有CLI打包、归档校验，输入前后稳定/包hash/旧Host与旧凭据包未变。
- [ ] 三项短人工用例与交付证据，记录实际HTTPS仍待测。
**Verify:** node packages/plugin-cli/dist/cli.mjs pack及validate（--platform windows --json），git diff --check、包hash与旧验收Hosthash核对。不重建或启动宿主。