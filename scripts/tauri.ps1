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
    if ($env:UIPILOT_PROVIDER_PREPARED -ne '1') {
        & python (Join-Path $PSScriptRoot 'dev_uipilot.py') @($TauriArguments | Select-Object -Skip 1)
        exit $LASTEXITCODE
    }
    & python (Join-Path $PSScriptRoot 'provider_build.py') validate-dev
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $devPort = Get-UiPilotAvailableDevPort -StartPort 14321
    $env:UIPILOT_DEV_PORT = [string]$devPort
    $baseConfig = Get-Content -LiteralPath (Join-Path $repositoryRoot 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
    $devResources = @{}
    foreach ($resource in $baseConfig.bundle.resources) {
        if ($resource -notlike 'resources/provider/*') { $devResources[$resource] = $resource }
    }
    $devResources[$env:UIPILOT_SCREENSHOT_PROVIDER_EXE] = 'resources/provider/UIAutoma.UIAutomation.Provider.exe'
    $devManifest = Join-Path (Split-Path -Parent $env:UIPILOT_SCREENSHOT_PROVIDER_EXE) 'asset-manifest.json'
    $devResources[$devManifest] = 'resources/provider/asset-manifest.json'
    $configOverride = @{
        build = @{
            devUrl = "http://127.0.0.1:$devPort"
        }
        bundle = @{ resources = $devResources }
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
elseif ($TauriArguments.Count -gt 0 -and $TauriArguments[0] -in @('build', 'bundle')) {
    & python (Join-Path $PSScriptRoot 'provider_build.py') build-session -- @TauriArguments
}
else {
    & $tauriPath @TauriArguments
}

exit $LASTEXITCODE
