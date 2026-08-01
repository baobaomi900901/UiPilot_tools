param(
    [switch]$SelfTestTransaction,

    [switch]$SelfTestZipEntrySelection
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$expectedLockProperties = @(
    'architecture'
    'artifactFileName'
    'artifactSha256'
    'authenticodePublisher'
    'everythingExeSha256'
    'license'
    'licenseSha256'
    'licenseUrl'
    'schemaVersion'
    'sourceUrl'
    'version'
)
$expectedVersion = '1.4.1.1032'
$expectedArchitecture = 'x64'
$expectedArtifactFileName = 'Everything-1.4.1.1032.x64.zip'
$expectedSourceUrl = 'https://www.voidtools.com/Everything-1.4.1.1032.x64.zip'
$expectedLicense = 'MIT'
$expectedLicenseUrl = 'https://www.voidtools.com/License.txt'
$lowerSha256Pattern = '^[0-9a-f]{64}$'

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

function Assert-RequiredFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LiteralPath,

        [Parameter(Mandatory = $true)]
        [string]$Description
    )

    if (-not (Test-Path -LiteralPath $LiteralPath -PathType Leaf)) {
        throw "$Description is missing: $LiteralPath"
    }
}

function Get-LowerSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LiteralPath
    )

    return (Get-FileHash -LiteralPath $LiteralPath -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-LowerSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Value,

        [Parameter(Mandatory = $true)]
        [string]$FieldName
    )

    Assert-Condition ($Value -cmatch $lowerSha256Pattern) "$FieldName must be exactly 64 lowercase hexadecimal characters"
}

function Assert-ExactProperties {
    param(
        [Parameter(Mandatory = $true)]
        [psobject]$Object,

        [Parameter(Mandatory = $true)]
        [string[]]$ExpectedProperties
    )

    $actualProperties = @($Object.PSObject.Properties.Name | Sort-Object -CaseSensitive)
    $expected = @($ExpectedProperties | Sort-Object -CaseSensitive)
    $difference = @(Compare-Object -ReferenceObject $expected -DifferenceObject $actualProperties -CaseSensitive)
    if ($difference.Count -ne 0) {
        $details = $difference | ForEach-Object { "$($_.SideIndicator)$($_.InputObject)" }
        throw "everything.lock.json properties differ from the exact schema: $($details -join ', ')"
    }
}

function Read-ReviewedLock {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LockPath
    )

    Assert-RequiredFile $LockPath 'Reviewed lock'
    $lock = Get-Content -LiteralPath $LockPath -Raw -Encoding utf8 | ConvertFrom-Json
    Assert-Condition ($null -ne $lock -and $lock -is [psobject]) 'everything.lock.json must contain one JSON object'
    Assert-ExactProperties $lock $expectedLockProperties
    Assert-Condition ($lock.schemaVersion -is [int] -or $lock.schemaVersion -is [long]) 'schemaVersion must be an integer'
    Assert-Condition ([long]$lock.schemaVersion -eq 1) 'schemaVersion must be exactly 1'
    Assert-Condition ([string]$lock.version -ceq $expectedVersion) "version must be exactly $expectedVersion"
    Assert-Condition ([string]$lock.architecture -ceq $expectedArchitecture) "architecture must be exactly $expectedArchitecture"
    Assert-Condition ([string]$lock.artifactFileName -ceq $expectedArtifactFileName) "artifactFileName must be exactly $expectedArtifactFileName"
    Assert-Condition ([string]$lock.sourceUrl -ceq $expectedSourceUrl) "sourceUrl must be exactly $expectedSourceUrl"
    Assert-Condition ([string]$lock.license -ceq $expectedLicense) "license must be exactly $expectedLicense"
    Assert-Condition ([string]$lock.licenseUrl -ceq $expectedLicenseUrl) "licenseUrl must be exactly $expectedLicenseUrl"
    Assert-LowerSha256 ([string]$lock.artifactSha256) 'artifactSha256'
    Assert-LowerSha256 ([string]$lock.everythingExeSha256) 'everythingExeSha256'
    Assert-LowerSha256 ([string]$lock.licenseSha256) 'licenseSha256'
    Assert-Condition (-not [string]::IsNullOrWhiteSpace([string]$lock.authenticodePublisher)) 'authenticodePublisher must contain the reviewed certificate Subject'
    Assert-Condition ([string]$lock.authenticodePublisher -ceq ([string]$lock.authenticodePublisher).Trim()) 'authenticodePublisher must not contain surrounding whitespace'
    Assert-Condition (-not ([string]$lock.authenticodePublisher).Contains("`r") -and -not ([string]$lock.authenticodePublisher).Contains("`n")) 'authenticodePublisher must be one line'
    return $lock
}

function Invoke-FixedHttpsDownload {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Uri,

        [Parameter(Mandatory = $true)]
        [string]$Destination
    )

    $parsedUri = [Uri]$Uri
    Assert-Condition ($parsedUri.IsAbsoluteUri -and $parsedUri.Scheme -ceq 'https') 'Download URI must be absolute HTTPS'
    Assert-Condition ($parsedUri.Host -ceq 'www.voidtools.com') 'Download URI must use www.voidtools.com'
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -Uri $Uri -OutFile $Destination
    Assert-RequiredFile $Destination 'Downloaded file'
}

function Copy-EverythingExecutableFromArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,

        [Parameter(Mandatory = $true)]
        [string]$Destination
    )

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $matches = @($archive.Entries | Where-Object {
            $normalizedName = $_.FullName.Replace('\', '/')
            $basename = @($normalizedName.Split('/'))[-1]
            $basename -ieq 'everything.exe'
        })
        Assert-Condition ($matches.Count -eq 1) 'Artifact must contain exactly one entry whose basename equals everything.exe case-insensitively'
        $entry = $matches[0]
        $normalizedEntryName = $entry.FullName.Replace('\', '/')
        Assert-Condition (-not $normalizedEntryName.Contains('/')) 'The everything.exe entry must be in the ZIP root directory'
        Assert-Condition (-not [string]::IsNullOrEmpty($entry.Name) -and -not $entry.FullName.EndsWith('/') -and -not $entry.FullName.EndsWith('\')) 'The everything.exe entry must be a regular file'

        $source = $entry.Open()
        try {
            $destinationStream = [System.IO.File]::Create($Destination)
            try {
                $source.CopyTo($destinationStream)
            }
            finally {
                $destinationStream.Dispose()
            }
        }
        finally {
            $source.Dispose()
        }
    }
    finally {
        $archive.Dispose()
    }
}

function Remove-TransactionDirectories {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$TransactionDirectories,

        [Parameter(Mandatory = $true)]
        [string]$TransactionId
    )

    Assert-Condition ($TransactionId -cmatch '^[0-9a-f]{32}$') 'Transaction ID must be exactly 32 lowercase hexadecimal characters'
    $expectedLeaf = ".uipilot-everything-txn-$TransactionId"
    $validatedDirectories = @()
    foreach ($entry in $TransactionDirectories.GetEnumerator()) {
        $expectedParent = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath ([string]$entry.Key) -ErrorAction Stop).Path)
        $resolvedDirectory = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath ([string]$entry.Value) -ErrorAction Stop).Path)
        $actualParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $resolvedDirectory))
        $actualLeaf = Split-Path -Leaf $resolvedDirectory
        $expectedDirectory = [System.IO.Path]::GetFullPath((Join-Path $expectedParent $expectedLeaf))
        Assert-Condition ($actualLeaf -ceq $expectedLeaf) "Transaction cleanup leaf mismatch: $actualLeaf"
        Assert-Condition ($actualParent -ceq $expectedParent) "Transaction cleanup parent mismatch: $actualParent"
        Assert-Condition ($resolvedDirectory -ceq $expectedDirectory) "Transaction cleanup path mismatch: $resolvedDirectory"
        $validatedDirectories += $resolvedDirectory
    }

    foreach ($directory in $validatedDirectories) {
        if (Test-Path -LiteralPath $directory -PathType Container) {
            Remove-Item -LiteralPath $directory -Recurse -Force
        }
    }
}

function Publish-VerifiedFiles {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Items,

        [int]$FailAfterPublishCount = -1
    )

    Assert-Condition ($Items.Count -gt 0) 'Publish transaction requires at least one item'
    $transactionId = [Guid]::NewGuid().ToString('N')
    $transactionDirectories = @{}
    $createdParents = @()
    $records = @()
    $destinations = @()

    try {
        for ($index = 0; $index -lt $Items.Count; $index++) {
            $source = [System.IO.Path]::GetFullPath([string]$Items[$index].Source)
            $destination = [System.IO.Path]::GetFullPath([string]$Items[$index].Destination)
            Assert-RequiredFile $source 'Verified publish source'
            Assert-Condition (-not ($destinations -ccontains $destination)) "Duplicate publish destination: $destination"
            $destinations += $destination
            Assert-Condition (-not (Test-Path -LiteralPath $destination -PathType Container)) "Publish destination is a directory: $destination"

            $parent = Split-Path -Parent $destination
            if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
                [void](New-Item -ItemType Directory -Path $parent -Force)
                $createdParents += $parent
            }
            if (-not $transactionDirectories.ContainsKey($parent)) {
                $transactionDirectory = Join-Path $parent ".uipilot-everything-txn-$transactionId"
                [void](New-Item -ItemType Directory -Path $transactionDirectory)
                $transactionDirectories[$parent] = $transactionDirectory
            }

            $candidate = Join-Path $transactionDirectories[$parent] "$index.candidate"
            $backup = Join-Path $transactionDirectories[$parent] "$index.backup"
            Copy-Item -LiteralPath $source -Destination $candidate
            Assert-Condition ((Get-LowerSha256 $candidate) -ceq (Get-LowerSha256 $source)) "Candidate copy hash mismatch: $destination"
            $records += [pscustomobject]@{
                Destination = $destination
                Candidate = $candidate
                Backup = $backup
                HadExisting = $false
                Published = $false
            }
        }

        $publishedCount = 0
        foreach ($record in $records) {
            if (Test-Path -LiteralPath $record.Destination -PathType Leaf) {
                Move-Item -LiteralPath $record.Destination -Destination $record.Backup
                $record.HadExisting = $true
            }
            Move-Item -LiteralPath $record.Candidate -Destination $record.Destination
            $record.Published = $true
            $publishedCount++
            if ($FailAfterPublishCount -eq $publishedCount) {
                throw "Injected publish failure after $publishedCount item(s)"
            }
        }
    }
    catch {
        for ($index = $records.Count - 1; $index -ge 0; $index--) {
            $record = $records[$index]
            if ($record.Published -and (Test-Path -LiteralPath $record.Destination -PathType Leaf)) {
                Remove-Item -LiteralPath $record.Destination -Force
            }
            if ($record.HadExisting -and (Test-Path -LiteralPath $record.Backup -PathType Leaf)) {
                Move-Item -LiteralPath $record.Backup -Destination $record.Destination
            }
        }
        Remove-TransactionDirectories $transactionDirectories $transactionId
        foreach ($parent in @($createdParents | Sort-Object Length -Descending)) {
            if ((Test-Path -LiteralPath $parent -PathType Container) -and @(Get-ChildItem -LiteralPath $parent -Force).Count -eq 0) {
                Remove-Item -LiteralPath $parent -Force
            }
        }
        throw
    }

    Remove-TransactionDirectories $transactionDirectories $transactionId
}

function Invoke-TransactionSelfTest {
    $testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "uipilot-everything-transaction-test-$([Guid]::NewGuid().ToString('N'))"
    [void](New-Item -ItemType Directory -Path $testRoot)
    try {
        $sourceRoot = Join-Path $testRoot 'sources'
        $parentOne = Join-Path $testRoot 'parent-one'
        $parentTwo = Join-Path $testRoot 'parent-two'
        $parentThree = Join-Path $testRoot 'parent-three'
        foreach ($directory in @($sourceRoot, $parentOne, $parentTwo, $parentThree)) {
            [void](New-Item -ItemType Directory -Path $directory)
        }

        $sources = @(
            (Join-Path $sourceRoot 'source-one')
            (Join-Path $sourceRoot 'source-two')
            (Join-Path $sourceRoot 'source-three')
            (Join-Path $sourceRoot 'source-four')
            (Join-Path $sourceRoot 'source-five')
        )
        $destinations = @(
            (Join-Path $parentOne 'destination-one')
            (Join-Path $parentOne 'destination-two')
            (Join-Path $parentTwo 'destination-three')
            (Join-Path $parentTwo 'destination-four')
            (Join-Path $parentThree 'destination-five')
        )
        $sourceBytes = @(
            [byte[]](0, 1, 2, 3, 255)
            [byte[]](4, 5, 6, 7, 254)
            [byte[]](8, 9, 10, 11, 253)
            [byte[]](12, 13, 14, 15, 252)
            [byte[]](16, 17, 18, 19, 251)
        )
        $existingBytes = @{
            0 = [byte[]](101, 0, 102, 255)
            2 = [byte[]](103, 1, 104, 254)
            4 = [byte[]](105, 2, 106, 253)
        }
        for ($index = 0; $index -lt $sources.Count; $index++) {
            [System.IO.File]::WriteAllBytes($sources[$index], $sourceBytes[$index])
        }
        $items = @(
            [pscustomobject]@{ Source = $sources[0]; Destination = $destinations[0] }
            [pscustomobject]@{ Source = $sources[1]; Destination = $destinations[1] }
            [pscustomobject]@{ Source = $sources[2]; Destination = $destinations[2] }
            [pscustomobject]@{ Source = $sources[3]; Destination = $destinations[3] }
            [pscustomobject]@{ Source = $sources[4]; Destination = $destinations[4] }
        )

        for ($failAfter = 1; $failAfter -le $items.Count; $failAfter++) {
            for ($index = 0; $index -lt $destinations.Count; $index++) {
                if (Test-Path -LiteralPath $destinations[$index]) {
                    Remove-Item -LiteralPath $destinations[$index] -Force
                }
                if ($existingBytes.ContainsKey($index)) {
                    [System.IO.File]::WriteAllBytes($destinations[$index], $existingBytes[$index])
                }
            }

            $failed = $false
            try {
                Publish-VerifiedFiles $items $failAfter
            }
            catch {
                $failed = $true
            }
            Assert-Condition $failed "Injected transaction failure after $failAfter item(s) did not fail"
            foreach ($index in $existingBytes.Keys) {
                $expectedBytes = [Convert]::ToBase64String($existingBytes[$index])
                $actualBytes = [Convert]::ToBase64String([System.IO.File]::ReadAllBytes($destinations[$index]))
                Assert-Condition ($actualBytes -ceq $expectedBytes) "Rollback changed existing destination $index after failure point $failAfter"
            }
            foreach ($index in @(1, 3)) {
                Assert-Condition (-not (Test-Path -LiteralPath $destinations[$index])) "Rollback left originally absent destination $index after failure point $failAfter"
            }
            $residue = @(Get-ChildItem -LiteralPath $testRoot -Recurse -Directory -Filter '.uipilot-everything-txn-*')
            Assert-Condition ($residue.Count -eq 0) "Transaction residue remained after failure point $failAfter"
        }

        for ($index = 0; $index -lt $destinations.Count; $index++) {
            if (Test-Path -LiteralPath $destinations[$index]) {
                Remove-Item -LiteralPath $destinations[$index] -Force
            }
            if ($existingBytes.ContainsKey($index)) {
                [System.IO.File]::WriteAllBytes($destinations[$index], $existingBytes[$index])
            }
        }
        Publish-VerifiedFiles $items
        for ($index = 0; $index -lt $destinations.Count; $index++) {
            $expectedBytes = [Convert]::ToBase64String($sourceBytes[$index])
            $actualBytes = [Convert]::ToBase64String([System.IO.File]::ReadAllBytes($destinations[$index]))
            Assert-Condition ($actualBytes -ceq $expectedBytes) "Successful transaction did not publish destination $index"
        }
        $successResidue = @(Get-ChildItem -LiteralPath $testRoot -Recurse -Directory -Filter '.uipilot-everything-txn-*')
        Assert-Condition ($successResidue.Count -eq 0) 'Transaction residue remained after successful publish'

        $guardTransactionId = [Guid]::NewGuid().ToString('N')
        $wrongLeafDirectory = Join-Path $parentOne '.uipilot-everything-txn-wrong'
        [void](New-Item -ItemType Directory -Path $wrongLeafDirectory)
        [System.IO.File]::WriteAllText((Join-Path $wrongLeafDirectory 'sentinel'), 'keep')
        $wrongLeafFailed = $false
        try {
            Remove-TransactionDirectories @{ $parentOne = $wrongLeafDirectory } $guardTransactionId
        }
        catch {
            $wrongLeafFailed = $true
        }
        Assert-Condition $wrongLeafFailed 'Cleanup accepted a transaction directory with the wrong leaf'
        Assert-Condition (Test-Path -LiteralPath (Join-Path $wrongLeafDirectory 'sentinel') -PathType Leaf) 'Fail-closed cleanup deleted the wrong-leaf directory'

        $wrongParentDirectory = Join-Path $parentOne ".uipilot-everything-txn-$guardTransactionId"
        [void](New-Item -ItemType Directory -Path $wrongParentDirectory)
        [System.IO.File]::WriteAllText((Join-Path $wrongParentDirectory 'sentinel'), 'keep')
        $wrongParentFailed = $false
        try {
            Remove-TransactionDirectories @{ $parentTwo = $wrongParentDirectory } $guardTransactionId
        }
        catch {
            $wrongParentFailed = $true
        }
        Assert-Condition $wrongParentFailed 'Cleanup accepted a transaction directory under the wrong parent'
        Assert-Condition (Test-Path -LiteralPath (Join-Path $wrongParentDirectory 'sentinel') -PathType Leaf) 'Fail-closed cleanup deleted the wrong-parent directory'

        $validGuardDirectory = Join-Path $parentThree ".uipilot-everything-txn-$guardTransactionId"
        [void](New-Item -ItemType Directory -Path $validGuardDirectory)
        [System.IO.File]::WriteAllText((Join-Path $validGuardDirectory 'sentinel'), 'keep')
        $mixedValidationFailed = $false
        try {
            Remove-TransactionDirectories @{
                $parentThree = $validGuardDirectory
                $parentTwo = $wrongLeafDirectory
            } $guardTransactionId
        }
        catch {
            $mixedValidationFailed = $true
        }
        Assert-Condition $mixedValidationFailed 'Cleanup accepted a mixed valid and invalid transaction directory set'
        Assert-Condition (Test-Path -LiteralPath (Join-Path $validGuardDirectory 'sentinel') -PathType Leaf) 'Cleanup deleted a valid directory before all records passed validation'
        Assert-Condition (Test-Path -LiteralPath (Join-Path $wrongLeafDirectory 'sentinel') -PathType Leaf) 'Cleanup deleted an invalid directory during mixed validation'
    }
    finally {
        Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function New-ZipSelectionTestArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,

        [Parameter(Mandatory = $true)]
        [string[]]$EntryNames
    )

    $file = [System.IO.File]::Create($ArchivePath)
    try {
        $archive = [System.IO.Compression.ZipArchive]::new($file, [System.IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            foreach ($entryName in $EntryNames) {
                $entry = $archive.CreateEntry($entryName)
                $entryStream = $entry.Open()
                try {
                    $payload = [System.Text.Encoding]::UTF8.GetBytes("payload:$entryName")
                    $entryStream.Write($payload, 0, $payload.Length)
                }
                finally {
                    $entryStream.Dispose()
                }
            }
        }
        finally {
            $archive.Dispose()
        }
    }
    finally {
        $file.Dispose()
    }
}

function Invoke-ZipEntrySelectionSelfTest {
    $testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "uipilot-everything-zip-test-$([Guid]::NewGuid().ToString('N'))"
    [void](New-Item -ItemType Directory -Path $testRoot)
    try {
        $lowercaseArchive = Join-Path $testRoot 'lowercase.zip'
        $nestedArchive = Join-Path $testRoot 'nested.zip'
        $duplicateArchive = Join-Path $testRoot 'duplicate.zip'
        $output = Join-Path $testRoot 'Everything.exe'
        New-ZipSelectionTestArchive $lowercaseArchive @('everything.exe')
        New-ZipSelectionTestArchive $nestedArchive @('nested/everything.exe')
        New-ZipSelectionTestArchive $duplicateArchive @('everything.exe', 'Everything.exe')

        Copy-EverythingExecutableFromArchive $lowercaseArchive $output
        Assert-Condition ([System.IO.File]::ReadAllText($output) -ceq 'payload:everything.exe') 'Lowercase root everything.exe was not selected'
        Remove-Item -LiteralPath $output -Force

        foreach ($invalidArchive in @($nestedArchive, $duplicateArchive)) {
            $failed = $false
            try {
                Copy-EverythingExecutableFromArchive $invalidArchive $output
            }
            catch {
                $failed = $true
            }
            Assert-Condition $failed "Invalid ZIP selection unexpectedly passed: $invalidArchive"
            Assert-Condition (-not (Test-Path -LiteralPath $output)) "Invalid ZIP selection published an output: $invalidArchive"
        }
    }
    finally {
        Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

try {
    if ($SelfTestTransaction) {
        Invoke-TransactionSelfTest
        Write-Output 'EVERYTHING_FETCH_TRANSACTION_SELF_TEST_PASS'
        exit 0
    }

    if ($SelfTestZipEntrySelection) {
        Invoke-ZipEntrySelectionSelfTest
        Write-Output 'EVERYTHING_FETCH_ZIP_ENTRY_SELF_TEST_PASS'
        exit 0
    }

    $repositoryRoot = Split-Path -Parent $PSScriptRoot
    $thirdPartyRoot = Join-Path $repositoryRoot 'third-party\everything'
    $resourceRoot = Join-Path $repositoryRoot 'src-tauri\resources\everything'
    $lockPath = Join-Path $thirdPartyRoot 'everything.lock.json'
    $reviewedLicensePath = Join-Path $thirdPartyRoot 'LICENSE.txt'
    $lock = Read-ReviewedLock $lockPath
    Assert-RequiredFile $reviewedLicensePath 'Reviewed License'
    Assert-Condition ((Get-LowerSha256 $reviewedLicensePath) -ceq [string]$lock.licenseSha256) 'Reviewed License SHA-256 does not match the reviewed lock'

    $stagingRoot = Join-Path ([System.IO.Path]::GetTempPath()) "uipilot-everything-fetch-$([Guid]::NewGuid().ToString('N'))"
    [void](New-Item -ItemType Directory -Path $stagingRoot)
    $previousSecurityProtocol = [Net.ServicePointManager]::SecurityProtocol
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $stagingArtifact = Join-Path $stagingRoot $expectedArtifactFileName
        $stagingLicense = Join-Path $stagingRoot 'License.txt'
        $stagingExecutable = Join-Path $stagingRoot 'Everything.exe'
        Invoke-FixedHttpsDownload $expectedSourceUrl $stagingArtifact
        Invoke-FixedHttpsDownload $expectedLicenseUrl $stagingLicense
        Assert-Condition ((Get-LowerSha256 $stagingArtifact) -ceq [string]$lock.artifactSha256) 'Downloaded artifact SHA-256 does not match the reviewed lock'
        Assert-Condition ((Get-LowerSha256 $stagingLicense) -ceq [string]$lock.licenseSha256) 'Downloaded License SHA-256 does not match the reviewed lock'

        Copy-EverythingExecutableFromArchive $stagingArtifact $stagingExecutable
        Assert-Condition ((Get-LowerSha256 $stagingExecutable) -ceq [string]$lock.everythingExeSha256) 'Extracted Everything.exe SHA-256 does not match the reviewed lock'
        $signature = Get-AuthenticodeSignature -LiteralPath $stagingExecutable
        Assert-Condition ([string]$signature.Status -ceq 'Valid') "Everything.exe Authenticode status must be Valid, got $($signature.Status)"
        Assert-Condition ($null -ne $signature.SignerCertificate) 'Everything.exe must have a signer certificate'
        Assert-Condition ([string]$signature.SignerCertificate.Subject -ceq [string]$lock.authenticodePublisher) 'Everything.exe Authenticode publisher does not match the reviewed lock'

        $items = @(
            [pscustomobject]@{ Source = $stagingArtifact; Destination = Join-Path (Join-Path $thirdPartyRoot 'artifacts') $expectedArtifactFileName }
            [pscustomobject]@{ Source = $stagingExecutable; Destination = Join-Path $thirdPartyRoot 'Everything.exe' }
            [pscustomobject]@{ Source = $stagingExecutable; Destination = Join-Path $resourceRoot 'Everything.exe' }
            [pscustomobject]@{ Source = $reviewedLicensePath; Destination = Join-Path $resourceRoot 'LICENSE.txt' }
            [pscustomobject]@{ Source = $lockPath; Destination = Join-Path $resourceRoot 'everything.lock.json' }
        )
        Publish-VerifiedFiles $items
        Write-Output 'EVERYTHING_ARTIFACT_VERIFIED'
        Write-Output "version=$($lock.version)"
        Write-Output "architecture=$($lock.architecture)"
        Write-Output "sha256=$($lock.artifactSha256)"
    }
    finally {
        [Net.ServicePointManager]::SecurityProtocol = $previousSecurityProtocol
        Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
catch {
    Write-Error $_
    exit 1
}
