<#
.SYNOPSIS
    APEX 기능별 프로그램 — 기능 하나를 번호 메뉴로 쓴다.

.DESCRIPTION
    마스터 플랜의 출시 방식은 "개별 프로그램 먼저, 통합은 나중"이다. 이 스크립트는
    웹사이트의 기능 섹션 하나에 대응하는 프로그램 하나를 만든다(AI Chat, PC 연결·저장,
    시스템 관리, 구독·비용). 설치·문제 해결은 APEX-TestCenter.ps1 이 맡는다.

    판단 로직을 갖지 않는다 — velox.exe 를 정해진 인자로 부르기만 한다.
    각 메뉴의 이름·설명·주의사항은 웹사이트의 해당 섹션과 같은 말을 쓴다.

.PARAMETER Feature
    chat | storage | system | plans

.PARAMETER VeloxDir
    velox.exe 가 있는 폴더. 생략하면 이 스크립트 폴더 → 설치 위치 → 개발 빌드 순으로 찾는다.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('chat', 'storage', 'system', 'plans')][string]$Feature,
    [string]$VeloxDir
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$OutputEncoding = [Text.Encoding]::UTF8

# --- 실행 파일 찾기 -------------------------------------------------------------
function Find-Velox {
    $candidates = @()
    if ($VeloxDir) { $candidates += $VeloxDir }
    if ($PSScriptRoot) {
        $candidates += $PSScriptRoot
        $candidates += (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\release')
    }
    if ($env:CARGO_TARGET_DIR) { $candidates += (Join-Path $env:CARGO_TARGET_DIR 'release') }
    $candidates += (Join-Path $env:LOCALAPPDATA 'ApexVelox')
    foreach ($c in $candidates) {
        if ($c -and (Test-Path (Join-Path $c 'velox.exe'))) { return (Join-Path (Resolve-Path $c).Path 'velox.exe') }
    }
    return $null
}

$Velox = Find-Velox
if (-not $Velox) {
    Write-Host "`n  velox.exe 를 찾을 수 없습니다." -ForegroundColor Red
    Write-Host '  다음 행동: ZIP 을 전부 압축 해제한 뒤, 그 폴더 안의 START 파일을 실행하세요.'
    Write-Host '            (ZIP 안에서 바로 실행하면 실행 파일을 찾지 못합니다)'
    Read-Host "`n  Enter 를 누르면 닫힙니다" | Out-Null
    exit 1
}

# --- 공통 도구 ------------------------------------------------------------------
function Invoke-V {
    # velox 가 stderr 에 쓰는 안내문을 PowerShell 5.1 은 오류로 감싼다 — 여기서는 Continue.
    $ErrorActionPreference = 'Continue'
    Write-Host ''
    try {
        & $Velox @args 2>&1 | ForEach-Object { Write-Host "  $_" }
    } catch {
        Write-Host "  ✗ 실행하지 못했습니다: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host '  다음 행동: SmartScreen 이 막았다면 velox.exe 를 한 번 더블클릭해 "추가 정보 → 실행".' -ForegroundColor Yellow
    }
}

function Get-VJson {
    # 사람용 출력을 파싱하지 않는다 — 목록이 필요하면 --json 을 쓴다.
    $ErrorActionPreference = 'Continue'
    try {
        $raw = (& $Velox @args 2>$null) -join "`n"
        if ([string]::IsNullOrWhiteSpace($raw)) { return @() }
        # PowerShell 5.1 의 ConvertFrom-Json 은 배열을 한 덩어리로 내보낸다 — 한 번 더 흘려 풀어 준다.
        $parsed = ConvertFrom-Json $raw
        return @($parsed | ForEach-Object { $_ })
    } catch { return @() }
}

function Ask {
    param([string]$Prompt, [string]$Default = '')
    $hint = if ($Default) { " [$Default]" } else { '' }
    $v = Read-Host "  $Prompt$hint"
    if ([string]::IsNullOrWhiteSpace($v)) { return $Default }
    return $v.Trim().Trim('"')
}

function Ask-Required {
    param([string]$Prompt)
    $v = Ask $Prompt
    if (-not $v) { Write-Host '  취소했습니다.' -ForegroundColor Yellow }
    return $v
}

function Confirm-Yes {
    param([string]$Prompt)
    return ((Read-Host "  $Prompt (y/N)").Trim().ToLower() -eq 'y')
}

# 목록에서 번호로 고른다. 빈 입력은 취소.
function Select-One {
    param([object[]]$Items, [scriptblock]$Label, [string]$Empty)
    if (-not $Items -or $Items.Count -eq 0) { Write-Host "`n  $Empty" -ForegroundColor Yellow; return $null }
    Write-Host ''
    for ($i = 0; $i -lt $Items.Count; $i++) { Write-Host ("  {0,2}  {1}" -f ($i + 1), (& $Label $Items[$i])) }
    $n = 0
    $v = Read-Host "`n  번호 (비우면 취소)"
    if ([int]::TryParse($v, [ref]$n) -and $n -ge 1 -and $n -le $Items.Count) { return $Items[$n - 1] }
    Write-Host '  취소했습니다.' -ForegroundColor Yellow
    return $null
}

$Providers = 'claude', 'gpt', 'gemini', 'grok'
function Ask-Provider {
    param([string]$Default = 'claude')
    $p = (Ask "AI 제공자 ($($Providers -join ' / '))" $Default).ToLower()
    return $p
}

# --- 01 AI Chat -----------------------------------------------------------------
function Get-Chats { Get-VJson chorus chat list --json }
function Select-Chat {
    Select-One (Get-Chats) { param($c) "$($c.title)  ·  메시지 $($c.message_count)  ·  $($c.updated_at.Substring(0,10))" } '저장된 대화가 없습니다. 먼저 "새 대화 시작"을 고르세요.'
}

$Chat = @{
    Number = '01'; Title = 'AI Chat'; Tagline = 'AI는 바꿔도, 이야기는 이어지도록.'
    Notes  = @(
        '클라우드 AI는 본인 API 키가 필요합니다. 구독과 API 사용 요금은 별개일 수 있습니다.',
        '키는 Windows 보안 저장소에 저장됩니다. 대화는 이 PC 안에만 저장됩니다.',
        '전송에 동의한 제공자에게만 질문이 나갑니다.'
    )
    Items  = @(
        @{ Key = '1'; Label = '연결 상태 보기 (키·동의·모델)'; Run = { Invoke-V chorus models } },
        @{ Key = '2'; Label = 'API 키 등록'; Run = {
                $p = Ask-Provider
                # 키는 화면에 표시하지 않고 받는다.
                $sec = Read-Host "  $p API 키 (입력해도 화면에 보이지 않습니다)" -AsSecureString
                $key = [Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec))
                if (-not $key) { Write-Host '  취소했습니다.' -ForegroundColor Yellow; return }
                Invoke-V chorus set $p $key
                $key = $null
            } },
        @{ Key = '3'; Label = '전송 동의 / 철회'; Run = {
                $p = Ask-Provider
                $a = (Ask '동의(y) / 철회(r)' 'y').ToLower()
                if ($a -eq 'r') { Invoke-V chorus revoke $p }
                else {
                    Write-Host '  동의하면 질문과 최소한의 시스템 정보가 이 제공자에게 전송됩니다.' -ForegroundColor Yellow
                    if (Confirm-Yes "$p 에 전송을 허용할까요?") { Invoke-V chorus consent $p --scope minimal }
                }
            } },
        @{ Key = '4'; Label = '새 대화 시작'; Run = {
                $t = Ask-Required '대화 제목'; if (-not $t) { return }
                Invoke-V chorus chat new $t --use (Ask-Provider)
            } },
        @{ Key = '5'; Label = '대화 이어서 질문하기 (다른 AI로 바꿔도 맥락이 이어집니다)'; Run = {
                $c = Select-Chat; if (-not $c) { return }
                $q = Ask-Required '질문'; if (-not $q) { return }
                $p = Ask-Provider $(if ($c.provider) { $c.provider } else { 'claude' })
                Invoke-V chorus ask $q --use $p --conversation $c.id
            } },
        @{ Key = '6'; Label = '대화 목록'; Run = { Invoke-V chorus chat list } },
        @{ Key = '7'; Label = '대화 내용 보기'; Run = { $c = Select-Chat; if ($c) { Invoke-V chorus chat show $c.id } } },
        @{ Key = '8'; Label = 'Markdown 파일로 내보내기 (노트 앱으로 옮기기)'; Run = {
                $c = Select-Chat; if (-not $c) { return }
                $out = Ask '저장할 경로 (비우면 APEX 리포트 폴더)'
                if ($out) { Invoke-V chorus chat export $c.id --out $out } else { Invoke-V chorus chat export $c.id }
            } },
        @{ Key = '9'; Label = '대화 삭제'; Run = {
                $c = Select-Chat; if (-not $c) { return }
                if (Confirm-Yes "`"$($c.title)`" 를 삭제할까요? 되돌릴 수 없습니다") { Invoke-V chorus chat delete $c.id }
            } }
    )
}

# --- 03 PC 연결·저장 ------------------------------------------------------------
function Select-Dest {
    Select-One (Get-VJson storage list --json) { param($d) "$($d.name)  ·  $($d.path)" } '등록된 저장 대상이 없습니다. 먼저 "저장 대상 등록"을 고르세요.'
}

$Storage = @{
    Number = '03'; Title = 'PC 연결·저장'; Tagline = '파일이 갈 곳을 한 번 정해두세요.'
    Notes  = @(
        '이름이 같은 파일을 덮어쓰지 않고 새 이름으로 나란히 저장합니다.',
        '다른 PC의 폴더는 먼저 Windows에서 공유와 접근 권한을 설정해야 합니다.',
        '파일 하나씩 보냅니다. 폴더 통째로 보내기·자동 동기화는 아직 없습니다.'
    )
    Items  = @(
        @{ Key = '1'; Label = '저장 대상과 연결 상태 보기'; Run = { Invoke-V storage list } },
        @{ Key = '2'; Label = '저장 대상 등록 (내 폴더 또는 다른 PC의 공유 폴더)'; Run = {
                $n = Ask-Required '알아보기 쉬운 이름 (예: 집 서버)'; if (-not $n) { return }
                $p = Ask-Required '폴더 경로 (예: D:\Backup 또는 \\서버\공유\APEX)'; if (-not $p) { return }
                Invoke-V storage add $n $p
            } },
        @{ Key = '3'; Label = '파일 보내기'; Run = {
                $d = Select-Dest; if (-not $d) { return }
                $f = Ask-Required '보낼 파일 경로 (탐색기에서 파일을 이 창에 끌어다 놓아도 됩니다)'; if (-not $f) { return }
                Invoke-V storage send $f --to $d.id
            } },
        @{ Key = '4'; Label = '대기 중인 전송 다시 시도'; Run = { Invoke-V storage retry } },
        @{ Key = '5'; Label = '전송 기록'; Run = { Invoke-V storage log } },
        @{ Key = '6'; Label = '저장 대상 등록 삭제 (폴더의 파일은 그대로)'; Run = {
                $d = Select-Dest; if (-not $d) { return }
                if (Confirm-Yes "`"$($d.name)`" 등록을 지울까요?") { Invoke-V storage remove $d.id }
            } }
    )
}

# --- 05 시스템 관리 -------------------------------------------------------------
$System = @{
    Number = '05'; Title = '시스템 관리'; Tagline = '내 PC의 상태와 달라진 점을 한눈에.'
    Notes  = @(
        '언제 읽은 정보인지 함께 보여줍니다.',
        '읽을 수 없는 값은 0으로 표시하지 않고 이유를 안내합니다.',
        '읽기 전용입니다. 서비스를 시작·중지하거나 설정을 바꾸지 않습니다.'
    )
    Items  = @(
        @{ Key = '1'; Label = '지금 상태 (디스크·네트워크·서비스·확인할 항목)'; Run = { Invoke-V system status } },
        @{ Key = '2'; Label = '자동 시작인데 멈춘 서비스'; Run = { Invoke-V system services --stopped } },
        @{ Key = '3'; Label = '서비스 전체 목록'; Run = { Invoke-V system services } },
        @{ Key = '4'; Label = '시작 프로그램'; Run = { Invoke-V system startup } },
        @{ Key = '5'; Label = '지금 상태를 기준점으로 저장'; Run = {
                if (Confirm-Yes '기준점을 지금 상태로 바꿀까요? 이전 기준점은 교체됩니다') { Invoke-V system save }
            } },
        @{ Key = '6'; Label = '기준점 이후 달라진 것'; Run = { Invoke-V system changes } }
    )
}

# --- 공통 구독·비용 -------------------------------------------------------------
function Select-Plan {
    $book = Get-VJson plans list --json
    $entries = if ($book -and $book[0].entries) { @($book[0].entries) } else { @() }
    Select-One $entries { param($e) "$($e.provider)  ·  $(if ($e.kind -eq 'api') { 'API' } else { '구독' })  ·  $($e.plan)" } '기록이 없습니다. 먼저 구독이나 API 계정을 추가하세요.'
}

$Plans = @{
    Number = '공통'; Title = '구독·비용'; Tagline = '구독료와 API 비용, 헷갈리지 않게 따로.'
    Notes  = @(
        '금액과 갱신일은 직접 등록합니다. 자동으로 조회하지 않습니다.',
        '구독과 API 비용을 섞어 더하지 않고 통화별로 보여줍니다.',
        'API 비용은 실제 청구액이 아닌 추정치입니다. 결제를 실행하지 않습니다.'
    )
    Items  = @(
        @{ Key = '1'; Label = '전체 기록과 월 고정 지출'; Run = { Invoke-V plans list } },
        @{ Key = '2'; Label = '14일 안에 갱신되는 구독'; Run = { Invoke-V plans upcoming } },
        @{ Key = '3'; Label = '구독 추가 (ChatGPT Plus, Claude Max 같은 정액제)'; Run = {
                $p = Ask-Required '제공자 (예: claude, chatgpt, gemini)'; if (-not $p) { return }
                $a = @('plans', 'add', '--provider', $p, '--kind', 'subscription',
                    '--plan', (Ask '플랜 이름 (예: Plus, Max)'), '--cycle', (Ask '주기 (monthly / yearly)' 'monthly'),
                    '--currency', (Ask '통화' 'USD').ToUpper(), '--purpose', (Ask '주로 무엇에 쓰나요'))
                $amt = Ask '주기당 금액 (모르면 비우기)'; if ($amt) { $a += @('--amount', $amt) }
                $ren = Ask '다음 갱신일 YYYY-MM-DD (모르면 비우기)'; if ($ren) { $a += @('--renews', $ren) }
                Invoke-V @a
            } },
        @{ Key = '4'; Label = 'API 계정 추가 (사용량만큼 내는 계정)'; Run = {
                $p = Ask-Required '제공자 (예: claude, gpt)'; if (-not $p) { return }
                $a = @('plans', 'add', '--provider', $p, '--kind', 'api', '--plan', (Ask '플랜 이름' 'Pay-as-you-go'),
                    '--currency', (Ask '통화' 'USD').ToUpper(), '--purpose', (Ask '주로 무엇에 쓰나요'))
                $b = Ask '월 예산 (넘으면 알려줍니다. 없으면 비우기)'; if ($b) { $a += @('--budget', $b) }
                Invoke-V @a
            } },
        @{ Key = '5'; Label = '값 확인 / 갱신일 고치기'; Run = {
                $e = Select-Plan; if (-not $e) { return }
                $ren = Ask '새 갱신일 YYYY-MM-DD (그대로면 비우기)'
                if ($ren) { Invoke-V plans confirm $e.id --renews $ren } else { Invoke-V plans confirm $e.id }
            } },
        @{ Key = '6'; Label = '기록 삭제'; Run = {
                $e = Select-Plan; if (-not $e) { return }
                if (Confirm-Yes "`"$($e.provider) $($e.plan)`" 기록을 삭제할까요?") { Invoke-V plans remove $e.id }
            } },
        @{ Key = '7'; Label = 'API 사용량과 추정 비용'; Run = { Invoke-V usage summary } }
    )
}

$Def = @{ chat = $Chat; storage = $Storage; system = $System; plans = $Plans }[$Feature]
$Host.UI.RawUI.WindowTitle = "APEX $($Def.Title)"

function Show-Menu {
    Clear-Host
    Write-Host ''
    Write-Host "  APEX" -ForegroundColor White
    Write-Host "  $($Def.Number)  $($Def.Title)" -ForegroundColor Cyan
    Write-Host "  $($Def.Tagline)"
    Write-Host '  ─────────────────────────────────────────────' -ForegroundColor DarkGray
    foreach ($i in $Def.Items) { Write-Host ("  {0,2}  {1}" -f $i.Key, $i.Label) }
    Write-Host '  ─────────────────────────────────────────────' -ForegroundColor DarkGray
    Write-Host '   ?  사용 전에 알아둘 점'
    Write-Host '   Q  종료'
    Write-Host ''
}

:menu while ($true) {
    Show-Menu
    $c = (Read-Host '  선택').Trim().ToUpper()
    if ($c -eq 'Q') { exit 0 }
    if ($c -eq '?') {
        Write-Host "`n  사용 전에 알아둘 점" -ForegroundColor Cyan
        foreach ($n in $Def.Notes) { Write-Host "  · $n" }
    } else {
        $item = $Def.Items | Where-Object { $_.Key -eq $c } | Select-Object -First 1
        if (-not $item) { continue menu }
        try { & $item.Run } catch { Write-Host "  ✗ $($_.Exception.Message)" -ForegroundColor Red }
    }
    Read-Host "`n  Enter 를 누르면 메뉴로" | Out-Null
}
