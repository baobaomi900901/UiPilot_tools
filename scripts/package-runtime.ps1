# Shared packaging core; dot-source only. Windows PowerShell 5.1.
Set-StrictMode -Version Latest
function Write-PackageJson([string]$Path, $Value) { [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false)) }
function Read-PackageJson([string]$Path) { return ([IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8) | ConvertFrom-Json) }
function Assert-PackageDirectoryPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Path]::IsPathRooted($Path)) { throw 'Choose an absolute output directory.' }
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.Length -gt [IO.Path]::GetPathRoot($full).Length) { $full=$full.TrimEnd('\') }
    $cursor = $full
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Output path contains a reparse point: $cursor" }
            if (-not $item.PSIsContainer) { throw "Output path is not a directory: $cursor" }
        }
        $cursor = Split-Path -Parent $cursor
    }
    return $full
}
function Get-PackageManifest([string]$Source) {
    $manifest = Read-PackageJson (Join-Path $Source 'plugin.json'); $id = [string]$manifest.pluginId; $version = [string]$manifest.version
    if ($id -notmatch '^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9_-])?$' -or $id -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)') { throw 'Plugin ID is not a safe filename.' }
    if ($version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') { throw 'Plugin version is not a safe version.' }
    return $manifest
}
function New-PackageRun([string]$Root, [ValidateSet('Setup','Plugin')][string]$Mode, [string]$OutputRoot, [string]$Source = '') {
    $output = Assert-PackageDirectoryPath $OutputRoot
    if ($Mode -eq 'Plugin') {
        $Source = [IO.Path]::GetFullPath($Source).TrimEnd('\'); $null = Get-PackageManifest $Source
        if ($output.Equals($Source,[StringComparison]::OrdinalIgnoreCase) -or $output.StartsWith($Source + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Output must be outside the plugin source directory.' }
    }
    $config = Read-PackageJson (Join-Path $Root 'src-tauri/tauri.conf.json')
    if ([string]$config.version -notmatch '^\d+\.\d+\.\d+([+-][A-Za-z0-9.-]+)?$' -or [string]$config.productName -notmatch '^[A-Za-z0-9_-]+$') { throw 'Unsafe application version or product filename.' }
    [IO.Directory]::CreateDirectory($output) | Out-Null; $null = Assert-PackageDirectoryPath $output
    $id = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N'); $dir = Join-Path $output ($Mode.ToLowerInvariant() + '-' + $id)
    New-Item -ItemType Directory -Path $dir -ErrorAction Stop | Out-Null
    return [pscustomobject]@{ SchemaVersion=1; RunId=$id; Mode=$Mode; Root=$Root; Source=$Source; RunDirectory=$dir; Version=[string]$config.version; ProductName=[string]$config.productName }
}
function Write-PackageLog($Request, [string]$Text) { [IO.File]::AppendAllText((Join-Path $Request.RunDirectory 'run.log'), $Text + [Environment]::NewLine, [Text.UTF8Encoding]::new($false)) }
function Set-PackageStage($Request, [string]$Stage) {
    Write-PackageLog $Request ('[' + (Get-Date -Format 'HH:mm:ss') + '] ' + $Stage)
    Write-PackageJson (Join-Path $Request.RunDirectory 'status.json') @{ Stage=$Stage }
}
function Test-PackageCancelled($Request) { if (Test-Path -LiteralPath (Join-Path $Request.RunDirectory 'cancel.flag')) { throw 'Packaging cancelled.' } }
function Clear-PackageBuildEnvironment {
    foreach ($name in @('CARGO_TARGET_DIR','CARGO_BUILD_TARGET','TAURI_CONFIG','TAURI_CONFIG_PATH','TAURI_BUILD_TARGET','CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS')) { [Environment]::SetEnvironmentVariable($name,$null,'Process') }
}
function Get-PackageTools([string]$Root, [string]$Mode) {
    if (-not (Test-Path -LiteralPath (Join-Path $Root 'node_modules/.bin/tauri.cmd') -PathType Leaf)) { throw 'Dependencies are missing. Run npm ci in the repository, then retry.' }
    $node = Get-Command node.exe -ErrorAction SilentlyContinue; $npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
    if (-not $node -or -not $npm) { throw 'Install Node.js 22 LTS with npm, then run npm ci.' }
    $npmCli = Join-Path (Split-Path $npm.Source) 'node_modules/npm/bin/npm-cli.js'
    if (-not (Test-Path -LiteralPath $npmCli -PathType Leaf)) { throw 'Cannot locate npm-cli.js beside npm.cmd. Repair the Node.js/npm installation.' }
    if ($Mode -eq 'Setup') {
        if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Setup requires Windows x64 and 64-bit PowerShell.' }
        foreach ($name in @('cargo.exe','rustc.exe')) { if (-not (Get-Command $name -ErrorAction SilentlyContinue)) { throw "Missing $name. Install the repository Rust toolchain and MSVC C++ build tools." } }
        $rust = @(& rustc.exe -vV 2>&1)
        if ($LASTEXITCODE -ne 0 -or ($rust -join "`n") -notmatch '(?m)^host: x86_64-pc-windows-msvc\s*$') { throw 'The active Rust toolchain must use host x86_64-pc-windows-msvc (see rust-toolchain.toml).' }
        foreach ($path in @('src-tauri/tauri.conf.json','src-tauri/Cargo.toml','src-tauri/resources/everything/Everything.exe','src-tauri/resources/everything/everything.lock.json','src-tauri/resources/everything/LICENSE.txt','src-tauri/resources/everything/Everything-UiPilot.ini','scripts/build-managed-everything-installer.ps1')) {
            if (-not (Test-Path -LiteralPath (Join-Path $Root $path) -PathType Leaf)) { throw "Missing required Setup resource: $path" }
        }
    }
    return @{ Node=$node.Source; NpmCli=$npmCli }
}
function ConvertTo-PackageArgument([string]$Value) {
    # Windows CRT quoting. No cmd.exe or script evaluation.
    return '"' + ([regex]::Replace(([regex]::Replace($Value, '(\\*)"', '$1$1\"')), '(\\+)$', '$1$1')) + '"'
}
function Invoke-PackageCommand([string]$File, [string[]]$Arguments, [string]$WorkingDirectory, $Request) {
    Test-PackageCancelled $Request
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName=$File; $info.Arguments=(@($Arguments | ForEach-Object { ConvertTo-PackageArgument $_ }) -join ' ')
    $info.WorkingDirectory=$WorkingDirectory; $info.UseShellExecute=$false; $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    $info.StandardOutputEncoding=[Text.Encoding]::UTF8; $info.StandardErrorEncoding=[Text.Encoding]::UTF8
    $process = New-Object Diagnostics.Process; $process.StartInfo=$info
    try {
        $null=$process.Start(); $stdout=$process.StandardOutput.ReadLineAsync(); $stderr=$process.StandardError.ReadLineAsync()
        while ($null -ne $stdout -or $null -ne $stderr) {
            Test-PackageCancelled $Request
            foreach ($stream in @('stdout','stderr')) {
                $task=Get-Variable -Name $stream -ValueOnly
                if ($null -ne $task -and $task.IsCompleted) {
                    $line=$task.GetAwaiter().GetResult()
                    if ($null -eq $line) { Set-Variable -Name $stream -Value $null }
                    else {
                        Write-PackageLog $Request $line
                        $reader=if ($stream -eq 'stdout') { $process.StandardOutput } else { $process.StandardError }
                        Set-Variable -Name $stream -Value ($reader.ReadLineAsync())
                    }
                }
            }
            Start-Sleep -Milliseconds 25
        }
        $process.WaitForExit(); Test-PackageCancelled $Request
        return $process.ExitCode
    } finally { $process.Dispose() }
}
function Invoke-PackageBuild($Request, $Tools, [scriptblock]$RunCommand = ${function:Invoke-PackageCommand}) {
    $root=[string]$Request.Root; $artifact=''; $before=''; $started=[DateTime]::UtcNow
    if ($Request.Mode -eq 'Setup') {
        $candidate=Join-Path $root ('src-tauri/target/release/bundle/nsis/' + $Request.ProductName + '_' + $Request.Version + '_x64-setup.exe')
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $item=Get-Item -LiteralPath $candidate; $before=$item.LastWriteTimeUtc.Ticks.ToString() + ':' + (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash }
        $steps=@(@{Stage='Building production Release / Windows x64 / NSIS'; Args=@($Tools.NpmCli,'run','tauri','--','build','--bundles','nsis')})
    } else {
        $manifest=Get-PackageManifest $Request.Source; $artifact=Join-Path $Request.RunDirectory ($manifest.pluginId + '-' + $manifest.version + '.uipilot-plugin')
        if (Test-Path -LiteralPath $artifact) { throw 'Output artifact already exists; start a new run.' }
        $cli=Join-Path $root 'packages/plugin-cli/dist/cli.mjs'
        $steps=@(
            @{Stage='Building current plugin CLI'; Args=@($Tools.NpmCli,'run','build','--workspace','@uipilot/plugin-cli')},
            @{Stage='Validating plugin source'; Args=@($cli,'validate',$Request.Source,'--platform','windows','--json')},
            @{Stage='Packing plugin archive'; Args=@($cli,'pack',$Request.Source,'--output',$artifact,'--platform','windows','--json')},
            @{Stage='Validating plugin archive'; Args=@($cli,'validate',$artifact,'--platform','windows','--json')}
        )
    }
    foreach ($step in $steps) {
        Test-PackageCancelled $Request; Set-PackageStage $Request $step.Stage
        $code=& $RunCommand $Tools.Node $step.Args $root $Request
        Test-PackageCancelled $Request
        if ($code -ne 0) { throw ($step.Stage + ' failed (exit ' + $code + '). See run.log.') }
    }
    if ($Request.Mode -eq 'Setup') {
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw 'No fresh current-version production Setup was generated.' }
        $item=Get-Item -LiteralPath $candidate; $after=$item.LastWriteTimeUtc.Ticks.ToString() + ':' + (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash
        if ($item.LastWriteTimeUtc -lt $started.AddSeconds(-2) -or $before -eq $after -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'No fresh current-version production Setup was generated; existing Setup is stale.' }
        $artifact=Join-Path $Request.RunDirectory $item.Name; [IO.File]::Copy($candidate,$artifact,$false)
    }
    Test-PackageCancelled $Request
    if (-not (Test-Path -LiteralPath $artifact -PathType Leaf) -or (Get-Item -LiteralPath $artifact).Length -eq 0) { throw 'Expected output artifact is missing or empty.' }
    $hash=(Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    $result=[pscustomobject]@{ SchemaVersion=1; RunId=$Request.RunId; State='Success'; Artifact=$artifact; Sha256=$hash; Message='Packaging completed.' }
    Write-PackageLog $Request ("SHA256 $hash  $artifact")
    return $result
}
function Get-PackageCompletion($Request, [int]$ExitCode, [bool]$Cancelled) {
    if ($Cancelled -or (Test-Path -LiteralPath (Join-Path $Request.RunDirectory 'cancel.flag'))) { return [pscustomobject]@{State='Cancelled'; Message='Packaging cancelled. Logs retained.'} }
    try {
        $result=Read-PackageJson (Join-Path $Request.RunDirectory 'result.json')
        if ($ExitCode -ne 0 -or $result.RunId -cne $Request.RunId -or $result.State -ne 'Success') { throw $result.Message }
        if ((Split-Path -Parent $result.Artifact) -cne $Request.RunDirectory -or -not (Test-Path -LiteralPath $result.Artifact -PathType Leaf)) { throw 'Expected artifact is missing.' }
        if ((Get-FileHash -LiteralPath $result.Artifact -Algorithm SHA256).Hash.ToLowerInvariant() -cne $result.Sha256) { throw 'Output SHA256 mismatch.' }
        return $result
    } catch { return [pscustomobject]@{State='Failed'; Message=('Packaging failed: ' + $_.Exception.Message + ' See run.log.')} }
}
function Publish-PackageArtifact($Request, $Result) {
    Test-PackageCancelled $Request
    if ($Result.State -ne 'Success' -or $Result.RunId -cne $Request.RunId) { throw 'No successful build to publish.' }
    $source=[string]$Result.Artifact
    if ((Split-Path -Parent $source) -cne $Request.RunDirectory -or -not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Unexpected artifact path.' }
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Result.Sha256) { throw 'Artifact hash mismatch.' }
    $folder=if ($Request.Mode -eq 'Setup') { 'setup' } else { 'plugins' }
    $output=Assert-PackageDirectoryPath (Join-Path $Request.Root ('artifacts/packages/' + $folder))
    [IO.Directory]::CreateDirectory($output) | Out-Null
    $null=Assert-PackageDirectoryPath $output
    $destination=Join-Path $output (Split-Path -Leaf $source)
    if (Test-Path -LiteralPath $destination) {
        $existing=Get-Item -LiteralPath $destination -Force
        if ($existing.PSIsContainer -or ($existing.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Output must be an ordinary file.' }
    }
    $temporary=Join-Path $output ('.publish-' + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::Copy($source,$temporary,$false)
        if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Result.Sha256) { throw 'Published artifact hash mismatch.' }
        Test-PackageCancelled $Request
        if ([IO.File]::Exists($destination)) { [IO.File]::Replace($temporary,$destination,[NullString]::Value) }
        else { [IO.File]::Move($temporary,$destination) }
        return $destination
    } finally { if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) } }
}
function Initialize-PackageOwnership {
    if ('UiPilot.PackagingJob' -as [type]) { return }
    # Assign a suspended worker before it can spawn children. No name scans.
    Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
namespace UiPilot {
 public sealed class PackagingJob : IDisposable {
  [StructLayout(LayoutKind.Sequential)] struct SI { public int cb; public IntPtr reserved, desktop, title; public int x,y,xsize,ysize,xchars,ychars,fill,flags; public short show,reservedSize; public IntPtr reserved2,input,output,error; }
  [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr process,thread; public int pid,tid; }
  [StructLayout(LayoutKind.Sequential)] struct Limits { public long user,kernel; public uint flags; public UIntPtr min,max; public uint active; public UIntPtr affinity; public uint priority,scheduling; }
  [StructLayout(LayoutKind.Sequential)] struct IO { public ulong r,w,o,rb,wb,ob; }
  [StructLayout(LayoutKind.Sequential)] struct Extended { public Limits basic; public IO io; public UIntPtr pm,jm,peakp,peakj; }
  [StructLayout(LayoutKind.Sequential)] struct Accounting { public long a,b,c,d; public uint faults,total,active,terminated; }
  [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr attr,string name);
  [DllImport("kernel32",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int type,ref Extended info,int size);
  [DllImport("kernel32",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int type,out Accounting info,int size,IntPtr length);
  [DllImport("kernel32",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
  [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcess(string app,StringBuilder args,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref SI si,out PI pi);
  [DllImport("kernel32",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
  [DllImport("kernel32")] static extern bool TerminateProcess(IntPtr process,uint code);
  [DllImport("kernel32",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint code);
  [DllImport("kernel32")] static extern bool CloseHandle(IntPtr handle);
  IntPtr job; public Process Process {get; private set;}
  public PackagingJob(string file,string args,string cwd) {
   job=CreateJobObject(IntPtr.Zero,null); if(job==IntPtr.Zero) throw new Win32Exception();
   try {
    Extended limits=new Extended(); limits.basic.flags=0x2000;
    if(!SetInformationJobObject(job,9,ref limits,Marshal.SizeOf(limits))) throw new Win32Exception();
    SI si=new SI(); si.cb=Marshal.SizeOf(si); si.flags=1; si.show=0; PI pi;
    if(!CreateProcess(file,new StringBuilder("\""+file+"\" "+args),IntPtr.Zero,IntPtr.Zero,false,0x08000004,IntPtr.Zero,cwd,ref si,out pi)) throw new Win32Exception();
    try {
     if(!AssignProcessToJobObject(job,pi.process)) throw new Win32Exception();
     Process=Process.GetProcessById(pi.pid);
     IntPtr retainedHandle=Process.Handle; // Retain exit status before resuming.
     if(ResumeThread(pi.thread)==0xffffffff) throw new Win32Exception();
    } catch { TerminateProcess(pi.process,1); throw; }
    finally { CloseHandle(pi.thread); CloseHandle(pi.process); }
   } catch { Dispose(); throw; }
  }
  public bool IsEmpty { get { Accounting a; if(!QueryInformationJobObject(job,1,out a,Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero)) throw new Win32Exception(); return a.active==0; } }
  public void Cancel() { if(!TerminateJobObject(job,2)) throw new Win32Exception(); }
  public void Dispose() { if(job!=IntPtr.Zero) { CloseHandle(job); job=IntPtr.Zero; } if(Process!=null) Process.Dispose(); }
 }
}
"@
}
function Start-PackageWorker($Request, [string]$RequestPath) {
    Initialize-PackageOwnership
    $runner=Join-Path $PSScriptRoot 'package-runner.ps1'
    $arguments=@('-NoProfile','-ExecutionPolicy','Bypass','-File',$runner,'-RequestPath',$RequestPath)
    $quoted=(@($arguments | ForEach-Object { ConvertTo-PackageArgument $_ }) -join ' ')
    return [UiPilot.PackagingJob]::new((Join-Path $PSHOME 'powershell.exe'),$quoted,$Request.Root)
}
function Stop-PackageWorker($Worker, $Request) {
    [IO.File]::WriteAllText((Join-Path $Request.RunDirectory 'cancel.flag'),'cancel', [Text.UTF8Encoding]::new($false))
    $Worker.Cancel()
}
