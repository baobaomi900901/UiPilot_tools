[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string] $SentinelManifest
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoManifest = Join-Path $repoRoot 'spikes/systemindex/Cargo.toml'
$binary = Join-Path $repoRoot 'spikes/systemindex/target/release/systemindex-spike.exe'
$manifestPath = [IO.Path]::GetFullPath($SentinelManifest)
$evidenceDir = Join-Path ([IO.Path]::GetDirectoryName($manifestPath)) `
    ("failfast-{0}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-WSearchSnapshot {
    $service = Get-CimInstance Win32_Service -Filter "Name='WSearch'" -ErrorAction Stop
    $delayed = (Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\WSearch' `
        -Name DelayedAutoStart -ErrorAction SilentlyContinue).DelayedAutoStart
    return [pscustomobject]@{
        StartMode = [string]$service.StartMode
        State = [string]$service.State
        DelayedAutoStart = if ($null -eq $delayed) { 0 } else { [int]$delayed }
    }
}

function Wait-WSearchState {
    param([Parameter(Mandatory)] [ValidateSet('Running', 'Stopped')] [string] $State)
    (Get-Service WSearch -ErrorAction Stop).WaitForStatus(
        $State,
        [TimeSpan]::FromSeconds(30)
    )
}

function Ensure-SpikeBinary {
    if (Test-Path -LiteralPath $binary -PathType Leaf) { return }
    [IO.Directory]::CreateDirectory($evidenceDir) | Out-Null
    $buildOut = Join-Path $evidenceDir 'build.stdout.log'
    $buildErr = Join-Path $evidenceDir 'build.stderr.log'
    $cargo = (Get-Command cargo -ErrorAction Stop).Source
    $process = Start-Process -FilePath $cargo -ArgumentList @(
        'build', '--release', '--manifest-path', $cargoManifest
    ) -Wait -PassThru -RedirectStandardOutput $buildOut -RedirectStandardError $buildErr
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Spike release build failed; see $buildErr"
    }
}

function Restore-WSearch {
    param([Parameter(Mandatory)] [object] $Prior)
    $current = Get-WSearchSnapshot
    if ($Prior.State -eq 'Running' -and $current.State -ne 'Running') {
        Start-Service WSearch -ErrorAction Stop
        Wait-WSearchState Running
    } elseif ($Prior.State -eq 'Stopped' -and $current.State -ne 'Stopped') {
        Stop-Service WSearch -Force -ErrorAction Stop
        Wait-WSearchState Stopped
    }

    $restored = Get-WSearchSnapshot
    if ($restored.State -ne $Prior.State -or
        $restored.StartMode -ne $Prior.StartMode -or
        $restored.DelayedAutoStart -ne $Prior.DelayedAutoStart) {
        throw 'Windows Search service state or start configuration was not restored exactly'
    }
}

if (-not (Test-IsAdministrator)) {
    [Console]::Error.WriteLine('NOT_RUNNABLE: elevated PowerShell is required before changing WSearch state')
    exit 2
}

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    [Console]::Error.WriteLine('NOT_RUNNABLE: sentinel manifest does not exist')
    exit 2
}

try {
    $sentinels = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    if ($sentinels.schemaVersion -ne 1 -or -not $sentinels.indexed -or -not $sentinels.unindexed) {
        throw 'Sentinel manifest schema is invalid'
    }
    [IO.Directory]::CreateDirectory($evidenceDir) | Out-Null
    Ensure-SpikeBinary
    $prior = Get-WSearchSnapshot
    if ($prior.State -notin @('Running', 'Stopped')) {
        [Console]::Error.WriteLine("NOT_RUNNABLE: WSearch is in transitional state $($prior.State)")
        exit 2
    }
    $prior | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidenceDir 'service-before.json') -Encoding utf8
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

$testFailed = $false
$restoreFailed = $false
try {
    if ($prior.State -eq 'Running') {
        Stop-Service WSearch -Force -ErrorAction Stop
        Wait-WSearchState Stopped
    }

    $stdout = Join-Path $evidenceDir 'service-off.stdout.json'
    $stderr = Join-Path $evidenceDir 'service-off.stderr.json'
    $process = Start-Process -FilePath $binary -ArgumentList @(
        'query', '--literal', 'uipilot-index-service-off-proof', '--limit', '20', '--json'
    ) -Wait -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    [ordered]@{ exitCode = $process.ExitCode } | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $evidenceDir 'service-off.exit.json') -Encoding utf8

    if ($process.ExitCode -eq 0) { throw 'Service-off query unexpectedly succeeded' }
    $errorEvidence = Get-Content -LiteralPath $stderr -Raw -Encoding utf8 | ConvertFrom-Json
    if ($errorEvidence.counters.searchFolderFactoryCreated -ne 0 -or
        $errorEvidence.counters.scopeSet -ne 0 -or
        $errorEvidence.counters.searchFolderEnumerated -ne 0) {
        throw 'Service-off query crossed a Search Folder creation or enumeration boundary'
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    $testFailed = $true
} finally {
    try {
        Restore-WSearch $prior
        (Get-WSearchSnapshot) | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $evidenceDir 'service-after.json') -Encoding utf8
    } catch {
        [Console]::Error.WriteLine("RESTORATION_FAILED: $($_.Exception.Message)")
        $restoreFailed = $true
    }
}

if ($restoreFailed -or $testFailed) { exit 1 }
Write-Output ([IO.Path]::GetFullPath($evidenceDir))
exit 0
