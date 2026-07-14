[CmdletBinding()]
param(
    [string]$Port = 'COM9',
    [string]$TargetIp
)

$ErrorActionPreference = 'Stop'
$provision = Join-Path $PSScriptRoot '..\firmware\esp32-csi-node\provision.py'

if (-not $TargetIp) {
    $TargetIp = (Get-NetIPAddress -InterfaceAlias 'Wi-Fi' -AddressFamily IPv4 |
        Where-Object IPAddress -NotLike '169.254.*' |
        Select-Object -First 1).IPAddress
}

$ssid = Read-Host 'Wi-Fi ag adi (SSID)'
$securePassword = Read-Host 'Wi-Fi sifresi' -AsSecureString
$passwordPtr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)

try {
    $password = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPtr)

    & python $provision `
        --port $Port `
        --chip esp32s3 `
        --ssid $ssid `
        --password $password `
        --target-ip $TargetIp `
        --target-port 5005

    if ($LASTEXITCODE -ne 0) {
        throw "Wi-Fi ayarlari yazilamadi (kod: $LASTEXITCODE)."
    }

    Write-Host "Tamam. ESP32 $ssid agina baglanacak; CSI hedefi $TargetIp`:5005."
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPtr)
    $password = $null
}
