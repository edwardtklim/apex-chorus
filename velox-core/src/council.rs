//! velox-core::council — 읽기 전용 Council (v0.15 Step D).
//!
//! 흐름: **Claude 제안 → GPT 검토 → 결정론적 Gate → (필요 시) Claude 수정 → GPT 재검토(최대 1회)
//! → CouncilDecision**. Council은 **아무 것도 실행하지 않는다** — 판단만 돌려준다.
//!
//! 안전(불변조건 2.3/2.4):
//! - 역할은 [`crate::policy::execute_agent`]만 호출한다 → 동의·범위·툴 권한이 강제된다.
//! - 역할은 시스템/프로젝트를 **다시 수집하지 않는다** — 승인된 [`EvidenceBundle`]만 쓴다.
//! - 한 provider 실패를 다른 provider로 몰래 대체하지 않는다.
//! - v0.15에서 역할은 tool을 요청하지 않는다(`requested_tools = {}`).
//! - `TypedProposal`은 **서술형 finding만** 담는다(실행 가능한 action/명령 필드 없음) →
//!   AI가 raw 명령을 만들어 실행하는 경로가 구조적으로 존재하지 않는다.
//!   (실행 가능한 typed action은 v0.17에서 별도 whitelist/승인과 함께 도입)
//!
//! IO(네트워크)와 순수 판단 로직을 분리해 게이트·파싱·판정을 단위 테스트한다.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::evidence::{EvidenceBundle, EvidenceId};
use crate::policy::{AgentPurpose, AgentRequest, PolicyError, execute_agent};
use crate::privacy::ContextScope;

/// 기본 역할 → provider. 모델 ID는 저장하지 않고 `execute_agent`가 `model_name()`으로 해석한다.
const PROPOSER: &str = "claude";
const REVIEWER: &str = "gpt";
const REVISER: &str = "claude";

/// Council에 들어가는 입력. 역할들은 이 Evidence 밖 데이터를 수집하지 않는다.
#[derive(Clone, Debug)]
pub struct CouncilRequest {
    pub objective: String,
    pub evidence: EvidenceBundle,
    pub approved_scope: ContextScope,
}

/// 검토 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approve,
    Revise,
    Reject,
}

/// Council 최종 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CouncilStatus {
    Approved,
    Rejected,
    Inconclusive,
    Cancelled,
}

/// 제안 안의 한 발견/분석 — 최소 하나의 EvidenceId를 인용해야 한다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProposalFinding {
    pub statement: String,
    pub evidence: Vec<EvidenceId>,
}

/// 제안자의 구조화 결과. **서술형 finding만** — 실행 가능한 action 필드는 없다(read-only).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TypedProposal {
    pub summary: String,
    pub findings: Vec<ProposalFinding>,
}

/// Council의 최종 판단. **action을 실행하지 않는다.**
#[derive(Clone, Debug, Serialize)]
pub struct CouncilDecision {
    pub status: CouncilStatus,
    pub proposal: Option<TypedProposal>,
    pub reviewer_reasons: Vec<String>,
    pub evidence_used: Vec<EvidenceId>,
    pub requires_human_confirmation: bool,
}

impl CouncilDecision {
    fn terminal(status: CouncilStatus, reasons: Vec<String>) -> Self {
        Self {
            status,
            proposal: None,
            reviewer_reasons: reasons,
            evidence_used: Vec::new(),
            requires_human_confirmation: false,
        }
    }
}

// ---------------- 순수 로직 (네트워크 없음, 테스트 대상) ----------------

/// 문자열에서 JSON 본문만 추출(마크다운/설명으로 감싼 응답 대비).
fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then(|| &s[start..=end])
}

#[derive(Deserialize)]
struct RawFinding {
    #[serde(default)]
    statement: String,
    #[serde(default)]
    evidence: Vec<String>,
}
#[derive(Deserialize)]
struct RawProposal {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<RawFinding>,
}
#[derive(Deserialize)]
struct RawReview {
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    reasons: Vec<String>,
}

/// AI 응답 텍스트 → TypedProposal. 파싱 실패면 None(→ Inconclusive, fail-closed).
fn parse_proposal(text: &str) -> Option<TypedProposal> {
    let raw: RawProposal = serde_json::from_str(extract_json(text)?).ok()?;
    Some(TypedProposal {
        summary: raw.summary,
        findings: raw
            .findings
            .into_iter()
            .map(|f| ProposalFinding {
                statement: f.statement,
                evidence: f.evidence.into_iter().map(EvidenceId).collect(),
            })
            .collect(),
    })
}

/// AI 응답 텍스트 → (판정, 이유). 파싱 실패면 None.
fn parse_review(text: &str) -> Option<(ReviewVerdict, Vec<String>)> {
    let raw: RawReview = serde_json::from_str(extract_json(text)?).ok()?;
    let verdict = match raw.verdict.trim().to_lowercase().as_str() {
        "approve" => ReviewVerdict::Approve,
        "revise" => ReviewVerdict::Revise,
        "reject" => ReviewVerdict::Reject,
        _ => return None, // 알 수 없는 판정 → fail-closed
    };
    Some((verdict, raw.reasons))
}

/// **결정론적 Gate.** 통과하면 인용된(중복 제거) EvidenceId 목록, 실패하면 사유 목록.
/// 검사: provider 독립 · finding ≥1 · 각 finding이 Evidence 인용 · 인용 ID가 Bundle에 존재.
fn gate(
    proposal: &TypedProposal,
    bundle: &EvidenceBundle,
    proposer: &str,
    reviewer: &str,
) -> Result<Vec<EvidenceId>, Vec<String>> {
    let mut reasons = Vec::new();
    if proposer == reviewer {
        reasons.push("제안자와 검토자가 같은 provider".into());
    }
    if proposal.findings.is_empty() {
        reasons.push("제안에 finding이 없음".into());
    }
    let mut used: Vec<EvidenceId> = Vec::new();
    for (i, f) in proposal.findings.iter().enumerate() {
        if f.evidence.is_empty() {
            reasons.push(format!("finding[{i}]이 Evidence를 인용하지 않음"));
        }
        for id in &f.evidence {
            if !bundle.contains(id) {
                reasons.push(format!(
                    "finding[{i}]이 존재하지 않는 Evidence '{}' 인용",
                    id.0
                ));
            } else if !used.contains(id) {
                used.push(id.clone());
            }
        }
    }
    if reasons.is_empty() {
        Ok(used)
    } else {
        Err(reasons)
    }
}

fn approved(
    proposal: TypedProposal,
    evidence_used: Vec<EvidenceId>,
    reasons: Vec<String>,
) -> CouncilDecision {
    CouncilDecision {
        status: CouncilStatus::Approved,
        proposal: Some(proposal),
        reviewer_reasons: reasons,
        evidence_used,
        // v0.15 Council은 read-only(실행 가능한 action 없음) → 사람 승인 대상 부수효과 없음.
        requires_human_confirmation: false,
    }
}

// ---------------- 프롬프트 ----------------

fn proposer_prompt(objective: &str, evidence: &str) -> String {
    format!(
        "너는 APEX Council의 제안자(Proposer)다. **아래 Evidence만** 근거로 목표를 분석하라.\n\
         Evidence 밖 정보를 지어내지 말고, 각 finding은 인용한 Evidence의 ID를 반드시 넣어라.\n\
         JSON 하나로만 답하라(설명·마크다운·코드펜스 금지):\n\
         {{\"summary\":\"한 줄 요약\",\"findings\":[{{\"statement\":\"발견/분석\",\"evidence\":[\"evidence_id\"]}}]}}\n\n\
         목표: {objective}\n\n{evidence}"
    )
}

fn reviewer_prompt(objective: &str, evidence: &str, proposal_json: &str) -> String {
    format!(
        "너는 APEX Council의 검토자(Reviewer)다. 아래 제안을 Evidence에 비추어 검토하라.\n\
         - Evidence로 뒷받침되지 않거나 과장/비약된 주장이 있으면 revise 또는 reject.\n\
         - 근거가 충분하고 안전하면 approve.\n\
         JSON 하나로만: {{\"verdict\":\"approve|revise|reject\",\"reasons\":[\"이유\"]}}\n\n\
         목표: {objective}\n\n{evidence}\n\n제안(JSON):\n{proposal_json}"
    )
}

fn reviser_prompt(
    objective: &str,
    evidence: &str,
    proposal_json: &str,
    reasons: &[String],
) -> String {
    format!(
        "너는 APEX Council의 수정자(Reviser)다. 검토자의 지적을 반영해 제안을 고쳐라.\n\
         제안자와 동일한 JSON 스키마로만 답하라. Evidence 밖 정보 금지, 각 finding은 Evidence 인용.\n\n\
         목표: {objective}\n\n{evidence}\n\n이전 제안:\n{proposal_json}\n\n검토자 지적:\n- {}",
        reasons.join("\n- ")
    )
}

// ---------------- IO 오케스트레이션 ----------------

type RoleFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;

/// 역할(AI)을 호출하는 방법. 실제 경로는 정책 게이트를 통과하는 [`LiveCaller`]이고,
/// 테스트는 스크립트된 응답을 주입해 상태 기계를 네트워크 없이 검증한다.
pub trait RoleCaller: Sync {
    fn call<'a>(
        &'a self,
        provider: &'a str,
        purpose: AgentPurpose,
        scope: ContextScope,
        prompt: String,
    ) -> RoleFuture<'a>;
}

/// 실제 호출 — 반드시 [`execute_agent`]를 거친다(동의·범위·툴 권한 강제, 자동 대체 없음).
struct LiveCaller;

impl RoleCaller for LiveCaller {
    fn call<'a>(
        &'a self,
        provider: &'a str,
        purpose: AgentPurpose,
        scope: ContextScope,
        prompt: String,
    ) -> RoleFuture<'a> {
        Box::pin(async move {
            let req = AgentRequest {
                provider: provider.to_string(),
                purpose,
                prompt,
                scope,
                requested_tools: std::collections::BTreeSet::new(),
            };
            match execute_agent(req).await {
                Ok(r) => Ok(r.text),
                Err(PolicyError::CloudNotAllowed(p)) => Err(format!("{p} 미동의 (consent 필요)")),
                Err(e) => Err(e.to_string()),
            }
        })
    }
}

fn proposal_to_json(p: &TypedProposal) -> String {
    serde_json::to_string(p).unwrap_or_else(|_| "{}".into())
}

/// Council 실행. 아무 것도 실행하지 않고 [`CouncilDecision`]만 돌려준다.
/// `cancel`이 참이 되면 다음 단계 진입 전에 [`CouncilStatus::Cancelled`]로 멈춘다.
pub async fn run(req: CouncilRequest, cancel: &AtomicBool) -> CouncilDecision {
    run_with(req, cancel, &LiveCaller).await
}

/// 호출자를 주입할 수 있는 Council 실행(테스트용 seam). 프로덕션은 [`run`]을 쓴다.
pub async fn run_with(
    req: CouncilRequest,
    cancel: &AtomicBool,
    caller: &dyn RoleCaller,
) -> CouncilDecision {
    let scope = req.evidence.approved_scope;
    // Gate 사전조건: Evidence 범위가 사용자 승인 범위를 넘지 않아야 한다.
    if req.evidence.approved_scope > req.approved_scope {
        return CouncilDecision::terminal(
            CouncilStatus::Inconclusive,
            vec!["Evidence 범위가 승인 범위를 초과".into()],
        );
    }
    let evidence = req.evidence.to_prompt();

    // 1) 제안 (Claude)
    if cancel.load(Ordering::Relaxed) {
        return CouncilDecision::terminal(CouncilStatus::Cancelled, vec![]);
    }
    let proposal = match caller
        .call(
            PROPOSER,
            AgentPurpose::Propose,
            scope,
            proposer_prompt(&req.objective, &evidence),
        )
        .await
    {
        Ok(text) => match parse_proposal(&text) {
            Some(p) => p,
            None => {
                return CouncilDecision::terminal(
                    CouncilStatus::Inconclusive,
                    vec!["제안 파싱 실패(구조화 JSON 아님)".into()],
                );
            }
        },
        Err(e) => {
            return CouncilDecision::terminal(
                CouncilStatus::Inconclusive,
                vec![format!("제안자 호출 실패: {e}")],
            );
        }
    };
    // 사전 Gate (검토 전 구조 검증)
    if let Err(reasons) = gate(&proposal, &req.evidence, PROPOSER, REVIEWER) {
        return CouncilDecision::terminal(CouncilStatus::Inconclusive, reasons);
    }

    // 2) 검토 (GPT)
    if cancel.load(Ordering::Relaxed) {
        return CouncilDecision::terminal(CouncilStatus::Cancelled, vec![]);
    }
    let (verdict, reasons) = match caller
        .call(
            REVIEWER,
            AgentPurpose::Review,
            scope,
            reviewer_prompt(&req.objective, &evidence, &proposal_to_json(&proposal)),
        )
        .await
    {
        Ok(text) => match parse_review(&text) {
            Some(v) => v,
            None => {
                return CouncilDecision::terminal(
                    CouncilStatus::Inconclusive,
                    vec!["검토 파싱 실패".into()],
                );
            }
        },
        Err(e) => {
            return CouncilDecision::terminal(
                CouncilStatus::Inconclusive,
                vec![format!("검토자 호출 실패: {e}")],
            );
        }
    };

    match verdict {
        ReviewVerdict::Reject => CouncilDecision::terminal(CouncilStatus::Rejected, reasons),
        ReviewVerdict::Approve => finalize(proposal, &req.evidence, reasons),
        ReviewVerdict::Revise => {
            // 3) 수정 (Claude) → 재검토 (GPT). 재수정 요구는 최대 1회 → 그 이상은 Inconclusive.
            if cancel.load(Ordering::Relaxed) {
                return CouncilDecision::terminal(CouncilStatus::Cancelled, vec![]);
            }
            let revised = match caller
                .call(
                    REVISER,
                    AgentPurpose::Revise,
                    scope,
                    reviser_prompt(
                        &req.objective,
                        &evidence,
                        &proposal_to_json(&proposal),
                        &reasons,
                    ),
                )
                .await
            {
                Ok(text) => match parse_proposal(&text) {
                    Some(p) => p,
                    None => {
                        return CouncilDecision::terminal(
                            CouncilStatus::Inconclusive,
                            vec!["수정 제안 파싱 실패".into()],
                        );
                    }
                },
                Err(e) => {
                    return CouncilDecision::terminal(
                        CouncilStatus::Inconclusive,
                        vec![format!("수정자 호출 실패: {e}")],
                    );
                }
            };
            if let Err(reasons) = gate(&revised, &req.evidence, REVISER, REVIEWER) {
                return CouncilDecision::terminal(CouncilStatus::Inconclusive, reasons);
            }
            if cancel.load(Ordering::Relaxed) {
                return CouncilDecision::terminal(CouncilStatus::Cancelled, vec![]);
            }
            let (v2, r2) = match caller
                .call(
                    REVIEWER,
                    AgentPurpose::Review,
                    scope,
                    reviewer_prompt(&req.objective, &evidence, &proposal_to_json(&revised)),
                )
                .await
            {
                Ok(text) => match parse_review(&text) {
                    Some(v) => v,
                    None => {
                        return CouncilDecision::terminal(
                            CouncilStatus::Inconclusive,
                            vec!["재검토 파싱 실패".into()],
                        );
                    }
                },
                Err(e) => {
                    return CouncilDecision::terminal(
                        CouncilStatus::Inconclusive,
                        vec![format!("재검토 호출 실패: {e}")],
                    );
                }
            };
            match v2 {
                ReviewVerdict::Approve => finalize(revised, &req.evidence, r2),
                ReviewVerdict::Reject => CouncilDecision::terminal(CouncilStatus::Rejected, r2),
                // 재수정 요구 = 반복 한도(1회) 초과 → Inconclusive.
                ReviewVerdict::Revise => CouncilDecision::terminal(CouncilStatus::Inconclusive, {
                    let mut r = r2;
                    r.push("재수정 요구 — 반복 한도(1회) 초과".into());
                    r
                }),
            }
        }
    }
}

/// Approve 판정 후 최종 Gate를 다시 걸고 결정을 만든다.
fn finalize(
    proposal: TypedProposal,
    bundle: &EvidenceBundle,
    reasons: Vec<String>,
) -> CouncilDecision {
    match gate(&proposal, bundle, PROPOSER, REVIEWER) {
        Ok(used) => approved(proposal, used, reasons),
        Err(mut gate_reasons) => {
            gate_reasons.push("승인됐으나 최종 Gate 불통과".into());
            CouncilDecision::terminal(CouncilStatus::Inconclusive, gate_reasons)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{EvidenceData, EvidenceItem, EvidenceSource};

    fn bundle() -> EvidenceBundle {
        EvidenceBundle::new(
            ContextScope::Minimal,
            vec![
                EvidenceItem {
                    id: EvidenceId("e1".into()),
                    source: EvidenceSource::Snapshot,
                    sensitivity: ContextScope::Minimal,
                    data: EvidenceData::Metric {
                        name: "CPU".into(),
                        value: 95.0,
                        unit: "%".into(),
                    },
                },
                EvidenceItem {
                    id: EvidenceId("e2".into()),
                    source: EvidenceSource::Snapshot,
                    sensitivity: ContextScope::Minimal,
                    data: EvidenceData::Metric {
                        name: "온도".into(),
                        value: 92.0,
                        unit: "°C".into(),
                    },
                },
            ],
        )
        .unwrap()
    }

    fn good_proposal() -> TypedProposal {
        TypedProposal {
            summary: "과열".into(),
            findings: vec![ProposalFinding {
                statement: "CPU가 95%에서 92°C".into(),
                evidence: vec![EvidenceId("e1".into()), EvidenceId("e2".into())],
            }],
        }
    }

    #[test]
    fn parse_proposal_extracts_json_from_prose() {
        let text = "여기 결과입니다:\n{\"summary\":\"s\",\"findings\":[{\"statement\":\"a\",\"evidence\":[\"e1\"]}]}\n감사합니다.";
        let p = parse_proposal(text).expect("파싱 성공");
        assert_eq!(p.findings[0].evidence, vec![EvidenceId("e1".into())]);
    }

    #[test]
    fn parse_review_maps_verdicts_and_rejects_unknown() {
        assert_eq!(
            parse_review("{\"verdict\":\"approve\",\"reasons\":[]}")
                .unwrap()
                .0,
            ReviewVerdict::Approve
        );
        assert_eq!(
            parse_review("{\"verdict\":\"reject\",\"reasons\":[\"근거 부족\"]}")
                .unwrap()
                .0,
            ReviewVerdict::Reject
        );
        assert!(parse_review("{\"verdict\":\"maybe\"}").is_none()); // 알 수 없는 판정 → fail-closed
        assert!(parse_review("not json").is_none());
    }

    #[test]
    fn gate_accepts_well_cited_proposal() {
        let used = gate(&good_proposal(), &bundle(), PROPOSER, REVIEWER).expect("통과");
        assert_eq!(used.len(), 2);
    }

    #[test]
    fn gate_rejects_same_provider() {
        let err = gate(&good_proposal(), &bundle(), "claude", "claude").unwrap_err();
        assert!(err.iter().any(|r| r.contains("같은 provider")));
    }

    #[test]
    fn gate_rejects_uncited_finding() {
        let p = TypedProposal {
            summary: "s".into(),
            findings: vec![ProposalFinding {
                statement: "근거 없는 주장".into(),
                evidence: vec![],
            }],
        };
        assert!(gate(&p, &bundle(), PROPOSER, REVIEWER).is_err());
    }

    #[test]
    fn gate_rejects_nonexistent_evidence_id() {
        let p = TypedProposal {
            summary: "s".into(),
            findings: vec![ProposalFinding {
                statement: "환각 인용".into(),
                evidence: vec![EvidenceId("does_not_exist".into())],
            }],
        };
        let err = gate(&p, &bundle(), PROPOSER, REVIEWER).unwrap_err();
        assert!(err.iter().any(|r| r.contains("존재하지 않는 Evidence")));
    }

    #[tokio::test]
    async fn cancel_before_start_returns_cancelled() {
        let req = CouncilRequest {
            objective: "왜 뜨겁나".into(),
            evidence: bundle(),
            approved_scope: ContextScope::Minimal,
        };
        let cancel = AtomicBool::new(true);
        let d = run(req, &cancel).await;
        assert_eq!(d.status, CouncilStatus::Cancelled);
    }

    #[tokio::test]
    async fn evidence_scope_exceeding_approved_is_inconclusive() {
        // Evidence는 System 범위인데 승인은 Minimal → 진행 불가(네트워크 호출 전에 종료).
        let sys_bundle = EvidenceBundle::new(
            ContextScope::System,
            vec![EvidenceItem {
                id: EvidenceId("s1".into()),
                source: EvidenceSource::Snapshot,
                sensitivity: ContextScope::System,
                data: EvidenceData::Fact {
                    name: "CPU".into(),
                    value: "x".into(),
                },
            }],
        )
        .unwrap();
        let req = CouncilRequest {
            objective: "o".into(),
            evidence: sys_bundle,
            approved_scope: ContextScope::Minimal,
        };
        let d = run(req, &AtomicBool::new(false)).await;
        assert_eq!(d.status, CouncilStatus::Inconclusive);
    }

    #[test]
    fn finalize_builds_approved_decision() {
        let d = finalize(good_proposal(), &bundle(), vec!["ok".into()]);
        assert_eq!(d.status, CouncilStatus::Approved);
        assert!(d.proposal.is_some());
        assert_eq!(d.evidence_used.len(), 2);
        assert!(!d.requires_human_confirmation); // read-only
    }

    // --- 전체 상태 기계: 스크립트된 역할 응답 주입(네트워크 없음) ---

    struct Scripted {
        queue: std::sync::Mutex<std::collections::VecDeque<Result<String, String>>>,
    }
    impl Scripted {
        fn new(items: Vec<Result<String, String>>) -> Self {
            Self {
                queue: std::sync::Mutex::new(items.into_iter().collect()),
            }
        }
    }
    impl RoleCaller for Scripted {
        fn call<'a>(
            &'a self,
            _provider: &'a str,
            _purpose: AgentPurpose,
            _scope: ContextScope,
            _prompt: String,
        ) -> RoleFuture<'a> {
            let r = self
                .queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("스크립트 응답 없음".into()));
            Box::pin(async move { r })
        }
    }

    const PJSON: &str =
        r#"{"summary":"과열","findings":[{"statement":"CPU 95%/92C","evidence":["e1","e2"]}]}"#;

    fn ok(s: &str) -> Result<String, String> {
        Ok(s.into())
    }

    async fn run_scripted(script: Vec<Result<String, String>>) -> CouncilDecision {
        let req = CouncilRequest {
            objective: "왜 뜨겁나".into(),
            evidence: bundle(),
            approved_scope: ContextScope::Minimal,
        };
        run_with(req, &AtomicBool::new(false), &Scripted::new(script)).await
    }

    #[tokio::test]
    async fn flow_approve_reaches_approved() {
        let d = run_scripted(vec![
            ok(PJSON),
            ok(r#"{"verdict":"approve","reasons":["근거 충분"]}"#),
        ])
        .await;
        assert_eq!(d.status, CouncilStatus::Approved);
        assert!(d.proposal.is_some());
        assert_eq!(d.evidence_used.len(), 2);
    }

    #[tokio::test]
    async fn flow_reviewer_reject_terminates() {
        let d = run_scripted(vec![
            ok(PJSON),
            ok(r#"{"verdict":"reject","reasons":["근거 부족"]}"#),
        ])
        .await;
        assert_eq!(d.status, CouncilStatus::Rejected);
        assert!(d.reviewer_reasons.iter().any(|r| r.contains("근거 부족")));
    }

    #[tokio::test]
    async fn flow_revise_then_approve() {
        let d = run_scripted(vec![
            ok(PJSON),
            ok(r#"{"verdict":"revise","reasons":["더 구체적으로"]}"#),
            ok(PJSON), // 수정본
            ok(r#"{"verdict":"approve","reasons":["이제 충분"]}"#),
        ])
        .await;
        assert_eq!(d.status, CouncilStatus::Approved);
    }

    #[tokio::test]
    async fn flow_second_revise_hits_iteration_limit() {
        let d = run_scripted(vec![
            ok(PJSON),
            ok(r#"{"verdict":"revise","reasons":["r1"]}"#),
            ok(PJSON),
            ok(r#"{"verdict":"revise","reasons":["r2"]}"#),
        ])
        .await;
        assert_eq!(d.status, CouncilStatus::Inconclusive);
        assert!(d.reviewer_reasons.iter().any(|r| r.contains("반복 한도")));
    }

    #[tokio::test]
    async fn flow_provider_failure_is_inconclusive_no_fallback() {
        let d = run_scripted(vec![Err("claude 미동의".into())]).await;
        assert_eq!(d.status, CouncilStatus::Inconclusive);
        assert!(
            d.reviewer_reasons
                .iter()
                .any(|r| r.contains("제안자 호출 실패"))
        );
    }

    #[tokio::test]
    async fn flow_unparseable_proposal_is_inconclusive() {
        let d = run_scripted(vec![ok("죄송하지만 JSON이 아닙니다")]).await;
        assert_eq!(d.status, CouncilStatus::Inconclusive);
    }
}
