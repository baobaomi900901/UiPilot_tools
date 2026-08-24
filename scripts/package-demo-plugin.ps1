[CmdletBinding()]
param(
    [ValidateSet('com.uipilot.demo-win', 'com.uipilot.demo-return')]
    [string]$PluginId = 'com.uipilot.demo-win',
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$workspace = Split-Path -Parent $PSScriptRoot
$exampleRoot = Join-Path $workspace "examples/public-plugins/$PluginId"
$packageRoot = Join-Path $exampleRoot 'package'
if (-not $OutputPath) {
    $OutputPath = Join-Path $exampleRoot "$PluginId.uipilot-plugin"
}
$output = [System.IO.Path]::GetFullPath($OutputPath)
if ([System.IO.Path]::GetExtension($output) -ne '.uipilot-plugin') {
    throw 'OutputPath must end in .uipilot-plugin'
}
$parent = Split-Path -Parent $output
[System.IO.Directory]::CreateDirectory($parent) | Out-Null
$temporary = Join-Path $parent ('.' + [System.IO.Path]::GetFileName($output) + '.' + [Guid]::NewGuid().ToString('N') + '.tmp')

try {
    $stream = [System.IO.File]::Open(
        $temporary,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $archive = New-Object -TypeName System.IO.Compression.ZipArchive -ArgumentList @(
            $stream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $false
        )
        try {
            $files = @(Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Sort-Object FullName)
            foreach ($file in $files) {
                $relative = $file.FullName.Substring($packageRoot.Length + 1).Replace('\', '/')
                $entry = $archive.CreateEntry(
                    $relative,
                    [System.IO.Compression.CompressionLevel]::Optimal
                )
                $input = $file.OpenRead()
                $outputStream = $entry.Open()
                try {
                    $input.CopyTo($outputStream)
                } finally {
                    $outputStream.Dispose()
                    $input.Dispose()
                }
            }
        } finally {
            $archive.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
    if ([System.IO.File]::Exists($output)) {
        $backup = $output + '.' + [Guid]::NewGuid().ToString('N') + '.bak'
        [System.IO.File]::Replace($temporary, $output, $backup, $true)
        [System.IO.File]::Delete($backup)
    } else {
        [System.IO.File]::Move($temporary, $output)
    }
} finally {
    if ([System.IO.File]::Exists($temporary)) {
        [System.IO.File]::Delete($temporary)
    }
}

Write-Output $output
