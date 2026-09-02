# Raycast-Inspired Plugin Platform Roadmap Design

**Date:** 2026-09-02
**Status:** Review — conversational sections approved; written specification awaits user acceptance

**Related:**

- [`Raycast Kill Process 与 UiPilot 功能差异研究`](../../research/2026-09-02-raycast-kill-process-uipilot-comparison.md)
- [`UiPilot Public Plugin API v1`](../../plugin-sdk/public-plugin-v1.md)
- [`UiPilot 第三方插件开发教程`](../../plugin-sdk/public-plugin-developer-guide.md)
- [`UiPilot Plugin API v1 declarations`](../../plugin-sdk/uipilot-plugin-api-v1.d.ts)
- [`UiPilot Plugin Manifest v1 schema`](../../plugin-sdk/uipilot-plugin-v1.schema.json)
- [`UiPilot UI 规范`](../../ui-guidelines.md)

## 1. Goal

在 UiPilot 现有 Public Plugin API v1 上，逐阶段建设完整的插件开发与分发生态：宿主管理的系统能力、Raycast 式命令与交互、稳定 Runtime 生命周期、开发者工具、AI Tools、跨平台 Adapter、开发者身份与签名、审核、插件商店、自动更新、回滚和长期兼容治理。

目标是参考 Raycast 的开发体验和功能结构，而不是复制其宽权限 Node.js 安全模型。内置功能、公共插件和 AI Tools 必须通过同一套宿主 Capability Interface 使用系统能力；插件不得绕过宿主直接访问 Shell、Tauri、任意文件、原生二进制或无限制 Node 能力。

## 2. Scope

本路线图包含：

- Windows 与 macOS 插件平台。
- 宿主管理的系统能力，以及权限、授权、撤销、配额和审计。
- 多命令、结构化视图、上下文 Actions、快捷键、确认和反馈。
- Runtime 生命周期、后台任务和资源治理。
- TypeScript SDK、CLI、打包、开发模式、测试和诊断。
- AI Tool 声明、调用、确认和 Evals。
- 插件身份、签名、审核、商店、安装、更新和回滚。
- Interface 版本、弃用、兼容、隐私和安全事件治理。

路线图只规定阶段目标、依赖、统一原则和退出条件。每个阶段的具体 Interface、文件、实现和测试必须在该阶段开始时单独设计；不得把本路线图当作后续阶段的详细实施计划。

## 3. Non-goals

- 直接运行未经适配的 Raycast 插件源码。
- 实现 `@raycast/api` 的源码级兼容层。
- 向公共插件开放通用 Shell、任意 Node built-ins、任意 Tauri invoke 或不受限原生二进制。
- 在前一阶段未验收时冻结后一阶段的详细 Interface 或开始开发。
- 在单个阶段中顺带重构与阶段目标无关的现有代码。
- 在商店、签名和更新合同未成熟前接入真实支付、商业分成或第三方广告系统。

## 4. Chosen Evolution Strategy

采用“宿主管理、逐阶段深化”的演进路线。

现有 Public Plugin API v1 是兼容基线。每项新增能力都以完整垂直切片交付：宿主 Module、权限、Runtime Interface、Manifest/Schema、CLI 校验、参考或验收 fixture、文档、自动测试和真实平台验收。平台先建立安全能力底座，再扩展插件表达能力、开发者体验和分发生态。

### 4.1 Rejected: Raycast source compatibility first

先引入完整 Node Runtime、React 声明式渲染和 `@raycast/api` 兼容层会降低部分 Raycast 插件的迁移成本，但会同时扩大 Runtime、渲染、依赖、权限和安全范围，并与 UiPilot 当前宿主管理能力的原则冲突。

### 4.2 Rejected: distribution first

先做签名、审核和商店会得到治理完整但能力有限的平台。之后 Runtime 和 Interface 的频繁演进还会增加商店兼容与迁移成本，因此分发体系必须等待包格式、权限模型和开发工具基本稳定。

## 5. Platform Architecture

平台由六组深模块组成：

1. **Runtime Module**：加载插件代码，建立请求所有权，执行命令和 Tool，并实施超时、取消和资源限制。
2. **Interaction Module**：宿主渲染或承载结构化 UI、Actions、快捷键、确认和反馈。
3. **Capability Module**：向 Runtime 暴露最小的权限化 Interface，并完成权限、请求和调用策略准入。
4. **Host-owned System Modules**：封装 Process、Files、Clipboard、Network、Notifications 等平台实现。
5. **Trust Module**：处理插件身份、权限授权、签名、审核状态、撤销和安全处置。
6. **Distribution Module**：处理 Registry、Store、版本选择、安装、更新、健康检查和回滚。

SDK、CLI、测试框架和开发模式是贯穿上述模块的开发者工具面。AI Tools 是 Runtime 的一种入口，不拥有单独的系统执行通道。

```text
Plugin Package
      │
      ▼
Runtime Module ───────► Interaction Module
      │                  UI / Actions / Feedback
      ▼
Capability Module
      │
      ▼
Host-owned System Modules
      ├─ Process
      ├─ Files
      ├─ Clipboard
      ├─ Network
      └─ Notifications

Trust Module ─────────► Identity / Permissions / Signing / Review
Distribution Module ──► Store / Install / Update / Rollback
```

内置功能、公共插件和 AI Tools 必须调用相同的 Host-owned System Module Interface。权限判断、目标复核、提权、确认和审计不能在调用者中各自实现。

## 6. Ordered Roadmap

### Stage 1: Process identity and operation admission

建立可靠的进程目标身份、PID 复用防护、禁止目标、调用准入和稳定错误语义。该阶段只定义后续进程操作必须共享的安全基础，不提前实现终止、提权、插件权限或 AI Tool。

**Exit:** 进程身份与准入 Module 可通过假 Adapter 和真实 Windows 场景验证；并发目标变化、UiPilot 自身和受保护目标具有确定结果。

### Stage 2: Host-owned process control

在 Stage 1 Interface 后实现普通终止、强制终止、进程树、同名批量、重启和 Windows 提权。先通过宿主内部入口完成端到端产品闭环。

**Exit:** 所有进程操作复用同一准入路径；真实 Windows 权限、目标变化、部分失败和重启行为通过人工验收。

### Stage 3: General Capability model

把 Process 垂直切片抽象为通用的 Capability 模型：Manifest 权限、安装授权、撤销、Runtime facade、调用配额、审计和稳定错误。Process 是第一项完整参考能力，不开放通用 Shell。

**Exit:** 至少一项能力从 Manifest、安装确认、Runtime 调用到宿主执行形成完整、可撤销和可测试的参考链。

### Stage 4: Raycast-style commands and interaction

增加多命令、结构化 List/Grid/Detail/Form、上下文 Actions、快捷键、破坏性样式、宿主确认和反馈。Interaction Module 负责一致的主题、键盘、可访问性和调用授权。

**Exit:** 插件可声明多个命令，并通过宿主 Interaction Module 使用结构化视图和授权 Action；不需要插件模拟宿主确认。

### Stage 5: Runtime and lifecycle

定义命令模式、请求取消、超时、资源预算、后台执行、菜单栏、状态恢复和故障隔离。后台能力必须独立授权，且不能成为长期逃逸请求所有权的通道。

**Exit:** 前台、后台和长生命周期入口具有明确的所有权、取消、故障和资源语义，并通过事件顺序测试。

### Stage 6: Developer toolchain

交付稳定 TypeScript SDK、CLI、构建打包、Schema 同步、开发模式、热重载、日志、测试框架、诊断和迁移辅助。开发者不需要阅读宿主实现即可完成插件开发、测试和打包。

**Exit:** 外部开发者只依赖公开 SDK、CLI 和文档即可创建、验证、运行、测试和打包插件。

### Stage 7: AI Tools

增加 Tool Manifest、参数类型、宿主确认、Capability 调用、Evals 和 AI 可用性策略。Tool 复用 Command/Action 的请求所有权、权限和系统准入，不直接执行平台命令。

**Exit:** AI Tool 能在同一权限链中完成只读与破坏性操作；破坏性调用必须经过宿主人在回路确认，并有可重复 Evals。

### Stage 8: Cross-platform adapters

为已经稳定的 Host-owned System Module Interface 补齐 macOS Adapter。平台差异通过能力检测、明确错误和 Manifest 平台声明表达，不能改变共享语义。

**Exit:** Windows/macOS 合同矩阵通过；不支持的能力在安装或调用前明确拒绝，而非运行时静默退化。

### Stage 9: Trust and review

建立开发者身份、包签名、来源证明、权限审查、静态检查、恶意包处置、签名撤销和审核记录。审核结论必须绑定不可变包内容。

**Exit:** 每个可分发包具有可验证身份、内容 Hash、签名、审核状态和撤销策略。

### Stage 10: Plugin Store

实现发布、版本、搜索、分类、详情、权限展示、兼容筛选、安装、下架和开发者管理。商店元数据不能替代包签名或本地 Host 校验。

**Exit:** 开发者发布到用户发现、检查权限、安装和禁用/卸载形成完整闭环；下架不会破坏已安装包的本地可控性。

### Stage 11: Automatic update and rollback

实现不可变包下载、签名验证、兼容检查、权限增量确认、Staging、原子切换、健康检查、失败回滚和版本固定。

**Exit:** 更新不能绕过签名和新增权限确认；任何失败保留或恢复上一健康 generation。

### Stage 12: Ecosystem governance

固化 Interface 版本、弃用周期、兼容测试、隐私政策、安全事件响应、审核申诉和长期维护规则。

**Exit:** 平台能够发布新版本、弃用旧能力、响应恶意插件或密钥泄露，并给开发者确定的迁移窗口。

## 7. Dependency Order

```text
Stage 1 → Stage 2 → Stage 3 → Stage 4 → Stage 5 → Stage 6
                                                │
                                                ▼
Stage 12 ← Stage 11 ← Stage 10 ← Stage 9 ← Stage 8 ← Stage 7
```

阶段之间默认串行。某阶段内部只有在该阶段实施计划证明任务独立、不会共享未冻结 Interface 或状态时才可并行。

Stage 8 可以为不同 Host-owned System Module 分批交付平台 Adapter，但任何 Adapter 都必须等待对应共享 Interface 在前置阶段验收。Stage 9 的签名原型可以在 Stage 8 期间做不进入生产的研究，但签名设计、实施计划和生产代码仍须等待 Stage 8 阶段验收。

## 8. Stage Gate State Machine

每个阶段遵循：

```text
需求确认
  → 设计规格验收
  → 实施计划验收
  → 开发
  → 自动验证
  → 人工验收
  → 阶段验收
  → 下一阶段
```

硬性规则：

- 当前阶段未完成“阶段验收”，不得编写下一阶段的详细设计、实施计划或产品代码。
- 总体路线图只能记录后续阶段的目标、依赖和退出条件，不能提前冻结其 Interface 或实现细节。
- 验收失败只修复当前阶段，不通过开发依赖阶段绕过失败。
- 发现新需求时，先归类到当前阶段、未来阶段或独立项目；不得顺手扩展当前实现。
- 每个阶段必须独立可测试、可回滚，并产生可使用或可验证的增量。

只有同时满足以下条件，两个阶段或任务才可合并开发：

1. 分开会产生无法测试或无法使用的中间状态。
2. 两部分共享一个不可拆分的安全事务或业务提交点。
3. 合并理由写入当前阶段设计，并获得用户明确批准。

必须作为同一垂直切片交付的典型事项包括：

- 权限声明、宿主执行检查和安装授权确认。
- Action 声明、宿主展示和授权后调用。
- Runtime facade、请求所有权和请求过期检查。
- 自动更新下载、签名验证和激活前准入。

## 9. Interface And Version Policy

- Public Plugin API v1 保持兼容。
- 向后兼容能力通过可选字段、`minimumHostVersion` 和能力检测增加。
- 破坏性变更发布新的 Interface 主版本；旧、新版本必须在明确迁移期内并存。
- Manifest、Runtime 请求和响应继续严格解析；未知字段、重复字段和非法状态默认拒绝。
- 后续阶段只能消费已验收的公共合同。
- SDK、Schema、CLI 和 Host 解析器必须由一致性测试证明语义相同。

## 10. Permission And Execution Policy

- Runtime 永远视为不可信输入源。
- 系统副作用只由 Host-owned System Module 执行。
- 读取、普通修改、强制操作和后台执行分别授权。
- 高风险操作由宿主显示确认；插件不能伪造、隐藏或替代确认。
- 每次调用重新验证插件 ID、generation、请求所有权、权限和目标身份。
- 禁用、卸载、升级或请求过期后，旧调用不能继续产生副作用。
- 权限减少可以直接收窄；权限增加必须再次获得用户确认。
- 不开放通用 Shell、任意原生二进制或无限制 Node 能力。

## 11. Canonical Data Flows

### 11.1 Installation

```text
Plugin Package
  → Static structure and schema validation
  → Developer identity and signature verification
  → Host/platform compatibility check
  → Permission diff
  → User consent
  → Staging Runtime readiness
  → Atomic generation activation
```

任何一步失败都保留旧版本；新版本不能部分生效。

### 11.2 Invocation

```text
Command / Action / AI Tool
  → Request ownership
  → Runtime handling
  → Capability facade
  → Permission and policy admission
  → Platform adapter
  → Stable outcome or stable error
  → Host UI feedback
```

Command、Action 和 AI Tool 只能改变入口，不能绕过中间的权限与准入链。

### 11.3 Update

```text
Store metadata
  → Immutable package download
  → Content hash and signature verification
  → Compatibility check
  → Added-permission consent
  → Staging
  → Atomic switch
  → Health check
  → Success or automatic rollback
```

## 12. Error Model

公共错误至少覆盖：

- 输入、Manifest、包或响应无效。
- Host 或 Platform 不兼容。
- 权限未声明、未授权或已撤销。
- 请求、窗口、Panel、Action 或 Tool 已过期。
- 目标身份失效或目标已改变。
- 系统拒绝、需要提权或目标受保护。
- 资源超限、超时或 Runtime 故障。
- 签名、审核、下载、激活、健康检查或回滚失败。

公共错误名称保持稳定。系统路径、用户数据、命令行、密钥和内部异常默认脱敏。每个阶段的设计必须把属于该阶段的错误名称、可重试性、用户提示和终端状态写清楚。

## 13. Verification Strategy

每阶段验收材料统一包括：

- 已批准的阶段设计规格和实施计划。
- 聚焦测试以及受影响的完整回归结果。
- 权限、调用者授权、异步所有权、升级切换和失败回滚的事件顺序证据。
- Host Module 的假 Adapter 测试和真实平台冒烟测试。
- SDK、Schema、CLI、Host 解析器和文档同步结果。
- 系统交互、权限提升、通知、输入或窗口行为的人工验收记录。
- 当前阶段实际改动范围、兼容性影响和已知限制。

平台级测试资产包括：

- 每个公共 Interface 的合同矩阵和稳定错误矩阵。
- 恶意包、越权请求、过期 generation、资源耗尽和故障注入 fixture。
- Windows 与 macOS Adapter 的共享合同套件和平台专有验收。
- 安装、升级、权限增加、签名错误、健康检查失败和回滚的端到端场景。
- 商店发布、下架、版本选择和撤销的集成环境。

## 14. Approval And Next Step

本文档验收后，任务 1“总体路线图”结束。下一任务只为 Stage 1“进程身份与操作准入”编写独立设计规格和实施计划。

在 Stage 1 设计、计划、开发和验收完成前，不启动 Stage 2 的详细设计、计划或开发。后续所有阶段沿用同一验收门。
