param(
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'Everything dev startup requires Windows.'
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$candidates = @(
    (Join-Path $repositoryRoot 'src-tauri\resources\everything\Everything.exe'),
    (Join-Path $repositoryRoot 'third-party\everything\Everything.exe')
)
$everythingPath = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($everythingPath)) {
    throw "Everything.exe was not found. Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-everything.ps1"
}

$existingProcess = Get-Process -Name 'Everything' -ErrorAction SilentlyContinue | Select-Object -First 1
$ownedProcess = $null
if ($null -eq $existingProcess) {
    $ownedProcess = Start-Process -FilePath $everythingPath -ArgumentList @('-startup') -WorkingDirectory (Split-Path -Parent $everythingPath) -PassThru
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($ownedProcess.HasExited) {
            throw "Everything.exe exited during startup with code $($ownedProcess.ExitCode)."
        }
        Start-Sleep -Milliseconds 250
    }
}

$exitCode = 0
try {
    if (-not $SkipFrontend) {
        $vitePath = Join-Path $repositoryRoot 'node_modules\.bin\vite.cmd'
        if (-not (Test-Path -LiteralPath $vitePath -PathType Leaf)) {
            throw 'Vite is not installed. Run npm.cmd ci before starting dev.'
        }
        & $vitePath '--port' '1420'
        $exitCode = $LASTEXITCODE
    }
}
finally {
    if ($null -ne $ownedProcess) {
        $runningProcess = Get-Process -Id $ownedProcess.Id -ErrorAction SilentlyContinue
        if ($null -ne $runningProcess) {
            Stop-Process -Id $ownedProcess.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

exit $exitCode
