//! velox-core::guidance — 실패를 "다음 행동"으로 바꾸는 계층.
//!
//! v0.19 Error UX 요구사항: **모든 에러는 "실패"뿐 아니라 다음 행동을 안내한다.**
//!
//! v0.18까지는 실패 문구가 CLI 각지에 문자열로 흩어져 있었다. 어떤 곳은
//! 해결 방법을 알려주고(chorus consent 안내) 어떤 곳은 그냥 "실패"만 찍었다.
//! 이 모듈은 그 판단을 **엔진 쪽에 typed 로 모은다** — CLI·서버·Pulse 가
//! 같은 문제에 같은 안내를 하도록 한다.
//!
//! 원칙: 하나의 실패는 세 가지에 답해야 한다.
//! 1. 무슨 일이 일어났나 (summary)
//! 2. 왜 (cause)
//! 3. **다음에 뭘 하면 되나** (next) ← 이게 없으면 안내가 아니다

use crate::policy::PolicyError;
use crate::privacy::ContextScope;
use crate::project::ProjectError;

/// 사용자가 받는 안내. 표시는 호출자가 한다(엔진은 데이터만 반환).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guidance {
    /// 무슨 일이 일어났는지 한 줄.
    pub summary: String,
    /// 왜 그런지. 몰라도 되면 None.
    pub cause: Option<String>,
    /// 다음 행동. **비어 있으면 안 된다** — 안내의 존재 이유다.
    pub next: Vec<String>,
    pub severity: Severity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 정상 동작이지만 알려줄 필요가 있음(예: deny-by-default 로 차단).
    Info,
    /// 기능이 제한되지만 계속 쓸 수 있음.
    Warning,
    /// 이 작업은 진행 불가.
    Blocked,
}

impl Severity {
    /// 표시용 기호. 색상은 호출자가 정한다.
    pub fn marker(&self) -> &str {
        match self {
            Severity::Info => "•",
            Severity::Warning => "⚠",
            Severity::Blocked => "✗",
        }
    }
}

/// 제품에서 실제로 발생하는 실패 유형. v0.19 Error UX 목록을 그대로 덮는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// 관리자 권한이 있어야 읽을 수 있는 자원.
    AdminRequired { what: String },
    /// 보드·드라이버가 해당 센서를 노출하지 않음.
    SensorUnsupported { what: String },
    /// provider 의 API 키가 자격증명 저장소에 없음.
    ProviderKeyMissing { provider: String },
    /// 클라우드 호출 미동의(deny-by-default). **정상 동작이다.**
    ConsentMissing {
        provider: String,
        needed_scope: ContextScope,
    },
    /// 동의는 했으나 요청 데이터 범위가 승인 범위를 넘음.
    ScopeTooLow {
        provider: String,
        requested: ContextScope,
        max: ContextScope,
    },
    /// 알 수 없는 provider 이름.
    UnknownProvider { provider: String },
    /// 네트워크 타임아웃.
    NetworkTimeout { provider: String, seconds: u64 },
    /// 로컬 모델 엔드포인트에 연결 불가(Ollama 등).
    LocalModelOffline { endpoint: String },
    /// 설정/상태 파일이 손상됨. fail-closed 로 기본값을 쓴 상태.
    CorruptConfig { file: String },
    /// 로컬 서버 기동 실패.
    ServerStartFailed { reason: String },
    /// 프로젝트 스캔 거부(경로 탈출·비밀파일·한도).
    ProjectRejected {
        detail: String,
        kind: ProjectRejection,
    },
    /// provider 호출 자체가 실패(정책은 통과).
    ProviderCallFailed { provider: String, detail: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRejection {
    NotADirectory,
    OutsideRoot,
    SecretExcluded,
    TooLarge,
    Io,
}

fn scope_flag(scope: ContextScope) -> &'static str {
    match scope {
        ContextScope::Minimal => "minimal",
        ContextScope::System => "system",
        ContextScope::Drivers => "drivers",
    }
}

impl Problem {
    /// 이 문제에 대한 안내를 만든다.
    pub fn guidance(&self) -> Guidance {
        match self {
            Problem::AdminRequired { what } => Guidance {
                summary: format!("{what} 읽기에 관리자 권한이 필요합니다"),
                cause: Some("이 값은 커널 수준 접근이 있어야 노출됩니다".into()),
                next: vec![
                    "터미널을 관리자 권한으로 실행한 뒤 같은 명령을 다시 실행하세요".into(),
                    "앱에서는 velox-app.exe 우클릭 → 관리자 권한으로 실행".into(),
                ],
                severity: Severity::Warning,
            },
            Problem::SensorUnsupported { what } => Guidance {
                summary: format!("{what} 센서를 이 시스템에서 읽을 수 없습니다"),
                cause: Some(
                    "메인보드나 드라이버가 해당 값을 노출하지 않습니다. 노트북·일부 보드에서 흔합니다"
                        .into(),
                ),
                next: vec![
                    "관리자 권한으로 한 번 더 시도해 보세요".into(),
                    "메인보드 칩셋 드라이버를 최신으로 업데이트해 보세요".into(),
                    "그래도 안 되면 이 값 없이 계속 쓸 수 있습니다 — 다른 지표는 정상 수집됩니다".into(),
                ],
                severity: Severity::Warning,
            },
            Problem::ProviderKeyMissing { provider } => Guidance {
                summary: format!("{provider} 의 API 키가 등록되어 있지 않습니다"),
                cause: Some(
                    "키는 Windows 자격증명 관리자에만 저장되며 기기마다 따로 등록해야 합니다".into(),
                ),
                next: vec![
                    format!("velox chorus set {provider} <키>"),
                    "또는 앱 Settings 탭의 provider 카드에서 등록".into(),
                    "키 없이 쓸 수 있는 기능: bench · drivers · gpu · thermals · snapshot".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::ConsentMissing {
                provider,
                needed_scope,
            } => Guidance {
                summary: format!("{provider} 는 클라우드 호출에 아직 동의하지 않았습니다"),
                cause: Some(
                    "APEX 는 deny-by-default 입니다 — 동의 전에는 어떤 데이터도 나가지 않습니다. 정상 동작입니다"
                        .into(),
                ),
                next: vec![
                    format!(
                        "velox chorus consent {provider} --scope {}",
                        scope_flag(*needed_scope)
                    ),
                    "또는 앱에서 진단 시작 시 뜨는 동의 화면에서 전송 항목을 확인하고 동의".into(),
                    format!("되돌리려면: velox chorus revoke {provider}"),
                ],
                severity: Severity::Info,
            },
            Problem::ScopeTooLow {
                provider,
                requested,
                max,
            } => Guidance {
                summary: format!(
                    "{provider} 에 승인된 데이터 범위({})로는 이 기능을 쓸 수 없습니다",
                    scope_flag(*max)
                ),
                cause: Some(format!("이 작업은 {} 범위가 필요합니다", scope_flag(*requested))),
                next: vec![
                    format!(
                        "velox chorus consent {provider} --scope {}",
                        scope_flag(*requested)
                    ),
                    "동의 전에 어떤 항목이 전송되는지 먼저 확인할 수 있습니다".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::UnknownProvider { provider } => Guidance {
                summary: format!("{provider} 는 등록된 provider 가 아닙니다"),
                cause: None,
                next: vec![
                    "내장: claude · gpt · gemini · grok".into(),
                    "velox chorus models 로 현재 목록 확인".into(),
                    "커스텀(OpenAI 호환)은 velox chorus add 로 등록".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::NetworkTimeout { provider, seconds } => Guidance {
                summary: format!("{provider} 응답이 {seconds}초 안에 오지 않았습니다"),
                cause: Some("네트워크가 끊겼거나 provider 가 지연되고 있습니다".into()),
                next: vec![
                    "인터넷 연결을 확인하고 다시 시도하세요".into(),
                    "계속 느리면 다른 provider 로 바꿔 보세요 (velox chorus models)".into(),
                    "오프라인에서는 키 없이 되는 기능(bench · snapshot · drivers)을 쓸 수 있습니다".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::LocalModelOffline { endpoint } => Guidance {
                summary: format!("로컬 모델 엔드포인트에 연결할 수 없습니다 ({endpoint})"),
                cause: Some("로컬 서버가 꺼져 있거나 주소·포트가 다릅니다".into()),
                next: vec![
                    "Ollama 라면: ollama serve 로 실행 중인지 확인".into(),
                    "모델이 받아져 있는지 확인: ollama list".into(),
                    "주소를 바꾸려면 velox chorus add 로 등록한 엔드포인트를 확인하세요".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::CorruptConfig { file } => Guidance {
                summary: format!("설정 파일이 손상되어 안전 기본값으로 되돌렸습니다 ({file})"),
                cause: Some(
                    "손상된 설정을 그대로 쓰면 권한이 잘못 열릴 수 있어 거부 쪽으로 닫습니다(fail-closed)"
                        .into(),
                ),
                next: vec![
                    "동의·모델 설정이 초기화됐을 수 있습니다 — velox chorus models 로 확인하세요".into(),
                    "필요하면 다시 동의: velox chorus consent <provider> --scope <범위>".into(),
                    "손상 파일은 옆에 백업으로 남겨둡니다".into(),
                ],
                severity: Severity::Warning,
            },
            Problem::ServerStartFailed { reason } => Guidance {
                summary: "로컬 엔진(velox-server)을 시작하지 못했습니다".into(),
                cause: Some(reason.clone()),
                next: vec![
                    "velox-server.exe 가 velox-app.exe 와 같은 폴더에 있는지 확인하세요".into(),
                    "이미 실행 중인 APEX 창이 있으면 닫고 다시 여세요".into(),
                    "백신·방화벽이 localhost 연결을 막지 않는지 확인하세요".into(),
                ],
                severity: Severity::Blocked,
            },
            Problem::ProjectRejected { detail, kind } => {
                let (cause, next) = match kind {
                    ProjectRejection::NotADirectory => (
                        "프로젝트 root 는 폴더여야 합니다",
                        vec!["파일이 아니라 프로젝트 폴더 경로를 지정하세요".to_string()],
                    ),
                    ProjectRejection::OutsideRoot => (
                        "프로젝트 밖 경로는 읽지 않습니다(경로 탈출·symlink 포함)",
                        vec![
                            "프로젝트 폴더 안의 상대 경로만 사용하세요".to_string(),
                            "밖의 파일이 필요하면 프로젝트 root 를 더 위로 잡으세요".to_string(),
                        ],
                    ),
                    ProjectRejection::SecretExcluded => (
                        "비밀로 취급되는 파일은 경로가 유효해도 열지 않습니다",
                        vec![
                            "env · pem · key · id_rsa · secret · apikey 계열은 설계상 제외됩니다"
                                .to_string(),
                            "이 제한은 해제할 수 없습니다 — 키가 AI 로 전송되는 것을 구조적으로 막습니다"
                                .to_string(),
                        ],
                    ),
                    ProjectRejection::TooLarge => (
                        "파일이 Evidence 한도를 초과합니다",
                        vec![
                            "분석할 파일을 좁혀서 다시 시도하세요".to_string(),
                            "한도는 전송량과 비용을 통제하기 위한 것입니다".to_string(),
                        ],
                    ),
                    ProjectRejection::Io => (
                        "파일을 읽지 못했습니다",
                        vec![
                            "경로가 맞는지, 다른 프로그램이 파일을 잠그고 있지 않은지 확인하세요"
                                .to_string(),
                        ],
                    ),
                };
                Guidance {
                    summary: format!("프로젝트 스캔이 거부됐습니다: {detail}"),
                    cause: Some(cause.into()),
                    next,
                    severity: Severity::Blocked,
                }
            }
            Problem::ProviderCallFailed { provider, detail } => Guidance {
                summary: format!("{provider} 호출이 실패했습니다"),
                cause: Some(detail.clone()),
                next: vec![
                    "키가 유효한지 확인하세요 (폐기된 키일 수 있습니다)".into(),
                    format!("velox chorus test {provider} 로 연결만 따로 검증"),
                    "provider 상태 페이지에서 장애 여부를 확인하세요".into(),
                ],
                severity: Severity::Blocked,
            },
        }
    }
}

impl From<&PolicyError> for Problem {
    fn from(e: &PolicyError) -> Self {
        match e {
            PolicyError::UnknownProvider(p) => Problem::UnknownProvider {
                provider: p.clone(),
            },
            PolicyError::CloudNotAllowed(p) => Problem::ConsentMissing {
                provider: p.clone(),
                needed_scope: ContextScope::Minimal,
            },
            PolicyError::ScopeExceeded { requested, max } => Problem::ScopeTooLow {
                provider: "provider".into(),
                requested: *requested,
                max: *max,
            },
            PolicyError::ToolNotAllowed(t) => Problem::ProjectRejected {
                detail: format!("{t:?}"),
                kind: ProjectRejection::OutsideRoot,
            },
            PolicyError::ProviderCallFailed(p) => Problem::ProviderCallFailed {
                provider: p.clone(),
                detail: p.clone(),
            },
        }
    }
}

impl From<&ProjectError> for Problem {
    fn from(e: &ProjectError) -> Self {
        let (detail, kind) = match e {
            ProjectError::NotADirectory(p) => (p.clone(), ProjectRejection::NotADirectory),
            ProjectError::OutsideRoot(p) => (p.clone(), ProjectRejection::OutsideRoot),
            ProjectError::SecretExcluded(p) => (p.clone(), ProjectRejection::SecretExcluded),
            ProjectError::TooLarge(p) => (p.clone(), ProjectRejection::TooLarge),
            ProjectError::Io(p) => (p.clone(), ProjectRejection::Io),
        };
        Problem::ProjectRejected { detail, kind }
    }
}

impl Guidance {
    /// 터미널용 여러 줄 텍스트. 색상 없음 — 색은 호출자가 입힌다.
    pub fn render_plain(&self) -> String {
        let mut out = format!("{} {}", self.severity.marker(), self.summary);
        if let Some(c) = &self.cause {
            out.push_str(&format!("\n  이유: {c}"));
        }
        if !self.next.is_empty() {
            out.push_str("\n  다음 행동:");
            for n in &self.next {
                out.push_str(&format!("\n    - {n}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_problems() -> Vec<Problem> {
        vec![
            Problem::AdminRequired {
                what: "CPU 온도".into(),
            },
            Problem::SensorUnsupported {
                what: "CPU 온도".into(),
            },
            Problem::ProviderKeyMissing {
                provider: "claude".into(),
            },
            Problem::ConsentMissing {
                provider: "claude".into(),
                needed_scope: ContextScope::System,
            },
            Problem::ScopeTooLow {
                provider: "claude".into(),
                requested: ContextScope::System,
                max: ContextScope::Minimal,
            },
            Problem::UnknownProvider {
                provider: "nope".into(),
            },
            Problem::NetworkTimeout {
                provider: "gpt".into(),
                seconds: 90,
            },
            Problem::LocalModelOffline {
                endpoint: "http://localhost:11434".into(),
            },
            Problem::CorruptConfig {
                file: "velox_policies.json".into(),
            },
            Problem::ServerStartFailed {
                reason: "포트 사용 중".into(),
            },
            Problem::ProjectRejected {
                detail: ".env".into(),
                kind: ProjectRejection::SecretExcluded,
            },
            Problem::ProviderCallFailed {
                provider: "gpt".into(),
                detail: "401".into(),
            },
        ]
    }

    /// v0.19 의 핵심 요구 — 안내 없는 실패는 없다.
    #[test]
    fn every_problem_offers_a_next_action() {
        for p in all_problems() {
            let g = p.guidance();
            assert!(!g.summary.is_empty(), "{p:?} 요약 없음");
            assert!(!g.next.is_empty(), "{p:?} 다음 행동 없음 — Error UX 위반");
            for n in &g.next {
                assert!(!n.trim().is_empty(), "{p:?} 빈 안내 항목");
            }
        }
    }

    /// 모든 프로젝트 거부 사유가 고유한 안내를 갖는지.
    #[test]
    fn every_project_rejection_has_its_own_guidance() {
        let kinds = [
            ProjectRejection::NotADirectory,
            ProjectRejection::OutsideRoot,
            ProjectRejection::SecretExcluded,
            ProjectRejection::TooLarge,
            ProjectRejection::Io,
        ];
        let mut seen = Vec::new();
        for k in kinds {
            let g = Problem::ProjectRejected {
                detail: "x".into(),
                kind: k,
            }
            .guidance();
            assert!(!g.next.is_empty());
            assert!(!seen.contains(&g.cause), "거부 사유별 안내가 중복됨: {k:?}");
            seen.push(g.cause);
        }
    }

    #[test]
    fn consent_missing_is_info_not_error() {
        // deny-by-default 는 고장이 아니라 설계다. 사용자를 놀라게 하면 안 된다.
        let g = Problem::ConsentMissing {
            provider: "claude".into(),
            needed_scope: ContextScope::System,
        }
        .guidance();
        assert_eq!(g.severity, Severity::Info);
        assert!(
            g.next
                .iter()
                .any(|n| n.contains("chorus consent claude --scope system"))
        );
    }

    #[test]
    fn secret_exclusion_is_not_presented_as_fixable() {
        let g = Problem::ProjectRejected {
            detail: ".env".into(),
            kind: ProjectRejection::SecretExcluded,
        }
        .guidance();
        assert!(g.next.iter().any(|n| n.contains("해제할 수 없습니다")));
    }

    #[test]
    fn blocked_problems_are_marked_blocked() {
        let g = Problem::ProviderKeyMissing {
            provider: "claude".into(),
        }
        .guidance();
        assert_eq!(g.severity, Severity::Blocked);
        assert_eq!(g.severity.marker(), "✗");
    }

    #[test]
    fn policy_error_maps_to_problem() {
        let p: Problem = (&PolicyError::CloudNotAllowed("gpt".into())).into();
        assert!(matches!(p, Problem::ConsentMissing { .. }));
        let p: Problem = (&PolicyError::UnknownProvider("x".into())).into();
        assert!(matches!(p, Problem::UnknownProvider { .. }));
    }

    #[test]
    fn project_error_maps_to_problem() {
        let p: Problem = (&ProjectError::SecretExcluded(".env".into())).into();
        assert!(matches!(
            p,
            Problem::ProjectRejected {
                kind: ProjectRejection::SecretExcluded,
                ..
            }
        ));
    }

    #[test]
    fn render_includes_all_three_parts() {
        let out = Problem::ProviderKeyMissing {
            provider: "claude".into(),
        }
        .guidance()
        .render_plain();
        assert!(out.contains("API 키가 등록되어 있지 않습니다"));
        assert!(out.contains("이유:"));
        assert!(out.contains("다음 행동:"));
        assert!(out.contains("velox chorus set claude"));
    }

    /// 안내에 키·프롬프트 같은 민감값이 섞이지 않는지(레닥션 회귀 방지).
    #[test]
    fn guidance_never_embeds_secrets() {
        for p in all_problems() {
            let out = p.guidance().render_plain();
            for pat in ["sk-ant-", "sk-proj-", "AIza", "xai-"] {
                assert!(!out.contains(pat), "{p:?} 안내에 키 패턴 포함: {pat}");
            }
        }
    }
}
