<#
.SYNOPSIS
    APEX Velox 업데이터 — 확인 · 검증 · 롤백 가능한 설치.

.DESCRIPTION
    v0.19 Updater 요구사항 구현:
      - GitHub releases/latest 에서 최신 버전 확인
      - 다운로드 링크가 **실제 asset** 을 가리키는지 확인(추측 URL 금지)
      - SHA256 checksum 검증 후에만 설치
      - 실행 중 바이너리 교체 금지
      - 실패 시 **이전 버전으로 롤백**

    설치 자체는 Install-ApexVelox.ps1 에 위임한다(설치 로직을 한 곳에만 둔다).

.PARAMETER CheckOnly
    확인만 하고 설치하지 않는다.

.PARAMETER Force
    같은 버전이어도 다시 설치한다.

.EXAMPLE
    .\Update-ApexVelox.ps1 -CheckOnly
    .\Update-ApexVelox.ps1
#>
[CmdletBinding()]
param(
    [switch]$CheckOnly,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$Repo       = 'edwardtklim/apex-chorus'
$InstallDir = Join-Path $env:LOCALAPPDATA 'ApexVelox'
$WorkRoot   = Join-Path $env:TEMP 'apex-velox-update'

function Write-Step { param($m) Write-Host "  $m" -ForegroundColor Gray }
function Write-Ok   { param($m) Write-Host "  OK  $m" -ForegroundColor Green }
function Write-Warn { param($m) Write-Host "  !   $m" -ForegroundColor Yellow }

function Get-InstalledVersion {
    $exe = Join-Path $InstallDir 'velox.exe'
    if (-not (Test-Path $exe)) { return $null }
    try {
        $raw = & $exe --version 2>$null
        if ($raw -match '(\d+\.\d+\.\d+)') { return [version]$Matches[1] }
    } catch { }
    return $null
}

function Assert-NotRunning {
    $running = Get-Process -Name 'velox', 'velox-server', 'velox-app' -ErrorAction SilentlyContinue
    if ($running) {
        $list = ($running | Select-Object -ExpandProperty ProcessName -Unique) -join ', '
        throw "실행 중인 APEX 프로세스가 있습니다: $list`n" +
              "  다음 행동: APEX 창을 모두 닫고 다시 실행하세요."
    }
}

Write-Host "`nAPEX Velox 업데이트 확인" -ForegroundColor Cyan

$installed = Get-InstalledVersion
if ($installed) { Write-Step "설치된 버전: $installed" }
else            { Write-Warn "설치된 버전을 확인할 수 없습니다 ($InstallDir)" }

# --- 최신 릴리스 조회 ----------------------------------------------------------
try {
    $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
                             -Headers @{ 'User-Agent' = 'apex-velox-updater' } -TimeoutSec 20
} catch {
    throw "릴리스 정보를 가져오지 못했습니다: $($_.Exception.Message)`n" +
          "  다음 행동: 인터넷 연결을 확인하거나 잠시 후 다시 시도하세요.`n" +
          "  수동 확인: https://github.com/$Repo/releases/latest"
}

if ($rel.tag_name -notmatch '(\d+\.\d+\.\d+)') {
    throw "릴리스 태그 형식을 해석할 수 없습니다: $($rel.tag_name)"
}
$latest = [version]$Matches[1]
Write-Step "최신 버전:   $latest  ($($rel.tag_name))"

if ($installed -and $latest -le $installed -and -not $Force) {
    Write-Host "`n이미 최신입니다.`n" -ForegroundColor Cyan
    return
}

# --- asset 확인 — URL 을 추측하지 않고 릴리스가 실제로 가진 것만 쓴다 -----------
$zipAsset = $rel.assets | Where-Object { $_.name -like '*win64.zip' } | Select-Object -First 1
if (-not $zipAsset) {
    throw "이 릴리스에 win64 ZIP asset 이 없습니다.`n" +
          "  다음 행동: https://github.com/$Repo/releases/latest 에서 직접 받으세요."
}
$shaAsset = $rel.assets | Where-Object { $_.name -eq "$($zipAsset.name).sha256" } | Select-Object -First 1

Write-Step "asset: $($zipAsset.name)  ($([math]::Round($zipAsset.size/1MB,2)) MB)"
if (-not $shaAsset) { Write-Warn 'checksum asset 이 없습니다 — 무결성 검증 없이 진행하지 않습니다' }

if ($CheckOnly) {
    Write-Host "`n업데이트 가능: $installed -> $latest" -ForegroundColor Yellow
    Write-Step "설치하려면 -CheckOnly 없이 다시 실행하세요."
    Write-Host ""
    return
}

if (-not $shaAsset) {
    throw "checksum 이 없어 설치를 중단합니다.`n" +
          "  이유: 검증하지 않은 바이너리를 자동 설치하지 않는 것이 APEX 원칙입니다.`n" +
          "  다음 행동: 릴리스 페이지에서 직접 받아 수동 설치하세요."
}

Assert-NotRunning

# --- 다운로드 + 검증 ------------------------------------------------------------
if (Test-Path $WorkRoot) { Remove-Item $WorkRoot -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkRoot | Out-Null

$zipPath = Join-Path $WorkRoot $zipAsset.name
$shaPath = "$zipPath.sha256"
Write-Step '다운로드 중...'
Invoke-WebRequest -Uri $zipAsset.browser_download_url -OutFile $zipPath -TimeoutSec 300
Invoke-WebRequest -Uri $shaAsset.browser_download_url -OutFile $shaPath -TimeoutSec 60

$expected = (Get-Content $shaPath -Raw).Trim().Split(' ')[0].ToLower()
$actual   = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
if ($expected -ne $actual) {
    throw "checksum 불일치 — 설치를 중단합니다.`n" +
          "  기대: $expected`n  실제: $actual`n" +
          "  다음 행동: 네트워크 문제이거나 파일이 변조됐습니다. 다시 시도하세요."
}
Write-Ok 'checksum 검증 통과'

$extract = Join-Path $WorkRoot 'extracted'
Expand-Archive -Path $zipPath -DestinationPath $extract -Force
# ZIP 안에 루트 폴더가 하나 있는 규격이므로 실제 파일 위치를 찾는다.
$payload = Get-ChildItem $extract -Directory | Select-Object -First 1
if (-not $payload) { $payload = Get-Item $extract }

# --- 백업 → 설치 → 실패 시 롤백 -------------------------------------------------
$backup = Join-Path $WorkRoot 'backup'
if (Test-Path $InstallDir) {
    Copy-Item $InstallDir $backup -Recurse -Force
    Write-Ok "이전 버전 백업 ($backup)"
}

$installer = Join-Path $payload.FullName 'Install-ApexVelox.ps1'
if (-not (Test-Path $installer)) { $installer = Join-Path $PSScriptRoot 'Install-ApexVelox.ps1' }

try {
    & $installer -Source $payload.FullName
    $new = Get-InstalledVersion
    if (-not $new) { throw '설치 후 버전을 확인할 수 없습니다' }
    Write-Ok "업데이트 완료: $installed -> $new"
} catch {
    Write-Warn "설치 실패 — 롤백합니다: $($_.Exception.Message)"
    if (Test-Path $backup) {
        if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
        Copy-Item $backup $InstallDir -Recurse -Force
        Write-Ok '이전 버전으로 롤백했습니다'
    } else {
        Write-Warn '백업이 없어 롤백하지 못했습니다 — 릴리스 페이지에서 수동 설치하세요'
    }
    throw
}

Remove-Item $WorkRoot -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ""
