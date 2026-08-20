<#
.SYNOPSIS
    APEX Velox 배포 패키지 생성 — 빌드 · 검사 · 압축 · 체크섬을 한 번에.

.DESCRIPTION
    v0.19 이전에는 이 과정이 손으로 이뤄져서 패키지 구성이 릴리스마다 달라졌다
    (어떤 ZIP 은 install.bat 이 있고 어떤 것은 LICENSE 가 들어가는 식).
    이 스크립트가 규격을 고정한다.

    ZIP 구조 (v0.10 부터의 규격을 유지):
        apex-velox-vX.Y.Z-win64/
          velox.exe · velox-server.exe · velox-app.exe
          README.txt
          install.bat                  (더블클릭용 얇은 래퍼)
          Install-ApexVelox.ps1        (실제 설치·제거)
          Update-ApexVelox.ps1         (확인·검증·롤백)
          LICENSE

    버전은 빌드된 바이너리에서 읽는다 — 인자로 받지 않는다(불일치 방지).

.PARAMETER SkipBuild
    이미 빌드된 target/release 를 그대로 쓴다.

.EXAMPLE
    .\scripts\New-ReleasePackage.ps1
#>
[CmdletBinding()]
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'

$Repo    = Split-Path $PSScriptRoot -Parent
$Release = Join-Path $Repo 'target\release'
$DistDir = Join-Path $Repo 'dist'

function Write-Step { param($m) Write-Host "  $m" -ForegroundColor Gray }
function Write-Ok   { param($m) Write-Host "  OK  $m" -ForegroundColor Green }

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Host "`n릴리스 빌드" -ForegroundColor Cyan
        cargo build --workspace --release --locked
        if ($LASTEXITCODE -ne 0) { throw '릴리스 빌드 실패' }
        Write-Ok '빌드 완료'
    }

    # --- 버전을 바이너리에서 확정 ---------------------------------------------
    $raw = & (Join-Path $Release 'velox.exe') --version
    if ($raw -notmatch '(\d+\.\d+\.\d+)') { throw "velox --version 을 해석할 수 없습니다: $raw" }
    $version = $Matches[1]
    $tag     = "v$version"
    Write-Ok "버전 $version (바이너리에서 확인)"

    # Cargo.toml 과 어긋나면 릴리스가 잘못된 것이다 — 여기서 잡는다.
    $manifest = (Select-String -Path (Join-Path $Repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
                 Select-Object -First 1).Matches.Groups[1].Value
    if ($manifest -ne $version) {
        throw "workspace 버전($manifest)과 바이너리 버전($version)이 다릅니다. 빌드를 다시 하세요."
    }

    # --- 스테이징 ---------------------------------------------------------------
    $name  = "apex-velox-$tag-win64"
    $stage = Join-Path $DistDir "_stage\$name"
    if (Test-Path (Join-Path $DistDir '_stage')) { Remove-Item (Join-Path $DistDir '_stage') -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    foreach ($b in @('velox.exe', 'velox-server.exe', 'velox-app.exe')) {
        Copy-Item (Join-Path $Release $b) $stage -Force
    }
    Copy-Item (Join-Path $Repo 'LICENSE') $stage -Force
    Copy-Item (Join-Path $PSScriptRoot 'Install-ApexVelox.ps1') $stage -Force
    Copy-Item (Join-Path $PSScriptRoot 'Update-ApexVelox.ps1')  $stage -Force

    # install.bat — PowerShell 스크립트를 부르는 얇은 래퍼(더블클릭 편의).
    @"
@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-ApexVelox.ps1"
pause
"@ | Set-Content (Join-Path $stage 'install.bat') -Encoding ascii

    $readme = Join-Path $Repo 'dist\README.txt'
    if (Test-Path $readme) { Copy-Item $readme $stage -Force }
    Write-Ok "스테이징 ($((Get-ChildItem $stage).Count)개 파일)"

    # --- 안전 검사 ---------------------------------------------------------------
    & (Join-Path $PSScriptRoot 'Test-ReleaseSafety.ps1') -ArtifactPath $stage
    if ($LASTEXITCODE -ne 0) { throw '패키지 안전 검사 실패' }

    # --- 압축 + 체크섬 -----------------------------------------------------------
    $zip = Join-Path $DistDir "$name.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    "$hash  $name.zip" | Set-Content "$zip.sha256" -Encoding ascii

    Remove-Item (Join-Path $DistDir '_stage') -Recurse -Force

    Write-Ok "$name.zip  ($([math]::Round((Get-Item $zip).Length/1MB,2)) MB)"
    Write-Step "sha256 $hash"
    Write-Host "`n패키지 준비 완료: $zip`n" -ForegroundColor Cyan
}
finally {
    Pop-Location
}
