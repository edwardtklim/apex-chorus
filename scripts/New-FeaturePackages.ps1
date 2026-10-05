<#
.SYNOPSIS
    기능별 다운로드 파일을 만든다 — 웹사이트의 기능 섹션 하나에 ZIP 하나.

.DESCRIPTION
    출시 방식은 "개별 프로그램 먼저, 통합은 나중"이다. 통합 앱(apex-velox-*.zip)과 별개로,
    기능마다 따로 받을 수 있는 ZIP 을 만든다. 엔진(velox.exe)은 같고 진입점만 다르다.

        dist/features/
          apex-ai-chat-vX.Y.Z-win64.zip      01 AI Chat
          apex-storage-vX.Y.Z-win64.zip      03 PC 연결·저장
          apex-repair-vX.Y.Z-win64.zip       04 설치·문제 해결
          apex-system-vX.Y.Z-win64.zip       05 시스템 관리
          apex-plans-vX.Y.Z-win64.zip        공통 구독·비용
          *.sha256
          manifest.json                      웹사이트가 읽을 목록(이름·크기·sha256·포함 기능)

    각 ZIP 안: velox.exe · START-*.bat · 메뉴 스크립트 · README.txt · LICENSE

    버전은 빌드된 바이너리에서 읽는다. 릴리스 빌드가 먼저 있어야 한다.
#>
[CmdletBinding()]
param(
    # GitHub Release 태그. 주면 manifest 에 published=true 와 파일별 url 을 넣는다.
    # 반드시 이 실행이 만든 ZIP 을 그대로 그 태그에 올려야 한다(sha256 이 맞아야 한다).
    [string]$ReleaseTag
)

$ErrorActionPreference = 'Stop'

$Repo      = Split-Path $PSScriptRoot -Parent
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Repo 'target' }
$VeloxExe  = Join-Path $TargetDir 'release\velox.exe'
$OutDir    = Join-Path $Repo 'dist\features'
$Utf8Bom   = New-Object Text.UTF8Encoding $true

if (-not (Test-Path $VeloxExe)) { throw "릴리스 빌드가 없습니다: $VeloxExe`n  다음 행동: cargo build --workspace --release --locked" }
$raw = & $VeloxExe --version
if ($raw -notmatch '(\d+\.\d+\.\d+)') { throw "velox --version 을 해석할 수 없습니다: $raw" }
$Version = $Matches[1]

# 기능 정의 — 웹사이트 섹션과 같은 번호·이름·문구를 쓴다.
$Features = @(
    @{ Slug = 'ai-chat'; Number = '01'; Title = 'AI Chat'; Feature = 'chat'; Start = 'START-AI-CHAT.bat'
       Tagline = 'AI는 바꿔도, 이야기는 이어지도록.'
       Does = @('여러 AI 연결 (Claude · GPT · Gemini · Grok)', '대화 저장과 이어가기', '다른 AI로 바꿔 같은 대화 이어가기', '대화를 Markdown 파일로 내보내기')
       Notes = @('클라우드 AI는 본인 API 키가 필요하고, 이용료는 사용자 부담입니다.', '키는 Windows 보안 저장소에, 대화는 이 PC 안에만 저장됩니다.', '전송에 동의한 제공자에게만 질문이 나갑니다.') },
    @{ Slug = 'storage'; Number = '03'; Title = 'PC 연결·저장'; Feature = 'storage'; Start = 'START-STORAGE.bat'
       Tagline = '파일이 갈 곳을 한 번 정해두세요.'
       Does = @('저장 폴더 등록 (내 폴더 · 다른 PC의 공유 폴더)', '파일 하나 보내기', '복사 후 원본과 같은지 검증', '연결이 끊기면 대기했다가 다시 시도')
       Notes = @('이름이 같은 파일을 덮어쓰지 않고 새 이름으로 나란히 저장합니다.', '다른 PC의 폴더는 먼저 Windows에서 공유와 접근 권한을 설정해야 합니다.', '폴더 통째로 보내기·자동 동기화·원격 연결은 아직 없습니다.') },
    @{ Slug = 'repair'; Number = '04'; Title = '설치·문제 해결'; Feature = $null; Start = 'START-REPAIR.bat'
       Tagline = '고치기 전과 후를, 숫자로.'
       Does = @('PC 상태·온도·드라이버 확인', 'CPU 벤치마크와 쿨링 부하 테스트', '수리 전후 비교 리포트 (HTML)', '동의 후 AI 진단')
       Notes = @('온도 센서는 관리자 권한이 필요하고, 보드에 따라 읽지 못할 수 있습니다.', '같은 PC에서도 측정마다 5~7% 차이가 납니다. 10% 넘게 달라져야 개선으로 봅니다.', 'AI 진단은 본인 API 키가 필요합니다. 키 없이도 측정과 리포트는 됩니다.') },
    @{ Slug = 'system'; Number = '05'; Title = '시스템 관리'; Feature = 'system'; Start = 'START-SYSTEM.bat'
       Tagline = '내 PC의 상태와 달라진 점을 한눈에.'
       Does = @('서비스·시작 프로그램·디스크·네트워크 조회', '자동 시작인데 멈춘 서비스 찾기', '기준점 저장과 그 이후 달라진 것 확인')
       Notes = @('언제 읽은 정보인지 함께 보여줍니다.', '읽을 수 없는 값은 0으로 표시하지 않고 이유를 안내합니다.', '읽기 전용입니다. 서비스를 시작·중지하거나 설정을 바꾸지 않습니다.') },
    @{ Slug = 'plans'; Number = '공통'; Title = '구독·비용'; Feature = 'plans'; Start = 'START-PLANS.bat'
       Tagline = '구독료와 API 비용, 헷갈리지 않게 따로.'
       Does = @('AI 구독·API 계정 기록', '가까운 갱신일 확인', '월 고정 지출 (구독만, 통화별)', 'API 예산 대비 추정 사용액')
       Notes = @('금액과 갱신일은 직접 등록합니다. 자동으로 조회하지 않습니다.', '구독과 API 비용을 섞어 더하지 않습니다.', 'API 비용은 실제 청구액이 아닌 추정치입니다. 결제를 실행하지 않습니다.') }
)

function New-Readme {
    param($F)
    $lines = @(
        "APEX $($F.Title)  ·  v$Version"
        ''
        $F.Tagline
        ''
        '═══ 시작하는 법 ═══'
        ''
        '  1. 이 ZIP 을 전부 압축 해제합니다. (ZIP 안에서 바로 실행하면 안 됩니다)'
        "  2. $($F.Start) 을 더블클릭합니다."
        '  3. 번호를 눌러 기능을 고릅니다.'
        ''
        '═══ 할 수 있는 것 ═══'
        ''
    )
    $lines += $F.Does | ForEach-Object { "  · $_" }
    $lines += @('', '═══ 사용 전에 알아둘 점 ═══', '')
    $lines += $F.Notes | ForEach-Object { "  · $_" }
    $lines += @(
        ''
        '═══ 처음 실행할 때 경고가 뜨면 ═══'
        ''
        '  "Windows의 PC 보호" 파란 창 → "추가 정보" → "실행"'
        '  아직 코드 서명을 하지 않아서 뜨는 경고입니다.'
        '  Smart App Control 이 켜진 PC 에서는 실행이 차단될 수 있습니다.'
        ''
        '═══ 알아두세요 ═══'
        ''
        '  · Windows 10/11 64비트 전용입니다.'
        '  · 설치하지 않습니다. 이 폴더를 지우면 프로그램이 지워집니다.'
        '  · 기록은 %LOCALAPPDATA%\APEX\Velox 에 남습니다. 다른 APEX 프로그램과 같은 기록을 씁니다.'
        '  · 이 프로그램은 번호 메뉴 방식입니다. 창으로 된 화면은 통합 APEX 에서 제공합니다.'
        ''
        'github.com/edwardtklim/apex-chorus'
    )
    return ($lines -join "`r`n") + "`r`n"
}

if (Test-Path $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stageRoot = Join-Path $OutDir '_stage'
$manifest = @()

foreach ($F in $Features) {
    $name  = "apex-$($F.Slug)-v$Version-win64"
    $stage = Join-Path $stageRoot $name
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    Copy-Item $VeloxExe $stage -Force
    Copy-Item (Join-Path $Repo 'LICENSE') $stage -Force
    if ($F.Feature) {
        Copy-Item (Join-Path $PSScriptRoot 'APEX-Feature.ps1') $stage -Force
        $bat = "@echo off`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0APEX-Feature.ps1`" -Feature $($F.Feature)`r`nif errorlevel 1 pause`r`n"
    } else {
        # 설치·문제 해결은 기존 테스트 센터가 그 프로그램이다.
        Copy-Item (Join-Path $PSScriptRoot 'APEX-TestCenter.ps1') $stage -Force
        $bat = "@echo off`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0APEX-TestCenter.ps1`"`r`nif errorlevel 1 pause`r`n"
    }
    # .bat 은 ASCII 만 — 다른 언어 Windows 에서도 깨지지 않게.
    [IO.File]::WriteAllText((Join-Path $stage $F.Start), $bat, [Text.Encoding]::ASCII)
    [IO.File]::WriteAllText((Join-Path $stage 'README.txt'), (New-Readme $F), $Utf8Bom)

    & (Join-Path $PSScriptRoot 'Test-ReleaseSafety.ps1') -ArtifactPath $stage | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "패키지 안전 검사 실패: $name" }

    $zip = Join-Path $OutDir "$name.zip"
    Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    [IO.File]::WriteAllText("$zip.sha256", "$hash  $name.zip`n", [Text.Encoding]::ASCII)

    $manifest += [ordered]@{
        section   = $F.Number
        title     = $F.Title
        tagline   = $F.Tagline
        file      = "$name.zip"
        bytes     = (Get-Item $zip).Length
        sha256    = $hash
        url       = $(if ($ReleaseTag) { "https://github.com/edwardtklim/apex-chorus/releases/download/$ReleaseTag/$name.zip" } else { $null })
        start     = $F.Start
        does      = $F.Does
        notes     = $F.Notes
    }
    Write-Host ("  OK  {0,-36} {1,6:N1} MB" -f "$name.zip", ((Get-Item $zip).Length / 1MB)) -ForegroundColor Green
}

Remove-Item -LiteralPath $stageRoot -Recurse -Force

$doc = [ordered]@{
    version   = $Version
    built_at  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    published = [bool]$ReleaseTag
    release   = $(if ($ReleaseTag) { $ReleaseTag } else { $null })
    note      = 'published=false 이면 아직 GitHub Release 에 올리지 않은 파일이다. 웹사이트는 다운로드 링크를 만들지 않는다.'
    features  = $manifest
}
[IO.File]::WriteAllText((Join-Path $OutDir 'manifest.json'), ($doc | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
Write-Host "`n기능별 패키지 $($manifest.Count)개 → $OutDir`n" -ForegroundColor Cyan
