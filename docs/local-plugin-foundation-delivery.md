# 本地插件基座最终交付

日期：2026-10-27。状态：本轮约定的 Windows 本地插件基座核心范围已完成；最后三项组合验收由用户明确确认“全部符合预期”。这里的完成指既定 M0–M7 核心交付及已下发验收，不表示完整复制 Raycast 的全部产品能力。

## 使用当前版本

继续使用 .superpowers/deliveries/uipilot-0.14.0-acceptance/uipilot.exe，版本0.14.0。现有安装、设置和数据无需重装。该目录中的附属资源应与程序一起保留；不要仅迁移exe。

宿主构建源码：77ab4f36b05325d2b03340c6bdd6fa75907f9b8c。
宿主SHA256：d6eb14f98cb8ea2bbde4049e82694a7b4756fb7bb2ef0b9a22083772acfc5973。
构建标识：com.uipilot.launcher.acceptance。
源码保留在本地分支 codex/local-plugin-foundation；本次收口没有重建程序、运行真实Host、合并主分支或推送远程。

## 已交付能力

| 范围 | 核心证据 |
| --- | --- |
| 本地开发和交付 | SDK/合同、参考插件、目录和归档校验、确定性pack、本地文件安装 |
| 命令和动作 | 多命令身份、收藏/改名、统一动作、逐命令全局快捷键及双向冲突处理 |
| 插件界面 | Panel与独立Window调用Runtime；List/Detail/Form/ActionPanel与主题、焦点、取消交互 |
| 设置和凭据 | 普通配置编辑、DPAPI凭据保存、Host绑定域名注入；实际HTTPS正确值200、错误值401及恢复200 |
| 授权系统能力 | 浏览器链接、用户选定UTF8文本导入、显式粘贴到原应用 |
| 生命周期与恢复 | 升级保留、开发重载、失败保留旧版、停用恢复、数据版本保护、卸载保留/清除与重装 |
| 诊断与组合 | 脱敏诊断；最终0.14.0重启保留、Panel/Window/主结果切换、快捷键和插件停用隔离通过 |

详细源码、自动验证、独立审查和分组用户反馈见[路线图](superpowers/specs/2026-10-23-local-plugin-foundation-design.md)。自动检查是各交付切片的聚焦/构建记录；没有将本轮文档整理表述为当前HEAD全仓重跑。

## 安装包索引

下列路径均相对项目根目录 .superpowers/deliveries/，已经核对存在、大小和SHA256。版本相同的历史重复包只列一个。已安装的无需重复安装。

| 插件 | 版本 | 包路径 |
| --- | --- | --- |
| Notes | 1.2.5 | uipilot-0.5.0-acceptance/plugins/com.uipilot.notes-1.2.5.uipilot-plugin |
| 多命令演示 | 1.0.1 | uipilot-0.5.0-acceptance/plugins/com.uipilot.demo-multi-1.0.1.uipilot-plugin |
| 普通设置演示 | 1.0.1 | uipilot-0.8.0-acceptance/plugins/com.uipilot.demo-settings-1.0.1.uipilot-plugin |
| 番茄钟 | 1.2.0 | uipilot-0.9.0-acceptance/plugins/com.uipilot.pomodoro-1.2.0.uipilot-plugin |
| JSON工作台 | 1.1.1 | uipilot-0.13.0-acceptance/plugins/com.uipilot.json-workbench-1.1.1.uipilot-plugin |
| 文本模板 | 1.2.0 | templates-1.2.0-acceptance/plugins/com.uipilot.templates-1.2.0.uipilot-plugin |
| 文本片段 | 1.2.0 | uipilot-0.14.0-acceptance/plugins/com.uipilot.text-snippet-1.2.0.uipilot-plugin |
| 凭据演示 | 1.0.0 | credentials-1.0.0-acceptance/plugins/com.uipilot.demo-credentials-1.0.0.uipilot-plugin |
| 认证链路验收 | 1.0.0 | auth-check-1.0.0-acceptance/plugins/com.uipilot.auth-check-1.0.0.uipilot-plugin |

本次最终清单：[final-inventory.json](../.superpowers/deliveries/uipilot-0.14.0-acceptance/final-inventory.json)。宿主及5项附属资源与原构建证据一致。各目录原始构建证据保留构建时状态，不覆盖为事后人工结果。

## 最终人工结论

认证两组：未配置状态、正确/错误/恢复值、清除凭据拒绝、网络关闭拒绝、恢复授权成功，均由用户确认。
最终组合三项：退出重启后原笔记/模板/片段/凭据配置保留；模板预览→JSON格式化→主结果切换无残留，Notes快捷键一次进入；停用认证插件不影响Notes/templates，重新启用仍保留凭据状态。用户均确认通过，无本轮已报告待修问题。

## 已知限制与后续边界

- 普通Enter需要多次确认的问题按用户明确要求搁置，未宣称修复。
- 当前交付为已人工验收的Windows x64程序目录；安装器/干净VM发行矩阵、签名及正式发布另行安排。
- 自动权限/会话边界验证与已下发人工用例已记录；全部原生竞态、全部输入法/屏幕尺度和故障注入排列未人工穷举。
- 无在线插件安装、商店/发布、云同步、OAuth、任意签名、任意Shell或无限文件访问。本轮范围本就排除这些能力。
- auth-check依赖公开测试服务可用性，仅使用公开合成值；不应填真实业务密钥。开发恢复夹具与认证演示用于验收，不要求日常启用。
- 历史构建的Vite体积及部分测试辅助方法提示不影响本轮已确认流程，未以关闭检查掩盖。
- 交付包和原始证据位于忽略目录，只存在于本机工作树；保留此目录。源码提交不能替代这些本地产物和用户数据的备份。

下一阶段可以开始独立业务插件开发或产品体验改进；本轮基座不再新增必测项。