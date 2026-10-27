param(
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'dev-everything-runtime.ps1')

$frontendPort = 14321
if (-not [string]::IsNullOrWhiteSpace($env:UIPILOT_DEV_PORT)) {
    if (-not [int]::TryParse($env:UIPILOT_DEV_PORT, [ref]$frontendPort) -or $frontendPort -lt 1 -or $frontendPort -gt 65535) {
        throw "UIPILOT_DEV_PORT must be an integer between 1 and 65535; received '$($env:UIPILOT_DEV_PORT)'."
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'Everything dev startup requires Windows.'
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$service = Get-CimInstance -ClassName Win32_Service -Filter "Name='Everything'" -ErrorAction SilentlyContinue
$serviceProcess = $null
if ($null -ne $service -and [uint32]$service.ProcessId -ne 0) {
    $serviceProcess = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$([uint32]$service.ProcessId)" -ErrorAction SilentlyContinue
}
$programFilesRoots = @(
    $env:ProgramFiles,
    ${env:ProgramFiles(x86)}
) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
$serviceFailure = Get-EverythingServiceFailure -Service $service -ServiceProcess $serviceProcess -ProgramFilesRoots $programFilesRoots
if ($null -ne $serviceFailure) {
    $message = switch ($serviceFailure) {
        'EVERYTHING_SERVICE_MISSING' { 'Everything Service is required for normal-user dev. Install the official Everything Service once, then rerun npm run tauri dev.' }
        'EVERYTHING_SERVICE_NOT_RUNNING' { 'Everything Service is installed but not running. Start it, then rerun npm run tauri dev.' }
        'EVERYTHING_SERVICE_PROCESS_UNAVAILABLE' { 'Everything Service process information is unavailable. Repair the official Everything installation, then rerun npm run tauri dev.' }
        'EVERYTHING_SERVICE_PATH_UNSAFE' { 'Everything Service must run from a protected Program Files directory. Reinstall it with the official Everything installer.' }
        default { 'Everything Service validation failed.' }
    }
    throw "$serviceFailure`: $message"
}

$exitCode = 0

if (-not $SkipFrontend) {
    $vitePath = Join-Path $repositoryRoot 'node_modules\.bin\vite.cmd'
    if (-not (Test-Path -LiteralPath $vitePath -PathType Leaf)) {
        throw 'Vite is not installed. Run npm.cmd ci before starting dev.'
    }
    & $vitePath '--host' '127.0.0.1' '--port' ([string]$frontendPort) '--strictPort'
    $exitCode = $LASTEXITCODE
}

exit $exitCode
