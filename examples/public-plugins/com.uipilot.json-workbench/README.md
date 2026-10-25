# JSON 工作台 1.0.0

Windows UiPilot **0.9.0+** 的离线单例子窗口插件，默认命令 `/json-workbench`。只申请 `ui.window`，不访问网络、用户文件、剪贴板或通知。安装目录是本目录下的 `package/`。

## 使用

1. 输入 `/json-workbench` 并按 Enter，读取已保存草稿；首次打开为空。
2. 在“JSON 输入”中输入内容，点击“校验并格式化”。结果显示在右侧，不替换输入，也不保存。
3. 点击“保存草稿”显式保存输入原文。空白、换行、空字符串和语法未完成的草稿均保留。
4. “重新读取”读取持久化内容；有未保存编辑或保存结果不确定时，先在窗口内确认“放弃草稿并重新读取”，或选择“保留草稿”。读取失败不会清空当前输入。

输入最多 **8192 UTF-8 字节**，格式化结果最多 **32768 字节**；最多 16 层嵌套（根值深度 0）和 2048 个值（含根值、容器）。语法错误、非有限数字（例如 `1e400`）和超限均明确报告，不静默改成 `null`。格式化使用 JavaScript JSON 数字语义，超出精确整数范围的数字可能失去精度。

草稿不会自动保存。关闭、隐藏、重新提交命令或退出进程后，未保存内容不保证保留；重开以持久化内容为准。保存处理中可以继续编辑，已确认保存和后续未保存编辑分别显示。

## 数据保护与边界

Runtime 只使用私有 key `workbench.draft`，完整记录为 `{schemaVersion:1,revision,text}`。缺失记录为版本 0；保存以当前版本比较后，一次写入版本 +1 的完整记录。冲突不会覆盖；损坏、未知字段、未知 schema 或非法版本会禁止保存，不重置数据。Runtime 物理调用由 Host 串行，不支持其他写入入口。

取消、超时、异常或写入失败可能发生在实际写入之后。插件保留输入、禁止再次保存，提示用户显式重新读取核对；不自动重试。重新读取期间产生的新编辑不会被晚到结果覆盖。此插件不提供导入/导出、复制按钮、历史版本、备份、迁移或恢复默认值。

窗口内容只捕获本次 `onUpdate` 提供的会话绑定 `request`，回调同步返回，不在 ACK 期间请求 Runtime；不直接调用内容端 storage。所有 load/format/save 业务与持久化均由 Runtime 完成。

## 本地验证

在仓库根目录运行（使用已有依赖，不安装新依赖）：

```powershell
node --test examples/public-plugins/com.uipilot.json-workbench/tests/runtime.test.js examples/public-plugins/com.uipilot.json-workbench/tests/window.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.json-workbench/tests/sdk-contract.ts
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.json-workbench/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.json-workbench/package --output <新文件.uipilot-plugin> --platform windows
node packages/plugin-cli/dist/cli.mjs validate <新文件.uipilot-plugin> --platform windows
```

Runtime 测试使用独立内存假存储，窗口测试只检查六个会话/编辑所有权与恢复场景；均不替代原生 Host 人工验收。

## 人工验收（待用户执行）

- 在隔离验收版 Host 安装本地包，打开命令；校验 `{"name":"甲","items":[1,true,null]}`，再检查 `{` 和 `1e400` 的反馈。
- 保存一份带空白和换行的草稿，关闭重开及退出进程重启，确认原文恢复；另存未完成 JSON，确认不会被自动格式化。
- 编辑后重新读取，分别选择保留和放弃；确认未保存关闭后重开使用已保存内容。快速重复点击、打开/关闭/重开时不得出现旧结果。
- 保存处理中继续编辑：已保存旧输入，后续编辑仍标为未保存。若出现保存结果待确认，先重新读取核对，不连续重复保存。
- Light/Dark 检查按钮、输入、状态、禁用和焦点；Tab/Shift+Tab 顺序与可访问标签；长输入和输出在 520×360 外层窗口内滚动（宿主标题栏 44px，内容约 520×316），确认无外层溢出。

代理未启动原生 Host、安装插件或操作真实数据。原生交互、WebView2 实际事件、主题和持久化人工验收均仍待用户完成。
