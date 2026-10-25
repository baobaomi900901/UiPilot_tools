# 普通设置离线示例

安装目录是 `package/`；命令 `/settings-demo` 每次执行都读取四项当前设置，标题展示问候文本和数量，副标题展示 uppercase、style 和构建标记。没有权限、凭据、网络、剪贴板动作或私有存储读写。输出是纯文本，HTML 不会执行。

在 Host 0.8.0 的设置 → 插件 → 详情 → 普通设置编辑，然后显式“保存设置”。默认值为“你好”、2、false、plain。试填“人工验收”、3、true、boxed，运行命令应看到 `【人工验收 × 3】` 和 `uppercase=true · style=boxed · build 1.0.0`。“取消修改”只丢弃草稿。数量允许 1–5 的有限数字，step 只是输入提示。保存会使旧请求失效；重新执行命令查看结果。

此示例保留 API v1 与 minimumHostVersion 0.2.0 的读取语义；普通设置编辑入口需要 Host 0.8.0。关闭并重启 Host 后再执行命令，值应保持。

从仓库根目录执行：

```powershell
node --test examples/public-plugins/com.uipilot.demo-settings/tests/runtime.test.js
npm run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.demo-settings/package --platform windows
node packages/plugin-cli/dist/cli.mjs pack examples/public-plugins/com.uipilot.demo-settings/package --output .superpowers/sdd/local-foundation/settings-editor-packages/com.uipilot.demo-settings-1.0.0.uipilot-plugin --platform windows
node examples/public-plugins/com.uipilot.demo-settings/prepare-upgrade.mjs .superpowers/sdd/local-foundation/settings-editor-upgrade
node packages/plugin-cli/dist/cli.mjs validate .superpowers/sdd/local-foundation/settings-editor-upgrade --platform windows
node packages/plugin-cli/dist/cli.mjs pack .superpowers/sdd/local-foundation/settings-editor-upgrade --output .superpowers/sdd/local-foundation/settings-editor-packages/com.uipilot.demo-settings-1.0.1.uipilot-plugin --platform windows
```

生成脚本要求空输出目录（已有文件拒绝覆盖）。1.0.1 只修改版本与展示构建标记；设置定义、键和默认值完全相同。安装兼容升级包后，再执行命令应保留四值并显示 build 1.0.1。脚本只生成可检查的文件，不安装插件。
