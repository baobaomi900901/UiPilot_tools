param(
    [ValidateSet('Release')]
    [string]$Configuration = 'Release',
    [ValidateSet('ProductionHelper', 'VmInstrumentedSetup', 'ProductionBundleInput', 'VmInstrumentedBundleInput')]
    [string]$Mode = 'ProductionHelper'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Get-PeMachine([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        try {
            Assert-Condition (($reader.ReadUInt16()) -eq 0x5a4d) 'Helper is not a PE image'
            $stream.Position = 0x3c
            $peOffset = $reader.ReadUInt32()
            $stream.Position = $peOffset
            Assert-Condition (($reader.ReadUInt32()) -eq 0x00004550) 'Helper has no PE signature'
            return $reader.ReadUInt16()
        }
        finally { $reader.Dispose() }
    }
    finally { $stream.Dispose() }
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

if (-not ('UiPilot.ManifestReader' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
namespace UiPilot {
  public static class ManifestReader {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr LoadLibraryEx(string file, IntPtr fileHandle, uint flags);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32", SetLastError=true)] static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32")] static extern bool FreeLibrary(IntPtr module);
    public static string Read(string file) {
      IntPtr module = LoadLibraryEx(file, IntPtr.Zero, 0x00000002);
      if (module == IntPtr.Zero) throw new Win32Exception();
      try {
        IntPtr resource = FindResource(module, (IntPtr)1, (IntPtr)24);
        if (resource == IntPtr.Zero) throw new Win32Exception();
        uint size = SizeofResource(module, resource);
        IntPtr loaded = LoadResource(module, resource);
        IntPtr bytes = LockResource(loaded);
        byte[] data = new byte[size]; Marshal.Copy(bytes, data, 0, (int)size);
        if (size >= 2 && data[0] == 0xff && data[1] == 0xfe) return Encoding.Unicode.GetString(data);
        return Encoding.UTF8.GetString(data);
      } finally { FreeLibrary(module); }
    }
  }
}
'@
}

function Assert-HelperIdentity([string]$Path, [bool]$ExpectFaultInjection) {
    Assert-Condition (Test-Path -LiteralPath $Path -PathType Leaf) "Expected Helper artifact is missing: $Path"
    Assert-Condition ((Get-PeMachine $Path) -eq 0x8664) 'Helper must be an x64 PE image'
    $embeddedManifest = [UiPilot.ManifestReader]::Read($Path)
    Assert-Condition $embeddedManifest.Contains('requestedExecutionLevel') 'Helper manifest is missing requestedExecutionLevel'
    Assert-Condition ($embeddedManifest -match 'level\s*=\s*["'']asInvoker["'']') 'Helper manifest must be asInvoker'
    Assert-Condition ($embeddedManifest -match 'uiAccess\s*=\s*["'']false["'']') 'Helper manifest must disable uiAccess'
    $strings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Path))
    if ($ExpectFaultInjection) {
        Assert-Condition $strings.Contains('fault-injection:') 'VM Instrumented Helper is missing fault-injection code'
    } else {
        Assert-Condition (-not $strings.Contains('fault-injection:')) 'Production Helper contains fault-injection code'
    }
}

function Copy-HelperBundleInput([string]$Source, [string]$Destination, [bool]$ExpectFaultInjection) {
    Assert-HelperIdentity $Source $ExpectFaultInjection
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
    Assert-HelperIdentity $Destination $ExpectFaultInjection
    $sourceHash = Get-LowerSha256 $Source
    $destinationHash = Get-LowerSha256 $Destination
    Assert-Condition ($destinationHash -ceq $sourceHash) 'Final NSIS Helper input does not match the explicit Helper artifact'
}

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'src-tauri\Cargo.toml'
$targetTriple = 'x86_64-pc-windows-msvc'
$vmInstrumented = @('VmInstrumentedSetup', 'VmInstrumentedBundleInput') -ccontains $Mode
$bundleInputMode = @('ProductionBundleInput', 'VmInstrumentedBundleInput') -ccontains $Mode
$artifactTargetRoot = if ($vmInstrumented) {
    Join-Path $root 'src-tauri\target\vm-instrumented'
} else {
    Join-Path $root 'src-tauri\target\managed-everything-production'
}
$bundleTargetRoot = if ($vmInstrumented) { $artifactTargetRoot } else { Join-Path $root 'src-tauri\target' }
$helper = Join-Path $artifactTargetRoot "$targetTriple\release\uipilot-everything-installer.exe"
$bundleInput = Join-Path $bundleTargetRoot 'release\uipilot-everything-installer.exe'

if ($bundleInputMode) {
    Copy-HelperBundleInput $helper $bundleInput $vmInstrumented
    Write-Output "MANAGED_EVERYTHING_BUNDLE_INPUT_PASS $bundleInput"
    exit 0
}

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'test-everything-package.ps1') -ResourceOnly
if ($LASTEXITCODE -ne 0) { throw 'Reviewed Everything resource validation failed' }
$cargoArgs = @('build', '--manifest-path', $manifest, '--bin', 'uipilot-everything-installer', '--target', $targetTriple, '--release', '--target-dir', $artifactTargetRoot, '--features', 'installer-helper-build')
if ($vmInstrumented) { $cargoArgs[-1] = 'installer-helper-build,fault-injection' }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw 'Managed Everything Helper cargo build failed' }
Assert-HelperIdentity $helper $vmInstrumented

if ($Mode -ceq 'VmInstrumentedSetup') {
    $env:CARGO_TARGET_DIR = $bundleTargetRoot
    try {
        & npm run tauri -- build --config src-tauri/tauri.vm-instrumented.conf.json --bundles nsis
        if ($LASTEXITCODE -ne 0) { throw 'VM Instrumented Setup build failed' }
    }
    finally { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
    Assert-HelperIdentity $bundleInput $true
    Assert-Condition ((Get-LowerSha256 $bundleInput) -ceq (Get-LowerSha256 $helper)) 'VM final NSIS Helper input does not match the explicit instrumented Helper'
    $bundles = @(Get-ChildItem (Join-Path $bundleTargetRoot 'release\bundle\nsis') -Filter '*VM*Instrumented*setup.exe' -File -ErrorAction SilentlyContinue)
    Assert-Condition ($bundles.Count -eq 1) 'Expected exactly one independently named VM Instrumented Setup'
    Write-Output "MANAGED_EVERYTHING_VM_INSTRUMENTED_BUILD_PASS $($bundles[0].FullName)"
} else {
    Copy-HelperBundleInput $helper $bundleInput $false
    Write-Output "MANAGED_EVERYTHING_HELPER_BUILD_PASS $helper"
}
