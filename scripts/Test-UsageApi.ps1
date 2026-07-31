[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ServerPath
)

$ErrorActionPreference = 'Stop'

$resolvedServer = (Resolve-Path -LiteralPath $ServerPath).Path
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$testDir = Join-Path $tempRoot ("apex-usage-api-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDir | Out-Null

# Reserve an available loopback port, then immediately release it for the test
# server. This avoids hard-coded ports on shared CI runners.
$probe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$probe.Start()
$port = ([Net.IPEndPoint]$probe.LocalEndpoint).Port
$probe.Stop()

$oldAddress = $env:VELOX_ADDR
$oldToken = $env:VELOX_SESSION_TOKEN
$token = "contract-" + [Guid]::NewGuid().ToString('N')
$env:VELOX_ADDR = "127.0.0.1:$port"
$env:VELOX_SESSION_TOKEN = $token
$server = $null

try {
    $server = Start-Process `
        -FilePath $resolvedServer `
        -WorkingDirectory $testDir `
        -RedirectStandardOutput (Join-Path $testDir 'stdout.log') `
        -RedirectStandardError (Join-Path $testDir 'stderr.log') `
        -WindowStyle Hidden `
        -PassThru

    $baseUrl = "http://127.0.0.1:$port"
    $ready = $false
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        Start-Sleep -Milliseconds 200
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/" -TimeoutSec 1 | Out-Null
            $ready = $true
            break
        } catch {
            if ($server.HasExited) {
                throw "velox-server exited before becoming ready (code $($server.ExitCode))."
            }
        }
    }
    if (-not $ready) {
        throw 'velox-server did not become ready within 10 seconds.'
    }

    $unauthorized = $null
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/usage/summary?period=month" |
            Out-Null
    } catch {
        $unauthorized = $_.Exception.Response.StatusCode.value__
    }
    if ($unauthorized -ne 401) {
        throw "Unauthenticated Usage API request must return 401, got $unauthorized."
    }

    $session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    $session.Cookies.Add(
        (New-Object System.Net.Cookie('apex_session', $token, '/', '127.0.0.1'))
    )
    $summary = Invoke-RestMethod `
        -Uri "$baseUrl/usage/summary?period=month" `
        -WebSession $session
    if ($summary.cost.display -ne 'unknown') {
        throw 'An empty, unconfigured ledger must report cost as unknown.'
    }
    if ($summary.notice -notmatch 'Not subscription billing') {
        throw 'Usage API must include the subscription/balance disclaimer.'
    }

    $recording = Invoke-RestMethod `
        -Method Post `
        -Uri "$baseUrl/usage/recording" `
        -WebSession $session `
        -ContentType 'application/json' `
        -Body '{"enabled":false}'
    if (-not $recording.ok -or $recording.enabled) {
        throw 'Usage recording endpoint returned an invalid response.'
    }

    $clear = Invoke-RestMethod `
        -Method Delete `
        -Uri "$baseUrl/usage/records" `
        -WebSession $session
    if (-not $clear.ok -or $clear.removed -ne 0) {
        throw 'Usage clear endpoint returned an invalid response.'
    }

    Write-Output 'Usage API contract test passed.'
} finally {
    if ($server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force
        $server.WaitForExit()
    }

    if ($null -eq $oldAddress) {
        Remove-Item Env:VELOX_ADDR -ErrorAction SilentlyContinue
    } else {
        $env:VELOX_ADDR = $oldAddress
    }
    if ($null -eq $oldToken) {
        Remove-Item Env:VELOX_SESSION_TOKEN -ErrorAction SilentlyContinue
    } else {
        $env:VELOX_SESSION_TOKEN = $oldToken
    }

    $resolvedTestDir = [IO.Path]::GetFullPath($testDir)
    if (-not $resolvedTestDir.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean unexpected test path: $resolvedTestDir"
    }
    Remove-Item -LiteralPath $resolvedTestDir -Recurse -Force
}
