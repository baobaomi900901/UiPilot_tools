[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoManifest = Join-Path $repoRoot 'spikes/systemindex/Cargo.toml'
$spikeExe = Join-Path $repoRoot 'spikes/systemindex/target/release/systemindex-spike.exe'
$prepareScript = Join-Path $PSScriptRoot 'prepare-systemindex-sentinels.ps1'
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$evidenceDir = Join-Path $repoRoot "artifacts/systemindex-spike/io-$timestamp"
$columns = @('Time of Day', 'Process Name', 'PID', 'Operation', 'Path', 'Result', 'Detail')

function Exit-NotRunnable {
    param([Parameter(Mandatory)] [string] $Message)
    [Console]::Error.WriteLine("NOT_RUNNABLE: $Message")
    exit 2
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-WSearchSnapshot {
    $service = Get-CimInstance Win32_Service -Filter "Name='WSearch'" -ErrorAction Stop
    $delayed = (Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\WSearch' `
        -Name DelayedAutoStart -ErrorAction SilentlyContinue).DelayedAutoStart
    return [pscustomobject]@{
        StartMode = [string]$service.StartMode
        State = [string]$service.State
        DelayedAutoStart = if ($null -eq $delayed) { 0 } else { [int]$delayed }
    }
}

function Wait-WSearchState {
    param([Parameter(Mandatory)] [ValidateSet('Running', 'Stopped')] [string] $State)
    (Get-Service WSearch -ErrorAction Stop).WaitForStatus($State, [TimeSpan]::FromSeconds(30))
}

function Restore-WSearch {
    param([Parameter(Mandatory)] [object] $Prior)
    $current = Get-WSearchSnapshot
    if ($Prior.State -eq 'Running' -and $current.State -ne 'Running') {
        Start-Service WSearch -ErrorAction Stop
        Wait-WSearchState Running
    } elseif ($Prior.State -eq 'Stopped' -and $current.State -ne 'Stopped') {
        Stop-Service WSearch -Force -ErrorAction Stop
        Wait-WSearchState Stopped
    }
    $restored = Get-WSearchSnapshot
    if ($restored.State -ne $Prior.State -or
        $restored.StartMode -ne $Prior.StartMode -or
        $restored.DelayedAutoStart -ne $Prior.DelayedAutoStart) {
        throw 'Windows Search service state or start configuration was not restored exactly'
    }
    return $restored
}

function ConvertTo-WindowsArgument {
    param([AllowEmptyString()] [string] $Value)
    $builder = [Text.StringBuilder]::new()
    [void]$builder.Append([char]34)
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq [char]92) {
            $backslashes++
        } elseif ($character -eq [char]34) {
            [void]$builder.Append([char]92, (2 * $backslashes) + 1)
            [void]$builder.Append([char]34)
            $backslashes = 0
        } else {
            if ($backslashes -gt 0) { [void]$builder.Append([char]92, $backslashes) }
            [void]$builder.Append($character)
            $backslashes = 0
        }
    }
    if ($backslashes -gt 0) { [void]$builder.Append([char]92, 2 * $backslashes) }
    [void]$builder.Append([char]34)
    return $builder.ToString()
}

function Join-WindowsArguments {
    param([Parameter(Mandatory)] [string[]] $Arguments)
    return (($Arguments | ForEach-Object { ConvertTo-WindowsArgument $_ }) -join ' ')
}

function Get-ProcmonProcesses {
    return @(Get-Process -Name Procmon, Procmon64, Procmon64a -ErrorAction SilentlyContinue)
}

function Stop-ProcmonCapture {
    & $procmon /Terminate /Quiet | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "ProcMon /Terminate failed with exit $LASTEXITCODE" }
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ((Get-ProcmonProcesses).Count -gt 0 -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    if ((Get-ProcmonProcesses).Count -gt 0) {
        throw 'Process Monitor did not exit within 30 seconds; it was not force-killed'
    }
}

function Test-AllowedRead {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $CaseDirectory
    )
    if ([string]::Equals($Path, $spikeExe, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    $casePrefix = [IO.Path]::GetFullPath($CaseDirectory).TrimEnd('\') + '\'
    if ($Path.StartsWith($casePrefix, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    $windowsPrefix = [IO.Path]::GetFullPath($env:SystemRoot).TrimEnd('\') + '\'
    if ($Path.StartsWith($windowsPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        return [IO.Path]::GetExtension($Path).ToLowerInvariant() -in @('.dll', '.mui', '.nls', '.manifest')
    }
    return $false
}

function Get-Classification {
    param(
        [Parameter(Mandatory)] [object] $Row,
        [Parameter(Mandatory)] [string] $CaseDirectory
    )
    $operation = [string]$Row.Operation
    $path = [string]$Row.Path
    if ($operation -eq 'Process Create') { return @('process activity', $true, 'spike child process') }
    if ($operation -eq 'QueryDirectory') { return @('directory enumeration', $true, 'host directory enumeration') }
    if ($operation -eq 'ReadFile') {
        if (Test-AllowedRead -Path $path -CaseDirectory $CaseDirectory) {
            return @('executable/DLL load', $false, 'allowlisted startup or evidence read')
        }
        return @('file content read', $true, 'non-allowlisted host ReadFile')
    }
    if ($operation -eq 'Load Image') { return @('executable/DLL load', $false, '') }
    if ($operation.StartsWith('Reg', [StringComparison]::OrdinalIgnoreCase)) {
        return @('configuration', $false, '')
    }
    if ($operation -match '^(Process|Thread|CreateFileMapping|Load Image)') {
        return @('process/thread activity', $false, '')
    }
    if ($operation -match '^(TCP|UDP)') { return @('network activity', $false, '') }
    if ($operation -in @('DeviceIoControl', 'FileSystemControl')) {
        return @('index API side effect', $false, '')
    }
    if ($operation -match '^(CreateFile|CloseFile|Query|Set|LockFile|UnlockFile)') {
        return @('metadata read', $false, '')
    }
    if ($operation -eq 'WriteFile') {
        $casePrefix = [IO.Path]::GetFullPath($CaseDirectory).TrimEnd('\') + '\'
        if ($path.StartsWith($casePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            return @('configuration/evidence output', $false, '')
        }
        return @('file content write', $false, 'manual review required')
    }
    return @('other activity', $false, 'manual review required')
}

function Assert-SuccessCounters {
    param([Parameter(Mandatory)] [object] $Evidence)
    if ($Evidence.counters.searchFolderFactoryCreated -ne 1 -or
        $Evidence.counters.scopeSet -ne 1 -or
        $Evidence.counters.searchFolderEnumerated -ne 1) {
        throw 'Successful query did not cross each expected Search Folder boundary exactly once'
    }
}

function Assert-FunctionalResult {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [int] $ExitCode,
        [Parameter(Mandatory)] [string] $Stdout,
        [Parameter(Mandatory)] [string] $Stderr,
        [Parameter(Mandatory)] [string] $Literal,
        [Parameter(Mandatory)] [object] $Sentinels
    )
    if ($Name -eq 'C') {
        if ($ExitCode -ne 2) { throw "Case C expected exit 2, got $ExitCode" }
        $errorEvidence = $Stderr | ConvertFrom-Json
        if ($errorEvidence.counters.searchFolderFactoryCreated -ne 0 -or
            $errorEvidence.counters.scopeSet -ne 0 -or
            $errorEvidence.counters.searchFolderEnumerated -ne 0) {
            throw 'Case C crossed a Search Folder boundary while WSearch was stopped'
        }
        return
    }

    if ($ExitCode -ne 0) { throw "Case $Name expected exit 0, got $ExitCode`: $Stderr" }
    $evidence = $Stdout | ConvertFrom-Json
    Assert-SuccessCounters $evidence
    if (-not [string]::Equals([string]$evidence.literal, $Literal, [StringComparison]::Ordinal)) {
        throw "Case $Name did not preserve the literal exactly"
    }
    if ($Name -eq 'A') {
        $expected = [IO.Path]::GetFullPath([string]$Sentinels.indexed.fullPath)
        $found = @($evidence.items | Where-Object {
            [string]::Equals(
                [IO.Path]::GetFullPath([string]$_.parsingPath),
                $expected,
                [StringComparison]::OrdinalIgnoreCase
            )
        }).Count -gt 0
        if (-not $found) { throw 'Case A did not return the proven indexed sentinel path' }
    } elseif ($Name -eq 'B' -and @($evidence.items).Count -ne 0) {
        throw 'Case B unexpectedly returned a result for the proven unindexed sentinel'
    }
}

function Invoke-CaptureCase {
    param(
        [Parameter(Mandatory)] [ValidateSet('A', 'B', 'C', 'D')] [string] $Name,
        [Parameter(Mandatory)] [string] $Literal,
        [Parameter(Mandatory)] [object] $Sentinels,
        [object] $ServiceSnapshot
    )
    $caseDirectory = Join-Path $evidenceDir "case-$Name"
    [IO.Directory]::CreateDirectory($caseDirectory) | Out-Null
    $pml = Join-Path $caseDirectory "$Name.pml"
    $fullCsv = Join-Path $caseDirectory "$Name-full.csv"
    $filteredCsv = Join-Path $caseDirectory "$Name-filtered.csv"
    $classifiedCsv = Join-Path $caseDirectory "$Name-classified.csv"
    $forbiddenCsv = Join-Path $caseDirectory "$Name-forbidden.csv"
    $stdout = Join-Path $caseDirectory "$Name.stdout.json"
    $stderr = Join-Path $caseDirectory "$Name.stderr.json"
    $caseArguments = @('query', '--literal', $Literal, '--limit', '20', '--json')
    $caseArgs = Join-WindowsArguments $caseArguments
    [ordered]@{ case = $Name; arguments = $caseArguments } | ConvertTo-Json -Depth 3 |
        Set-Content -LiteralPath (Join-Path $caseDirectory "$Name-arguments.json") -Encoding utf8

    $errors = [Collections.Generic.List[string]]::new()
    $procmonStarted = $false
    $spike = $null
    $elapsed = [Diagnostics.Stopwatch]::new()
    try {
        Start-Process -FilePath $procmon -ArgumentList @(
            '/AcceptEula', '/Quiet', '/Minimized', '/BackingFile', $pml
        )
        $procmonStarted = $true
        & $procmon /WaitForIdle | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "ProcMon /WaitForIdle failed with exit $LASTEXITCODE" }

        if ($Name -eq 'C') {
            $ServiceSnapshot | ConvertTo-Json |
                Set-Content -LiteralPath (Join-Path $caseDirectory 'service-before.json') -Encoding utf8
            if ($ServiceSnapshot.State -eq 'Running') {
                Stop-Service WSearch -Force -ErrorAction Stop
                Wait-WSearchState Stopped
            } elseif ($ServiceSnapshot.State -ne 'Stopped') {
                throw "WSearch was in unsupported state $($ServiceSnapshot.State)"
            }
        }

        $elapsed.Start()
        $spike = Start-Process -FilePath $spikeExe -ArgumentList $caseArgs -Wait -PassThru `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $elapsed.Stop()
    } catch {
        $errors.Add($_.Exception.Message)
    } finally {
        if ($procmonStarted) {
            try { Stop-ProcmonCapture } catch { $errors.Add($_.Exception.Message) }
        }
        if ($Name -eq 'C' -and $ServiceSnapshot) {
            try {
                $restored = Restore-WSearch $ServiceSnapshot
                $restored | ConvertTo-Json |
                    Set-Content -LiteralPath (Join-Path $caseDirectory 'service-after.json') -Encoding utf8
            } catch {
                $errors.Add("RESTORATION_FAILED: $($_.Exception.Message)")
            }
        }
    }
    if ($errors.Count -gt 0) { throw ($errors -join '; ') }
    if (-not $spike) { throw "Case $Name did not start the spike process" }

    [ordered]@{
        case = $Name
        pid = $spike.Id
        exitCode = $spike.ExitCode
        elapsedMs = [math]::Round($elapsed.Elapsed.TotalMilliseconds, 3)
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $caseDirectory "$Name-exit.json") -Encoding utf8

    Start-Process -FilePath $procmon -ArgumentList @(
        '/AcceptEula', '/Quiet', '/OpenLog', $pml, '/SaveAs', $fullCsv
    ) -Wait
    if (-not (Test-Path -LiteralPath $pml -PathType Leaf) -or
        -not (Test-Path -LiteralPath $fullCsv -PathType Leaf)) {
        throw "Case $Name did not produce both PML and full CSV evidence"
    }

    $rows = @(Import-Csv -LiteralPath $fullCsv)
    if ($rows.Count -eq 0) { throw 'ProcMon CSV is empty' }
    $missing = $columns | Where-Object { $_ -notin $rows[0].PSObject.Properties.Name }
    if ($missing) { throw "ProcMon CSV columns missing: $($missing -join ', ')" }
    $filtered = @($rows | Where-Object { [int]$_.PID -eq $spike.Id } | Select-Object $columns)
    if ($filtered.Count -eq 0) { throw "Case $Name has no rows for spike PID $($spike.Id)" }
    $filtered | Export-Csv -LiteralPath $filteredCsv -NoTypeInformation -Encoding utf8

    $classified = @($filtered | ForEach-Object {
        $classification = Get-Classification -Row $_ -CaseDirectory $caseDirectory
        [pscustomobject][ordered]@{
            'Time of Day' = $_.'Time of Day'
            'Process Name' = $_.'Process Name'
            PID = $_.PID
            Operation = $_.Operation
            Path = $_.Path
            Result = $_.Result
            Detail = $_.Detail
            Classification = $classification[0]
            Forbidden = [bool]$classification[1]
            Reason = $classification[2]
        }
    })
    $classified | Export-Csv -LiteralPath $classifiedCsv -NoTypeInformation -Encoding utf8
    $forbidden = @($classified | Where-Object { $_.Forbidden })
    if ($forbidden.Count -gt 0) {
        $forbidden | Export-Csv -LiteralPath $forbiddenCsv -NoTypeInformation -Encoding utf8
    } else {
        '"Time of Day","Process Name","PID","Operation","Path","Result","Detail","Classification","Forbidden","Reason"' |
            Set-Content -LiteralPath $forbiddenCsv -Encoding utf8
    }

    if (@($filtered | Where-Object { $_.Operation -eq 'Process Create' }).Count -gt 0) {
        throw "Case $Name shows a child Process Create event for the spike PID"
    }
    Assert-FunctionalResult -Name $Name -ExitCode $spike.ExitCode `
        -Stdout (Get-Content -LiteralPath $stdout -Raw -Encoding utf8) `
        -Stderr (Get-Content -LiteralPath $stderr -Raw -Encoding utf8) `
        -Literal $Literal -Sentinels $Sentinels

    [ordered]@{
        case = $Name
        filteredRows = $filtered.Count
        forbiddenRows = $forbidden.Count
        queryDirectoryRows = @($filtered | Where-Object { $_.Operation -eq 'QueryDirectory' }).Count
        nonAllowlistedReadRows = @($forbidden | Where-Object { $_.Operation -eq 'ReadFile' }).Count
        requiresManualReview = $forbidden.Count -gt 0
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $caseDirectory "$Name-summary.json") -Encoding utf8
}

$os = Get-CimInstance Win32_OperatingSystem
if ($os.Caption -notlike '*Windows 11*' -or $os.OSArchitecture -notlike '*64*') {
    Exit-NotRunnable 'SystemIndex I/O capture requires Windows 11 x64'
}
if (-not (Test-IsAdministrator)) { Exit-NotRunnable 'elevated PowerShell is required' }
if ([string]::IsNullOrWhiteSpace($env:PROCMON64_EXE)) {
    Exit-NotRunnable 'PROCMON64_EXE is not set; WPR is not an automatic fallback'
}
try {
    $procmon = (Resolve-Path -LiteralPath $env:PROCMON64_EXE -ErrorAction Stop).Path
} catch {
    Exit-NotRunnable 'PROCMON64_EXE does not resolve to a file'
}
if (-not (Test-Path -LiteralPath $procmon -PathType Leaf) -or
    [IO.Path]::GetFileName($procmon) -ne 'Procmon64.exe') {
    Exit-NotRunnable 'PROCMON64_EXE must point to Procmon64.exe'
}
$versionText = (Get-Item -LiteralPath $procmon).VersionInfo.ProductVersion
$versionMatch = [regex]::Match([string]$versionText, '(?<!\d)(?<major>\d+)\.(?<minor>\d+)')
if (-not $versionMatch.Success -or
    [int]$versionMatch.Groups['major'].Value -ne 4 -or
    [int]$versionMatch.Groups['minor'].Value -ne 4) {
    Exit-NotRunnable "Procmon64.exe product version '$versionText' is not pinned v4.04"
}
$signature = Get-AuthenticodeSignature -LiteralPath $procmon
if ($signature.Status -ne 'Valid' -or
    -not $signature.SignerCertificate -or
    $signature.SignerCertificate.Subject -notmatch 'Microsoft') {
    Exit-NotRunnable 'Procmon64.exe does not have a valid Microsoft Authenticode signature'
}
if ((Get-ProcmonProcesses).Count -gt 0) {
    Exit-NotRunnable 'a Process Monitor instance is already running'
}

[IO.Directory]::CreateDirectory($evidenceDir) | Out-Null
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$disk = Get-CimInstance Win32_DiskDrive | Select-Object Model, MediaType, Size
[ordered]@{
    capturedAt = (Get-Date).ToString('o')
    windows = [ordered]@{
        caption = $os.Caption
        version = $os.Version
        build = $os.BuildNumber
        architecture = $os.OSArchitecture
    }
    cpu = $cpu.Name
    memoryBytes = [uint64]$os.TotalVisibleMemorySize * 1KB
    storage = @($disk)
    windowsSearch = Get-WSearchSnapshot
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidenceDir 'environment.json') -Encoding utf8
[ordered]@{
    source = 'https://learn.microsoft.com/sysinternals/downloads/procmon'
    productVersion = $versionText
    sha256 = (Get-FileHash -LiteralPath $procmon -Algorithm SHA256).Hash
    signatureStatus = [string]$signature.Status
    signerSubject = $signature.SignerCertificate.Subject
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidenceDir 'tool.json') -Encoding utf8

$cargo = (Get-Command cargo -ErrorAction Stop).Source
$buildOut = Join-Path $evidenceDir 'build.stdout.log'
$buildErr = Join-Path $evidenceDir 'build.stderr.log'
$build = Start-Process -FilePath $cargo -ArgumentList @(
    'build', '--release', '--manifest-path', $cargoManifest
) -Wait -PassThru -RedirectStandardOutput $buildOut -RedirectStandardError $buildErr
if ($build.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $spikeExe -PathType Leaf)) {
    [Console]::Error.WriteLine("Spike release build failed; see $buildErr")
    exit 1
}
$spikeExe = (Resolve-Path -LiteralPath $spikeExe).Path

$sentinelManifest = $null
$finalExit = 0
try {
    $sentinelManifest = & $prepareScript -Create
    $prepareExit = $LASTEXITCODE
    if ($prepareExit -eq 2) { throw [InvalidOperationException]::new('NOT_RUNNABLE: sentinel preparation failed') }
    if ($prepareExit -ne 0 -or -not $sentinelManifest) { throw 'Sentinel preparation failed' }
    $sentinelManifest = ([string]$sentinelManifest).Trim()
    $sentinels = Get-Content -LiteralPath $sentinelManifest -Raw -Encoding utf8 | ConvertFrom-Json

    Invoke-CaptureCase -Name A -Literal ([string]$sentinels.indexed.fileName) -Sentinels $sentinels
    Invoke-CaptureCase -Name B -Literal ([string]$sentinels.unindexed.fileName) -Sentinels $sentinels
    $serviceSnapshot = Get-WSearchSnapshot
    Invoke-CaptureCase -Name C -Literal 'uipilot-index-service-off-proof' `
        -Sentinels $sentinels -ServiceSnapshot $serviceSnapshot
    $literalD = "'" + '"' + '*?%_[]文件' + [char]::ConvertFromUtf32(0x1F600) + 'e' + [char]0x0301
    Invoke-CaptureCase -Name D -Literal $literalD -Sentinels $sentinels
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    $finalExit = if ($_.Exception.Message.StartsWith('NOT_RUNNABLE:')) { 2 } else { 1 }
} finally {
    if ($sentinelManifest) {
        & $prepareScript -Cleanup -Manifest $sentinelManifest
        if ($LASTEXITCODE -ne 0) {
            [Console]::Error.WriteLine('Sentinel cleanup failed')
            $finalExit = 1
        }
    }
}

if ($finalExit -ne 0) { exit $finalExit }
Write-Output ([IO.Path]::GetFullPath($evidenceDir))
exit 0
