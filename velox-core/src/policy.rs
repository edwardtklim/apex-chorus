//! velox-core::policy — Agent Policy + 정책 게이트웨이 (Step C).
//!
//! 각 provider가 **무엇을 볼 수 있고 / 무엇을 할 수 있고 / 클라우드로 나가도 되는지 /
//! 부수효과 전에 사람 승인이 필요한지**를 결정론적으로 규정하고 강제한다.
//!
//! 원칙: **deny-by-default, fail-closed.** 명시적으로 부여하지 않은 권한은 닫혀 있고,
//! 알 수 없는 provider/scope/tool이나 손상된 정책 파일은 절대 permissive로 폴백하지 않는다.
//!
//! 강제 지점은 [`execute_agent`] — 정책 검사를 통과해야만 `query_text_with`를 호출한다.
//! 기존 `query_text_with` 직접 경로는 이 단계에서 유지되고, 새 경로만 게이트를 탄다.
//! (Council 역할 정책과 Settings UI는 아직 구현하지 않음.)

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::privacy::ContextScope;

/// provider별 정책 저장 파일.
pub const POLICIES_FILE: &str = "velox_policies.json";

/// 에이전트가 호출할 수 있는 **능력(툴)**. 정책 화이트리스트의 원소.
/// 미지의 문자열은 역직렬화 단계에서 **에러**가 되어 fail-closed 된다(폴백 없음).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermission {
    ReadHealth,
    ReadSnapshot,
    CompareSnapshot,
    ReadDrivers,
    RunCpuBenchmark,
    ReadProject,
    WriteProject,
    ChangeSystem,
}

impl ToolPermission {
    /// 부수효과(쓰기/시스템 변경)를 내는 툴인가 — 실행 직전 사람 승인 대상.
    pub fn is_side_effect(self) -> bool {
        matches!(
            self,
            ToolPermission::WriteProject | ToolPermission::ChangeSystem
        )
    }
}

/// provider가 로컬(오프라인/사설)인지 클라우드(원격)인지.
/// 기본값은 **Cloud** — 확실히 로컬임이 증명될 때만 Local로 낮춘다(fail-closed).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLocation {
    Local,
    #[default]
    Cloud,
}

/// 한 provider에 걸리는 권한 정책. `#[serde(default)]` 이므로 부분 JSON도 나머지는
/// 안전 기본값(deny)으로 채워진다 — 부분 정책이 기본값을 상향(elevate)하지 못한다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentPolicy {
    /// 허용되는 **최대 데이터 범위** (privacy의 `ContextScope` 재사용).
    pub max_context_scope: ContextScope,
    /// 호출 가능한 **툴 화이트리스트**. 비어 있으면 툴 없음(읽기·추론만).
    pub allowed_tools: BTreeSet<ToolPermission>,
    /// **클라우드(원격)** provider 호출 허용 여부. false면 로컬만.
    pub allow_cloud: bool,
    /// 부수효과 툴 실행 전에 **사람 승인**을 요구할지.
    pub require_confirmation: bool,
}

impl Default for AgentPolicy {
    /// 안전 우선 기본값: 최소 데이터 · 툴 없음 · 클라우드 불가 · 승인 필수.
    fn default() -> Self {
        Self {
            max_context_scope: ContextScope::Minimal,
            allowed_tools: BTreeSet::new(),
            allow_cloud: false,
            require_confirmation: true,
        }
    }
}

impl AgentPolicy {
    /// 이 정책이 주어진 툴 호출을 허용하는지 (화이트리스트 검사).
    pub fn permits_tool(&self, tool: ToolPermission) -> bool {
        self.allowed_tools.contains(&tool)
    }
}

// ---------------- 정책 저장소 (provider별) ----------------

/// provider 이름 → 정책. 파일에 없는 provider는 `AgentPolicy::default()`(deny)로 해석.
pub type PolicyStore = BTreeMap<String, AgentPolicy>;

/// 정책 파일 로드. 없거나 **손상되면 빈 맵**(= 모든 provider deny) — permissive 폴백 금지.
pub fn load_policies() -> PolicyStore {
    match std::fs::read_to_string(POLICIES_FILE) {
        // 파싱 실패(손상/미지 토큰)는 빈 맵으로 — 안전한 deny 기본으로 닫는다.
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => PolicyStore::new(),
    }
}

/// 정책 저장소를 원자적으로 저장.
pub fn save_policies(store: &PolicyStore) -> bool {
    serde_json::to_string_pretty(store)
        .ok()
        .and_then(|s| crate::ai::atomic_write(POLICIES_FILE, &s).ok())
        .is_some()
}

/// 내장 provider 별칭을 표준 이름으로. 커스텀 이름은 그대로(대소문자 보존).
fn canonical_provider(provider: &str) -> String {
    match provider.to_lowercase().as_str() {
        "claude" | "anthropic" => "claude".to_string(),
        "gpt" | "openai" => "gpt".to_string(),
        "gemini" | "google" => "gemini".to_string(),
        "grok" | "xai" => "grok".to_string(),
        _ => provider.to_string(),
    }
}

/// 이 provider의 정책 — 없으면 안전 기본값(deny).
pub fn policy_for(provider: &str) -> AgentPolicy {
    load_policies()
        .get(&canonical_provider(provider))
        .cloned()
        .unwrap_or_default()
}

/// UI/CLI의 **명시적 사용자 동의** 후에만 호출 — 이 provider에 클라우드 호출을 연다.
/// 정책 파일이 없다고 자동으로 열리지 않는다(이 함수만이 연다). 저장값:
/// `allow_cloud=true`, `max_context_scope=scope`, `allowed_tools=[]`, `require_confirmation=true`.
pub fn grant_consent(provider: &str, scope: ContextScope) -> bool {
    let mut store = load_policies();
    store.insert(
        canonical_provider(provider),
        AgentPolicy {
            allow_cloud: true,
            max_context_scope: scope,
            allowed_tools: BTreeSet::new(),
            require_confirmation: true,
        },
    );
    save_policies(&store)
}

/// 동의 철회 — provider 정책을 제거해 deny-by-default로 되돌린다.
pub fn revoke_consent(provider: &str) -> bool {
    let mut store = load_policies();
    store.remove(&canonical_provider(provider));
    save_policies(&store)
}

fn is_builtin(provider: &str) -> bool {
    matches!(
        provider.to_lowercase().as_str(),
        "claude" | "anthropic" | "gpt" | "openai" | "gemini" | "google" | "grok" | "xai"
    )
}

/// provider가 알려진 이름인지 (내장 또는 등록된 커스텀).
pub fn provider_exists(provider: &str) -> bool {
    is_builtin(provider)
        || crate::ai::load_providers()
            .iter()
            .any(|x| x.name == provider)
}

/// base_url의 host가 loopback인지 엄격 판정 (부분일치 아님 — fail-closed).
fn is_loopback(base_url: &str) -> bool {
    let after_scheme = base_url.split("://").nth(1).unwrap_or(base_url);
    let host_port = after_scheme.split('/').next().unwrap_or("");
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or("") // IPv6 [::1]:port
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// provider의 위치. 내장 API는 항상 Cloud. 커스텀은 기본 Cloud;
/// **명시적 `local` 플래그 + loopback 엔드포인트**일 때만 Local.
pub fn provider_location(provider: &str) -> ProviderLocation {
    if is_builtin(provider) {
        return ProviderLocation::Cloud;
    }
    if let Some(cfg) = crate::ai::load_providers()
        .into_iter()
        .find(|x| x.name == provider)
        && cfg.local
        && is_loopback(&cfg.base_url)
    {
        return ProviderLocation::Local;
    }
    ProviderLocation::Cloud
}

// ---------------- 게이트웨이 (요청/응답/에러) ----------------

/// 에이전트 호출의 목적 — 감사/로깅/라우팅용 분류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentPurpose {
    Diagnose,
    Propose,
    Review,
    Revise,
    Consensus,
    Other,
}

/// 정책 게이트를 통과해야 하는 에이전트 요청.
#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub provider: String,
    pub purpose: AgentPurpose,
    pub prompt: String,
    /// 이 요청이 필요로 하는 데이터 범위 (정책의 max와 비교).
    pub scope: ContextScope,
    /// 이 요청이 호출하려는 툴들.
    pub requested_tools: BTreeSet<ToolPermission>,
}

/// 게이트 통과 후 실제 호출 결과.
#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub provider: String,
    pub text: String,
    /// 실행 직전 **사람 승인이 필요한** 부수효과 툴들.
    pub tools_requiring_confirmation: BTreeSet<ToolPermission>,
}

/// 정책 위반 또는 호출 실패.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    /// 알 수 없는 provider.
    UnknownProvider(String),
    /// Cloud provider인데 정책이 클라우드 호출을 불허.
    CloudNotAllowed(String),
    /// 요청 scope가 정책의 최대 scope를 초과.
    ScopeExceeded {
        requested: ContextScope,
        max: ContextScope,
    },
    /// 화이트리스트에 없는 툴 요청.
    ToolNotAllowed(ToolPermission),
    /// 정책은 통과했으나 실제 provider 호출이 실패.
    ProviderCallFailed(String),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::UnknownProvider(p) => write!(f, "알 수 없는 provider: {p}"),
            PolicyError::CloudNotAllowed(p) => {
                write!(f, "정책상 클라우드 호출 불가: {p} (allow_cloud=false)")
            }
            PolicyError::ScopeExceeded { requested, max } => {
                write!(f, "데이터 범위 초과: 요청 {requested:?} > 허용 {max:?}")
            }
            PolicyError::ToolNotAllowed(t) => write!(f, "허용되지 않은 툴: {t:?}"),
            PolicyError::ProviderCallFailed(p) => write!(f, "provider 호출 실패: {p}"),
        }
    }
}

impl std::error::Error for PolicyError {}

/// 정책 검사 결과 — 어떤 부수효과 툴이 사람 승인을 요구하는지 포함.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authorization {
    pub tools_requiring_confirmation: BTreeSet<ToolPermission>,
}

/// **순수** 정책 검사(파일·네트워크 없음) — 테스트/재사용을 위해 정책·위치를 인자로 받는다.
/// 검사 순서: location/allow_cloud → context scope → requested tools.
/// 통과하면 승인 필요한 부수효과 툴 집합을 돌려준다.
pub fn authorize_with(
    provider: &str,
    policy: &AgentPolicy,
    location: ProviderLocation,
    req_scope: ContextScope,
    requested_tools: &BTreeSet<ToolPermission>,
) -> Result<Authorization, PolicyError> {
    // 2) location / allow_cloud
    if location == ProviderLocation::Cloud && !policy.allow_cloud {
        return Err(PolicyError::CloudNotAllowed(provider.to_string()));
    }
    // 3) context scope (Minimal < System < Drivers)
    if req_scope > policy.max_context_scope {
        return Err(PolicyError::ScopeExceeded {
            requested: req_scope,
            max: policy.max_context_scope,
        });
    }
    // 4) requested tools
    for tool in requested_tools {
        if !policy.permits_tool(*tool) {
            return Err(PolicyError::ToolNotAllowed(*tool));
        }
    }
    // 6) require_confirmation: AI 호출을 막지 않고, 부수효과 툴에 승인 필요를 표시.
    let tools_requiring_confirmation = if policy.require_confirmation {
        requested_tools
            .iter()
            .copied()
            .filter(|t| t.is_side_effect())
            .collect()
    } else {
        BTreeSet::new()
    };
    Ok(Authorization {
        tools_requiring_confirmation,
    })
}

/// 요청에 대한 전체 정책 검사. 순서: provider 존재 → location/allow_cloud → scope → tools.
pub fn authorize(req: &AgentRequest) -> Result<Authorization, PolicyError> {
    // 1) provider 존재
    if !provider_exists(&req.provider) {
        return Err(PolicyError::UnknownProvider(req.provider.clone()));
    }
    let policy = policy_for(&req.provider);
    let location = provider_location(&req.provider);
    authorize_with(
        &req.provider,
        &policy,
        location,
        req.scope,
        &req.requested_tools,
    )
}

/// 정책 게이트웨이: `authorize` 통과 → `query_text_with` 호출.
/// 검사에 실패하면 AI를 **호출하지 않는다** (5단계: 마지막이 query_text_with).
pub async fn execute_agent(req: AgentRequest) -> Result<AgentResponse, PolicyError> {
    let auth = authorize(&req)?;
    match crate::ai::query_text_with(&req.provider, &req.prompt).await {
        Some(text) => Ok(AgentResponse {
            provider: req.provider,
            text,
            tools_requiring_confirmation: auth.tools_requiring_confirmation,
        }),
        None => Err(PolicyError::ProviderCallFailed(req.provider)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(list: &[ToolPermission]) -> BTreeSet<ToolPermission> {
        list.iter().copied().collect()
    }

    #[test]
    fn default_policy_is_conservative() {
        let p = AgentPolicy::default();
        assert_eq!(p.max_context_scope, ContextScope::Minimal);
        assert!(p.allowed_tools.is_empty());
        assert!(!p.allow_cloud);
        assert!(p.require_confirmation);
    }

    #[test]
    fn default_deny_blocks_local_tool_request() {
        // 기본 정책은 툴 화이트리스트가 비어 로컬 읽기조차 거부.
        let err = authorize_with(
            "ollama",
            &AgentPolicy::default(),
            ProviderLocation::Local,
            ContextScope::Minimal,
            &tools(&[ToolPermission::ReadHealth]),
        )
        .unwrap_err();
        assert_eq!(err, PolicyError::ToolNotAllowed(ToolPermission::ReadHealth));
    }

    #[test]
    fn cloud_denied_by_default() {
        let err = authorize_with(
            "gpt",
            &AgentPolicy::default(),
            ProviderLocation::Cloud,
            ContextScope::Minimal,
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert_eq!(err, PolicyError::CloudNotAllowed("gpt".into()));
    }

    #[test]
    fn scope_escalation_denied() {
        let policy = AgentPolicy {
            allow_cloud: true,
            max_context_scope: ContextScope::Minimal,
            ..Default::default()
        };
        let err = authorize_with(
            "gpt",
            &policy,
            ProviderLocation::Cloud,
            ContextScope::Drivers, // Drivers > Minimal
            &BTreeSet::new(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            PolicyError::ScopeExceeded {
                requested: ContextScope::Drivers,
                max: ContextScope::Minimal,
            }
        );
    }

    #[test]
    fn unknown_tool_fails_deserialize() {
        // 미지의 툴 문자열은 역직렬화 에러 → 손상 정책은 로드 시 deny로 닫힌다.
        let json = r#"{"allowed_tools":["read_health","not_a_real_tool"]}"#;
        assert!(serde_json::from_str::<AgentPolicy>(json).is_err());
    }

    #[test]
    fn allowed_read_request_accepted() {
        let policy = AgentPolicy {
            allow_cloud: true,
            max_context_scope: ContextScope::System,
            allowed_tools: tools(&[ToolPermission::ReadHealth, ToolPermission::ReadSnapshot]),
            require_confirmation: true,
        };
        let auth = authorize_with(
            "gpt",
            &policy,
            ProviderLocation::Cloud,
            ContextScope::System,
            &tools(&[ToolPermission::ReadHealth]),
        )
        .expect("정책 통과 기대");
        // 읽기 툴은 부수효과 아님 → 승인 필요 없음.
        assert!(auth.tools_requiring_confirmation.is_empty());
    }

    #[test]
    fn side_effect_request_requires_confirmation() {
        let policy = AgentPolicy {
            allow_cloud: true,
            max_context_scope: ContextScope::Minimal,
            allowed_tools: tools(&[ToolPermission::WriteProject]),
            require_confirmation: true,
        };
        let auth = authorize_with(
            "gpt",
            &policy,
            ProviderLocation::Cloud,
            ContextScope::Minimal,
            &tools(&[ToolPermission::WriteProject]),
        )
        .expect("정책 통과 기대");
        assert!(
            auth.tools_requiring_confirmation
                .contains(&ToolPermission::WriteProject)
        );
    }

    #[test]
    fn partial_provider_policy_cannot_elevate_defaults() {
        // allow_cloud만 켠 부분 정책 → 나머지는 안전 기본값(deny) 유지.
        let policy: AgentPolicy = serde_json::from_str(r#"{"allow_cloud":true}"#).unwrap();
        assert!(policy.allow_cloud);
        assert!(policy.allowed_tools.is_empty()); // 상향 안 됨
        assert_eq!(policy.max_context_scope, ContextScope::Minimal); // 상향 안 됨
        assert!(policy.require_confirmation); // 안전값 유지

        // 그래서 scope/tool 요청은 여전히 거부된다.
        let err = authorize_with(
            "gpt",
            &policy,
            ProviderLocation::Cloud,
            ContextScope::System,
            &tools(&[ToolPermission::ReadHealth]),
        )
        .unwrap_err();
        assert_eq!(
            err,
            PolicyError::ScopeExceeded {
                requested: ContextScope::System,
                max: ContextScope::Minimal,
            }
        );
    }

    #[test]
    fn loopback_detection_is_strict() {
        assert!(is_loopback("http://127.0.0.1:11434/v1"));
        assert!(is_loopback("http://localhost:8080"));
        assert!(is_loopback("http://[::1]:11434/v1"));
        assert!(!is_loopback("http://myhost.localhost.evil.com/v1")); // 부분일치 거부
        assert!(!is_loopback("https://api.openai.com/v1"));
    }
}
