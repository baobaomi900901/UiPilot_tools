# UiPilot Note Plugin

独立窗口笔记插件：搜索、新建、编辑、复制和删除笔记，数据保存在插件私有存储中。

## 选型

- `activationMode`: `submit`
- `outputMode`: `window`
- 权限: `ui.window`
- 参考实现: `com.uipilot.demo-win`

## 安装与使用

1. 在 UiPilot 的公开插件面板选择 **开发目录**。
2. 选择本目录下的 `package` 文件夹。
3. 确认 `ui.window` 权限。
4. 在主界面输入 `/note` 并回车。

### 交互说明

- 左侧（30%）：搜索框、新建按钮、笔记列表（含删除按钮）。
- 右侧（70%）：选中笔记的标题、文本编辑区、复制与保存按钮。
- 新建笔记需输入目录名；保存按钮在输入为空时禁用。
- 删除笔记会弹出二次确认。
- 切换笔记时若有未保存更改，会提示保存、不保存或取消。

## 验证与打包

```powershell
node --test examples/public-plugins/com.uipilot.note/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.note/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.note/package --platform windows
```

打包 `.uipilot-plugin`（Windows 上需使用正斜杠路径写入 ZIP）：

```powershell
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$packageRoot = (Resolve-Path 'examples/public-plugins/com.uipilot.note/package').Path
$output = Join-Path $PWD 'examples/public-plugins/com.uipilot.note/com.uipilot.note.uipilot-plugin'
$temporary = "$output.$([Guid]::NewGuid().ToString('N')).tmp"
$stream = [System.IO.File]::Open($temporary, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
  $archive = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create, $false)
  try {
    Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Sort-Object FullName | ForEach-Object {
      $relative = $_.FullName.Substring($packageRoot.Length + 1).Replace('\', '/')
      $entry = $archive.CreateEntry($relative, [System.IO.Compression.CompressionLevel]::Optimal)
      $input = $_.OpenRead()
      $outputStream = $entry.Open()
      try { $input.CopyTo($outputStream) } finally { $outputStream.Dispose(); $input.Dispose() }
    }
  } finally { $archive.Dispose() }
} finally { $stream.Dispose() }
Move-Item -LiteralPath $temporary -Destination $output -Force
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.note/com.uipilot.note.uipilot-plugin --platform windows
```

完整合同见 `docs/plugin-sdk/public-plugin-v1.md`。
