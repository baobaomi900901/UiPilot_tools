# macOS 截图接入 mac_acc

目标：复用独立 mac_acc 的 AX 几何，不在 UiPilot 重写 AX 遍历，宿主管理冻结帧/选择/输出及 Windows 已有截图交互。遵循[消费设计与协作请求](../specs/2026-11-16-macos-screenshot-mac-acc-design.md)，具体新增 RPC 已收到对方设计回复，消费方首轮取舍已确认；最终 schema/制品与 G0 仍待完成。

技术为现有 Rust/Tauri2/AppKit 与独立 arm64 Swift/Rust Provider，Mac 最低 15.7.4。无新运行依赖决定，禁止 GUI 自动化或真实屏幕/权限操作；不合并 main。协议/签名门槛不阻塞纯模型和 fake-service；不能提前开放截图 capability。

全局规则：行为改动先有聚焦失败断言再实现，按逻辑单元提交并自查；不重复无新信息的全量测试。Task 1 → Task 2；Task 3 依赖 Task 2 及 mac_acc 回传接口/制品，Task 4 依赖 Task 3。实际 GUI 由用户一次一个动作、合并同类检查。

### Task 1: 核对现有能力并发出跨项目需求

**Files:** 消费设计、最终需求/总计划链接、mac_acc GitHub issues。

- [x] 读取当前协议/原生实现/打包脚本，对照 Windows 最终截图合同，区分 AX 与图像采集。
- [x] 创建 #1 geometry-only 目标约束接口、#2 arm64 制品/TCC/进程合同，复核远端标题/正文。

**Distinct test coverage:** 当前全局命中仅排除 Provider 本身不能满足遮罩穿透；现有预览不能当作只读几何；现有 CLI TCC 不能当作嵌入 UiPilot 授权证据。仅核对代码与文档，不执行原生能力。

**Verify:** `gh issue view 1 --repo baobaomi900901/mac_acc --json number,title,body,url,state`，同命令查 2；文档 `git diff --check`。

## 回传后的当前顺序

**2026-11-17 用户暂缓截图开发，等待 mac_acc 准备好；Tasks 2–4（包括宿主 G0 验证器及纯逻辑/模拟测试）均暂停。** 下列顺序在后续恢复时执行，不继续创建截图代码或安排截图人工测试。

恢复后先准备 UiPilot G0 宿主验证器和纯逻辑/模拟测试，取得 mac_acc 的最小开发包/schema；G0 遮罩与窗口关联实证完成后冻结新协议。G0 前不把自然语言字段写成生产 DTO，不要求对方先完成全量 G1–G6。Task 3 运行接线/交付另受内部许可及嵌入 TCC 门槛约束。

- [x] 已读取两条回复，接受 API 命中、条件性唯一几何关联、冻结图尽力一致性；方法数组与 features 对象明确分开。
- [x] 普通 CLI 首选 Helpers 目录，记录重签名前后 hash；生产许可由权利人明确。
- [ ] 对方 schema、最小包及 source/hash 尚未回传，不把建议版本号计作交付。

### Task 2: 独立完成宿主截图基础及模拟 Provider

**Files:** `src-tauri/src/screenshot/` 共享模型/geometry/service 与新增 macOS 后端测试，Mac 调用来源/能力门禁；具体采集 API 在实现前核对本地 SDK。

**Dependencies:** Task 1；消费设计“双方分工”“可先推进与验收”。

- [ ] 在真实 UiPilot Mac 宿主准备隔离 G0 验证器（默认产品截图仍关闭），只在用户动作下执行权限检查/遮罩；不读取业务正文，不自动运行 GUI。
- [ ] 区分 AX point、AppKit point、每屏帧像素，固定显示器布局身份与跨屏合成策略；冻结帧不混入后续时刻。
- [ ] 以 fake capture/probe/output 跑取消、迟到、最新点、按下锁定及拖动状态机；采用现有共享像素预算/输出逻辑，保持 Windows 不变。

**Distinct test coverage:** 负原点/上下屏/混合 scale/掉屏；新 probe 未完时取消、按下或 target/layout 变化后旧响应零发布；AX 无权限/超时自由框选仍可用，录屏拒绝无帧；旧保存完成不能关闭新截图会话。

**Verify:** `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked --offline screenshot`；Mac default/preview all-targets；新增过滤器必须命中有效断言，未创建的后端不能记作通过。

### Task 3: 冻结 Provider 合同并接入制品与原生服务

**Files:** 后续 `macos_acc` 客户端/协议测试、资源构建和校验脚本、Mac host、ACL/配置、版本化制品清单。

**Dependencies:** Task 2 的 G0 准备与实证；mac_acc #1/#2 回传最终 API、源码/制品哈希和部署身份、内部许可范围；消费设计“请求的 Provider 合同”“打包、权限和安全门槛”。

- [ ] 按最终 schema 做私有 stdio、有界队列/超时/回收及严格响应校验；只接受目标身份匹配的几何。
- [ ] 构建 staging/校验/资源定位和嵌套签名，验证无开发路径依赖；明确 ad-hoc 与生产 TCC 的实际边界。
- [ ] 宿主自行实现原生采集/遮罩/编辑输出并复用现有收藏/启停/快捷键，不暴露 Provider 到插件通用 RPC。

**Distinct test coverage:** 缺文件/错 hash/缺 capability/错误 response id/超大帧拒绝；EOF/崩溃/卡死只回收自有进程；撤权和重授权旧结果仍失效；保存取消保留旧预览，失败不得沿用旧制品假装成功。

**Verify:** Provider schema/transport 聚焦测试，离开源码目录握手 smoke（不调用 AX/截屏），资源 hash/arm64/minOS/rpath/codesign 检查及截图相关回归；证书部分单列。

### Task 4: 对齐完整截图交互并交付最小人工节点

**Files:** 截图原生/预览界面、快捷键/启停/收藏、相关主题与键盘测试、设备验收单和内部包。

**Dependencies:** Task 3；Windows 后续标注/调整/贴图合同及消费设计“可先推进与验收”。

- [ ] 对齐区域/窗口、调整/标注/贴图/复制/PNG 保存，沿用 UI 规范及 Mac ⌘ 修饰键；未开放项明确记录。
- [ ] 自动验证和内部包校验后才通知用户；一条截图链路加混合 scale/取消必要检查，不重复无关人工测试。

**Distinct test coverage:** 遮罩不穿透点击，不捕获本应用遮罩；权限拒绝/撤销、换显示器、重复触发、重新截图取消、输出失败，均有受控行为；Developer ID/公证和稳定权限另作发布门槛。

**Verify:** Mac default/preview build、TypeScript/Vite、相关 Rust/前端/资源校验；Windows 原生回归与用户最小人工闭环分别记录，不把自动模拟计作实机通过。
