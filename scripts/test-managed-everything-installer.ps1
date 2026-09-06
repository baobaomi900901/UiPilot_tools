param(
    [ValidateSet('Source', 'SelfTest', 'VmInstall', 'VmUpdate', 'VmCancelBeforeInstall', 'VmSourceReplacement', 'VmSidPreservation', 'VmFaultCheckpoint', 'VmUninstallCheckpoint', 'VmConcurrency', 'VmExplicitUninstall')]
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

function Assert-OrderedTokens {
    param([string]$Text, [string[]]$Tokens, [string]$Message)
    $start = 0
    foreach ($token in $Tokens) {
        $index = $Text.IndexOf($token, $start, [StringComparison]::Ordinal)
        if ($index -lt 0) { throw $Message }
        $start = $index + $token.Length
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

function Add-ManifestReader {
    if ('UiPilot.ManagedEverythingInstaller.ManifestReader' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
namespace UiPilot.ManagedEverythingInstaller {
  public static class ManifestReader {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr LoadLibraryEx(string file, IntPtr handle, uint flags);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32", SetLastError=true)] static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32", SetLastError=true)] static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32")] static extern IntPtr LockResource(IntPtr resource);
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

function Add-InstallerPathScopeProbe {
    if ('UiPilot.ManagedEverythingInstaller.PinnedDirectoryScope' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace UiPilot.ManagedEverythingInstaller {
  public sealed class PinnedDirectoryScope : IDisposable {
    const uint FILE_READ_ATTRIBUTES = 0x00000080;
    const uint FILE_SHARE_READ = 0x00000001, FILE_SHARE_WRITE = 0x00000002;
    const uint FILE_SHARE_DELETE = 0x00000004;
    const uint OPEN_EXISTING = 3;
    const uint FILE_ATTRIBUTE_DIRECTORY = 0x00000010, FILE_ATTRIBUTE_REPARSE_POINT = 0x00000400;
    const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000, FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    static readonly IntPtr INVALID_HANDLE_VALUE = new IntPtr(-1);

    [StructLayout(LayoutKind.Sequential)]
    struct FILETIME { public uint LowDateTime; public uint HighDateTime; }
    [StructLayout(LayoutKind.Sequential)]
    struct BY_HANDLE_FILE_INFORMATION {
      public uint FileAttributes;
      public FILETIME CreationTime, LastAccessTime, LastWriteTime;
      public uint VolumeSerialNumber, FileSizeHigh, FileSizeLow, NumberOfLinks, FileIndexHigh, FileIndexLow;
    }

    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetFileInformationByHandle(IntPtr handle, out BY_HANDLE_FILE_INFORMATION information);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint GetFinalPathNameByHandle(IntPtr handle, StringBuilder path, uint length, uint flags);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);

    readonly List<IntPtr> handles = new List<IntPtr>();
    readonly List<string> expectedPaths = new List<string>();
    readonly uint shareMode;
    public string Path { get; private set; }
    public static uint ProtectedDirectoryShareMode { get { return FILE_SHARE_READ | FILE_SHARE_WRITE; } }
    public static uint RenameTestDirectoryShareMode { get { return FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE; } }

    PinnedDirectoryScope(string path, uint share) { Path = path; shareMode = share; }

    static string TrimDirectory(string path) {
      string root = System.IO.Path.GetPathRoot(path);
      return path.Length == root.Length ? path : path.TrimEnd('\\');
    }

    static string NormalizeFinalPath(string path) {
      if (path.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) return @"\\" + path.Substring(8);
      if (path.StartsWith(@"\\?\", StringComparison.OrdinalIgnoreCase)) return path.Substring(4);
      return path;
    }

    static void VerifyExactDirectory(IntPtr handle, string expectedPath) {
      BY_HANDLE_FILE_INFORMATION information;
      if (!GetFileInformationByHandle(handle, out information)) throw new Win32Exception();
      if ((information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0) throw new InvalidOperationException("Custom install scope component is not a directory");
      if ((information.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) throw new InvalidOperationException("Custom install scope contains a reparse point");
      var buffer = new StringBuilder(32768);
      uint written = GetFinalPathNameByHandle(handle, buffer, (uint)buffer.Capacity, 0);
      if (written == 0 || written >= buffer.Capacity) throw new Win32Exception();
      string actual = TrimDirectory(System.IO.Path.GetFullPath(NormalizeFinalPath(buffer.ToString())));
      string expected = TrimDirectory(System.IO.Path.GetFullPath(expectedPath));
      if (!String.Equals(actual, expected, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Custom install scope canonical path mismatch");
    }

    void PinExactDirectory(string expectedPath) {
      IntPtr handle = CreateFile(
        expectedPath,
        FILE_READ_ATTRIBUTES,
        shareMode,
        IntPtr.Zero,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        IntPtr.Zero);
      if (handle == INVALID_HANDLE_VALUE) throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not pin custom install directory component");
      try {
        VerifyExactDirectory(handle, expectedPath);
        handles.Add(handle);
        expectedPaths.Add(expectedPath);
        handle = IntPtr.Zero;
      } finally {
        if (handle != IntPtr.Zero && handle != INVALID_HANDLE_VALUE) CloseHandle(handle);
      }
    }

    public void ValidateUnchanged() {
      if (handles.Count == 0 || handles.Count != expectedPaths.Count) throw new ObjectDisposedException("PinnedDirectoryScope");
      for (int index = 0; index < handles.Count; index++) VerifyExactDirectory(handles[index], expectedPaths[index]);
    }

    static PinnedDirectoryScope OpenWithShare(string rootPath, string candidatePath, uint share) {
      if (String.IsNullOrWhiteSpace(rootPath) || String.IsNullOrWhiteSpace(candidatePath)) throw new ArgumentException("Custom install scope paths are required");
      string root = TrimDirectory(System.IO.Path.GetFullPath(rootPath));
      string candidate = TrimDirectory(System.IO.Path.GetFullPath(candidatePath));
      string prefix = root + "\\";
      if (!candidate.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Custom install directory escaped VmTestRoot");
      if (!Directory.Exists(root) || !Directory.Exists(candidate)) throw new InvalidOperationException("Custom install scope directories must already exist");

      var scope = new PinnedDirectoryScope(candidate, share);
      try {
        scope.PinExactDirectory(root);
        string current = root;
        foreach (string component in candidate.Substring(prefix.Length).Split(new[] {'\\'}, StringSplitOptions.RemoveEmptyEntries)) {
          if (component == "." || component == "..") throw new InvalidOperationException("Custom install scope is not canonical");
          current = System.IO.Path.Combine(current, component);
          scope.PinExactDirectory(current);
        }
        return scope;
      } catch {
        scope.Dispose();
        throw;
      }
    }

    public static PinnedDirectoryScope Open(string rootPath, string candidatePath) {
      return OpenWithShare(rootPath, candidatePath, ProtectedDirectoryShareMode);
    }

    public static PinnedDirectoryScope OpenForRenameTest(string rootPath, string candidatePath) {
      return OpenWithShare(rootPath, candidatePath, RenameTestDirectoryShareMode);
    }

    public void Dispose() {
      for (int index = handles.Count - 1; index >= 0; index--) CloseHandle(handles[index]);
      handles.Clear();
      expectedPaths.Clear();
    }
  }
}
'@
}

function Resolve-ScopedInstallDirectory {
    if ([string]::IsNullOrWhiteSpace($InstallDirectory)) { return $null }
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($VmTestRoot)) 'A custom install directory requires VmTestRoot'
    Add-InstallerPathScopeProbe
    return [UiPilot.ManagedEverythingInstaller.PinnedDirectoryScope]::Open($VmTestRoot, $InstallDirectory)
}

function Assert-RedactedEvidence([string]$Json, [string]$RepositoryRoot) {
    foreach ($value in @($env:COMPUTERNAME, $env:USERNAME, $env:USERPROFILE, $RepositoryRoot)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            Assert-Condition (-not $Json.Contains($value)) 'Installer evidence contains machine, user, or path data'
        }
    }
    Assert-Condition ($Json -notmatch '(?i)S-1-\d+(?:-\d+){1,14}') 'Installer evidence contains SID data'
    Assert-Condition ($Json -notmatch '(?i)[A-Z]:[\\/]') 'Installer evidence contains absolute path data'
    Assert-Condition ($Json -notmatch '(?i)\\\\[^\\\s]+\\') 'Installer evidence contains UNC path data'
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

function Get-ManagedDesktopUserSid {
    $programData = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)
    $managedRoot = Join-Path $programData 'UiPilot_tools\everything'
    $rootItem = Get-Item -LiteralPath $managedRoot -Force -ErrorAction Stop
    Assert-Condition $rootItem.PSIsContainer 'Managed runtime root is not a directory'
    Assert-Condition (($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Managed runtime root is a reparse point'
    $manifestPath = Resolve-StrictLeaf (Join-Path $rootItem.FullName 'managed-runtime.json') 'Managed runtime manifest'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    Assert-Condition ($manifest.schema_version -eq 1) 'Managed runtime manifest version mismatch'
    Assert-Condition ($manifest.instance -ceq 'UiPilot') 'Managed runtime manifest instance mismatch'
    Assert-Condition ($manifest.service_name -ceq 'Everything (UiPilot)') 'Managed runtime manifest Service mismatch'
    $sid = [string]$manifest.desktop_user_sid
    Assert-Condition ($sid -match '^S-1-(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))+$') 'Managed runtime desktop SID is invalid'
    return $sid
}

function Test-DesktopSidPreserved([string]$Before, [string]$After) {
    return -not [string]::IsNullOrWhiteSpace($Before) -and $Before -ceq $After
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
    $savedVmTestRoot = $VmTestRoot
    $savedInstallDirectory = $InstallDirectory
    try {
        $marker = Join-Path $tempRoot 'marker.txt'
        [IO.File]::WriteAllText($marker, 'not-an-acknowledgement')
        $caught = $false
        try { Assert-DisposableMarker $marker | Out-Null } catch { $caught = $true }
        Assert-Condition $caught 'Disposable marker guard accepted invalid content'
        [IO.File]::WriteAllText($marker, $script:DisposableMarkerText)
        Assert-Condition ((Assert-DisposableMarker $marker) -ceq $marker) 'Disposable marker guard rejected exact acknowledgement'
        $caught = $false
        try { Assert-RedactedEvidence '{"value":"\\\\server\\share\\secret"}' (Split-Path -Parent $PSScriptRoot) } catch { $caught = $true }
        Assert-Condition $caught 'Installer evidence redaction accepted a UNC path'
        $caught = $false
        try { Resolve-VmInstrumentedSetup (Split-Path -Parent $PSScriptRoot) | Out-Null } catch { $caught = $true }
        Assert-Condition $caught 'VM Setup guard accepted a missing path'

        $scopeRoot = Join-Path $tempRoot 'scope'
        $junctionParent = Join-Path $scopeRoot 'nested'
        $junctionTarget = Join-Path $tempRoot 'outside'
        $junction = Join-Path $junctionParent 'redirect'
        [IO.Directory]::CreateDirectory($junctionParent) | Out-Null
        [IO.Directory]::CreateDirectory((Join-Path $junctionTarget 'deeper\install')) | Out-Null
        New-Item -ItemType Junction -Path $junction -Target $junctionTarget | Out-Null
        $script:VmTestRoot = $scopeRoot
        $script:InstallDirectory = Join-Path $junction 'deeper\install'
        $caught = $false
        try {
            $scope = Resolve-ScopedInstallDirectory
            if ($scope -is [IDisposable]) { $scope.Dispose() }
        } catch { $caught = $true }
        Assert-Condition $caught 'Custom install scope accepted a nested junction escape'
        Assert-Condition (Test-DesktopSidPreserved 'S-1-5-21-1-2-3-1001' 'S-1-5-21-1-2-3-1001') 'SID preservation self-test rejected equal canonical SIDs'
        Assert-Condition (-not (Test-DesktopSidPreserved 'S-1-5-21-1-2-3-1001' 'S-1-5-21-1-2-3-1002')) 'SID preservation self-test accepted a changed SID'

        $validInstall = Join-Path $scopeRoot 'valid\install'
        [IO.Directory]::CreateDirectory($validInstall) | Out-Null
        $script:InstallDirectory = $validInstall
        $scope = [UiPilot.ManagedEverythingInstaller.PinnedDirectoryScope]::OpenForRenameTest($scopeRoot, $validInstall)
        try {
            Assert-Condition ([UiPilot.ManagedEverythingInstaller.PinnedDirectoryScope]::ProtectedDirectoryShareMode -eq 3) 'VM path handles unexpectedly permit directory replacement'
            Assert-Condition ([UiPilot.ManagedEverythingInstaller.PinnedDirectoryScope]::RenameTestDirectoryShareMode -eq 7) 'Rename-test handles do not permit the identity test'
            Assert-Condition ($scope.Path -ceq [IO.Path]::GetFullPath($validInstall)) 'Custom install scope did not return its canonical path'
            $movedInstall = Join-Path $scopeRoot 'moved'
            Move-Item -LiteralPath $validInstall -Destination $movedInstall -ErrorAction Stop
            Assert-Condition ((Test-Path -LiteralPath $movedInstall -PathType Container) -and -not (Test-Path -LiteralPath $validInstall)) 'Rename identity self-test did not complete the directory move'
            $caught = $false
            try { $scope.ValidateUnchanged() } catch { $caught = $true }
            Assert-Condition $caught 'Custom install scope did not detect a renamed directory identity'
        }
        finally { $scope.Dispose() }
        Write-Output 'MANAGED_EVERYTHING_INSTALLER_SELFTEST_PASS'
    }
    finally {
        $script:VmTestRoot = $savedVmTestRoot
        $script:InstallDirectory = $savedInstallDirectory
        if (Test-Path -LiteralPath $tempRoot -PathType Container) { Remove-Item -LiteralPath $tempRoot -Recurse -Force }
    }
}

function Invoke-InstallerVmMode([string]$RepositoryRoot) {
    $marker = Assert-VmAdmission
    $setup = Resolve-VmInstrumentedSetup $RepositoryRoot
    $setupHash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
    $flow = switch ($Mode) {
        'VmUpdate' { 'TauriUpdate' }
        'VmExplicitUninstall' { 'MaintenanceUninstall' }
        'VmUninstallCheckpoint' { 'MaintenanceUninstall' }
        default { $SetupFlow }
    }
    if (@('VmFaultCheckpoint', 'VmUninstallCheckpoint') -contains $Mode) { Assert-Condition (-not [string]::IsNullOrWhiteSpace($Checkpoint)) "$Mode requires Checkpoint" }
    if ($Mode -ceq 'VmUninstallCheckpoint') { Assert-Condition $Checkpoint.StartsWith('uninstall-') 'VmUninstallCheckpoint accepts only uninstall checkpoints' }
    $installScope = Resolve-ScopedInstallDirectory
    try {
        $scopedInstall = if ($null -eq $installScope) { $null } else { $installScope.Path }
        if ($Mode -ceq 'VmSourceReplacement') { Assert-Condition (-not [string]::IsNullOrWhiteSpace($scopedInstall)) 'VmSourceReplacement requires a scoped InstallDirectory' }
        $arguments = Get-SetupArguments $flow $scopedInstall

        if ($Mode -ceq 'VmConcurrency') {
            $oldMarker = $env:UIPILOT_VM_DISPOSABLE_MARKER
            try {
                $env:UIPILOT_VM_DISPOSABLE_MARKER = $marker
                if ($null -ne $installScope) { $installScope.ValidateUnchanged() }
                $first = if ($arguments.Count -eq 0) { Start-Process -FilePath $setup -PassThru } else { Start-Process -FilePath $setup -ArgumentList $arguments -PassThru }
                if ($null -ne $installScope) { $installScope.ValidateUnchanged() }
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

        if ($Mode -ceq 'VmSourceReplacement') {
            throw 'VmSourceReplacement is blocked until the Instrumented Setup provides a synchronized low-privilege source-handle probe'
        }

        if ($Mode -ceq 'VmSidPreservation') {
            $sidBefore = Get-ManagedDesktopUserSid
            if ($null -ne $installScope) { $installScope.ValidateUnchanged() }
            $exitCode = Invoke-OneVmSetup $setup $arguments $marker $null
            $sidAfter = if ($exitCode -eq 0) { Get-ManagedDesktopUserSid } else { $null }
            $sidPreserved = $exitCode -eq $ExpectedExitCode -and (Test-DesktopSidPreserved $sidBefore $sidAfter)
            Write-InstallerVmEvidence $RepositoryRoot ([ordered]@{
                schema_version = 1; kind = 'installer-sid-preservation'; mode = $Mode.ToLowerInvariant()
                outcome = $(if ($sidPreserved) { 'pass' } else { 'fail' }); setup_sha256 = $setupHash
                expected_exit_code = $ExpectedExitCode; actual_exit_code = $exitCode; desktop_sid_preserved = $sidPreserved
            })
            if (-not $sidPreserved) {
                if ($exitCode -ne 0) { exit $exitCode }
                exit 1
            }
            return
        }

        $fault = if (@('VmFaultCheckpoint', 'VmUninstallCheckpoint') -contains $Mode) { $Checkpoint } else { $null }
        if ($null -ne $installScope) { $installScope.ValidateUnchanged() }
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
    finally {
        if ($null -ne $installScope) { $installScope.Dispose() }
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
$helperWindows = Join-Path $helperRoot 'windows.rs'
$cargoManifest = Join-Path $root 'src-tauri\Cargo.toml'
$template = Join-Path $root 'src-tauri\windows\installer.nsi'
$hooks = Join-Path $root 'src-tauri\windows\nsis-hooks.nsh'
$build = Join-Path $root 'scripts\build-managed-everything-installer.ps1'
$package = Join-Path $root 'package.json'
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
    "[ValidateSet('Local', 'Source', 'SelfTest')]",
    'Helper tests', 'Installer Source', 'Production NSIS',
    "'--bin', 'uipilot-everything-installer', '--features', 'installer-helper-build'",
    "Action = { Invoke-CapturedCommand 'cargo.exe' @('test', '--manifest-path', 'src-tauri/Cargo.toml') }"
)
Assert-SourceContains $vmMatrix @('# Managed Everything Windows VM Matrix', '## Row 1', '## Row 13', 'VmInstall', 'VmUpdate', 'Status: `NOT RUN`')
$installerHarness = Get-Content -LiteralPath $PSCommandPath -Raw -Encoding utf8
foreach ($contract in @(
    'synchronized low-privilege source-handle probe', 'VmSidPreservation',
    'desktop_sid_preserved', '$installScope.ValidateUnchanged()'
)) {
    Assert-Condition $installerHarness.Contains($contract) "Installer VM harness contract is missing: $contract"
}
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
Assert-SourceContains $helperWindows @(
    'GetShellWindow', 'GetWindowThreadProcessId', 'ProcessIdToSessionId',
    'OpenProcessToken', 'GetTokenInformation', 'ConvertSidToStringSidW',
    'CreateFileW', 'FILE_SHARE_READ', 'OPEN_EXISTING', 'FILE_FLAG_OPEN_REPARSE_POINT',
    'OpenSCManagerW', 'QueryServiceConfigW', 'QueryServiceStatusEx',
    'WaitForSingleObject', 'GetSecurityInfo', 'GetNamedSecurityInfoW',
    'WinVerifyTrust', 'CryptQueryObject', 'CertStrToNameW', 'GetFileVersionInfoW',
    'GetSecurityDescriptorControl', 'GetSecurityDescriptorDacl', 'GetAclInformation',
    'GetAce', 'validate_pipe_sddl'
)
$cargoSource = Get-Content -LiteralPath $cargoManifest -Raw -Encoding utf8
Assert-Condition ($cargoSource.Contains('required-features = ["installer-helper-build"]')) 'Helper Cargo target must require its explicit build feature so Tauri cannot install it as an application binary'
$knownFolderFunction = [regex]::Match((Get-Content -LiteralPath $helperWindows -Raw -Encoding utf8), '(?ms)^fn known_folder\(.*?^}')
Assert-Condition $knownFolderFunction.Success 'Helper known-folder function is missing'
Assert-Condition $knownFolderFunction.Value.Contains('CoTaskMemFree') 'Helper known-folder buffer must use CoTaskMemFree'
Assert-Condition (-not $knownFolderFunction.Value.Contains('LocalFree')) 'Helper known-folder buffer must not use LocalFree'
$knownFolderDecision = $knownFolderFunction.Value.IndexOf('let path = if result < 0 || value.is_null()', [StringComparison]::Ordinal)
$knownFolderGuard = $knownFolderFunction.Value.IndexOf('if !value.is_null()', [StringComparison]::Ordinal)
$knownFolderFree = $knownFolderFunction.Value.IndexOf('CoTaskMemFree(value.cast())', [StringComparison]::Ordinal)
$knownFolderReturn = $knownFolderFunction.Value.LastIndexOf('    path', [StringComparison]::Ordinal)
Assert-Condition ($knownFolderDecision -ge 0 -and $knownFolderDecision -lt $knownFolderGuard -and $knownFolderGuard -lt $knownFolderFree -and $knownFolderFree -lt $knownFolderReturn) 'Helper known-folder buffer must be released on both success and failure before returning'
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
    '${UnStrCase} $UiPilotOperationId $UiPilotOperationId "L"',
    'OpenProcessToken', 'GetTokenInformation', 'ConvertSidToStringSidW',
    'ConvertStringSidToSidW', 'SetNamedSecurityInfoW',
    'O:$UiPilotOwnerSid${UIPILOT_STAGING_SDDL}',
    'File "/oname=uipilot-everything-installer.exe" "${__FILEDIR__}\..\..\uipilot-everything-installer.exe"'
)
Assert-SourceContains $template @(
    'TAURI_CLI_VERSION: 2.11.4', 'UIPILOT_REGION_REINSTALL_BEGIN',
    'UIPILOT_REGION_UNINSTALL_GATE_BEGIN', 'UIPILOT_REGION_HELPER_LIFETIME_BEGIN'
)
Assert-SourceContains $build @(
    'x86_64-pc-windows-msvc', '--release', 'uipilot-everything-installer.exe',
    'requestedExecutionLevel', 'asInvoker', 'VmInstrumentedSetup',
    'target\vm-instrumented', 'installer-helper-build,fault-injection',
    'ProductionBundleInput', 'VmInstrumentedBundleInput'
)
Assert-SourceContains $package @('stage:managed-everything:production', 'stage:managed-everything:vm')
Assert-SourceContains $tauri @(
    '"installMode": "perMachine"', '"installerHooks"', '"template"',
    '"beforeBundleCommand": "npm run stage:managed-everything:production"'
)
Assert-SourceContains $vmTauri @(
    'VM Instrumented Setup', 'vm-instrumented',
    '"beforeBundleCommand": "npm run stage:managed-everything:vm"'
)
$hooksSource = Get-Content -LiteralPath $hooks -Raw -Encoding utf8
Assert-Condition $hooksSource.Contains('!define UIPILOT_NAMED_OBJECT_SDDL "O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)"') 'NSIS named-object creation must retain the exact raw GA descriptor'
Assert-Condition $hooksSource.Contains('!define UIPILOT_MUTEX_OBJECT_SDDL "O:BAG:BAD:P(A;;0x1f0001;;;SY)(A;;0x1f0001;;;BA)"') 'NSIS mutex validation must use the kernel-normalized mutex access mask'
Assert-Condition $hooksSource.Contains('!define UIPILOT_EVENT_OBJECT_SDDL "O:BAG:BAD:P(A;;0x1f0003;;;SY)(A;;0x1f0003;;;BA)"') 'NSIS event validation must use the kernel-normalized event access mask'
Assert-Condition $hooksSource.Contains('!insertmacro UIPILOT_VALIDATE_NAMED_OBJECT $UiPilotMutexHandle ${UIPILOT_MUTEX_OBJECT_SDDL}') 'NSIS must validate an existing mutex against the mutex-specific descriptor'
Assert-Condition $hooksSource.Contains('!insertmacro UIPILOT_VALIDATE_NAMED_OBJECT $R5 ${UIPILOT_EVENT_OBJECT_SDDL}') 'NSIS must validate an existing event against the event-specific descriptor'
$validateNamedObjectMacro = [regex]::Match($hooksSource, '(?ms)^!macro UIPILOT_VALIDATE_NAMED_OBJECT HANDLE EXPECTED_SDDL\r?\n.*?^!macroend')
Assert-Condition $validateNamedObjectMacro.Success 'NSIS named-object validation macro must accept the expected canonical descriptor'
Assert-Condition $validateNamedObjectMacro.Value.Contains('${If} $R7 != "${EXPECTED_SDDL}"') 'NSIS named-object validation macro must compare the actual descriptor with its expected-descriptor parameter'
Assert-Condition (-not $hooksSource.Contains('x86_64-pc-windows-msvc\release\uipilot-everything-installer.exe')) 'NSIS must consume the late-staged Helper from its own final release target'
$createStagingMacro = [regex]::Match($hooksSource, '(?ms)^!macro UIPILOT_CREATE_STAGING .*?^!macroend')
Assert-Condition $createStagingMacro.Success 'NSIS staging macro is missing'
Assert-Condition $createStagingMacro.Value.Contains('SHGetKnownFolderPath(g "{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}", i 0, p 0, *p .R7)') 'NSIS known-folder API must return its allocated pointer into a fixed System register'
Assert-Condition $createStagingMacro.Value.Contains('lstrcpynW(w .R4, p R7') 'NSIS known-folder pointer must be copied into a separate path register before release'
Assert-Condition $createStagingMacro.Value.Contains('ole32::CoTaskMemFree(p R7)') 'NSIS known-folder buffer must use CoTaskMemFree'
Assert-Condition (-not $createStagingMacro.Value.Contains('UIPILOT_LOCAL_FREE $R7')) 'NSIS known-folder buffer must not use LocalFree'
Assert-Condition $createStagingMacro.Value.Contains('CreateDirectoryW(w "$UiPilotStaging", p R9)') 'NSIS protected staging directory must use the named variable value'
$stagingFileOwnerMacro = [regex]::Match($hooksSource, '(?ms)^!macro UIPILOT_SET_STAGING_FILE_OWNER .*?^!macroend')
Assert-Condition $stagingFileOwnerMacro.Success 'NSIS staging file owner macro is missing'
Assert-Condition $stagingFileOwnerMacro.Value.Contains('StrCpy $R4 "${PATH}"') 'NSIS staging file owner path must be copied into a fixed System register'
Assert-Condition $stagingFileOwnerMacro.Value.Contains('SetNamedSecurityInfoW(w R4, i 1, i 1, p R8') 'NSIS staging file owner API must consume the fixed path register'
Assert-Condition (-not $stagingFileOwnerMacro.Value.Contains('SetNamedSecurityInfoW(w ${PATH}')) 'NSIS must not pass a macro-expanded custom variable directly to the System plug-in'
Assert-Condition (-not $hooksSource.Contains('[DEBUG-staging-owner]')) 'Temporary staging owner instrumentation must not ship'
$acquireLeaseMacro = [regex]::Match($hooksSource, '(?ms)^!macro UIPILOT_ACQUIRE_LEASE\r?\n.*?^!macroend')
Assert-Condition $acquireLeaseMacro.Success 'NSIS installer lease macro is missing'
$rawNamedObjectDescriptorCalls = [regex]::Matches($acquireLeaseMacro.Value, [regex]::Escape('!insertmacro UIPILOT_SECURITY_ATTRIBUTES "${UIPILOT_NAMED_OBJECT_SDDL}"'))
Assert-Condition ($rawNamedObjectDescriptorCalls.Count -eq 2) 'NSIS must use the raw GA descriptor exactly once for mutex creation and once for event creation'
Assert-Condition ([regex]::IsMatch($acquireLeaseMacro.Value, '(?m)^\s*!insertmacro UIPILOT_SECURITY_ATTRIBUTES "\$\{UIPILOT_NAMED_OBJECT_SDDL\}"\r?\n\s*System::Call ''kernel32::CreateMutexW')) 'NSIS mutex creation must immediately consume the raw GA descriptor'
Assert-Condition ([regex]::IsMatch($acquireLeaseMacro.Value, '(?m)^\s*!insertmacro UIPILOT_SECURITY_ATTRIBUTES "\$\{UIPILOT_NAMED_OBJECT_SDDL\}"\r?\n\s*System::Call ''kernel32::SetLastError\(i 0\)''\r?\n\s*System::Call ''kernel32::CreateEventW')) 'NSIS event creation must immediately consume the raw GA descriptor after clearing LastError'
$createMutexBlock = $acquireLeaseMacro.Value.Substring(0, $acquireLeaseMacro.Value.IndexOf('WaitForSingleObject', [StringComparison]::Ordinal))
Assert-Condition $createMutexBlock.Contains('${If} $UiPilotMutexHandle = 0') 'NSIS mutex creation must reject a null handle'
Assert-Condition $createMutexBlock.Contains('${If} $R6 = 183') 'NSIS mutex creation must validate an existing named mutex'
Assert-Condition (-not $createMutexBlock.Contains('${ElseIf} $R6 != 0')) 'NSIS mutex creation must not reject a valid new handle based on an unspecified success LastError'
foreach ($atomicLastErrorPattern in @(
    '(?m)System::Call ''advapi32::GetTokenInformation\(p R5, i 1, p 0, i 0, \*i \.R7\) i \.R6 \?e''\r?\n\s+Pop \$R9',
    '(?m)System::Call ''kernel32::CreateMutexW\(p R9, i 0, w "\$\{UIPILOT_MUTEX_NAME\}"\) p \.R5 \?e''\r?\n\s+Pop \$R6',
    '(?m)System::Call ''kernel32::OpenEventW\(i 1179648, i 0, w "\$\{UIPILOT_ACTIVITY_NAME\}"\) p \.R5 \?e''\r?\n\s+Pop \$R6',
    '(?m)System::Call ''kernel32::SetLastError\(i 0\)''\r?\n\s+System::Call ''kernel32::CreateEventW\(p R9, i 1, i 0, w "\$\{UIPILOT_ACTIVITY_NAME\}"\) p \.R5 \?e''\r?\n\s+Pop \$R6'
)) {
    Assert-Condition ([regex]::IsMatch($hooksSource, $atomicLastErrorPattern)) 'NSIS must capture each Win32 LastError atomically before the System plug-in or NSIS instructions can overwrite it'
}
Assert-Condition (-not $hooksSource.Contains('kernel32::GetLastError()')) 'NSIS must not retrieve a target call LastError through a later System plug-in call'
$systemCalls = (($hooksSource -split '\r?\n') | Where-Object { $_.Contains('System::Call') }) -join "`n"
foreach ($invalidSystemToken in @('.UiPilot', 'p UiPilot', 'w UiPilot', '*l .UiPilot', '*w .R9')) {
    Assert-Condition (-not $systemCalls.Contains($invalidSystemToken)) "NSIS System call contains an invalid source/destination token: $invalidSystemToken"
}

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
$pageLeaveReinstall = [regex]::Match($customText, '(?ms)^Function PageLeaveReinstall\r?\n.*?^FunctionEnd\r?\n')
Assert-Condition $pageLeaveReinstall.Success 'Could not locate the custom reinstall leave function'
$reinstallDecision = $pageLeaveReinstall.Value.Substring(0, $pageLeaveReinstall.Value.IndexOf('reinst_uninstall:', [StringComparison]::Ordinal))
Assert-Condition (-not $reinstallDecision.Contains('$WixMode')) 'WiX upgrade/downgrade must not select the old uninstaller before the explicit same-version uninstall branch'
$updateInPlace = [regex]::Match($reinstallDecision, '(?ms)\$\{If\} \$UpdateMode = 1\s+Goto reinst_done\s+\$\{EndIf\}')
$sameVersionChoice = [regex]::Match($reinstallDecision, '(?ms)\$\{If\} \$R0 = 0[^\r\n]*\s+\$\{If\} \$R1 = 1[^\r\n]*\s+Goto reinst_done\s+\$\{Else\}[^\r\n]*\s+Goto reinst_uninstall\s+\$\{EndIf\}')
$upgradeInPlace = [regex]::Match($reinstallDecision, '(?ms)\$\{ElseIf\} \$R0 = 1[^\r\n]*\s+Goto reinst_done')
$downgradeInPlace = [regex]::Match($reinstallDecision, '(?ms)\$\{ElseIf\} \$R0 = -1[^\r\n]*\s+Goto reinst_done')
Assert-Condition ($updateInPlace.Success -and $sameVersionChoice.Success -and $upgradeInPlace.Success -and $downgradeInPlace.Success) 'Update, repair, upgrade, and downgrade must each take the approved explicit branch'
$explicitUninstall = $pageLeaveReinstall.Value.Substring($pageLeaveReinstall.Value.IndexOf('reinst_uninstall:', [StringComparison]::Ordinal))
$wixUninstall = [regex]::Match($explicitUninstall, '(?ms)\$\{If\} \$WixMode = 1\s+ReadRegStr \$R1 HKLM "\$R6" "UninstallString"\s+ExecWait ''\$R1'' \$0\s+\$\{Else\}')
$nsisUninstall = [regex]::Match($explicitUninstall, '(?ms)\$\{Else\}\s+ReadRegStr \$4 SHCTX "\$\{MANUPRODUCTKEY\}" ""\s+ReadRegStr \$R1 SHCTX "\$\{UNINSTKEY\}" "UninstallString".*?ExecWait ''\$R1'' \$0\s+\$\{EndIf\}')
Assert-Condition ($wixUninstall.Success -and $nsisUninstall.Success) 'Explicit same-version uninstall must execute the registered old uninstaller'
Assert-Condition (-not $explicitUninstall.Contains('/UPDATE')) 'Explicit same-version uninstall must not invoke the old uninstaller in update mode'
$lastExecWait = $explicitUninstall.LastIndexOf('ExecWait ''$R1'' $0', [StringComparison]::Ordinal)
$failureCheck = $explicitUninstall.IndexOf('${If} $0 <> 0', [StringComparison]::Ordinal)
$quit = $explicitUninstall.IndexOf('Quit', [StringComparison]::Ordinal)
$continue = $explicitUninstall.IndexOf('reinst_done:', [StringComparison]::Ordinal)
Assert-Condition ($lastExecWait -ge 0 -and $lastExecWait -lt $failureCheck -and $failureCheck -lt $quit -and $quit -lt $continue) 'Explicit uninstall must check failure before quitting and never continue into installation'
Assert-OrderedTokens $explicitUninstall @(
    '${If} $0 <> 0',
    '${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"',
    '${If} $WixMode = 1',
    '${AndIf} $0 = 1602',
    'Abort',
    '${EndIf}',
    '${If} $0 = 1',
    'Abort',
    '${EndIf}',
    'MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"',
    'Abort',
    '${EndIf}',
    'Quit',
    'reinst_done:'
) 'Explicit uninstall must abort every failed/cancelled old-uninstaller outcome and quit only after success'
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
Add-ManifestReader
$productionManifest = [UiPilot.ManagedEverythingInstaller.ManifestReader]::Read($productionHelper)
Assert-Condition ($productionManifest -match 'requestedExecutionLevel') 'Production Helper manifest is missing requestedExecutionLevel'
Assert-Condition ($productionManifest -match 'level\s*=\s*["'']asInvoker["'']') 'Production Helper must be asInvoker'
Assert-Condition ($productionManifest -match 'uiAccess\s*=\s*["'']false["'']') 'Production Helper must disable uiAccess'
$productionStrings = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($productionHelper))
Assert-Condition (-not $productionStrings.Contains('fault-injection:')) 'Production Helper contains VM fault-injection code'
$installedHelperCopies = @(Get-ChildItem (Join-Path $root 'src-tauri\target\release\resources') -Recurse -Filter 'uipilot-everything-installer.exe' -File -ErrorAction SilentlyContinue)
Assert-Condition ($installedHelperCopies.Count -eq 0) 'Helper must not be copied into installed runtime resources'

Write-Output 'MANAGED_EVERYTHING_INSTALLER_SOURCE_PASS'
