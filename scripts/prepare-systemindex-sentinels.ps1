[CmdletBinding(DefaultParameterSetName = 'Create')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Create')]
    [switch] $Create,

    [Parameter(Mandatory, ParameterSetName = 'Cleanup')]
    [switch] $Cleanup,

    [Parameter(Mandatory, ParameterSetName = 'Cleanup')]
    [ValidateNotNullOrEmpty()]
    [string] $Manifest
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoManifest = Join-Path $repoRoot 'spikes/systemindex/Cargo.toml'
$binary = Join-Path $repoRoot 'spikes/systemindex/target/release/systemindex-spike.exe'
$sentinelRoot = Join-Path $repoRoot 'spikes/systemindex/target/sentinels'

function Throw-NotRunnable {
    param([Parameter(Mandatory)] [string] $Message)
    throw [InvalidOperationException]::new("NOT_RUNNABLE: $Message")
}

function Get-NormalizedDirectory {
    param([Parameter(Mandatory)] [string] $Path)
    $full = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar)
    return "$full$([IO.Path]::DirectorySeparatorChar)"
}

function ConvertTo-NormalizedFileUrl {
    param([Parameter(Mandatory)] [string] $Path)
    return 'file:///' + (Get-NormalizedDirectory $Path).Replace('/', '\')
}

function Normalize-FileRule {
    param([Parameter(Mandatory)] [string] $Rule)
    if (-not $Rule.StartsWith('file:///', [StringComparison]::OrdinalIgnoreCase)) {
        return $null
    }
    return 'file:///' + $Rule.Substring(8).Replace('/', '\')
}

function Test-RulePrefixMatch {
    param(
        [Parameter(Mandatory)] [string] $CandidateUrl,
        [Parameter(Mandatory)] [string] $Rule
    )
    $normalizedRule = Normalize-FileRule $Rule
    if (-not $normalizedRule) { return $false }
    $pattern = '^' + [regex]::Escape($normalizedRule).Replace('\*', '.*').Replace('\?', '.')
    return [regex]::IsMatch($CandidateUrl, $pattern, [Text.RegularExpressions.RegexOptions]::IgnoreCase)
}

function Test-ProvenIncludedDirectory {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [object] $Scopes
    )
    $candidate = ConvertTo-NormalizedFileUrl $Path
    $included = @($Scopes.includedFileRoots | Where-Object {
        $root = Normalize-FileRule ([string]$_)
        $root -and $candidate.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
    }).Count -gt 0
    if (-not $included) { return $false }
    return @($Scopes.exclusionRules | Where-Object {
        Test-RulePrefixMatch -CandidateUrl $candidate -Rule ([string]$_)
    }).Count -eq 0
}

function Test-OutsideIncludedRoots {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [object] $Scopes
    )
    $candidate = ConvertTo-NormalizedFileUrl $Path
    return @($Scopes.includedFileRoots | Where-Object {
        $root = Normalize-FileRule ([string]$_)
        $root -and $candidate.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
    }).Count -eq 0
}

function Test-WritableDirectory {
    param([Parameter(Mandatory)] [string] $Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $false }
    $probe = Join-Path $Path ("uipilot-write-probe-{0}.tmp" -f [Guid]::NewGuid().ToString('N'))
    try {
        [IO.File]::Create($probe).Dispose()
        return $true
    } catch {
        return $false
    } finally {
        if (Test-Path -LiteralPath $probe) { Remove-Item -LiteralPath $probe -Force }
    }
}

function Ensure-SpikeBinary {
    if (Test-Path -LiteralPath $binary -PathType Leaf) { return }
    [IO.Directory]::CreateDirectory($sentinelRoot) | Out-Null
    $buildOut = Join-Path $sentinelRoot 'build.stdout.log'
    $buildErr = Join-Path $sentinelRoot 'build.stderr.log'
    $cargo = (Get-Command cargo -ErrorAction Stop).Source
    $process = Start-Process -FilePath $cargo -ArgumentList @(
        'build', '--release', '--manifest-path', $cargoManifest
    ) -Wait -PassThru -RedirectStandardOutput $buildOut -RedirectStandardError $buildErr
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Spike release build failed; see $buildErr"
    }
}

function Invoke-Spike {
    param([Parameter(Mandatory)] [string[]] $Arguments)
    $id = [Guid]::NewGuid().ToString('N')
    $stdout = Join-Path $sentinelRoot "$id.stdout.json"
    $stderr = Join-Path $sentinelRoot "$id.stderr.json"
    try {
        $process = Start-Process -FilePath $binary -ArgumentList $Arguments -Wait -PassThru `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            Stdout = if (Test-Path -LiteralPath $stdout) { Get-Content -LiteralPath $stdout -Raw -Encoding utf8 } else { '' }
            Stderr = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw -Encoding utf8 } else { '' }
        }
    } finally {
        Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-Query {
    param([Parameter(Mandatory)] [string] $FileName)
    $result = Invoke-Spike -Arguments @('query', '--literal', $FileName, '--limit', '20', '--json')
    if ($result.ExitCode -eq 2) { Throw-NotRunnable "SystemIndex query precondition failed: $($result.Stderr)" }
    if ($result.ExitCode -ne 0) { throw "SystemIndex query failed: $($result.Stderr)" }
    return $result.Stdout | ConvertFrom-Json
}

function Remove-SentinelPair {
    param(
        [Parameter(Mandatory)] [string] $Directory,
        [Parameter(Mandatory)] [string] $FullPath,
        [Parameter(Mandatory)] [string] $FileName
    )
    $resolvedDirectory = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    $resolvedFile = [IO.Path]::GetFullPath($FullPath)
    if ([IO.Path]::GetDirectoryName($resolvedFile) -ne $resolvedDirectory) {
        throw 'Sentinel file is not an immediate child of its recorded directory'
    }
    if ([IO.Path]::GetFileName($resolvedFile) -ne $FileName) {
        throw 'Sentinel file name does not match its recorded full path'
    }
    if ([IO.Path]::GetFileName($resolvedDirectory) -notmatch '^uipilot-systemindex-spike-[0-9a-f]{32}$') {
        throw 'Sentinel directory does not have the owned unique-directory shape'
    }
    if (Test-Path -LiteralPath $resolvedFile) { Remove-Item -LiteralPath $resolvedFile -Force }
    if (Test-Path -LiteralPath $resolvedDirectory) {
        [IO.Directory]::Delete($resolvedDirectory, $false)
    }
}

function Invoke-Cleanup {
    $manifestPath = [IO.Path]::GetFullPath($Manifest)
    $allowedRoot = Get-NormalizedDirectory $sentinelRoot
    if (-not $manifestPath.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Manifest is outside the spike sentinel evidence directory'
    }
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw 'Sentinel manifest does not exist'
    }
    $data = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    if ($data.schemaVersion -ne 1 -or -not $data.indexed -or -not $data.unindexed) {
        throw 'Sentinel manifest schema is invalid'
    }
    $indexedDirectory = [IO.Path]::GetFullPath([string]$data.indexed.directory)
    $unindexedDirectory = [IO.Path]::GetFullPath([string]$data.unindexed.directory)
    if ($indexedDirectory -eq $unindexedDirectory) { throw 'Sentinel directories must be distinct' }

    Remove-SentinelPair -Directory $indexedDirectory -FullPath ([string]$data.indexed.fullPath) `
        -FileName ([string]$data.indexed.fileName)
    Remove-SentinelPair -Directory $unindexedDirectory -FullPath ([string]$data.unindexed.fullPath) `
        -FileName ([string]$data.unindexed.fileName)
    Remove-Item -LiteralPath $manifestPath -Force
}

function Invoke-Create {
    Ensure-SpikeBinary
    [IO.Directory]::CreateDirectory($sentinelRoot) | Out-Null

    $scopeResult = Invoke-Spike -Arguments @('scopes', '--json')
    if ($scopeResult.ExitCode -eq 2) { Throw-NotRunnable "SystemIndex scopes are unavailable: $($scopeResult.Stderr)" }
    if ($scopeResult.ExitCode -ne 0) { throw "Reading SystemIndex scopes failed: $($scopeResult.Stderr)" }
    $scopes = $scopeResult.Stdout | ConvertFrom-Json

    $indexedBase = @(
        [Environment]::GetFolderPath('MyDocuments'),
        [Environment]::GetFolderPath('Desktop'),
        $env:TEMP
    ) | Where-Object {
        $_ -and (Test-WritableDirectory $_) -and (Test-ProvenIncludedDirectory -Path $_ -Scopes $scopes)
    } | Select-Object -First 1
    if (-not $indexedBase) { Throw-NotRunnable 'No writable Documents, Desktop, or TEMP directory is provably indexed' }

    $unindexedBase = @(
        $env:ProgramData,
        (Join-Path $env:SystemRoot 'Temp'),
        $repoRoot
    ) | Where-Object {
        $_ -and (Test-WritableDirectory $_) -and (Test-OutsideIncludedRoots -Path $_ -Scopes $scopes)
    } | Select-Object -First 1
    if (-not $unindexedBase) { Throw-NotRunnable 'No writable local directory outside all included roots is available' }

    $indexedDirectory = Join-Path $indexedBase ("uipilot-systemindex-spike-{0}" -f [Guid]::NewGuid().ToString('N'))
    $unindexedDirectory = Join-Path $unindexedBase ("uipilot-systemindex-spike-{0}" -f [Guid]::NewGuid().ToString('N'))
    $indexedFileName = "uipilot-indexed-$([Guid]::NewGuid().ToString('N')).txt"
    $unindexedFileName = "uipilot-unindexed-$([Guid]::NewGuid().ToString('N')).txt"
    $indexedPath = Join-Path $indexedDirectory $indexedFileName
    $unindexedPath = Join-Path $unindexedDirectory $unindexedFileName
    $manifestPath = Join-Path $sentinelRoot ("manifest-{0}.json" -f [Guid]::NewGuid().ToString('N'))
    $evidencePath = [IO.Path]::ChangeExtension($manifestPath, '.evidence.json')

    try {
        [IO.Directory]::CreateDirectory($indexedDirectory) | Out-Null
        [IO.File]::Create($indexedPath).Dispose()
        $stopwatch = [Diagnostics.Stopwatch]::StartNew()
        $indexedObserved = $false
        while ($stopwatch.Elapsed.TotalSeconds -le 120) {
            $query = Invoke-Query $indexedFileName
            $indexedObserved = @($query.items | Where-Object {
                [string]::Equals(
                    [IO.Path]::GetFullPath([string]$_.parsingPath),
                    [IO.Path]::GetFullPath($indexedPath),
                    [StringComparison]::OrdinalIgnoreCase
                )
            }).Count -gt 0
            if ($indexedObserved) { break }
            Start-Sleep -Seconds 2
        }
        if (-not $indexedObserved) { Throw-NotRunnable 'Indexed sentinel was not observed within 120 seconds' }
        $indexedObservedAfterMs = [math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)

        [IO.Directory]::CreateDirectory($unindexedDirectory) | Out-Null
        [IO.File]::Create($unindexedPath).Dispose()
        $unindexedStopwatch = [Diagnostics.Stopwatch]::StartNew()
        $unindexedQuery = Invoke-Query $unindexedFileName
        $unindexedStopwatch.Stop()
        if (@($unindexedQuery.items).Count -ne 0) {
            throw 'Sentinel outside all included roots unexpectedly appeared in SystemIndex results'
        }

        [ordered]@{
            schemaVersion = 1
            capturedAt = (Get-Date).ToString('o')
            includedFileRoots = @($scopes.includedFileRoots)
            exclusionRules = @($scopes.exclusionRules)
            indexed = [ordered]@{
                fileName = $indexedFileName
                fullPath = [IO.Path]::GetFullPath($indexedPath)
                observedAfterMs = $indexedObservedAfterMs
                resultCount = @($query.items).Count
                counters = $query.counters
            }
            unindexed = [ordered]@{
                fileName = $unindexedFileName
                fullPath = [IO.Path]::GetFullPath($unindexedPath)
                elapsedMs = [math]::Round($unindexedStopwatch.Elapsed.TotalMilliseconds, 3)
                resultCount = @($unindexedQuery.items).Count
                counters = $unindexedQuery.counters
            }
        } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $evidencePath -Encoding utf8

        [ordered]@{
            schemaVersion = 1
            indexed = [ordered]@{
                directory = [IO.Path]::GetFullPath($indexedDirectory)
                fileName = $indexedFileName
                fullPath = [IO.Path]::GetFullPath($indexedPath)
            }
            unindexed = [ordered]@{
                directory = [IO.Path]::GetFullPath($unindexedDirectory)
                fileName = $unindexedFileName
                fullPath = [IO.Path]::GetFullPath($unindexedPath)
            }
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding utf8
        Write-Output ([IO.Path]::GetFullPath($manifestPath))
    } catch {
        if (Test-Path -LiteralPath $indexedPath) { Remove-Item -LiteralPath $indexedPath -Force }
        if (Test-Path -LiteralPath $indexedDirectory) { [IO.Directory]::Delete($indexedDirectory, $false) }
        if (Test-Path -LiteralPath $unindexedPath) { Remove-Item -LiteralPath $unindexedPath -Force }
        if (Test-Path -LiteralPath $unindexedDirectory) { [IO.Directory]::Delete($unindexedDirectory, $false) }
        if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
        if (Test-Path -LiteralPath $evidencePath) { Remove-Item -LiteralPath $evidencePath -Force }
        throw
    }
}

try {
    if ($PSCmdlet.ParameterSetName -eq 'Cleanup') { Invoke-Cleanup } else { Invoke-Create }
    exit 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    if ($_.Exception.Message.StartsWith('NOT_RUNNABLE:')) { exit 2 }
    exit 1
}
