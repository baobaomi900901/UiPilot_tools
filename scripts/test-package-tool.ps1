$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw "ASSERT: $Message" } }
$runtime = Join-Path $PSScriptRoot 'package-runtime.ps1'
Assert (Test-Path -LiteralPath $runtime) 'Packaging runtime must exist before runner behavior can work'
. $runtime
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('uipilot-pack-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$script:passed = 0
function Test([string]$Name, [scriptblock]$Body) {
    & $Body
    $script:passed++
    Write-Output "PASS $Name"
}
function MustFail([scriptblock]$Body, [string]$Pattern) {
    $caught = $false
    try { & $Body | Out-Null } catch { $caught = $true; Assert ($_.Exception.Message -match $Pattern) "Expected $Pattern, got $($_.Exception.Message)" }
    Assert $caught "Expected failure: $Pattern"
}
function Fixture {
    $root = Join-Path $testRoot ([guid]::NewGuid().ToString('N'))
    foreach ($dir in @('src-tauri', 'node_modules/.bin', 'packages/plugin-cli', 'plugin 中文 & 空格')) {
        New-Item -ItemType Directory -Path (Join-Path $root $dir) -Force | Out-Null
    }
    Write-PackageJson (Join-Path $root 'src-tauri/tauri.conf.json') @{ version='0.14.0'; productName='UiPilot_tools' }
    Write-PackageJson (Join-Path $root 'plugin 中文 & 空格/plugin.json') @{ pluginId='com.test.plugin'; version='1.2.3' }
    return $root
}
$script:tools = @{ Node='fake-node'; NpmCli='fake-npm-cli' }
$script:calls = New-Object Collections.Generic.List[object]
$script:failAt = 0
$script:createSetup = $true
$script:cancelAt = 0
$fake = {
    param($File, $Arguments, $WorkingDirectory, $Request)
    $script:calls.Add(@{ File=$File; Args=@($Arguments); Root=$WorkingDirectory })
    if ($script:calls.Count -eq $script:cancelAt) { [IO.File]::WriteAllText((Join-Path $Request.RunDirectory 'cancel.flag'), '') }
    if ($script:calls.Count -eq $script:failAt) { return 7 }
    if ($Arguments -contains 'pack') {
        $index = [Array]::IndexOf([object[]]$Arguments, '--output')
        [IO.File]::WriteAllText($Arguments[$index + 1], 'fake-archive')
    }
    if (($Arguments -contains 'nsis') -and $script:createSetup) {
        $setup = Join-Path $WorkingDirectory 'src-tauri/target/release/bundle/nsis/UiPilot_tools_0.14.0_x64-setup.exe'
        New-Item -ItemType Directory -Path (Split-Path $setup) -Force | Out-Null
        [IO.File]::WriteAllText($setup, 'new-production-setup-' + [guid]::NewGuid())
    }
    return 0
}
function ResetFake { $script:calls.Clear(); $script:failAt=0; $script:createSetup=$true; $script:cancelAt=0 }
try {
    Test 'plugin pipeline passes literal special paths and validates final archive' {
        ResetFake; $root=Fixture
        $request=New-PackageRun -Root $root -Mode Plugin -OutputRoot (Join-Path $root '输出 & packages') -Source (Join-Path $root 'plugin 中文 & 空格')
        $result=Invoke-PackageBuild $request $script:tools $fake
        Assert ($script:calls.Count -eq 4) 'build + source validate + pack + archive validate'
        Assert (($script:calls[0].Args -join '|') -eq 'fake-npm-cli|run|build|--workspace|@uipilot/plugin-cli') 'build current CLI first'
        Assert ($script:calls[1].Args[2] -ceq $request.Source) 'source argument preserved literally'
        Assert ($script:calls[3].Args[2] -ceq $result.Artifact) 'final archive validated'
        Assert ($result.Sha256 -eq (Get-FileHash -LiteralPath $result.Artifact -Algorithm SHA256).Hash.ToLowerInvariant()) 'record actual SHA256'
        Assert ($result.State -eq 'Success') 'success only after all stages'
        Assert (Test-Path -LiteralPath (Join-Path $request.RunDirectory 'run.log')) 'persistent log'
    }
    Test 'single-character plugin ID remains compatible with CLI' {
        ResetFake; $root=Fixture; $source=Join-Path $root 'plugin 中文 & 空格'
        Write-PackageJson (Join-Path $source 'plugin.json') @{pluginId='a'; version='1.2.3'}
        $request=New-PackageRun $root Plugin (Join-Path $root 'out') $source
        $result=Invoke-PackageBuild $request $script:tools $fake
        Assert ($script:calls.Count -eq 4) 'single-character ID completes full pipeline'
        Assert ((Split-Path $result.Artifact -Leaf) -ceq 'a-1.2.3.uipilot-plugin') 'single-character ID creates safe expected filename'
    }
    Test 'unique runs preserve previous outputs' {
        $root=Fixture; $output=Join-Path $root 'out'
        $a=New-PackageRun $root Plugin $output (Join-Path $root 'plugin 中文 & 空格')
        [IO.File]::WriteAllText((Join-Path $a.RunDirectory 'keep.txt'),'keep')
        $b=New-PackageRun $root Plugin $output (Join-Path $root 'plugin 中文 & 空格')
        Assert ($a.RunDirectory -ne $b.RunDirectory) 'unique run directories'
        Assert ([IO.File]::ReadAllText((Join-Path $a.RunDirectory 'keep.txt')) -eq 'keep') 'old files preserved'
    }
    Test 'each command failure short circuits' {
        foreach ($stage in 1..4) {
            ResetFake; $script:failAt=$stage; $root=Fixture
            $request=New-PackageRun $root Plugin (Join-Path $root 'out') (Join-Path $root 'plugin 中文 & 空格')
            MustFail { Invoke-PackageBuild $request $script:tools $fake } 'exit 7'
            Assert ($script:calls.Count -eq $stage) 'no commands after failure'
        }
    }
    Test 'missing archive cannot report success despite command exit zero' {
        $root=Fixture; $request=New-PackageRun $root Plugin (Join-Path $root 'out') (Join-Path $root 'plugin 中文 & 空格')
        MustFail { Invoke-PackageBuild $request $script:tools { return 0 } } 'artifact'
    }
    Test 'unsafe metadata and output within source are rejected' {
        $root=Fixture; $source=Join-Path $root 'plugin 中文 & 空格'
        MustFail { New-PackageRun $root Plugin (Join-Path $source 'out') $source } 'outside'
        foreach ($id in @('../escape','CON','a:b','com.good.')) {
            Write-PackageJson (Join-Path $source 'plugin.json') @{pluginId=$id; version='1.0.0'}
            MustFail { New-PackageRun $root Plugin (Join-Path $root 'out') $source } 'filename'
        }
        Write-PackageJson (Join-Path $source 'plugin.json') @{pluginId='com.good'; version='../bad'}
        MustFail { New-PackageRun $root Plugin (Join-Path $root 'out') $source } 'version'
    }
    Test 'output reparse ancestor is rejected' {
        $root=Fixture; $link=Join-Path $root 'junction'; $target=Join-Path $root 'real'
        New-Item -ItemType Directory -Path $target | Out-Null
        New-Item -ItemType Junction -Path $link -Target $target | Out-Null
        try { MustFail { New-PackageRun $root Setup (Join-Path $link 'out') } 'reparse' }
        finally { [IO.Directory]::Delete($link) }
    }
    Test 'setup uses unchanged production command and only fresh current artifact' {
        ResetFake; $root=Fixture; $request=New-PackageRun $root Setup (Join-Path $root 'out')
        $result=Invoke-PackageBuild $request $script:tools $fake
        Assert (($script:calls[0].Args -join '|') -eq 'fake-npm-cli|run|tauri|--|build|--bundles|nsis') 'normal production hooks'
        Assert ((Split-Path $result.Artifact -Leaf) -ceq 'UiPilot_tools_0.14.0_x64-setup.exe') 'expected filename'
    }
    Test 'stale same-version setup and wrong-version VM artifacts are rejected' {
        foreach ($name in @('UiPilot_tools_0.14.0_x64-setup.exe','UiPilot_tools_0.13.0_x64-setup.exe','UiPilot_VM_Instrumented_0.14.0_x64-setup.exe')) {
            ResetFake; $script:createSetup=$false; $root=Fixture
            $bundle=Join-Path $root 'src-tauri/target/release/bundle/nsis'; New-Item -ItemType Directory -Path $bundle -Force | Out-Null
            [IO.File]::WriteAllText((Join-Path $bundle $name),'old')
            $request=New-PackageRun $root Setup (Join-Path $root 'out')
            MustFail { Invoke-PackageBuild $request $script:tools $fake } 'fresh'
        }
    }
    Test 'cancellation after successful command cannot report success' {
        ResetFake; $script:cancelAt=4; $root=Fixture
        $request=New-PackageRun $root Plugin (Join-Path $root 'out') (Join-Path $root 'plugin 中文 & 空格')
        MustFail { Invoke-PackageBuild $request $script:tools $fake } 'cancel'
    }
    Test 'dependency preflight gives repair instructions without running commands' {
        $root=Fixture
        MustFail { Get-PackageTools $root Setup } 'npm ci'
    }
    Test 'worker clears only process build overrides' {
        $env:CARGO_TARGET_DIR='sentinel'; $env:CARGO_BUILD_TARGET='other'; $env:TAURI_CONFIG='instrumented'
        Clear-PackageBuildEnvironment
        Assert (-not $env:CARGO_TARGET_DIR -and -not $env:CARGO_BUILD_TARGET -and -not $env:TAURI_CONFIG) 'overrides cleared'
    }
    Test 'worker reconciliation rejects cancelled, failed, or mismatched outputs' {
        ResetFake; $root=Fixture
        $request=New-PackageRun $root Plugin (Join-Path $root 'out') (Join-Path $root 'plugin 中文 & 空格')
        $result=Invoke-PackageBuild $request $script:tools $fake
        Write-PackageJson (Join-Path $request.RunDirectory 'result.json') $result
        Assert ((Get-PackageCompletion $request 0 $false).State -eq 'Success') 'matching success'
        Assert ((Get-PackageCompletion $request 9 $false).State -eq 'Failed') 'nonzero process overrides success'
        Assert ((Get-PackageCompletion $request 0 $true).State -eq 'Cancelled') 'cancel overrides success'
        $result.RunId='wrong'; Write-PackageJson (Join-Path $request.RunDirectory 'result.json') $result
        Assert ((Get-PackageCompletion $request 0 $false).State -eq 'Failed') 'request correlation'
    }
    Test 'native process quoting preserves metacharacters without a shell' {
        $root=Fixture; $request=New-PackageRun $root Setup (Join-Path $root 'out')
        $arg='中文 & space "quoted" trailing\'
        $node=(Get-Command node.exe).Source
        $probe=Join-Path $root 'echo.cjs'; [IO.File]::WriteAllText($probe,'console.log(JSON.stringify(process.argv.slice(2)))')
        $code=Invoke-PackageCommand $node @($probe,$arg) $root $request
        Assert ($code -eq 0) 'native child exits zero'
        $log=[IO.File]::ReadAllText((Join-Path $request.RunDirectory 'run.log'))
        Assert ((@($log.Trim() | ConvertFrom-Json)[0]) -ceq $arg) 'literal argument appears in child output'
    }
    Test 'owned cancellation kills descendants and preserves unrelated process' {
        Initialize-PackageOwnership
        $root=Fixture; $request=New-PackageRun $root Setup (Join-Path $root 'out')
        $node=(Get-Command node.exe).Source
        $childScript=Join-Path $root 'owned.cjs'; $marker=Join-Path $root 'child.json'
        [IO.File]::WriteAllText($childScript,'const {spawn}=require("child_process"); const fs=require("fs"); const c=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore",windowsHide:true}); fs.writeFileSync(process.argv[2],JSON.stringify({pid:c.pid})); setInterval(()=>{},1000);')
        $info=New-Object Diagnostics.ProcessStartInfo; $info.FileName=$node; $info.Arguments='-e "setInterval(()=>{},1000)"'; $info.UseShellExecute=$false; $info.CreateNoWindow=$true
        $unrelated=[Diagnostics.Process]::Start($info); $job=$null; $child=$null
        try {
            $argsText=(ConvertTo-PackageArgument $childScript) + ' ' + (ConvertTo-PackageArgument $marker)
            $job=[UiPilot.PackagingJob]::new($node,$argsText,$root)
            $deadline=[DateTime]::UtcNow.AddSeconds(10)
            while (-not (Test-Path -LiteralPath $marker) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 30 }
            Assert (Test-Path -LiteralPath $marker) 'owned worker spawned descendant'
            $child=[Diagnostics.Process]::GetProcessById((Read-PackageJson $marker).pid)
            Stop-PackageWorker $job $request
            while (-not $job.IsEmpty -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 30 }
            Assert $job.IsEmpty 'owned tree fully terminated'
            Assert ($job.Process.HasExited -and $child.HasExited) 'worker and descendant exited'
            Assert (-not $unrelated.HasExited) 'unrelated process remains alive'
            Assert ((Get-PackageCompletion $request 0 $false).State -eq 'Cancelled') 'cancellation marker wins over exit zero'
        } finally {
            if ($null -ne $job) { $job.Dispose() }
            if ($null -ne $child) { $child.Dispose() }
            if (-not $unrelated.HasExited) { $unrelated.Kill(); $unrelated.WaitForExit() }; $unrelated.Dispose()
        }
    }
    Test 'owned worker natural exit preserves its real exit code' {
        Initialize-PackageOwnership
        $root=Fixture; $node=(Get-Command node.exe).Source
        $job=[UiPilot.PackagingJob]::new($node,'-e "process.exit(7)"',$root)
        try {
            $deadline=[DateTime]::UtcNow.AddSeconds(10)
            while (-not $job.IsEmpty -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 30 }
            Assert $job.IsEmpty 'bounded natural worker completion'
            Assert ($job.Process.HasExited -and $job.Process.ExitCode -eq 7) 'natural exit code retained'
        } finally { $job.Dispose() }
    }
    Test 'fixed outputs replace same-name packages only after successful validation' {
        ResetFake; $root=Fixture
        $request=New-PackageRun $root Plugin (Join-Path $root 'artifacts/packages/.runs') (Join-Path $root 'plugin 中文 & 空格')
        $result=Invoke-PackageBuild $request $script:tools $fake
        $published=Publish-PackageArtifact $request $result
        Assert ($published -eq (Join-Path $root 'artifacts/packages/plugins/com.test.plugin-1.2.3.uipilot-plugin')) 'fixed plugin output'
        [IO.File]::WriteAllText($published,'old-package')
        $again=Publish-PackageArtifact $request $result
        Assert ($again -eq $published -and [IO.File]::ReadAllText($published) -eq 'fake-archive') 'replace same filename'
        $result.Sha256='invalid'
        MustFail { Publish-PackageArtifact $request $result } 'hash'
        Assert ([IO.File]::ReadAllText($published) -eq 'fake-archive') 'failed validation preserves previous artifact'
        ResetFake; $setup=New-PackageRun $root Setup (Join-Path $root 'artifacts/packages/.runs')
        $setupResult=Invoke-PackageBuild $setup $script:tools $fake
        Assert ((Publish-PackageArtifact $setup $setupResult) -eq (Join-Path $root 'artifacts/packages/setup/UiPilot_tools_0.14.0_x64-setup.exe')) 'fixed setup output'
    }
    Test 'console menu accepts only listed numbers and cancellation' {
        . (Join-Path $PSScriptRoot 'package-console.ps1')
        Assert ((ConvertTo-PackageMenuIndex '2' 2) -eq 2) 'plugin menu option'
        Assert ((ConvertTo-PackageMenuIndex '0' 2) -eq 0) 'cancel selection'
        foreach ($invalid in @('3','-1','abc','1;exit','')) { MustFail { ConvertTo-PackageMenuIndex $invalid 2 } 'number' }
    }
    Test 'PowerShell 5.1 parser and console entry' {
        foreach ($name in @('package-runtime.ps1','package-runner.ps1','package-console.ps1','test-package-tool.ps1')) {
            $tokens=$null; $errors=$null
            [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name),[ref]$tokens,[ref]$errors) | Out-Null
            Assert ($errors.Count -eq 0) "PS5.1 parser: $name $errors"
        }
        $cmd=[IO.File]::ReadAllText((Join-Path (Split-Path $PSScriptRoot) 'Pack.cmd'))
        Assert ($cmd.Contains('package-console.ps1') -and $cmd -match '%~dp0') 'fixed root console entry'
        Assert ($cmd -notmatch 'WindowStyle Hidden|MessageBox|package-gui') 'interactive console stays visible'
    }
    Write-Output "PACKAGE_TOOL_PASS $script:passed tests"
}
finally {
    if ([IO.Path]::GetFullPath($testRoot).StartsWith([IO.Path]::GetTempPath(),[StringComparison]::OrdinalIgnoreCase)) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
