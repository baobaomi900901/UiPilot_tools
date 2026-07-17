$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path "$PSScriptRoot/..").Path

Push-Location $repoRoot
try {
  $npm = (Get-Command npm.cmd -ErrorAction Stop).Source
  $build = Start-Process -FilePath $npm -ArgumentList @(
    'run',
    'tauri',
    'build',
    '--',
    '--no-bundle',
    '--features',
    'test-instrumentation',
    '--config',
    'src-tauri/tauri.security-probe.conf.json'
  ) -NoNewWindow -Wait -PassThru
  if ($build.ExitCode -ne 0) {
    throw "Security probe build failed with exit code $($build.ExitCode)"
  }

  $metadata = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0) {
    throw 'Could not resolve Cargo target directory'
  }
  $executable = Join-Path $metadata.target_directory 'release/uipilot.exe'
  if (-not (Test-Path -LiteralPath $executable)) {
    throw 'Security probe executable was not produced'
  }

  Write-Output (Resolve-Path -LiteralPath $executable).Path
}
finally {
  Pop-Location
}
