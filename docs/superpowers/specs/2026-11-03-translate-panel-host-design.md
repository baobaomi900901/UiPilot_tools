# 翻译 Panel 宿主扩展实施合同

日期：2026-11-03。用户已批准产品计划并明确授权宿主、SDK 与插件整套迁移。本合同将能力请求落实为实施接口，范围限 Windows，最低 Host 0.18.0；保留旧 mainResult 翻译兼容。

## H1 服务与检测
- PluginYoudaoTranslationInput: text string, from 'auto'|'en'|'zh-CHS', to 'en'|'zh-CHS'。响应追加 sourceLanguage:'en'|'zh-CHS'|null、targetLanguage:'en'|'zh-CHS'、unchanged:boolean，保留 translations:string[]。
- 固定 SDK 支持语言清单为英文和简体中文；源语言另支持 auto。插件只用此清单，不调用未经验证的其他供应商语言。
- 有道请求 strict=true，解析响应 l 字段（EN2zh-CHS / zh-CHS2en，英文大小写规范化），未识别源语言返回 null，禁止用文本正则猜测。实际目标语言与请求不符应报受控响应错误。自动检测得到其他语言时源语言显示未识别，不能冒充英语。
- 明确选择同语种时返回原文、unchanged:true；完成调用者、当前请求、网络及服务授权、凭据启用验证后本地返回，不发供应商请求。不能用同语种绕过撤权与失效上下文。
- 有效 legacy mainResult 保持原有允许范围；新允许 legacy submit+panel 且 panel.runtimeActions、已声明/授权 ui.panel 的当前 Panel Runtime 操作。仍拒绝多命令、window、resultActions 组合。扩展能力要求 minimumHostVersion>=0.18.0，旧 mainResult 输入/输出兼容。
- 保留请求调度、网络 admission、凭据 revision、完成前复核及最终结果发布保护；审计 PanelAction 的请求所有者与最终发布路径，不能只放宽 manifest 判断。
- SDK 服务存在不等于授权。输入 8192 UTF-8 字节、非空、无 NUL；不允许额外字段、服务 URL 或凭据参数。

## H2 Panel 复制
- 新增可选 readonly panel.clipboard facade，仅声明 clipboard.write 的 0.18.0+ 合法 Panel 暴露；每次调用仍复核当前授权。
- clipboard.writeText({text:string}):Promise<void>。文本必须非空、无 NUL、最多16384 UTF-8字节；空白文本允许复制以保持用户原文。
- 仅当前已激活内容 WebView 与绑定会话调用；先确认插件 generation/activation/admission/授权，再在会话核对保护范围内写入系统剪贴板。
- 不读原剪贴板、不模拟输入、不隐藏、不夺焦点，不自动重试；OS 写成功后才 resolve。过期旧方法无法借用新会话。
- 固定错误名沿用明确权限/过期会话语义，新增输入和复制失败错误在 SDK 公布，JS 不向用户暴露 native 错误文本。

## H3 Panel 朗读
- 新权限 speech.speak：Windows、legacy submit+panel、ui.panel、minimumHostVersion>=0.18.0；新授权默认关闭，安装/更新/权限页使用既有完整同意流程。
- 可选 readonly panel.speech：voices():Promise<readonly SpeechVoice[]>；play({text,language}):Promise<SpeechState>；stop({playbackId}):Promise<SpeechState>；state():Promise<SpeechState>；onState(handler):unsubscribe。
- SpeechVoice={id:string,name:string,language:'en'|'zh-CHS'}；本版系统自动选择匹配语言声音，不提供用户 voiceId 覆盖。
- SpeechState={playbackId:string|null,status:'idle'|'playing'|'completed'|'stopped'|'error',language:'en'|'zh-CHS'|null,errorCode?:string}。playbackId 由宿主生成，旧 ID stop 不得停止新播放；状态事件绑定原 Panel 会话，不能将别的插件文本或状态广播进来。
- 文本非空、无 NUL、最多16384 UTF-8字节；语言精确白名单。voices 只返回可用本地英语/简体中文声音；缺少匹配声音返回可解释固定错误，不自动安装、联网或用别的语言替代。
- 单一原生 worker 持有 SAPI/COM 对象；同一时刻全局只有一个播放，新播放停止旧播放；调用参数使用纯文本标志，禁止 XML/SSML、文件名或任意音频 URL。
- 插件隐藏/替换会话、禁用/重载/卸载、相关权限撤销均停止对应播放；原生 worker 在执行排队操作前也复核所有者有效性，避免撤权后排队 play 又启动。
- 所有者失效须先撤销再排队停止，旧完成事件不能改变新播放。不能持有 plugin/session 锁等待原生 worker，避免锁反转和 UI 阻塞。
- WebView 静音与媒体 CSP 保持不变。只通过明确用户按钮/菜单动作朗读，插件本身不自动播放。
- 固定错误类型、DTO 校验、IPC allowlist、桥接快照与调用者标签隔离纳入实现及测试。

## H4 划词、版本和发布
- selection.read 入口对原有合法 mainResult 和新合法 Panel 兼容，Panel 要求 ui.panel 与 runtimeActions；用户主动取词后初始化 Panel 并翻译一次。
- 保持当前捕获及剪贴板恢复策略，失败不翻译旧内容；更新/撤权/替换会话后旧捕获结果无效。
- 版本发布为 Host0.18.0，SDK apiVersion/schemaVersion 仍为1（加法扩展）；更新实际版本源、CLI target、SDK/schema/文档及测试，禁止只改插件最低版本。
- 仅做专项相关修改，保留 ID、默认启动键、授权记录与用户绑定。运行自动化使用假供应商/复制/语音，不接触真实凭据。

## 验证与来源
复用能力请求文档的验收顺序，并特别测试：A翻译开始→撤权/换密钥→A返回；A播放排队→会话销毁→worker处理；A播放→B播放→A迟到完成/stop；复制拿到旧facade→新会话；取词捕获→用户输入替换→捕获返回。预期旧工作均不能影响新所有者。

- 有道文本翻译官方文档：https://ai.youdao.com/DOCSIRMA/html/trans/api/wbfy/index.html （2026-11-03核对：auto、strict、l字段；不使用返回音频URL）
- Microsoft ISpVoice::Speak：https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms719820(v=vs.85)
- Microsoft SPEAKFLAGS：https://learn.microsoft.com/en-us/previous-versions/office/developer/speech-technologies/jj127460(v=msdn.10)

实施顺序：H1/H4服务及划词→专项评审→H2/H3复制朗读和版本交付→专项评审→插件状态/Runtime/UI/能力接入→集成评审。实施代理可按此拆分能力请求Task1；插件包形态最后切换。
