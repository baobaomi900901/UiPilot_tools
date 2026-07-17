[CmdletBinding()]
param()

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
    $process = Start-Process -FilePath $binary -ArgumentList $Arguments -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    [ordered]@{ case = $Name; exitCode = $process.ExitCode } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidenceDir "$Name.exit.json") -Encoding utf8
}

Invoke-SpikeCase -Name 'status' -Arguments @('status', '--json')
Invoke-SpikeCase -Name 'scopes' -Arguments @('scopes', '--json')
Invoke-SpikeCase -Name 'query' -Arguments @('query', '--literal', 'uipilot-spike-probe', '--limit', '1', '--json')

Write-Output $evidenceDir
