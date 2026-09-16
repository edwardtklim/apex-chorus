<#
.SYNOPSIS
    APEX Velox 테스트 센터 — 명령어를 몰라도 벤치마크와 APEX 기능을 메뉴로 실행한다.

.DESCRIPTION
    v0.20 Closed Alpha 실장비 검증용. 런칭·심사 전에 사람이 직접 돌리는 용도다.
    이 스크립트는 판단 로직을 갖지 않는다 — velox.exe 를 정해진 인자로 부르고
    출력을 결과 폴더에 남기기만 한다. 판정은 전부 velox-core 가 한다.

    실행 파일 탐색 순서:
      1. -VeloxDir 로 지정한 폴더
      2. 이 스크립트가 있는 폴더 (배포 ZIP 을 푼 폴더)
      3. %LOCALAPPDATA%\ApexVelox (install.bat 으로 설치한 위치)
      4. <repo>\target\release (개발 빌드)

    결과 저장 위치: 문서\APEX Velox 테스트\
      세션폴더\  이번 실행의 로그·스냅샷
      수리\      수리 전/후 측정 파일과 리포트 (재부팅을 넘어 유지)

.PARAMETER VeloxDir
    velox.exe 가 있는 폴더를 직접 지정.

.EXAMPLE
    START-APEX-TEST.bat 더블클릭
    .\APEX-TestCenter.ps1 -VeloxDir D:\build\release
#>
[CmdletBinding()]
param([string]$VeloxDir)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$OutputEncoding = [Text.Encoding]::UTF8
$Host.UI.RawUI.WindowTitle = 'APEX Velox 테스트 센터'

# --- 실행 파일 찾기 -------------------------------------------------------------
function Find-VeloxDir {
    $candidates = @()
    if ($VeloxDir) { $candidates += $VeloxDir }
    if ($PSScriptRoot) {
        $candidates += $PSScriptRoot
        $candidates += (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\release')
    }
    if ($env:CARGO_TARGET_DIR) { $candidates += (Join-Path $env:CARGO_TARGET_DIR 'release') }
    $candidates += (Join-Path $env:LOCALAPPDATA 'ApexVelox')
    foreach ($c in $candidates) {
        if ($c -and (Test-Path (Join-Path $c 'velox.exe'))) { return (Resolve-Path $c).Path }
    }
    return $null
}

$Bin = Find-VeloxDir
if (-not $Bin) {
    Write-Host "`n  velox.exe 를 찾을 수 없습니다." -ForegroundColor Red
    Write-Host '  다음 행동: ZIP 을 전부 압축 해제한 뒤, 그 폴더 안의 START-APEX-TEST.bat 을 실행하세요.'
    Write-Host '            (ZIP 안에서 바로 실행하면 실행 파일을 찾지 못합니다)'
    Read-Host "`n  Enter 를 누르면 닫힙니다"
    exit 1
}
$Velox = Join-Path $Bin 'velox.exe'
$App   = Join-Path $Bin 'velox-app.exe'

# --- 결과 폴더 ------------------------------------------------------------------
$Docs = [Environment]::GetFolderPath('MyDocuments')
if (-not $Docs) { $Docs = $env:USERPROFILE }
$Root      = Join-Path $Docs 'APEX Velox 테스트'
$RepairDir = Join-Path $Root '수리'
$Session   = Join-Path $Root (Get-Date -Format 'yyyy-MM-dd_HHmmss')
New-Item -ItemType Directory -Force -Path $Session, $RepairDir | Out-Null
$Log = Join-Path $Session '전체로그.txt'

$IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).
    IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$Utf8NoBom = New-Object Text.UTF8Encoding $false
function Write-Log {
    param([string]$Text)
    [IO.File]::AppendAllText($Log, "$Text`r`n", $Utf8NoBom)
}

# --- 공통 실행기 ----------------------------------------------------------------
# 출력은 화면과 로그에 동시에 남긴다. 실행 자체가 막히면(SmartScreen·Smart App Control)
# 원인과 다음 행동을 알려준다.
function Invoke-Velox {
    param(
        [Parameter(Mandatory)][string]$Title,
        [Parameter(Mandatory)][string[]]$Arguments,
        [switch]$NoLog   # 화면을 계속 갱신하는 명령(대시보드 등)은 파이프를 거치면 깨진다
    )
    Write-Host "`n━━━ $Title ━━━" -ForegroundColor Cyan
    Write-Host "  velox $($Arguments -join ' ')" -ForegroundColor DarkGray
    $started = Get-Date
    Write-Log "`r`n===== [$($started.ToString('HH:mm:ss'))] $Title  (velox $($Arguments -join ' ')) ====="
    # velox 가 stderr 에 쓰는 안내문을 PowerShell 5.1 은 오류 레코드로 감싼다.
    # 여기서 Stop 이면 첫 안내문에서 메뉴가 죽으므로 이 함수 안에서는 Continue.
    $ErrorActionPreference = 'Continue'
    try {
        if ($NoLog) {
            & $Velox @Arguments
        } else {
            # Tee-Object 는 5.1 에서 UTF-16 으로 써서 로그가 깨진다 — 직접 UTF-8 로 남긴다.
            & $Velox @Arguments 2>&1 | ForEach-Object {
                $line = "$_"
                Write-Host $line
                Write-Log $line
            }
        }
        $code = $LASTEXITCODE
    } catch {
        Write-Host "  ✗ 실행하지 못했습니다: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host '  다음 행동: SmartScreen 이 막았다면 velox.exe 를 한 번 더블클릭해 "추가 정보 → 실행".' -ForegroundColor Yellow
        Write-Host '            Smart App Control 이 켜져 있으면 실행이 차단됩니다 — 이 경우를 기록해 두세요.' -ForegroundColor Yellow
        Write-Log "실행 실패: $($_.Exception.Message)"
        return $false
    }
    $secs = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    Write-Log "----- 종료 코드 $code · ${secs}초 -----"
    if ($code -ne 0 -and $null -ne $code) {
        Write-Host "  ! 종료 코드 $code (${secs}초)" -ForegroundColor Yellow
        return $false
    }
    Write-Host "  ✓ 완료 (${secs}초)" -ForegroundColor Green
    return $true
}

function Read-Choice {
    param([string]$Prompt, [string]$Default)
    $v = Read-Host "  $Prompt [$Default]"
    if ([string]::IsNullOrWhiteSpace($v)) { return $Default }
    return $v.Trim()
}

function Read-Seconds {
    param([string]$Prompt, [int]$Default)
    while ($true) {
        $v = Read-Choice $Prompt "$Default"
        $n = 0
        if ([int]::TryParse($v, [ref]$n) -and $n -gt 0 -and $n -le 3600) { return $n }
        Write-Host '  1 ~ 3600 사이 숫자로 입력하세요.' -ForegroundColor Yellow
    }
}

function Pause-Menu { Read-Host "`n  Enter 를 누르면 메뉴로 돌아갑니다" | Out-Null }

# --- 수리 전/후 -------------------------------------------------------------------
function Invoke-Capture {
    param([ValidateSet('before', 'after')][string]$Label)
    $name = if ($Label -eq 'before') { '수리 전' } else { '수리 후' }
    $out  = Join-Path $RepairDir "$Label-$(Get-Date -Format 'yyyyMMdd-HHmmss').json"
    [void](Invoke-Velox "$name 측정 (CPU 점수 포함)" @('report', 'capture', '--label', $Label, '--bench', '--out', $out))
}

function Get-Latest {
    param([string]$Label)
    Get-ChildItem -Path $RepairDir -Filter "$Label-*.json" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
}

function Invoke-RepairReport {
    $before = Get-Latest 'before'
    $after  = Get-Latest 'after'
    if (-not $before -or -not $after) {
        Write-Host "`n  수리 전/후 측정이 모두 있어야 합니다." -ForegroundColor Yellow
        Write-Host "  수리 전: $(if ($before) { $before.Name } else { '없음 → 메뉴 6' })"
        Write-Host "  수리 후: $(if ($after) { $after.Name } else { '없음 → 메뉴 7' })"
        return
    }
    if ($after.LastWriteTime -lt $before.LastWriteTime) {
        Write-Host "`n  ! 가장 최근 '수리 후' 측정이 '수리 전' 보다 오래됐습니다. 수리 후 측정을 다시 하세요." -ForegroundColor Yellow
        return
    }
    Write-Host "`n  비교할 파일 (가장 최근 것):"
    Write-Host "    수리 전: $($before.Name)"
    Write-Host "    수리 후: $($after.Name)"
    $machine = Read-Choice 'PC 이름' $env:COMPUTERNAME
    $note    = Read-Choice '무엇을 작업했나요' '청소 및 점검'
    $html    = Join-Path $RepairDir "리포트-$(Get-Date -Format 'yyyyMMdd-HHmmss').html"
    $ok = Invoke-Velox '수리 리포트 생성' @('report', 'repair', '--before', $before.FullName, '--after', $after.FullName,
                                          '--out', $html, '--machine', $machine, '--note', $note)
    if ($ok -and (Test-Path $html)) { Start-Process $html }
}

# --- 전체 점검 ------------------------------------------------------------------
function Invoke-FullCheck {
    Write-Host "`n  전체 점검: 시스템 정보 → 스냅샷 → GPU → 드라이버 → CPU 벤치 → 지속 성능 → 쿨링 테스트"
    $thermal = Read-Seconds '쿨링 테스트 시간(초) — 정식 5분=300, 빠른 확인=60' 300
    $total = [math]::Ceiling(($thermal + 90) / 60)
    Write-Host "  예상 소요: 약 ${total}분. 그동안 다른 무거운 작업은 하지 마세요." -ForegroundColor Yellow
    if (-not $IsAdmin) {
        Write-Host '  (관리자 권한이 아니라 온도가 "측정 불가"로 나올 수 있습니다 — 메뉴 A)' -ForegroundColor DarkYellow
    }

    $snap = Join-Path $Session 'snapshot.json'
    $steps = @(
        @{ T = '1/7 시스템 정보';       A = @('info') },
        @{ T = '2/7 스냅샷 저장';       A = @('snapshot', '--out', $snap) },
        @{ T = '3/7 GPU 상태';          A = @('gpu', 'status') },
        @{ T = '4/7 드라이버·장치';     A = @('drivers') },
        @{ T = '5/7 CPU 벤치마크';      A = @('bench', 'cpu') },
        @{ T = '6/7 지속 성능(쓰로틀링)'; A = @('bench', 'stability', '--seconds', '15') },
        @{ T = '7/7 쿨링 부하 테스트';   A = @('bench', 'thermal', '--seconds', "$thermal") }
    )
    $failed = @()
    foreach ($s in $steps) {
        if (-not (Invoke-Velox $s.T $s.A)) { $failed += $s.T }
    }

    Write-Host "`n━━━ 전체 점검 결과 ━━━" -ForegroundColor Cyan
    if ($failed.Count -eq 0) {
        Write-Host '  모든 단계 완료' -ForegroundColor Green
    } else {
        Write-Host "  문제 있었던 단계: $($failed -join ', ')" -ForegroundColor Yellow
    }
    Write-Host "  결과 폴더: $Session"
    Start-Process explorer.exe $Session
}

# --- 기타 -------------------------------------------------------------------------
function Restart-AsAdmin {
    if ($IsAdmin) { Write-Host '  이미 관리자 권한입니다.' -ForegroundColor Green; return }
    $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-VeloxDir', "`"$Bin`"")
    try {
        Start-Process powershell.exe -Verb RunAs -ArgumentList $argList
        exit 0
    } catch {
        Write-Host '  관리자 권한 요청이 취소됐습니다.' -ForegroundColor Yellow
    }
}

function Invoke-Custom {
    Write-Host "`n  velox 뒤에 붙일 명령을 입력하세요. 예: bench everyday / timeline show / chorus models"
    $line = Read-Host '  velox'
    if ([string]::IsNullOrWhiteSpace($line)) { return }
    # 따옴표로 묶인 인자를 하나로 유지한다.
    $parts = [regex]::Matches($line, '"([^"]*)"|(\S+)') | ForEach-Object {
        if ($_.Groups[1].Success) { $_.Groups[1].Value } else { $_.Groups[2].Value }
    }
    [void](Invoke-Velox '직접 입력' @($parts))
}

function Show-Header {
    Clear-Host
    # velox --version 은 clap 이 바로 종료시켜 알파 지표에 가짜 crash 로 남는다 — 부르지 않는다.
    $ver = "velox.exe 빌드 $((Get-Item $Velox).LastWriteTime.ToString('yyyy-MM-dd HH:mm'))"
    Write-Host ''
    Write-Host '  ╔══════════════════════════════════════════════╗' -ForegroundColor Cyan
    Write-Host '  ║        APEX Velox  ·  테스트 센터            ║' -ForegroundColor Cyan
    Write-Host '  ╚══════════════════════════════════════════════╝' -ForegroundColor Cyan
    Write-Host "  버전   $ver"
    Write-Host "  권한   $(if ($IsAdmin) { '관리자 (온도 센서 포함)' } else { '일반 (온도는 측정 불가일 수 있음 → A)' })"
    Write-Host "  PC     $env:COMPUTERNAME"
    Write-Host "  결과   $Session" -ForegroundColor DarkGray
    Write-Host ''
    Write-Host '  ── 벤치마크 ─────────────────────────────' -ForegroundColor DarkCyan
    Write-Host '   1  전체 점검 (추천 · 한 번에 다)'
    Write-Host '   2  CPU 벤치마크 (싱글/멀티)'
    Write-Host '   3  쿨링 부하 테스트 (기본 5분)'
    Write-Host '   4  지속 성능 / 쓰로틀링'
    Write-Host '   5  체감 성능 + GPU 모니터 (bench all)'
    Write-Host '  ── 수리 전후 비교 ───────────────────────' -ForegroundColor DarkCyan
    Write-Host '   6  수리 전 측정'
    Write-Host '   7  수리 후 측정'
    Write-Host '   8  수리 리포트 만들기 (HTML 열림)'
    Write-Host '  ── APEX 기능 ────────────────────────────' -ForegroundColor DarkCyan
    Write-Host '   9  APEX 앱 열기'
    Write-Host '  10  시스템 정보 · 온도'
    Write-Host '  11  드라이버 · 장치 확인'
    Write-Host '  12  실시간 대시보드 (Ctrl+C 로 종료)'
    Write-Host '  13  AI 종합 진단 Doctor (API 키 필요)'
    Write-Host '  14  AI 연결 상태 (chorus models)'
    Write-Host '  15  알파 지표 보기 + 내보내기'
    Write-Host '  16  틀린 경고 신고 (false-warning)'
    Write-Host '  ── 기타 ─────────────────────────────────' -ForegroundColor DarkCyan
    Write-Host '   A  관리자 권한으로 다시 열기'
    Write-Host '   O  결과 폴더 열기'
    Write-Host '   C  velox 명령 직접 입력'
    Write-Host '   Q  종료'
    Write-Host ''
}

# --- 메인 루프 ------------------------------------------------------------------
Write-Log "APEX Velox 테스트 센터 · $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') · 관리자=$IsAdmin · 실행파일=$Bin"

:menu while ($true) {
    Show-Header
    $c = (Read-Host '  선택').Trim().ToUpper()
    switch ($c) {
        '1'  { Invoke-FullCheck }
        '2'  { [void](Invoke-Velox 'CPU 벤치마크' @('bench', 'cpu')) }
        '3'  { $s = Read-Seconds '시간(초)' 300; $l = Read-Choice '온도 한계(°C)' '85'
               [void](Invoke-Velox '쿨링 부하 테스트' @('bench', 'thermal', '--seconds', "$s", '--limit', $l)) }
        '4'  { $s = Read-Seconds '단계별 시간(초)' 15
               [void](Invoke-Velox '지속 성능' @('bench', 'stability', '--seconds', "$s")) }
        '5'  { [void](Invoke-Velox '전체 벤치 (bench all)' @('bench', 'all')) }
        '6'  { Invoke-Capture 'before' }
        '7'  { Invoke-Capture 'after' }
        '8'  { Invoke-RepairReport }
        '9'  { if (Test-Path $App) { Start-Process $App -WorkingDirectory $Bin; Write-Host '  앱을 열었습니다.' -ForegroundColor Green }
               else { Write-Host "  velox-app.exe 가 없습니다: $Bin" -ForegroundColor Red } }
        '10' { [void](Invoke-Velox '시스템 정보' @('info')) }
        '11' { [void](Invoke-Velox '드라이버 · 장치' @('drivers')) }
        '12' { [void](Invoke-Velox '실시간 대시보드' @('dashboard') -NoLog) }
        '13' { [void](Invoke-Velox 'AI 종합 진단' @('doctor') -NoLog) }
        '14' { [void](Invoke-Velox 'AI 연결 상태' @('chorus', 'models')) }
        '15' { [void](Invoke-Velox '알파 지표' @('metrics', 'summary'))
               $out = Join-Path $Session 'alpha-metrics.json'
               if (Invoke-Velox '알파 지표 내보내기' @('metrics', 'export', '--out', $out)) {
                   Write-Host "  보낼 파일: $out" -ForegroundColor Green
               } }
        '16' { [void](Invoke-Velox '틀린 경고 신고' @('metrics', 'false-warning')) }
        'A'  { Restart-AsAdmin }
        'O'  { Start-Process explorer.exe $Root; continue menu }
        'C'  { Invoke-Custom }
        'Q'  { exit 0 }
        default { Write-Host '  목록에 있는 번호를 입력하세요.' -ForegroundColor Yellow; Start-Sleep -Milliseconds 800; continue menu }
    }
    Pause-Menu
}
