# 翻译插件宿主密钥接入计划

目标：将 `/translate` 改名“翻译”并迁移有道调用，插件详情与权限页提供三种独立密钥服务授权。架构与边界依据 [设计](../specs/2026-10-31-translate-host-credentials-design.md) 的“权限及页面”“宿主边界”“错误与验收”。

技术沿用 React/Ant Design、Tauri/Rust、既有插件 Runtime 和 HTTPS 请求上下文。留在已授权的当前功能分支，保留所有既有改动，不混合提交；不修改 rust_uia，不读取用户真实密钥、不调用真实供应商、不自动开启授权。

## 共用合同与顺序

三个服务权限为 `credentials.youdaoTranslation`、`credentials.baiduOcr`、`credentials.deepseek`。Windows Host 最低版本提升到 `0.15.0`；只增加有道业务 API：`api.services.youdao.translate({text, from, to})`，语言仅 `en`/`zh-CHS`，响应 `{translations: string[]}`。业务 API 首版仅支持旧式 `command + mainResult` 且未开启 `runtime.resultActions`，与当前翻译插件一致；其他形态可管理授权但不能调用，后续扩展需补齐各自发布边界。插件 Runtime 不接受或返回密钥、URL 或认证参数。调用通过内部 `plugin_youdao_translate` 命令，封装现有 request context，错误固定映射。

Task 1 与 Task 2 按已确定合同独立开发；Task 3 接入 Task 1 后，Task 4 统一验证。先添加并观察专项测试的预期失败，再实现并复测；独立评审本轮代码和跨模块授权边界。无需中途重复请求批准。

### Task 1: 宿主服务与权限边界

**Files:** `src-tauri/src/credentials/`、`src-tauri/src/public_plugins/manifest.rs`、`activation.rs`、`state.rs`、新宿主服务模块、`commands.rs`、`lib.rs`、`build.rs`、Runtime capability、生成 Schema。

- [x] 支持三个权限且默认不授予；旧版记录及更新不扩大授权。
- [x] 由宿主安全读取当前启用配置、签名和代调用有道；共享网络和请求上下文限制。
- [x] 服务撤权、网络撤权、插件失效、配置变更均阻止旧调用及晚到结果；真实凭据不出 DTO/日志。

**Distinct test coverage:** 调用者与声明校验、三种授权互不替代、配置状态错误、验证/请求中撤权、删除与重新添加配置不恢复旧上下文、网络超时/响应限制/无重定向。

**Verify:** 针对 `host_services`/`credentials`/权限管理 Rust 模块运行测试，`cargo check --lib --release`。测试允许使用进程内空 bundle resources 覆盖以避免复制被用户 dev 占用的资源。

### Task 2: 授权页面

**Files:** `src/protocol.ts`、`src/public-plugin-panel.tsx`、`src/plugin-permissions-panel.tsx`、相关测试与 CSS、安装/更新同意的前端流程。

- [x] 三个独立中英标题分组与插件详情“主程序密钥”区域；复用权限存储和变更事件。
- [x] 区分服务未授权、配置缺失/关闭/就绪；开启时展示用途与额度说明；不回填密钥。
- [x] 默认未授权，取消不变更；陈旧清单刷新后重试，不自动重放授权。

**Distinct test coverage:** 三项独立开关、详情与权限页一致、配置关闭但授权仍开、安装/刷新不自动新增授权、对话框取消焦点。

**Verify:** `npm test -- src/plugin-permissions-panel.test.tsx src/public-plugin-settings.test.tsx src/protocol.test.ts`，按需要覆盖 launcher 授权流程。

### Task 3: Runtime、SDK 与翻译插件迁移

**Files:** Runtime bootstrap、SDK 类型/合同/示例文档、`examples/public-plugins/com.uipilot.translate/`、版本文件。

**Dependencies:** Task 1 的服务接口与三项权限。

- [x] 冻结且绑定请求的有道业务 facade，固定错误映射；过期 API 不可借用新调用。
- [x] 插件改名并清除旧测试凭据/本地签名；申报三项权限但只调用有道。
- [x] SDK/Schema/CLI 和版本一致，不开放 OCR/DeepSeek 业务方法。

**Distinct test coverage:** 正常中英互译、无输入、未授权/配置关闭提示、保留复制动作、非法 facade 参数/过期上下文、包内无旧密钥。

**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/runtime.test.js`、SDK typecheck、最新 CLI 校验/打包、Runtime bootstrap 离线测试。

### Task 4: 集成验收

**Dependencies:** Task 1–3。

- [x] 独立评审本轮所有变化，修复实质问题。
- [x] 前端生产构建、Rust 检查、专项回归与深浅主题浏览器验收。
- [x] 打包翻译插件提供可安装产物，记录真实有道 API 调用仍需用户手动验收。

**Verify:** `npm run build`；最新 CLI validate/package；手动流程为升级插件→授予有道与网络→执行 `/translate hello`→撤销有道授权后验证正确提示。
## 验证记录（2026-10-31）

- 前端生产构建通过；权限、插件详情、协议和启动器 4 个测试文件共 418 项通过、18 项跳过。插件离线测试 9/9、Runtime bootstrap 26/26、CLI 87/87；SDK 类型检查和 CLI 检查通过。
- Rust 发布模式检查通过。定向测试：host_service 10、credentials 21、permissions 40、broker 19、manifest 14、异步接收器授权元数据回归 1，以及生产命令和 Runtime capability 两项 source guard 均通过。父代理复跑 host_services 过滤 5 项与接收器回归 1 项通过。
- Rust 检查仅在测试进程覆盖空 bundle resources，避免复制正在使用的开发资源；未改生产打包配置。本轮未执行完整 setup 打包。
- 深浅主题和窄窗口浏览器检查通过，确认授权取消、焦点恢复、独立开关及详情状态同步。使用模拟配置，未读取真实凭据或发送真实供应商请求。
- 翻译插件 1.1.0 已由最新 CLI 校验并打包：`artifacts/plugins/com.uipilot.translate-1.1.0.uipilot-plugin`。SHA-256：`68d495d4c44a721417104e02499d84ecefe0911e1de2c831b9f5914f2ada698e`。
- 扩大网络回归 48 项中 42 项通过、6 项本地 TLS 测试返回 `Err(Tls)`。隔离执行仍失败；测试使用临时自签名证书，证书默认有效期覆盖当前日期，生产 transport 无改动。尚未定位 Windows TLS 原因，也未证明是基线问题；不宣称全套测试通过。
- 独立评审无剩余发现。真实有道验收待用户重启新版主程序、升级插件并明确授权后执行；OCR/DeepSeek 本轮仅提供授权管理。
