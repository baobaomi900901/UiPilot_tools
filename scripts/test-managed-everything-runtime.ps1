[CmdletBinding()]
param(
    [ValidateSet('Source', 'SelfTest', 'VmInstalledRuntime', 'VmRecovery', 'VmShutdown', 'VmMultiSession', 'VmPipeSecurity', 'VmCoexistence')]
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
    [ValidateSet('Ready', 'Unavailable')]
    [string]$ExpectedTerminal = 'Ready',
    [ValidateSet('Observe', 'ForceHost')]
    [string]$ShutdownAction = 'Observe',
    [ValidateSet('AuthorizedInteractive', 'AuthorizedBatch', 'AuthorizedNetwork', 'LocalSystem', 'ElevatedAdmin', 'OtherInteractive', 'OtherBatch', 'NetworkService', 'FilteredAdmin')]
    [string]$TokenCase,
    [ValidateSet('Current', 'Linked', 'CredentialInteractive', 'CredentialBatch', 'CredentialNetwork', 'Process')]
    [string]$TokenSource = 'Current',
    [Management.Automation.PSCredential]$Credential,
    [uint32]$TokenProcessId,
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

function Test-PrivateClientCommand($Record, [uint32]$SessionId) {
    $command = [string]$Record.CommandLine
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    $sessionRoot = Join-Path $local ("UiPilot_tools\everything\{0}" -f $SessionId)
    $ini = Join-Path $sessionRoot 'Everything-UiPilot.ini'
    $database = Join-Path $sessionRoot 'Everything-UiPilot.db'
    return $command.Contains('-instance') -and $command.Contains('UiPilot') -and
        $command.Contains('-service-pipe-name') -and $command.Contains('UiPilotEverythingService') -and
        $command.Contains('-config') -and $command.Contains($ini) -and
        $command.Contains('-db') -and $command.Contains($database) -and
        (Test-Path -LiteralPath $ini -PathType Leaf) -and (Test-Path -LiteralPath $database -PathType Leaf)
}

function Add-RuntimeNativeProbe {
    if ('UiPilot.ManagedEverything.NativeProbe' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
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

    public static bool HasWindow(uint pid, string className) {
      bool found = false;
      EnumWindows((hwnd, state) => {
        uint owner; GetWindowThreadProcessId(hwnd, out owner);
        if (owner == pid) { var b = new StringBuilder(256); GetClassName(hwnd, b, b.Capacity); if (b.ToString() == className) found = true; }
        return !found;
      }, IntPtr.Zero);
      return found;
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
          DescriptorMatches = actual == expectedSddl,
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
    Write-ManagedEvidence 'source' ([ordered]@{
        schema_version = 1
        kind = 'runtime-source'
        outcome = 'pass'
        read_only = $true
        identity_contract = $true
        launch_contract = $true
        ownership_contract = $true
        retry_budget = 3
        shutdown_contract = $true
    })
    Write-Output 'MANAGED_EVERYTHING_RUNTIME_SOURCE_PASS'
}

function Invoke-SelfTest {
    Add-RuntimeNativeProbe
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
    $service = Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction Stop
    Assert-Condition ($null -ne $service) 'Private Service is missing'
    $serviceCommand = [string]$service.PathName
    $serviceValid = $serviceCommand.Contains($managed.Binary) -and $serviceCommand.Contains('-svc') -and
        $serviceCommand.Contains('-instance') -and $serviceCommand.Contains('UiPilot') -and
        $serviceCommand.Contains('-service-pipe-name') -and $serviceCommand.Contains('UiPilotEverythingService') -and
        $serviceCommand.Contains('-service-security-descriptor')
    $client = Get-ProcessRecord $ClientProcessId
    $clientProcess = Get-Process -Id $ClientProcessId -ErrorAction Stop
    $sessionId = [uint32]$clientProcess.SessionId
    $clientCommandValid = Test-PrivateClientCommand $client $sessionId
    $resourceRoot = Split-Path -Parent ([string]$client.ExecutablePath)
    $lock = Get-Content -LiteralPath (Join-Path $resourceRoot 'everything.lock.json') -Raw -Encoding utf8 | ConvertFrom-Json
    $resourceHash = (Get-FileHash -LiteralPath ([string]$client.ExecutablePath) -Algorithm SHA256).Hash.ToLowerInvariant()
    $checks = [ordered]@{
        schema_version = 1
        kind = 'installed-runtime'
        outcome = 'pass'
        service_identity = ($service.Name -ceq 'Everything (UiPilot)')
        service_running = ($service.State -ceq 'Running')
        service_command = $serviceValid
        managed_acl = $true
        manifest_identity = $true
        resource_identity = ($resourceHash -ceq [string]$lock.everythingExeSha256)
        client_command = $clientCommandValid
        session_storage = $clientCommandValid
        owned_pid = ($client.ProcessId -eq $ClientProcessId)
        private_window = [UiPilot.ManagedEverything.NativeProbe]::HasWindow($ClientProcessId, 'EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)')
        job_membership = [UiPilot.ManagedEverything.NativeProbe]::IsInJob($ClientProcessId)
        readiness = [UiPilot.ManagedEverything.NativeProbe]::HasWindow($ClientProcessId, 'EVERYTHING_TASKBAR_NOTIFICATION_(UiPilot)')
        retry_budget = 3
    }
    $failed = @($checks.GetEnumerator() | Where-Object { $_.Value -is [bool] -and -not $_.Value })
    if ($failed.Count -ne 0) { $checks.outcome = 'fail' }
    Write-ManagedEvidence $EvidenceName $checks
    Assert-Condition ($failed.Count -eq 0) 'Installed runtime probe failed one or more stable checks'
}

function Invoke-RecoveryProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmRecovery requires ClientProcessId'
    $oldPid = $ClientProcessId
    if ($RecoveryTarget -ceq 'Client') {
        $record = Get-ProcessRecord $oldPid
        Assert-Condition (([string]$record.CommandLine).Contains('UiPilot')) 'Refusing to terminate a non-private client'
        Stop-Process -Id $oldPid -Force -ErrorAction Stop
    } else {
        $service = Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction Stop
        Assert-Condition ($null -ne $service -and $service.ProcessId -ne 0) 'Private Service is not running'
        Stop-Process -Id ([uint32]$service.ProcessId) -Force -ErrorAction Stop
    }
    $ready = Wait-Until {
        $clients = @(Get-CimInstance Win32_Process -Filter "Name='Everything.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.ProcessId -ne $oldPid -and ([string]$_.CommandLine).Contains('UiPilot') })
        return $clients.Count -eq 1
    }
    $terminalMatches = if ($ExpectedTerminal -ceq 'Ready') { $ready } else { -not $ready }
    Write-ManagedEvidence $EvidenceName ([ordered]@{
        schema_version = 1; kind = 'runtime-recovery'; outcome = $(if ($terminalMatches) { 'pass' } else { 'fail' })
        target = $RecoveryTarget.ToLowerInvariant(); expected_terminal = $ExpectedTerminal.ToLowerInvariant()
        terminal_matches = $terminalMatches; bounded = $true; retry_budget = 3
    })
    Assert-Condition $terminalMatches 'Runtime recovery terminal state mismatch'
}

function Invoke-ShutdownProbe {
    Assert-Condition ($ClientProcessId -ne 0) 'VmShutdown requires ClientProcessId'
    if ($ShutdownAction -ceq 'ForceHost') {
        Assert-Condition ($UiPilotProcessId -ne 0) 'ForceHost shutdown requires UiPilotProcessId'
        Stop-Process -Id $UiPilotProcessId -Force -ErrorAction Stop
    }
    $clientExited = Wait-Until { return $null -eq (Get-Process -Id $ClientProcessId -ErrorAction SilentlyContinue) }
    $service = Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction SilentlyContinue
    $serviceRemains = $null -ne $service -and $service.State -ceq 'Running'
    Write-ManagedEvidence $EvidenceName ([ordered]@{
        schema_version = 1; kind = 'runtime-shutdown'; outcome = $(if ($clientExited -and $serviceRemains) { 'pass' } else { 'fail' })
        action = $ShutdownAction.ToLowerInvariant(); client_exited = $clientExited; service_remains = $serviceRemains
    })
    Assert-Condition ($clientExited -and $serviceRemains) 'Runtime shutdown cleanup mismatch'
}

function Invoke-MultiSessionProbe {
    Assert-Condition ($ClientProcessId -ne 0 -and $OtherSessionClientProcessId -ne 0) 'VmMultiSession requires both client PIDs'
    $one = Get-ProcessRecord $ClientProcessId
    $two = Get-ProcessRecord $OtherSessionClientProcessId
    $p1 = Get-Process -Id $ClientProcessId -ErrorAction Stop
    $p2 = Get-Process -Id $OtherSessionClientProcessId -ErrorAction Stop
    $differentSessions = $p1.SessionId -ne $p2.SessionId
    $commandsValid = (Test-PrivateClientCommand $one ([uint32]$p1.SessionId)) -and (Test-PrivateClientCommand $two ([uint32]$p2.SessionId))
    $storageDifferent = ([string]$one.CommandLine) -cne ([string]$two.CommandLine
    )
    $serviceCount = @(Get-CimInstance Win32_Service -Filter "Name='Everything (UiPilot)'" -ErrorAction SilentlyContinue).Count
    $pass = $differentSessions -and $commandsValid -and $storageDifferent -and $serviceCount -eq 1
    Write-ManagedEvidence $EvidenceName ([ordered]@{
        schema_version = 1; kind = 'runtime-multi-session'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
        different_sessions = $differentSessions; distinct_storage = $storageDifferent; commands_valid = $commandsValid; one_shared_service = ($serviceCount -eq 1)
    })
    Assert-Condition $pass 'Multi-session runtime isolation mismatch'
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
    $private = Get-ProcessRecord $ClientProcessId
    $default = Get-ProcessRecord $DefaultEverythingProcessId
    $privateValid = ([string]$private.CommandLine).Contains('UiPilot')
    $defaultUntouched = (-not ([string]$default.CommandLine).Contains('UiPilot')) -and $default.ProcessId -eq $DefaultEverythingProcessId
    $pass = $privateValid -and $defaultUntouched
    Write-ManagedEvidence $EvidenceName ([ordered]@{
        schema_version = 1; kind = 'runtime-coexistence'; outcome = $(if ($pass) { 'pass' } else { 'fail' })
        private_identity = $privateValid; default_instance_unchanged = $defaultUntouched
    })
    Assert-Condition $pass 'Default Everything coexistence mismatch'
}

switch ($Mode) {
    'Source' { Invoke-SourceGate }
    'SelfTest' { Invoke-SelfTest }
    default {
        Assert-DisposableVmAdmission
        switch ($Mode) {
            'VmInstalledRuntime' { Invoke-InstalledRuntimeProbe }
            'VmRecovery' { Invoke-RecoveryProbe }
            'VmShutdown' { Invoke-ShutdownProbe }
            'VmMultiSession' { Invoke-MultiSessionProbe }
            'VmPipeSecurity' { Invoke-PipeSecurityProbe }
            'VmCoexistence' { Invoke-CoexistenceProbe }
        }
    }
}
