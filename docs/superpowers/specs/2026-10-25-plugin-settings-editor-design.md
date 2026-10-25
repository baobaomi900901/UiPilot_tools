# 普通插件设置编辑设计

## 目标与范围

在插件详情中编辑现有 Manifest 定义的 text、number、boolean、select 设置，显式保存后下一次命令读取新值，并在重启及兼容升级后保留。用户已授权继续普通设置编辑开发和自主安排 agent；功能以用户执行的人工用例验收。本轮不改变公开 API 或存储合同，不实现删除覆盖值、迁移、在线功能或新凭据能力。

方案选择：使用一个包级普通设置表单、显式保存和取消修改。逐字段自动保存会在输入过程中频繁触发已有撤权/请求退休逻辑；插件自带设置面板无法覆盖第三方普通字段。独立编辑组件接入现有详情并沿用 savePublicPluginSettings 是本轮方案。

## 数据与校验

以已接受 inventory 的 settings 为基线（有value则使用value，否则使用definition.default；不能从inventory推断保存值的来源），按声明顺序渲染，secret 不进入此组件或请求普通 settings。text 使用文本输入（保留空白，允许空字符串，禁止NUL和除CR/LF/TAB外控制字符，遵循Rust plain_text）；number 接受有限数字且遵循min/max，step仅为输入提示而非新的整除约束；boolean 显示开关；select 仅可提交声明的选项值，合法空字符串选项必须与未设置状态区分。无value无default为未设置，不能仅因打开/保存其他字段而写入空字符串、false或零；应标注未设置，未编辑字段不提交。已编辑数字为空、非有限或越界须显示字段错误并阻止请求，不能把空值转为0。无null/reset协议，不提供“删除设置”或假恢复默认。

保存调用现有 client.savePublicPluginSettings({ input: { pluginId, settings: changedValues, secrets: {} } })，仅传变化字段；无变化禁用保存。取消修改只丢弃草稿、恢复最新已接受基线，不写IPC。有效字段值是string/number/boolean；使用安全字典构造，合法特殊key（如constructor）不能变更原型或丢失。

## 交互与异步所有权

普通设置独立分区，标签/字段错误/保存状态可访问，采用现有Ant Design控件和语义token。延续详情单滚动容器与no-drag，控件不侵入标题栏。只有enabled且无fault的插件可保存；停用时只读并提示先启用。已有secret/network/开发重载操作与普通保存互相禁用，避免详情内并发写入。命令改名/收藏的已存在流程不扩展为新全局事务。

一次保存只能提交一次，pending使用同步门防重复。提交期间禁用草稿编辑/取消；对话框关闭、详情卸载/换插件后的旧完成不得刷新或写入新详情、抢焦点。复用现有detail session/reload(beforePublish)和命令行的所有权处理经验。无关inventory revision不应丢弃草稿；定义/当前普通值真正改变时，应明确提示状态已变化并要求重新核对，不静默提交过期草稿。现有后端没有expectedRevision/CAS，本轮不承诺跨窗口原子比较，后端当前类型校验与串行化合同仍有效。

保存成功后刷新已接受inventory，再显示已保存并重置基线。IPC失败保留草稿并尝试刷新，提示保存未确认、落盘值可能已改变，不能声称未写入或回滚；刷新失败时提供明确的重新读取入口并阻止盲目重复提交。重新读取后用户显式重新编辑/保存；不得自动重试。成功写入但刷新失败须区别显示“已保存，无法刷新当前状态”。若实际值变化与失败草稿冲突，保留草稿供核对并要求显式取消/重新编辑，不直接覆盖实际值。

## 可运行示例与版本

Host与CLI校验target同步到0.8.0；保留既有API最低版本语义，避免全仓替换历史文档。独立示例 com.uipilot.demo-settings，命令 /settings-demo（submit/mainResult），无权限、网络、凭据或私有存储写入。参考demo-return/multi的结果形态，onCommand每次通过api.settings.get读取四项并在title/subtitle直接展示便于肉眼核对：greeting文本默认你好；count数字默认2/min1/max5/step1；uppercase布尔默认false；style选项plain/boxed默认plain。text输出不执行HTML。准备1.0.0与1.0.1同定义兼容升级包，升级版改变展示构建标记，不改变用户值。

## 验证与交付

聚焦验证类型和值到IPC的边界（含未设置/空数字/secret排除/特殊key），保存失败及成功后刷新失败，不同详情旧完成失效、重复提交防护、定义变化处理和普通保存与恢复操作互斥。不创建机械布局或大量mock矩阵。示例用小型Runtime单测验证读取当前设置而非缓存默认、没有副作用。npm build、必要版本一致性及CLI validate/pack验证；原生最终构建且记录源码/资源/包散列，不运行原生程序。

人工验收：安装示例看到默认值；四类型编辑保存命令立即反映；未保存取消无变化；非法数字被阻止；完整退出重启后保留；1.0.0→1.0.1升级后四值保留；双主题/键盘与旧Notes/recovery简单回归。交付保持acceptance数据标识与旧交付目录，列出完整路径。通过源码/自动检查不等于人工通过。