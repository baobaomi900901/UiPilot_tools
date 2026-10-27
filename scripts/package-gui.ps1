$ErrorActionPreference='Stop'
try {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    [Windows.Forms.Application]::EnableVisualStyles()
    . (Join-Path $PSScriptRoot 'package-runtime.ps1')
    if ([Threading.Thread]::CurrentThread.ApartmentState -ne 'STA') { throw 'Use Pack.cmd to start this window in STA mode.' }
    $root=Split-Path -Parent $PSScriptRoot
    $config=Read-PackageJson (Join-Path $root 'src-tauri/tauri.conf.json')
    $script:state=@{ Worker=$null; Request=$null; Cancelled=$false; CloseAfterCancel=$false; Reader=$null; Watch=[Diagnostics.Stopwatch]::new(); LastOutput=(Join-Path $root 'artifacts/packages') }
    $form=New-Object Windows.Forms.Form
    $form.Text='UiPilot 打包工具'; $form.Size=New-Object Drawing.Size(860,680); $form.MinimumSize=New-Object Drawing.Size(720,560)
    $form.StartPosition='CenterScreen'; $form.Font=[Drawing.SystemFonts]::MessageBoxFont
    $layout=New-Object Windows.Forms.TableLayoutPanel
    $layout.Dock='Fill'; $layout.Padding=New-Object Windows.Forms.Padding(12); $layout.ColumnCount=1; $layout.RowCount=8
    foreach ($height in @(52,158,40,38,36,0,30,26)) {
        $style=New-Object Windows.Forms.RowStyle
        if ($height -eq 0) { $style.SizeType='Percent'; $style.Height=100 } else { $style.SizeType='Absolute'; $style.Height=$height }
        $layout.RowStyles.Add($style) | Out-Null
    }
    $form.Controls.Add($layout)
    function New-Label([string]$Text) { $item=New-Object Windows.Forms.Label; $item.Text=$Text; $item.AutoSize=$true; $item.Dock='Fill'; return $item }
    $repository=New-Object Windows.Forms.TextBox
    $repository.Multiline=$true; $repository.ReadOnly=$true; $repository.Dock='Fill'; $repository.Text="仓库：$root`r`n应用版本：$($config.version)"; $repository.AccessibleName='仓库路径及应用版本'
    $layout.Controls.Add($repository,0,0)
    $tabs=New-Object Windows.Forms.TabControl; $tabs.Dock='Fill'; $tabs.AccessibleName='打包类型'
    $setup=New-Object Windows.Forms.TabPage; $setup.Text='Setup 安装包'
    $setupInfo=New-Label "生产 Release / Windows x64 / NSIS Setup`r`n复用正常构建与 Everything Helper 钩子；仅生成安装包。`r`n需要 Node/npm、Rust MSVC、C++ 构建工具与已准备的资源。`r`n首次构建可能下载 Rust/NSIS 依赖，请预留时间及网络。"
    $setupInfo.Padding=New-Object Windows.Forms.Padding(10); $setup.Controls.Add($setupInfo)
    $plugin=New-Object Windows.Forms.TabPage; $plugin.Text='插件包'
    $pluginLayout=New-Object Windows.Forms.TableLayoutPanel; $pluginLayout.Dock='Fill'; $pluginLayout.ColumnCount=2; $pluginLayout.RowCount=3
    $pluginLayout.ColumnStyles.Add((New-Object Windows.Forms.ColumnStyle('Percent',100))) | Out-Null
    $pluginLayout.ColumnStyles.Add((New-Object Windows.Forms.ColumnStyle('Absolute',100))) | Out-Null
    $pluginLayout.Controls.Add((New-Label '选择参考 package，或浏览含 plugin.json 的外部目录：'),0,0); $pluginLayout.SetColumnSpan($pluginLayout.GetControlFromPosition(0,0),2)
    $source=New-Object Windows.Forms.ComboBox; $source.DropDownStyle='DropDown'; $source.Dock='Fill'; $source.AccessibleName='插件 package 目录'
    foreach ($dir in @(Get-ChildItem -LiteralPath (Join-Path $root 'examples/public-plugins') -Directory | Sort-Object Name)) {
        $package=Join-Path $dir.FullName 'package'
        if (Test-Path -LiteralPath (Join-Path $package 'plugin.json') -PathType Leaf) { $source.Items.Add($package) | Out-Null }
    }
    if ($source.Items.Count -gt 0) { $source.SelectedIndex=0 }
    $browseSource=New-Object Windows.Forms.Button; $browseSource.Text='浏览插件(&B)'; $browseSource.Dock='Fill'
    $pluginLayout.Controls.Add($source,0,1); $pluginLayout.Controls.Add($browseSource,1,1)
    $metadata=New-Label ''; $metadata.AccessibleName='插件 ID 及版本'; $pluginLayout.Controls.Add($metadata,0,2); $pluginLayout.SetColumnSpan($metadata,2)
    $plugin.Controls.Add($pluginLayout); $tabs.TabPages.AddRange(@($setup,$plugin)); $layout.Controls.Add($tabs,0,1)
    $outputLayout=New-Object Windows.Forms.TableLayoutPanel; $outputLayout.Dock='Fill'; $outputLayout.ColumnCount=3
    $outputLayout.ColumnStyles.Add((New-Object Windows.Forms.ColumnStyle('Absolute',80))) | Out-Null
    $outputLayout.ColumnStyles.Add((New-Object Windows.Forms.ColumnStyle('Percent',100))) | Out-Null
    $outputLayout.ColumnStyles.Add((New-Object Windows.Forms.ColumnStyle('Absolute',100))) | Out-Null
    $outputLayout.Controls.Add((New-Label '输出目录：'),0,0)
    $output=New-Object Windows.Forms.TextBox; $output.Dock='Fill'; $output.Text=$script:state.LastOutput; $output.AccessibleName='输出根目录'
    $browseOutput=New-Object Windows.Forms.Button; $browseOutput.Text='浏览输出(&O)'; $browseOutput.Dock='Fill'
    $outputLayout.Controls.Add($output,1,0); $outputLayout.Controls.Add($browseOutput,2,0); $layout.Controls.Add($outputLayout,0,2)
    $buttons=New-Object Windows.Forms.FlowLayoutPanel; $buttons.Dock='Fill'; $buttons.WrapContents=$false
    $start=New-Object Windows.Forms.Button; $start.Text='开始打包(&S)'; $start.AutoSize=$true
    $cancel=New-Object Windows.Forms.Button; $cancel.Text='取消(&C)'; $cancel.Enabled=$false; $cancel.AutoSize=$true
    $open=New-Object Windows.Forms.Button; $open.Text='打开输出目录(&D)'; $open.AutoSize=$true
    $buttons.Controls.AddRange(@($start,$cancel,$open)); $layout.Controls.Add($buttons,0,3)
    $status=New-Label '就绪'; $status.AccessibleName='任务状态'; $layout.Controls.Add($status,0,4)
    $log=New-Object Windows.Forms.RichTextBox; $log.Dock='Fill'; $log.ReadOnly=$true; $log.DetectUrls=$false; $log.WordWrap=$false
    $log.AccessibleName='可复制的打包日志'; $log.BackColor=[Drawing.SystemColors]::Window; $log.ForeColor=[Drawing.SystemColors]::WindowText; $layout.Controls.Add($log,0,5)
    $location=New-Label '每次运行创建独立目录；产物、SHA256 及日志保留其中。'; $layout.Controls.Add($location,0,6)
    $elapsed=New-Label '耗时 00:00:00'; $elapsed.AccessibleName='已用时间'; $layout.Controls.Add($elapsed,0,7)
    $form.AcceptButton=$start
    function Update-Metadata {
        try { $manifest=Get-PackageManifest $source.Text; $metadata.Text="ID：$($manifest.pluginId)    版本：$($manifest.version)" }
        catch { $metadata.Text='请选择有效插件目录：' + $_.Exception.Message }
    }
    function Select-Folder([string]$Current) {
        $dialog=New-Object Windows.Forms.FolderBrowserDialog; $dialog.Description='选择目录'; $dialog.SelectedPath=$Current
        try { if ($dialog.ShowDialog($form) -eq 'OK') { return $dialog.SelectedPath } } finally { $dialog.Dispose() }
        return $Current
    }
    function Set-Busy([bool]$Busy) {
        $tabs.Enabled=-not $Busy; $outputLayout.Enabled=-not $Busy; $start.Enabled=-not $Busy; $cancel.Enabled=$Busy
    }
    function Cancel-Run {
        if ($null -eq $script:state.Worker -or $script:state.Cancelled) { return }
        $script:state.Cancelled=$true; $cancel.Enabled=$false; $status.Text='正在取消，等待本次进程树退出…'
        Stop-PackageWorker $script:state.Worker $script:state.Request
    }
    $source.add_TextChanged({ Update-Metadata }); Update-Metadata
    $browseSource.add_Click({ $source.Text=Select-Folder $source.Text })
    $browseOutput.add_Click({ $output.Text=Select-Folder $output.Text })
    $open.add_Click({
        try {
            $path=$script:state.LastOutput
            if (-not (Test-Path -LiteralPath $path -PathType Container)) { throw '输出目录尚未创建。' }
            $info=New-Object Diagnostics.ProcessStartInfo; $info.FileName=$path; $info.UseShellExecute=$true
            [Diagnostics.Process]::Start($info) | Out-Null
        } catch { [Windows.Forms.MessageBox]::Show($form,$_.Exception.Message,'无法打开目录') | Out-Null }
    })
    $start.add_Click({
        try {
            Set-Busy $true; $log.Clear(); $status.Text='预检'; $script:state.Request=$null; $script:state.Cancelled=$false; $script:state.CloseAfterCancel=$false
            $mode=if ($tabs.SelectedIndex -eq 0) { 'Setup' } else { 'Plugin' }
            $request=New-PackageRun $root $mode $output.Text $source.Text
            $script:state.Request=$request; $script:state.LastOutput=$request.RunDirectory; $location.Text=$request.RunDirectory
            $requestPath=Join-Path $request.RunDirectory 'request.json'; Write-PackageJson $requestPath $request
            $script:state.Watch.Restart(); $script:state.Worker=Start-PackageWorker $request $requestPath
        } catch {
            $script:state.Watch.Stop(); Set-Busy $false; $status.Text='失败：' + $_.Exception.Message; $log.AppendText($_.Exception.Message + "`r`n")
            if ($null -ne $script:state.Request) { Write-PackageLog $script:state.Request $_.Exception.Message }
        }
    })
    $cancel.add_Click({ try { Cancel-Run } catch { $status.Text='取消失败：' + $_.Exception.Message + '；请再次取消。'; $script:state.Cancelled=$false; $cancel.Enabled=$true } })
    $timer=New-Object Windows.Forms.Timer; $timer.Interval=300
    $timer.add_Tick({
        if ($null -eq $script:state.Worker) { return }
        try {
            $request=$script:state.Request; $worker=$script:state.Worker
            $elapsed.Text='耗时 ' + $script:state.Watch.Elapsed.ToString('hh\:mm\:ss')
            $logPath=Join-Path $request.RunDirectory 'run.log'
            if ($null -eq $script:state.Reader -and (Test-Path -LiteralPath $logPath)) {
                $stream=[IO.File]::Open($logPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
                $script:state.Reader=[IO.StreamReader]::new($stream,[Text.Encoding]::UTF8)
            }
            if ($null -ne $script:state.Reader) { $text=$script:state.Reader.ReadToEnd(); if ($text) { $log.AppendText($text); $log.ScrollToCaret() } }
            if (-not $script:state.Cancelled) {
                try { $status.Text=(Read-PackageJson (Join-Path $request.RunDirectory 'status.json')).Stage } catch { }
            }
            if ($worker.Process.HasExited) {
                # Reap any descendants still shutting down before releasing ownership.
                if (-not $worker.IsEmpty) { $worker.Cancel(); return }
                $completion=Get-PackageCompletion $request $worker.Process.ExitCode $script:state.Cancelled
                $label=switch ($completion.State) { 'Success' {'成功'} 'Cancelled' {'已取消'} default {'失败'} }
                $status.Text=$label + '：' + $completion.Message
                if ($script:state.Cancelled) { Write-PackageJson (Join-Path $request.RunDirectory 'result.json') @{RunId=$request.RunId; State='Cancelled'; Message=$completion.Message} }
                $script:state.Watch.Stop(); $worker.Dispose(); $script:state.Worker=$null
                if ($null -ne $script:state.Reader) { $script:state.Reader.Dispose(); $script:state.Reader=$null }
                Set-Busy $false
                if ($script:state.CloseAfterCancel) { $form.Close() }
            }
        } catch { $status.Text='诊断：' + $_.Exception.Message }
    })
    $form.add_FormClosing({
        param($sender,$event)
        if ($null -ne $script:state.Worker) {
            $event.Cancel=$true; $script:state.CloseAfterCancel=$true
            try { Cancel-Run } catch { $status.Text='取消失败：' + $_.Exception.Message; $script:state.Cancelled=$false; $cancel.Enabled=$true }
        }
    })
    $timer.Start()
    try { [Windows.Forms.Application]::Run($form) }
    finally { $timer.Stop(); $timer.Dispose(); $form.Dispose() }
} catch {
    $message='UiPilot 打包工具无法启动：' + $_.Exception.Message
    $startupLog=Join-Path ([IO.Path]::GetTempPath()) 'uipilot-package-startup.log'
    [IO.File]::WriteAllText($startupLog,$message,[Text.UTF8Encoding]::new($false))
    try { Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.MessageBox]::Show($message + "`r`n日志：$startupLog",'UiPilot 打包工具') | Out-Null } catch { Write-Error $message }
    exit 1
}
