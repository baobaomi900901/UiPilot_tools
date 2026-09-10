param(
    [ValidateSet('Prepare', 'Check', 'Verify')]
    [string]$Mode = 'Check',
    [ValidatePattern('^[0-9a-f]{64}$')]
    [string]$ApprovedSha256
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not [Environment]::Is64BitProcess) { throw 'Use 64-bit PowerShell.' }
$probeRepo = Split-Path -Parent $PSScriptRoot
$source = Join-Path $probeRepo 'spikes/process-elevation-feasibility/target/release/uipilot-uac-probe.exe'
$destination = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'UiPilot Elevation Probe'
$installed = Join-Path $destination 'uipilot-uac-probe.exe'

if ($Mode -eq 'Prepare') {
    Push-Location $probeRepo
    try {
        # Static CRT avoids a pre-main VCRUNTIME DLL lookup outside the fixed package.
        & cargo rustc --manifest-path spikes/process-elevation-feasibility/Cargo.toml --locked --offline --release --features uac-probe --bin uipilot-uac-probe -- -C target-feature=+crt-static
        if ($LASTEXITCODE -ne 0) { throw 'Probe build failed.' }
    }
    finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Run -Mode Prepare to build the probe.' }
$digest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()

if ($Mode -eq 'Verify') {
    if ([string]::IsNullOrEmpty($ApprovedSha256) -or $ApprovedSha256 -cne $digest) {
        throw 'Verify requires the explicitly approved build SHA-256.'
    }
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf) -or
        (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ApprovedSha256) {
        throw 'The approved binary is not installed at the fixed probe destination.'
    }
    # Installation is deliberately not part of this runner. Verify is the only
    # mode that can request UAC and must be explicitly approved for this digest.
    & $installed inspect
    if ($LASTEXITCODE -ne 0) { throw 'Installed probe directory validation failed.' }
    & $installed host
    if ($LASTEXITCODE -ne 0) { throw 'UAC experiment did not pass; do not retry automatically.' }
    return
}

[pscustomobject]@{
    source = $source
    sha256 = $digest
    destination = $destination
    files = @('uipilot-uac-probe.exe')
    destinationExists = (Test-Path -LiteralPath $destination)
    requiredDirectorySddl = 'O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)'
    deploymentRule = 'After explicit approval only: require destination absent; create admin-owned protected directory; copy bytes from a read-locked source matching the approved hash; rehash installed file. Never reuse or repair an existing directory.'
    cleanupRule = 'After probe processes exit: verify the owned directory and exact installed file identity/hash; delete only that file and then the empty directory. No recursive removal or process-name termination.'
    installPerformed = $false
    uacRequested = $false
} | ConvertTo-Json -Depth 4
