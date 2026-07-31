[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ServerPath
)

$ErrorActionPreference = 'Stop'

$resolvedServer = (Resolve-Path -LiteralPath $ServerPath).Path
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$testDir = Join-Path $tempRoot ("apex-project-api-" + [Guid]::NewGuid().ToString('N'))
$projectDir = Join-Path $testDir 'sample-project'
New-Item -ItemType Directory -Path (Join-Path $projectDir 'src') -Force | Out-Null
$manifest = "[package]`nname = `"sample`"`nversion = `"0.1.0`"`n"
$source = "fn main() { /* TODO: verify */ }`n"
Set-Content -LiteralPath (Join-Path $projectDir 'Cargo.toml') -Value $manifest -NoNewline
Set-Content -LiteralPath (Join-Path $projectDir 'src/main.rs') -Value $source -NoNewline
Set-Content -LiteralPath (Join-Path $projectDir '.env') -Value 'OPENAI_API_KEY=forbidden-secret' -NoNewline

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

    $body = @{ path = $projectDir } | ConvertTo-Json -Compress
    $unauthorized = $null
    try {
        Invoke-WebRequest -UseBasicParsing -Method Post -Uri "$baseUrl/project/scan" `
            -ContentType 'application/json' -Body $body | Out-Null
    } catch {
        $unauthorized = $_.Exception.Response.StatusCode.value__
    }
    if ($unauthorized -ne 401) {
        throw "Unauthenticated Project API request must return 401, got $unauthorized."
    }

    $session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    $session.Cookies.Add(
        (New-Object System.Net.Cookie('apex_session', $token, '/', '127.0.0.1'))
    )
    $result = Invoke-RestMethod -Method Post -Uri "$baseUrl/project/scan" `
        -WebSession $session -ContentType 'application/json' -Body $body
    $json = $result | ConvertTo-Json -Depth 20 -Compress

    if (-not $result.safety.read_only -or $result.safety.cloud_sent) {
        throw 'Project API safety contract is invalid.'
    }
    if ($result.safety.writes_performed -or $result.safety.commands_executed) {
        throw 'Project API reported a side effect.'
    }
    if ($result.scan.files.Count -lt 1) {
        throw 'Project API did not return scanned files.'
    }
    foreach ($forbidden in @($projectDir, '.env', 'forbidden-secret')) {
        if ($json.Contains($forbidden)) {
            throw "Project API leaked forbidden data: $forbidden"
        }
    }
    if ((Get-Content -LiteralPath (Join-Path $projectDir 'Cargo.toml') -Raw) -ne $manifest) {
        throw 'Project scan changed Cargo.toml.'
    }
    if ((Get-Content -LiteralPath (Join-Path $projectDir 'src/main.rs') -Raw) -ne $source) {
        throw 'Project scan changed source code.'
    }

    Write-Output 'Project API contract test passed.'
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
