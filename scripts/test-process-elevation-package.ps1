$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$packageRoot = Split-Path -Parent $PSScriptRoot
$basePath = Join-Path $packageRoot 'src-tauri/tauri.conf.json'
$before = (Get-FileHash -LiteralPath $basePath).Hash
$json = & (Join-Path $PSScriptRoot 'build-process-elevation-package.ps1') -Mode Check
$report = $json | ConvertFrom-Json
if ($report.processElevationAvailable -ne $false -or $report.inputs.Count -lt 3) {
    throw 'Package check must report inventory without claiming implemented elevation.'
}
if ((Get-FileHash -LiteralPath $basePath).Hash -cne $before) { throw 'Default configuration changed.' }
$checker = Join-Path $packageRoot 'spikes/process-elevation-feasibility/target/release/uipilot-elevation-install-check.exe'
if (-not (Test-Path -LiteralPath $checker -PathType Leaf)) { throw 'Build the release installation checker first.' }
# Rejection test only: no directory is created, installed, modified or removed.
$start = [System.Diagnostics.ProcessStartInfo]::new()
$start.FileName = $checker
$start.Arguments = 'preinstall "C:\Users\Public\UiPilot_tools Protected"'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardError = $true
$process = [System.Diagnostics.Process]::Start($start)
try {
    $errorText = $process.StandardError.ReadToEnd()
    if (-not $process.WaitForExit(10000)) { throw 'Read-only checker did not return in time.' }
    if ($process.ExitCode -eq 0 -or $errorText.Trim() -cne 'UnexpectedInstallDirectory') {
        throw 'User-writable alternate installation destination was not rejected.'
    }
}
finally { $process.Dispose() }
Write-Output 'PROTECTED_PACKAGE_CHECK_PASS'
