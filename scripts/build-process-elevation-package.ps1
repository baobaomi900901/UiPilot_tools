param(
    [ValidateSet('Check', 'Build')]
    [string]$Mode = 'Check'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$packageRoot = Split-Path -Parent $PSScriptRoot
$profilePath = Join-Path $packageRoot 'src-tauri/tauri.elevation.conf.json'
$profile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json
if ($profile.bundle.windows.nsis.installMode -cne 'perMachine' -or
    @($profile.bundle.targets).Count -ne 1 -or $profile.bundle.targets[0] -cne 'nsis' -or
    $profile.identifier -cne 'com.uipilot.launcher.protected' -or
    $profile.productName -cne 'UiPilot_tools Protected' -or
    $profile.bundle.windows.nsis.installerHooks -cne './windows/elevation-installer-hooks.nsh') {
    throw 'Protected package profile does not match its fixed installation policy.'
}
$base = Get-Content -Raw -LiteralPath (Join-Path $packageRoot 'src-tauri/tauri.conf.json') | ConvertFrom-Json
$windowsConfig = Get-Content -Raw -LiteralPath (Join-Path $packageRoot 'src-tauri/tauri.windows.conf.json') | ConvertFrom-Json
$inventory = @(
    foreach ($relative in @('src-tauri/tauri.conf.json', 'src-tauri/tauri.windows.conf.json', 'src-tauri/tauri.elevation.conf.json',
        'src-tauri/windows/elevation-installer-hooks.nsh')) {
        $path = Join-Path $packageRoot $relative
        [pscustomobject]@{ path = $relative; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    foreach ($resource in $windowsConfig.bundle.resources) {
        $path = Join-Path $packageRoot "src-tauri/$resource"
        [pscustomobject]@{ path = "src-tauri/$resource"; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
)
if ($Mode -eq 'Check') {
    [pscustomobject]@{
        profile = 'protected-nsis'
        processElevationAvailable = $false
        note = 'Installation profile only. Helper and trusted runtime manifest await Stage 2D implementation.'
        inputs = $inventory
    } | ConvertTo-Json -Depth 5
    return
}

# Build generates an installer artifact; this script never starts it.
Push-Location $packageRoot
try {
    & cargo build --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline --release
    if ($LASTEXITCODE -ne 0) { throw 'Installation checker build failed.' }
    $tauri = Join-Path $packageRoot 'node_modules/.bin/tauri.cmd'
    if (-not (Test-Path -LiteralPath $tauri -PathType Leaf)) { throw 'Install locked Node dependencies with npm.cmd ci first.' }
    & $tauri build --target x86_64-pc-windows-msvc --bundles nsis --config $profilePath -- --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Protected NSIS build failed.' }
    $bundlePath = Join-Path $packageRoot 'src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis'
    $installer = Join-Path $bundlePath "$($profile.productName)_$($base.version)_x64-setup.exe"
    $hostBinary = Join-Path $packageRoot 'src-tauri/target/x86_64-pc-windows-msvc/release/UiPilot_tools.exe'
    $checker = Join-Path $packageRoot 'spikes/process-elevation-feasibility/target/release/uipilot-elevation-install-check.exe'
    $artifacts = @(foreach ($file in @($installer, $hostBinary, $checker)) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Expected build artifact missing: $file" }
        [pscustomobject]@{ artifact = $file; sha256 = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $report = [pscustomobject]@{
        profile = 'protected-nsis'
        processElevationAvailable = $false
        inputs = $inventory
        artifacts = $artifacts
    } | ConvertTo-Json -Depth 5
    $reportPath = Join-Path $bundlePath 'protected-package-inventory.json'
    [System.IO.File]::WriteAllText($reportPath, $report, [System.Text.UTF8Encoding]::new($false))
    Write-Output $reportPath
}
finally { Pop-Location }
