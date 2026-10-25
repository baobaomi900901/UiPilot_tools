# 文本片段 1.0.0

Windows UiPilot Host **0.10.0+** 的离线主结果动作示例。以 demo-multi 的双命令包为结构参考，借鉴 json-workbench 的完整记录校验与版本比较；图标原样复用仓库 notes 插件的固定 notebook PNG。只有 `clipboard.write` 权限，无网络、剪贴板读取、窗口或面板。

## 安装

在设置 → 插件中使用 **选择开发目录**，选择这个完整目录（不要选择其父级项目根目录）：

```text
C:\Users\moby\.codex\worktrees\local-plugin-foundation\UiPilot_tools\examples\public-plugins\com.uipilot.text-snippet\package
```

或者使用 **选择插件包**，选择验证后生成的归档文件：

```text
C:\Users\moby\.codex\worktrees\local-plugin-foundation\UiPilot_tools\.superpowers\deliveries\uipilot-0.10.0-acceptance\plugins\com.uipilot.text-snippet-1.0.0.uipilot-plugin
```

安装本示例不会自动保存或读取系统剪贴板。插件的最新片段仅存在于本插件的私有存储中。

## 人工检查

1. 输入 `/text-snippet Hello 片段` 并提交。应出现“原文”和“大写”两行；仅转换不会保存。选“大写”，用底栏 **动作 Ctrl+K** 选择“保存为最新片段”。确认“已保存”，再输入 `/latest-snippet`；应读到 `HELLO 片段`。也分别用右键和 Ctrl+K 打开动作菜单，确认同样可用。
2. 从最新片段行按 Enter 复制，核对“已复制”、启动器保持打开、旧结果动作禁用；在临时编辑器中自行粘贴核对内容。显式“重新运行”后才可再次操作。
3. 在最新片段行选择“重新读取”，应换成新结果。关闭重开，再重启 Host，运行 `/latest-snippet` 核对持久化。首次没有数据时应显示“尚未保存片段”，并可在下一行通过菜单显式重新读取。
4. 动作期间修改输入、切换视图或隐藏重开，旧反馈不应覆盖新视图。保存结果未确认时先重新读取核对，再决定是否重新保存，勿盲目重试。
5. 在 Light/Dark 中检查结果、菜单、焦点和禁用态；用方向键、Enter、Escape、Tab/Shift+Tab 与中文输入法操作。长文本不应撑破布局。

原生菜单、复制与重启持久化由用户在实际 Host 验收；Node 检查只使用内存假存储，不能证明这些原生交互。

## 数据与边界

两个稳定命令 ID 为 `transform` 与 `latest`，均为 submit/mainResult。onCommand 只读取私有数据；只有显式保存动作写入 `snippet.latest`，完整记录为 `{schemaVersion:1,revision:正安全整数,text:字符串}`。不存在的记录视为版本 0；空字符串与不存在不同。

输入和大写结果均限 8192 UTF-8 字节。NUL、结果整体超过 64 KiB 时显示固定信息且无动作。未知记录版本、额外字段、非法版本或过长文本阻止写入，绝不重置。保存独立校验 payload，重新读取并比较版本，检查递增溢出，再等待唯一一次完整记录写入；冲突不会覆盖已有内容。写入失败也可能已经提交，显示“保存结果未确认，请重新读取核对”，不会自动重试或声称未保存。

## 本地检查

```powershell
node --test examples/public-plugins/com.uipilot.text-snippet/tests/runtime.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --strict --noEmit examples/public-plugins/com.uipilot.text-snippet/tests/sdk-contract.ts
node --test scripts/verify-local-foundation.test.mjs
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.text-snippet/package --platform windows --json
```

开发目录之外的 README、测试及 package.json 不属于安装包；不要将它们移入 package。
