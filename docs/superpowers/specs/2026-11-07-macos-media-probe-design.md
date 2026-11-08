# macOS WKWebView 媒体暂停独立探针

为约 70% 的统一人工节点提供一个可重复的原生测试 app，观察 WKWebView 的媒体暂停及异步生命周期行为。本探针不运行插件包，不授予 production GuardProof，不打开 UiPilot 插件能力，不以实验观测替代现有原生 readback/即时撤权合同。

## 已知边界

Apple 的 [setAllMediaPlaybackSuspended](https://developer.apple.com/documentation/webkit/wkwebview/setallmediaplaybacksuspended(_:completionhandler:)?language=objc)承诺暂停期间页面和用户不能恢复媒体，直到宿主恢复。上游 [WebPageProxy](https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebKit/UIProcess/WebPageProxy.cpp)将挂起状态传入新页面进程；这不是目标 macOS 15.7.4 已实测的保证。`requestMediaPlaybackState` 是活动状态，空白页 None 或 Paused 都不能作为隔离开关读回。未找到公开的挂起 getter 或意外恢复通知，探针不使用 private SPI、轮询修复或 JS API 覆盖填补缺口。

[SpeechSynthesis](https://raw.githubusercontent.com/WebKit/WebKit/main/Source/WebCore/Modules/speech/SpeechSynthesis.cpp)采用另一路径，其是否受暂停覆盖尚无充分证据。必须单列为未验证；HTMLMedia/WebAudio 不推进也不能证明所有发声途径都隔离。

## 独立运行与用户操作

新增 `spikes/macos-media-probe/` 中的 Objective-C/AppKit 测试程序、固定本地 fixture、纯生命周期核心与构建脚本。仅 arm64、部署下限 15.7.4、独立 bundle ID、临时非持久 WKWebsiteDataStore。原生窗口使用系统 Light/Dark 和标准紧凑控件，中文说明与日志，可键盘操作。没有网络、文件索引、剪贴板、Keychain 或系统权限修改；不改系统音量。

默认启动只显示说明，不创建会执行媒体代码的 WebView。全部测试由用户明确点击：

1. 正常播放基线：明确标注会尝试播放短促低音量合成音（最多 2 秒），用于验证 fixture 自身能推进。
2. 先暂停后加载：创建没有包脚本的 inert WebView，调用原生暂停；仅 completion 到达且原实例/代际仍当前时，加载固定媒体 fixture。
3. 暂停后的重新加载/新导航：在同一已暂停实例加载新的固定 fixture，检查页面时间与 WebAudio 状态；不重新调用 false，不把成功完成加载当作静音确认。
4. 迟到回调与同窗口实例替换：人为延迟第一次完成的消费，先退役旧实例并创建新实例，再释放旧回调；旧回调不得加载任何 fixture、更新新实例结论或撤销新实例。

语音合成另有明确 opt-in 选项，默认关闭；开启后各被选择的测试会尝试说固定测试词，并记录 start/end/error。说明可能听到测试语音，结束/取消时调用 fixture 的 speechSynthesis.cancel。没有用户点击就不播放合成音或语音。语音未选或基线未成功时，报告该项未验证。

取消、超时和关窗均停止加载、退役当前 generation 并移除 handler/视图。测试音不循环，页面内固定时限停止节点，原生另有兜底超时。原生或 JS 错误仅记录测试阶段与固定错误类别，不输出系统对象正文。

## 归属与记录

使用共享到纯 C 测试的生命周期状态机控制 generation、等待暂停、准入和退役。原生回调同时绑定 WKWebView 实例与 generation；不能只按窗口标签认领。页面消息绑定当前原生 webview 和固定本次 nonce，旧页面消息不修改新案例。取消/新实例/重复 completion/计数溢出分别拒绝。native suspension 完成前 fixture 执行数必须为零；完成不能被记录为原生 mute proof。

fixture 只用 CSP 明确允许的内联固定脚本、内存 data WAV、合成 oscillator；拒绝网络/导航到任意来源、新窗口和下载。固定采样报告 HTML media 的 paused/currentTime、WebAudio state/currentTime、可选 speech start/end/error；不能收集设备名、个人页面、路径或音频输入。没有麦克风请求。

原生 `requestMediaPlaybackState` 按 None/Playing/Paused/Suspended 原值记录。若被暂停案例仍出现持续时间推进/发声事件，报告可疑播放推进；若没有推进，只报告“本次未观察到推进”，并明确“不等于已证明全部音频隔离”。基线未推进、超时、缺报告均为不确定，不显示通过。跨 WebContent 进程恢复未被强制触发时必须保留未验证，不能用普通 reload 代替。

## 验证和交付

默认自动检查只编译/link、签名和检查 Mach-O/Info.plist，以及运行纯生命周期核心的确定性测试；不启动 AppKit/WKWebView，不播放音频。源构建禁止覆写已有输出，交付独立 `.app`/zip、源码提交与 SHA-256，附固定顺序人工清单。

先执行正常基线，再执行暂停/新导航/迟到回调。记录每次 case、generation、fixture 是否获准加载、原生原值及脚本观测；无论结果怎样，UiPilot 生产插件保持关闭，待原生合同缺口单独解决。本次不是插件双端实测通过，不能增加其计数。

## 2026-11-08 实机结论：暂停路径未通过

M1 Pro / macOS 15.7.4，探针 b279271：HTMLMedia 与 WebAudio 基线均推进并有短音；“先暂停再加载” completion 已到，HTML play 被拒绝且时钟不动，WebAudio resume 后 running 至约 1.698 秒，用户听到声音；原生活动状态仍为 Suspended。详见设备验收单同日原始关键日志。

这项实测反证了当前暂停路径可作为所有音频出口隔离的假设，不能用 completion 或活动状态签发 GuardProof。生产插件保持关闭。其余 reload、替换、语音、WebContent 进程更换未验，不继续要求用户重复发声来将失败“刷过”。后续方案须提供可验证的原生全音频禁用/隔离，或采用不具备网页媒体能力的独立 runtime 并审查兼容性；页面脚本覆写、CSP、自动播放手势限制及轮询暂停均不能作为当前严格合同的替代。没有在本轮擅自更改插件合同或开放运行。

SDK WKWebView.h 与 [Apple 暂停接口](https://developer.apple.com/documentation/webkit/wkwebview/setallmediaplaybacksuspended(_:completionhandler:))是 API 参照，设备实测决定当前候选能否通过；本轮未找到足以证明此失败已经修复的公开原生替代接口。
