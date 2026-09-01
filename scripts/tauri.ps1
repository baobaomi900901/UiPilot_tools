param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArguments
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'dev-port-runtime.ps1')

if ($null -eq $TauriArguments) {
    $TauriArguments = @()
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$tauriPath = Join-Path $repositoryRoot 'node_modules\.bin\tauri.cmd'
if (-not (Test-Path -LiteralPath $tauriPath -PathType Leaf)) {
    throw 'Tauri CLI is not installed. Run npm.cmd ci before starting dev.'
}

if ($TauriArguments.Count -gt 0 -and $TauriArguments[0] -eq 'dev') {
    $devPort = Get-UiPilotAvailableDevPort -StartPort 14321
    $env:UIPILOT_DEV_PORT = [string]$devPort
    $configOverride = @{
        build = @{
            devUrl = "http://127.0.0.1:$devPort"
        }
    } | ConvertTo-Json -Compress
    $configPath = Join-Path ([System.IO.Path]::GetTempPath()) "uipilot-tauri-dev-$PID.json"
    [System.IO.File]::WriteAllText($configPath, $configOverride, [System.Text.UTF8Encoding]::new($false))

    Write-Output "Using development port $devPort."

    $remainingArguments = @($TauriArguments | Select-Object -Skip 1)
    try {
        & $tauriPath 'dev' '--config' $configPath @remainingArguments
    }
    finally {
        Remove-Item -LiteralPath $configPath -Force -ErrorAction SilentlyContinue
    }
}
else {
    & $tauriPath @TauriArguments
}

exit $LASTEXITCODE
