[CmdletBinding()]
param(
    [ValidateSet('Source', 'SelfTest', 'VmInstalledRuntime', 'VmRecovery', 'VmRetryBudget', 'VmShutdown', 'VmMultiSession', 'VmPipeSecurity', 'VmCoexistence')]
    [string]$Mode = 'Source',

    [string]$DisposableVmMarker,
    [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')]
    [string]$EvidenceName,
    [uint32]$UiPilotProcessId,
    [uint32]$ClientProcessId,
    [uint32]$OtherSessionClientProcessId,
    [uint32]$DefaultEverythingProcessId,
    [ValidateSet('Client', 'Service')]
    [string]$RecoveryTarget = 'Client',
    [ValidateSet('Observe', 'ForceHost')]
    [string]$ShutdownAction = 'Observe',
    [ValidateSet('AuthorizedInteractive', 'AuthorizedBatch', 'AuthorizedNetwork', 'LocalSystem', 'ElevatedAdmin', 'OtherInteractive', 'OtherBatch', 'NetworkService', 'FilteredAdmin')]
    [string]$TokenCase,
    [ValidateSet('Current', 'Linked', 'CredentialInteractive', 'CredentialBatch', 'CredentialNetwork', 'Process')]
    [string]$TokenSource = 'Current',
    [Management.Automation.PSCredential]$Credential,
    [uint32]$TokenProcessId,
    [ValidateRange(1, 300)]
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script:RepositoryRoot = Split-Path -Parent $PSScriptRoot
$script:ArtifactRoot = Join-Path $script:RepositoryRoot 'artifacts\managed-everything'
$script:DisposableMarkerText = 'UIPILOT_DISPOSABLE_VM=I_UNDERSTAND_THIS_MACHINE_WILL_BE_MUTATED'
$script:ManagedObjectSddl = 'O:BAG:BAD:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)'
$script:ExpectedManifestKeys = @(
    'architecture', 'desktop_user_sid', 'expanded_sddl', 'instance', 'pipe_name',
    'publisher', 'schema_version', 'service_binary_path', 'service_name', 'sha256', 'version'
)

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-SourceContains([string]$Path, [string[]]$Patterns) {
    Assert-Condition (Test-Path -LiteralPath $Path -PathType Leaf) 'A required runtime source file is missing'
    $source = Get-Content -LiteralPath $Path -Raw -Encoding utf8
    foreach ($pattern in $Patterns) {
        Assert-Condition $source.Contains($pattern) "Runtime source contract is missing: $pattern"
    }
}

function Assert-ExactKeys($Value, [string[]]$Expected) {
    $actual = @($Value.PSObject.Properties.Name | Sort-Object)
    $wanted = @($Expected | Sort-Object)
    Assert-Condition (($actual -join "`n") -ceq ($wanted -join "`n")) 'Runtime manifest schema mismatch'
}

function Assert-RedactedJson([string]$Json) {
    foreach ($value in @($env:COMPUTERNAME, $env:USERNAME, $env:USERPROFILE, $script:RepositoryRoot)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            Assert-Condition (-not $Json.Contains($value)) 'Evidence contains machine, user, or path data'
        }
    }
    Assert-Condition ($Json -notmatch '(?i)S-1-\d+(?:-\d+){1,14}') 'Evidence contains SID data'
    Assert-Condition ($Json -notmatch '(?i)[A-Z]:[\\/]') 'Evidence contains absolute path data'
    Assert-Condition ($Json -notmatch '(?i)\\\\[^\\\s]+\\') 'Evidence contains UNC path data'
    Assert-Condition ($Json -notmatch '(?i)"(?:query|search_text|result_path)"\s*:') 'Evidence contains search or result data'
}

function Write-ManagedEvidence([string]$Name, $Evidence) {
    if ([string]::IsNullOrWhiteSpace($Name)) { throw 'EvidenceName is required for VM modes' }
    $json = $Evidence | ConvertTo-Json -Depth 8
    Assert-RedactedJson $json
    [IO.Directory]::CreateDirectory($script:ArtifactRoot) | Out-Null
    $destination = Join-Path $script:ArtifactRoot ("runtime-{0}.json" -f $Name)
    [IO.File]::WriteAllText($destination, $json, [Text.UTF8Encoding]::new($false))
    Write-Output ("MANAGED_EVERYTHING_RUNTIME_EVIDENCE {0}" -f (Split-Path -Leaf $destination))
}

function Resolve-StrictLeaf([string]$Path, [string]$Description) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    Assert-Condition (-not $item.PSIsContainer) "$Description must be a file"
    Assert-Condition (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) "$Description must not be a reparse point"
    return $item.FullName
}

function Assert-DisposableVmAdmission {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    Assert-Condition $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) 'VM mode requires an explicitly elevated administrator shell'
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($DisposableVmMarker)) 'VM mode requires DisposableVmMarker'
    $marker = Resolve-StrictLeaf $DisposableVmMarker 'Disposable VM marker'
    $content = [IO.File]::ReadAllText($marker).Trim()
    Assert-Condition ($content -ceq $script:DisposableMarkerText) 'Disposable VM marker acknowledgement is invalid'
}

function Get-ManagedRuntimeState {
    $programData = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)
    $managedRoot = Join-Path $programData 'UiPilot_tools\everything'
    $rootItem = Get-Item -LiteralPath $managedRoot -Force -ErrorAction Stop
    Assert-Condition $rootItem.PSIsContainer 'Managed runtime root is not a directory'
    Assert-Condition (($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Managed runtime root is a reparse point'
    $manifestPath = Resolve-StrictLeaf (Join-Path $rootItem.FullName 'managed-runtime.json') 'Managed runtime manifest'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    Assert-ExactKeys $manifest $script:ExpectedManifestKeys
    Assert-Condition ($manifest.schema_version -eq 1) 'Managed runtime manifest version mismatch'
    Assert-Condition ($manifest.architecture -ceq 'x64') 'Managed runtime architecture mismatch'
    Assert-Condition ($manifest.instance -ceq 'UiPilot') 'Managed runtime instance mismatch'
    Assert-Condition ($manifest.service_name -ceq 'Everything (UiPilot)') 'Managed runtime Service mismatch'
    Assert-Condition ($manifest.pipe_name -ceq '\\.\PIPE\UiPilotEverythingService') 'Managed runtime pipe mismatch'
    Assert-Condition ([string]$manifest.desktop_user_sid -match '^S-1-(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))+$') 'Managed runtime SID is not canonical'
    $expectedPipeSddl = 'D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;{0})' -f $manifest.desktop_user_sid
    Assert-Condition ($manifest.expanded_sddl -ceq $expectedPipeSddl) 'Managed runtime pipe descriptor mismatch'
    $binary = Resolve-StrictLeaf ([string]$manifest.service_binary_path) 'Managed Service binary'
    $versionRoot = Split-Path -Parent $binary
    Assert-Condition ((Split-Path -Parent $versionRoot) -ceq $rootItem.FullName) 'Managed Service binary escaped the managed root'
    Assert-Condition ((Split-Path -Leaf $versionRoot) -ceq [string]$manifest.version) 'Managed Service version directory mismatch'
    Assert-Condition ((Split-Path -Leaf $binary) -ceq 'Everything.exe') 'Managed Service binary name mismatch'
    $hash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Condition ($hash -ceq [string]$manifest.sha256) 'Managed Service binary hash mismatch'
    Assert-Condition ((Get-Acl -LiteralPath $rootItem.FullName).Sddl -ceq $script:ManagedObjectSddl) 'Managed root ACL mismatch'
    Assert-Condition ((Get-Acl -LiteralPath $manifestPath).Sddl -ceq $script:ManagedObjectSddl) 'Managed manifest ACL mismatch'
    return [pscustomobject]@{
        Root = $rootItem.FullName
        ManifestPath = $manifestPath
        Manifest = $manifest
        Binary = $binary
        Hash = $hash
    }
}

function Get-ProcessRecord([uint32]$ProcessId) {
    $record = Get-CimInstance Win32_Process -Filter ("ProcessId={0}" -f $ProcessId) -ErrorAction Stop
    Assert-Condition ($null -ne $record) 'Expected process is not running'
    return $record
}

function Add-RuntimeNativeProbe {
    if ('UiPilot.ManagedEverything.NativeProbe' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;

namespace UiPilot.ManagedEverything {
  public sealed class PipeProbeResult {
    public bool DescriptorMatches;
    public bool ReadAllowed;
    public bool WriteAllowed;
    public bool Connected;
    public bool CreatedInstance;
    public int ConnectError;
    public int CreateError;
    public string Sid;
    public bool Administrator;
  }

  public sealed class ProcessLease : IDisposable {
    const uint PROCESS_TERMINATE = 0x0001, PROCESS_QUERY_LIMITED_INFORMATION = 0x1000, SYNCHRONIZE = 0x00100000;
    const uint TOKEN_QUERY = 0x0008, WAIT_OBJECT_0 = 0, WAIT_TIMEOUT = 258;
    IntPtr handle;

    [StructLayout(LayoutKind.Sequential)] struct FILETIME { public uint LowDateTime; public uint HighDateTime; }
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder path, ref uint length);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out FILETIME creation, out FILETIME exit, out FILETIME kernel, out FILETIME user);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid, out uint session);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);

    internal ProcessLease(uint pid, bool allowTerminate) {
      uint access = PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | (allowTerminate ? PROCESS_TERMINATE : 0);
      handle = OpenProcess(access, false, pid);
      if (handle == IntPtr.Zero) throw new Win32Exception();
      try {
        ProcessId = pid;
        var path = new StringBuilder(32768);
        uint length = (uint)path.Capacity;
        if (!QueryFullProcessImageName(handle, 0, path, ref length)) throw new Win32Exception();
        ImagePath = path.ToString();
        FILETIME creation, exit, kernel, user;
        if (!GetProcessTimes(handle, out creation, out exit, out kernel, out user)) throw new Win32Exception();
        CreationTime = ((ulong)creation.HighDateTime << 32) | creation.LowDateTime;
        uint session;
        if (!ProcessIdToSessionId(pid, out session)) throw new Win32Exception();
        SessionId = session;
        IntPtr token;
        if (!OpenProcessToken(handle, TOKEN_QUERY, out token)) throw new Win32Exception();
        try { UserSid = new WindowsIdentity(token).User.Value; }
        finally { CloseHandle(token); }
        CanTerminate = allowTerminate;
        if (!IsAlive) throw new InvalidOperationException("Process exited while its identity was acquired");
      } catch {
        Dispose();
        throw;
      }
    }

    public uint ProcessId { get; private set; }
    public string ImagePath { get; private set; }
    public ulong CreationTime { get; private set; }
    public uint SessionId { get; private set; }
    public string UserSid { get; private set; }
    public bool CanTerminate { get; private set; }
    public bool IsAlive {
      get {
        if (handle == IntPtr.Zero) return false;
        uint wait = WaitForSingleObject(handle, 0);
        if (wait == WAIT_TIMEOUT) return true;
        if (wait == WAIT_OBJECT_0) return false;
        throw new Win32Exception();
      }
    }

    public bool IsInAnyJob() {
      if (!IsAlive) return false;
      bool result;
      if (!IsProcessInJob(handle, IntPtr.Zero, out result)) throw new Win32Exception();
      return result;
    }

    public void TerminateAndWait(uint exitCode, uint timeoutMilliseconds) {
      if (!CanTerminate) throw new InvalidOperationException("Process lease is read-only");
      if (!IsAlive) throw new InvalidOperationException("Process identity exited before termination");
      if (!TerminateProcess(handle, exitCode)) throw new Win32Exception();
      if (WaitForSingleObject(handle, timeoutMilliseconds) != WAIT_OBJECT_0) throw new InvalidOperationException("Verified process did not terminate");
    }

    public void Dispose() {
      if (handle != IntPtr.Zero) { CloseHandle(handle); handle = IntPtr.Zero; }
    }
  }

  public static class NativeProbe {
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint TOKEN_QUERY = 0x0008, TOKEN_DUPLICATE = 0x0002, TOKEN_IMPERSONATE = 0x0004;
    const uint MAXIMUM_ALLOWED = 0x02000000, READ_CONTROL = 0x00020000;
    const uint GENERIC_READ = 0x80000000, GENERIC_WRITE = 0x40000000;
    const uint FILE_SHARE_READ = 1, FILE_SHARE_WRITE = 2, OPEN_EXISTING = 3;
    const uint PIPE_ACCESS_DUPLEX = 3, PIPE_TYPE_BYTE = 0, PIPE_WAIT = 0;
    const int TokenImpersonation = 2, SecurityImpersonation = 2, TokenLinkedToken = 19;
    const uint DACL_SECURITY_INFORMATION = 4, SE_KERNEL_OBJECT = 6;
    static readonly IntPtr INVALID_HANDLE_VALUE = new IntPtr(-1);

    [StructLayout(LayoutKind.Sequential)] struct GENERIC_MAPPING { public uint Read, Write, Execute, All; }
    [StructLayout(LayoutKind.Sequential)] struct LUID { public uint LowPart; public int HighPart; }
    [StructLayout(LayoutKind.Sequential)] struct LUID_AND_ATTRIBUTES { public LUID Luid; public uint Attributes; }
    [StructLayout(LayoutKind.Sequential)] struct PRIVILEGE_SET { public uint PrivilegeCount, Control; public LUID_AND_ATTRIBUTES Privilege; }

    delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr state);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool DuplicateTokenEx(IntPtr token, uint access, IntPtr attributes, int level, int type, out IntPtr duplicate);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool LogonUser(string user, string domain, string password, int type, int provider, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int infoClass, out IntPtr linkedToken, int length, out int returned);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool ImpersonateLoggedOnUser(IntPtr token);
    [DllImport("advapi32.dll")] static extern bool RevertToSelf();
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern IntPtr CreateNamedPipe(string name, uint openMode, uint pipeMode, uint instances, uint outSize, uint inSize, uint timeout, IntPtr security);
    [DllImport("advapi32.dll", SetLastError=true)] static extern uint GetSecurityInfo(IntPtr handle, uint type, uint info, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl, out IntPtr descriptor);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool ConvertSecurityDescriptorToStringSecurityDescriptor(IntPtr descriptor, uint revision, uint info, out IntPtr text, out uint length);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool AccessCheck(IntPtr descriptor, IntPtr token, uint desired, ref GENERIC_MAPPING mapping, IntPtr privileges, ref uint privilegeLength, out uint granted, out bool allowed);
    [DllImport("advapi32.dll")] static extern void MapGenericMask(ref uint mask, ref GENERIC_MAPPING mapping);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CommandLineToArgvW(string commandLine, out int argumentCount);

    public static string[] SplitCommandLine(string commandLine) {
      int count;
      IntPtr arguments = CommandLineToArgvW(commandLine, out count);
      if (arguments == IntPtr.Zero) throw new Win32Exception();
      try {
        string[] result = new string[count];
        for (int index = 0; index < count; index++) {
          IntPtr value = Marshal.ReadIntPtr(arguments, index * IntPtr.Size);
          result[index] = Marshal.PtrToStringUni(value);
        }
        return result;
      } finally { LocalFree(arguments); }
    }

    public static ProcessLease OpenProcessLease(uint pid, bool allowTerminate) {
      return new ProcessLease(pid, allowTerminate);
    }

    public static bool HasWindow(uint pid, string className) {
      bool found = false;
      EnumWindows((hwnd, state) => {
        uint owner; GetWindowThreadProcessId(hwnd, out owner);
        if (owner == pid) { var b = new StringBuilder(256); GetClassName(hwnd, b, b.Capacity); if (b.ToString() == className) found = true; }
        return !found;
      }, IntPtr.Zero);
      return found;
    }

    public static long GetExactWindow(uint pid, string className) {
      IntPtr found = IntPtr.Zero;
      int classCount = 0;
      EnumWindows((hwnd, state) => {
        var name = new StringBuilder(256);
        if (GetClassName(hwnd, name, name.Capacity) != 0 && name.ToString() == className) {
          classCount++;
          uint owner;
          if (GetWindowThreadProcessId(hwnd, out owner) != 0 && owner == pid) found = hwnd;
        }
        return true;
      }, IntPtr.Zero);
      return classCount == 1 ? found.ToInt64() : 0;
    }

    public static bool WindowBelongsToProcess(long window, uint pid) {
      IntPtr hwnd = new IntPtr(window);
      if (window == 0 || !IsWindow(hwnd)) return false;
      uint owner;
      return GetWindowThreadProcessId(hwnd, out owner) != 0 && owner == pid;
    }

    public static bool DatabaseLoaded(long window, uint timeoutMilliseconds) {
      IntPtr hwnd = new IntPtr(window);
      if (window == 0 || !IsWindow(hwnd)) return false;
      IntPtr result;
      IntPtr sent = SendMessageTimeout(hwnd, 0x0400, new IntPtr(401), IntPtr.Zero, 0x0002 | 0x0020, timeoutMilliseconds, out result);
      return sent != IntPtr.Zero && result != IntPtr.Zero;
    }

    public static bool IsInJob(uint pid) {
      IntPtr process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
      if (process == IntPtr.Zero) throw new Win32Exception();
      try { bool result; if (!IsProcessInJob(process, IntPtr.Zero, out result)) throw new Win32Exception(); return result; }
      finally { CloseHandle(process); }
    }

    static IntPtr DuplicateImpersonation(IntPtr token) {
      IntPtr duplicate;
      if (!DuplicateTokenEx(token, MAXIMUM_ALLOWED, IntPtr.Zero, SecurityImpersonation, TokenImpersonation, out duplicate)) throw new Win32Exception();
      return duplicate;
    }

    static IntPtr AcquireToken(string source, uint pid, string user, string domain, string password) {
      IntPtr original = IntPtr.Zero;
      if (source == "Current") {
        if (!OpenProcessToken(System.Diagnostics.Process.GetCurrentProcess().Handle, TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, out original)) throw new Win32Exception();
      } else if (source == "Linked") {
        IntPtr current;
        if (!OpenProcessToken(System.Diagnostics.Process.GetCurrentProcess().Handle, TOKEN_QUERY, out current)) throw new Win32Exception();
        try { int returned; if (!GetTokenInformation(current, TokenLinkedToken, out original, IntPtr.Size, out returned)) throw new Win32Exception(); }
        finally { CloseHandle(current); }
      } else if (source == "Process") {
        IntPtr process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (process == IntPtr.Zero) throw new Win32Exception();
        try { if (!OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, out original)) throw new Win32Exception(); }
        finally { CloseHandle(process); }
      } else {
        int logonType = source == "CredentialInteractive" ? 2 : source == "CredentialBatch" ? 4 : 3;
        if (!LogonUser(user, domain, password, logonType, 0, out original)) throw new Win32Exception();
      }
      try { return DuplicateImpersonation(original); }
      finally { CloseHandle(original); }
    }

    static bool Check(IntPtr descriptor, IntPtr token, uint desired) {
      var mapping = new GENERIC_MAPPING { Read = 0x00120089, Write = 0x00120116, Execute = 0x001200A0, All = 0x001F01FF };
      MapGenericMask(ref desired, ref mapping);
      uint length = 1024, granted; bool allowed;
      IntPtr privileges = Marshal.AllocHGlobal((int)length);
      try { if (!AccessCheck(descriptor, token, desired, ref mapping, privileges, ref length, out granted, out allowed)) throw new Win32Exception(); return allowed; }
      finally { Marshal.FreeHGlobal(privileges); }
    }

    public static bool DaclEquivalent(string actualSddl, string expectedSddl) {
      var actual = new RawSecurityDescriptor(actualSddl);
      var expected = new RawSecurityDescriptor(expectedSddl);
      const ControlFlags relevant = ControlFlags.DiscretionaryAclPresent | ControlFlags.DiscretionaryAclProtected;
      if ((actual.ControlFlags & relevant) != (expected.ControlFlags & relevant)) return false;
      if (actual.DiscretionaryAcl == null || expected.DiscretionaryAcl == null) return actual.DiscretionaryAcl == expected.DiscretionaryAcl;
      byte[] actualBytes = new byte[actual.DiscretionaryAcl.BinaryLength];
      byte[] expectedBytes = new byte[expected.DiscretionaryAcl.BinaryLength];
      actual.DiscretionaryAcl.GetBinaryForm(actualBytes, 0);
      expected.DiscretionaryAcl.GetBinaryForm(expectedBytes, 0);
      if (actualBytes.Length != expectedBytes.Length) return false;
      for (int index = 0; index < actualBytes.Length; index++) if (actualBytes[index] != expectedBytes[index]) return false;
      return true;
    }

    public static PipeProbeResult ProbePipe(string name, string expectedSddl, string source, uint pid, string user, string domain, string password) {
      IntPtr pipe = CreateFile(name, READ_CONTROL, FILE_SHARE_READ | FILE_SHARE_WRITE, IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
      if (pipe == INVALID_HANDLE_VALUE) throw new Win32Exception();
      IntPtr descriptor = IntPtr.Zero, text = IntPtr.Zero, token = IntPtr.Zero;
      try {
        uint status = GetSecurityInfo(pipe, SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, out descriptor);
        if (status != 0) throw new Win32Exception((int)status);
        uint chars; if (!ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor, 1, DACL_SECURITY_INFORMATION, out text, out chars)) throw new Win32Exception();
        string actual = Marshal.PtrToStringUni(text);
        token = AcquireToken(source, pid, user, domain, password);
        var identity = new WindowsIdentity(token);
        var result = new PipeProbeResult {
          DescriptorMatches = DaclEquivalent(actual, expectedSddl),
          ReadAllowed = Check(descriptor, token, GENERIC_READ),
          WriteAllowed = Check(descriptor, token, GENERIC_WRITE),
          Sid = identity.User.Value,
          Administrator = new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator)
        };
        if (!ImpersonateLoggedOnUser(token)) throw new Win32Exception();
        try {
          IntPtr connected = CreateFile(name, GENERIC_READ | GENERIC_WRITE, 0, IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
          result.Connected = connected != INVALID_HANDLE_VALUE;
          result.ConnectError = result.Connected ? 0 : Marshal.GetLastWin32Error();
          if (result.Connected) CloseHandle(connected);
          IntPtr created = CreateNamedPipe(name, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, IntPtr.Zero);
          result.CreatedInstance = created != INVALID_HANDLE_VALUE;
          result.CreateError = result.CreatedInstance ? 0 : Marshal.GetLastWin32Error();
          if (result.CreatedInstance) CloseHandle(created);
        } finally { RevertToSelf(); }
        return result;
      } finally {
        if (token != IntPtr.Zero) CloseHandle(token);
        if (text != IntPtr.Zero) LocalFree(text);
        if (descriptor != IntPtr.Zero) LocalFree(descriptor);
        CloseHandle(pipe);
      }
    }
  }
}
'@
}

function Get-CommandLineTokens([string]$CommandLine) {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($CommandLine)) 'Expected process command line is unavailable'
    return @([UiPilot.ManagedEverything.NativeProbe]::SplitCommandLine($CommandLine))
}

function Assert-ExactToken([string]$Actual, [string]$Expected, [string]$Message, [bool]$Path = $false) {
    $comparison = if ($Path) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    Assert-Condition ([string]::Equals($Actual, $Expected, $comparison)) $Message
}

function Get-ReviewedEverythingHash {
    $lockPath = Resolve-StrictLeaf (Join-Path $script:RepositoryRoot 'src-tauri\resources\everything\everything.lock.json') 'Reviewed Everything lock'
    $lock = Get-Content -LiteralPath $lockPath -Raw -Encoding utf8 | ConvertFrom-Json
    $hash = [string]$lock.everythingExeSha256
    Assert-Condition ($hash -match '^[0-9a-f]{64}$') 'Reviewed Everything hash is invalid'
    return $hash
}

function Get-PrivateClientContext {
    param(
        [uint32]$ProcessId,
        $Managed,
        $ExpectedHostLease,
        [bool]$AllowClientTerminate = $false,
        [bool]$AllowHostTerminate = $false,
        [bool]$RequireWindow = $true
    )
    Add-RuntimeNativeProbe
    $lease = $null
    $hostLease = $null
    $ownsHostLease = $false
    try {
        $lease = [UiPilot.ManagedEverything.NativeProbe]::OpenProcessLease($ProcessId, $AllowClientTerminate)
        $record = Get-ProcessRecord $ProcessId
        Assert-Condition ([uint32]$record.ProcessId -eq $lease.ProcessId) 'Client PID changed after its process handle was opened'
        Assert-Condition ([string]$record.Name -ceq 'Everything.exe') 'Private client image name mismatch'
        Assert-Condition ((Split-Path -Leaf $lease.ImagePath) -ceq 'Everything.exe') 'Private client native image mismatch'
        Assert-Condition ($lease.UserSid -ceq [string]$Managed.Manifest.desktop_user_sid) 'Private client user SID does not match the managed desktop user'

        $everythingResourceRoot = Split-Path -Parent $lease.ImagePath
        $resourcesRoot = Split-Path -Parent $everythingResourceRoot
        $applicationRoot = Split-Path -Parent $resourcesRoot
        Assert-Condition ((Split-Path -Leaf $everythingResourceRoot) -ceq 'everything') 'Private client is outside the Everything resource directory'
        Assert-Condition ((Split-Path -Leaf $resourcesRoot) -ceq 'resources') 'Private client is outside the application resources directory'
        $expectedHostPath = Join-Path $applicationRoot 'UiPilot_tools.exe'
        $clientHash = (Get-FileHash -LiteralPath $lease.ImagePath -Algorithm SHA256).Hash.ToLowerInvariant()
        Assert-Condition ($clientHash -ceq (Get-ReviewedEverythingHash)) 'Private client hash does not match the reviewed package'

        $tokens = Get-CommandLineTokens ([string]$record.CommandLine)
        Assert-Condition ($tokens.Count -eq 10) 'Private client command line has an unexpected token count'
        Assert-ExactToken $tokens[0] $lease.ImagePath 'Private client command image mismatch' $true
        Assert-ExactToken $tokens[1] '-instance' 'Private client instance option mismatch'
        Assert-ExactToken $tokens[2] 'UiPilot' 'Private client instance value mismatch'
        Assert-ExactToken $tokens[3] '-service-pipe-name' 'Private client pipe option mismatch'
        Assert-ExactToken $tokens[4] '\\.\PIPE\UiPilotEverythingService' 'Private client pipe value mismatch'
        Assert-ExactToken $tokens[5] '-config' 'Private client config option mismatch'
        Assert-ExactToken $tokens[7] '-db' 'Private client database option mismatch'
        Assert-ExactToken $tokens[9] '-startup' 'Private client startup option mismatch'

        $ini = [IO.Path]::GetFullPath($tokens[6])
        $database = [IO.Path]::GetFullPath($tokens[8])
        $sessionRoot = Split-Path -Parent $ini
        Assert-ExactToken (Split-Path -Parent $database) $sessionRoot 'Private client INI and DB directories differ' $true
        Assert-ExactToken (Split-Path -Leaf $ini) 'Everything-UiPilot.ini' 'Private client INI name mismatch'
        Assert-ExactToken (Split-Path -Leaf $database) 'Everything-UiPilot.db' 'Private client DB name mismatch'
        Assert-ExactToken (Split-Path -Leaf $sessionRoot) ([string]$lease.SessionId) 'Private client storage session mismatch'
        Assert-ExactToken (Split-Path -Leaf (Split-Path -Parent $sessionRoot)) 'everything' 'Private client storage root mismatch'
        Assert-ExactToken (Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $sessionRoot))) 'UiPilot_tools' 'Private client application storage root mismatch'
        Resolve-StrictLeaf $ini 'Private client INI' | Out-Null
        Resolve-StrictLeaf $database 'Private client DB' | Out-Null
        $storageItem = Get-Item -LiteralPath $sessionRoot -Force -ErrorAction Stop
        Assert-Condition (($storageItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Private client storage directory is a reparse point'

        $parentPid = [uint32]$record.ParentProcessId
        Assert-Condition ($parentPid -ne 0) 'Private client has no host parent'
        if ($null -eq $ExpectedHostLease) {
            $hostLease = [UiPilot.ManagedEverything.NativeProbe]::OpenProcessLease($parentPid, $AllowHostTerminate)
            $ownsHostLease = $true
        } else {
            $hostLease = $ExpectedHostLease
            Assert-Condition $hostLease.IsAlive 'Expected UiPilot host exited during client validation'
            Assert-Condition ($hostLease.ProcessId -eq $parentPid) 'Private client parent PID is not the verified UiPilot host'
        }
        Assert-ExactToken $hostLease.ImagePath $expectedHostPath 'Private client parent image is not the matching UiPilot host' $true
        Assert-Condition ((Split-Path -Leaf $hostLease.ImagePath) -ceq 'UiPilot_tools.exe') 'UiPilot host image name mismatch'
        Assert-Condition ($hostLease.SessionId -eq $lease.SessionId) 'UiPilot host and private client session mismatch'
        Assert-Condition ($hostLease.UserSid -ceq $lease.UserSid) 'UiPilot host and private client user mismatch'
        $window = [UiPilot.ManagedEverything.NativeProbe]::GetExactWindow($lease.ProcessId, 'EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)')
        if ($RequireWindow) {
            Assert-Condition ($window -ne 0) 'Private client does not own the unique private window'
            Assert-Condition ([UiPilot.ManagedEverything.NativeProbe]::WindowBelongsToProcess($window, $lease.ProcessId)) 'Private client window ownership changed'
        }
        Assert-Condition $lease.IsAlive 'Private client exited during identity validation'

        return [pscustomobject]@{
            Lease = $lease
            HostLease = $hostLease
            OwnsHostLease = $ownsHostLease
            Record = $record
            Window = [long]$window
            Ini = $ini
            Database = $database
            ClientHash = $clientHash
        }
    }
    catch {
        if ($null -ne $lease) { $lease.Dispose() }
        if ($ownsHostLease -and $null -ne $hostLease) { $hostLease.Dispose() }
        throw
    }
}

function Close-PrivateClientContext($Context) {
    if ($null -eq $Context) { return }
    if ($null -ne $Context.Lease) { $Context.Lease.Dispose() }
    if ($Context.OwnsHostLease -and $null -ne $Context.HostLease) { $Context.HostLease.Dispose() }
}

function Get-ServiceProcessContext($Managed, [bool]$AllowTerminate) {
    $lease = $null
    try {
        $service = Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction Stop
        Assert-Condition ($null -ne $service) 'Private Service is missing'
        Assert-Condition ([string]$service.Name -ceq 'Everything (UiPilot)') 'Private Service name mismatch'
        Assert-Condition ([string]$service.State -ceq 'Running') 'Private Service is not running'
        Assert-Condition ([uint32]$service.ProcessId -ne 0) 'Private Service has no process identity'
        $tokens = Get-CommandLineTokens ([string]$service.PathName)
        Assert-Condition ($tokens.Count -eq 8) 'Private Service command line has an unexpected token count'
        Assert-ExactToken $tokens[0] ([string]$Managed.Binary) 'Private Service command image mismatch' $true
        Assert-ExactToken $tokens[1] '-svc' 'Private Service mode mismatch'
        Assert-ExactToken $tokens[2] '-instance' 'Private Service instance option mismatch'
        Assert-ExactToken $tokens[3] 'UiPilot' 'Private Service instance value mismatch'
        Assert-ExactToken $tokens[4] '-service-pipe-name' 'Private Service pipe option mismatch'
        Assert-ExactToken $tokens[5] '\\.\PIPE\UiPilotEverythingService' 'Private Service pipe value mismatch'
        Assert-ExactToken $tokens[6] '-service-security-descriptor' 'Private Service descriptor option mismatch'
        Assert-ExactToken $tokens[7] ([string]$Managed.Manifest.expanded_sddl) 'Private Service descriptor mismatch'
        $lease = [UiPilot.ManagedEverything.NativeProbe]::OpenProcessLease([uint32]$service.ProcessId, $AllowTerminate)
        Assert-ExactToken $lease.ImagePath ([string]$Managed.Binary) 'Private Service process image mismatch' $true
        Assert-Condition ($lease.SessionId -eq 0) 'Private Service process is not in session zero'
        Assert-Condition ($lease.UserSid -ceq 'S-1-5-18') 'Private Service process is not LocalSystem'
        Assert-Condition $lease.IsAlive 'Private Service process exited during identity validation'
        return [pscustomobject]@{ Service = $service; Lease = $lease }
    }
    catch {
        if ($null -ne $lease) { $lease.Dispose() }
        throw
    }
}

function Invoke-PrivateReadinessProbe($Context, [uint32]$OperationTimeoutMilliseconds = 0) {
    Assert-Condition $Context.Lease.IsAlive 'Private client exited before readiness probing'
    Assert-Condition ([UiPilot.ManagedEverything.NativeProbe]::WindowBelongsToProcess($Context.Window, $Context.Lease.ProcessId)) 'Private readiness window is no longer PID-bound'
    $timeoutMilliseconds = if ($OperationTimeoutMilliseconds -eq 0) {
        [uint32][Math]::Min(60000, [Math]::Max(1, $TimeoutSeconds * 1000))
    } else {
        [uint32][Math]::Min(60000, [Math]::Max(1, $OperationTimeoutMilliseconds))
    }
    $databaseLoaded = [UiPilot.ManagedEverything.NativeProbe]::DatabaseLoaded($Context.Window, $timeoutMilliseconds)
    $query2Metadata = $false
    if ($databaseLoaded) {
        $probePath = Resolve-StrictLeaf (Join-Path $script:RepositoryRoot 'spikes\everything-ipc\target\debug\everything-ipc-spike.exe') 'Everything IPC probe CLI'
        Assert-Condition ((Split-Path -Leaf $probePath) -ceq 'everything-ipc-spike.exe') 'Everything IPC probe CLI name mismatch'
        $rawOutput = ''
        try {
            $oldPreference = $ErrorActionPreference
            $ErrorActionPreference = 'Continue'
            $rawOutput = (@(& $probePath '--instance' 'UiPilot' '--query' '' '--limit' '1' '--timeout-ms' ([string]$timeoutMilliseconds) '--format' 'json' 2>$null) -join "`n")
            $exitCode = [int]$LASTEXITCODE
            $ErrorActionPreference = $oldPreference
            $query2Metadata = $exitCode -eq 0 -and (Test-Query2Metadata $rawOutput)
        }
        finally {
            $ErrorActionPreference = $oldPreference
            $rawOutput = $null
        }
    }
    $sameWindow = $Context.Lease.IsAlive -and
        [UiPilot.ManagedEverything.NativeProbe]::GetExactWindow($Context.Lease.ProcessId, 'EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)') -eq $Context.Window
    return [pscustomobject]@{
        DatabaseLoaded = $databaseLoaded
        Query2Metadata = $query2Metadata
        SamePidBoundWindow = $sameWindow
        Ready = $databaseLoaded -and $query2Metadata -and $sameWindow
    }
}

function Wait-ForReadyPrivateClient($Managed, $HostLease, [string[]]$ExcludedCreationIdentities, [bool]$AllowTerminate, [int]$ObservationTimeoutSeconds = 0) {
    $effectiveTimeout = if ($ObservationTimeoutSeconds -gt 0) { $ObservationTimeoutSeconds } else { $TimeoutSeconds }
    $deadline = [DateTime]::UtcNow.AddSeconds($effectiveTimeout)
    do {
        $readyContexts = @()
        $records = @(Get-CimInstance Win32_Process -Filter "Name='Everything.exe'" -ErrorAction SilentlyContinue)
        foreach ($record in $records) {
            if ([uint32]$record.ParentProcessId -ne $HostLease.ProcessId) { continue }
            $context = $null
            try {
                $context = Get-PrivateClientContext ([uint32]$record.ProcessId) $Managed $HostLease $AllowTerminate $false $true
                $identity = '{0}:{1}' -f $context.Lease.ProcessId, $context.Lease.CreationTime
                if ($ExcludedCreationIdentities -contains $identity) {
                    Close-PrivateClientContext $context
                    continue
                }
                $readiness = Invoke-PrivateReadinessProbe $context 1000
                if ($readiness.Ready) { $readyContexts += $context } else { Close-PrivateClientContext $context }
            }
            catch {
                if ($null -ne $context) { Close-PrivateClientContext $context }
            }
        }
        if ($readyContexts.Count -eq 1) { return $readyContexts[0] }
        foreach ($context in $readyContexts) { Close-PrivateClientContext $context }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline -and $HostLease.IsAlive)
    return $null
}

function Test-HasExactPrivateClient($Managed, $HostLease, [string[]]$ExcludedCreationIdentities) {
    $records = @(Get-CimInstance Win32_Process -Filter "Name='Everything.exe'" -ErrorAction SilentlyContinue)
    foreach ($record in $records) {
        if ([uint32]$record.ParentProcessId -ne $HostLease.ProcessId) { continue }
        $context = $null
        try {
            $context = Get-PrivateClientContext ([uint32]$record.ProcessId) $Managed $HostLease $false $false $false
            $identity = '{0}:{1}' -f $context.Lease.ProcessId, $context.Lease.CreationTime
            if (-not ($ExcludedCreationIdentities -contains $identity)) { return $true }
        }
        catch { }
        finally { if ($null -ne $context) { Close-PrivateClientContext $context } }
    }
    return $false
}

function Get-CredentialParts {
    if ($null -eq $Credential) { return @('', '', '') }
    $network = $Credential.GetNetworkCredential()
    return @($network.UserName, $network.Domain, $network.Password)
}

function Wait-Until([scriptblock]$Condition) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if (& $Condition) { return $true }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    return $false
}

function Test-Query2Metadata([string]$Json) {
    try {
        $result = $Json | ConvertFrom-Json -ErrorAction Stop
        return [uint32]$result.requestFlags -eq 0x155 -and [uint32]$result.sortType -eq 14
    }
    catch { return $false }
}

function Test-RetryBudgetSequence([bool[]]$ReplacementReady) {
    return $ReplacementReady.Count -eq 4 -and
        $ReplacementReady[0] -and $ReplacementReady[1] -and $ReplacementReady[2] -and
        (-not $ReplacementReady[3])
}

function Invoke-SourceGate {
    $runtime = Join-Path $script:RepositoryRoot 'src-tauri\src\file_search\everything_runtime.rs'
    $process = Join-Path $script:RepositoryRoot 'src-tauri\src\file_search\windows\everything_process.rs'
    $lifecycle = Join-Path $script:RepositoryRoot 'src-tauri\src\lifecycle.rs'
    Assert-SourceContains $runtime @(
        'EverythingRuntimeSupervisor', 'start_async', 'restart_backoffs: [Duration; 3]',
        'EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)', 'Everything-UiPilot.ini', 'Everything-UiPilot.db',
        'EverythingClient::connect_owned(INSTANCE_NAME, pid, timeout)', 'runtime.client.close();',
        'runtime.process.shutdown();'
    )
    Assert-SourceContains $process @(
        'PROC_THREAD_ATTRIBUTE_JOB_LIST', 'CREATE_SUSPENDED', 'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE',
        'ResumeThread', 'TerminateProcess'
    )
    Assert-SourceContains $lifecycle @('prepare_application_shutdown', 'runtime.shutdown();')
    $harness = Get-Content -LiteralPath $PSCommandPath -Raw -Encoding utf8
    foreach ($contract in @(
        "'VmRetryBudget'", 'OpenProcessLease', 'TerminateAndWait',
        'GetExactWindow', 'DatabaseLoaded', 'requestFlags -eq 0x155',
        'sortType -eq 14', 'Test-RetryBudgetSequence'
    )) {
        Assert-Condition $harness.Contains($contract) "Runtime VM harness contract is missing: $contract"
    }
    Assert-Condition ($harness -notmatch '(?m)^\s*Stop-Process\b') 'Runtime VM harness must terminate only through a verified open process handle'
    Assert-Condition ($harness -notmatch '(?m)\.Contains\(["'']UiPilot["'']\)') 'Runtime VM harness must not use fuzzy UiPilot command matching'
    Write-ManagedEvidence 'source' ([ordered]@{
        schema_version = 1
        kind = 'runtime-source'
        outcome = 'pass'
        read_only = $true
        identity_contract = $true
        launch_contract = $true
        ownership_contract = $true
        retry_budget_contract = 3
        shutdown_contract = $true
    })
    Write-Output 'MANAGED_EVERYTHING_RUNTIME_SOURCE_PASS'
}

function Invoke-SelfTest {
    Add-RuntimeNativeProbe
    Assert-Condition ([UiPilot.ManagedEverything.NativeProbe]::DaclEquivalent(
        'D:P(A;;GA;;;SY)',
        'D:P(A;;GA;;;S-1-5-18)'
    )) 'Pipe descriptor canonicalization rejected equivalent SDDL representations'
    Assert-Condition (-not [UiPilot.ManagedEverything.NativeProbe]::DaclEquivalent(
        'D:P(A;;GA;;;SY)',
        'D:P(A;;GR;;;SY)'
    )) 'Pipe descriptor canonicalization accepted a different DACL'
    $parsed = [UiPilot.ManagedEverything.NativeProbe]::SplitCommandLine(
        '"C:\Program Files\UiPilot_tools\Everything.exe" -instance UiPilot -startup'
    )
    Assert-Condition ($parsed.Count -eq 4) 'Exact process command parser returned the wrong token count'
    Assert-Condition ($parsed[2] -ceq 'UiPilot') 'Exact process command parser changed a private argument'
    Assert-Condition (-not (($parsed -join "`n") -ceq (([UiPilot.ManagedEverything.NativeProbe]::SplitCommandLine(
        '"C:\Program Files\UiPilot_tools\Everything.exe" -instance UiPilot-decoy -startup'
    )) -join "`n"))) 'Exact process command parser accepted a lookalike instance'
    $lease = [UiPilot.ManagedEverything.NativeProbe]::OpenProcessLease([uint32]$PID, $false)
    try {
        Assert-Condition $lease.IsAlive 'Current-process identity lease was not alive'
        Assert-Condition ($lease.CreationTime -gt 0) 'Current-process identity lease has no creation identity'
        Assert-Condition ($lease.SessionId -eq [uint32](Get-Process -Id $PID).SessionId) 'Current-process identity lease session mismatch'
        Assert-Condition ((Split-Path -Leaf $lease.ImagePath) -ieq 'powershell.exe') 'Current-process identity lease image mismatch'
    }
    finally { $lease.Dispose() }
    Assert-Condition (Test-Query2Metadata '{"requestFlags":341,"sortType":14,"items":[{"fullPath":"C:\\sensitive"}]}') 'Query2 metadata self-test rejected the fixed contract'
    Assert-Condition (-not (Test-Query2Metadata '{"requestFlags":340,"sortType":14,"items":[]}')) 'Query2 metadata self-test accepted the wrong flags'
    Assert-Condition (Test-RetryBudgetSequence @($true, $true, $true, $false)) 'Retry-budget self-test rejected three observed replacements and the terminal fourth death'
    Assert-Condition (-not (Test-RetryBudgetSequence @($true, $true, $false, $false))) 'Retry-budget self-test accepted fewer than three observed replacements'
    Assert-Condition (-not (Test-RetryBudgetSequence @($true, $true, $true, $true))) 'Retry-budget self-test accepted a fourth replacement'
    $caught = $false
    try { Assert-RedactedJson '{"value":"C:\\sensitive"}' } catch { $caught = $true }
    Assert-Condition $caught 'Evidence redaction self-test did not reject a path'
    $caught = $false
    try { Assert-RedactedJson '{"value":"S-1-5-21-1-2-3-1001"}' } catch { $caught = $true }
    Assert-Condition $caught 'Evidence redaction self-test did not reject a SID'
    $caught = $false
    $savedMarker = $DisposableVmMarker
    try {
        $script:DisposableVmMarker = $null
        Assert-DisposableVmAdmission
    } catch { $caught = $true }
    finally { $script:DisposableVmMarker = $savedMarker }
    Assert-Condition $caught 'VM dual guard self-test did not fail closed'
    Write-Output 'MANAGED_EVERYTHING_RUNTIME_SELFTEST_PASS'
}

function Invoke-InstalledRuntimeProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmInstalledRuntime requires ClientProcessId'
    Add-RuntimeNativeProbe
    $managed = Get-ManagedRuntimeState
    $serviceContext = $null
    $clientContext = $null
    try {
        $serviceContext = Get-ServiceProcessContext $managed $false
        $clientContext = Get-PrivateClientContext $ClientProcessId $managed $null $false $false $true
        $readiness = Invoke-PrivateReadinessProbe $clientContext
        $ownedPid = [uint32]$clientContext.Record.ParentProcessId -eq $clientContext.HostLease.ProcessId -and
            $clientContext.Lease.CreationTime -gt 0 -and $clientContext.HostLease.CreationTime -gt 0
        $checks = [ordered]@{
            schema_version = 1
            kind = 'installed-runtime'
            outcome = 'pass'
            service_identity = ($serviceContext.Service.Name -ceq 'Everything (UiPilot)' -and $serviceContext.Lease.CreationTime -gt 0)
            service_running = $serviceContext.Lease.IsAlive
            service_command = $true
            managed_acl = $true
            manifest_identity = $true
            resource_identity = ($clientContext.ClientHash -ceq (Get-ReviewedEverythingHash))
            client_command = $true
            session_storage = $true
            owned_pid = $ownedPid
            host_parent_session = ($clientContext.HostLease.SessionId -eq $clientContext.Lease.SessionId)
            private_window = [UiPilot.ManagedEverything.NativeProbe]::WindowBelongsToProcess($clientContext.Window, $clientContext.Lease.ProcessId)
            job_membership = $clientContext.Lease.IsInAnyJob()
            database_loaded = $readiness.DatabaseLoaded
            query2_contract = $readiness.Query2Metadata
            readiness = $readiness.Ready
        }
        $failed = @($checks.GetEnumerator() | Where-Object { $_.Value -is [bool] -and -not $_.Value })
        if ($failed.Count -ne 0) { $checks.outcome = 'fail' }
        Write-ManagedEvidence $EvidenceName $checks
        Assert-Condition ($failed.Count -eq 0) 'Installed runtime probe failed one or more stable checks'
    }
    finally {
        if ($null -ne $clientContext) { Close-PrivateClientContext $clientContext }
        if ($null -ne $serviceContext) { $serviceContext.Lease.Dispose() }
    }
}

function Invoke-RecoveryProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmRecovery requires ClientProcessId'
    Add-RuntimeNativeProbe
    $managed = Get-ManagedRuntimeState
    if ($RecoveryTarget -ceq 'Client') {
        $current = $null
        $replacement = $null
        $hostLease = $null
        try {
            $current = Get-PrivateClientContext $ClientProcessId $managed $null $true $false $true
            $hostLease = $current.HostLease
            $current.OwnsHostLease = $false
            $oldIdentity = '{0}:{1}' -f $current.Lease.ProcessId, $current.Lease.CreationTime
            $current.Lease.TerminateAndWait(1, 5000)
            Close-PrivateClientContext $current
            $current = $null
            $replacement = Wait-ForReadyPrivateClient $managed $hostLease @($oldIdentity) $false
            $terminalMatches = $null -ne $replacement -and $replacement.Lease.CreationTime -gt 0 -and $hostLease.IsAlive
            Write-ManagedEvidence $EvidenceName ([ordered]@{
                schema_version = 1; kind = 'runtime-recovery'; outcome = $(if ($terminalMatches) { 'pass' } else { 'fail' })
                target = 'client'; expected_terminal = 'ready'; terminal_matches = $terminalMatches
                verified_handle_termination = $true; replacement_ready = $terminalMatches; restart_count = $(if ($terminalMatches) { 1 } else { 0 })
            })
            Assert-Condition $terminalMatches 'Runtime client recovery did not produce one exact Ready replacement'
        }
        finally {
            if ($null -ne $current) { Close-PrivateClientContext $current }
            if ($null -ne $replacement) { Close-PrivateClientContext $replacement }
            if ($null -ne $hostLease) { $hostLease.Dispose() }
        }
    } else {
        $serviceContext = $null
        $clientContext = $null
        try {
            $serviceContext = Get-ServiceProcessContext $managed $true
            $clientContext = Get-PrivateClientContext $ClientProcessId $managed $null $false $false $true
            $serviceContext.Lease.TerminateAndWait(1, 5000)
            $terminalMatches = Wait-Until {
                $service = Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction SilentlyContinue
                $serviceStopped = $null -eq $service -or [string]$service.State -cne 'Running' -or [uint32]$service.ProcessId -eq 0
                if (-not $serviceStopped) { return $false }
                if (-not $clientContext.Lease.IsAlive) { return $true }
                return -not (Invoke-PrivateReadinessProbe $clientContext 500).Ready
            }
            Write-ManagedEvidence $EvidenceName ([ordered]@{
                schema_version = 1; kind = 'runtime-recovery'; outcome = $(if ($terminalMatches) { 'pass' } else { 'fail' })
                target = 'service'; expected_terminal = 'unavailable'; terminal_matches = $terminalMatches
                verified_handle_termination = $true; service_stopped = $terminalMatches
            })
            Assert-Condition $terminalMatches 'Runtime did not become unavailable after exact Service termination'
        }
        finally {
            if ($null -ne $clientContext) { Close-PrivateClientContext $clientContext }
            if ($null -ne $serviceContext) { $serviceContext.Lease.Dispose() }
        }
    }
}

function Invoke-RetryBudgetProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmRetryBudget requires ClientProcessId'
    Add-RuntimeNativeProbe
    $managed = Get-ManagedRuntimeState
    $current = $null
    $hostLease = $null
    $serviceContext = $null
    $observations = @()
    $excluded = @()
    $restartCount = 0
    try {
        $current = Get-PrivateClientContext $ClientProcessId $managed $null $true $false $true
        $hostLease = $current.HostLease
        $current.OwnsHostLease = $false
        for ($attempt = 0; $attempt -lt 4; $attempt++) {
            $identity = '{0}:{1}' -f $current.Lease.ProcessId, $current.Lease.CreationTime
            $excluded += $identity
            $current.Lease.TerminateAndWait(1, 5000)
            Close-PrivateClientContext $current
            $current = $null
            $replacement = Wait-ForReadyPrivateClient $managed $hostLease $excluded ($attempt -lt 3) 65
            $replacementReady = $null -ne $replacement
            $observations += $replacementReady
            if ($attempt -lt 3) {
                Assert-Condition $replacementReady 'Runtime exhausted its restart budget before three Ready replacements'
                $restartCount++
                $current = $replacement
            } else {
                if ($null -ne $replacement) { Close-PrivateClientContext $replacement }
            }
        }
        $terminalStable = $hostLease.IsAlive
        for ($sample = 0; $sample -lt 10 -and $terminalStable; $sample++) {
            if (Test-HasExactPrivateClient $managed $hostLease $excluded) { $terminalStable = $false; break }
            Start-Sleep -Milliseconds 200
        }
        $serviceContext = Get-ServiceProcessContext $managed $false
        $sequenceMatches = Test-RetryBudgetSequence ([bool[]]$observations)
        $pass = $sequenceMatches -and $terminalStable -and $serviceContext.Lease.IsAlive -and $restartCount -eq 3
        Write-ManagedEvidence $EvidenceName ([ordered]@{
            schema_version = 1; kind = 'runtime-retry-budget'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
            restart_count = $restartCount; termination_count = $observations.Count
            first_three_ready = ($observations.Count -eq 4 -and $observations[0] -and $observations[1] -and $observations[2])
            fourth_replacement_absent = ($observations.Count -eq 4 -and -not $observations[3])
            terminal_unavailable = $terminalStable; host_remains = $hostLease.IsAlive; service_remains = $serviceContext.Lease.IsAlive
        })
        Assert-Condition $pass 'Runtime retry budget did not show three replacements followed by stable unavailable'
    }
    finally {
        if ($null -ne $current) { Close-PrivateClientContext $current }
        if ($null -ne $serviceContext) { $serviceContext.Lease.Dispose() }
        if ($null -ne $hostLease) { $hostLease.Dispose() }
    }
}

function Invoke-ShutdownProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmShutdown requires ClientProcessId'
    Add-RuntimeNativeProbe
    $managed = Get-ManagedRuntimeState
    $clientContext = $null
    $serviceContext = $null
    try {
        $clientContext = Get-PrivateClientContext $ClientProcessId $managed $null $false ($ShutdownAction -ceq 'ForceHost') $true
        if ($ShutdownAction -ceq 'ForceHost') {
            Assert-Condition ($UiPilotProcessId -ne 0) 'ForceHost shutdown requires UiPilotProcessId'
            Assert-Condition ($clientContext.HostLease.ProcessId -eq $UiPilotProcessId) 'ForceHost PID is not the exact parent of the private client'
            $clientContext.HostLease.TerminateAndWait(1, 5000)
        }
        $clientExited = Wait-Until { return -not $clientContext.Lease.IsAlive }
        $serviceContext = Get-ServiceProcessContext $managed $false
        $serviceRemains = $serviceContext.Lease.IsAlive
        Write-ManagedEvidence $EvidenceName ([ordered]@{
            schema_version = 1; kind = 'runtime-shutdown'; outcome = $(if ($clientExited -and $serviceRemains) { 'pass' } else { 'fail' })
            action = $ShutdownAction.ToLowerInvariant(); client_exited = $clientExited; service_remains = $serviceRemains
            host_parent_verified = $true; verified_handle_termination = ($ShutdownAction -ceq 'ForceHost')
        })
        Assert-Condition ($clientExited -and $serviceRemains) 'Runtime shutdown cleanup mismatch'
    }
    finally {
        if ($null -ne $clientContext) { Close-PrivateClientContext $clientContext }
        if ($null -ne $serviceContext) { $serviceContext.Lease.Dispose() }
    }
}

function Invoke-MultiSessionProbe {
    Assert-Condition ($ClientProcessId -ne 0 -and $OtherSessionClientProcessId -ne 0) 'VmMultiSession requires both client PIDs'
    $managed = Get-ManagedRuntimeState
    $one = $null
    $two = $null
    $serviceContext = $null
    try {
        $one = Get-PrivateClientContext $ClientProcessId $managed $null $false $false $true
        $two = Get-PrivateClientContext $OtherSessionClientProcessId $managed $null $false $false $false
        $serviceContext = Get-ServiceProcessContext $managed $false
        $differentSessions = $one.Lease.SessionId -ne $two.Lease.SessionId
        $storageDifferent = -not [string]::Equals($one.Ini, $two.Ini, [StringComparison]::OrdinalIgnoreCase) -and
            -not [string]::Equals($one.Database, $two.Database, [StringComparison]::OrdinalIgnoreCase)
        $pass = $differentSessions -and $storageDifferent -and $one.Lease.UserSid -ceq $two.Lease.UserSid -and $serviceContext.Lease.IsAlive
        Write-ManagedEvidence $EvidenceName ([ordered]@{
            schema_version = 1; kind = 'runtime-multi-session'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
            different_sessions = $differentSessions; distinct_storage = $storageDifferent; commands_valid = $true; one_shared_service = $serviceContext.Lease.IsAlive
        })
        Assert-Condition $pass 'Multi-session runtime isolation mismatch'
    }
    finally {
        if ($null -ne $one) { Close-PrivateClientContext $one }
        if ($null -ne $two) { Close-PrivateClientContext $two }
        if ($null -ne $serviceContext) { $serviceContext.Lease.Dispose() }
    }
}

function Invoke-PipeSecurityProbe {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($TokenCase)) 'VmPipeSecurity requires TokenCase'
    if ($TokenSource.StartsWith('Credential')) { Assert-Condition ($null -ne $Credential) 'Credential token source requires Credential' }
    if ($TokenSource -ceq 'Process') { Assert-Condition ($TokenProcessId -ne 0) 'Process token source requires TokenProcessId' }
    Add-RuntimeNativeProbe
    $managed = Get-ManagedRuntimeState
    $parts = Get-CredentialParts
    $probe = [UiPilot.ManagedEverything.NativeProbe]::ProbePipe(
        '\\.\PIPE\UiPilotEverythingService', [string]$managed.Manifest.expanded_sddl,
        $TokenSource, $TokenProcessId, $parts[0], $parts[1], $parts[2]
    )
    $allowCases = @('AuthorizedInteractive', 'AuthorizedBatch', 'AuthorizedNetwork', 'LocalSystem', 'ElevatedAdmin')
    $expectedAllow = $allowCases -contains $TokenCase
    if ($TokenCase.StartsWith('Authorized')) { Assert-Condition ($probe.Sid -ceq [string]$managed.Manifest.desktop_user_sid) 'Authorized token SID does not match the manifest' }
    if (@('OtherInteractive', 'OtherBatch', 'NetworkService', 'FilteredAdmin') -contains $TokenCase) {
        Assert-Condition ($probe.Sid -cne [string]$managed.Manifest.desktop_user_sid) 'A desktop-user token cannot be used as a negative case'
    }
    if ($TokenCase -ceq 'LocalSystem') { Assert-Condition ($probe.Sid -ceq 'S-1-5-18') 'LocalSystem token mismatch' }
    if ($TokenCase -ceq 'NetworkService') { Assert-Condition ($probe.Sid -ceq 'S-1-5-20') 'NetworkService token mismatch' }
    if ($TokenCase -ceq 'ElevatedAdmin') { Assert-Condition $probe.Administrator 'Elevated administrator token mismatch' }
    $accessMatches = if ($expectedAllow) {
        $probe.ReadAllowed -and $probe.WriteAllowed -and $probe.Connected
    } else {
        (-not $probe.ReadAllowed) -and (-not $probe.WriteAllowed) -and (-not $probe.Connected) -and
            (-not $probe.CreatedInstance) -and $probe.ConnectError -eq 5 -and $probe.CreateError -eq 5
    }
    $pass = $probe.DescriptorMatches -and $accessMatches
    Write-ManagedEvidence $EvidenceName ([ordered]@{
        schema_version = 1; kind = 'pipe-security'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
        token_case = $TokenCase.ToLowerInvariant(); descriptor_matches = $probe.DescriptorMatches
        expected_access = $(if ($expectedAllow) { 'allow' } else { 'deny' }); access_matches = $accessMatches
        read = $probe.ReadAllowed; write = $probe.WriteAllowed; connect = $probe.Connected; create_instance = $probe.CreatedInstance
    })
    Assert-Condition $pass 'Named-pipe security probe mismatch'
}

function Invoke-CoexistenceProbe {
    Assert-Condition ($ClientProcessId -ne 0 -and $DefaultEverythingProcessId -ne 0) 'VmCoexistence requires private and default Everything PIDs'
    $managed = Get-ManagedRuntimeState
    $private = $null
    $defaultLease = $null
    try {
        $private = Get-PrivateClientContext $ClientProcessId $managed $null $false $false $true
        $defaultLease = [UiPilot.ManagedEverything.NativeProbe]::OpenProcessLease($DefaultEverythingProcessId, $false)
        $default = Get-ProcessRecord $DefaultEverythingProcessId
        Assert-Condition ([string]$default.Name -ceq 'Everything.exe') 'Default Everything image name mismatch'
        Assert-Condition ((Split-Path -Leaf $defaultLease.ImagePath) -ceq 'Everything.exe') 'Default Everything native image mismatch'
        $defaultTokens = Get-CommandLineTokens ([string]$default.CommandLine)
        $looksPrivate = $defaultTokens.Count -eq 10 -and $defaultTokens[1] -ceq '-instance' -and $defaultTokens[2] -ceq 'UiPilot' -and
            $defaultTokens[3] -ceq '-service-pipe-name' -and $defaultTokens[4] -ceq '\\.\PIPE\UiPilotEverythingService'
        $defaultUntouched = -not $looksPrivate -and $defaultLease.IsAlive -and $defaultLease.CreationTime -gt 0
        $pass = $private.Lease.IsAlive -and $defaultUntouched
        Write-ManagedEvidence $EvidenceName ([ordered]@{
            schema_version = 1; kind = 'runtime-coexistence'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
            private_identity = $private.Lease.IsAlive; default_instance_unchanged = $defaultUntouched
        })
        Assert-Condition $pass 'Default Everything coexistence mismatch'
    }
    finally {
        if ($null -ne $private) { Close-PrivateClientContext $private }
        if ($null -ne $defaultLease) { $defaultLease.Dispose() }
    }
}

switch ($Mode) {
    'Source' { Invoke-SourceGate }
    'SelfTest' { Invoke-SelfTest }
    default {
        Assert-DisposableVmAdmission
        switch ($Mode) {
            'VmInstalledRuntime' { Invoke-InstalledRuntimeProbe }
            'VmRecovery' { Invoke-RecoveryProbe }
            'VmRetryBudget' { Invoke-RetryBudgetProbe }
            'VmShutdown' { Invoke-ShutdownProbe }
            'VmMultiSession' { Invoke-MultiSessionProbe }
            'VmPipeSecurity' { Invoke-PipeSecurityProbe }
            'VmCoexistence' { Invoke-CoexistenceProbe }
        }
    }
}
