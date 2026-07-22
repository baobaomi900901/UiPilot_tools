param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('SelfTest', 'Io', 'Smoke', 'Accessibility')]
    [string] $Mode,

    [Parameter(Mandatory = $true)]
    [string] $RepoRoot,

    [Parameter(Mandatory = $true)]
    [string] $ArtifactRoot,

    [string] $ProcMonExe,

    [ValidateSet('NormalLight', 'NormalDark', 'ForcedColors', 'Narrator')]
    [string] $AccessibilityMode,

    [ValidateSet('Prepare', 'Read')]
    [string] $HitlPhase,

    [ValidateSet('IoCapture', 'SearchKeyboard', 'FileActions', 'TrayLifecycle', 'Accessibility')]
    [string] $HitlGroup
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'

function Resolve-StrictDirectory([string] $Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (-not $item.PSIsContainer) { throw "not a directory" }
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "reparse directory rejected" }
    return $item.FullName.TrimEnd('\')
}

function Resolve-FindRoots([string] $RepoRootValue, [string] $ArtifactRootValue) {
    $repo = Resolve-StrictDirectory $RepoRootValue
    $expected = Join-Path $repo 'artifacts\find-local-file-search'
    $parent = Split-Path -Parent $expected
    if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent | Out-Null }
    if (-not (Test-Path -LiteralPath $ArtifactRootValue)) {
        New-Item -ItemType Directory -Path $ArtifactRootValue | Out-Null
    }
    $artifact = Resolve-StrictDirectory $ArtifactRootValue
    if ($artifact -cne $expected) { throw "ArtifactRoot must be $expected" }
    return @{ Repo = $repo; Artifact = $artifact }
}

function Assert-ExactKeys($Object, [string[]] $Keys) {
    $actual = @($Object.PSObject.Properties.Name | Sort-Object)
    $expected = @($Keys | Sort-Object)
    if (($actual -join "`n") -cne ($expected -join "`n")) { throw "schema mismatch" }
}

function Assert-NoSensitiveValue([string] $Json) {
    foreach ($forbidden in @($env:USERNAME, $env:USERPROFILE, 'C:\', '\\', 'Volume{', 'query', 'item-', 'req-', '.txt', 'content')) {
        if ($forbidden -and $Json.Contains($forbidden)) { throw "sensitive field leaked" }
    }
}

function New-OwnedTempTree {
    $root = Join-Path ([IO.Path]::GetTempPath()) ("uipilot-find-owned-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $root | Out-Null
    $acl = Get-Acl -LiteralPath $root
    $marker = [guid]::NewGuid().ToString('N')
    Set-Content -LiteralPath (Join-Path $root '.uipilot-find-owned') -Value $marker -NoNewline -Encoding UTF8
    return @{ Root = $root; Marker = $marker; Acl = $acl }
}

function Remove-OwnedTempTree($Tree) {
    $markerPath = Join-Path $Tree.Root '.uipilot-find-owned'
    if (-not (Test-Path -LiteralPath $markerPath)) { throw "owned marker missing" }
    $actual = Get-Content -LiteralPath $markerPath -Raw -Encoding UTF8
    if ($actual -cne $Tree.Marker) { throw "owned marker mismatch" }
    Set-Acl -LiteralPath $Tree.Root -AclObject $Tree.Acl
    Remove-Item -LiteralPath $Tree.Root -Recurse -Force
}

function Assert-HitlEvidenceArgs {
    if ($Mode -ne 'SelfTest') {
        if ($Mode -eq 'Accessibility') {
            if (-not $AccessibilityMode) { throw "Accessibility requires AccessibilityMode" }
        } elseif (-not $HitlPhase -or -not $HitlGroup) {
            throw "$Mode requires HitlPhase and HitlGroup"
        }
    }
}

function Invoke-FindEvidenceSelfTest {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    Assert-HitlEvidenceArgs
    Assert-ExactKeys ([pscustomobject]@{ mode = 'NormalLight'; keyboard = $true; mouse = $true; screenReader = $true; forcedColors = $true; themeMatches = $true; zoom100 = $true; zoom150 = $true; zoom200 = $true; noOverlap = $true; noHorizontalScroll = $true; focusOrder = $true; activeOptionVisible = $true; pathWraps = $true; disabledSettingsSkipped = $true; liveRegionPathFree = $true; osStateUnchanged = $true; zoomRestored = $true }) @('mode', 'keyboard', 'mouse', 'screenReader', 'forcedColors', 'themeMatches', 'zoom100', 'zoom150', 'zoom200', 'noOverlap', 'noHorizontalScroll', 'focusOrder', 'activeOptionVisible', 'pathWraps', 'disabledSettingsSkipped', 'liveRegionPathFree', 'osStateUnchanged', 'zoomRestored')
    try { Assert-ExactKeys ([pscustomobject]@{ mode = 'NormalLight'; extra = $true }) @('mode') ; throw "schema accepted extra key" } catch { if ($_.Exception.Message -eq "schema accepted extra key") { throw } }
    try { Assert-NoSensitiveValue (@{ operation = 'C:\Users\name\secret.txt' } | ConvertTo-Json -Compress); throw "sensitive accepted" } catch { if ($_.Exception.Message -eq "sensitive accepted") { throw } }
    $tree = New-OwnedTempTree
    Set-Content -LiteralPath (Join-Path $tree.Root 'raw.pml') -Value 'discard' -Encoding UTF8
    Remove-OwnedTempTree $tree
    if (Test-Path -LiteralPath $tree.Root) { throw "owned cleanup failed" }
    $summary = [pscustomobject]@{ ioCapture = $true; searchKeyboard = $true; fileActions = $true; trayLifecycle = $true }
    $json = $summary | ConvertTo-Json -Compress
    Assert-NoSensitiveValue $json
    $json | Set-Content -LiteralPath (Join-Path $roots.Artifact 'evidence-selftest.json') -Encoding UTF8
    Remove-Item -LiteralPath (Join-Path $roots.Artifact 'evidence-selftest.json') -Force
    Write-Output 'TASK11_EVIDENCE_SELFTEST_PASS'
}

function Start-HitlSession([string] $Name) {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    if (-not $HitlPhase -or -not $HitlGroup) { throw "$Name requires HitlPhase and HitlGroup" }
    $session = Join-Path $roots.Artifact ("$Name-$HitlGroup-session.json")
    if ($HitlPhase -eq 'Prepare') {
        [pscustomobject]@{ mode = $Name; group = $HitlGroup; pid = $PID; creationDateUtc = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o') } |
            ConvertTo-Json -Compress | Set-Content -LiteralPath $session -Encoding UTF8
        Write-Output "HITL_PENDING $HitlGroup"
        exit 3
    }
    if (-not (Test-Path -LiteralPath $session)) { throw "owned $Name session missing" }
    throw "$Name Read requires user-completed evidence and is not executed in Task11"
}

function Start-AccessibilitySession {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    if (-not $AccessibilityMode) { throw "Accessibility requires AccessibilityMode" }
    if ($HitlPhase -eq 'Read') {
        throw "Accessibility Read requires user-completed evidence and is not executed in Task11"
    }
    [pscustomobject]@{ mode = $AccessibilityMode; osStateChanged = $false } |
        ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $roots.Artifact "accessibility-$AccessibilityMode-session.json") -Encoding UTF8
    Write-Output 'HITL_PENDING Accessibility'
    exit 3
}

switch ($Mode) {
    'SelfTest' { Invoke-FindEvidenceSelfTest }
    'Io' {
        if (-not $ProcMonExe) { throw "Io requires ProcMonExe" }
        $procmon = Get-Item -LiteralPath $ProcMonExe -ErrorAction Stop
        if ($procmon.PSIsContainer) { throw "ProcMonExe must be a file" }
        Start-HitlSession 'io'
    }
    'Smoke' { Start-HitlSession 'smoke' }
    'Accessibility' { Start-AccessibilitySession }
}
