# Panel 统一底栏实施计划

遵循 [设计](../specs/2026-11-01-panel-status-bar-design.md)，在现有分支实现用户授权的宿主和 Notes 联动。Rust/Tauri 负责会话鉴权与事件，TypeScript 负责显示，Notes 保持插件内业务。定向回归测试先失败后修复，不访问用户笔记内容，不自动提交。

### Task 1: 会话状态接口与宿主底栏

**Files:** `src-tauri/src/plugin_panel.rs`, `commands.rs`, `panel_status.rs`, `lib.rs`, `build.rs`, capabilities；`src/protocol.ts`, `main.ts`, `launcher-core.ts`, `launcher-view.tsx`, `styles.css`。

- [x] 新增受限会话接口、权限注册、SDK 合同与测试。
- [x] 始终保留底栏，当前面板状态按序显示，退出即清理。

**Verify:** 面板 bootstrap/launcher 定向 Vitest、Rust panel_status 及相关 panel 测试、前端构建。

### Task 2: Notes 接入

**Dependencies:** Task 1。

**Files:** Notes 包、测试、预览、SDK 文档。

- [x] 将编辑区操作提示接到底栏，清理旧气泡及调整预览和测试。
- [x] 升级插件元数据，说明开发目录重新载入。

**Verify:** Notes runtime tests、SDK 类型检查、插件验证；保存成功/失败与连续状态测试。

### 验证记录（2026-11-01）

- Panel bootstrap / launcher 定向测试 39 通过；Rust `cargo test --lib panel` 117 通过。
- Notes runtime 27 通过（含保存成功、失败、旧定时器不清除新提示）；SDK 类型检查、插件 CLI 校验、`npm run build` 通过。
- 深色及浅色浏览器预览确认底栏保留，Ctrl+S 后底栏显示绿色“已保存”；测试只使用预览数据。原生已安装 Notes 尚需更新至 1.2.6 后人工验收，未修改用户笔记。
- 扩大 launcher 回归时发现 `opens the catalog find row directly with an empty query` 失败（预期 1 个结果实际 0 个）；本次没有修改该功能逻辑，单独保留待排查，不宣称全套测试通过。
- 构建仍有包体积提示及截图模块既有 dead-code 警告。工作区已有其他改动，未提交。

### 后续左右分区调整

用户追加要求：状态左对齐、操作提示右对齐且互不覆盖，移除 Notes 内容区保存标记。已完成，Notes 版本为 1.2.7。前端定向 85 项、Notes 27 项、生产构建与插件校验通过；浏览器预览确认左侧保存提示和右侧操作提示同时显示。内部未保存检测和离开确认保持有效。

### 主结果插件统一（翻译 1.2.1）

默认动作提示由宿主生成，结果副标题只保留描述。覆盖旧版复制、无动作、动作菜单、默认动作切换、执行中及失败/消费后的提示。完整 launcher 测试 373 通过、18 跳过，仅此前记录的 `/find` 空查询测试失败；翻译离线测试 9 通过，插件校验通过。浏览器合成数据预览确认左右分区；没有调用真实翻译服务或修改用户已安装插件库。
