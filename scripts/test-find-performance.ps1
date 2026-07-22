param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('SelfTest', 'Database', 'Runtime')]
    [string] $Mode,

    [Parameter(Mandatory = $true)]
    [string] $RepoRoot,

    [string] $ReleaseExe,

    [Parameter(Mandatory = $true)]
    [string] $ArtifactRoot,

    [ValidateSet('Prepare', 'Read')]
    [string] $HitlPhase,

    [ValidateSet('ColdPanel', 'FirstResult', 'SubsequentQuery', 'WatcherRefresh')]
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
    foreach ($forbidden in @($env:USERNAME, $env:USERPROFILE, 'C:\', '\\', 'Volume{', 'query', 'item-', 'req-', '.txt')) {
        if ($forbidden -and $Json.Contains($forbidden)) { throw "sensitive field leaked" }
    }
}

function New-OwnedTempTree {
    $root = Join-Path ([IO.Path]::GetTempPath()) ("uipilot-find-owned-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $root | Out-Null
    $marker = [guid]::NewGuid().ToString('N')
    Set-Content -LiteralPath (Join-Path $root '.uipilot-find-owned') -Value $marker -NoNewline -Encoding UTF8
    return @{ Root = $root; Marker = $marker }
}

function Remove-OwnedTempTree($Tree) {
    $markerPath = Join-Path $Tree.Root '.uipilot-find-owned'
    if (-not (Test-Path -LiteralPath $markerPath)) { throw "owned marker missing" }
    $actual = Get-Content -LiteralPath $markerPath -Raw -Encoding UTF8
    if ($actual -cne $Tree.Marker) { throw "owned marker mismatch" }
    Remove-Item -LiteralPath $Tree.Root -Recurse -Force
}

function Assert-HitlRuntimeArgs {
    if ($Mode -eq 'Runtime') {
        if (-not $HitlPhase -or -not $HitlGroup) { throw "Runtime requires HitlPhase and HitlGroup" }
    }
}

function Invoke-FindPerformanceSelfTest {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    Assert-HitlRuntimeArgs
    Assert-ExactKeys ([pscustomobject]@{ rows = 1; samples = 1; p95Ms = 1; databaseBytes = 1; peakWorkingSetBytes = 0 }) @('rows', 'samples', 'p95Ms', 'databaseBytes', 'peakWorkingSetBytes')
    try { Assert-ExactKeys ([pscustomobject]@{ rows = 1; extra = 2 }) @('rows') ; throw "schema accepted extra key" } catch { if ($_.Exception.Message -eq "schema accepted extra key") { throw } }
    try { Assert-NoSensitiveValue (@{ p95Ms = 1; path = 'C:\Users\name\file.txt' } | ConvertTo-Json -Compress); throw "sensitive accepted" } catch { if ($_.Exception.Message -eq "sensitive accepted") { throw } }
    $tree = New-OwnedTempTree
    Set-Content -LiteralPath (Join-Path $tree.Root 'raw.tmp') -Value 'discard' -Encoding UTF8
    Remove-OwnedTempTree $tree
    if (Test-Path -LiteralPath $tree.Root) { throw "owned cleanup failed" }
    $manifest = [pscustomobject]@{
        mode = 'SelfTest'
        artifactRoot = 'artifacts/find-local-file-search'
        pid = $PID
        creationDateUtc = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
    }
    $json = $manifest | ConvertTo-Json -Compress
    Assert-NoSensitiveValue $json
    $json | Set-Content -LiteralPath (Join-Path $roots.Artifact 'performance-selftest.json') -Encoding UTF8
    Remove-Item -LiteralPath (Join-Path $roots.Artifact 'performance-selftest.json') -Force
    Write-Output 'TASK11_PERFORMANCE_SELFTEST_PASS'
}

function Invoke-DatabaseMode {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    $listed = @(cargo test --manifest-path (Join-Path $roots.Repo 'src-tauri\Cargo.toml') --locked --offline file_index::store::tests::million_row_query_gate -- --list --ignored)
    if ($LASTEXITCODE -ne 0 -or @($listed | Where-Object { $_ -cmatch '^file_index::store::tests::million_row_query_gate: test$' }).Count -ne 1) { throw "million-row test count is not exactly one" }
    $output = @(cargo test --manifest-path (Join-Path $roots.Repo 'src-tauri\Cargo.toml') --locked --offline file_index::store::tests::million_row_query_gate -- --ignored --exact --nocapture)
    if ($LASTEXITCODE -ne 0) { throw "million-row test failed" }
    $line = @($output | Where-Object { $_ -cmatch '^UIPILOT_FIND_DATABASE_SUMMARY ' })[-1]
    if (-not $line) { throw "missing database summary" }
    $summary = $line.Substring('UIPILOT_FIND_DATABASE_SUMMARY '.Length) | ConvertFrom-Json
    Assert-ExactKeys $summary @('rows', 'samples', 'p95Ms', 'databaseBytes', 'peakWorkingSetBytes')
    $json = $summary | ConvertTo-Json -Compress
    Assert-NoSensitiveValue $json
    $json | Set-Content -LiteralPath (Join-Path $roots.Artifact 'database-summary.json') -Encoding UTF8
}

function Invoke-RuntimeMode {
    $roots = Resolve-FindRoots $RepoRoot $ArtifactRoot
    Assert-HitlRuntimeArgs
    $session = Join-Path $roots.Artifact ("runtime-$HitlGroup-session.json")
    if ($HitlPhase -eq 'Prepare') {
        if (-not $ReleaseExe) { throw "Runtime Prepare requires ReleaseExe" }
        $exe = Get-Item -LiteralPath $ReleaseExe -ErrorAction Stop
        if ($exe.PSIsContainer) { throw "ReleaseExe must be a file" }
        [pscustomobject]@{ group = $HitlGroup; exeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $exe.FullName).Hash; pid = $null; creationDateUtc = $null } |
            ConvertTo-Json -Compress | Set-Content -LiteralPath $session -Encoding UTF8
        Write-Output "HITL_PENDING $HitlGroup"
        exit 3
    }
    if (-not (Test-Path -LiteralPath $session)) { throw "owned Runtime session missing" }
    throw "Runtime Read requires user-completed product evidence and is not executed in Task11"
}

switch ($Mode) {
    'SelfTest' { Invoke-FindPerformanceSelfTest }
    'Database' { Invoke-DatabaseMode }
    'Runtime' { Invoke-RuntimeMode }
}
