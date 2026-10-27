$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'package-runtime.ps1')

function ConvertTo-PackageMenuIndex([string]$Value, [int]$Count) {
    $index=0
    if (-not [int]::TryParse($Value,[ref]$index) -or $index -lt 0 -or $index -gt $Count) { throw 'Choose a listed number.' }
    return $index
}
function Read-PackageMenuIndex([int]$Count) {
    while ($true) {
        $answer=Read-Host '请输入编号'
        if ($null -eq $answer) { return 0 }
        try { return (ConvertTo-PackageMenuIndex $answer $Count) }
        catch { Write-Host "请输入 0 到 $Count 之间的编号。" -ForegroundColor Yellow }
    }
}
function Get-PackageMenuPlugins([string]$Root) {
    foreach ($dir in @(Get-ChildItem -LiteralPath (Join-Path $Root 'examples/public-plugins') -Directory | Sort-Object Name)) {
        $source=Join-Path $dir.FullName 'package'
        if (-not (Test-Path -LiteralPath (Join-Path $source 'plugin.json') -PathType Leaf)) { continue }
        try {
            $manifest=Get-PackageManifest $source
            [pscustomobject]@{ Name=[string]$manifest.name; Id=[string]$manifest.pluginId; Version=[string]$manifest.version; Source=$source }
        } catch { Write-Host ("跳过无效插件目录：$source — " + $_.Exception.Message) -ForegroundColor Yellow }
    }
}
function Invoke-PackageConsole {
    $root=Split-Path -Parent $PSScriptRoot
    $worker=$null; $reader=$null; $request=$null
    try {
        Write-Host "`nUiPilot 打包工具"
        Write-Host "1. 主程序 Setup 包"
        Write-Host "2. 插件"
        Write-Host "0. 退出"
        $choice=Read-PackageMenuIndex 2
        if ($choice -eq 0) { return 0 }
        $source=''; $mode='Setup'
        if ($choice -eq 2) {
            $mode='Plugin'; $plugins=@(Get-PackageMenuPlugins $root)
            if ($plugins.Count -eq 0) { throw '没有找到可打包插件（examples/public-plugins/*/package）。' }
            Write-Host "`n选择要打包的插件："
            for ($i=0; $i -lt $plugins.Count; $i++) {
                $entry=$plugins[$i]
                Write-Host ("{0}. {1} ({2}) v{3}" -f ($i+1),$entry.Name,$entry.Id,$entry.Version)
            }
            Write-Host '0. 取消'
            $selection=Read-PackageMenuIndex $plugins.Count
            if ($selection -eq 0) { return 0 }
            $source=$plugins[$selection-1].Source
        }
        $request=New-PackageRun $root $mode (Join-Path $root 'artifacts/packages/.runs') $source
        $requestPath=Join-Path $request.RunDirectory 'request.json'
        Write-PackageJson $requestPath $request
        Write-Host "`n开始打包（Ctrl+C 可取消）。同名产物将在成功后覆盖。"
        Write-Host ("日志：" + (Join-Path $request.RunDirectory 'run.log'))
        $worker=Start-PackageWorker $request $requestPath
        do {
            $finished=$worker.Process.HasExited
            $logPath=Join-Path $request.RunDirectory 'run.log'
            if ($null -eq $reader -and [IO.File]::Exists($logPath)) {
                $stream=[IO.File]::Open($logPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
                $reader=[IO.StreamReader]::new($stream,[Text.Encoding]::UTF8)
            }
            if ($null -ne $reader) {
                $chunk=$reader.ReadToEnd()
                if ($chunk.Length -gt 0) { Write-Host -NoNewline $chunk }
            }
            if (-not $finished) { Start-Sleep -Milliseconds 200 }
        } while (-not $finished)
        if (-not $worker.IsEmpty) { $worker.Cancel() }
        $result=Get-PackageCompletion $request $worker.Process.ExitCode $false
        if ($result.State -ne 'Success') { throw $result.Message }
        $artifact=Publish-PackageArtifact $request $result
        Write-Host "`n打包成功：$artifact" -ForegroundColor Green
        Write-Host ("SHA256：" + $result.Sha256)
        return 0
    } catch {
        Write-Host ("`n打包失败：" + $_.Exception.Message) -ForegroundColor Red
        if ($null -ne $request) { Write-Host ("日志目录：" + $request.RunDirectory) }
        return 1
    } finally {
        if ($null -ne $reader) { $reader.Dispose() }
        if ($null -ne $worker) { $worker.Dispose() }
    }
}
if ($MyInvocation.InvocationName -ne '.') { exit (Invoke-PackageConsole) }
