# WKWebView 媒体暂停独立探针

仅供 UiPilot 约 70% 统一人工节点使用。**本轮只构建，不运行。** 此 app 不运行插件，不是 GuardProof，UiPilot 生产插件保持关闭。

## 构建

需要 macOS、Xcode Command Line Tools 和本机 SDK；目标仅 arm64，最低系统 15.7.4。

```sh
./spikes/macos-media-probe/build.sh
```

默认输出到新的 `/private/tmp/uipilot-media-probe.XXXXXX`，也可传入一个尚不存在的绝对目录。禁止覆写既有输出。构建执行纯 C 测试、Objective-C 编译/link、ad-hoc 签名和校验，生成 `.app`、zip、SHA-256、Mach-O/版本信息与源码哈希；**不启动 app**。`source-base-commit.txt` 为构建时仓库 HEAD，源码哈希另记录尚未提交的变更；最终归档须记录实际提交。

## 人工清单（以后统一执行）

先确认允许听到短促测试音；不要为此修改系统音量或系统权限。解压并自行打开 app，默认只显示说明、无 WKWebView/媒体播放。系统 Light/Dark、窗口缩放、键盘焦点和日志选择均需人工复核。

1. 保持“额外测试语音”关闭。点击 **播放短音基线**（⌘1）。低音量 HTML data WAV 和 WebAudio oscillator 尝试播放；JS 约 1.7 秒清理，原生自加载准入起 2 秒截止并请求暂停、移除 handler 和销毁视图。只有相应通道的时间推进才说明该通道基线有效；任一基线失败不能把后续不播放当作隔离通过。
2. 点击 **先暂停再加载**（⌘2）。确认暂停 completion 前没有 `fixtureStarted`，之后记录原生四态和脚本观测。不观察到推进只代表本次观测；若听到声音或时间推进，记录异常。完成回调不是静音证明。
3. 点击 **暂停 → reload → 新导航**（⌘3）。自动运行三个 2 秒阶段，始终同一 WKWebView，不调用恢复。第二阶段调用真实 `reloadFromOrigin`，第三阶段使用新固定 URL。各阶段必须有对应 generation 的报告；若缓存导致旧报告被拒绝，则记录缺报告/不确定，不能算成功。
4. 点击 **迟到回调 / 替换实例**（⌘4）。先收到旧 completion，但延迟消费；退役并替换旧实例，500ms 后处理旧回调。应记录旧回调拒绝，不能加载旧页面或撤销新实例。
5. 在准备/加载期间用 **取消**（⌘.）、新案例或关窗打断。记录没有旧页面污染新案例；停止先退役 token 再原生清理。停止动作不等待 JS 成功。
6. 只有明确愿意听到固定词“测试”时才勾选语音，分别重复基线与暂停案例。SpeechSynthesis 是独立未证实路径；记录 start/end/error 和是否听见，不以 HTML/WebAudio 结果推断语音隔离。未勾选、缺 start 或基线失败均未验证。
7. 用鼠标选择日志，手动 ⌘C 复制；app 不自动访问剪贴板、不自动保存日志。记录系统版本、源码哈希、逐通道基线和所有不确定项。

## 安全与解释边界

只使用非持久 WKWebsiteDataStore、内存 WAV、合成 oscillator 和固定语音。页面 CSP 拒绝网络、frame、表单；原生导航只准入本次固定 scheme URL 的主 frame 一次，外部 URL、popup 和 download 全拒绝。不会读取用户页面、设备名、文件索引、音频输入或 Keychain，不申请麦克风权限。

页面消息需要原生 WKWebView 实例、generation、mainFrame 和 nonce 同时匹配，且仅记录固定字段。未知消息/系统错误正文不输出。原生请求只在固定阶段读取活动状态，不做轮询修复，不覆盖 WebAudio API，不调用 private SPI。原生定时器以主线程调度为准，不承诺操作系统卡顿期间精确实时截止；语音取消和 WebView 销毁是额外清理，不能当作已证明该路径被原生暂停覆盖。

None/Paused 是活动状态，不是隔离开关。没有公开的意外恢复通知；空白页读回也不能证明静音。普通 reload 不等于 WebContent 进程更换；后者本探针未强制触发，始终未验证。探针不会输出“全部通过”或“已安全开放插件”。

纯 C 测试覆盖真实共用准入核心：准备前禁止加载、重复 completion、重复 load、取消/超时等价退役、实例替换、同实例新代际、mainFrame/nonce、计数耗尽。它不验证 AppKit/WebKit 行为；编译签名检查也不是实机播放结论。
