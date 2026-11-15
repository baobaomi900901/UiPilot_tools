# 插件与开发示例索引

本索引区分插件的维护用途。`examples/public-plugins/` 是现有源码与验证入口，目录名不代表其中全部是 Demo，也不代表全部是正式业务插件。安装目录始终是各插件的 `package/`，版本与最低 Host 版本以当前 `plugin.json` 为准。

`json-workbench`、`templates`、`auth-check` 的源码目录已分别改为 `com.uipilot.demo-json-workbench`、`com.uipilot.demo-templates`、`com.uipilot.demo-auth-check`。本次只改目录，Manifest 的 pluginId 与启动命令保持原值；历史交付包名仍沿用原插件 ID。

代理开发入口是 [uipilot-public-plugin skill](../../skills/uipilot-public-plugin/SKILL.md)；API 使用方法见[开发教程](public-plugin-developer-guide.md)，视觉和交互以 [UI 规范](../ui-guidelines.md) 为准。

## 业务插件

| 插件 | 用途 | 可参考的专用能力 |
| --- | --- | --- |
| [notes](../../examples/public-plugins/com.uipilot.notes/README.md) | 内嵌面板笔记 | 列表与编辑、目录、当前 Panel 动态操作 |
| [clipboard-history](../../examples/public-plugins/com.uipilot.clipboard-history/README.md) | 剪贴板历史 | 历史搜索/复制、分类、Delete Host Key |
| [pomodoro](../../examples/public-plugins/com.uipilot.pomodoro/README.md) | 番茄钟 | 宿主持有计时器、通知、固定 WAV、时长设置 |
| [translate](../../examples/public-plugins/com.uipilot.translate/README.md) | 翻译 | 有道业务服务与受限划词入口 |
| [ocr](../../examples/public-plugins/com.uipilot.ocr/README.md) | 图片文字识别 | Host 0.21.0 双入口图片 Panel、可编辑文字、百度 OCR 与全文复制 |

`com.uipilot.note` 是已移除的旧独立窗口笔记插件；与仍维护的 `com.uipilot.notes` 不同。删除仓库源码不卸载用户本机的已安装副本，也不清除笔记数据。

## 基座能力参考插件

这些插件是在开发插件基座时形成的完整纵向示例，保留业务操作是为了验证能力组合、错误恢复和数据持久化，不列为正式业务插件。

| 插件 | 开发时选用场景 |
| --- | --- |
| [json-workbench](../../examples/public-plugins/com.uipilot.demo-json-workbench/README.md) | `window.runtimeActions` / `onWindowAction`、文件选择导入、草稿版本与保存结果待确认 |
| [templates](../../examples/public-plugins/com.uipilot.demo-templates/README.md) | `panel.runtimeActions` / `onPanelAction`、Form/List/Detail、模板预览、冲突保护 |
| [text-snippet](../../examples/public-plugins/com.uipilot.text-snippet/README.md) | `runtime.resultActions` / `onAction`、多命令、复制/URL/粘贴动作、共享存储 |

`templates` 中的包内 ActionPanel 展示旧版 UI 组件能力；面向 Host 0.16.0+ 的新 Panel 应按 UI 规范使用 `onActions` 接入宿主统一菜单，参考 `notes`。不要整包照搬旧交互或提高最低版本后假定 UI 已自动迁移。

## Skill 配套 Demo

仅为明确的插件开发场景保留 Demo。新插件先选输出形态，再按需组合能力；不复制无关权限、测试域名、哨兵数据或 Demo 的身份。

| Demo | 独立参考价值 | 选用时机 |
| --- | --- | --- |
| [demo-return](../../examples/public-plugins/com.uipilot.demo-return/README.md) | 最小旧式单命令结果与 `copyText` | 简单主结果插件；有道服务当前也要求旧式结果形态 |
| [demo-win](../../examples/public-plugins/com.uipilot.demo-win/README.md) | 最小 Window 生命周期及通知 | 独立窗口起步；不需要通知时去掉对应调用和权限 |
| [demo-panel](../../examples/public-plugins/com.uipilot.demo-panel/README.md) | 最小 Panel、Host Key、焦点桥 | 内嵌面板起步；当前动态菜单另参考 `notes` |
| [demo-multi](../../examples/public-plugins/com.uipilot.demo-multi/README.md) | 多命令身份、live/submit、共享设置/存储 | 一个包提供多个主结果命令 |
| [demo-settings](../../examples/public-plugins/com.uipilot.demo-settings/README.md) | 四类普通设置及兼容升级 | 插件需要可配置选项 |
| [demo-http](../../examples/public-plugins/com.uipilot.demo-http/README.md) | 宿主 HTTPS 请求及失败分支 | 外部服务集成；自动测试用假服务，不请求公共测试站点 |
| [demo-credentials](../../examples/public-plugins/com.uipilot.demo-credentials/README.md) | Bearer 引用及 secret 状态 | 需要由宿主注入凭据；示例域名只是占位符 |
| [demo-recovery](../../examples/public-plugins/com.uipilot.demo-recovery/README.md) | 重载、升级降级、未知数据版本、故障恢复 | 开发带持久化数据的插件或排查生命周期问题 |

`demo-http` 和 `demo-recovery` 同时承担验收用途，但各自提供 HTTPS 与恢复流程的独立参考，因此保留为 Skill 配套示例。修改时保留离线测试，故障变体和公共网络验收只按具体任务执行。

## 测试与验收夹具

- [auth-check](../../examples/public-plugins/com.uipilot.demo-auth-check/README.md)：真实 Host 认证链路验收，仅用公开测试值，不是业务插件或默认脚手架。
- [clipboard-history-host-fixture](../../tests/fixtures/public-plugins/clipboard-history-host-fixture/plugin.json)：宿主剪贴板桥测试，和业务 `clipboard-history` 分开。
- [CLI multi-command fixture](../../packages/plugin-cli/tests/fixtures/multi-command/plugin.json)：清单、报告和 CLI 边界测试。
- [pack fixtures](../../tests/fixtures/public-plugin-pack/README.md)：确定性归档与跨语言读取测试。
- `artifacts/diagnostics/` 的副本、`artifacts/` 和 `.superpowers/deliveries/` 的旧版本归档是历史证据或构建产物，不计为新增插件或选型入口。

## 维护规则

- 非正式插件（基座能力参考、Skill 配套 Demo、测试验收插件）的 `plugin.json.name` 统一以小写 `demo ` 开头，例如 `demo JSON 工作台`。此规则针对显示名称；保持既有插件 ID、目录和启动命令稳定。业务插件不加该前缀。
- Demo 必须在上表有具体的开发场景；新增前先判断现有示例是否已覆盖。
- 需要清理示例时，同时检查教程、SDK 文档、Node/类型测试、Rust 静态夹具、CLI smoke 和打包脚本的引用。
- 当前验证器递归发现 `examples/public-plugins/`。按用户要求重命名目录时同步脚本、测试、预览与文档路径；目录变更不自动修改 pluginId 或启动命令。
- 参考实现的测试通过只证明该层验证结果；真实 Host、原生焦点、剪贴板、外部服务及重启持久化单独记录。

## 文档覆盖核对（2026-11-03）

| 开发环节 | 入口与覆盖 |
| --- | --- |
| 需求与能力边界 | Skill：三种输出形态、最小权限、API 缺口转宿主能力请求 |
| Manifest 与兼容性 | 教程第 3 节、API v1、类型和 Schema；区分 Host / 包 / API / 私有数据版本 |
| Runtime 与动作 | 教程第 4–7 节及 Panel/Window/Main-result actions 专节，配套三个基座示例 |
| 网络、设置与凭据 | 教程 HTTPS、Bearer、普通设置、有道业务服务专节，配套三个能力 Demo |
| UI 与键盘 | UI 规范、Panel 动态操作/Delete 专节、Notes；组件使用见 Plugin UI 文档 |
| 生命周期与数据 | 教程 Host 0.7.0 恢复专节与 demo-recovery，覆盖未知版本和保存不确定性 |
| 验证、打包、安装 | 教程第 9–12、16 节；Skill 区分离线检查、静态校验与真实 Host 验收 |

此次补齐 Skill 入口、分类与能力选型，修正教程遗漏的权限、WAV 例外、剪贴板复制、快捷键边界和发布检查清单。历史设计/交付文档继续保留当时版本，不作为当前能力清单。

尚需按具体插件核对的边界：旧示例可能保留旧版 UI；`selection.read` 当前仅有翻译专用宿主入口；百度 OCR mainResult 业务方法要求 Host 0.20.0，图片 Panel action 要求 Host 0.21.0 与对应能力/授权，DeepSeek 仍只有权限声明。`com.uipilot.ocr` 是新身份，正常安装确认授权，旧 `com.uipilot.baidu-ocr` 需用户按需手动停用，历史旧包保留。SDK 的旧 `Unsupported In v1` 段落仍把 per-command hotkeys 一概排除，与已交付的用户配置逐命令启动快捷键不一致；索引区分“宿主管理启动快捷键”和“插件注册任意全局快捷键”。不能凭权限枚举、方法存在或新需求文档推断能力已交付，应同时核对最低 Host 和具体调用合同。
