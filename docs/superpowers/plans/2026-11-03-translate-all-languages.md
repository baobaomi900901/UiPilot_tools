# 有道全语言翻译实现计划

目标：将中英 Panel 扩展为官方目录的全部 112 种语言。宿主仍代理有道请求，系统 SAPI 提供按实际声音可用的朗读。合同见 [设计](../specs/2026-11-03-translate-all-languages-design.md) 的语言合同、面板、朗读和验证各节。

技术：Rust/Tauri、原生 JS Panel、TypeScript SDK、Node/Vitest。保留既有用户改动；不改密钥、授权，不自动安装。当前隔离工作树继续使用。

全局：各实现先做专项失败回归再实现、复测；按独立模块提交，不包含既有改动。语言数据先确立，随后 Tasks 1/2/3 可在不相交文件上并行，最后整体验证、审查及发布。复核不把模拟边界当真实供应商/系统验收。

### Task 1: 宿主翻译全语言校验

**Files:** public_plugins/host_services.rs、runtime.rs、activation.rs；共享语言 Rust 模块及 lib.rs 注册；对应 Host/bridge 测试。
**Dependencies:** 设计“语言合同”；官方 JSON 目录。
- [x] 共享规范代码目录与响应归一化，保留未知检测/目标校验。
- [x] Runtime bridge 同步白名单，新增语种要求最低 Host 0.19.0 声明。
**Distinct test coverage:** 多语种请求/返回、规范大小写、非法代码、旧版本声明拒绝新语种；凭据和会话撤销回归。
**Verify:** `npm test -- --run src/plugin-network-bootstrap.test.ts`；`cargo test --manifest-path src-tauri/Cargo.toml public_plugins::host_services::tests --lib`。

### Task 2: 系统朗读语言映射

**Files:** panel_effects.rs、panel_effects/native.rs、Panel bridge 与测试。
**Dependencies:** 设计“朗读”；Task 1 共享语言模块合同（由协调者提供）。
- [x] SAPI locale 转规范语言代码，区分文字变体，不猜测不支持语言。
- [x] 语音 SDK 桥接接受目录代码，仍按真实声音提供可用性。
**Distinct test coverage:** 日韩法、简繁、塞尔维亚、无映射及无声音；已有播放撤销语义。
**Verify:** `npm test -- --run src/plugin-panel-bootstrap.test.ts`；`cargo test --manifest-path src-tauri/Cargo.toml panel_effects --lib`。

### Task 3: Panel、SDK 与发布

**Files:** 插件 languages.js/panel.js/html 与 tests；SDK 类型/文档；版本与交付记录。
**Dependencies:** 设计“面板”“语言合同”；官方 JSON。
- [x] 112 种语言搜索/选择，检测能力提示，RTL 文本与交换。
- [x] SDK 合同与 0.19.0/1.4.0 版本升级，打包唯一新归档。
**Distinct test coverage:** 官方目录集合一致、搜索/记忆、日/韩/阿语识别与交换、未知代码拒绝、长列表键盘导航、旧请求隔离。
**Verify:** `node --test examples/public-plugins/com.uipilot.translate/tests/*.test.js`；SDK tsc、npm build、CLI validate 与 pack。


## 交付与验证记录（2026-11-03）

- 官方目录去重112语言、83可自动检测；插件与桥接/共享Rust目录一致性检查通过。
- 插件全量107/107，最终文案修正后Panel15/15复测；Runtime/Panel桥接52/52，CLI186/186，SDK严格类型检查通过。
- 前端生产构建通过，保留既有大chunk提示。完整Rust lib-test编译通过；宿主服务/共享目录/Runtime/兼容专项35项、语音11项通过；仅既有截图模块dead-code提示。
- Rust验证使用进程级 `TAURI_CONFIG={"bundle":{"resources":[]}}` 与 `--no-default-features`，用于排除开发工作树缺少打包资源，不等于Windows安装程序构建验收。
- 实际离线浏览器检查深浅主题、350/380宽布局、RTL计算方向、English名称搜索、粤语手选提示、长列表末项滚动和Escape焦点恢复；使用假翻译，不访问真实供应商或音频。
- 独立代码审查无阻塞项；SDK旧中英文注释、Panel旧版本提示两项轻微问题已修正。
- 安装包：`examples/public-plugins/com.uipilot.translate/artifacts/com.uipilot.translate-1.4.0-windows-multilang.uipilot-plugin`，由新CLI打包并重新validate通过。SHA256：`e9797ce2d4ebfabbbccf5869866ae9e208be048cf7c16b214b1209ac78f59148`。
- Host0.19.0 / 插件1.4.0。没有安装插件、修改授权、使用有道密钥、播放声音或创建系统声音。真实语言方向与声音安装情况仍待用户原生验收。
- 预览截图：`C:/Users/moby/.codex/visualizations/2026/11/03/01a19f3f-3859-72a3-b423-58712fec6158/translate-multilang-dark.png` 和同目录 `translate-multilang-light.png`。临时页面与预览服务已关闭。
