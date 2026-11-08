# macOS 公开插件 HTTPS 共用基础

将 Windows 已有的 HTTPS broker、请求 registry、地址策略和原生 TLS transport 原样纳入 Mac 编译与自动验证。沿用[公开插件合同](../../plugin-sdk/public-plugin-v1.md)及[开发指南](../../plugin-sdk/public-plugin-developer-guide.md)，不增加 SDK 方法，不开放 Mac 插件运行或网络权限。

## 编译边界

Mac 的窄 `public_plugins` 模块只暴露内部 manifest 同一类型、网络核心及共用音频 authority 状态机，复用既有源文件和测试。它不实例化插件 manager、scheduler、state、runtime、secret store 或权限服务；没有初始化 DNS worker/传输，生产 HostCapabilities.plugins 继续 false。`network::authority_gate` 仍只由 Windows 完整装配编译；Mac 尚未完成的上下文/撤权接线必须明确留待后续，不能写一个成功的占位服务让全量模块通过编译。

将 `SensitiveBytes` 从 Windows DPAPI 文件提取为共用模块，Windows 原路径 re-export，保持 new/as_bytes、Debug 脱敏与 Drop volatile 清理的行为和调用兼容。Mac 复用此真实内存容器，不提供假的 PluginSecretStore，也不在这里装配 Keychain。manifest 直接 re-export 当前 Mac 已编译定义，不复制 schema 或产生第二套类型。

音频 authority 只是实例/owner/generation 准入状态机，编译通过不等于 WKWebView 原生隔离成立。现有 non-Windows native prepare/verify 继续明确失败；不得颁发生产 proof，不以 JS 覆盖或轮询假装提供 native mute/readback。WK 的暂停适配与探针另立规格，既有 readback/即时撤权缺口如实保留。

## 共有行为不得改变

HTTPS 固定合同继续使用当前 broker：合法请求形状、manifest 主机限制、解析地址过滤、连接地址固定与原始 TLS hostname、重定向逐跳检查、请求/响应大小上限、总截止时间、取消/终态和凭据目标绑定。保留全部 Windows 用例，避免为了 Mac 放宽 host/IP/证书或授权检查。

Native TLS 在 Mac 使用锁定依赖的系统后端。任何 Mac 失败先定位实际平台差异；禁止改成接受无效证书、禁用 hostname 校验、信任所有 root、继承不受控代理或跳过失败用例。未经授权的请求在最终 Host 权限服务接线前不会进入生产网络路径；测试中可直接实例化核心来验证行为。

## 自动验证与验收范围

首先确认新增 Mac 网络模块测试在平台上被真实列出，不能以零命中通过。测试分为 fake broker/DNS 控制流及真实 loopback TLS fixture：后者仅在 127.0.0.1 临时端口启动测试服务，使用动态生成证书/内存 trust anchor，绝不写系统 Keychain 或信任设置，不访问公网。

复用并实际运行现有 provider credential 注入、重定向、限额、过期/错误主机/不受信任证书、代理环境变量子进程、DNS 队列和取消、原生 TLS 限额/取消、并发终态等用例。真实 TLS 成功和各拒绝条件都须通过，不能只跑 fake transport。共用音频 authority 用例覆盖旧 native instance/旧 generation/重复 label 的撤权与导航准入，但 native mute 仍未验。

重点验证提取后的 SensitiveBytes 格式不暴露内容、Windows imports 和静态接口保持原样；不将本次 Mac 测试称为 Windows 原生回归。运行 `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib public_plugins::network::`、同命令过滤 `public_plugins::webview_audio_guard::`，并完成全仓协调器及 Find/登录项内部 feature 组合验证。

生产权限 gateway、运行时 bootstrap、WKWebView 原生隔离和同包双端插件实测仍待后续。本单元只是提前验证真实跨平台传输与生命周期底层，70% 节点交付清单必须保持这些边界。

## 2026-11-08 共享数据调用准入补充

Mac foundation 同源编译既有 `public_plugins/data_call_gate.rs`。不复制实现，不改 Windows 生产行为，不实例化数据服务或把该模块接成可调用的 Mac gateway。身份仍为 plugin ID、generation、activation ID、admission epoch；相同身份的重复 activate 不重新打开已经关闭的门。

自动验证除原有身份、关闭与等待调用结束用例外，明确覆盖：关闭后同身份 activate 仍拒绝新调用；旧 generation、activation 或 admission epoch 的 close/remove 不能撤销替代身份；旧实例两个 lease 逐个释放时仅其 drain 完成，新实例 lease 不受影响。测试直接观察各自 drain 的状态，不能用等待时间代替新旧归属断言。没有 manager、scheduler、凭据存储或原生媒体 proof 的装配，也不据此增加同包双端插件通过计数。
