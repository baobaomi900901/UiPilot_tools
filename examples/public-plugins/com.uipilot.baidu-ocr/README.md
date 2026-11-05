# 百度 OCR

使用主程序中已配置的百度 OCR 识别粘贴的单张图片，返回完整文字并支持回车复制。

- 插件：`com.uipilot.baidu-ocr`，版本 `1.0.0`。
- 要求：Windows、UiPilot `0.20.0+`。
- 命令：`baidu-ocr`；`submit + mainResult`，声明 `image.ocr` 输入能力。
- 权限：`credentials.baiduOcr`、`network.https`、`clipboard.write`；固定网络主机 `aip.baidubce.com`。

## 使用

在主程序设置中配置并启用百度 OCR，授予插件上述权限。主输入框粘贴截图后，选择列表中的“百度 OCR”并回车；出现识别文字后，再按一次回车复制全文。

使用百度通用文字识别标准版。粘贴图片不调用服务，执行识别时才发送图片；插件不能读取 API Key、Secret Key 或访问令牌。失败不自动重试，无文字时不修改剪贴板。

单次只处理一张图。图片边界由宿主校验；详情上限 16 KiB、整个结果上限 64 KiB，超过时提示分区域识别，不复制被截断的内容。

## 本地验证

从仓库根目录运行：

```powershell
node --test examples/public-plugins/com.uipilot.baidu-ocr/tests/runtime.test.js
node node_modules/typescript/bin/tsc --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.baidu-ocr/tests/sdk-contract.ts
npm.cmd run build --workspace @uipilot/plugin-cli
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.baidu-ocr/package --platform windows
```

自动测试使用假服务，不读取系统剪贴板或真实凭据。原生粘贴、百度识别及实际复制需要在支持本能力的 Host 中另外验收。
