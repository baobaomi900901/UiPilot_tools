# 可视化打包工具实施计划
目标：交付Windows双击GUI，同时包装既有生产NSIS和插件CLI流程。架构/行为以[设计](../specs/2026-10-27-visual-packager-design.md)为准。技术为PowerShell5.1、WinForms及仓库既有工具链，无新增依赖。

工作区：复用既有隔离工作树，分支codex/visual-packager；D主目录只读。核心/worker/UI作为一个可独立验收切片，一位实现者，先聚焦RED再GREEN，完整切片Spec/Quality独立审查。无宿主实现/安装器钩子/CLI合同修改，不启动真实GUI或Setup，不自动推送。脚本中文使用UTF8 BOM。

### Task 1: 完整桌面打包切片
**Files:** Pack.cmd；scripts/package-gui.ps1；scripts/package-runner.ps1；scripts/package-runtime.ps1；scripts/test-package-tool.ps1；docs/packaging-tool.md；README.md；.gitignore（仅新输出根）。
**Dependencies:** 设计的操作、执行边界、Setup链路、插件链路、验证与交付各节。
- [ ] 纯核心/worker实现固定生产流程、路径预检、JSON协议、日志/结果/新产物证据。
- [ ] WinForms双页签、选择目录、版本/元数据显示、后台执行、滚动日志、状态/耗时、限定取消和打开输出。
- [ ] 离线fake runner测试先RED后GREEN；不覆盖、失败短路、旧Setup及取消防误报。
- [ ] PS5.1解析/静态UI检查、真实插件CLI smoke、独立审查，记录实际GUI/Setup待人工验收。
- [ ] 用户文档、README双击入口和三项短人工用例，原子提交。
**Distinct test coverage:** 用户路径不作为脚本求值；取消只影响owned worker树；worker退出与结果核对不可误报成功；旧setup不能被选为新输出；CLI产物失败不得继续；既有输出保持。
**Verify:** powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-package-tool.ps1；Windows PowerShell Parser；既有CLI validate真实smoke归档；git diff --check。只验证本切片，不重复全仓测试或重新构建UiPilot。