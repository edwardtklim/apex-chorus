//! velox-core::policy — Agent Policy (Step C 설계용 타입).
//!
//! 각 AI 에이전트(역할)가 **무엇을 볼 수 있고 / 무엇을 할 수 있고 / 클라우드로 나가도 되는지 /
//! 사람 승인이 필요한지**를 결정론적으로 규정한다. 나중에 Council의 최종 게이트가 이 정책을
//! 강제(enforce)한다.
//!
//! **주의: 아직 강제는 구현하지 않음 — 타입·안전 기본값·직렬화 설계만 (Step C 준비).**

use serde::{Deserialize, Serialize};

use crate::privacy::ContextScope;

/// 한 에이전트 역할(proposer/reviewer/…)에 걸리는 권한 정책.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentPolicy {
    /// 이 에이전트에게 허용되는 **최대 데이터 범위** (privacy의 `ContextScope` 재사용).
    pub max_context_scope: ContextScope,
    /// 이 에이전트가 호출할 수 있는 **툴 이름 화이트리스트**. 비어 있으면 툴 없음(읽기·추론만).
    pub allowed_tools: Vec<String>,
    /// **원격/클라우드** provider 호출 허용 여부. false면 로컬(예: Ollama)만.
    pub allow_cloud: bool,
    /// 부수효과(파일 편집·명령 실행 등) 전에 **사람 승인**을 요구할지.
    pub require_confirmation: bool,
}

impl Default for AgentPolicy {
    /// 안전 우선 기본값: 최소 데이터 · 툴 없음 · 클라우드 불가 · 승인 필수.
    /// (권한은 명시적으로 부여해야 열린다 — deny-by-default.)
    fn default() -> Self {
        Self {
            max_context_scope: ContextScope::Minimal,
            allowed_tools: Vec::new(),
            allow_cloud: false,
            require_confirmation: true,
        }
    }
}

impl AgentPolicy {
    /// 이 정책이 주어진 툴 호출을 허용하는지 (allowed_tools 화이트리스트 검사).
    pub fn permits_tool(&self, tool: &str) -> bool {
        self.allowed_tools.iter().any(|t| t == tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_conservative() {
        let p = AgentPolicy::default();
        assert_eq!(p.max_context_scope, ContextScope::Minimal);
        assert!(p.allowed_tools.is_empty());
        assert!(!p.allow_cloud);
        assert!(p.require_confirmation);
    }

    #[test]
    fn permits_tool_checks_whitelist() {
        let mut p = AgentPolicy::default();
        assert!(!p.permits_tool("read_report"));
        p.allowed_tools.push("read_report".into());
        assert!(p.permits_tool("read_report"));
        assert!(!p.permits_tool("run_command")); // 화이트리스트에 없는 툴은 거부
    }

    #[test]
    fn policy_round_trips_through_json() {
        let p = AgentPolicy {
            max_context_scope: ContextScope::System,
            allowed_tools: vec!["read_report".into()],
            allow_cloud: true,
            require_confirmation: false,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: AgentPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
