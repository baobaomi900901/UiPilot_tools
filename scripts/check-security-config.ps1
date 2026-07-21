$ErrorActionPreference = 'Stop'

function Assert-ExactProperties {
  param(
    [Parameter(Mandatory)] [object] $Object,
    [Parameter(Mandatory)] [string[]] $Names,
    [Parameter(Mandatory)] [string] $Label
  )

  $actual = @($Object.PSObject.Properties.Name) | Sort-Object
  $expected = @($Names) | Sort-Object
  if (Compare-Object $expected $actual) {
    throw "$Label properties differ from the exact allowlist"
  }
}

$config = Get-Content "$PSScriptRoot/../src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$probeConfig = Get-Content "$PSScriptRoot/../src-tauri/tauri.security-probe.conf.json" -Raw | ConvertFrom-Json
$capabilityDirectory = "$PSScriptRoot/../src-tauri/capabilities"
$capabilityFiles = @(
  Get-ChildItem $capabilityDirectory -Recurse -File |
    Where-Object { $_.Extension -in @('.json', '.json5', '.toml') }
)
$expectedCapability = [IO.Path]::GetFullPath((Join-Path $capabilityDirectory 'main.json'))
if (
  $capabilityFiles.Count -ne 1 -or
  -not [string]::Equals($capabilityFiles[0].FullName, $expectedCapability, [StringComparison]::OrdinalIgnoreCase)
) {
  throw 'Exactly one main capability file is allowed'
}
$capability = Get-Content $capabilityFiles[0].FullName -Raw | ConvertFrom-Json

Assert-ExactProperties $config @('$schema', 'productName', 'version', 'identifier', 'build', 'app', 'bundle') 'Tauri root'
Assert-ExactProperties $config.build @('frontendDist', 'devUrl', 'beforeDevCommand', 'beforeBuildCommand') 'Tauri build'
Assert-ExactProperties $config.app @('withGlobalTauri', 'windows', 'security') 'Tauri app'
if ($config.app.windows.Count -ne 1) {
  throw 'Exactly one main window is allowed'
}
Assert-ExactProperties $config.app.windows[0] @('label', 'title', 'width', 'height', 'visible', 'decorations', 'resizable', 'fullscreen') 'Main window'
Assert-ExactProperties $config.app.security @('csp') 'Tauri security'
Assert-ExactProperties $config.bundle @('active', 'targets', 'icon', 'android') 'Tauri bundle'
Assert-ExactProperties $config.bundle.android @('debugApplicationIdSuffix') 'Tauri Android bundle'
Assert-ExactProperties $capability @('$schema', 'identifier', 'description', 'windows', 'permissions') 'Main capability'
Assert-ExactProperties $probeConfig @('$schema', 'build') 'Security probe override'
Assert-ExactProperties $probeConfig.build @('beforeBuildCommand') 'Security probe build'

$expectedCsp = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'"
if (
  $config.'$schema' -ne '../node_modules/@tauri-apps/cli/config.schema.json' -or
  $config.productName -ne 'UiPilot' -or
  $config.version -ne '0.1.0' -or
  $config.identifier -ne 'com.uipilot.launcher' -or
  $config.build.frontendDist -ne '../dist' -or
  $config.build.devUrl -ne 'http://localhost:1420' -or
  $config.build.beforeDevCommand -ne 'npm run dev' -or
  $config.build.beforeBuildCommand -ne 'npm run build' -or
  $config.app.withGlobalTauri -ne $false -or
  $config.app.security.csp -ne $expectedCsp -or
  $config.bundle.active -ne $true -or
  $config.bundle.targets -ne 'all' -or
  $config.bundle.android.debugApplicationIdSuffix -ne '.debug'
) {
  throw 'Tauri configuration values differ from the exact allowlist'
}

$window = $config.app.windows[0]
if (
  $window.label -ne 'main' -or
  $window.title -ne 'UiPilot' -or
  $window.width -ne 720 -or
  $window.height -ne 420 -or
  $window.visible -ne $false -or
  $window.decorations -ne $false -or
  $window.resizable -ne $false -or
  $window.fullscreen -ne $false
) {
  throw 'Main window values differ from the exact allowlist'
}

$expectedIcons = @(
  'icons/32x32.png',
  'icons/128x128.png',
  'icons/128x128@2x.png',
  'icons/icon.icns',
  'icons/icon.ico'
)
if (Compare-Object $expectedIcons @($config.bundle.icon)) {
  throw 'Bundle icons differ from the exact allowlist'
}

if (
  $capability.'$schema' -ne '../gen/schemas/desktop-schema.json' -or
  $capability.identifier -ne 'main-capability' -or
  $capability.description -ne 'Exact permissions for the local launcher WebView' -or
  $capability.windows.Count -ne 1 -or
  $capability.windows[0] -ne 'main' -or
  $probeConfig.'$schema' -ne '../node_modules/@tauri-apps/cli/config.schema.json' -or
  $probeConfig.build.beforeBuildCommand -ne 'npm run build -- --mode security-probe'
) {
  throw 'Capability or probe values differ from the exact allowlist'
}

$expectedPermissions = @(
  'allow-search-apps',
  'allow-search-files',
  'allow-execute-result',
  'allow-load-settings',
  'allow-save-settings',
  'allow-set-file-preview-preference',
  'allow-rescan-apps',
  'allow-export-validation-data',
  'allow-clear-validation-data',
  'allow-hide-launcher',
  'core:event:allow-listen',
  'core:event:allow-unlisten'
) | Sort-Object
if ($capability.permissions | Where-Object { $_ -isnot [string] }) {
  throw 'Scoped permission objects are not allowed'
}
$actualPermissions = @($capability.permissions) | Sort-Object
if (Compare-Object $expectedPermissions $actualPermissions) {
  throw 'Capability permission set differs from the exact allowlist'
}
Write-Output 'security config ok'
