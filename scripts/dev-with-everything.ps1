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
$candidates = @(
    (Join-Path $repositoryRoot 'src-tauri\resources\everything\Everything.exe'),
    (Join-Path $repositoryRoot 'third-party\everything\Everything.exe')
)
$everythingPath = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($everythingPath)) {
    throw "Everything.exe was not found. Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-everything.ps1"
}

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

$currentSessionId = (Get-Process -Id $PID).SessionId
$everythingProcesses = @(Get-Process -Name 'Everything' -ErrorAction SilentlyContinue)
$existingProcess = Select-EverythingUserClient -Processes $everythingProcesses -SessionId $currentSessionId
$ownedProcess = $null
if ($null -eq $existingProcess) {
    $ownedProcess = Start-Process -FilePath $everythingPath -ArgumentList @('-startup') -WorkingDirectory (Split-Path -Parent $everythingPath) -PassThru
    $clientProcess = $ownedProcess
}
else {
    $clientProcess = $existingProcess
}

if (-not ('UiPilotEverythingWindow' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class UiPilotEverythingWindow
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr FindWindow(string className, string windowName);
}
'@
}

$ipcReady = $false
for ($attempt = 0; $attempt -lt 40; $attempt++) {
    $runningClient = Get-Process -Id $clientProcess.Id -ErrorAction SilentlyContinue
    if ($null -eq $runningClient) {
        if ($null -ne $ownedProcess) {
            throw "Everything.exe exited during startup with code $($ownedProcess.ExitCode)."
        }
        throw 'The existing Everything client exited during startup.'
    }
    if ([UiPilotEverythingWindow]::FindWindow('EVERYTHING_TASKBAR_NOTIFICATION', $null) -ne [IntPtr]::Zero) {
        $ipcReady = $true
        break
    }
    Start-Sleep -Milliseconds 250
}
if (-not $ipcReady) {
    throw 'Everything IPC did not become ready within 10 seconds.'
}

$exitCode = 0
try {
    if (-not $SkipFrontend) {
        $vitePath = Join-Path $repositoryRoot 'node_modules\.bin\vite.cmd'
        if (-not (Test-Path -LiteralPath $vitePath -PathType Leaf)) {
            throw 'Vite is not installed. Run npm.cmd ci before starting dev.'
        }
        & $vitePath '--host' '127.0.0.1' '--port' ([string]$frontendPort) '--strictPort'
        $exitCode = $LASTEXITCODE
    }
}
finally {
    if ($null -ne $ownedProcess) {
        $runningProcess = Get-Process -Id $ownedProcess.Id -ErrorAction SilentlyContinue
        if ($null -ne $runningProcess) {
            Start-Process -FilePath $everythingPath -ArgumentList @('-quit') -WorkingDirectory (Split-Path -Parent $everythingPath) -Wait -WindowStyle Hidden
            for ($attempt = 0; $attempt -lt 20; $attempt++) {
                if ($null -eq (Get-Process -Id $ownedProcess.Id -ErrorAction SilentlyContinue)) {
                    break
                }
                Start-Sleep -Milliseconds 100
            }
            if ($null -ne (Get-Process -Id $ownedProcess.Id -ErrorAction SilentlyContinue)) {
                Stop-Process -Id $ownedProcess.Id -Force -ErrorAction SilentlyContinue
            }
        }
    }
}

exit $exitCode
