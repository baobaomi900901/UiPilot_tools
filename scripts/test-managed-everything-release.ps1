[CmdletBinding()]
param(
    [ValidateSet('Local', 'Source', 'SelfTest')]
    [string]$Mode = 'Local'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script:RepositoryRoot = Split-Path -Parent $PSScriptRoot
$script:EvidenceRoot = Join-Path $script:RepositoryRoot 'artifacts\managed-everything'

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Get-PeMachine([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        try {
            Assert-Condition ($reader.ReadUInt16() -eq 0x5a4d) 'Artifact is not a PE image'
            $stream.Position = 0x3c
            $peOffset = $reader.ReadUInt32()
            $stream.Position = $peOffset
            Assert-Condition ($reader.ReadUInt32() -eq 0x00004550) 'Artifact has no PE signature'
            return $reader.ReadUInt16()
        }
        finally { $reader.Dispose() }
    }
    finally { $stream.Dispose() }
}

function Add-ManifestReader {
    if ('UiPilot.ManagedEverythingRelease.ManifestReader' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
namespace UiPilot.ManagedEverythingRelease {
  public static class ManifestReader {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr LoadLibraryEx(string file, IntPtr handle, uint flags);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32", SetLastError=true)] static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32")] static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32")] static extern bool FreeLibrary(IntPtr module);
    public static string Read(string file) {
      IntPtr module = LoadLibraryEx(file, IntPtr.Zero, 0x00000002);
      if (module == IntPtr.Zero) throw new Win32Exception();
      try {
        IntPtr resource = FindResource(module, (IntPtr)1, (IntPtr)24);
        if (resource == IntPtr.Zero) throw new Win32Exception();
        uint size = SizeofResource(module, resource);
        IntPtr loaded = LoadResource(module, resource);
        IntPtr bytes = LockResource(loaded);
        byte[] data = new byte[size]; Marshal.Copy(bytes, data, 0, (int)size);
        if (size >= 2 && data[0] == 0xff && data[1] == 0xfe) return Encoding.Unicode.GetString(data);
        return Encoding.UTF8.GetString(data);
      } finally { FreeLibrary(module); }
    }
  }
}
'@
}

function Protect-ReleaseText([string]$Text) {
    if ($null -eq $Text) { return '' }
    $redacted = $Text
    foreach ($value in @($script:RepositoryRoot, $env:USERPROFILE, $env:USERNAME, $env:COMPUTERNAME)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            $redacted = $redacted -replace [regex]::Escape($value), '<redacted>'
        }
    }
    $redacted = $redacted -replace '(?i)S-1-\d+(?:-\d+){1,14}', '<sid>'
    $redacted = $redacted -replace '(?im)[A-Z]:[\\/][^\r\n]*', '<path>'
    $redacted = $redacted -replace '(?im)\\\\[^\r\n]+', '<path>'
    $redacted = $redacted -replace '(?im)^.*(?:search[ _-]?text|result[ _-]?path).*(?:\r?\n|$)', '<sensitive-output>`r`n'
    return $redacted
}

function Assert-ReleaseTextRedacted([string]$Text) {
    Assert-Condition ($Text -notmatch '(?i)S-1-\d+(?:-\d+){1,14}') 'Release evidence contains SID data'
    Assert-Condition ($Text -notmatch '(?i)[A-Z]:[\\/]') 'Release evidence contains absolute path data'
    Assert-Condition ($Text -notmatch '(?i)\\\\[^\\\s]+\\') 'Release evidence contains UNC path data'
    foreach ($value in @($env:USERPROFILE, $env:USERNAME, $env:COMPUTERNAME)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            Assert-Condition (-not $Text.Contains($value)) 'Release evidence contains machine or user data'
        }
    }
}

function Write-Utf8([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

function Invoke-CapturedCommand([string]$FilePath, [string[]]$Arguments) {
    $oldPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $lines = @(& $FilePath @Arguments 2>&1)
        $code = $LASTEXITCODE
        return [pscustomobject]@{ ExitCode = [int]$code; Output = (($lines | Out-String) -as [string]) }
    }
    catch {
        return [pscustomobject]@{ ExitCode = 1; Output = ('command-exception:{0}' -f $_.Exception.GetType().Name) }
    }
    finally { $ErrorActionPreference = $oldPreference }
}

function Join-CommandResults($First, $Second) {
    return [pscustomobject]@{
        ExitCode = if ($First.ExitCode -ne 0) { [int]$First.ExitCode } else { [int]$Second.ExitCode }
        Output = (([string]$First.Output) + "`r`n" + ([string]$Second.Output))
    }
}

function Invoke-ProductionHelperBuild {
    $build = Invoke-CapturedCommand 'powershell.exe' @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
        (Join-Path $PSScriptRoot 'build-managed-everything-installer.ps1'),
        '-Configuration', 'Release', '-Mode', 'ProductionHelper'
    )
    if ($build.ExitCode -ne 0) { return $build }
    try {
        $helper = Join-Path $script:RepositoryRoot 'src-tauri\target\release\uipilot-everything-installer.exe'
        Assert-Condition (Test-Path -LiteralPath $helper -PathType Leaf) 'Production Helper was not copied to the bundle input'
        Assert-Condition ((Get-PeMachine $helper) -eq 0x8664) 'Production Helper is not x64'
        Add-ManifestReader
        $manifest = [UiPilot.ManagedEverythingRelease.ManifestReader]::Read($helper)
        Assert-Condition ($manifest -match 'requestedExecutionLevel') 'Production Helper manifest is missing requestedExecutionLevel'
        Assert-Condition ($manifest -match 'level\s*=\s*["'']asInvoker["'']') 'Production Helper is not asInvoker'
        Assert-Condition ($manifest -match 'uiAccess\s*=\s*["'']false["'']') 'Production Helper enables uiAccess'
        $strings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($helper))
        Assert-Condition (-not $strings.Contains('fault-injection:')) 'Production Helper contains the VM fault path'
        return [pscustomobject]@{ ExitCode = 0; Output = (([string]$build.Output) + "`r`nhelper-inspection:pass") }
    }
    catch {
        return [pscustomobject]@{ ExitCode = 1; Output = (([string]$build.Output) + "`r`nhelper-inspection:fail") }
    }
}

function Invoke-ProductionNsisBuild {
    $build = Invoke-CapturedCommand 'npm.cmd' @('run', 'tauri', '--', 'build', '--bundles', 'nsis')
    if ($build.ExitCode -ne 0) { return $build }
    try {
        $helper = Join-Path $script:RepositoryRoot 'src-tauri\target\release\uipilot-everything-installer.exe'
        Assert-Condition (Test-Path -LiteralPath $helper -PathType Leaf) 'Final production NSIS Helper input is missing'
        Assert-Condition ((Get-PeMachine $helper) -eq 0x8664) 'Final production NSIS Helper input is not x64'
        Add-ManifestReader
        $manifest = [UiPilot.ManagedEverythingRelease.ManifestReader]::Read($helper)
        Assert-Condition ($manifest -match 'requestedExecutionLevel') 'Final production NSIS Helper manifest is missing requestedExecutionLevel'
        Assert-Condition ($manifest -match 'level\s*=\s*["'']asInvoker["'']') 'Final production NSIS Helper is not asInvoker'
        Assert-Condition ($manifest -match 'uiAccess\s*=\s*["'']false["'']') 'Final production NSIS Helper enables uiAccess'
        $strings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($helper))
        Assert-Condition (-not $strings.Contains('fault-injection:')) 'Final production NSIS Helper contains the VM fault path'
        $generatedNsis = Join-Path $script:RepositoryRoot 'src-tauri\target\release\nsis\x64\installer.nsi'
        Assert-Condition (Test-Path -LiteralPath $generatedNsis -PathType Leaf) 'Generated production NSIS script is missing'
        $generatedSource = Get-Content -LiteralPath $generatedNsis -Raw -Encoding utf8
        Assert-Condition (-not $generatedSource.Contains('File /a "/oname=uipilot-everything-installer.exe"')) 'Helper is installed as an application binary'
        Assert-Condition (-not $generatedSource.Contains('Delete "$INSTDIR\uipilot-everything-installer.exe"')) 'Uninstaller treats Helper as an installed application binary'
        Assert-Condition ([regex]::Matches($generatedSource, 'nsis-hooks\.nsh').Count -eq 1) 'Generated NSIS must include the managed Everything hooks exactly once'
        $hookSource = Get-Content -LiteralPath (Join-Path $script:RepositoryRoot 'src-tauri\windows\nsis-hooks.nsh') -Raw -Encoding utf8
        Assert-Condition ([regex]::Matches($hookSource, 'File "/oname=uipilot-everything-installer\.exe"').Count -eq 1) 'Managed Everything hooks must contain exactly one temporary Helper staging reference'
        return [pscustomobject]@{ ExitCode = 0; Output = (([string]$build.Output) + "`r`nfinal-helper-inspection:pass`r`nhelper-install-isolation:pass") }
    }
    catch {
        return [pscustomobject]@{ ExitCode = 1; Output = (([string]$build.Output) + "`r`nfinal-helper-inspection:fail") }
    }
}

function Invoke-FrontendGate {
    $tests = Invoke-CapturedCommand 'npm.cmd' @('test')
    if ($tests.ExitCode -ne 0) { return $tests }
    $build = Invoke-CapturedCommand 'npm.cmd' @('run', 'build')
    return Join-CommandResults $tests $build
}

function New-ReleaseStages {
    return @(
        [pscustomobject]@{
            Name = 'Helper tests'
            Action = { Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--bin', 'uipilot-everything-installer', '--features', 'installer-helper-build') }
        },
        [pscustomobject]@{
            Name = 'Production Helper build and inspection'
            Action = { Invoke-ProductionHelperBuild }
        },
        [pscustomobject]@{
            Name = 'Installer Source'
            Action = { Invoke-CapturedCommand 'powershell.exe' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/test-managed-everything-installer.ps1', '-Mode', 'Source') }
        },
        [pscustomobject]@{
            Name = 'Everything ResourceOnly'
            Action = { Invoke-CapturedCommand 'powershell.exe' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/test-everything-package.ps1', '-ResourceOnly') }
        },
        [pscustomobject]@{
            Name = 'IPC full'
            Action = { Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'spikes/everything-ipc/Cargo.toml') }
        },
        [pscustomobject]@{
            Name = 'Rust host full'
            Action = { Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml') }
        },
        [pscustomobject]@{
            Name = 'Frontend tests and build'
            Action = { Invoke-FrontendGate }
        },
        [pscustomobject]@{
            Name = 'Production NSIS'
            Action = { Invoke-ProductionNsisBuild }
        }
    )
}

function Invoke-ReleasePipeline($Stages, [string]$EvidenceDirectory) {
    $records = @()
    for ($index = 0; $index -lt $Stages.Count; $index++) {
        $stage = $Stages[$index]
        Write-Host ('STAGE_START {0}' -f $stage.Name)
        try {
            $result = & $stage.Action
            if ($null -eq $result -or $null -eq $result.ExitCode) { throw 'Stage did not return an exit code' }
        }
        catch {
            $result = [pscustomobject]@{ ExitCode = 1; Output = ('stage-exception:{0}' -f $_.Exception.GetType().Name) }
        }
        $safeName = (($stage.Name.ToLowerInvariant() -replace '[^a-z0-9]+', '-') -replace '(^-|-$)', '')
        $logName = ('{0:d2}-{1}.log' -f ($index + 1), $safeName)
        $redacted = Protect-ReleaseText ([string]$result.Output)
        Assert-ReleaseTextRedacted $redacted
        Write-Utf8 (Join-Path $EvidenceDirectory $logName) $redacted
        $outcome = if ($result.ExitCode -eq 0) { 'pass' } else { 'fail' }
        $records += [pscustomobject]@{ stage = $stage.Name; outcome = $outcome; exit_code = [int]$result.ExitCode; evidence = $logName }
        if ($result.ExitCode -ne 0) {
            Write-Host ('STAGE_FAIL {0} EXIT_CODE {1}' -f $stage.Name, $result.ExitCode)
            return [pscustomobject]@{ ExitCode = [int]$result.ExitCode; FailingStage = $stage.Name; Records = $records }
        }
        Write-Host ('STAGE_PASS {0}' -f $stage.Name)
    }
    return [pscustomobject]@{ ExitCode = 0; FailingStage = $null; Records = $records }
}

function Write-ReleaseSummary([string]$Directory, $Pipeline, [string]$Commit) {
    $summary = [ordered]@{
        schema_version = 1
        kind = 'managed-everything-local-release-gate'
        outcome = $(if ($Pipeline.ExitCode -eq 0) { 'pass' } else { 'fail' })
        exit_code = [int]$Pipeline.ExitCode
        failing_stage = $Pipeline.FailingStage
        commit = $Commit
        stages = @($Pipeline.Records)
        vm_matrix_status = 'NOT RUN'
        release_ready = $false
    }
    $json = $summary | ConvertTo-Json -Depth 8
    Assert-ReleaseTextRedacted $json
    Write-Utf8 (Join-Path $Directory 'release-local.json') $json
}

function Write-SetupHashEvidence([string]$Directory) {
    $bundle = Join-Path $script:RepositoryRoot 'src-tauri\target\release\bundle\nsis'
    $setups = @(Get-ChildItem -LiteralPath $bundle -Filter '*.exe' -File -ErrorAction Stop)
    Assert-Condition ($setups.Count -eq 1) 'Production bundle must contain exactly one Setup'
    Assert-Condition (-not $setups[0].Name.Contains('VM Instrumented')) 'VM Instrumented Setup leaked into production output'
    Assert-Condition ((Get-PeMachine $setups[0].FullName) -eq 0x014c) 'Production NSIS Setup has the wrong bootstrap architecture'
    $helper = Join-Path $script:RepositoryRoot 'src-tauri\target\release\uipilot-everything-installer.exe'
    Assert-Condition (Test-Path -LiteralPath $helper -PathType Leaf) 'Final production NSIS Helper input is missing'
    Assert-Condition ((Get-PeMachine $helper) -eq 0x8664) 'Final production NSIS Helper input is not x64'
    Add-ManifestReader
    $manifest = [UiPilot.ManagedEverythingRelease.ManifestReader]::Read($helper)
    Assert-Condition ($manifest -match 'level\s*=\s*["'']asInvoker["'']') 'Final production NSIS Helper is not asInvoker'
    Assert-Condition ($manifest -match 'uiAccess\s*=\s*["'']false["'']') 'Final production NSIS Helper enables uiAccess'
    $helperStrings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($helper))
    Assert-Condition (-not $helperStrings.Contains('fault-injection:')) 'Final production NSIS Helper contains the VM fault path'
    $evidence = [ordered]@{
        schema_version = 1
        kind = 'production-setup'
        setup_count = 1
        sha256 = (Get-FileHash -LiteralPath $setups[0].FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        length = [long]$setups[0].Length
        helper_sha256 = (Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant()
        helper_length = [long](Get-Item -LiteralPath $helper).Length
        vm_instrumented = $false
    } | ConvertTo-Json
    Assert-ReleaseTextRedacted $evidence
    Write-Utf8 (Join-Path $Directory 'setup-sha256.json') $evidence
}

function Invoke-SourceGate {
    $actualOrder = @((New-ReleaseStages) | ForEach-Object { $_.Name })
    $expectedOrder = @(
        'Helper tests', 'Production Helper build and inspection', 'Installer Source',
        'Everything ResourceOnly', 'IPC full', 'Rust host full',
        'Frontend tests and build', 'Production NSIS'
    )
    Assert-Condition (($actualOrder -join "`n") -ceq ($expectedOrder -join "`n")) 'Release stage ordering contract changed'
    $source = Get-Content -LiteralPath $PSCommandPath -Raw -Encoding utf8
    foreach ($contract in @(
        "Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml')",
        'VM_MATRIX_NOT_RUN RELEASE_READY_FALSE',
        "vm_matrix_status = 'NOT RUN'",
        'release_ready = $false'
    )) {
        Assert-Condition $source.Contains($contract) "Release Source contract is missing: $contract"
    }
    $matrix = Get-Content -LiteralPath (Join-Path $script:RepositoryRoot 'docs\testing\managed-everything-vm-matrix.md') -Raw -Encoding utf8
    Assert-Condition ([regex]::Matches($matrix, '(?m)^## Row (?:[1-9]|1[0-3])\b').Count -eq 13) 'Release matrix must contain exactly 13 rows'
    Assert-Condition $matrix.Contains('BLOCKED: Instrumented Setup termination/recovery interface missing') 'Release matrix hides the Row 8 interface blocker'
    Assert-Condition $matrix.Contains('BLOCKED: Instrumented Setup concurrency-control interface missing') 'Release matrix hides the Row 13 interface blocker'
    $installerSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'test-managed-everything-installer.ps1') -Raw -Encoding utf8
    Assert-Condition (-not $installerSource.Contains("'src-tauri\target\release\bundle\nsis'")) 'Installer Source must not depend on a pre-existing production Setup'
    $productionNsisAction = 'Action = { Invoke-' + 'ProductionNsisBuild }'
    $helperHashField = 'helper_' + 'sha256 ='
    $helperInstallIsolation = 'Helper is installed as an application binary'
    Assert-Condition $source.Contains($productionNsisAction) 'Production NSIS stage must inspect the final Helper after Tauri builds it'
    Assert-Condition $source.Contains($helperHashField) 'Production Setup evidence must bind the final Helper identity'
    Assert-Condition $source.Contains($helperInstallIsolation) 'Production NSIS stage must reject an installed Helper application binary'
    $pipelineIndex = $source.IndexOf('$pipeline = Invoke-ReleasePipeline', [StringComparison]::Ordinal)
    $setupEvidenceIndex = $source.IndexOf('try { Write-SetupHashEvidence', [StringComparison]::Ordinal)
    Assert-Condition ($pipelineIndex -ge 0 -and $setupEvidenceIndex -gt $pipelineIndex) 'Production Setup inspection must run only after the NSIS stage succeeds'
    Write-Output 'MANAGED_EVERYTHING_RELEASE_SOURCE_PASS'
}

function Invoke-SelfTest {
    $directory = Join-Path $script:EvidenceRoot ('selftest-' + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    try {
        $actualOrder = @((New-ReleaseStages) | ForEach-Object { $_.Name })
        $expectedOrder = @(
            'Helper tests', 'Production Helper build and inspection', 'Installer Source',
            'Everything ResourceOnly', 'IPC full', 'Rust host full',
            'Frontend tests and build', 'Production NSIS'
        )
        Assert-Condition (($actualOrder -join "`n") -ceq ($expectedOrder -join "`n")) 'Release stage ordering contract changed'
        $caught = $false
        try { Assert-ReleaseTextRedacted '\\server\share\secret' } catch { $caught = $true }
        Assert-Condition $caught 'Release evidence assertion accepted a UNC path'
        $script:ThirdStageRan = $false
        $stages = @(
            [pscustomobject]@{ Name = 'one'; Action = { [pscustomobject]@{ ExitCode = 0; Output = 'ok' } } },
            [pscustomobject]@{ Name = 'two'; Action = { [pscustomobject]@{ ExitCode = 37; Output = 'C:\Users\person\private S-1-5-21-1-2-3-1001' } } },
            [pscustomobject]@{ Name = 'three'; Action = { $script:ThirdStageRan = $true; [pscustomobject]@{ ExitCode = 0; Output = 'wrong' } } }
        )
        $pipeline = Invoke-ReleasePipeline $stages $directory
        Assert-Condition ($pipeline.ExitCode -eq 37) 'Release pipeline did not preserve a stage exit code'
        Assert-Condition ($pipeline.FailingStage -ceq 'two') 'Release pipeline did not identify the failing stage'
        Assert-Condition (-not $script:ThirdStageRan) 'Release pipeline did not stop at first failure'
        $log = Get-Content -LiteralPath (Join-Path $directory '02-two.log') -Raw
        Assert-ReleaseTextRedacted $log
        Assert-Condition (-not $log.Contains('person')) 'Release self-test leaked a user name'
        Write-Output 'MANAGED_EVERYTHING_RELEASE_SELFTEST_PASS'
    }
    finally {
        if (Test-Path -LiteralPath $directory -PathType Container) { Remove-Item -LiteralPath $directory -Recurse -Force }
    }
}

if ($Mode -ceq 'SelfTest') {
    Invoke-SelfTest
    exit 0
}
if ($Mode -ceq 'Source') {
    Invoke-SourceGate
    exit 0
}

[IO.Directory]::CreateDirectory($script:EvidenceRoot) | Out-Null
$runDirectory = Join-Path $script:EvidenceRoot ('local-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($runDirectory) | Out-Null
Push-Location $script:RepositoryRoot
try {
    $pipeline = Invoke-ReleasePipeline (New-ReleaseStages) $runDirectory
    $commitResult = Invoke-CapturedCommand 'git.exe' @('rev-parse', 'HEAD')
    $commit = if ($commitResult.ExitCode -eq 0) { ([string]$commitResult.Output).Trim() } else { 'unavailable' }
    if ($pipeline.ExitCode -eq 0) {
        try { Write-SetupHashEvidence $runDirectory }
        catch {
            $pipeline = [pscustomobject]@{
                ExitCode = 1
                FailingStage = 'Production Setup evidence'
                Records = @($pipeline.Records) + [pscustomobject]@{ stage = 'Production Setup evidence'; outcome = 'fail'; exit_code = 1; evidence = 'setup-sha256.json' }
            }
            Write-Output 'STAGE_FAIL Production Setup evidence EXIT_CODE 1'
        }
    }
    Write-ReleaseSummary $runDirectory $pipeline $commit
    if ($pipeline.ExitCode -ne 0) { exit $pipeline.ExitCode }
    Write-Output 'MANAGED_EVERYTHING_LOCAL_GATE_PASS VM_MATRIX_NOT_RUN RELEASE_READY_FALSE'
}
finally { Pop-Location }
