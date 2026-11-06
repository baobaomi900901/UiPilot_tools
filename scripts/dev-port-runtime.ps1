$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-UiPilotTcpPortAvailable {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 65535)]
        [int]$Port
    )

    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
        return $true
    }
    catch [System.Net.Sockets.SocketException] {
        return $false
    }
    finally {
        $listener.Stop()
    }
}

function Get-UiPilotAvailableDevPort {
    param(
        [ValidateRange(1, 65535)]
        [int]$StartPort = 14321,

        [ValidateRange(1, 65535)]
        [int]$EndPort = 65535
    )

    if ($EndPort -lt $StartPort) {
        throw 'EndPort must be greater than or equal to StartPort.'
    }

    for ($port = $StartPort; $port -le $EndPort; $port++) {
        if (Test-UiPilotTcpPortAvailable -Port $port) {
            return $port
        }
    }

    throw "No available development port was found between $StartPort and $EndPort."
}

function New-UiPilotDevConfigOverride {
    param(
        [Parameter(Mandatory = $true)] [string]$RepositoryRoot,
        [Parameter(Mandatory = $true)] [int]$Port,
        [Parameter(Mandatory = $true)] [string]$ProviderExe
    )

    # Tauri replaces resource arrays when merging the Windows configuration.
    $windows = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'src-tauri/tauri.windows.conf.json') -Raw | ConvertFrom-Json
    $resources = @{}
    foreach ($resource in $windows.bundle.resources) {
        if ($resource -notlike 'resources/provider/*') { $resources[$resource] = $resource }
    }
    $resources[$ProviderExe] = 'resources/provider/UIAutoma.UIAutomation.Provider.exe'
    $manifest = Join-Path (Split-Path -Parent $ProviderExe) 'asset-manifest.json'
    $resources[$manifest] = 'resources/provider/asset-manifest.json'
    return @{
        build = @{ devUrl = "http://127.0.0.1:$Port" }
        bundle = @{ resources = $resources }
    }
}
