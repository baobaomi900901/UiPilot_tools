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
