# 密钥配置管理实现计划

目标：完成三类、四种服务的紧凑管理列表、独立详情、加密持久化和验证后保存。依据 [设计](../specs/2026-10-31-credential-manager-design.md)。

技术：现有 React/Ant Design、Tauri/Rust、reqwest、Windows DPAPI。保留当前功能分支与所有既有改动，不提交混杂变更；不修改 rust_uia，不自动修改真实用户配置或使用真实密钥测试。

## 共用合同

服务标识：youdaoTranslation、baiduTranslation、baiduOcr、deepseek。每种服务最多一项。列表返回 revision 十进制字符串与 items；条目包含 service、name、enabled、configured，不返回凭据。

管理操作均校验 expectedRevision。新增条目未配置且关闭；保存必须验证成功才能原子持久化，保留原有 enabled。未配置条目不能启用。删除、启停和新增均使 revision 增长；验证前快照和验证后提交之间再检查 revision，防止旧请求覆盖删除、启停或更新。

保存输入 values 仅接受对应服务的字段：有道 appId/appKey，百度翻译 appId/appKey，百度 OCR appId/apiKey/secretKey，DeepSeek apiKey；空值保留已有字段。密钥字段在列表、错误和日志中不可见。错误使用静态 code/message。

## 全局验证

行为变化使用真实调用接缝的离线测试；UI 样式使用双主题预览。不访问真实服务，不替用户启用权限。针对变化运行前端构建、必要 Rust 检查与测试。任务完成前独立审查本轮涉及文件；检查并发、存储和调用者权限。

### Task 1: 宿主管理与验证

**Files:** `src-tauri/src/credentials/`、`src-tauri/src/lib.rs`、`src-tauri/build.rs`、`src-tauri/capabilities/main.json`。

**Dependencies:** 设计的“存储与验证边界”“实施范围确认”“验证接口依据”。

- [x] 新增 src-tauri/src/credentials/，实现 DPAPI 存储、版本控制、固定供应商验证、main-only IPC，并注册 lib.rs。
- [x] 离线验证成功/失败、持久化重载、无明文输出、并发修改拒绝、删除不复活及未配置不能启用。

**Verify:** `cargo test --lib credentials::`（测试进程使用空 bundle resources 的临时配置覆盖，避免复制正在运行的资源）。

### Task 2: 界面与前端桥接

**Files:** `src/credential-*`、`src/protocol.ts`、`src/main.ts`、`src/launcher-view.tsx`、`src/launcher-browser-preview.tsx`。

**Dependencies:** Task 1 的 IPC 合同；设计的“列表”“详情”。

- [x] 新增 src/credential-protocol.ts 合同及 IPC 桥接，替换临时草稿页面。
- [x] 分组列表、删除确认、名称进入详情、编辑与验证状态、离开提示、底栏反馈及双主题。
- [x] 合成预览和必要组件测试；不提供静默成功的后备实现。

**Verify:** `npm test -- src/credential-client.test.ts src/credential-settings.test.tsx`；`npm test -- src/launcher.test.tsx`。

### Task 3: 集成验收

**Dependencies:** Task 1、Task 2；设计的“验收”。

- [x] 运行构建和针对性测试，双主题窄窗口检查，独立代码审查并处理问题。
- [x] 更新实际验证记录，说明真实供应商凭据仍需用户手动验收。

**Verify:** `npm run build`；`cargo check --lib --release`；520px 浅色与 720px 深色预览，检查详情表单、离开确认及取消后的焦点。

## 实际验证记录

- 前端：IPC 客户端 7 项、密钥组件 12 项通过；启动器 370 项通过、18 项跳过；最终 `npm run build` 通过，仅现有大体积 chunk 提示。
- 后端：18 项凭据测试、4 项现有注册/生命周期/模块边界检查通过；Release library check 与新增 Rust 文件格式检查通过。
- Cargo 使用进程内 `TAURI_CONFIG={"bundle":{"resources":[]}}` 跳过资源复制，避免干扰正在运行的应用；未改打包配置。未执行完整安装包验证。既有 dead-code 警告仍存在，没有新增 credentials 警告。
- 浏览器：720px 深色、520px 浅色的列表及横向详情已检查；合成 DeepSeek 凭据保存反馈通过。实际复现并修复取消离开确认后因 tab 焦点恢复而重复弹窗的问题，最终点击取消后留在详情、保留草稿且恢复到返回按钮。
- 独立源码审查及最终增量复查完成，无待修复问题。未读取真实密钥、未调用真实供应商验证接口；真实认证、保存后重启读取的实机体验仍需用户验收。DPAPI 重载已由合成配置的自动测试覆盖。
- 预览截图：`.superpowers/credentials-list-final.png`、`.superpowers/credentials-detail-final.png`。截图内条目为合成预览数据。
