# 截图接入 rust_uia 原有编译物

按用户最新要求，撤回 rust_uia 本次截图任务的全部源码修改。主程序仅调用原有 EXE 的元素树协议，不复制 UIA 实现、不新增外部仓库接口。交互继续使用入口桌面帧、原生遮罩、单击选区与防抖自由拖框。

技术：现有 Rust/Win32、Tauri、命名管道 JSON-RPC、Python dev/Setup 构建链。以 [设计](../specs/2026-10-29-screenshot-element-probe-design.md) 的最新边界与协议为准；用户原有未提交文件不纳入提交。外部仓库恢复后只允许按既有脚本编译，不再修改源码。

共同流程：聚焦回归先失败后通过；每个实现单元经复审并单独提交，保留实际验证证据。依赖：Task 1 → Tasks 2、3 → Task 4。真实测试只创建自有夹具，不模拟用户输入或安装 Setup。

### Task 1: 恢复独立仓库

**Files:** rust_uia 本任务引入的 Cargo.toml、README.md、src/main.rs、src/point_probe.rs。

- [x] 备份差异，确认没有其他用户修改，恢复到初始源码9cefba7。
- [x] 创建撤回提交ca3b9eb并验证完整源码差异为空。

**Distinct test coverage:** 恢复仅覆盖本次截图引入内容；已有提交历史保留。
**Verify:** `git -C D:/code/rust_uia diff 9cefba7 --exit-code`。

### Task 2: 使用原有构建能力声明

**Files:** scripts/provider_build.py 及对应 Python 测试。
**Dependencies:** Task 1；设计第 5 节。

- [x] dev、Setup、清单校验改用原有元素树能力（f54c64f）。
- [x] 缺少能力或资产不一致仍阻断启动/打包，不回退旧新增接口 EXE；Python38项通过。

**Distinct test coverage:** 任一必需原有能力缺失时失败；完整原有能力无point_probe_v1时成功。
**Verify:** `python -m unittest discover -s scripts -p test_provider_build*.py` 及已有 dev/打包测试。

### Task 3: 主程序元素树适配

**Files:** src-tauri/src/screenshot/probe.rs。
**Dependencies:** Task 1；设计第 2、4 节。

- [x] 使用原有elementtree会话及有界子树读取，缓存几何供悬停查询（2e77c6b）。
- [x] 原生目标窗口排除覆盖层；完整性、大小、身份和会话边界校验；第三项会话能力经复审补齐（7d8ed91）。
- [x] 主程序自有原生夹具替代已撤回的外部探测测试夹具；原版EXE真实管道集成通过。

**Distinct test coverage:** 空矩形、父子不相交的树条目、重复矩形；部分/过期/错序响应不冒充完整命中；缓存命中不重复远程遍历，取消关闭自有Provider。
**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib screenshot::`，再显式运行真实 Provider 集成。

### Task 4: 构建与使用验证

**Files:** 截图开发、设计及验收文档；生成的Provider/Setup资源。
**Dependencies:** Tasks 2、3。

- [x] 从恢复后的独立源码重新编译并核对能力及哈希，源码仍与9cefba7完全一致。
- [x] 验证 dev 共享准备链与 Setup 均可使用原有编译物，记录实际产物；79项截图回归、38项Python测试及真实集成通过。
- [x] 提供重启完整dev和资源管理器导航树复测步骤；原版接入后的实机结果仍待用户反馈。

**Verify:** 共享Provider准备工具、真实管道集成、`npm run tauri -- build --bundles nsis`。
