[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$scanRoot = Join-Path $tempRoot ("uipilot-boundary-scan-" + [Guid]::NewGuid())
$outsideRoot = Join-Path $tempRoot ("uipilot-boundary-outside-" + [Guid]::NewGuid())
$userRoot = Join-Path $scanRoot 'user'
$commonRoot = Join-Path $scanRoot 'common'
$junction = Join-Path $userRoot 'outside-link'
$sentinel = Join-Path $outsideRoot 'Outside.lnk'

function Assert-TemporaryChild([string] $Path) {
  $fullPath = [IO.Path]::GetFullPath($Path)
  $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing cleanup outside the temporary directory"
  }
}

try {
  New-Item -ItemType Directory -Path $userRoot, $commonRoot, $outsideRoot | Out-Null
  Set-Content -LiteralPath $sentinel -Value '' -NoNewline
  New-Item -ItemType Junction -Path $junction -Target $outsideRoot | Out-Null

  $env:UIPILOT_TEST_USER_ROOT = $userRoot
  $env:UIPILOT_TEST_COMMON_ROOT = $commonRoot
  $env:UIPILOT_TEST_OUTSIDE_SENTINEL = $sentinel

  cargo test --manifest-path src-tauri/Cargo.toml `
    apps::discovery::tests::junction_does_not_escape_injected_root `
    -- --ignored --exact
  if ($LASTEXITCODE -ne 0) {
    throw 'Start Menu junction boundary test failed'
  }
}
finally {
  Remove-Item Env:UIPILOT_TEST_USER_ROOT -ErrorAction SilentlyContinue
  Remove-Item Env:UIPILOT_TEST_COMMON_ROOT -ErrorAction SilentlyContinue
  Remove-Item Env:UIPILOT_TEST_OUTSIDE_SENTINEL -ErrorAction SilentlyContinue

  Assert-TemporaryChild $scanRoot
  Assert-TemporaryChild $outsideRoot
  if (Test-Path -LiteralPath $junction) {
    [IO.Directory]::Delete($junction, $false)
  }
  if (Test-Path -LiteralPath $scanRoot) {
    Remove-Item -LiteralPath $scanRoot -Recurse -Force
  }
  if (Test-Path -LiteralPath $outsideRoot) {
    Remove-Item -LiteralPath $outsideRoot -Recurse -Force
  }
}

Write-Output 'start menu boundary ok'
