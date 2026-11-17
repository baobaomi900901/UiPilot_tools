# macOS 应用图标兼容修复

用户报告主界面部分应用没有图标，截图中的 Tabby 显示宿主占位标记。本机只读检查确认 Tabby 的 `icon.icns` 只有一张 1024 × 1024 JPEG 2000 表示；旧读取器仅直接提取不超过 128 × 128 的内嵌 PNG，因此无法读取它。

修复使用系统 ImageIO 解码并归一化为不超过 64 × 64 的 PNG，覆盖 JPEG 2000 ICNS、大尺寸 PNG ICNS 和直接 PNG。源文件仍使用原有有界/no-follow 读取；文件名按声明优先读取，再尝试省略的 `.icns` 后缀，按真实内容解码，兼容 Ghelper 等扩展名为 `.ico` 但内容实际为 ICNS 的应用。没有 `CFBundleIconFile` 而声明 `CFBundleIconName` 的资源目录应用，通过 NSWorkspace 获取图标，再仅转换一个所需的 CGImage 表示，避免序列化整个多分辨率/多外观 NSImage。

保留 8 MiB 输入及 49,134 字节 PNG 输出限制，增加源尺寸、表示数量与 ICNS chunk 完整性校验，保留 PNG 透明度和纵横比。损坏、不支持或超限的图标仍安全回退。只修改 Mac 应用图标后端及所需 AppKit feature，不改变应用执行权限、Windows 路径、插件能力或界面布局。

自动及只读验证：

- 合成 JPEG 2000 ICNS 与大 PNG 用例在旧实现下失败，修复后通过。测试图标完全由合成像素生成，不复制第三方应用素材。
- 应用发现/排名/主程序应用命令相关：60 passed、3 既有/明确忽略的设备用例；包括非法尺寸、坏 chunk、不支持内容、透明度、路径穿越/符号链接、扩展名省略及不匹配。
- 显式执行本机只读设备用例：Tabby、系统设置均成功得到有界 PNG。没有启动这些应用或操作其窗口。
- 最终本机目录扫描发现 151 个去重有效应用，151 个均有图标；此数字仅描述当前设备扫描，不表示所有外部应用均已实测。
- 默认 all-targets 检查、TypeScript/Vite 和 arm64 release App 构建通过；实际 DMG 只读挂载检查通过，签名、书签 entitlement、资源、架构、版本及最低系统正确，ZIP CRC 与包内清单哈希通过。

全屏置顶修复继续包含在候选内。未合并 main，截图开发仍等待 mac_acc；没有 GUI 自动化、权限变更或自动替换已安装应用。需要用户替换重启后，仅确认搜索 Tabby 时图标实际显示，不重复已通过的全屏及插件测试。

候选源码：`859705a3d00a8f7a9bfb4b3112e28ec7ff660fe6`。交付 `/Users/mobytang/Documents/UiPilot-Mac-application-icons-859705a.zip`，内含 DMG、安装说明、源码提交和校验清单；内部 ad-hoc 签名，未公证。GUI 显示仍待用户确认，本机图标解码成功不代替界面验收。

| 制品 | SHA-256 |
| --- | --- |
| ZIP | `ab98727b01cbe90bb4c27f52be8e0287f0d52e1720dcfa99a2c52438a08559ab` |
| DMG | `66a9c5974f2ce4fce12aede5e41ef9eb128939b7d29e1b7b5f821c2354250100` |
| 主程序 | `e462751f53e398e21c259f0ec6fd850d4a0af7dfbb566c1857a7a9dbf31a01da` |
