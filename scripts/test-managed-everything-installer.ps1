param(
    [ValidateSet('Source', 'SelfTest', 'VmInstall', 'VmUpdate', 'VmCancelBeforeInstall', 'VmSourceReplacement', 'VmFaultCheckpoint', 'VmUninstallCheckpoint', 'VmConcurrency', 'VmExplicitUninstall')]
    [string]$Mode = 'Source',
    [string]$VmSetupPath,
    [string]$DisposableVmMarker,
    [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')]
    [string]$EvidenceName,
    [ValidateSet('Manual', 'TauriUpdate', 'MaintenanceUninstall')]
    [string]$SetupFlow = 'Manual',
    [ValidateSet(
        'bootstrap-root-created',
        'transaction-temp-created',
        'transaction-temp-flushed',
        'transaction-temp-renamed',
        'new-binary-verified',
        'old-service-stopped',
        'old-service-removed',
        'new-service-installed',
        'new-service-started',
        'runtime-manifest-replaced',
        'cleanup-complete',
        'rollback-started',
        'rollback-complete',
        'uninstall-referenced-directory-deleted',
        'uninstall-manifest-deleted'
    )]
    [string]$Checkpoint,
    [string]$VmTestRoot,
    [string]$InstallDirectory,
    [int]$ExpectedExitCode = 0
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-Condition {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-SourceContains {
    param([string]$Path, [string[]]$Patterns)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing required file: $Path" }
    $source = Get-Content -LiteralPath $Path -Raw -Encoding utf8
    foreach ($pattern in $Patterns) {
        if (-not $source.Contains($pattern)) { throw "Missing source contract '$pattern' in $Path" }
    }
}

function Assert-OrderedUniqueTokens {
    param([string]$Text, [string[]]$Tokens, [string]$Message)
    $previous = -1
    foreach ($token in $Tokens) {
        $index = $Text.IndexOf($token, [StringComparison]::Ordinal)
        if ($index -lt 0 -or $Text.LastIndexOf($token, [StringComparison]::Ordinal) -ne $index -or $index -le $previous) {
            throw $Message
        }
        $previous = $index
    }
}

function Get-LowerSha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return (($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) -join '') }
        finally { $sha.Dispose() }
    }
    finally { $stream.Dispose() }
}

function Get-PeMachine([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        try {
            Assert-Condition (($reader.ReadUInt16()) -eq 0x5a4d) "Not a PE image: $Path"
            $stream.Position = 0x3c
            $peOffset = $reader.ReadUInt32()
            $stream.Position = $peOffset
            Assert-Condition (($reader.ReadUInt32()) -eq 0x00004550) "Missing PE signature: $Path"
            return $reader.ReadUInt16()
        }
        finally { $reader.Dispose() }
    }
    finally { $stream.Dispose() }
}

$script:DisposableMarkerText = 'UIPILOT_DISPOSABLE_VM=I_UNDERSTAND_THIS_MACHINE_WILL_BE_MUTATED'

function Test-IsElevated {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Resolve-StrictLeaf([string]$Path, [string]$Description) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    Assert-Condition (-not $item.PSIsContainer) "$Description must be a file"
    Assert-Condition (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) "$Description must not be a reparse point"
    return $item.FullName
}

function Assert-DisposableMarker([string]$Path) {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($Path)) 'VM mode requires DisposableVmMarker'
    $marker = Resolve-StrictLeaf $Path 'Disposable VM marker'
    Assert-Condition ([IO.File]::ReadAllText($marker).Trim() -ceq $script:DisposableMarkerText) 'Disposable VM marker acknowledgement is invalid'
    return $marker
}

function Assert-VmAdmission {
    Assert-Condition (Test-IsElevated) 'VM mode requires an explicitly elevated administrator shell'
    return Assert-DisposableMarker $DisposableVmMarker
}

function Resolve-VmInstrumentedSetup([string]$RepositoryRoot) {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($VmSetupPath)) 'VM mode requires VmSetupPath'
    $setup = Resolve-StrictLeaf $VmSetupPath 'VM Instrumented Setup'
    $bundleRoot = Join-Path $RepositoryRoot 'src-tauri\target\vm-instrumented\release\bundle\nsis'
    $bundleItem = Get-Item -LiteralPath $bundleRoot -Force -ErrorAction Stop
    Assert-Condition $bundleItem.PSIsContainer 'VM Instrumented Setup bundle root is missing'
    Assert-Condition (($bundleItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'VM Instrumented Setup bundle root is a reparse point'
    Assert-Condition ((Split-Path -Parent $setup) -ceq $bundleItem.FullName) 'VM Setup must be the independently named instrumented bundle'
    Assert-Condition ((Split-Path -Leaf $setup) -like 'UiPilot_tools VM Instrumented Setup_*_x64-setup.exe') 'VM Setup has the wrong independent product name'
    Assert-Condition ((Get-PeMachine $setup) -eq 0x014c) 'VM Instrumented Setup is not an NSIS PE bootstrapper'
    return $setup
}

function Resolve-ScopedInstallDirectory {
    if ([string]::IsNullOrWhiteSpace($InstallDirectory)) { return $null }
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($VmTestRoot)) 'A custom install directory requires VmTestRoot'
    $testRoot = Get-Item -LiteralPath $VmTestRoot -Force -ErrorAction Stop
    Assert-Condition $testRoot.PSIsContainer 'VmTestRoot must be a directory'
    Assert-Condition (($testRoot.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'VmTestRoot must not be a reparse point'
    $candidate = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
    $prefix = $testRoot.FullName.TrimEnd('\') + '\'
    Assert-Condition ($candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) 'Custom install directory escaped VmTestRoot'
    $ancestor = Split-Path -Parent $candidate
    while (-not [string]::IsNullOrWhiteSpace($ancestor) -and -not (Test-Path -LiteralPath $ancestor)) {
        $ancestor = Split-Path -Parent $ancestor
    }
    $ancestorItem = Get-Item -LiteralPath $ancestor -Force -ErrorAction Stop
    Assert-Condition (($ancestorItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Custom install ancestor is a reparse point'
    Assert-Condition ($ancestorItem.FullName.StartsWith($testRoot.FullName, [StringComparison]::OrdinalIgnoreCase)) 'Custom install ancestor escaped VmTestRoot'
    return $candidate
}

function Assert-RedactedEvidence([string]$Json, [string]$RepositoryRoot) {
    foreach ($value in @($env:COMPUTERNAME, $env:USERNAME, $env:USERPROFILE, $RepositoryRoot)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            Assert-Condition (-not $Json.Contains($value)) 'Installer evidence contains machine, user, or path data'
        }
    }
    Assert-Condition ($Json -notmatch '(?i)S-1-\d+(?:-\d+){1,14}') 'Installer evidence contains SID data'
    Assert-Condition ($Json -notmatch '(?i)[A-Z]:[\\/]') 'Installer evidence contains absolute path data'
    Assert-Condition ($Json -notmatch '(?i)"(?:query|search_text|result_path)"\s*:') 'Installer evidence contains search or result data'
}

function Write-InstallerVmEvidence([string]$RepositoryRoot, $Evidence) {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($EvidenceName)) 'VM mode requires EvidenceName'
    $json = $Evidence | ConvertTo-Json -Depth 6
    Assert-RedactedEvidence $json $RepositoryRoot
    $artifactRoot = Join-Path $RepositoryRoot 'artifacts\managed-everything'
    [IO.Directory]::CreateDirectory($artifactRoot) | Out-Null
    $destination = Join-Path $artifactRoot ("installer-{0}.json" -f $EvidenceName)
    [IO.File]::WriteAllText($destination, $json, [Text.UTF8Encoding]::new($false))
    Write-Output ("MANAGED_EVERYTHING_INSTALLER_EVIDENCE {0}" -f (Split-Path -Leaf $destination))
}

function Get-SetupArguments([string]$Flow, [string]$ScopedInstallDirectory) {
    $arguments = @()
    if ($Flow -ceq 'TauriUpdate') { $arguments += '/UPDATE' }
    if (-not [string]::IsNullOrWhiteSpace($ScopedInstallDirectory)) { $arguments += ("/D={0}" -f $ScopedInstallDirectory) }
    return $arguments
}

function Invoke-OneVmSetup([string]$Setup, [string[]]$Arguments, [string]$Marker, [string]$FaultCheckpoint) {
    $oldMarker = $env:UIPILOT_VM_DISPOSABLE_MARKER
    $oldCheckpoint = $env:UIPILOT_VM_FAULT_CHECKPOINT
    try {
        $env:UIPILOT_VM_DISPOSABLE_MARKER = $Marker
        if ([string]::IsNullOrWhiteSpace($FaultCheckpoint)) {
            Remove-Item Env:UIPILOT_VM_FAULT_CHECKPOINT -ErrorAction SilentlyContinue
        } else {
            $env:UIPILOT_VM_FAULT_CHECKPOINT = $FaultCheckpoint
        }
        $null = @(& $Setup @Arguments 2>&1)
        return [int]$LASTEXITCODE
    }
    finally {
        if ($null -eq $oldMarker) { Remove-Item Env:UIPILOT_VM_DISPOSABLE_MARKER -ErrorAction SilentlyContinue } else { $env:UIPILOT_VM_DISPOSABLE_MARKER = $oldMarker }
        if ($null -eq $oldCheckpoint) { Remove-Item Env:UIPILOT_VM_FAULT_CHECKPOINT -ErrorAction SilentlyContinue } else { $env:UIPILOT_VM_FAULT_CHECKPOINT = $oldCheckpoint }
    }
}

function Invoke-InstallerSelfTest {
    $tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('uipilot-vm-guard-' + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($tempRoot) | Out-Null
    try {
        $marker = Join-Path $tempRoot 'marker.txt'
        [IO.File]::WriteAllText($marker, 'not-an-acknowledgement')
        $caught = $false
        try { Assert-DisposableMarker $marker | Out-Null } catch { $caught = $true }
        Assert-Condition $caught 'Disposable marker guard accepted invalid content'
        [IO.File]::WriteAllText($marker, $script:DisposableMarkerText)
        Assert-Condition ((Assert-DisposableMarker $marker) -ceq $marker) 'Disposable marker guard rejected exact acknowledgement'
        $caught = $false
        try { Resolve-VmInstrumentedSetup (Split-Path -Parent $PSScriptRoot) | Out-Null } catch { $caught = $true }
        Assert-Condition $caught 'VM Setup guard accepted a missing path'
        Write-Output 'MANAGED_EVERYTHING_INSTALLER_SELFTEST_PASS'
    }
    finally {
        if (Test-Path -LiteralPath $tempRoot -PathType Container) { Remove-Item -LiteralPath $tempRoot -Recurse -Force }
    }
}

function Invoke-InstallerVmMode([string]$RepositoryRoot) {
    $marker = Assert-VmAdmission
    $setup = Resolve-VmInstrumentedSetup $RepositoryRoot
    $scopedInstall = Resolve-ScopedInstallDirectory
    $flow = switch ($Mode) {
        'VmUpdate' { 'TauriUpdate' }
        'VmExplicitUninstall' { 'MaintenanceUninstall' }
        'VmUninstallCheckpoint' { 'MaintenanceUninstall' }
        default { $SetupFlow }
    }
    if ($Mode -ceq 'VmSourceReplacement') { Assert-Condition (-not [string]::IsNullOrWhiteSpace($scopedInstall)) 'VmSourceReplacement requires a scoped InstallDirectory' }
    if (@('VmFaultCheckpoint', 'VmUninstallCheckpoint') -contains $Mode) { Assert-Condition (-not [string]::IsNullOrWhiteSpace($Checkpoint)) "$Mode requires Checkpoint" }
    if ($Mode -ceq 'VmUninstallCheckpoint') { Assert-Condition $Checkpoint.StartsWith('uninstall-') 'VmUninstallCheckpoint accepts only uninstall checkpoints' }
    $arguments = Get-SetupArguments $flow $scopedInstall
    $setupHash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($Mode -ceq 'VmConcurrency') {
        $oldMarker = $env:UIPILOT_VM_DISPOSABLE_MARKER
        try {
            $env:UIPILOT_VM_DISPOSABLE_MARKER = $marker
            $first = if ($arguments.Count -eq 0) { Start-Process -FilePath $setup -PassThru } else { Start-Process -FilePath $setup -ArgumentList $arguments -PassThru }
            $second = if ($arguments.Count -eq 0) { Start-Process -FilePath $setup -PassThru } else { Start-Process -FilePath $setup -ArgumentList $arguments -PassThru }
            $first.WaitForExit()
            $second.WaitForExit()
            $codes = @([int]$first.ExitCode, [int]$second.ExitCode)
            $pass = @($codes | Where-Object { $_ -eq 0 }).Count -eq 1
            Write-InstallerVmEvidence $RepositoryRoot ([ordered]@{
                schema_version = 1; kind = 'installer-vm'; mode = $Mode.ToLowerInvariant(); outcome = $(if ($pass) { 'pass' } else { 'fail' })
                setup_sha256 = $setupHash; one_winner = $pass; first_exit_code = $codes[0]; second_exit_code = $codes[1]
            })
            Assert-Condition $pass 'VM concurrency mode did not produce exactly one winner'
            return
        }
        finally {
            if ($null -eq $oldMarker) { Remove-Item Env:UIPILOT_VM_DISPOSABLE_MARKER -ErrorAction SilentlyContinue } else { $env:UIPILOT_VM_DISPOSABLE_MARKER = $oldMarker }
        }
    }

    $fault = if (@('VmFaultCheckpoint', 'VmUninstallCheckpoint') -contains $Mode) { $Checkpoint } else { $null }
    $exitCode = Invoke-OneVmSetup $setup $arguments $marker $fault
    $pass = $exitCode -eq $ExpectedExitCode
    Write-InstallerVmEvidence $RepositoryRoot ([ordered]@{
        schema_version = 1; kind = 'installer-vm'; mode = $Mode.ToLowerInvariant(); flow = $flow.ToLowerInvariant()
        checkpoint = $fault; outcome = $(if ($pass) { 'pass' } else { 'fail' }); setup_sha256 = $setupHash
        expected_exit_code = $ExpectedExitCode; actual_exit_code = $exitCode
    })
    if (-not $pass) {
        if ($exitCode -ne 0) { exit $exitCode }
        exit 1
    }
}

$root = Split-Path -Parent $PSScriptRoot

if ($Mode -ceq 'SelfTest') {
    Invoke-InstallerSelfTest
    exit 0
}
if ($Mode.StartsWith('Vm', [StringComparison]::Ordinal)) {
    Invoke-InstallerVmMode $root
    exit 0
}

$helperRoot = Join-Path $root 'src-tauri\src\bin\uipilot-everything-installer'
$template = Join-Path $root 'src-tauri\windows\installer.nsi'
$hooks = Join-Path $root 'src-tauri\windows\nsis-hooks.nsh'
$build = Join-Path $root 'scripts\build-managed-everything-installer.ps1'
$tauri = Join-Path $root 'src-tauri\tauri.conf.json'
$vmTauri = Join-Path $root 'src-tauri\tauri.vm-instrumented.conf.json'
$runtimeGate = Join-Path $root 'scripts\test-managed-everything-runtime.ps1'
$releaseGate = Join-Path $root 'scripts\test-managed-everything-release.ps1'
$vmMatrix = Join-Path $root 'docs\testing\managed-everything-vm-matrix.md'

Assert-SourceContains $runtimeGate @(
    "[ValidateSet('Source', 'SelfTest', 'VmInstalledRuntime'",
    '[Security.Principal.WindowsBuiltInRole]::Administrator',
    'UIPILOT_DISPOSABLE_VM=I_UNDERSTAND_THIS_MACHINE_WILL_BE_MUTATED'
)
Assert-SourceContains $releaseGate @(
    "[ValidateSet('Local', 'SelfTest')]",
    'Helper tests', 'Installer Source', 'Production NSIS',
    "Action = { Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml') }"
)
Assert-SourceContains $vmMatrix @('# Managed Everything Windows VM Matrix', '## Row 1', '## Row 13', 'VmInstall', 'VmUpdate', 'Status: `NOT RUN`')
$ignoreSource = Get-Content -LiteralPath (Join-Path $root '.gitignore') -Raw -Encoding utf8
Assert-Condition $ignoreSource.Contains('/artifacts/managed-everything/') 'Managed Everything evidence root must be ignored explicitly'
$selfTestOutput = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -Mode SelfTest 2>&1)
Assert-Condition ($LASTEXITCODE -eq 0) 'Installer VM guard self-test failed'
Assert-Condition (($selfTestOutput -join "`n").Contains('MANAGED_EVERYTHING_INSTALLER_SELFTEST_PASS')) 'Installer VM guard self-test did not report success'

Assert-SourceContains (Join-Path $helperRoot 'model.rs') @(
    'Everything (UiPilot)', '\\.\PIPE\UiPilotEverythingService',
    'managed-runtime.json', 'managed-transaction.json', 'deny_unknown_fields',
    'include_bytes!("../../../resources/everything/everything.lock.json")',
    'include_bytes!("../../../resources/everything/LICENSE.txt")',
    'include_bytes!("../../../resources/everything/Everything-UiPilot.ini")'
)
Assert-SourceContains (Join-Path $helperRoot 'windows.rs') @(
    'GetShellWindow', 'GetWindowThreadProcessId', 'ProcessIdToSessionId',
    'OpenProcessToken', 'GetTokenInformation', 'ConvertSidToStringSidW',
    'CreateFileW', 'FILE_SHARE_READ', 'OPEN_EXISTING', 'FILE_FLAG_OPEN_REPARSE_POINT',
    'OpenSCManagerW', 'QueryServiceConfigW', 'QueryServiceStatusEx',
    'WaitForSingleObject', 'GetSecurityInfo', 'GetNamedSecurityInfoW',
    'WinVerifyTrust', 'CryptQueryObject', 'CertStrToNameW', 'GetFileVersionInfoW'
)
Assert-SourceContains (Join-Path $helperRoot 'transaction.rs') @(
    'StageNewBinary', 'StopOldService', 'RemoveOldService', 'InstallNewService',
    'StartNewService', 'ReplaceRuntimeManifest', 'CleanupUnreferencedVersions',
    'FailClosedRetainAll', 'DeleteManifest'
)
Assert-SourceContains $hooks @(
    'Global\UiPilotManagedEverythingInstaller',
    'Global\UiPilotManagedEverythingInstallerActivity',
    'CoCreateGuid', 'CreateMutexW', 'CreateEventW', 'CreateDirectoryW',
    'prepare-install', 'recover-install', 'commit-install', 'rollback-install',
    'pre-uninstall', 'post-uninstall',
    '!insertmacro UIPILOT_RUN_HELPER "pre-uninstall" "no" "yes" "yes"',
    '${StrCase} $UiPilotOperationId $UiPilotOperationId "L"',
    '${UnStrCase} $UiPilotOperationId $UiPilotOperationId "L"'
)
Assert-SourceContains $template @(
    'TAURI_CLI_VERSION: 2.11.4', 'UIPILOT_REGION_REINSTALL_BEGIN',
    'UIPILOT_REGION_UNINSTALL_GATE_BEGIN', 'UIPILOT_REGION_HELPER_LIFETIME_BEGIN'
)
Assert-SourceContains $build @(
    'x86_64-pc-windows-msvc', '--release', 'uipilot-everything-installer.exe',
    'requestedExecutionLevel', 'asInvoker', 'VmInstrumentedSetup',
    'target\vm-instrumented', 'installer-helper-build,fault-injection'
)
Assert-SourceContains $tauri @('"installMode": "perMachine"', '"installerHooks"', '"template"')
Assert-SourceContains $vmTauri @('VM Instrumented Setup', 'vm-instrumented')

$allInstallerSources = @(
    Get-ChildItem -LiteralPath $helperRoot -File
    Get-Item -LiteralPath $hooks
    Get-Item -LiteralPath $template
) | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding utf8 }
$joined = $allInstallerSources -join "`n"
foreach ($forbidden in @('powershell.exe', 'cmd.exe', 'wmic.exe', 'Win32_Service')) {
    if ($joined.Contains($forbidden)) { throw "Forbidden script/WMI interpreter contract found: $forbidden" }
}

if ($joined.Contains('#[cfg(feature = "fault-injection")]') -and
    -not (Get-Content -LiteralPath $build -Raw).Contains('installer-helper-build,fault-injection')) {
    throw 'Instrumented checkpoints are not isolated to the VM build mode'
}

$baseline = Join-Path $root 'src-tauri\windows\installer.tauri-cli-2.11.4.nsi'
$expectedBaselineHash = '20f4ecc730defb71f1342eaeaec4021df13be3d843abba0effe88ea5835fa079'
if ((Get-LowerSha256 $baseline) -cne $expectedBaselineHash) {
    throw 'Frozen Tauri CLI 2.11.4 installer template snapshot changed'
}
$baselineText = Get-Content -LiteralPath $baseline -Raw -Encoding utf8
$customText = Get-Content -LiteralPath $template -Raw -Encoding utf8
$uninstallSection = [regex]::Match($customText, '(?ms)^Section Uninstall\r?\n.*?^SectionEnd\r?\n')
Assert-Condition $uninstallSection.Success 'Could not locate the custom uninstall section'
Assert-OrderedUniqueTokens $uninstallSection.Value @(
    '!insertmacro CheckIfAppIsRunning',
    '!insertmacro NSIS_HOOK_PREUNINSTALL',
    '!insertmacro NSIS_HOOK_POSTUNINSTALL',
    'Delete "$INSTDIR\${MAINBINARYNAME}.exe"',
    'Delete "$INSTDIR\uninstall.exe"',
    '; Remove registry information for add/remove programs'
) 'Uninstall continuation ordering must retain the app, uninstaller, and registration until post-uninstall succeeds'
$hooksText = Get-Content -LiteralPath $hooks -Raw -Encoding utf8
$preUninstallHook = [regex]::Match($hooksText, '(?ms)^!macro NSIS_HOOK_PREUNINSTALL\r?\n.*?^!macroend\r?\n')
Assert-Condition $preUninstallHook.Success 'Could not locate the pre-uninstall hook'
Assert-OrderedUniqueTokens $preUninstallHook.Value @(
    '!insertmacro UIPILOT_RUN_HELPER "pre-uninstall" "no" "no" "yes"',
    '${If} $UiPilotHelperExit = 15',
    'FileRead $R5 $UiPilotRecoveryId',
    '!insertmacro UIPILOT_RUN_HELPER "pre-uninstall" "no" "yes" "yes"',
    '${If} $UiPilotHelperExit != 0'
) 'Pre-uninstall recovery must bind the durable transaction id before convergence'
$reinstallMatch = [regex]::Match($baselineText, '(?ms)^  ; Reinstalling the same version\r?\n.*?^  reinst_done:\r?\n')
$uninstallMatch = [regex]::Match($baselineText, '(?ms)^  !ifmacrodef NSIS_HOOK_PREUNINSTALL\r?\n.*?^  !ifmacrodef NSIS_HOOK_POSTUNINSTALL\r?\n    !insertmacro NSIS_HOOK_POSTUNINSTALL\r?\n  !endif\r?\n')
Assert-Condition $reinstallMatch.Success 'Could not locate the upstream reinstall snapshot region'
Assert-Condition $uninstallMatch.Success 'Could not locate the upstream uninstall-gate snapshot region'
$normalized = [regex]::Replace(
    $customText,
    '(?ms)^; UIPILOT_REGION_HELPER_LIFETIME_BEGIN\r?\n\$\{StrCase\}\r?\n\$\{UnStrCase\}\r?\n\$\{StrLoc\}\r?\n\$\{StrRep\}\r?\n\$\{UnStrRep\}\r?\n; UIPILOT_REGION_HELPER_LIFETIME_END\r?\n',
    "`${StrCase}`n`${StrLoc}`n"
)
$normalized = [regex]::Replace(
    $normalized,
    '(?ms)^  ; UIPILOT_REGION_REINSTALL_BEGIN\r?\n.*?^  ; UIPILOT_REGION_REINSTALL_END\r?\n',
    [Text.RegularExpressions.MatchEvaluator]{ param($match) $reinstallMatch.Value }
)
$normalized = [regex]::Replace(
    $normalized,
    '(?ms)^  ; UIPILOT_REGION_UNINSTALL_GATE_BEGIN\r?\n.*?^  ; UIPILOT_REGION_UNINSTALL_GATE_END\r?\n',
    [Text.RegularExpressions.MatchEvaluator]{ param($match) $uninstallMatch.Value }
)
$normalizedBytes = [Text.Encoding]::UTF8.GetBytes($normalized)
$sha = [Security.Cryptography.SHA256]::Create()
try { $normalizedHash = (($sha.ComputeHash($normalizedBytes) | ForEach-Object { $_.ToString('x2') }) -join '') }
finally { $sha.Dispose() }
Assert-Condition ($normalizedHash -ceq $expectedBaselineHash) 'Custom installer differs from Tauri CLI 2.11.4 outside the three approved regions'

$productionHelper = Join-Path $root 'src-tauri\target\release\uipilot-everything-installer.exe'
Assert-Condition (Test-Path -LiteralPath $productionHelper -PathType Leaf) 'Production Helper is required for Source artifact isolation checks'
Assert-Condition ((Get-PeMachine $productionHelper) -eq 0x8664) 'Production Helper must be x64'
$productionStrings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($productionHelper))
Assert-Condition (-not $productionStrings.Contains('fault-injection:')) 'Production Helper contains VM fault-injection code'
$installedHelperCopies = @(Get-ChildItem (Join-Path $root 'src-tauri\target\release\resources') -Recurse -Filter 'uipilot-everything-installer.exe' -File -ErrorAction SilentlyContinue)
Assert-Condition ($installedHelperCopies.Count -eq 0) 'Helper must not be copied into installed runtime resources'

$productionBundleRoot = Join-Path $root 'src-tauri\target\release\bundle\nsis'
Assert-Condition (Test-Path -LiteralPath $productionBundleRoot -PathType Container) 'Production bundle directory is required for Source artifact isolation checks'
$setups = @(Get-ChildItem $productionBundleRoot -Filter '*.exe' -File)
Assert-Condition ($setups.Count -eq 1) 'Production bundle directory must contain exactly one Setup'
Assert-Condition (-not $setups[0].Name.Contains('VM Instrumented')) 'VM Instrumented Setup leaked into the production bundle path'
Assert-Condition ((Get-PeMachine $setups[0].FullName) -eq 0x014c) 'NSIS Setup must be a valid x86 bootstrapper PE for the x64 payload'

Write-Output 'MANAGED_EVERYTHING_INSTALLER_SOURCE_PASS'
