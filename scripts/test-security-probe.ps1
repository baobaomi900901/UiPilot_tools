[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string] $Executable,

  [string] $AppDataDir = (Join-Path `
    -Path ([Environment]::GetFolderPath('ApplicationData')) `
    -ChildPath 'com.uipilot.launcher')
)

$ErrorActionPreference = 'Stop'

function Get-ProtectedAppDataSnapshot([string] $Root) {
  $patterns = @(
    'settings.json*',
    'validation-data.json*',
    'open-session.json*'
  )
  $entries = @()
  $rootExists = Test-Path -LiteralPath $Root -PathType Container
  if ($rootExists) {
    $entries = @(
      Get-ChildItem -LiteralPath $Root -Force -File | Where-Object {
        $name = $_.Name
        @($patterns | Where-Object { $name -like $_ }).Count -ne 0
      } | Sort-Object Name | ForEach-Object {
        [pscustomobject]@{
          Name = $_.Name
          Length = $_.Length
          Sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
          LastWriteTicks = $_.LastWriteTimeUtc.Ticks
        }
      }
    )
  }
  ConvertTo-Json -Compress -Depth 3 -InputObject ([pscustomobject]@{
    RootExists = $rootExists
    Entries = @($entries)
  })
}

function Test-ProtectedAppDataSnapshot {
  $root = Join-Path `
    -Path ([IO.Path]::GetTempPath()) `
    -ChildPath "uipilot-probe-snapshot-$PID-$([Guid]::NewGuid().ToString('N'))"
  try {
    $missing = Get-ProtectedAppDataSnapshot $root
    [IO.Directory]::CreateDirectory($root) | Out-Null
    $created = Get-ProtectedAppDataSnapshot $root
    if ($missing -ceq $created) {
      throw 'protected app data snapshot does not record root existence'
    }

    $hidden = Join-Path -Path $root -ChildPath 'settings.json.hidden-test'
    [IO.File]::WriteAllText($hidden, 'hidden')
    [IO.File]::SetAttributes($hidden, [IO.FileAttributes]::Hidden)
    $withHidden = Get-ProtectedAppDataSnapshot $root
    if ($created -ceq $withHidden) {
      throw 'protected app data snapshot does not record hidden files'
    }
  }
  finally {
    if ([IO.Directory]::Exists($root)) {
      [IO.Directory]::Delete($root, $true)
    }
  }
}

$executableUri = $null
if (
  -not [Uri]::TryCreate($Executable, [UriKind]::Absolute, [ref] $executableUri) -or
  -not $executableUri.IsFile
) {
  throw 'Executable must be an absolute path'
}
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
Test-ProtectedAppDataSnapshot
$before = Get-ProtectedAppDataSnapshot $AppDataDir
$process = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden

try {
  if (-not $process.WaitForExit(15000)) {
    throw 'Security probe timed out'
  }
  if ($process.ExitCode -ne 73) {
    throw "Security probe expected ACL denial exit code 73, got $($process.ExitCode)"
  }
}
finally {
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
  }
  $after = Get-ProtectedAppDataSnapshot $AppDataDir
  if ($before -cne $after) {
    throw 'security probe modified protected app data'
  }
}

Write-Output 'security probe ok'
