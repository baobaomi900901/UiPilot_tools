$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-Condition {
    param(
        [Parameter(Mandatory = $true)]
        [bool]$Condition,

        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

$runtimePath = Join-Path $PSScriptRoot 'dev-everything-runtime.ps1'
Assert-Condition (Test-Path -LiteralPath $runtimePath -PathType Leaf) 'Dev Everything runtime helper is missing'
. $runtimePath

$processes = @(
    [pscustomobject]@{ ProcessName = 'Everything'; SessionId = 0; Id = 10 },
    [pscustomobject]@{ ProcessName = 'Everything'; SessionId = 4; Id = 20 },
    [pscustomobject]@{ ProcessName = 'Other'; SessionId = 4; Id = 30 }
)
$selected = Select-EverythingUserClient -Processes $processes -SessionId 4
Assert-Condition ($selected.Id -eq 20) 'Session 0 service must not replace the interactive client'
Assert-Condition ($null -eq (Select-EverythingUserClient -Processes $processes -SessionId 7)) 'Missing interactive client must return null'

$roots = @('C:\Program Files', 'C:\Program Files (x86)')
Assert-Condition (Test-ProtectedEverythingServicePath 'C:\Program Files\Everything\Everything.exe' $roots) 'Program Files service should pass'
Assert-Condition (-not (Test-ProtectedEverythingServicePath 'C:\Program Files-Evil\Everything.exe' $roots)) 'Prefix lookalike must fail'
Assert-Condition (-not (Test-ProtectedEverythingServicePath 'D:\code\UiPilot_tools\src-tauri\resources\everything\Everything.exe' $roots)) 'Repository service must fail'

$running = [pscustomobject]@{ Name = 'Everything'; State = 'Running'; ProcessId = 10 }
$serviceProcess = [pscustomobject]@{ ProcessId = 10; ExecutablePath = 'C:\Program Files\Everything\Everything.exe' }
Assert-Condition ($null -eq (Get-EverythingServiceFailure $running $serviceProcess $roots)) 'Protected running service should pass'
$runningWithPath = [pscustomobject]@{ Name = 'Everything'; State = 'Running'; ProcessId = 10; PathName = '"C:\Program Files\Everything\Everything.exe" -svc' }
$restrictedServiceProcess = [pscustomobject]@{ ProcessId = 10; ExecutablePath = '' }
Assert-Condition ($null -eq (Get-EverythingServiceFailure $runningWithPath $restrictedServiceProcess $roots)) 'Service PathName should be used when a normal user cannot read the LocalSystem process path'
Assert-Condition ((Get-EverythingServiceFailure $null $null $roots) -ceq 'EVERYTHING_SERVICE_MISSING') 'Missing service must be stable'
Assert-Condition ((Get-EverythingServiceFailure ([pscustomobject]@{ Name = 'Everything'; State = 'Stopped'; ProcessId = 0 }) $null $roots) -ceq 'EVERYTHING_SERVICE_NOT_RUNNING') 'Stopped service must be stable'

$mainPath = Join-Path $PSScriptRoot 'dev-with-everything.ps1'
$mainSource = Get-Content -LiteralPath $mainPath -Raw -Encoding utf8
Assert-Condition $mainSource.Contains(". (Join-Path `$PSScriptRoot 'dev-everything-runtime.ps1')") 'Dev script must load the runtime helper'
Assert-Condition (-not $mainSource.Contains("Get-Process -Name 'Everything' -ErrorAction SilentlyContinue | Select-Object -First 1")) 'Dev script must not confuse the service with an interactive client'
Assert-Condition (-not $mainSource.Contains('-Verb RunAs')) 'Dev startup must never elevate'

Write-Output 'DEV_EVERYTHING_RUNTIME_PASS'
