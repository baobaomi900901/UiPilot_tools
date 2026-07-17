$ErrorActionPreference = 'Stop'
$config = Get-Content "$PSScriptRoot/../src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$capabilityFiles = @(Get-ChildItem "$PSScriptRoot/../src-tauri/capabilities" -File -Filter '*.json')
if ($capabilityFiles.Count -ne 1 -or $capabilityFiles[0].Name -ne 'main.json') {
  throw 'Exactly one main capability file is allowed'
}
$capability = Get-Content $capabilityFiles[0].FullName -Raw | ConvertFrom-Json

if ($config.app.security.csp -ne "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'") {
  throw 'Unexpected CSP'
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
