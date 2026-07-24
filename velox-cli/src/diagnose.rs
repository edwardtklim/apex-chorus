// APEX Velox — diagnose  (Core의 첫 세포)
//
// Action Loop: 읽기 → 3단계 AI → 화이트리스트 → [Confirmer] → [승인] → 체크포인트 → 실행 → 검증 → 롤백
//
// 3단계 AI (오래된 꿈, 키 3개 활용):
//   1) Customer (Claude)  — 의도/상황 이해
//   2) Engineer (GPT)     — 구조화된 조치 제안 (화이트리스트 JSON)
//   3) Confirmer (Gemini) — Engineer 조치를 독립 검수 → APPROVE/REJECT
//
// 안전 원칙: AI는 메뉴에서 "선택"만(명령 생성 X) · 모든 동작 가역 · 실행 전 자동 체크포인트 · 사람 최종 승인.

use serde::Deserialize;
use std::io::{self, Write};

const TEMP_WARN_C: f32 = 85.0;
const LOG_FILE: &str = "velox_actions.log";

struct Snapshot {
    max_temp_c: Option<f32>,
    cpu_usage: f32,
    plan_guid: String,
    plan_label: String,
    summary: String,
}

/// 현실성 검사: AI에게 데이터를 주기 전, 입력이 물리적으로 말이 되는지 코드가 먼저 본다.
/// 버그/센서 오류로 멀쩡한 시스템을 건드리는 걸 막는 게이트. Some(이유)면 실행 보류.
fn suspicious(snap: &Snapshot) -> Option<String> {
    let t = snap.max_temp_c?;
    if !(20.0..=110.0).contains(&t) {
        return Some(format!(
            "온도 {:.1}°C가 정상 범위(20~110°C) 밖 — 센서 오류 의심",
            t
        ));
    }
    if t >= TEMP_WARN_C && snap.cpu_usage < 20.0 {
        return Some(format!(
            "고온 {:.1}°C인데 CPU 부하가 {:.0}%로 낮음 — 데이터 의심(센서/버그 가능)",
            t, snap.cpu_usage
        ));
    }
    None
}

impl Snapshot {
    fn is_hot(&self) -> bool {
        self.max_temp_c.is_some_and(|t| t >= TEMP_WARN_C)
    }
}

enum Action {
    SetPowerPlan {
        label: &'static str,
        guid: &'static str,
        rollback_guid: String,
    },
    None,
}

#[derive(Deserialize, Default)]
struct AiProposal {
    #[serde(default)]
    action: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    reason: String,
}

struct Pipeline {
    transcript: String,
    action: Action,
    confirmed: bool,
}

// ---------------- [1] 읽기 (엔진: velox_core::snapshot) ----------------

fn collect() -> Snapshot {
    // 데이터 읽기는 엔진(core)에 위임. CLI는 AI용 summary만 조립한다.
    let core = velox_core::snapshot::Snapshot::collect();
    let temp_str = match core.max_temp_c {
        Some(c) => format!("{:.1}°C", c),
        None => "N/A (센서 읽기 실패 — 관리자 권한 필요)".to_string(),
    };
    let summary = format!(
        "- 현재 전원 모드: {} ({})\n- CPU 사용률: {:.0}%\n- 최고 온도: {}\n{}",
        core.plan_label,
        core.plan_guid,
        core.cpu_usage,
        temp_str,
        crate::drivers::problem_summary()
    );
    Snapshot {
        max_temp_c: core.max_temp_c,
        cpu_usage: core.cpu_usage,
        plan_guid: core.plan_guid,
        plan_label: core.plan_label,
        summary,
    }
}

// ---------------- 화이트리스트 검증 ----------------

fn validate(p: &AiProposal, snap: &Snapshot) -> Action {
    if !p.action.contains("set_power_plan") {
        return Action::None;
    }
    // target: 명시 필드 우선, 없으면 action 문자열에서 추출 (예: "set_power_plan|balanced")
    let key = p
        .target
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| p.action.split('|').nth(1).map(|s| s.trim().to_string()))
        .unwrap_or_default();
    match velox_core::action::plan_by_key(&key) {
        Some((label, guid)) if guid.to_lowercase() != snap.plan_guid.to_lowercase() => {
            Action::SetPowerPlan {
                label,
                guid,
                rollback_guid: snap.plan_guid.clone(),
            }
        }
        _ => Action::None,
    }
}

fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then(|| &s[start..=end])
}

// ---------------- [2] 3단계 AI 파이프라인 ----------------

/// diagnose가 AI에 보낼 Evidence — Minimal(전원·CPU·온도)만. simulate_hot로 주입된 온도도 반영.
fn diagnose_evidence(snap: &Snapshot) -> Option<velox_core::evidence::EvidenceBundle> {
    use velox_core::evidence::{EvidenceData, EvidenceId, EvidenceItem, EvidenceSource};
    use velox_core::privacy::ContextScope::Minimal;
    let mut items = vec![
        EvidenceItem {
            id: EvidenceId("diag.power_plan".into()),
            source: EvidenceSource::Snapshot,
            sensitivity: Minimal,
            data: EvidenceData::Fact {
                name: "전원 계획".into(),
                value: snap.plan_label.clone(),
            },
        },
        EvidenceItem {
            id: EvidenceId("diag.cpu_usage".into()),
            source: EvidenceSource::Snapshot,
            sensitivity: Minimal,
            data: EvidenceData::Metric {
                name: "CPU 사용률".into(),
                value: snap.cpu_usage as f64,
                unit: "%".into(),
            },
        },
    ];
    if let Some(t) = snap.max_temp_c {
        items.push(EvidenceItem {
            id: EvidenceId("diag.max_temp_c".into()),
            source: EvidenceSource::Snapshot,
            sensitivity: Minimal,
            data: EvidenceData::Metric {
                name: "최고 온도".into(),
                value: t as f64,
                unit: "°C".into(),
            },
        });
    }
    velox_core::evidence::EvidenceBundle::new(Minimal, items).ok()
}

async fn ai_pipeline(snap: &Snapshot) -> Option<Pipeline> {
    // AI로 나가는 payload는 typed EvidenceBundle에서만 생성한다(Minimal: 전원·CPU·온도).
    let evidence = diagnose_evidence(snap)?.to_prompt();

    // 1) Customer (Claude) — 의도/상황 이해 (정책 게이트 경유)
    let intent = crate::chorus::gated_text(
        "claude",
        velox_core::policy::AgentPurpose::Diagnose,
        velox_core::privacy::ContextScope::Minimal,
        format!(
            "다음 시스템 상태에서 사용자가 가장 걱정할 점이나 원하는 바를 한국어 한 줄로 요약:\n{evidence}"
        ),
    )
    .await
    .unwrap_or_else(|| "(의도 파악 실패)".to_string());

    // 2) Engineer (GPT) — 구조화된 조치 제안
    let eng_prompt = format!(
        "너는 시스템 엔지니어 AI다.\n사용자 의도: {}\n시스템 상태:\n{}\n\n\
         아래 형식의 JSON 한 개로만 답하라(설명/마크다운/코드펜스 금지).\n\
         필드 정의:\n\
         - action: \"set_power_plan\" 또는 \"none\" 중 정확히 하나의 문자열\n\
         - target: action이 set_power_plan이면 \"balanced\" 또는 \"high_performance\" 또는 \"power_saver\" 중 하나, 아니면 null\n\
         - reason: 한 줄 이유\n\
         정확한 예시: {{\"action\":\"set_power_plan\",\"target\":\"balanced\",\"reason\":\"발열이 높아 균형 모드로 낮춤\"}}\n\
         규칙: 안전하고 되돌릴 수 있는 경우에만 set_power_plan, 애매하면 action을 \"none\"으로.",
        intent.trim(),
        evidence
    );
    let eng_raw = crate::chorus::gated_text(
        "gpt",
        velox_core::policy::AgentPurpose::Propose,
        velox_core::privacy::ContextScope::Minimal,
        eng_prompt,
    )
    .await?;
    let proposal: AiProposal = serde_json::from_str(extract_json(&eng_raw)?).ok()?;
    let action = validate(&proposal, snap);

    // 3) Confirmer (Gemini) — 독립 검수
    let (confirmed, verdict) = match &action {
        Action::None => (false, "제안된 조치 없음".to_string()),
        Action::SetPowerPlan { label, .. } => {
            let conf_prompt = format!(
                "너는 검수 AI다. 제안된 조치: 전원 모드를 '{}'(으)로 변경.\n시스템 상태:\n{}\n\n\
                 안전하고 합리적이면 'APPROVE', 아니면 'REJECT'로 시작해 한국어 한 줄 이유.",
                label, evidence
            );
            let r = crate::chorus::gated_text(
                "gemini",
                velox_core::policy::AgentPurpose::Review,
                velox_core::privacy::ContextScope::Minimal,
                conf_prompt,
            )
            .await
            .unwrap_or_else(|| "REJECT (검수 AI 응답 없음)".to_string());
            (r.to_uppercase().contains("APPROVE"), r)
        }
    };

    let transcript = format!(
        "  1·Customer(Claude): {}\n  2·Engineer(GPT):   action={} ({})\n  3·Confirmer(Gemini): {}",
        intent.trim(),
        proposal.action,
        proposal.reason.trim(),
        verdict.trim()
    );

    Some(Pipeline {
        transcript,
        action,
        confirmed,
    })
}

fn propose_rule_based(snap: &Snapshot) -> Action {
    if snap.is_hot()
        && let Some((label, guid)) = velox_core::action::plan_by_key("balanced")
        && guid.to_lowercase() != snap.plan_guid.to_lowercase()
    {
        return Action::SetPowerPlan {
            label,
            guid,
            rollback_guid: snap.plan_guid.clone(),
        };
    }
    Action::None
}

// ---------------- 실행 ----------------

fn execute_with_safety(label: &str, guid: &str, rollback_guid: &str) {
    // 위험 동작 전 자동 체크포인트 (AI 오판/블루스크린 대비)
    velox_core::checkpoint::save_silent();
    println!("[체크포인트 저장됨 — 문제 시 `velox checkpoint restore`]");

    if !velox_core::action::apply_power_plan(guid) {
        println!("✗ 실행 실패 (권한/모드 부재).");
        return;
    }
    let (new_guid, new_label) = velox_core::snapshot::active_power_plan();
    let verified = new_guid.to_lowercase() == guid.to_lowercase();
    println!(
        "검증: 현재 전원 모드 = {} {}",
        new_label,
        if verified { "✓" } else { "✗ (불일치)" }
    );
    println!("되돌리려면: powercfg /setactive {}", rollback_guid);
    log_action(label, guid, rollback_guid, verified);
}

fn log_action(label: &str, guid: &str, rollback: &str, verified: bool) {
    use std::fs::OpenOptions;
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(
            f,
            "[diagnose] set_power_plan label={} guid={} verified={} rollback={}",
            label, guid, verified, rollback
        );
    }
}

// ---------------- CLI: velox diagnose [--fix] ----------------

pub async fn run(fix: bool, simulate_hot: bool) {
    println!("=== APEX Velox — diagnose (3단계 AI) ===\n");
    let mut snap = collect();
    if simulate_hot {
        let fake = 95.0_f32;
        snap.max_temp_c = Some(fake);
        snap.summary = format!(
            "- 현재 전원 모드: {} ({})\n- CPU 사용률: {:.0}%\n- 최고 온도: {:.1}°C  [⚙ SIMULATED]\n{}",
            snap.plan_label,
            snap.plan_guid,
            snap.cpu_usage,
            fake,
            crate::drivers::problem_summary()
        );
        println!(
            "⚙ 시뮬레이션: 온도 {:.1}°C 주입 (실제 센서 아님). AI·체크포인트·실행·롤백은 전부 진짜(가역).\n",
            fake
        );
    }
    println!("[1] 시스템 상태\n{}\n", snap.summary);

    println!("[2] 3단계 AI 파이프라인...");
    let (action, confirmed) = match ai_pipeline(&snap).await {
        Some(p) => {
            println!("{}\n", p.transcript);
            (p.action, p.confirmed)
        }
        None => {
            println!("(AI 사용 불가 — 규칙 기반 대체)\n");
            (propose_rule_based(&snap), true)
        }
    };

    match &action {
        Action::None => {
            println!("[3] 결론: 적용할 안전한 조치 없음.");
            return;
        }
        Action::SetPowerPlan { label, .. } => {
            if !confirmed {
                println!(
                    "[3] Confirmer AI가 반려함 → 실행 차단 🛑 (제안: 전원 모드 → {})",
                    label
                );
                return;
            }
            println!(
                "[3] 조치 제안: 전원 모드 → '{}' (화이트리스트 ✓ · Confirmer ✓ · 가역)",
                label
            );
        }
    }

    // 안전: 시뮬레이션은 절대 실제 상태를 바꾸지 않는다 (테스트 ≠ 실제).
    // 버그나 잘못된 시뮬레이션이 멀쩡한 시스템을 건드리는 걸 원천 차단.
    if simulate_hot {
        println!("\n[시뮬레이션] 실제 실행 안 함 — 테스트는 상태를 바꾸지 않습니다.");
        println!("(진짜 실행은 실제 데이터로: velox diagnose --fix)");
        return;
    }

    if !fix {
        println!("\n실제 적용: velox diagnose --fix");
        return;
    }

    // 현실성 검사: 의심스러운 데이터면 실행하지 않는다.
    if let Some(reason) = suspicious(&snap) {
        println!("\n[안전·현실성 검사] {}", reason);
        println!("→ 실행 보류: 데이터가 의심스러울 땐 시스템을 바꾸지 않습니다.");
        return;
    }

    if let Action::SetPowerPlan {
        label,
        guid,
        rollback_guid,
    } = action
    {
        print!("\n[4] 적용할까요? (y/N): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        if input.trim().to_lowercase() != "y" {
            println!("취소됨.");
            return;
        }
        println!("[5] 실행: 전원 모드 → {}", label);
        execute_with_safety(label, guid, &rollback_guid);
    }
}

// ---------------- daemon이 쓰는 함수 ----------------

/// 가벼운 상태 읽기 (heartbeat 문자열, 과열 여부). 매 틱 호출되므로 무거운 작업 제외.
pub fn quick_status() -> (String, bool) {
    let (_, plan_label) = velox_core::snapshot::active_power_plan();
    let temp = velox_core::snapshot::max_temp_c();
    let hot = temp.is_some_and(|t| t >= TEMP_WARN_C);
    let temp_s = temp
        .map(|c| format!("{:.1}°C", c))
        .unwrap_or_else(|| "N/A".to_string());
    (format!("전원={} 온도={}", plan_label, temp_s), hot)
}

/// 무거운 반응 단계: 전체 스냅샷 → 3단계 AI → 현실성 검사 → (auto면) 실행.
/// 데몬이 "지속성 + 쿨다운"을 통과시켜 호출할 때만 실행된다.
pub async fn react(auto: bool) {
    let snap = collect();
    let (action, confirmed) = match ai_pipeline(&snap).await {
        Some(p) => {
            println!("{}", p.transcript);
            (p.action, p.confirmed)
        }
        None => (propose_rule_based(&snap), true),
    };

    match action {
        Action::None => println!("  → 조치 없음"),
        Action::SetPowerPlan {
            label,
            guid,
            rollback_guid,
        } => {
            if !confirmed {
                println!("  → Confirmer 반려 🛑 실행 안 함");
            } else if let Some(reason) = suspicious(&snap) {
                println!("  → [안전] {} → 실행 보류", reason);
            } else if auto {
                println!("  → AUTO: {} 적용", label);
                execute_with_safety(label, guid, &rollback_guid);
            } else {
                println!("  → 제안: 전원 모드 → {} (--auto로 자동 실행)", label);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    const HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

    fn snap(max_temp_c: Option<f32>, cpu_usage: f32, plan_guid: &str) -> Snapshot {
        Snapshot {
            max_temp_c,
            cpu_usage,
            plan_guid: plan_guid.to_string(),
            plan_label: "Test".to_string(),
            summary: String::new(),
        }
    }

    fn proposal(action: &str, target: Option<&str>) -> AiProposal {
        AiProposal {
            action: action.to_string(),
            target: target.map(|s| s.to_string()),
            reason: String::new(),
        }
    }

    // --- 현실성 검사(suspicious): AI에 데이터 주기 전 안전 게이트 ---

    #[test]
    fn suspicious_flags_impossible_temperature() {
        assert!(suspicious(&snap(Some(150.0), 50.0, BALANCED)).is_some());
        assert!(suspicious(&snap(Some(5.0), 50.0, BALANCED)).is_some());
    }

    #[test]
    fn suspicious_flags_hot_but_idle() {
        // 90°C인데 CPU 5% → 센서/버그 의심 → 동작 보류.
        assert!(suspicious(&snap(Some(90.0), 5.0, BALANCED)).is_some());
    }

    #[test]
    fn suspicious_passes_real_load_and_missing_sensor() {
        assert!(suspicious(&snap(Some(90.0), 80.0, BALANCED)).is_none()); // 진짜 부하
        assert!(suspicious(&snap(Some(60.0), 30.0, BALANCED)).is_none()); // 평범
        assert!(suspicious(&snap(None, 30.0, BALANCED)).is_none()); // 센서 실패 → 의심 아님
    }

    // --- 화이트리스트 검증(validate): AI 제안 → 안전한 Action ---

    #[test]
    fn validate_accepts_whitelisted_switch() {
        let s = snap(Some(90.0), 80.0, BALANCED);
        match validate(&proposal("set_power_plan", Some("high_performance")), &s) {
            Action::SetPowerPlan {
                guid,
                rollback_guid,
                ..
            } => {
                assert_eq!(guid, HIGH_PERF);
                assert_eq!(rollback_guid.as_str(), BALANCED); // 롤백 대상 = 직전 상태
            }
            Action::None => panic!("화이트리스트 동작이 승인돼야 함"),
        }
    }

    #[test]
    fn validate_rejects_non_whitelisted_action() {
        // AI가 화이트리스트 밖 동작을 지어내도 None — 명령 생성 차단.
        let s = snap(Some(90.0), 80.0, BALANCED);
        assert!(matches!(
            validate(&proposal("delete_system_files", None), &s),
            Action::None
        ));
        assert!(matches!(
            validate(&proposal("set_power_plan", Some("turbo_mode")), &s),
            Action::None
        ));
    }

    #[test]
    fn validate_skips_when_already_in_target_plan() {
        // 이미 balanced인데 balanced 제안 → 변화 없음(None).
        let s = snap(Some(90.0), 80.0, BALANCED);
        assert!(matches!(
            validate(&proposal("set_power_plan", Some("balanced")), &s),
            Action::None
        ));
    }

    #[test]
    fn validate_parses_combined_action_target() {
        // "set_power_plan|balanced" 합쳐진 형태도 파싱 (현재 high_perf → balanced).
        let s = snap(Some(90.0), 80.0, HIGH_PERF);
        match validate(&proposal("set_power_plan|balanced", None), &s) {
            Action::SetPowerPlan { guid, .. } => assert_eq!(guid, BALANCED),
            Action::None => panic!("합쳐진 형태도 파싱돼야 함"),
        }
    }
}
