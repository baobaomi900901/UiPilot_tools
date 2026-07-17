# Foundation Result Registry 合同设计

## 状态

- 日期：2026-07-18
- 状态：待书面复审
- 影响范围：Foundation Task 2 可信结果注册表

## 目标

主 WebView 只能接收展示 DTO，并只能用 `requestId + resultId` 请求执行。快捷方式、可执行文件和真实动作始终由 Rust 保存。注册表必须拒绝乱序查询、旧唤起、隐藏后的在途结果以及未知或过期 ID。

## 数据合同

Rust `ResultKind` 只包含 `Application` 和 `Status`，通过 camelCase 序列化为 `application` 和 `status`。TypeScript 镜像类型固定为：

```ts
export type ResultKind = 'application' | 'status'
```

不保留 `file` 变体或文件搜索占位。`subtitle` 与 `icon` 为 `None` 时不写入 JSON，以匹配 TypeScript 可选字段。

`ResultAction` 不实现 `Serialize`。响应 JSON 只能包含 `requestId`、`items` 以及每项的展示字段，不能包含 `appId`、快捷方式、可执行文件或其他动作参数。

## 注册表状态

`ResultRegistry` 使用现有计划中的一个 `AtomicU64` 和一个 `Mutex<RegistryState>`，通过 `Default` 构造，不增加依赖。

- `on_show(invocation_id)`：递增 generation，设为 active，保存新 invocation，序号归零并清空当前结果。
- `begin_query(invocation_id, sequence)`：只接受 active、invocation 匹配且 sequence 严格递增的查询；接受后立即清空当前结果，再返回只含 generation 与 sequence 的 token。
- `publish_if_latest(token, entries)`：只接受仍 active、generation 相同且 sequence 仍最新的 token。注册表忽略每个输入 `ResultItem.resultId`，为本次结果重新分配固定格式的 `requestId` 与 `resultId`，并同时保存私有动作映射。
- `resolve(request_id, result_id)`：只解析当前结果集，返回克隆的 Rust 动作；其他组合返回固定、不含 ID 或路径的错误。
- `hide_and_clear()`：递增 generation，设为 inactive，并清除 invocation、序号和当前结果。

新查询一旦被接受，上一份映射立即失效，即使新查询尚未发布。无效序号或 invocation 不清空当前结果，因为该请求从未取得查询所有权。

## 标识与错误

标识由标准库原子计数器生成，格式固定为 `req-` 或 `item-` 加 16 位小写十六进制数字。标识只承载唯一性，不承载应用身份或目标信息。

错误最小化为 `StaleRequest` 与 `UnknownResult`。其显示文本为固定常量，不拼接请求 ID、结果 ID、应用 ID 或路径。

## 测试合同

Rust 单元测试必须覆盖：

1. 最新 token 发布时覆盖调用方伪造的 `resultId`，并生成唯一固定格式标识。
2. 当前 `requestId + resultId` 能解析到 Rust 动作，响应序列化不包含动作或路径。
3. 查询 2 发布后，晚完成的查询 1 不能覆盖或执行。
4. 查询 2 先到 Rust 后，查询 1 不能取得 token。
5. 新查询开始后，上一份已发布映射立即失效。
6. 查询开始后隐藏，晚到结果不能重新发布。
7. 新 show 后，旧 invocation 的 IPC 被拒绝且不清空新状态。
8. 未知和过期 ID 返回固定错误，错误文本不泄露路径。
9. JSON 字段为 camelCase，kind 仅为 `application` 或 `status`，空可选字段被省略。

Task 2 不新增 Vitest 文件；`src/protocol.ts` 只通过现有生产 TypeScript 构建验证。

## 非目标

- 不实现应用扫描、搜索、执行、窗口生命周期或 UI 渲染。
- 不实现文件搜索，不增加 `file` 类型、文件路径或文件动作。
- 不增加 UUID、数据库、缓存、异步运行时或新依赖。
