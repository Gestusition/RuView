[CmdletBinding()]
param(
    [ValidateSet('esp32', 'auto', 'wifi', 'simulate', 'simulated')]
    [string]$Source = 'esp32',

    [ValidateSet('observatory', 'dashboard', 'pose-fusion', 'viz')]
    [string]$Page = 'observatory',

    [ValidateRange(1, 65535)]
    [int]$HttpPort = 8080,

    [ValidateRange(1, 65535)]
    [int]$WsPort = 8765,

    [ValidateRange(1, 65535)]
    [int]$UdpPort = 5005,

    [switch]$Rebuild,
    [switch]$NoBrowser,
    [switch]$SkipFirewall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$v2Root = Join-Path $repoRoot 'v2'
$uiRoot = Join-Path $repoRoot 'ui'
$serverExe = Join-Path $v2Root 'target\debug\sensing-server.exe'
$healthUrl = "http://127.0.0.1:$HttpPort/health"
$pagePath = switch ($Page) {
    'dashboard' { 'index.html' }
    'observatory' { 'observatory.html' }
    'pose-fusion' { 'pose-fusion.html' }
    'viz' { 'viz.html' }
}
$pageUrl = "http://127.0.0.1:$HttpPort/ui/$pagePath"
$firewallRuleName = "RuView Sensing Server UDP $UdpPort (Private)"

function Test-DesiredSource {
    param([string]$ActualSource)

    if ($Source -eq 'auto') { return $true }
    if ($Source -in @('simulate', 'simulated')) {
        return $ActualSource.StartsWith('simulat', [StringComparison]::OrdinalIgnoreCase)
    }
    return $ActualSource.StartsWith($Source, [StringComparison]::OrdinalIgnoreCase)
}

function Get-HealthyServer {
    try {
        $health = Invoke-RestMethod -Uri $healthUrl -TimeoutSec 2
        if ($health.status -eq 'ok' -and (Test-DesiredSource ([string]$health.source))) {
            return $health
        }
    }
    catch {
        return $null
    }
    return $null
}

function Ensure-FirewallRule {
    if ($SkipFirewall -or $Source -ne 'esp32') { return }

    $isCorrect = $false
    try {
        $rule = Get-NetFirewallRule -DisplayName $firewallRuleName -ErrorAction Stop
        $portFilter = $rule | Get-NetFirewallPortFilter
        $appFilter = $rule | Get-NetFirewallApplicationFilter
        $isCorrect = (
            $rule.Enabled -eq 'True' -and
            $rule.Action -eq 'Allow' -and
            [string]$rule.Profile -match 'Private' -and
            $portFilter.Protocol -eq 'UDP' -and
            [int]$portFilter.LocalPort -eq $UdpPort -and
            $appFilter.Program -eq $serverExe
        )
    }
    catch {
        $isCorrect = $false
    }

    if ($isCorrect) { return }

    Write-Host "Configuring Windows Firewall for ESP32 CSI UDP $UdpPort..."
    Write-Host 'Windows may ask for administrator approval.'

    $escapedName = $firewallRuleName.Replace("'", "''")
    $escapedExe = $serverExe.Replace("'", "''")
    $elevatedScript = @"
`$ErrorActionPreference = 'Stop'
`$existing = Get-NetFirewallRule -DisplayName '$escapedName' -ErrorAction SilentlyContinue
if (`$existing) { Remove-NetFirewallRule -DisplayName '$escapedName' }
New-NetFirewallRule -DisplayName '$escapedName' -Description 'Allow ESP32 CSI UDP ingestion for local RuView sensing server.' -Direction Inbound -Program '$escapedExe' -Protocol UDP -LocalPort $UdpPort -Profile Private -Action Allow -Enabled True | Out-Null
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($elevatedScript))
    $admin = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
        '-NoProfile', '-EncodedCommand', $encoded
    ) -Verb RunAs -WindowStyle Hidden -Wait -PassThru

    if ($admin.ExitCode -ne 0) {
        throw "Windows Firewall configuration failed with exit code $($admin.ExitCode)."
    }
}

if ($Rebuild -or -not (Test-Path -LiteralPath $serverExe -PathType Leaf)) {
    Write-Host 'Building wifi-densepose-sensing-server...'
    Push-Location $v2Root
    try {
        & cargo build -p wifi-densepose-sensing-server --bin sensing-server --no-default-features
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo build failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}

Ensure-FirewallRule

$health = Get-HealthyServer
if (-not $health) {
    $listener = Get-NetTCPConnection -State Listen -LocalPort $HttpPort -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($listener) {
        $owner = Get-Process -Id $listener.OwningProcess -ErrorAction Stop
        if ($owner.Path -ne $serverExe) {
            throw "HTTP port $HttpPort is already used by $($owner.ProcessName) (PID $($owner.Id))."
        }
        Write-Host "Restarting RuView server with source '$Source'..."
        Stop-Process -Id $owner.Id -Force
        Start-Sleep -Milliseconds 500
    }

    $logDir = Join-Path $env:LOCALAPPDATA 'RuView\logs'
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    $stdoutLog = Join-Path $logDir "sensing-server-$HttpPort.log"
    $stderrLog = Join-Path $logDir "sensing-server-$HttpPort.error.log"
    Remove-Item -LiteralPath $stdoutLog, $stderrLog -Force -ErrorAction SilentlyContinue

    Write-Host "Starting RuView sensing server (source=$Source)..."
    $server = Start-Process -FilePath $serverExe -ArgumentList @(
        '--http-port', [string]$HttpPort,
        '--ws-port', [string]$WsPort,
        '--udp-port', [string]$UdpPort,
        '--source', $Source,
        '--ui-path', $uiRoot
    ) -WorkingDirectory $v2Root -WindowStyle Hidden -RedirectStandardOutput $stdoutLog `
        -RedirectStandardError $stderrLog -PassThru

    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 500
        if ($server.HasExited) {
            $errorTail = if (Test-Path -LiteralPath $stderrLog) {
                (Get-Content -LiteralPath $stderrLog -Tail 30) -join [Environment]::NewLine
            } else {
                'No error log was produced.'
            }
            throw "RuView server exited before becoming ready.$([Environment]::NewLine)$errorTail"
        }
        $health = Get-HealthyServer
    } while (-not $health -and [DateTime]::UtcNow -lt $deadline)

    if (-not $health) {
        throw "RuView server did not become healthy within 30 seconds. Logs: $logDir"
    }
}

Write-Host "RuView is ready: source=$($health.source), tick=$($health.tick)"
Write-Host "Observatory: $pageUrl"

if (-not $NoBrowser) {
    Start-Process $pageUrl
}
