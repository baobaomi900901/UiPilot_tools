Set-StrictMode -Version Latest

function Test-ProtectedEverythingServicePath {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$ExecutablePath,

        [Parameter(Mandatory = $true)]
        [string[]]$ProgramFilesRoots
    )

    if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
        return $false
    }

    try {
        $candidate = [IO.Path]::GetFullPath($ExecutablePath)
    }
    catch {
        return $false
    }

    foreach ($root in $ProgramFilesRoots) {
        if ([string]::IsNullOrWhiteSpace($root)) {
            continue
        }
        try {
            $boundary = [IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
        }
        catch {
            continue
        }
        if ($candidate.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }

    return $false
}

function Get-WindowsServiceExecutablePath {
    param(
        [AllowEmptyString()]
        [string]$CommandLine
    )

    if ([string]::IsNullOrWhiteSpace($CommandLine)) {
        return $null
    }
    $trimmed = $CommandLine.Trim()
    if ($trimmed[0] -eq '"') {
        $closingQuote = $trimmed.IndexOf('"', 1)
        if ($closingQuote -le 1) {
            return $null
        }
        return $trimmed.Substring(1, $closingQuote - 1)
    }
    $separator = $trimmed.IndexOf(' ')
    if ($separator -lt 0) {
        return $trimmed
    }
    return $trimmed.Substring(0, $separator)
}

function Select-EverythingUserClient {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$Processes,

        [Parameter(Mandatory = $true)]
        [int]$SessionId
    )

    $candidates = @($Processes | Where-Object {
        $_.ProcessName -ceq 'Everything' -and $_.SessionId -eq $SessionId
    } | Sort-Object Id | Select-Object -First 1)
    if ($candidates.Count -eq 0) {
        return $null
    }
    return $candidates[0]
}

function Get-EverythingServiceFailure {
    param(
        [AllowNull()]
        [object]$Service,

        [AllowNull()]
        [object]$ServiceProcess,

        [Parameter(Mandatory = $true)]
        [string[]]$ProgramFilesRoots
    )

    if ($null -eq $Service) {
        return 'EVERYTHING_SERVICE_MISSING'
    }
    if ([string]$Service.State -cne 'Running' -or [uint32]$Service.ProcessId -eq 0) {
        return 'EVERYTHING_SERVICE_NOT_RUNNING'
    }
    if ($null -eq $ServiceProcess -or [uint32]$ServiceProcess.ProcessId -ne [uint32]$Service.ProcessId) {
        return 'EVERYTHING_SERVICE_PROCESS_UNAVAILABLE'
    }

    $executablePath = [string]$ServiceProcess.ExecutablePath
    if ([string]::IsNullOrWhiteSpace($executablePath)) {
        $pathNameProperty = $Service.PSObject.Properties['PathName']
        if ($null -eq $pathNameProperty) {
            return 'EVERYTHING_SERVICE_PATH_UNSAFE'
        }
        $executablePath = Get-WindowsServiceExecutablePath ([string]$pathNameProperty.Value)
    }
    if (-not (Test-ProtectedEverythingServicePath $executablePath $ProgramFilesRoots)) {
        return 'EVERYTHING_SERVICE_PATH_UNSAFE'
    }
    return $null
}
