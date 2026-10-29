# 截图元素探测实施计划

> 历史计划，已被用户“恢复rust_uia初始状态，仅使用原有编译物和能力”的要求替代。不得继续执行其中新增外部接口的任务；当前实施见 [原版Provider接入计划](2026-10-29-screenshot-original-provider.md)。

目标：独立rust_uia编译物提供按点探测，UiPilot实现实时50%遮罩镂空、单击元素和防抖自由拖框，并贯通dev与Setup。

架构：Provider JSON-RPC + 宿主后台管道客户端 + 原生选择状态机；复用现有预览与输出。设计合同见 ../specs/2026-10-29-screenshot-element-probe-design.md。

## 技术与全局约束

Windows Rust/windows、Tauri 2、Python构建工具。rust_uia源码不迁移；物理半开坐标；不触发底层点击；主消息线程不等UIA；50%黑遮罩；无过期结果；现有用户改动保留。设计§2为唯一协议来源。

## 执行规则

每任务先聚焦失败测试再实现/验证，单独审查。日志和报告存.superpowers/sdd/element-probe/。提交只包含本任务文件/hunk，不夹带原有修改。任务1、2可独立；任务3消费协议；任务4消费任务3；最终统一验证。功能分支原地工作沿用用户选择。受限目录写入通过正式权限升级；不安装Setup。

### Task 1: 独立Provider按点探测

**Files:** D:/code/rust_uia/src/point_probe.rs、src/main.rs、src/uia_inspector.rs（仅必要复用）、README.md及对应tests。

- [x] 实现设计§2目标选择、UIA下钻、预算及轻量响应，注册方法和能力。
- [x] 提供可控原生窗口fixture，覆盖存在排除覆盖层时的真实子控件命中。

**Distinct test coverage:** 错误参数、负坐标、边界、不可见/排除目标、循环节点、超时不返回部分命中。调用不写文件不启动inspect。

**Verify:** `cargo test point_probe -- --test-threads=1`，`cargo build --release --locked`（rust_uia目录）。

### Task 2: 共享构建工具与dev/Setup

**Files:** scripts/provider_build.py、scripts/dev_uipilot.py、scripts/test_provider_build.py、scripts/test_dev_uipilot.py、D:/code/.tools/dev-uipilot.py、src-tauri/tauri.conf.json、scripts/package.py、scripts/package_progress.py、对应测试、.gitignore。

- [x] 实现设计§5独立源码构建、核验、清单、原子暂存、debug路径覆盖。
- [x] 创建外部薄入口，贯通Tauri钩子、Setup资源和阶段日志，保留已有Everything流程。

**Distinct test coverage:** 构建失败绝不复制旧EXE、错误能力阻断、dirty记录、路径含空格、哈希不符阻断bundle、dev只启动本次资产、无递归dev调用、Setup不用开发绝对路径。

**Verify:** `python -m unittest discover -s scripts -p "test_*provider*.py"`及dev/package相关测试。

### Task 3: 宿主Provider通信

**Files:** src-tauri/src/screenshot/probe.rs、screenshot/mod.rs、screenshot/window.rs、Cargo.toml（若必要）。

- [x] 实现设计§4异步worker/受限读取/子进程所有权/能力握手/最新位置合并。
- [x] 提供ProbeWorker::start(PathBuf, Vec<isize>)、request(PhysicalPoint)->u64、poll()->Option<ProbeReply>；ProbeReply含sequence,point,rect:Option<PhysicalRect>,unavailable:bool。Drop非阻塞取消，工作线程关闭进程。路径解析debug环境UIPILOT_SCREENSHOT_PROVIDER_EXE，release安装resources/provider。

**Distinct test coverage:** A在途B替换等待C后仅发送C、错误id/矩形/尺寸拒绝、硬超时回收、取消后不接收结果、命名管道无响应不阻塞选择。

**Verify:** `cargo test --lib screenshot::probe -- --test-threads=1`。

### Task 4: 原生交互与镂空

**Files:** src-tauri/src/screenshot/selection.rs、windows/overlay.rs、windows/capture.rs、windows/mod.rs、model.rs（必要）、src相关文案（必要）。

**Dependencies:** Task3客户端接口；设计§3。

- [x] 实现防抖状态机与异步候选代际检查。
- [x] 原生实时50%遮罩+透明镂空，输入捕获防穿透；按下取帧冻结，drag自由跨屏；加入客户端请求和轮询。
- [x] start_selection增加provider:Option<PathBuf>参数，宿主window.rs提供路径。

**Distinct test coverage:** 抖动点击原候选、拖动后回起点不变点击、按下后晚到候选无效、零面积恢复、光标负坐标、无命中不留旧框、取帧切换不丢输入，取消焦点不窃取。

**Verify:** `cargo test --lib screenshot:: -- --test-threads=1`；可控桌面fixture验收。

### Task 5: 集成审查与交付

**Dependencies:** Tasks1-4。

- [x] 代码审查修复重要问题，运行前端全测、Rust聚焦测试、Python构建测试和Release/NSIS构建。
- [x] 更新 docs/testing/builtin-screenshot-acceptance.md，记录自动验证和未执行的真实多屏验收。
- [x] 提供dev脚本、Setup及测试指引，保留所有原有用户修改。
