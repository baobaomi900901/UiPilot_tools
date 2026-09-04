param(
    [ValidateSet('Source')]
    [string]$Mode = 'Source'
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

$root = Split-Path -Parent $PSScriptRoot
$helperRoot = Join-Path $root 'src-tauri\src\bin\uipilot-everything-installer'
$template = Join-Path $root 'src-tauri\windows\installer.nsi'
$hooks = Join-Path $root 'src-tauri\windows\nsis-hooks.nsh'
$build = Join-Path $root 'scripts\build-managed-everything-installer.ps1'
$tauri = Join-Path $root 'src-tauri\tauri.conf.json'
$vmTauri = Join-Path $root 'src-tauri\tauri.vm-instrumented.conf.json'

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
