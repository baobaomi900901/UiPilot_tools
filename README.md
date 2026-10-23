# UiPilot

Windows x64 启动器与本地公开插件宿主（当前版本 0.4.0）。宿主为 Tauri 2、React 和 Rust；公开插件使用受控的 v1 Runtime / Panel / Window API。

## 工具链与开发

安装 Node.js 22 LTS（CLI 最低要求 Node 20）、npm、Rust stable / Cargo、Windows MSVC C++ 构建工具与 WebView2。首次准备依赖运行 `npm ci`；Rust 离线验证需要本地已有 Cargo 依赖缓存。验证不会下载依赖或修复生成文件。

```powershell
npm run dev:vite   # 仅浏览器前端预览
npm run dev        # Windows 完整开发入口（检查 Everything 运行条件并启动 Tauri）
npm run build      # TypeScript 检查与 Vite 构建
npm run tauri -- build # 原生应用构建
```

完整开发入口需要已准备的 Everything.exe 及可用服务；请参阅 `scripts/dev-with-everything.ps1`。验证不安装或修改服务。安装器构建入口为 `npm run build:managed-everything`，其系统与 VM 验收独立进行。

## 验证

```powershell
npm run verify:local -- --list # 只列出阶段与实际发现数量
npm run verify:local           # 全仓本地门禁
npm run verify                 # 同义入口
```

九个阶段依次为：协调器测试、前端测试、前端构建、Rust 库离线测试、manifest Schema 只读检查、插件 CLI 完整 verify、全部公开插件 Node 测试、全部 SDK contract 类型检查、全部参考 package 静态校验。任一失败立即退出非零。公开示例递归发现并稳定排序，缺失、空集合或符号链接会失败；不自动更新 Schema，也不执行真实插件 Runtime 或请求真实 API。

聚焦检查（从仓库根目录运行）：

```powershell
node --test scripts/verify-local-foundation.test.mjs
npm test                         # 原有 Vitest 语义
npm test -- src/protocol.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
cargo run --manifest-path src-tauri/Cargo.toml --offline --bin generate_public_plugin_schema -- --check
npm run verify --workspace @uipilot/plugin-cli
node --test examples/public-plugins/com.uipilot.demo-panel/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.note/tests/sdk-contract.ts
```

部分 React 测试存在已知 `act(...)` 与 Ant Design 警告，应保留并结合断言结果检查。浏览器预览和单元测试不能替代真实 Windows 宿主的焦点、键盘、双主题、剪贴板粘贴、安装/升级/卸载以及 Everything 服务/安装器 VM 验收。

## 源码导航

| 位置 | 职责 |
| --- | --- |
| `src/` | React 启动器、设置及前端状态 |
| `src-tauri/src/` | Rust 宿主、系统集成与公开插件运行时 |
| `packages/plugin-cli/` | 独立本地插件打包与静态校验 CLI |
| `examples/public-plugins/` | 可测试的本地插件参考包 |
| `docs/plugin-sdk/` | 公开插件教程、API、类型和 Schema |
| `docs/ui-guidelines.md` | 宿主及插件 UI 规范 |
| `scripts/` | 验证、开发和安装器辅助入口 |

## 本地插件流程

先读[公开插件教程](docs/plugin-sdk/public-plugin-developer-guide.md)和 [UI 规范](docs/ui-guidelines.md)，选择最接近的参考示例，保持 `tests/` 与实际安装的 `package/` 分离。API 合同见 [v1 规范](docs/plugin-sdk/public-plugin-v1.md)、[类型定义](docs/plugin-sdk/uipilot-plugin-api-v1.d.ts)和 [Schema](docs/plugin-sdk/uipilot-plugin-v1.schema.json)。

```powershell
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-panel/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.demo-panel/package --output ./demo-panel.uipilot-plugin --platform windows
node packages/plugin-cli/dist/cli.mjs validate ./demo-panel.uipilot-plugin --platform windows
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-demo-plugin.ps1 -PluginId com.uipilot.demo-return
```

在 UiPilot 插件设置中选择 `package` 目录或生成的 `.uipilot-plugin` 产物进行本地安装。`pack` 必须显式指定源目录外的输出路径，后缀严格为 `.uipilot-plugin`，父目录须已存在且祖先无链接或 reparse point；输出文件系统须支持硬链接（Windows 通常为 NTFS）。已有目标不会覆盖，没有 `--force`，再次打包请换文件名。第三方开发者可安装提供的 npm tarball，使用同样的 `uipilot-plugin pack` / `validate` 命令，详见 [CLI README](packages/plugin-cli/README.md)。运行示例自身 Node 测试与 SDK contract 检查后，在真实宿主确认权限提示、打开/关闭/重载和数据持久化。

本仓库路线以本地包为范围；在线商店、在线发布、自动下载和云同步不属于当前基座。真实凭据请求、系统安装和服务变更需要单独的人工验收环境。

Windows Host `0.4.0+` 支持将已授权的 secret 作为精确 HTTPS host 的 bearer 引用消费，Runtime 仅提交 credential ID。参见 [credential demo](examples/public-plugins/com.uipilot.demo-credentials/README.md) 与 [SDK 合同](docs/plugin-sdk/public-plugin-v1.md#host-managed-https)。示例测试仅使用合成值和离线 fake provider；服务端回显、配置保存失败的非回滚与后续核验恢复边界必须按合同处理。CLI npm 包版本独立保持 `0.1.0`，校验目标 Host 为 `0.4.0`。
