param([Parameter(Mandatory=$true)][string]$RequestPath)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'package-runtime.ps1')
$request=$null
try {
    $request=Read-PackageJson $RequestPath
    $root=Split-Path -Parent $PSScriptRoot
    if ([IO.Path]::GetFullPath($request.Root) -cne $root) { throw 'Request repository does not match this worker.' }
    $null=Assert-PackageDirectoryPath $request.RunDirectory
    if ((Split-Path -Parent ([IO.Path]::GetFullPath($RequestPath))) -cne $request.RunDirectory) { throw 'Request must be inside its run directory.' }
    Set-Location -LiteralPath $root
    Clear-PackageBuildEnvironment
    Test-PackageCancelled $request
    Set-PackageStage $request 'Preflight'
    $tools=Get-PackageTools $root $request.Mode
    $result=Invoke-PackageBuild $request $tools
    Test-PackageCancelled $request
    Write-PackageJson (Join-Path $request.RunDirectory 'result.json') $result
    exit 0
} catch {
    if ($null -ne $request -and (Test-Path -LiteralPath $request.RunDirectory -PathType Container)) {
        $state=if (Test-Path -LiteralPath (Join-Path $request.RunDirectory 'cancel.flag')) { 'Cancelled' } else { 'Failed' }
        Write-PackageLog $request $_.Exception.Message
        Write-PackageJson (Join-Path $request.RunDirectory 'result.json') @{SchemaVersion=1;RunId=$request.RunId;State=$state;Message=$_.Exception.Message}
    }
    exit 1
}
