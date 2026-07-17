[CmdletBinding()]
param(
    [switch] $VerifyFailFast,
    [switch] $CaptureIo
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $repoRoot 'spikes/systemindex/Cargo.toml'
$binary = Join-Path $repoRoot 'spikes/systemindex/target/release/systemindex-spike.exe'
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$evidenceDir = Join-Path $repoRoot "artifacts/systemindex-spike/$timestamp"

$os = Get-CimInstance Win32_OperatingSystem
if ($os.Caption -notlike '*Windows 11*' -or $os.OSArchitecture -notlike '*64*') {
    Write-Error 'SystemIndex Spike requires Windows 11 x64'
    exit 2
}

[IO.Directory]::CreateDirectory($evidenceDir) | Out-Null

$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$disk = Get-CimInstance Win32_DiskDrive | Select-Object Model, MediaType, Size
$search = Get-Service WSearch -ErrorAction SilentlyContinue
[ordered]@{
    capturedAt = (Get-Date).ToString('o')
    windows = [ordered]@{
        caption = $os.Caption
        version = $os.Version
        build = $os.BuildNumber
        architecture = $os.OSArchitecture
    }
    cpu = $cpu.Name
    memoryBytes = [uint64]$os.TotalVisibleMemorySize * 1KB
    storage = @($disk)
    windowsSearch = if ($search) {
        [ordered]@{ status = [string]$search.Status; startType = [string]$search.StartType }
    } else {
        $null
    }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidenceDir 'environment.json') -Encoding utf8

& cargo build --release --manifest-path $manifest
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $binary)) {
    throw 'Spike release build failed'
}

function Invoke-SpikeCase {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string[]] $Arguments
    )

    $stdout = Join-Path $evidenceDir "$Name.stdout.json"
    $stderr = Join-Path $evidenceDir "$Name.stderr.json"
    $elapsed = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $binary -ArgumentList $Arguments -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $elapsed.Stop()
    [ordered]@{
        case = $Name
        pid = $process.Id
        exitCode = $process.ExitCode
        elapsedMs = [math]::Round($elapsed.Elapsed.TotalMilliseconds, 3)
    } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidenceDir "$Name.exit.json") -Encoding utf8
}

Invoke-SpikeCase -Name 'status' -Arguments @('status', '--json')
Invoke-SpikeCase -Name 'scopes' -Arguments @('scopes', '--json')
Invoke-SpikeCase -Name 'query' -Arguments @('query', '--literal', 'uipilot-spike-probe', '--limit', '1', '--json')

if ($VerifyFailFast) {
    $sentinelScript = Join-Path $PSScriptRoot 'prepare-systemindex-sentinels.ps1'
    $failfastScript = Join-Path $PSScriptRoot 'test-systemindex-failfast.ps1'
    $sentinelManifest = & $sentinelScript -Create
    if ($LASTEXITCODE -ne 0 -or -not $sentinelManifest) {
        throw 'Sentinel preparation failed'
    }
    $sentinelManifest = ([string]$sentinelManifest).Trim()
    try {
        $failfastEvidence = & $failfastScript -SentinelManifest $sentinelManifest
        $failfastExit = $LASTEXITCODE
        $failfastEvidence | Set-Content -LiteralPath (Join-Path $evidenceDir 'failfast-evidence-path.txt') -Encoding utf8
        if ($failfastExit -ne 0) { throw 'Fail-fast verification failed' }
    } finally {
        & $sentinelScript -Cleanup -Manifest $sentinelManifest
        if ($LASTEXITCODE -ne 0) { throw 'Sentinel cleanup failed' }
    }
}

if ($CaptureIo) {
    $captureScript = Join-Path $PSScriptRoot 'capture-systemindex-io.ps1'
    $ioEvidence = & $captureScript
    $ioExit = $LASTEXITCODE
    $ioEvidence | Set-Content -LiteralPath (Join-Path $evidenceDir 'io-evidence-path.txt') -Encoding utf8
    if ($ioExit -ne 0) { throw 'SystemIndex I/O capture failed or was not runnable' }
}

Write-Output $evidenceDir
