$ErrorActionPreference = 'Stop'
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

if ($config.app.security.csp -ne "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'") {
  throw 'Unexpected CSP'
}
if ($config.app.security.PSObject.Properties['capabilities']) {
  throw 'Inline or explicitly enabled capabilities are not allowed'
}
$expectedProbeProperties = @('$schema', 'build') | Sort-Object
$actualProbeProperties = @($probeConfig.PSObject.Properties.Name) | Sort-Object
if (Compare-Object $expectedProbeProperties $actualProbeProperties) {
  throw 'Security probe override contains unexpected configuration'
}
$probeBuildProperties = @($probeConfig.build.PSObject.Properties.Name)
if (
  $probeBuildProperties.Count -ne 1 -or
  $probeBuildProperties[0] -ne 'beforeBuildCommand' -or
  $probeConfig.build.beforeBuildCommand -ne 'npm run build -- --mode security-probe'
) {
  throw 'Unexpected security probe build configuration'
}
if ($config.app.windows.Count -ne 1 -or $config.app.windows[0].label -ne 'main') {
  throw 'Only the main WebView is allowed'
}
if ($capability.windows.Count -ne 1 -or $capability.windows[0] -ne 'main') {
  throw 'Capability must target only the main window'
}
$expectedPermissions = @(
  'allow-search-apps',
  'allow-execute-result',
  'allow-load-settings',
  'allow-save-settings',
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
