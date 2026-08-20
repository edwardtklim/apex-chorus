# APEX Velox — Claude Engineering Handoff & Product Roadmap

> 이 문서는 Claude가 APEX Velox 개발을 독립적으로 이어가기 위한 공식 기준이다.
> 아이디어 목록이 아니라 **제품 범위, 버전별 작업, 보안 불변조건, 완료 조건**을 정의한다.

## 0. 현재 기준점

*(2026-08-19 갱신 — 미국 새 데스크톱으로 이주 후 실측)*

- 기준 브랜치: `main`
- 현재 HEAD: `5b06f3f` (`chore(v0.18): align workspace version and roadmap`)
- 원격 `origin/main`: `5b06f3f` — **동기 상태, 미푸시 커밋 0**
- Workspace package version: `0.18.0`
- 개발 중인 다음 버전: `0.19.0` (Product Hardening)
- 게시된 GitHub Release: **v0.15.0 · v0.16.0 · v0.18.0(Latest)**
  - `v0.17.0` 태그는 만들지 않는다 — workspace version 이 0.16.0 에서 0.18.0 으로
    직접 올라가 0.17.0 인 커밋이 저장소에 없다. v0.17 Local Usage Ledger 는
    v0.18.0 릴리스에 포함해 배포했다.
- 기존 태그: v0.5.0 ~ v0.9.0, v0.15.0, v0.16.0, v0.18.0

코드 규모:

```text
velox-core     17 모듈  5,894줄   엔진(데이터·정책·판단·안전)
velox-cli      24 파일  4,330줄   터미널 인터페이스
velox-server    1 파일    730줄   인증된 localhost HTTP/SSE (라우트 24개)
velox-app       1 파일    115줄   wry+tao 네이티브 창
site/index.html          585줄   사이트 = 앱 (권한만 다름)
```

검증 상태 (2026-08-19, Windows 11 26200):

```text
cargo test --workspace --locked                                  PASS (109 tests)
cargo fmt --all -- --check                                       PASS
cargo clippy --workspace --all-targets --all-features --locked    PASS (-D warnings)
cargo build --workspace --release --locked                       PASS
scripts/Test-ReleaseSafety.ps1                                   PASS
scripts/Test-UsageApi.ps1                                        PASS
scripts/Test-ProjectApi.ps1                                      PASS
working tree                                                     clean
```

### 개발 환경 주의사항 (새 PC 이주에서 확인)

- **Smart App Control(SAC)** 이 켜져 있으면 서명 안 된 자체 빌드 산출물이 차단되어
  `cargo test` 가 `os error 4551` 로 실패한다. CodeIntegrity 로그에 이벤트 3077/3118 이 남는다.
  개발 머신에서는 SAC 를 끄는 수밖에 없다(끄면 재설치 전까지 복구 불가).
  **이는 코드 서명이 v0.19 필수 항목인 이유를 그대로 보여준다** — 사용자 PC 에서도 같은 일이 일어난다.
- MSVC Build Tools(VCTools 워크로드)가 없으면 링커 에러로 빌드 자체가 실패한다.
- CI 스크립트는 `pwsh`(PowerShell 7)를 요구한다.

## 1. 제품 정의

APEX는 단순한 AI 채팅 앱도, 기능을 모아놓은 PC 유틸리티도 아니다.

목표 제품:

> 사용자가 GPT, Claude, Ollama 및 데스크톱 AI를 연결하고,
> 각 AI가 볼 수 있는 데이터와 사용할 수 있는 도구를 명시적으로 통제하며,
> 여러 AI의 제안과 검토를 안전하게 개발·시스템 작업으로 연결하는
> 로컬 우선 AI 개발 런타임.

Velox는 APEX의 첫 실행 제품이다.

```text
APEX
├─ Velox Core    데이터·정책·Evidence·Council·도구·안전 엔진
├─ Velox CLI     개발자/엔지니어 인터페이스
├─ Local API     인증된 localhost 브리지
├─ Pulse         일반 사용자용 데스크톱 UI
└─ Chorus        AI Provider·모델·Council 오케스트레이션
```

### 대표 제품 흐름

v1.0 이전에는 아래 네 흐름을 제품의 중심으로 삼는다.

1. **PC Health**
   - 시스템 상태를 읽는다.
   - AI 없이 결정론적 Health Report를 먼저 생성한다.
   - 사용자가 허용한 Evidence만 AI에게 전달한다.
   - AI 설명은 사실 데이터와 분리해 표시한다.

2. **CPU & Cooling Benchmark**
   - CPU 싱글/멀티 성능을 측정한다.
   - 지속 성능 유지율과 온도 한계를 확인한다.
   - 같은 APEX benchmark version끼리만 비교한다.

3. **Snapshot / Compare**
   - 수리·업데이트 전후 상태를 저장한다.
   - 하드웨어·드라이버·전원 계획 변화를 비교한다.
   - AI 분석은 비교 결과 Evidence만 받는다.

4. **AI Development Session**
   - 사용자가 프로젝트를 선택한다.
   - APEX가 허용된 파일만 Evidence로 만든다.
   - Claude가 제안하고 GPT가 검토한다.
   - APEX가 정책·Evidence·도구 권한을 검사한다.
   - 파일 변경은 diff 미리보기와 사용자 승인 후에만 적용한다.

### 제품 밖으로 밀어둘 기능

다음 기능은 삭제하지 않지만 v1.0 대표 흐름에 포함하지 않는다.

- ETW FPS 측정
- 장기 Daemon 자동화
- AI 모델 벤치 리더보드
- 다수 Provider consensus
- 자동 드라이버 설치
- Plugin Marketplace
- 모바일 원격 제어
- BIOS 변경
- 직원/마스터키 시스템

이 기능들은 CLI 또는 Velox Ultra에 유지하고 대표 UI를 복잡하게 만들지 않는다.

## 2. 절대 보안 불변조건

아래 규칙은 일정이나 기능 편의를 이유로 깨면 안 된다.

### 2.1 Credential

- API 키는 Git, 로그, UI 응답, HTTP 응답에 노출하지 않는다.
- 신규 키는 Windows Credential Manager에 저장한다.
- `.env`는 개발/이전 호환용일 뿐 배포물에 포함하지 않는다.
- Custom Provider 키도 일반 JSON에 저장하지 않는다.
- API 키를 AI prompt에 절대 포함하지 않는다.

### 2.2 Local API

- `127.0.0.1`에만 바인딩한다.
- 앱 실행마다 랜덤 포트와 일회용 세션 토큰을 사용한다.
- 상태 변경 endpoint는 세션 인증과 명시적 사용자 동작을 요구한다.
- 브라우저 페이지 로드만으로 consent, 실행, 파일 변경을 수행하지 않는다.
- 임의 shell command endpoint를 만들지 않는다.

### 2.3 AI Data

- Provider 호출은 deny-by-default다.
- 사용자가 Provider별 Cloud 호출과 최대 데이터 범위를 승인한다.
- 실제 AI payload는 typed Evidence에서 생성한다.
- 호출자가 임의 prompt를 만든 뒤 `scope=Minimal`이라고 라벨만 붙이는 방식을
  보안으로 간주하지 않는다.
- Council 역할은 원본 시스템/프로젝트를 다시 수집하지 않는다.
- 한 AI의 실패를 다른 AI로 몰래 fallback하지 않는다.
- 어떤 Provider와 어떤 데이터가 전송됐는지 사용자에게 표시한다.

### 2.4 Tool & Action

- 도구 권한은 typed `ToolPermission` 화이트리스트로만 표현한다.
- AI가 raw shell/PowerShell 명령을 생성해 직접 실행할 수 없다.
- `WriteProject`와 `ChangeSystem`은 실행 전에 diff/계획과 사람 승인이 필요하다.
- `ChangeSystem`은 정책 설정과 무관하게 항상 사람 승인을 요구한다.
- 시스템 변경 전 가능한 경우 checkpoint를 생성한다.
- BIOS 자동 변경은 영구적으로 범위 밖이다.

### 2.5 Claims

- 가짜 벤치 점수, 다운로드 수, testimonial, 지원 Provider를 표시하지 않는다.
- 구현되지 않은 기능은 `Planned` 또는 `In Development`로 표시한다.
- APEX CPU 점수는 Geekbench 호환 점수라고 주장하지 않는다.
- 현재 checkpoint는 전체 Windows 복원이 아니라 APEX가 관리한 범위만 복원한다고 표시한다.

## 3. 아키텍처 원칙

```text
velox-core   = 데이터, 정책, 판단, 상태 전이, 안전 엔진
velox-cli    = 명령어, 출력, 대화형 승인
velox-server = 인증 HTTP/SSE 변환, 취소 신호, UI 연결
velox-app    = 네이티브 창, 서버 수명주기
site         = 표현과 사용자 입력
```

금지:

- Core에서 `println!`, HTML, dialog 입력 사용
- Server에서 제품 판단 로직 구현
- UI에서 보안 정책을 결정
- CLI subprocess의 사람용 텍스트를 Core 데이터처럼 파싱
- 같은 데이터 수집 코드를 CLI/Server/UI에 중복 구현

모든 대표 기능은 구조화된 Core 타입을 반환해야 한다.

## 4. Git·버전 운영

### 버전 규칙

- Workspace 모든 crate는 root `workspace.package.version`을 사용한다.
- 개발 중에는 이전 안정 버전을 유지한다.
- 해당 버전 Definition of Done이 모두 통과한 커밋에서만 버전을 올린다.
- Tag는 `v0.15.0`처럼 세 자리 SemVer를 사용한다.
- Tag와 GitHub Release는 같은 커밋을 가리켜야 한다.

### 커밋 규칙

```text
feat(v0.15): ...
fix(policy): ...
test(council): ...
docs(roadmap): ...
```

- 한 커밋은 빌드 가능한 논리 단위다.
- 이미 검증된 커밋을 단순히 기록을 예쁘게 만들 목적으로 재작성하지 않는다.
- `.env`, key, profile, baseline, user report를 stage하지 않는다.
- 사용자 데이터 파일을 자동 삭제하지 않는다.

### 필수 검증

각 Step 완료 시:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
git diff --check
git status --short
```

릴리스 직전 추가:

- 새 Windows 사용자 프로필에서 설치
- 일반 권한/관리자 권한 각각 실행
- API 키 없음/한 개/두 개
- 네트워크 차단
- Ollama 꺼짐
- 잘못된 policy/model/provider JSON
- 앱 강제 종료와 재실행
- Smart App Control/Defender 경고 확인
- ZIP 내부에 `.env` 및 사용자 파일이 없는지 검사

---

# Version Roadmap

> **Roadmap revision (2026-07-31):** v0.16 shipped the **Local Management Hub**, v0.17 shipped
> the metadata-only **Local Usage Ledger**, and v0.18 is the **Read-only Project Intelligence**
> release candidate. Safe Project Actions remain deferred; v0.18 never executes project
> commands or writes project files. **Accounts / multi-device sync** remain v2.0, and API keys
> are never synced — each device uses its own OS credential store.

## v0.18.0 — Read-only Project Intelligence (release candidate)

- Bounded `ProjectSession` scanning with symlink/junction escape prevention
- Secret filename filtering, token redaction, and relative-path-only Evidence
- Typed Project Evidence connected to the read-only Council
- CLI `project scan` / `project analyze`
- Authenticated local `POST /project/scan`
- Pulse Project room with explicit no-write/no-execute/no-cloud safety state
- CLI smoke test and authenticated API contract test in CI

**Not included:** automatic build/test execution, project writes, patch application, or hidden
cloud upload. Compiler/test output may be accepted later only as user-supplied Evidence or via
an explicitly approved, sandboxed job design.

## v0.17.0 — Local Usage Ledger (shipped in main)

- Metadata-only provider/model/token/session records stored locally
- No prompts, responses, API keys, or evidence content in the ledger
- User-supplied dated pricing; missing/stale data is `unknown`, never fabricated
- Provider token normalization and cached-token accounting
- Cross-process writer lock, unique IDs, and corrupt-ledger quarantine
- CLI, authenticated local API, and Settings usage surface

## v0.16.0 — Local Management Hub (shipped)

One place to manage, per provider: **API keys** (add / status / delete), **cloud consent** +
data scope + revoke, and **model IDs** (select / reset). Keys stay in the Windows Credential
Manager; values are never shown. Destructive actions (key delete, consent revoke) require an
explicit confirm. **Excluded:** session history, account server, multi-device sync, API-key sync.

## v0.15.0 — Policy-Enforced Multi-AI Foundation

### 목표

모든 대표 AI 호출이 Agent Policy를 통과하고, 승인된 Evidence만 사용하며,
Claude 제안 → GPT 검토 → 결정론적 Gate의 읽기 전용 Council을 제공한다.

### 완료된 작업

- ModelConfig와 Provider별 model ID 설정
- CLI `chorus model set/reset`
- typed ToolPermission
- Provider별 AgentPolicy
- Local/Cloud Provider 판정
- `execute_agent()` Policy Gateway
- Cloud consent/revoke CLI
- Pulse diagnose, CLI diagnose, doctor, Chorus ask의 gateway 이전
- Provider 자동 fallback 제거

### Step C.6 — Policy 우회 제거와 Evidence 기반

#### 1. Pulse consent API/UI

API:

```text
GET    /policies/status
POST   /policies/consent
DELETE /policies/:provider
```

POST 입력:

```json
{
  "provider": "claude",
  "scope": "system"
}
```

규칙:

- 앱 세션 인증 필수
- Provider 존재 검증
- scope enum 검증
- `allowed_tools=[]` 고정
- `require_confirmation=true` 고정
- 사용자의 클릭 전 자동 호출 금지
- revoke 제공
- 응답에 key 또는 전체 policy 원문 금지

Pulse 흐름:

```text
Diagnose 시작
→ 필요한 Provider와 Scope 확인
→ 미동의 Provider 표시
→ 전송 항목 미리보기
→ 사용자 동의
→ 정책 저장
→ 진단 시작
```

#### 2. 남은 직접 호출 제거

- `route_semantic`
- Chorus test
- Chorus bench
- Chorus consensus
- drivers AI analysis
- tempcheck AI advice
- fpscheck AI advice

원칙:

- Cloud AI를 호출하는 모든 제품 경로는 `execute_agent()`를 거친다.
- `route_semantic`은 동의된 Provider만 사용한다.
- 라우터 사용 불가 시 네트워크 없는 `route_model()`로 fallback한다.
- `query_text_with()`는 gateway 내부와 격리된 provider adapter test에서만 사용한다.
- 최종적으로 `query_text_with()` 가시성을 `pub(crate)`로 축소한다.

#### 3. Evidence 모듈

파일:

```text
velox-core/src/evidence.rs
```

필수 타입:

```rust
pub struct EvidenceId(pub String);

pub struct EvidenceBundle {
    pub approved_scope: ContextScope,
    pub items: Vec<EvidenceItem>,
}

pub struct EvidenceItem {
    pub id: EvidenceId,
    pub source: EvidenceSource,
    pub sensitivity: ContextScope,
    pub data: EvidenceData,
}

pub enum EvidenceSource {
    Health,
    Snapshot,
    SnapshotCompare,
    Benchmark,
    DriverScan,
    Project,
    User,
}

pub enum EvidenceData {
    Metric { name: String, value: f64, unit: String },
    Fact { name: String, value: String },
    Finding { code: String, message: String },
    Change { item: String, old: String, new: String },
    CodeFinding {
        path: String,
        line: Option<u32>,
        message: String,
    },
}
```

검증:

- 빈 bundle 거부
- 중복 EvidenceId 거부
- ID 길이와 허용 문자 검사
- item sensitivity가 approved scope를 넘으면 거부
- `serde_json::Value` 임의 payload 금지
- API key, secret, 전체 사용자 경로, 장치 serial을 Evidence로 만들지 않음
- AI prompt는 Bundle serializer만 생성

Builder:

- `HealthReport -> EvidenceBundle`
- `Snapshot -> EvidenceBundle`
- `SnapshotDiff -> EvidenceBundle`
- `CpuBenchmarkReport -> EvidenceBundle`

### Step D — Council

파일:

```text
velox-core/src/council.rs
```

Core가 담당:

- 역할 상태 전이
- Agent Policy 호출
- 구조화 응답 파싱
- Evidence 인용 검증
- 반복 횟수 제한
- 최종 CouncilDecision

Server가 담당:

- SSE 이벤트 변환
- 취소 토큰 전달
- 진행 상황 표시

기본 역할:

```text
Proposer = claude
Reviewer = gpt
Reviser  = claude
Gate     = deterministic APEX code
Approval = user
```

모델 ID는 역할에 직접 저장하지 않고 Provider 이름을 통해 `model_name()`으로 해석한다.

Council 입력:

```rust
pub struct CouncilRequest {
    pub objective: String,
    pub evidence: EvidenceBundle,
    pub approved_scope: ContextScope,
}
```

Council 역할은 v0.15에서 tool을 요청하지 않는다.

```text
requested_tools = {}
```

검토 결과:

```rust
pub enum ReviewVerdict {
    Approve,
    Revise,
    Reject,
}
```

흐름:

```text
Evidence 검증
→ Claude Proposal
→ GPT Review
→ Approve: Gate
→ Revise: Claude Revision → GPT Re-review (최대 1회)
→ Reject: 종료
→ CouncilDecision 반환
```

최종 타입:

```rust
pub enum CouncilStatus {
    Approved,
    Rejected,
    Inconclusive,
    Cancelled,
}

pub struct CouncilDecision {
    pub status: CouncilStatus,
    pub proposal: Option<TypedProposal>,
    pub reviewer_reasons: Vec<String>,
    pub evidence_used: Vec<EvidenceId>,
    pub requires_human_confirmation: bool,
}
```

결정론적 Gate:

- Proposer와 Reviewer가 정상 완료
- 기본 설정에서 서로 다른 Provider
- 구조화 JSON schema 통과
- raw command 포함 시 거부
- action은 typed whitelist
- 모든 EvidenceId가 Bundle에 존재
- action마다 최소 하나의 EvidenceId
- 요청 범위가 사용자 승인 범위 이하
- Reviewer Reject 시 즉시 종료
- Revise 반복 최대 1회
- Provider 실패 시 자동 대체 금지
- Council은 action을 실행하지 않음
- ChangeSystem은 항상 사용자 승인 표시

### Step E — Pulse Council UI

```text
System Evidence
Claude Proposal
GPT Review
Claude Revision (필요 시)
APEX Safety Gate
Final Decision
```

필수 UX:

- 현재 단계 표시
- 각 Provider/model 표시
- Provider별로 보낸 Evidence 항목 표시
- 취소 버튼
- 실패 이유 표시
- 자동 fallback이 없음을 표시
- 승인 전에는 실행 버튼 없음

### v0.15 Definition of Done

- 대표 AI 경로에 직접 `query_text_with()` 호출 없음
- Consent 없이 Cloud 네트워크 요청 0건
- 실제 payload가 EvidenceBundle에서만 생성
- Council read-only end-to-end 성공
- Reviewer Reject/Revise/timeout/cancel 테스트
- 손상 policy/evidence/model config fail closed
- 앱에서 CLI 없이 consent 가능
- 모든 crate version `0.15.0`
- README와 UI 구현 상태 일치
- v0.15.0 tag와 GitHub Release는 위 조건 이후에만 생성

---

## (reslotted, was v0.16) AI Development Workspace

### 목표

APEX를 PC 진단 프로그램에서 실제 AI 개발 도구로 확장한다.
이 버전은 **읽기 전용 프로젝트 분석**까지만 제공한다.

### 기능

#### Project Session

```rust
ProjectSession {
    root,
    language_summary,
    allowed_paths,
    ignored_paths,
    created_at,
}
```

- 사용자가 프로젝트 root를 직접 선택
- `.gitignore` 기본 존중
- `.env`, credential, key 파일 기본 제외
- 프로젝트 밖 경로 접근 거부
- symlink/junction 탈출 검사
- 최대 파일 수·크기·총 Evidence 용량 제한

#### Project Evidence

- Rust/C/C++/Python/JS/TS 우선 지원
- 파일 목록
- Cargo/package manifest
- 컴파일 오류
- 테스트 결과
- 사용자가 선택한 코드 조각
- `rg` 기반 TODO/FIXME
- 전체 저장소를 무조건 Cloud에 전송하지 않음

#### AI Project Analyzer

```text
Collect selected evidence
→ Council analysis
→ architecture / bug / debt findings
→ cited files and lines
→ no edits
```

#### Provider Routing

- 사용자가 Provider를 직접 선택 가능
- Auto routing은 consent된 Provider만 후보
- Local Ollama Provider 명확히 표시
- 모델 ID와 latency/error를 세션에 기록

### 금지

- AI 자동 파일 수정
- 임의 shell 실행
- 프로젝트 전체 zip 업로드
- background autonomous agent

### Definition of Done

- 프로젝트 밖 파일 Evidence 생성 불가
- secret pattern/redaction 테스트
- 1,000개 파일 프로젝트에서 제한 동작
- 분석 결과가 실제 EvidenceId를 인용
- Claude/GPT/Ollama 각각 단독 모드
- 네트워크 끊김 시 Local Provider 선택 가능

---

## v0.17.0 — Safe Project Actions

### 목표

AI 제안을 실제 코드 변경으로 연결하되, diff·checkpoint·승인·검증을 강제한다.

### 기능

#### Typed Project Tools

```rust
ReadProject
ProposePatch
ApplyPatch
RunAllowedCheck
RestoreProjectCheckpoint
```

`WriteProject`를 바로 주지 않고 Proposal과 Apply를 분리한다.

#### Patch Workflow

```text
CouncilDecision
→ typed patch proposal
→ path boundary validation
→ diff preview
→ user approval
→ checkpoint
→ apply
→ formatter/test
→ result
→ optional rollback
```

#### Checkpoint

- 변경 파일 원본만 보존
- Git 저장소라면 current commit/status 기록
- 사용자 기존 변경을 덮어쓰지 않음
- `git reset --hard` 사용 금지
- rollback은 APEX가 바꾼 파일만 대상으로 함

#### Allowed Checks

- Cargo: fmt/check/test/clippy
- Python: 사용자가 설정한 test command
- Node: manifest에 존재하는 script만
- raw shell은 기본 거부

### Definition of Done

- path traversal/symlink 탈출 거부
- dirty worktree 보존
- diff 승인 없이 파일 변경 0건
- 실패한 테스트 후 rollback 선택 가능
- 변경 전후 Evidence와 Audit 기록
- Council이 직접 ApplyPatch를 호출하지 못함

---

## v0.18.0 — Runtime, Sessions & Provider Platform

### 목표

CLI/App/향후 MCP가 같은 장기 실행 Core를 사용하도록 런타임을 정리한다.

### 기능

#### Job Runtime

```rust
JobId
JobState { Queued, Running, WaitingApproval, Completed, Failed, Cancelled }
ProgressEvent
CancellationToken
```

- Benchmark/Doctor/Council/Project Analysis 공통 job
- UI 종료 후 작업 정책 명확화
- 취소 전파
- timeout
- 동시 실행 제한

#### Session Store

- 사용자별이 아니라 로컬 PC profile 기준
- 민감 prompt 원문 저장은 opt-in
- 기본 저장: metadata, Provider, model, Evidence ID, 결과 상태
- key 저장 금지
- retention 설정

#### Provider Adapter

공통 trait:

```rust
trait AiProvider {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn invoke(&self, request: ProviderRequest)
        -> Result<ProviderResponse, ProviderError>;
}
```

지원:

- Anthropic
- OpenAI
- OpenAI-compatible
- Ollama/local

Provider마다 HTTP 응답 파싱을 `ai.rs` 한 함수에 계속 쌓지 않는다.

#### Desktop AI Integration

- Local endpoint 등록
- health check
- model discovery가 가능하면 read-only discovery
- Cloud/Local 명확한 표시
- Local이라고 해도 tool 권한은 자동 부여하지 않음

### Definition of Done

- Server가 CLI subprocess에 의존하지 않는 대표 기능
- 모든 장기 작업 취소 가능
- Provider adapter mock contract test
- Ollama offline end-to-end
- 세션 DB 손상 복구/격리
- 동시 benchmark와 Council 충돌 방지

---

## v0.19.0 — Product Hardening

### 목표

개발자 머신이 아닌 새 Windows PC에서 설치하고 사용할 수 있는 품질을 만든다.

### 기능

#### Config

단일 사용자 설정:

```text
%LOCALAPPDATA%\APEX\Velox\
├─ config.toml
├─ models.json
├─ policies.json
├─ providers.json
├─ sessions/
├─ reports/
└─ logs/
```

- 실행 위치에 상태 파일 저장 금지 — **velox-core 완료(`paths` 모듈). CLI/server 잔여분은 문서 끝 참고**
- config schema version
- migration
- atomic save
- corruption backup

#### Logging

- `tracing` 도입
- error/warn/info/debug 구분
- key/prompt/system serial redaction
- 회전 로그
- 사용자가 export 전에 미리보기

#### Installer/Updater

- 실제 installer
- Desktop/Start Menu shortcut
- 버전 확인
- checksum/signature 검증
- 실행 중 바이너리 교체 금지
- rollback 가능한 updater
- 다운로드 링크가 실제 asset을 가리킴

#### Error UX

- 관리자 권한 필요
- 센서 미지원
- Provider key 없음
- consent 없음
- network timeout
- local model offline
- corrupt config
- port/server startup failure

모든 에러는 “실패”뿐 아니라 다음 행동을 안내한다.

### Definition of Done

- 깨끗한 Windows 10/11 VM 설치
- uninstall 후 사용자 데이터 보존/삭제 선택
- ZIP/installer secret scan
- Release binary 서명 계획 확정
- 앱 crash 후 orphan server 없음
- Local API 보안 회귀 테스트
- README 설치 절차가 실제와 일치

---

## v0.20.0 — Closed Alpha / Real Repair Workflow

### 목표

친구 PC와 본인 장비에서 실제 사용 데이터를 얻고, 기능이 아니라 결과를 검증한다.

### 테스트 장비

- Galaxy Book 계열 노트북
- 다른 Windows 노트북
- i3-12100F 서버 시스템
- i7-14700 편집 시스템

### 실제 시나리오

```text
수리 전 Snapshot
→ PC Health
→ CPU benchmark
→ 5분 cooling test
→ 드라이버 확인
→ 재조립/업데이트
→ 같은 테스트 반복
→ Snapshot Compare
→ Repair Report export
```

### Report

- HTML/JSON 우선
- PDF는 렌더 검증 후
- APEX version과 benchmark version
- 측정 시간
- 하드웨어
- before/after
- 온도와 성능 유지율
- 드라이버 변화
- AI 해석과 결정론적 측정값 분리
- “통과/실패” 기준 설명

### Alpha 지표

- 앱 실행 성공률
- 진단 완료율
- 평균 소요 시간
- 취소율
- 센서 미지원률
- AI policy 거부 이유
- crash 수
- 잘못된 경고 수

가짜 사용자 수나 마케팅 숫자를 만들지 않는다.

### Definition of Done

- 최소 2대의 서로 다른 PC에서 전체 흐름 완료
- 측정 재현성 기록
- 사용자 1명이 개발자 도움 없이 실행
- 치명적 데이터 손실 0
- 잘못된 시스템 자동 변경 0
- 발견된 P0/P1 버그 해결

---

## v0.21.0 — Public Beta Candidate

### 목표

외부 사용자가 제한된 범위에서 안전하게 설치·사용하고 피드백을 제출할 수 있게 한다.

### 포함

- PC Health
- CPU/Cooling Benchmark
- Snapshot/Compare
- Read-only Council
- Read-only Project Analyzer
- 승인 기반 Project Patch는 실험 기능
- Provider/Model/Policy 관리
- Ollama 및 Cloud Provider

### 제외 또는 Experimental

- 시스템 자동 변경
- 드라이버 자동 설치
- background autonomous agent
- Plugin SDK
- 원격 모바일 제어

### Beta Gate

- 개인정보 처리 설명
- 로컬/클라우드 전송 구분
- crash reporting opt-in
- security contact
- known limitations
- reproducible release process
- checksum

---

## v1.0.0 — First Stable Product

### 제품 약속

> APEX Velox는 사용자가 선택한 시스템 및 프로젝트 Evidence만 AI에게 전달하고,
> 여러 AI의 제안을 정책과 근거로 검토하며,
> 모든 쓰기 작업을 미리 보여주고 승인받는 Windows AI 개발·진단 도구다.

### Stable 기능

- Native Windows App
- Secure Provider Credential Store
- Provider/Model Management
- Agent Policy
- Evidence Engine
- Claude/GPT Council
- Ollama/OpenAI-compatible Local Provider
- PC Health
- CPU/Cooling Benchmark
- Snapshot/Compare
- Read-only Project Intelligence
- Approved Project Patch workflow
- Session/Audit history
- Report export
- Installer/Updater

### v1.0 금지

- 무승인 파일 변경
- 무승인 시스템 변경
- raw AI shell execution
- BIOS write
- secret upload
- fake benchmark/marketing data
- “모든 문제 자동 해결” 주장

### Stable Release Gate

- 모든 v0.19~v0.21 Definition of Done 통과
- P0/P1 known bug 0
- security boundary test
- dependency audit
- signed release 또는 서명 미완료를 명시한 제한 배포
- clean machine install/uninstall
- upgrade from previous release
- rollback test
- docs and UI truth audit
- tag/release/checksum 일치

---

# Post-1.0 Roadmap

## v1.1 — Performance Intelligence

- 게임/개발/AI/편집 용도별 해석
- 장기 성능 Timeline
- regression detection
- 사용자의 개인 baseline 중심

## v1.2 — APEX Runtime Core

- 안정적인 background service
- Event Bus
- scheduled read-only monitoring
- anomaly notification
- 자동 action은 계속 opt-in

## v1.3 — Plugin SDK Preview

- signed/manifested plugin
- capability declaration
- sandbox/process isolation
- Provider와 tool plugin 분리
- Marketplace는 SDK 보안 검증 후

## v1.4 — Remote Status Preview

- 기본 비활성
- end-to-end encryption
- read-only device health
- 사용자가 직접 pairing
- 인터넷 공개 포트 금지

## v2.0 — APEX Platform

- CLI, Pulse, MCP, plugin이 같은 Core 사용
- 여러 기기와 AI Provider를 연결
- 조직/팀 기능은 개인용 보안 모델이 안정된 뒤

---

# Claude 작업 방식

## 매 Step 시작 전

1. `git status` 확인
2. 이전 커밋과 테스트 확인
3. 이번 Step의 write scope 선언
4. 관련 타입과 호출 경로 검색
5. 구현하지 않을 항목 명시

## 구현 중

- 새 기능보다 기존 Core 재사용
- 보안 판단은 UI가 아닌 Core
- 네트워크 호출은 timeout
- 파일 저장은 atomic
- 실패는 typed error
- 자동 fallback은 명시적 정책 없이는 금지
- 사용자 데이터는 최소 수집

## 완료 보고 형식

```text
[Claude → GPT] vX.Y Step Z 완료

commit:
changed:
security impact:
tests:
clippy:
remaining direct/bypass paths:
known limitations:
decisions needed:
push/tag/release status:
```

단순히 “테스트 통과”만 보고하지 말고 아직 우회 가능한 경로와 구현하지 않은 부분도 함께 보고한다.

## 중단하고 확인해야 하는 경우

- 데이터 삭제
- Git history rewrite
- API key migration 실패
- 외부 push/release
- system-changing action 추가
- 새로운 Cloud endpoint
- 로그인/계정 체계
- 원격 접속
- BIOS/driver write
- 기존 사용자 파일 형식의 비호환 변경

# 최우선 다음 작업

*(2026-08-19 갱신 — v0.15~v0.18 은 전부 완료·게시됨)*

현재 위치: **v0.19.0 Product Hardening**. 목표는 "개발자 머신이 아닌 새 Windows PC에서
설치하고 쓸 수 있는 품질"이다. 아래 순서만 따른다.

```text
1. [완료] Config 위치 분리 — 상태 파일을 실행 위치(CWD)에서 분리
          velox-core::paths → %LOCALAPPDATA%\APEX\Velox (VELOX_DATA_DIR 로 override)
          레거시 파일 자동 이전 포함
2. Logging — tracing 도입, key/prompt/serial 레닥션, 회전 로그
3. Error UX — 모든 실패가 "다음 행동"을 안내
4. Installer/Updater — 실제 인스톨러, 바로가기, checksum 검증, rollback
5. 코드 서명 계획 확정  ← 태경 결정 사항(비용·본인확인)
6. v0.19 RC 검증(깨끗한 VM 설치·uninstall·secret scan) → 태그 → 릴리스
```

## 2인 엔지니어 분담 (2026-07-31 체계)

```text
Claude : velox-core  — 데이터 모델·AI 실행 경로·정책·성능
GPT    : velox-cli / velox-server / velox-app / site / .github / scripts
태경    : 제품 방향·승인·외부 결정(서명·과금)
```

작업 전 `git fetch` + `pull --ff-only`. 공유 파일(lib.rs / Cargo.toml / Cargo.lock / README)
변경 시 커밋 메시지에 명시. **CI 실패는 최우선 수정.**

## GPT 담당으로 남은 CWD 상태 파일

velox-core 쪽은 `paths` 로 전부 이전했다. 아래는 CLI/server 소유라 손대지 않았다 —
같은 방식으로 `velox_core::paths::resolve()` 를 쓰면 된다.

```text
velox-cli/src/daemon.rs    velox_daemon.log
velox-cli/src/diagnose.rs  velox_actions.log
velox-cli/src/timeline.rs  velox_timeline.csv
velox-server/src/main.rs   velox_baseline.json · velox_profile.json
```

로그는 `%LOCALAPPDATA%\APEX\Velox\logs\`, 리포트는 `reports\` 로 가는 게
v0.19 Config 설계와 맞는다.

## 시작하지 않는 것

v0.19 범위 밖(Safe Project Actions, Plugin SDK, 계정/동기화)은 시작하지 않는다.
보안 불변조건을 완화하지 않는다. push/tag/release 는 게이트 통과 후에만.
