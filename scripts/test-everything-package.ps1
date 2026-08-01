param(
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

function Get-EverythingZipEntrySha256FromArchive {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Archive
    )

    $matches = @($Archive.Entries | Where-Object {
        $normalizedName = $_.FullName.Replace('\', '/')
        $basename = @($normalizedName.Split('/'))[-1]
        $basename -ieq 'everything.exe'
    })
    Assert-Condition ($matches.Count -eq 1) 'Artifact must contain exactly one entry whose basename equals everything.exe case-insensitively'

    $entry = $matches[0]
    $normalizedEntryName = $entry.FullName.Replace('\', '/')
    Assert-Condition (-not $normalizedEntryName.Contains('/')) 'The everything.exe entry must be in the ZIP root directory'
    Assert-Condition (-not [string]::IsNullOrEmpty($entry.Name) -and -not $entry.FullName.EndsWith('/') -and -not $entry.FullName.EndsWith('\')) 'The everything.exe entry must be a regular file'

    $stream = $entry.Open()
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash($stream)
            return (($hashBytes | ForEach-Object { $_.ToString('x2') }) -join '')
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Get-EverythingZipEntrySha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath
    )

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        return Get-EverythingZipEntrySha256FromArchive $archive
    }
    finally {
        $archive.Dispose()
    }
}

function Invoke-InMemoryZipSelectionCase {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$EntryNames,

        [Parameter(Mandatory = $true)]
        [bool]$ShouldPass
    )

    $memory = [System.IO.MemoryStream]::new()
    try {
        $writer = [System.IO.Compression.ZipArchive]::new($memory, [System.IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            foreach ($entryName in $EntryNames) {
                $entry = $writer.CreateEntry($entryName)
                if (-not $entryName.EndsWith('/') -and -not $entryName.EndsWith('\')) {
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
        }
        finally {
            $writer.Dispose()
        }

        $memory.Position = 0
        $reader = [System.IO.Compression.ZipArchive]::new($memory, [System.IO.Compression.ZipArchiveMode]::Read, $true)
        try {
            $failed = $false
            try {
                $hash = Get-EverythingZipEntrySha256FromArchive $reader
            }
            catch {
                $failed = $true
            }

            if ($ShouldPass) {
                Assert-Condition (-not $failed) "Expected ZIP selection to pass: $($EntryNames -join ', ')"
                Assert-LowerSha256 $hash 'in-memory everything.exe SHA-256'
            }
            else {
                Assert-Condition $failed "Expected ZIP selection to fail: $($EntryNames -join ', ')"
            }
        }
        finally {
            $reader.Dispose()
        }
    }
    finally {
        $memory.Dispose()
    }
}

try {
    if ($SelfTestZipEntrySelection) {
        Invoke-InMemoryZipSelectionCase @('everything.exe') $true
        Invoke-InMemoryZipSelectionCase @('nested/everything.exe') $false
        Invoke-InMemoryZipSelectionCase @('everything.exe', 'Everything.exe') $false
        Write-Output 'EVERYTHING_ZIP_ENTRY_SELF_TEST_PASS'
        exit 0
    }

    $repositoryRoot = Split-Path -Parent $PSScriptRoot
    $thirdPartyRoot = Join-Path $repositoryRoot 'third-party\everything'
    $resourceRoot = Join-Path $repositoryRoot 'src-tauri\resources\everything'
    $lockPath = Join-Path $thirdPartyRoot 'everything.lock.json'
    $licensePath = Join-Path $thirdPartyRoot 'LICENSE.txt'
    $artifactPath = Join-Path (Join-Path $thirdPartyRoot 'artifacts') $expectedArtifactFileName
    $stagedExecutablePath = Join-Path $thirdPartyRoot 'Everything.exe'
    $resourceLockPath = Join-Path $resourceRoot 'everything.lock.json'
    $resourceLicensePath = Join-Path $resourceRoot 'LICENSE.txt'
    $resourceExecutablePath = Join-Path $resourceRoot 'Everything.exe'

    Assert-RequiredFile $lockPath 'Reviewed lock'
    Assert-RequiredFile $licensePath 'Reviewed License'
    Assert-RequiredFile $artifactPath 'Reviewed artifact'
    Assert-RequiredFile $stagedExecutablePath 'Reviewed Everything.exe'
    Assert-RequiredFile $resourceLockPath 'Resource lock copy'
    Assert-RequiredFile $resourceLicensePath 'Resource License copy'
    Assert-RequiredFile $resourceExecutablePath 'Resource Everything.exe copy'

    $lock = Get-Content -LiteralPath $lockPath -Raw -Encoding utf8 | ConvertFrom-Json
    Assert-Condition ($null -ne $lock -and $lock -is [psobject]) 'everything.lock.json must contain one JSON object'
    Assert-ExactProperties $lock $expectedLockProperties

    Assert-Condition ($lock.schemaVersion -is [long] -or $lock.schemaVersion -is [int]) 'schemaVersion must be an integer'
    Assert-Condition ([long]$lock.schemaVersion -eq 1) 'schemaVersion must be exactly 1'
    Assert-Condition ([string]$lock.version -ceq $expectedVersion) "version must be exactly $expectedVersion"
    Assert-Condition ([string]$lock.architecture -ceq $expectedArchitecture) "architecture must be exactly $expectedArchitecture"
    Assert-Condition ([string]$lock.artifactFileName -ceq $expectedArtifactFileName) "artifactFileName must be exactly $expectedArtifactFileName"
    Assert-Condition ([string]$lock.sourceUrl -ceq $expectedSourceUrl) "sourceUrl must be exactly $expectedSourceUrl"
    Assert-Condition ([string]$lock.license -ceq $expectedLicense) "license must be exactly $expectedLicense"
    Assert-Condition ([string]$lock.licenseUrl -ceq $expectedLicenseUrl) "licenseUrl must be exactly $expectedLicenseUrl"

    $sourceUri = [Uri]$lock.sourceUrl
    Assert-Condition ($sourceUri.IsAbsoluteUri -and $sourceUri.Scheme -ceq 'https') 'sourceUrl must be an absolute HTTPS URL'
    Assert-Condition ($sourceUri.Host -ceq 'www.voidtools.com') 'sourceUrl must use the official www.voidtools.com host'
    Assert-Condition ($sourceUri.AbsolutePath -ceq "/$expectedArtifactFileName") 'sourceUrl path must identify the frozen artifact exactly'
    Assert-Condition ([string]::IsNullOrEmpty($sourceUri.Query) -and [string]::IsNullOrEmpty($sourceUri.Fragment)) 'sourceUrl must not contain a query or fragment'

    Assert-LowerSha256 ([string]$lock.artifactSha256) 'artifactSha256'
    Assert-LowerSha256 ([string]$lock.everythingExeSha256) 'everythingExeSha256'
    Assert-LowerSha256 ([string]$lock.licenseSha256) 'licenseSha256'

    $publisher = [string]$lock.authenticodePublisher
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($publisher)) 'authenticodePublisher must contain the independently reviewed certificate Subject'
    Assert-Condition ($publisher -ceq $publisher.Trim()) 'authenticodePublisher must not contain surrounding whitespace'
    Assert-Condition (-not $publisher.Contains("`r") -and -not $publisher.Contains("`n")) 'authenticodePublisher must be one line'

    Assert-Condition ((Get-LowerSha256 $artifactPath) -ceq [string]$lock.artifactSha256) 'Artifact SHA-256 does not match the reviewed lock'
    Assert-Condition ((Get-EverythingZipEntrySha256 $artifactPath) -ceq [string]$lock.everythingExeSha256) 'everything.exe inside the artifact does not match the reviewed lock'
    Assert-Condition ((Get-LowerSha256 $stagedExecutablePath) -ceq [string]$lock.everythingExeSha256) 'Staged Everything.exe does not match the reviewed lock'
    Assert-Condition ((Get-LowerSha256 $resourceExecutablePath) -ceq [string]$lock.everythingExeSha256) 'Resource Everything.exe does not match the reviewed lock'
    Assert-Condition ((Get-LowerSha256 $licensePath) -ceq [string]$lock.licenseSha256) 'License SHA-256 does not match the reviewed lock'
    Assert-Condition ((Get-LowerSha256 $resourceLicensePath) -ceq [string]$lock.licenseSha256) 'Resource License does not match the reviewed lock'
    Assert-Condition ((Get-LowerSha256 $resourceLockPath) -ceq (Get-LowerSha256 $lockPath)) 'Resource lock is not an exact copy of the reviewed lock'

    $signature = Get-AuthenticodeSignature -LiteralPath $stagedExecutablePath
    Assert-Condition ([string]$signature.Status -ceq 'Valid') "Everything.exe Authenticode status must be Valid, got $($signature.Status)"
    Assert-Condition ($null -ne $signature.SignerCertificate) 'Everything.exe must have a signer certificate'
    Assert-Condition ([string]$signature.SignerCertificate.Subject -ceq $publisher) 'Everything.exe Authenticode publisher does not match the reviewed lock'

    $resourceSignature = Get-AuthenticodeSignature -LiteralPath $resourceExecutablePath
    Assert-Condition ([string]$resourceSignature.Status -ceq 'Valid') "Resource Everything.exe Authenticode status must be Valid, got $($resourceSignature.Status)"
    Assert-Condition ($null -ne $resourceSignature.SignerCertificate) 'Resource Everything.exe must have a signer certificate'
    Assert-Condition ([string]$resourceSignature.SignerCertificate.Subject -ceq $publisher) 'Resource Everything.exe Authenticode publisher does not match the reviewed lock'

    Write-Output 'EVERYTHING_PACKAGE_PASS'
}
catch {
    Write-Error $_
    exit 1
}
