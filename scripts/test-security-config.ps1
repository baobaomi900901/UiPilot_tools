$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path "$PSScriptRoot/..").Path
$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$fixtureRoot = Join-Path $tempBase "uipilot-security-config-$([guid]::NewGuid().ToString('N'))"

function Invoke-FixtureCheck {
  $previousPreference = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  $output = & powershell -NoProfile -ExecutionPolicy Bypass -File "$fixtureRoot/scripts/check-security-config.ps1" 2>&1
  $exitCode = $LASTEXITCODE
  $ErrorActionPreference = $previousPreference
  [pscustomobject]@{ ExitCode = $exitCode; Output = $output }
}

New-Item -ItemType Directory -Path "$fixtureRoot/scripts", "$fixtureRoot/src-tauri/capabilities" | Out-Null
try {
  Copy-Item "$repoRoot/scripts/check-security-config.ps1" "$fixtureRoot/scripts/check-security-config.ps1"
  Copy-Item "$repoRoot/src-tauri/tauri.conf.json" "$fixtureRoot/src-tauri/tauri.conf.json"
  Copy-Item "$repoRoot/src-tauri/capabilities/main.json" "$fixtureRoot/src-tauri/capabilities/main.json"

  $baseline = Invoke-FixtureCheck
  if ($baseline.ExitCode -ne 0) {
    throw "Baseline fixture failed: $($baseline.Output)"
  }

  $nestedDirectory = New-Item -ItemType Directory -Path "$fixtureRoot/src-tauri/capabilities/nested"
  @'
identifier = "extra-capability"
windows = ["main"]
permissions = ["core:default"]
'@ | Set-Content -Encoding utf8 "$nestedDirectory/extra.toml"
  $tomlResult = Invoke-FixtureCheck
  if ($tomlResult.ExitCode -eq 0) {
    throw 'Nested TOML capability was not rejected'
  }
  Remove-Item -LiteralPath "$nestedDirectory/extra.toml"

  $configPath = "$fixtureRoot/src-tauri/tauri.conf.json"
  $config = Get-Content $configPath -Raw | ConvertFrom-Json
  $inlineCapability = [pscustomobject]@{
    identifier = 'inline-extra'
    windows = @('main')
    permissions = @('core:default')
  }
  $config.app.security | Add-Member -NotePropertyName capabilities -NotePropertyValue @($inlineCapability)
  $config | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 $configPath
  $inlineResult = Invoke-FixtureCheck
  if ($inlineResult.ExitCode -eq 0) {
    throw 'Inline capability was not rejected'
  }

  Write-Output 'security config regression tests ok'
}
finally {
  if (Test-Path -LiteralPath $fixtureRoot) {
    $resolvedFixture = (Resolve-Path -LiteralPath $fixtureRoot).Path
    $expectedPrefix = "$tempBase\uipilot-security-config-"
    if (-not $resolvedFixture.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
      throw "Refusing to remove unexpected fixture path: $resolvedFixture"
    }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
  }
}
