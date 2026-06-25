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
use std::process::Command;
use wmi::{COMLibrary, WMIConnection};

const TEMP_WARN_C: f32 = 85.0;
const LOG_FILE: &str = "velox_actions.log";

/// 전원 구성표 화이트리스트. AI는 이 key 중에서만 고를 수 있다. (GUID 하드코딩)
fn plan_by_key(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "balanced" => Some(("Balanced", "381b4222-f694-41f0-9685-ff5bb260df2e")),
        "high_performance" => Some(("High performance", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")),
        "power_saver" => Some(("Power saver", "a1841308-3541-4fab-bc81-f71556f20b4a")),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    current_temperature: u32,
}

struct Snapshot {
    max_temp_c: Option<f32>,
    plan_guid: String,
    plan_label: String,
    summary: String,
}

impl Snapshot {
    fn is_hot(&self) -> bool {
        self.max_temp_c.map_or(false, |t| t >= TEMP_WARN_C)
    }
    fn heartbeat(&self) -> String {
        let temp = self
            .max_temp_c
            .map(|c| format!("{:.1}°C", c))
            .unwrap_or_else(|| "N/A".to_string());
        format!("전원={} 온도={}", self.plan_label, temp)
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

// ---------------- [1] 읽기 ----------------

fn read_active_plan() -> (String, String) {
    if let Ok(out) = Command::new("powercfg").arg("/getactivescheme").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let guid = s
            .split("GUID:")
            .nth(1)
            .and_then(|t| t.trim().split_whitespace().next())
            .unwrap_or("")
            .to_string();
        let label = s
            .split('(')
            .nth(1)
            .and_then(|t| t.split(')').next())
            .unwrap_or("Unknown")
            .trim()
            .to_string();
        return (guid, label);
    }
    (String::new(), "Unknown".to_string())
}

fn read_max_temp() -> Option<f32> {
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
    let temps: Vec<ThermalZone> = wmi.query().unwrap_or_default();
    temps
        .iter()
        .map(|t| (t.current_temperature as f32 / 10.0) - 273.15)
        .fold(None, |acc, c| Some(acc.map_or(c, |m: f32| m.max(c))))
}

fn collect() -> Snapshot {
    let (plan_guid, plan_label) = read_active_plan();
    let max_temp_c = read_max_temp();
    let temp_str = match max_temp_c {
        Some(c) => format!("{:.1}°C", c),
        None => "N/A (센서 읽기 실패 — 관리자 권한 필요)".to_string(),
    };
    let summary = format!(
        "- 현재 전원 모드: {} ({})\n- 최고 온도: {}",
        plan_label, plan_guid, temp_str
    );
    Snapshot {
        max_temp_c,
        plan_guid,
        plan_label,
        summary,
    }
}

// ---------------- 화이트리스트 검증 ----------------

fn validate(p: &AiProposal, snap: &Snapshot) -> Action {
    match p.action.as_str() {
        "set_power_plan" => {
            let key = p.target.as_deref().unwrap_or("");
            match plan_by_key(key) {
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
        _ => Action::None,
    }
}

fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then(|| &s[start..=end])
}

// ---------------- [2] 3단계 AI 파이프라인 ----------------

async fn ai_pipeline(snap: &Snapshot) -> Option<Pipeline> {
    // 1) Customer (Claude) — 의도/상황 이해
    let intent = crate::chorus::query_text_with(
        "claude",
        &format!(
            "다음 시스템 상태에서 사용자가 가장 걱정할 점이나 원하는 바를 한국어 한 줄로 요약:\n{}",
            snap.summary
        ),
    )
    .await
    .unwrap_or_else(|| "(의도 파악 실패)".to_string());

    // 2) Engineer (GPT) — 구조화된 조치 제안
    let eng_prompt = format!(
        "너는 시스템 엔지니어 AI다.\n사용자 의도: {}\n시스템 상태:\n{}\n\n\
         아래 JSON 한 개로만 답하라(설명/마크다운 금지).\n\
         {{\"action\":\"set_power_plan|none\",\"target\":\"balanced|high_performance|power_saver|null\",\"reason\":\"한 줄\"}}\n\
         규칙: 안전·가역한 경우에만 조치 제안, 애매하면 action=\"none\".",
        intent.trim(),
        snap.summary
    );
    let eng_raw = crate::chorus::query_text_with("gpt", &eng_prompt).await?;
    let proposal: AiProposal = serde_json::from_str(extract_json(&eng_raw)?).ok()?;
    let action = validate(&proposal, snap);

    // 3) Confirmer (Gemini) — 독립 검수
    let (confirmed, verdict) = match &action {
        Action::None => (false, "제안된 조치 없음".to_string()),
        Action::SetPowerPlan { label, .. } => {
            let conf_prompt = format!(
                "너는 검수 AI다. 제안된 조치: 전원 모드를 '{}'(으)로 변경.\n시스템 상태:\n{}\n\n\
                 안전하고 합리적이면 'APPROVE', 아니면 'REJECT'로 시작해 한국어 한 줄 이유.",
                label, snap.summary
            );
            let r = crate::chorus::query_text_with("gemini", &conf_prompt)
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
    if snap.is_hot() {
        if let Some((label, guid)) = plan_by_key("balanced") {
            if guid.to_lowercase() != snap.plan_guid.to_lowercase() {
                return Action::SetPowerPlan {
                    label,
                    guid,
                    rollback_guid: snap.plan_guid.clone(),
                };
            }
        }
    }
    Action::None
}

// ---------------- 실행 ----------------

fn apply_power_plan(guid: &str) -> bool {
    Command::new("powercfg")
        .args(["/setactive", guid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn execute_with_safety(label: &str, guid: &str, rollback_guid: &str) {
    // 위험 동작 전 자동 체크포인트 (AI 오판/블루스크린 대비)
    crate::checkpoint::save_silent();
    println!("[체크포인트 저장됨 — 문제 시 `velox checkpoint restore`]");

    if !apply_power_plan(guid) {
        println!("✗ 실행 실패 (권한/모드 부재).");
        return;
    }
    let (new_guid, new_label) = read_active_plan();
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

pub async fn run(fix: bool) {
    println!("=== APEX Velox — diagnose (3단계 AI) ===\n");
    let snap = collect();
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
                println!("[3] Confirmer AI가 반려함 → 실행 차단 🛑 (제안: 전원 모드 → {})", label);
                return;
            }
            println!(
                "[3] 조치 제안: 전원 모드 → '{}' (화이트리스트 ✓ · Confirmer ✓ · 가역)",
                label
            );
        }
    }

    if !fix {
        println!("\n실제 적용: velox diagnose --fix");
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

// ---------------- daemon이 호출하는 1회 점검 ----------------

/// 데몬의 한 틱. auto=true 면 Confirmer 승인 시 체크포인트 후 자동 실행(사람 승인 생략).
pub async fn daemon_tick(auto: bool) {
    let snap = collect();
    println!("· {}", snap.heartbeat());

    if !snap.is_hot() {
        return; // 정상 → 감시만
    }
    println!("  ⚠ 임계 초과 → 3단계 AI 파이프라인 가동");

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
            } else if auto {
                println!("  → AUTO: {} 적용", label);
                execute_with_safety(label, guid, &rollback_guid);
            } else {
                println!("  → 제안: 전원 모드 → {} (실행하려면 --auto 또는 `velox diagnose --fix`)", label);
            }
        }
    }
}
