[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string] $Executable
)

$ErrorActionPreference = 'Stop'
$executableUri = $null
if (
  -not [Uri]::TryCreate($Executable, [UriKind]::Absolute, [ref] $executableUri) -or
  -not $executableUri.IsFile
) {
  throw 'Executable must be an absolute path'
}
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$process = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden

try {
  if (-not $process.WaitForExit(15000)) {
    throw 'Security probe timed out'
  }
  if ($process.ExitCode -ne 0) {
    throw "Security probe failed with exit code $($process.ExitCode)"
  }
}
finally {
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
  }
}

Write-Output 'security probe ok'
