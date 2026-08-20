<#
.SYNOPSIS
    APEX Velox 설치 관리자.

.DESCRIPTION
    v0.19 Installer 요구사항 구현:
      - %LOCALAPPDATA%\ApexVelox 에 설치
      - 시작 메뉴 + 바탕화면 바로가기
      - checksum 검증(.sha256 이 있으면 반드시 통과해야 진행)
      - 실행 중 바이너리 교체 금지 (프로세스가 살아 있으면 중단)
      - 제어판 "프로그램 및 기능" 등록 → 정상적인 제거 경로 제공
      - 사용자 데이터(%LOCALAPPDATA%\APEX\Velox)는 건드리지 않는다

    관리자 권한이 필요 없다 — 전부 사용자 프로필 안에서 끝난다.

.PARAMETER Source
    설치할 파일이 있는 폴더. 기본값은 이 스크립트가 있는 폴더.

.PARAMETER Uninstall
    설치 제거. 사용자 데이터는 기본적으로 남긴다(-PurgeData 로 삭제).

.PARAMETER PurgeData
    제거 시 사용자 데이터(정책·키 설정·로그·장부)까지 삭제.

.EXAMPLE
    .\Install-ApexVelox.ps1
    .\Install-ApexVelox.ps1 -Uninstall
    .\Install-ApexVelox.ps1 -Uninstall -PurgeData
#>
[CmdletBinding()]
param(
    [string]$Source,
    [switch]$Uninstall,
    [switch]$PurgeData
)

$ErrorActionPreference = 'Stop'

$AppName    = 'APEX Velox'
$InstallDir = Join-Path $env:LOCALAPPDATA 'ApexVelox'
$DataDir    = Join-Path $env:LOCALAPPDATA 'APEX\Velox'
$RegKey     = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ApexVelox'
$Binaries   = @('velox.exe', 'velox-server.exe', 'velox-app.exe')

function Write-Step { param($m) Write-Host "  $m" -ForegroundColor Gray }
function Write-Ok   { param($m) Write-Host "  OK  $m" -ForegroundColor Green }
function Write-Warn { param($m) Write-Host "  !   $m" -ForegroundColor Yellow }

# --- 실행 중인 프로세스 확인 --------------------------------------------------
# 실행 중 바이너리 교체는 조용히 실패하거나 반쯤 갱신된 설치를 만든다. 아예 막는다.
function Assert-NotRunning {
    $names = @('velox', 'velox-server', 'velox-app')
    $running = Get-Process -Name $names -ErrorAction SilentlyContinue
    if ($running) {
        $list = ($running | Select-Object -ExpandProperty ProcessName -Unique) -join ', '
        throw "실행 중인 APEX 프로세스가 있습니다: $list`n" +
              "  다음 행동: APEX 창을 모두 닫고 다시 실행하세요.`n" +
              "  강제로 종료하려면: Stop-Process -Name $($names -join ',') -Force"
    }
}

# --- checksum 검증 ------------------------------------------------------------
function Test-Checksum {
    param([string]$Dir)
    $shaFiles = Get-ChildItem -Path $Dir -Filter '*.sha256' -ErrorAction SilentlyContinue
    if (-not $shaFiles) {
        Write-Warn 'checksum 파일이 없어 무결성 검증을 건너뜁니다'
        return
    }
    foreach ($s in $shaFiles) {
        $expected = (Get-Content $s.FullName -Raw).Trim().Split(' ')[0].ToLower()
        $target   = Join-Path $Dir ($s.BaseName)   # foo.zip.sha256 -> foo.zip
        if (-not (Test-Path $target)) { continue }
        $actual = (Get-FileHash $target -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) {
            throw "checksum 불일치: $($s.BaseName)`n" +
                  "  기대: $expected`n  실제: $actual`n" +
                  "  다음 행동: 파일이 손상됐거나 변조됐습니다. 다시 내려받으세요."
        }
        Write-Ok "checksum 검증: $($s.BaseName)"
    }
}

# --- 바로가기 ------------------------------------------------------------------
function New-Shortcut {
    param([string]$Path, [string]$Target, [string]$WorkDir)
    $ws = New-Object -ComObject WScript.Shell
    $sc = $ws.CreateShortcut($Path)
    $sc.TargetPath       = $Target
    $sc.WorkingDirectory = $WorkDir
    $sc.Description      = 'APEX Velox — system health and AI development tool'
    $sc.Save()
}

# --- 제거 ---------------------------------------------------------------------
function Invoke-Uninstall {
    Write-Host "`n$AppName 제거" -ForegroundColor Cyan
    Assert-NotRunning

    $desktop   = Join-Path ([Environment]::GetFolderPath('Desktop')) 'APEX Velox.lnk'
    $startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'APEX Velox.lnk'
    foreach ($lnk in @($desktop, $startMenu)) {
        if (Test-Path $lnk) { Remove-Item $lnk -Force; Write-Ok "바로가기 제거: $lnk" }
    }

    if (Test-Path $InstallDir) {
        Remove-Item $InstallDir -Recurse -Force
        Write-Ok "프로그램 파일 제거: $InstallDir"
    }

    if (Test-Path $RegKey) { Remove-Item $RegKey -Recurse -Force; Write-Ok '제어판 등록 해제' }

    if ($PurgeData) {
        if (Test-Path $DataDir) {
            Remove-Item $DataDir -Recurse -Force
            Write-Ok "사용자 데이터 삭제: $DataDir"
        }
        Write-Warn 'API 키는 Windows 자격증명 관리자에 남아 있습니다.'
        Write-Warn '완전히 지우려면 설치 전에 velox chorus revoke / 자격증명 관리자에서 직접 삭제하세요.'
    } else {
        Write-Ok "사용자 데이터 보존: $DataDir"
        Write-Step '(완전히 지우려면 -PurgeData 옵션을 쓰세요)'
    }

    Write-Host "`n제거 완료.`n" -ForegroundColor Cyan
}

# --- 설치 ---------------------------------------------------------------------
function Invoke-Install {
    if (-not $Source) { $Source = $PSScriptRoot }
    if (-not $Source) { $Source = (Get-Location).Path }

    Write-Host "`n$AppName 설치" -ForegroundColor Cyan
    Write-Step "원본: $Source"
    Write-Step "대상: $InstallDir"

    foreach ($b in $Binaries) {
        if (-not (Test-Path (Join-Path $Source $b))) {
            throw "설치 파일을 찾을 수 없습니다: $b`n" +
                  "  다음 행동: ZIP 을 압축 해제한 폴더 안에서 이 스크립트를 실행하세요."
        }
    }

    Assert-NotRunning
    Test-Checksum -Dir $Source

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($b in $Binaries) {
        Copy-Item (Join-Path $Source $b) $InstallDir -Force
    }
    foreach ($extra in @('README.txt', 'LICENSE')) {
        $p = Join-Path $Source $extra
        if (Test-Path $p) { Copy-Item $p $InstallDir -Force }
    }
    Write-Ok "파일 복사 ($($Binaries.Count)개 실행 파일)"

    $appExe  = Join-Path $InstallDir 'velox-app.exe'
    $desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'APEX Velox.lnk'
    $start   = Join-Path ([Environment]::GetFolderPath('Programs')) 'APEX Velox.lnk'
    New-Shortcut -Path $desktop -Target $appExe -WorkDir $InstallDir
    New-Shortcut -Path $start   -Target $appExe -WorkDir $InstallDir
    Write-Ok '바탕화면 · 시작 메뉴 바로가기 생성'

    # 버전은 실제 바이너리에서 읽는다 — 스크립트에 하드코딩하지 않는다.
    $version = 'unknown'
    try {
        $raw = & (Join-Path $InstallDir 'velox.exe') --version 2>$null
        if ($raw -match '(\d+\.\d+\.\d+)') { $version = $Matches[1] }
    } catch { }

    New-Item -Path $RegKey -Force | Out-Null
    $uninstallCmd = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$InstallDir\Install-ApexVelox.ps1`" -Uninstall"
    Set-ItemProperty $RegKey DisplayName     $AppName
    Set-ItemProperty $RegKey DisplayVersion  $version
    Set-ItemProperty $RegKey Publisher       'APEX'
    Set-ItemProperty $RegKey InstallLocation $InstallDir
    Set-ItemProperty $RegKey UninstallString $uninstallCmd
    Set-ItemProperty $RegKey NoModify        1 -Type DWord
    Set-ItemProperty $RegKey NoRepair        1 -Type DWord
    Copy-Item $PSCommandPath (Join-Path $InstallDir 'Install-ApexVelox.ps1') -Force
    Write-Ok "제어판 등록 (버전 $version)"

    Write-Host "`n설치 완료." -ForegroundColor Cyan
    Write-Host "  실행: 바탕화면의 'APEX Velox' 또는 $appExe"
    Write-Host "  제거: 설정 > 앱 > 설치된 앱 > APEX Velox"
    Write-Host ""
    Write-Warn '코드 서명 전이라 첫 실행 시 SmartScreen 경고가 뜹니다.'
    Write-Step '"추가 정보" -> "실행"을 누르면 됩니다. (서명은 향후 버전 예정)'
    Write-Host ""
    Write-Host "  AI 기능을 쓰려면 키 등록과 동의가 필요합니다:" -ForegroundColor Gray
    Write-Host "    velox chorus set claude <키>" -ForegroundColor Gray
    Write-Host "    velox chorus consent claude --scope system" -ForegroundColor Gray
    Write-Host ""
}

if ($Uninstall) { Invoke-Uninstall } else { Invoke-Install }
